//! Uji ujung-ke-ujung: dua vault (Budi & Sari) berbagi memori proyek lewat hub sungguhan
//! (router axum in-process, store in-memory), termasuk migrasi dari bundel git lama.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use memory_core::MemoryConfig;
use memory_core::config::{HubConfig, IndexConfig};
use memory_core::model::{EpisodeStatus, Evidence, Lesson, LessonKind};
use memory_extract::det_id;
use memory_extract::ingest::DistilledDoc;
use memory_hub::client::{HubClient, HubCreds};
use memory_hub::server::auth::users;
use memory_hub::server::{GithubApi, HubSecrets, HubState, Provider, ProviderClient, ProviderIdentity};
use memory_llm::HashEmbedder;
use memory_llm::mock::ScriptedLlm;
use memory_store::{EpisodeFilter, Store};
use serde_json::{Value, json};

use crate::repo_bundle as rb;
use crate::repo_hub as rh;
use crate::{JobError, Runtime};

struct NoProvider;

#[async_trait]
impl ProviderClient for NoProvider {
    async fn exchange(&self, _p: Provider, _code: &str, _ctx: &memory_hub::server::auth::providers::ExchangeCtx) -> memory_hub::server::Result<ProviderIdentity> {
        Err(memory_hub::server::HubError::OAuth("tidak dipakai".into()))
    }
}

struct NoGithub;

#[async_trait]
impl GithubApi for NoGithub {
    async fn repo_role(&self, _t: &str, _f: &str) -> memory_hub::server::Result<Option<memory_hub::server::authz::Role>> {
        Ok(None)
    }
}

async fn spawn_hub() -> (String, HubState) {
    let store = Store::memory(16, &IndexConfig::default()).await.unwrap();
    let cfg = HubConfig { enabled: true, base_url: "http://127.0.0.1:1".into(), google_client_id: "gid".into(), ..Default::default() };
    let st = HubState::with_parts(store, cfg, HubSecrets::for_tests(), reqwest::Client::new(), Arc::new(NoProvider), Arc::new(NoGithub)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = axum::Router::new().nest("/hub", memory_hub::router(st.clone(), "<html></html>"));
    tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
    (format!("http://{addr}"), st)
}

/// Pengguna hub + berkas kredensial `hub.json` miliknya sendiri.
async fn user(st: &HubState, base: &str, dir: &Path, name: &str) -> HubClient {
    let ident = ProviderIdentity { provider_id: format!("id-{name}"), email: format!("{}@example.com", name.to_lowercase()), email_verified: true, name: Some(name.into()), avatar_url: None };
    let u = users::login_identity(st, Provider::Google, &ident).await.unwrap();
    let tok = users::issue_tokens(st, &u, "uji").await.unwrap();
    let path = dir.join(format!("hub-{name}.json"));
    HubCreds::from_token(base, &tok).save_to(&path).unwrap();
    HubClient::with_creds_file(path).unwrap()
}

async fn rt(dir: &Path) -> Runtime {
    let mut cfg = MemoryConfig::for_tests(64);
    cfg.vault.path = dir.to_path_buf();
    Runtime::open_with(cfg, Arc::new(ScriptedLlm::new(|_| Ok("{}".into()))), Arc::new(HashEmbedder::new(64))).await.unwrap()
}

fn clone_repo(base: &Path, name: &str, user: &str) -> PathBuf {
    let r = base.join(name);
    std::fs::create_dir_all(r.join(".git")).unwrap();
    std::fs::write(r.join(".git/config"), format!("[user]\n\tname = {user}\n\temail = {}@example.com\n[remote \"origin\"]\n\turl = git@github.com:tim/api-a.git\n", user.to_lowercase())).unwrap();
    r
}

fn doc(subject: &str, title: &str, text: &str, extraction: Value) -> DistilledDoc {
    DistilledDoc { subject: subject.into(), title: title.into(), summary: format!("ringkasan {title}"), text: text.into(), project: Some("api-a".into()), original_name: None, meta: json!({"extraction": extraction}) }
}

fn lesson(subject: &str, title: &str) -> Lesson {
    let now = memory_core::ids::now_rfc3339();
    Lesson {
        id: det_id(&["lesson", subject, "api-a", &Lesson::title_key_of(title)]),
        subject: subject.into(),
        project: Some("api-a".into()),
        kind: LessonKind::Pitfall,
        title: title.into(),
        title_key: Lesson::title_key_of(title),
        detail: "Tes integrasi butuh skema terbaru.".into(),
        occurrences: 2,
        evidence: vec![Evidence { episode_id: "ep-lokal".into(), quote: String::new() }],
        status: "active".into(),
        promoted_to: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

/// Isi `.engram/` (yang akan ikut git).
fn git_tree(root: &Path) -> Vec<String> {
    fn walk(base: &Path, cur: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(cur).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.push(p.strip_prefix(base).unwrap().to_string_lossy().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join(".engram"), &root.join(".engram"), &mut out);
    out.sort();
    out
}

fn copy_engram(from: &Path, to: &Path) {
    fn c(a: &Path, b: &Path) {
        std::fs::create_dir_all(b).unwrap();
        for e in std::fs::read_dir(a).unwrap().flatten() {
            let (s, d) = (e.path(), b.join(e.file_name()));
            if s.is_dir() {
                c(&s, &d)
            } else {
                std::fs::copy(&s, &d).unwrap();
            }
        }
    }
    c(&from.join(".engram"), &to.join(".engram"));
}

#[tokio::test]
async fn two_vaults_share_project_memory_through_the_hub() {
    let base = tempfile::tempdir().unwrap();
    let (hub, st) = spawn_hub().await;
    let (budi_c, sari_c) = (user(&st, &hub, base.path(), "Budi").await, user(&st, &hub, base.path(), "Sari").await);
    let (repo_a, repo_b) = (clone_repo(base.path(), "a", "Budi"), clone_repo(base.path(), "b", "Sari"));
    let (a, b) = (rt(&base.path().join("vault-a")).await, rt(&base.path().join("vault-b")).await);
    a.pipeline.ensure_subject("budi", Some("Budi")).await.unwrap();
    b.pipeline.ensure_subject("sari", Some("Sari")).await.unwrap();

    // Memori proyek Budi: pelajaran, keputusan, fakta, dan satu pelajaran berisi blok privat.
    a.store.put_lesson(&lesson("budi", "Jalankan migrasi sebelum tes")).await.unwrap();
    let mut private = lesson("budi", "Kunci server staging");
    private.detail = "<private>kata sandi ada di catatan pribadi</private>".into();
    a.store.put_lesson(&private).await.unwrap();
    let dec = a
        .pipeline
        .ingest_distilled(doc("budi", "Pilih Postgres", "Kami memilih Postgres untuk api-a.", json!({"obs_type": "decision", "decisions": [{"situation": "Butuh database transaksional", "action": "Pakai Postgres 16", "reason": "Transaksi"}]})))
        .await
        .unwrap();
    let dec_ep = dec.episode_id.unwrap();
    a.pipeline
        .ingest_distilled(doc(
            "budi",
            "Cache api-a",
            "api-a memakai Redis untuk cache sesi.",
            json!({"entities": [{"name": "api-a", "kind": "project"}], "facts": [{"subject": "api-a", "predicate": "memakai", "object": "Redis untuk cache sesi", "category": "teknologi_cache", "time": {"kind": "current"}, "quote": "api-a memakai Redis untuk cache sesi", "confidence": 0.9}]}),
        ))
        .await
        .unwrap();

    // Budi menghubungkan repo: hanya repo.toml yang ditulis ke git.
    let init = rh::init_repo(&a, &budi_c, "budi", &repo_a, Some("api-a")).await.unwrap();
    assert!(init.pushed >= 4 && init.role.as_deref() == Some("owner"), "{init:?}");
    assert_eq!(git_tree(&repo_a), vec!["repo.toml".to_string()]);
    let toml = rh::read_repo_toml(&repo_a).unwrap();
    assert!(toml.join_key.starts_with("ek1_") && toml.hub == hub && toml.project == "api-a");
    // Push ulang tanpa perubahan tidak mengirim apa pun.
    let again = rh::push_repo(&a, &budi_c, "budi", &repo_a, false, false).await.unwrap();
    assert_eq!((again.pushed, again.withdrawn), (0, 0), "{again:?}");

    // Sari meng-clone (repo.toml ikut git); job pull pertama otomatis bergabung lalu menarik.
    copy_engram(&repo_a, &repo_b);
    let out = rh::run_job_with(&b, &sari_c, rh::REPO_PULL, &json!({"subject": "sari", "root": repo_b})).await.unwrap();
    assert_eq!(out["role"], "member", "{out}");
    let imp = &out["import"];
    assert_eq!((imp["lessons_new"].as_u64(), imp["decisions_new"].as_u64(), imp["facts_new"].as_u64()), (Some(1), Some(1), Some(1)), "{out}");
    assert_eq!(imp["episodes_new"].as_u64(), Some(1), "{out}");
    let eps = b.store.list_episodes("sari", &EpisodeFilter { projects: vec!["api-a".into()], limit: 50, ..Default::default() }).await.unwrap();
    let shared_dec = eps.iter().find(|e| e.obs_type.as_deref() == Some("decision")).expect("keputusan Budi jadi episode keputusan di vault Sari");
    assert!(shared_dec.text.contains("dibagikan Budi lewat hub tim") && shared_dec.concepts.iter().any(|c| c == rb::SHARED_CONCEPT), "{}", shared_dec.text);
    assert!(b.store.list_lessons("sari", &["api-a".into()], None, 10).await.unwrap().iter().all(|l| !l.detail.contains("kata sandi")));
    let ent = b.retriever.find_entity("sari", "api-a").await.unwrap().unwrap();
    assert!(b.store.facts_for_entity(&ent.id).await.unwrap().iter().any(|f| f.is_current() && f.object_text == "Redis untuk cache sesi"));
    assert_eq!(git_tree(&repo_b), vec!["repo.toml".to_string()], "pull tidak menulis apa pun ke repo");
    // Repo yang sudah bergabung ikut pemeriksaan berkala: pull + push diantre sekali per periode.
    assert_eq!(rh::schedule_due(&b).await.unwrap(), 2);
    assert_eq!(rh::schedule_due(&b).await.unwrap(), 0);
    let kinds: Vec<String> = b.store.list_jobs(None, 50).await.unwrap().into_iter().map(|j| j.kind).filter(|k| k.starts_with("repo_")).collect();
    assert!(kinds.contains(&rh::REPO_PULL.to_string()) && kinds.contains(&rh::REPO_PUSH.to_string()), "{kinds:?}");

    // Memori impor tidak dipantulkan balik; pull berikutnya tidak menerapkan ulang apa pun.
    let back = rh::push_repo(&b, &sari_c, "sari", &repo_b, false, false).await.unwrap();
    assert_eq!(back.pushed, 0, "{back:?}");
    let quiet = rh::pull_repo(&b, &sari_c, "sari", &repo_b).await.unwrap();
    assert_eq!(quiet.pulled, 0, "{quiet:?}");

    // Budi mencabut keputusannya: hub membawa pencabutan, Sari mengkarantina episodenya.
    a.store.patch("episode", &dec_ep, json!({"status": EpisodeStatus::Quarantined})).await.unwrap();
    let w = rh::push_repo(&a, &budi_c, "budi", &repo_a, false, false).await.unwrap();
    assert!(w.withdrawn >= 1, "{w:?}");
    let p = rh::pull_repo(&b, &sari_c, "sari", &repo_b).await.unwrap();
    assert!(p.import.as_ref().unwrap().withdrawn >= 1, "{p:?}");
    assert_eq!(b.store.get_episode(&shared_dec.id).await.unwrap().unwrap().status, EpisodeStatus::Quarantined);

    // Status lokal tanpa jaringan.
    let s = rh::status(&b, &repo_b, None).await.unwrap();
    assert_eq!((s.mode.as_str(), s.joined, s.role.as_deref()), ("hub", true, Some("member")));
    assert!(s.cursor >= p.cursor && s.last_pull_at.is_some());

    // Galat permanen (repo hub tidak dikenal) tidak diulang; hub mati = galat sementara (diulang).
    let mut bad = toml.clone();
    bad.repo_id = "01tidakada".into();
    let repo_c = clone_repo(base.path(), "c", "Sari");
    rh::write_repo_toml(&repo_c, &bad).unwrap();
    let skipped = rh::run_job_with(&b, &sari_c, rh::REPO_PULL, &json!({"subject": "sari", "root": repo_c})).await.unwrap();
    assert!(skipped["skipped"].as_str().unwrap().contains("404"), "{skipped}");
    let mut down = toml.clone();
    down.hub = "http://127.0.0.1:9".into();
    let repo_d = clone_repo(base.path(), "d", "Sari");
    rh::write_repo_toml(&repo_d, &down).unwrap();
    let dead = HubClient::new("http://127.0.0.1:9", memory_hub::client::http::Auth::Pat("engram_pat_x".into())).unwrap();
    let err = rh::run_job_with(&b, &dead, rh::REPO_PULL, &json!({"subject": "sari", "root": repo_d})).await.unwrap_err();
    assert!(matches!(err, JobError::Hub { retry: true, .. }), "{err}");
}

#[tokio::test]
async fn migrate_moves_legacy_bundle_to_hub_and_cleans_the_repo() {
    let base = tempfile::tempdir().unwrap();
    let (hub, st) = spawn_hub().await;
    let (budi_c, sari_c) = (user(&st, &hub, base.path(), "Budi").await, user(&st, &hub, base.path(), "Sari").await);
    let repo = clone_repo(base.path(), "a", "Budi");
    let (a, b) = (rt(&base.path().join("vault-a")).await, rt(&base.path().join("vault-b")).await);
    a.pipeline.ensure_subject("budi", Some("Budi")).await.unwrap();
    b.pipeline.ensure_subject("sari", Some("Sari")).await.unwrap();

    // Sari dulu berbagi lewat bundel git (format v2), dan clone Budi punya git hook engram.
    let repo_s = clone_repo(base.path(), "s", "Sari");
    b.store.put_lesson(&lesson("sari", "Pakai cargo test -p")).await.unwrap();
    rb::init_repo(&b, "sari", &repo_s, Some("api-a"), false).await.unwrap();
    copy_engram(&repo_s, &repo);
    rb::git_hooks(&repo, false).unwrap();
    assert!(repo.join(".engram/members").is_dir() && !rb::installed_hooks(&repo).is_empty());

    // Budi memigrasi: bundel Sari dipindah ke hub, repo bersih, hook dicabut.
    a.store.put_lesson(&lesson("budi", "Jalankan migrasi sebelum tes")).await.unwrap();
    let m = rh::migrate_repo(&a, &budi_c, "budi", &repo, Some("api-a")).await.unwrap();
    assert!(m.pushed >= 2, "{m:?}");
    assert_eq!(git_tree(&repo), vec!["repo.toml".to_string()]);
    assert!(rb::installed_hooks(&repo).is_empty());
    assert!(m.commit_hint.as_deref().unwrap().contains("git rm -r --cached"));
    // Idempoten.
    let again = rh::migrate_repo(&a, &budi_c, "budi", &repo, Some("api-a")).await.unwrap();
    assert_eq!(again.pushed, 0, "{again:?}");

    // Rekan yang bergabung menerima pelajaran Sari (dengan nama penulis aslinya) dan Budi.
    let repo_x = clone_repo(base.path(), "x", "Sari");
    copy_engram(&repo, &repo_x);
    let c = rt(&base.path().join("vault-c")).await;
    c.pipeline.ensure_subject("cici", Some("Cici")).await.unwrap();
    let j = rh::join_repo(&c, &sari_c, "cici", &repo_x, None).await.unwrap();
    let imp = j.import.as_ref().unwrap();
    assert_eq!(imp.lessons_new, 2, "{j:?}");
    assert!(imp.members.iter().any(|m| m.starts_with("Sari")), "{j:?}");
}
