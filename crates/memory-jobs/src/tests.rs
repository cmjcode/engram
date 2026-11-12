use std::sync::Arc;

use memory_core::MemoryConfig;
use memory_core::model::{ImportBatch, SourceKind};
use memory_extract::Vault;
use memory_extract::ingest::IngestDoc;
use memory_llm::mock::{ScriptedLlm, last_user};
use memory_llm::{ChatRequest, HashEmbedder};
use serde_json::json;

use crate::Runtime;

const DIM: usize = 128;

fn llm(req: &ChatRequest) -> memory_llm::Result<String> {
    let user = last_user(req);
    Ok(match req.task.as_str() {
        "segment" => {
            let n = user.lines().filter(|l| l.starts_with('[')).count();
            json!({"segments": (0..n).map(|i| json!({"start": i, "end": i, "significance": 0.9})).collect::<Vec<_>>()}).to_string()
        }
        "extract" => json!({"title": "cerita", "summary": "ringkas", "entities": [{"name": "Budi", "kind": "person", "is_self": true}]}).to_string(),
        _ => json!({"text": "ok"}).to_string(),
    })
}

async fn rt(dir: &std::path::Path) -> Runtime {
    let mut cfg = MemoryConfig::for_tests(DIM);
    cfg.vault.path = dir.to_path_buf();
    Runtime::open_with(cfg, Arc::new(ScriptedLlm::new(llm)), Arc::new(HashEmbedder::new(DIM))).await.unwrap()
}

#[tokio::test]
async fn batch_progress_scheduler_and_encrypted_backup_roundtrip() {
    let d1 = tempfile::tempdir().unwrap();
    let r = rt(d1.path()).await;
    let batch = ImportBatch { id: "b1".into(), subject: "budi".into(), label: "uji".into(), total: 0, done: 0, failed: 0, duplicates: 0, failures: vec![], created_at: memory_core::ids::now_rfc3339(), updated_at: String::new() };
    r.store.put_batch(&batch).await.unwrap();
    for i in 0..3 {
        r.pipeline
            .ingest(IngestDoc {
                subject: "budi".into(),
                kind: SourceKind::Memoir,
                text: format!("Cerita nomor {i}: waktu kecil saya tinggal di rumah kayu dekat sungai bersama simbah di desa {i}."),
                batch_id: Some("b1".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        r.store.batch_incr("b1", "total", None).await.unwrap();
    }
    assert_eq!(r.drain().await.unwrap(), 3);
    let b = r.store.get_batch("b1").await.unwrap().unwrap();
    assert_eq!((b.total, b.done, b.failed), (3, 3, 0));
    let eps = r.store.count_episodes("budi").await.unwrap();
    assert_eq!(eps, 3);

    // Scheduler idempoten dalam satu periode.
    assert!(r.schedule_due().await.unwrap() > 0);
    assert_eq!(r.schedule_due().await.unwrap(), 0);
    r.drain().await.unwrap();

    // Ekspor + backup terenkripsi.
    use age::secrecy::ExposeSecret;
    let id = age::x25519::Identity::generate();
    let id_path = d1.path().join("key.txt");
    std::fs::write(&id_path, id.to_string().expose_secret()).unwrap();
    let out = crate::archive::backup(&r, &d1.path().join("backup.tar.gz"), Some(&id.to_public().to_string())).await.unwrap();
    assert!(out.to_string_lossy().ends_with(".age"));
    let head = std::fs::read(&out).unwrap();
    assert!(head.starts_with(b"age-encryption.org/v1"));

    // Restore ke vault baru.
    let d2 = tempfile::tempdir().unwrap();
    let r2 = rt(d2.path()).await;
    let rep = crate::archive::restore(&r2, &out, Some(&id_path)).await.unwrap();
    assert!(rep.rows > 0 && rep.source_files >= 3, "{rep:?}");
    assert_eq!(r2.store.count_episodes("budi").await.unwrap(), 3);
    let hits = r2.retriever.recall(&memory_retrieve::RecallQuery::new("budi", "rumah kayu dekat sungai")).await.unwrap();
    assert!(hits.found && !hits.episodes.is_empty(), "embedding harus dihitung ulang setelah restore");
    // Ekspor berisi markdown yang bisa dibaca manusia.
    let exp = crate::archive::export(&r2, None).await.unwrap();
    assert!(exp.join("manifest.json").exists());
    assert!(exp.join("markdown/budi/README.md").exists());
    let _ = Vault::new(d2.path());
}

#[tokio::test]
async fn failed_sources_are_retried_only_when_backends_are_up() {
    use memory_core::model::SourceStatus;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Embedder yang bisa "dimatikan" seperti server TEI yang mati.
    struct FlakyEmbedder(Arc<AtomicBool>, HashEmbedder);
    #[async_trait::async_trait]
    impl memory_llm::Embedder for FlakyEmbedder {
        async fn embed(&self, texts: &[String]) -> memory_llm::Result<Vec<Vec<f32>>> {
            if self.0.load(Ordering::SeqCst) {
                return Err(memory_llm::LlmError::Other("server embedding mati".into()));
            }
            self.1.embed(texts).await
        }
        fn dimension(&self) -> usize {
            DIM
        }
    }

    let down = Arc::new(AtomicBool::new(true));
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = MemoryConfig::for_tests(DIM);
    cfg.vault.path = dir.path().to_path_buf();
    cfg.schedule.retry_failed_max = 1;
    let embedder = FlakyEmbedder(down.clone(), HashEmbedder::new(DIM));
    let r = Runtime::open_with(cfg, Arc::new(ScriptedLlm::new(llm)), Arc::new(embedder)).await.unwrap();

    let out = r
        .pipeline
        .ingest(IngestDoc {
            subject: "budi".into(),
            kind: SourceKind::Memoir,
            text: "Waktu kecil saya tinggal di rumah kayu dekat sungai bersama simbah di desa.".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let id = out.source.id.clone();
    // Sumber yang gagal di versi lama / karena server mati.
    let fail = |r: Runtime, id: String| async move { r.store.set_source_status(&id, SourceStatus::Failed, Some("http: server mati".into())).await.unwrap() };
    fail(r.clone(), id.clone()).await;
    let status = |r: Runtime, id: String| async move { r.store.get_source(&id).await.unwrap().unwrap() };
    assert_eq!(status(r.clone(), id.clone()).await.status, SourceStatus::Failed);

    // Server masih mati: tidak ada yang diantre, jatah otomatis tidak terpakai.
    let rep = r.retry_failed(true).await.unwrap();
    assert_eq!((rep.failed, rep.requeued), (1, 0));
    assert!(rep.backend_error.is_some());
    assert!(r.retry_source(&id).await.is_err(), "manual juga menolak bila server mati");
    assert_eq!(status(r.clone(), id.clone()).await.status, SourceStatus::Failed);

    // Server mati lagi saat diproses: jatah otomatis (1) habis.
    down.store(false, Ordering::SeqCst);
    assert_eq!(r.retry_failed(true).await.unwrap().requeued, 1);
    let s = status(r.clone(), id.clone()).await;
    assert_eq!((s.status, s.meta["auto_retries"].as_u64()), (SourceStatus::Pending, Some(1)));
    fail(r.clone(), id.clone()).await;
    let rep = r.retry_failed(true).await.unwrap();
    assert_eq!((rep.requeued, rep.exhausted), (0, 1));

    // Manual tetap bisa, lalu sumber selesai diproses.
    assert!(r.retry_source(&id).await.unwrap().is_some());
    r.drain().await.unwrap();
    let s = status(r.clone(), id.clone()).await;
    assert!(matches!(s.status, SourceStatus::Done | SourceStatus::Skipped), "{:?} {:?}", s.status, s.error);
    assert_eq!(s.meta["auto_retries"].as_u64(), Some(0));
    assert!(r.retry_source(&id).await.unwrap().is_none(), "sumber yang tidak gagal tidak diantre");
}

#[tokio::test]
async fn two_computers_sync_through_shared_folder() {
    use crate::sync::{self, SyncSettings};

    let drive = tempfile::tempdir().unwrap();
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (rt(da.path()).await, rt(db.path()).await);
    let setup = |r: &Runtime, name: &str| {
        let s = sync::save_settings(&r.vault, SyncSettings { dir: Some(drive.path().to_path_buf()), interval_minutes: 0, device_name: Some(name.into()) }).unwrap();
        sync::set_passphrase(&r.vault, Some("rahasia-keluarga")).unwrap();
        s
    };
    let ingest = |r: Runtime, text: &'static str| async move {
        r.pipeline.ingest(IngestDoc { subject: "budi".into(), kind: SourceKind::Memoir, text: text.into(), ..Default::default() }).await.unwrap();
        r.drain().await.unwrap();
    };

    // Folder Drive kosong dipakai langsung.
    assert_eq!(setup(&a, "Laptop").dir.unwrap(), drive.path());
    setup(&b, "PC kantor");
    ingest(a.clone(), "Waktu kecil saya tinggal di rumah kayu dekat sungai bersama simbah di desa.").await;

    // A mengunggah; snapshot terenkripsi, tidak ada teks polos di folder Drive.
    let ra = sync::sync_now(&a).await.unwrap();
    assert!(ra.pushed && ra.source_files_out >= 1, "{ra:?}");
    let devs = sync::status(&a).devices;
    assert_eq!(devs.len(), 1);
    assert!(devs[0].encrypted && devs[0].this_device);
    for e in walk(drive.path()) {
        let bytes = std::fs::read(&e).unwrap();
        if e.to_string_lossy().contains("snapshot") {
            assert!(bytes.starts_with(b"age-encryption.org/v1"), "{}", e.display());
        }
    }

    // B menarik data A dan bisa langsung mencarinya (embedding dihitung di B).
    let rb = sync::sync_now(&b).await.unwrap();
    assert_eq!(rb.pulled_from, vec!["Laptop".to_string()], "{rb:?}");
    assert!(rb.added > 0 && rb.embedded > 0 && rb.pending_embed == 0, "{rb:?}");
    assert_eq!(b.store.count_episodes("budi").await.unwrap(), 1);
    let hits = b.retriever.recall(&memory_retrieve::RecallQuery::new("budi", "rumah kayu dekat sungai")).await.unwrap();
    assert!(hits.found, "embedding harus ada setelah sync");

    // B menambah cerita; A menerimanya. Data A tidak hilang.
    ingest(b.clone(), "Tahun 1998 saya merantau ke Jakarta dan bekerja di pabrik tekstil di Tangerang.").await;
    sync::sync_now(&b).await.unwrap();
    let ra = sync::sync_now(&a).await.unwrap();
    assert_eq!(ra.pulled_from, vec!["PC kantor".to_string()], "{ra:?}");
    assert_eq!(a.store.count_episodes("budi").await.unwrap(), 2);
    assert_eq!(b.store.count_episodes("budi").await.unwrap(), 2);

    // B menarik snapshot A yang isinya sudah sama: tidak ada record yang ditimpa.
    let same = sync::sync_now(&b).await.unwrap();
    assert_eq!(same.pulled_from.len(), 1, "{same:?}");
    assert_eq!((same.added, same.updated, same.embedded), (0, 0, 0), "{same:?}");

    // Tanpa perubahan: tidak ada yang diunggah atau ditarik ulang.
    let again = sync::sync_now(&a).await.unwrap();
    let again_b = sync::sync_now(&b).await.unwrap();
    assert!(!again_b.pushed && again_b.pulled_from.is_empty() && again_b.added + again_b.updated == 0, "{again_b:?} {again:?}");

    // Komputer ketiga dengan kata sandi salah: ditolak dengan pesan jelas, data tidak tercampur.
    let dc = tempfile::tempdir().unwrap();
    let c = rt(dc.path()).await;
    sync::save_settings(&c.vault, SyncSettings { dir: Some(drive.path().to_path_buf()), interval_minutes: 0, device_name: None }).unwrap();
    sync::set_passphrase(&c.vault, Some("kata-sandi-lain")).unwrap();
    let rc = sync::sync_now(&c).await.unwrap();
    assert!(rc.pulled_from.is_empty() && rc.warnings.iter().any(|w| w.contains("tidak cocok")), "{rc:?}");
    assert_eq!(c.store.count_episodes("budi").await.unwrap(), 0);
    assert!(sync::set_passphrase(&c.vault, Some("pendek")).is_err());
}

#[tokio::test]
async fn repo_bundle_roundtrip_distilled_import_and_lesson_promotion() {
    use memory_core::model::{Evidence, Lesson, LessonKind};
    let d1 = tempfile::tempdir().unwrap();
    let r = rt(d1.path()).await;
    r.pipeline.ensure_subject("budi", Some("Budi")).await.unwrap();
    let now = memory_core::ids::now_rfc3339();
    let mk = |project: &str, title: &str, n: u32| Lesson {
        id: memory_extract::det_id(&["lesson", "budi", project, &Lesson::title_key_of(title)]),
        subject: "budi".into(),
        project: Some(project.into()),
        kind: LessonKind::Convention,
        title: title.into(),
        title_key: Lesson::title_key_of(title),
        detail: String::new(),
        occurrences: n,
        evidence: vec![Evidence { episode_id: format!("ep-{project}"), quote: String::new() }],
        status: "active".into(),
        promoted_to: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    r.store.put_lesson(&mk("api-a", "Jangan commit tanpa diminta", 2)).await.unwrap();
    r.store.put_lesson(&mk("api-b", "Jangan commit tanpa diminta!", 2)).await.unwrap();
    r.store.put_lesson(&mk("api-a", "Pakai cargo test -p", 1)).await.unwrap();

    // Impor terdistilasi tanpa LLM: episode langsung jadi, duplikat terdeteksi.
    let doc = || memory_extract::ingest::DistilledDoc { subject: "budi".into(), title: "Arsitektur api-a".into(), summary: "Pakai Postgres".into(), text: "api-a memakai Postgres 16 dan Redis untuk cache sesi.".into(), project: Some("api-a".into()), original_name: None, meta: json!({}) };
    let o1 = r.pipeline.ingest_distilled(doc()).await.unwrap();
    assert!(!o1.duplicate && o1.episode_id.is_some());
    assert!(r.pipeline.ingest_distilled(doc()).await.unwrap().duplicate);
    let ep = r.store.get_episode(o1.episode_id.as_deref().unwrap()).await.unwrap().unwrap();
    assert_eq!(ep.project.as_deref(), Some("api-a"));
    assert_eq!(ep.title, "Arsitektur api-a");

    // Pelajaran lintas proyek yang berulang -> kandidat prinsip (pending review).
    let rep = r.pipeline.distill("budi").await.unwrap();
    assert_eq!(rep.lessons_promoted, 1, "{rep:?}");
    let ps = r.store.list_principles("budi", None).await.unwrap();
    assert!(ps.iter().any(|p| p.statement.starts_with("Jangan commit") && p.status == memory_core::model::ReflectiveStatus::PendingReview));
    assert!(r.store.list_lessons("budi", &[], None, 50).await.unwrap().iter().filter(|l| l.promoted_to.is_some()).count() == 2);

    // Ekspor bundel repo lalu impor ke vault lain.
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join(".git")).unwrap();
    let ex = crate::repo_bundle::export_repo(&r, "budi", repo.path(), Some("api-a")).await.unwrap();
    assert_eq!(ex.lessons, 2);
    assert!(repo.path().join(".engram/manifest.json").exists() && repo.path().join(".engram/README.md").exists());
    let d2 = tempfile::tempdir().unwrap();
    let r2 = rt(d2.path()).await;
    r2.pipeline.ensure_subject("budi", Some("Budi")).await.unwrap();
    let im = crate::repo_bundle::import_repo(&r2, "budi", repo.path(), None).await.unwrap();
    assert_eq!((im.project.as_str(), im.lessons_new, im.lessons_merged), ("api-a", 2, 0));
    let im2 = crate::repo_bundle::import_repo(&r2, "budi", repo.path(), None).await.unwrap();
    assert_eq!((im2.lessons_new, im2.lessons_merged), (0, 2));
    let l = r2.store.list_lessons("budi", &["api-a".into()], None, 10).await.unwrap();
    assert_eq!(l.len(), 2);
    assert!(l.iter().all(|x| x.promoted_to.is_none() && x.subject == "budi"));
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().is_dir() {
            out.extend(walk(&e.path()));
        } else {
            out.push(e.path());
        }
    }
    out
}

#[test]
fn merge_unions_id_lists_and_converges() {
    use crate::sync::{merge, same};
    let a = json!({"id": "e1", "name": "Budi", "episode_ids": ["x", "y"], "mention_count": 2, "error": null});
    let b = json!({"id": "e1", "name": "Budi", "episode_ids": ["x", "z"], "mention_count": 3});
    // Masing-masing komputer menggabung dari sisinya; hasilnya setara.
    let on_a = merge(&a, &b);
    let on_b = merge(&b, &a);
    assert!(same(&on_a, &on_b), "{on_a} vs {on_b}");
    assert_eq!(on_a["mention_count"], 3);
    assert_eq!(on_a["episode_ids"].as_array().unwrap().len(), 3);
    // null sama dengan field tidak ada.
    assert!(same(&json!({"id": "s", "error": null}), &json!({"id": "s"})));
}

#[tokio::test]
async fn remap_projects_dry_run_then_apply() {
    use crate::remap::{RemapPlan, remap_projects};
    let d = tempfile::tempdir().unwrap();
    let r = rt(d.path()).await;
    let out = r
        .pipeline
        .ingest(IngestDoc {
            subject: "budi".into(),
            kind: SourceKind::AgentConversation,
            title: "Diingat agent: antigravity:T".into(),
            text: "Budi menambah filter unit pada laporan panen di api-trans-panen.".into(),
            project: Some("T".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    r.pipeline.process_source(&out.source.id, None).await.unwrap();
    let eps = r.store.list_episodes("budi", &memory_store::EpisodeFilter { limit: 10, ..Default::default() }).await.unwrap();
    assert!(!eps.is_empty());
    assert!(eps.iter().all(|e| e.project.as_deref() == Some("T")));

    let mut plan = RemapPlan { subject: "budi".into(), ..Default::default() };
    for e in &eps {
        plan.episodes.insert(e.id.clone(), Some("api-trans-panen".into()));
    }
    plan.sources.insert(out.source.id.clone(), Some("api-trans-panen".into()));
    plan.episodes.insert("01tidakada".into(), Some("x".into()));
    plan.tags.insert("api-trans-panen".into(), vec!["#VOLTUNES".into()]);

    let dry = remap_projects(&r, &plan, false).await.unwrap();
    assert_eq!(dry.changes.len(), eps.len() + 1);
    assert_eq!(dry.missing, vec!["episode:01tidakada".to_string()]);
    let still = r.store.get_episode(&eps[0].id).await.unwrap().unwrap();
    assert_eq!(still.project.as_deref(), Some("T"), "uji coba tidak menulis");
    assert!(r.store.get_project_meta("budi", "api-trans-panen").await.unwrap().is_none());

    let done = remap_projects(&r, &plan, true).await.unwrap();
    assert!(done.applied);
    let moved = r.store.get_episode(&eps[0].id).await.unwrap().unwrap();
    assert_eq!(moved.project.as_deref(), Some("api-trans-panen"));
    let src = r.store.get_source(&out.source.id).await.unwrap().unwrap();
    assert_eq!(src.project.as_deref(), Some("api-trans-panen"));
    assert_eq!(r.store.get_project_meta("budi", "api-trans-panen").await.unwrap().unwrap().tags, vec!["VOLTUNES".to_string()]);

    // Idempoten: menjalankan ulang tidak mengubah apa pun.
    let again = remap_projects(&r, &plan, true).await.unwrap();
    assert!(again.changes.is_empty() && again.tagged.is_empty());

    // Subjek lain tidak tersentuh, nama proyek kosong ditolak.
    let other = RemapPlan { subject: "lain".into(), episodes: [(eps[0].id.clone(), Some("Z".to_string()))].into(), ..Default::default() };
    assert_eq!(remap_projects(&r, &other, true).await.unwrap().missing.len(), 1);
    let bad = RemapPlan { subject: "budi".into(), episodes: [(eps[0].id.clone(), Some(" ".to_string()))].into(), ..Default::default() };
    assert!(remap_projects(&r, &bad, false).await.is_err());

    // `null` mengosongkan proyek (sumber di folder temp tanpa proyek).
    let clear = RemapPlan { subject: "budi".into(), sources: [(out.source.id.clone(), None)].into(), ..Default::default() };
    assert_eq!(remap_projects(&r, &clear, true).await.unwrap().changes.len(), 1);
    assert_eq!(r.store.get_source(&out.source.id).await.unwrap().unwrap().project, None);
}

#[tokio::test]
async fn v3_rocksdb_vault_is_refused_then_migrated_from_backup() {
    // Vault sumber (v0.4) dengan satu subjek, lalu backup logis (format yang sama dengan v0.3).
    let src = tempfile::tempdir().unwrap();
    let r = rt(src.path()).await;
    r.store.upsert("subject", "uji", &json!({"name": "Subjek Uji", "created_at": "2026-11-12T00:00:00Z"}), None).await.unwrap();
    let backup = crate::archive::backup(&r, &src.path().join("v3.tar.gz"), None).await.unwrap();

    // Vault tujuan berisi RocksDB v0.3 di db/ (cukup berkas CURRENT).
    let dst = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dst.path().join("db")).unwrap();
    std::fs::write(dst.path().join("db/CURRENT"), "MANIFEST-000001\n").unwrap();
    let mut cfg = MemoryConfig::for_tests(DIM);
    cfg.vault.path = dst.path().to_path_buf();
    cfg.vault.engine = "surrealkv".into();
    let mk = || (Arc::new(ScriptedLlm::new(llm)) as Arc<dyn memory_llm::LlmProvider>, Arc::new(HashEmbedder::new(DIM)) as Arc<dyn memory_llm::Embedder>);

    let (l, e) = mk();
    let err = Runtime::open_with(cfg.clone(), l, e).await.err().expect("vault v0.3 harus ditolak").to_string();
    assert!(err.contains("migrate-v3"), "{err}");

    let (l, e) = mk();
    let m = Runtime::open_for_migration_with(cfg.clone(), l, e).await.unwrap();
    let rep = crate::archive::migrate_v3(&m, &backup, None).await.unwrap();
    assert!(rep.restore.rows >= 1);
    assert!(rep.archived_legacy_db.as_ref().is_some_and(|p| p.join("CURRENT").exists()));
    assert!(!dst.path().join("db").exists());
    assert!(m.store.list_subjects().await.unwrap().iter().any(|s| s.id == "uji"));
    drop(m);

    // Setelah migrasi vault terbuka normal; "rocksdb" lama diterima sebagai alias.
    cfg.vault.engine = "rocksdb".into();
    let mut opened = None;
    for _ in 0..50 {
        let (l, e) = mk();
        match Runtime::open_with(cfg.clone(), l, e).await {
            Ok(r) => {
                opened = Some(r);
                break;
            }
            Err(e) if e.to_string().to_lowercase().contains("lock") => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            Err(e) => panic!("{e}"),
        }
    }
    assert!(opened.unwrap().store.list_subjects().await.unwrap().iter().any(|s| s.id == "uji"));
}
