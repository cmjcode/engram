//! Memori proyek bersama lewat hub tim (pengganti bundel `.engram/members/` di git).
//!
//! Satu-satunya berkas di repo adalah `.engram/repo.toml` (repo_id, URL hub, join key, apa yang
//! dibagikan). Memori terdistilasi dikirim ke hub sebagai delta (`push`) dan memori rekan ditarik
//! inkremental lewat cursor `seq` (`pull`). Tidak ada lagi berkas per record, tombstone, atau
//! README yang berubah di git, jadi `git status` tetap bersih.
//!
//! Penyaringan privasi (`Cleaner`), pengumpulan record (`collect`), dan penerapan record rekan
//! (`apply_record`) dipakai bersama dengan jalur bundel lama di [`crate::repo_bundle`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use memory_core::ids::now_rfc3339;
use memory_core::model::Job;
use memory_hub::client::{HubClient, HubClientError};
use memory_hub::types::{JoinIn, PushIn, RecordIn, RecordRef, RepoCreateIn, ShareSettings};

use crate::repo_bundle::{self as rb, Author, BundleConfig, BundleReport, RUN_LOCK, RepoState};
use crate::{JobError, Result, Runtime};

pub const REPO_TOML: &str = "repo.toml";
pub const REPO_PUSH: &str = "repo_push";
pub const REPO_PULL: &str = "repo_pull";
pub const REPO_TOML_FORMAT: u32 = 1;
/// Record per permintaan push (batas hub bawaan 2000).
const PUSH_CHUNK: usize = 500;
const PULL_PAGE: usize = 500;

/// `.engram/repo.toml`: aman di-commit; siapa pun yang bisa clone repo boleh bergabung.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RepoToml {
    pub format: u32,
    pub repo_id: String,
    pub hub: String,
    pub join_key: String,
    pub project: String,
    pub share: ShareSettings,
}

impl Default for RepoToml {
    fn default() -> Self {
        Self { format: REPO_TOML_FORMAT, repo_id: String::new(), hub: String::new(), join_key: String::new(), project: String::new(), share: ShareSettings::default() }
    }
}

const TOML_HEADER: &str = "# Memori tim engram. Aman di-commit: siapa pun yang bisa clone repo ini boleh bergabung\n# ke memori proyeknya di hub. Rotasi key: `engram repo rotate-key`.\n# Memori disimpan di hub, bukan di repo; hanya berkas ini yang ikut git.\n\n";

pub fn repo_toml_path(root: &Path) -> PathBuf {
    root.join(rb::BUNDLE_DIR).join(REPO_TOML)
}

pub fn read_repo_toml(root: &Path) -> Option<RepoToml> {
    let s = std::fs::read_to_string(repo_toml_path(root)).ok()?;
    toml::from_str::<RepoToml>(&s).ok().filter(|t| !t.repo_id.trim().is_empty() && !t.hub.trim().is_empty())
}

pub fn write_repo_toml(root: &Path, t: &RepoToml) -> Result<()> {
    let p = repo_toml_path(root);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let body = toml::to_string_pretty(t).map_err(|e| JobError::Invalid(format!("repo.toml: {e}")))?;
    std::fs::write(&p, format!("{TOML_HEADER}{body}"))?;
    Ok(())
}

/// Status sinkronisasi hub per repo (bagian dari `<vault>/repo_share.json`, tidak dibagikan).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HubRepoState {
    pub repo_id: String,
    pub hub_url: String,
    /// `seq` terakhir yang sudah diterapkan dari hub.
    pub cursor: u64,
    /// `<jenis>/<rid>` -> hash isi yang terakhir berhasil dikirim.
    pub pushed: BTreeMap<String, String>,
    pub role: Option<String>,
    pub last_push_at: Option<String>,
    pub last_pull_at: Option<String>,
    pub last_error: Option<String>,
}

/// Hasil push/pull/init/join untuk CLI & UI.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HubSyncReport {
    pub root: PathBuf,
    pub project: String,
    pub repo_id: String,
    pub hub: String,
    /// Push: record yang dikirim / dicabut / tidak berubah / ditolak.
    pub pushed: u32,
    pub withdrawn: u32,
    pub unchanged: u32,
    pub rejected: u32,
    /// Pull: record rekan yang diterapkan.
    pub pulled: u32,
    pub cursor: u64,
    pub head_seq: u64,
    pub dry_run: bool,
    pub preview: Vec<String>,
    /// Rincian penerapan record rekan (pelajaran baru, keputusan, fakta, episode...).
    pub import: Option<BundleReport>,
    pub warnings: Vec<String>,
    /// `init`/`migrate`/`rotate-key`: perintah git yang perlu dijalankan.
    pub commit_hint: Option<String>,
    pub role: Option<String>,
}

/// Status hub sebuah repo tanpa panggilan jaringan (kartu proyek, `engram repo status`).
#[derive(Debug, Clone, Default, Serialize)]
pub struct HubShareStatus {
    pub root: PathBuf,
    pub project: String,
    /// `hub` (repo.toml ada), `legacy` (hanya bundel lama), `none`.
    pub mode: String,
    pub repo_id: Option<String>,
    pub hub: Option<String>,
    pub joined: bool,
    pub role: Option<String>,
    pub cursor: u64,
    pub pushed: usize,
    pub imported: usize,
    pub last_push_at: Option<String>,
    pub last_pull_at: Option<String>,
    pub last_error: Option<String>,
    pub logged_in: bool,
    pub login_hub: Option<String>,
    /// Folder `members/` lama masih ada (perlu `engram repo migrate`).
    pub legacy_bundle: bool,
    pub legacy_hooks: Vec<String>,
    pub enabled: bool,
    pub auto_pull: bool,
    pub auto_push: bool,
}

/// Galat klien hub -> galat job; `retry` menandai galat sementara (jaringan, 5xx, 429).
fn hub_err(e: HubClientError) -> JobError {
    if e.is_retryable() { JobError::Hub { msg: e.to_string(), retry: true } } else { JobError::Hub { msg: e.to_string(), retry: false } }
}

fn content_hash(body: &Value) -> String {
    blake3::hash(&rb::canonical(body)).to_hex()[..32].to_string()
}

/// URL remote `origin` dari `.git/config` (untuk menautkan repo hub ke repo GitHub).
fn git_remote(root: &Path) -> Option<String> {
    let gd = rb::git_dir_of(root)?;
    let text = std::fs::read_to_string(gd.join("config")).ok()?;
    let mut cur = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            cur = l.trim_start_matches('[').trim_end_matches(']').trim().eq_ignore_ascii_case("remote \"origin\"");
            continue;
        }
        if cur
            && let Some((k, v)) = l.split_once('=')
            && k.trim() == "url"
        {
            return Some(v.trim().to_string()).filter(|v| !v.is_empty());
        }
    }
    None
}

/// Konfigurasi `collect` dari `repo.toml` (+ saklar `share_episodes` lokal).
fn bundle_cfg(rt: &Runtime, t: &RepoToml) -> BundleConfig {
    let share = t.share.kinds.iter().filter(|k| rt.cfg.repo_share.share_episodes || k.as_str() != "episodes").cloned().collect();
    BundleConfig { share, max_records: t.share.max_records.max(1), exclude: t.share.exclude.clone() }
}

fn state_of(rt: &Runtime, root: &Path) -> Result<(String, RepoState)> {
    let key = rb::key_of(root);
    rb::with_state(&rt.vault, |st| (st.device_id.clone(), st.repos.get(&key).cloned().unwrap_or_default()))
}

fn save_state(rt: &Runtime, root: &Path, f: impl FnOnce(&mut RepoState)) -> Result<()> {
    let key = rb::key_of(root);
    rb::with_state(&rt.vault, |st| f(st.repos.entry(key).or_default()))
}

fn note_error(rt: &Runtime, root: &Path, toml: &RepoToml, e: &JobError) {
    let msg = e.to_string();
    let _ = save_state(rt, root, |r| {
        let mut h = hub_state_for(r, toml);
        h.last_error = Some(msg);
        r.hub = Some(h);
    });
}

fn require_toml(root: &Path) -> Result<RepoToml> {
    read_repo_toml(root).ok_or_else(|| JobError::Invalid(format!("{} belum terhubung ke hub (tidak ada .engram/repo.toml); jalankan `engram repo init`", root.display())))
}

fn hub_state_for(rs: &RepoState, t: &RepoToml) -> HubRepoState {
    match &rs.hub {
        Some(h) if h.repo_id == t.repo_id => h.clone(),
        _ => HubRepoState { repo_id: t.repo_id.clone(), hub_url: t.hub.clone(), ..Default::default() },
    }
}

fn git_root(repo: &Path) -> Result<PathBuf> {
    Ok(rb::resolve_target(repo, Some("_"))?.root)
}

// ---------------------------------------------------------------- push

/// Kirim memori proyek perangkat ini ke hub. Hanya delta (record baru/berubah/hilang) yang
/// dikirim; `full` mengirim semuanya dan membuat hub mencabut record lain milik perangkat ini.
pub async fn push_repo(rt: &Runtime, client: &HubClient, subject: &str, repo: &Path, full: bool, dry_run: bool) -> Result<HubSyncReport> {
    let _g = RUN_LOCK.lock().await;
    push_unlocked(rt, client, subject, repo, full, dry_run).await
}

async fn push_unlocked(rt: &Runtime, client: &HubClient, subject: &str, repo: &Path, full: bool, dry_run: bool) -> Result<HubSyncReport> {
    let root = git_root(repo)?;
    let toml = require_toml(&root)?;
    let res = push_locked(rt, client, subject, &root, &toml, full, dry_run).await;
    if let Err(e) = &res {
        note_error(rt, &root, &toml, e);
    }
    res
}

#[allow(clippy::too_many_arguments)]
async fn push_locked(rt: &Runtime, client: &HubClient, subject: &str, root: &Path, toml: &RepoToml, full: bool, dry_run: bool) -> Result<HubSyncReport> {
    let t = rb::resolve_target(root, Some(&toml.project))?;
    let names = rt.store.project_names(subject, &t.project).await?;
    let (device_id, skip, rs) = {
        let key = rb::key_of(&t.root);
        let project = t.project.clone();
        rb::with_state(&rt.vault, |st| (st.device_id.clone(), rb::imported_lessons(st, &project), st.repos.get(&key).cloned().unwrap_or_default()))?
    };
    let records = rb::collect(rt, subject, &t, &names, &bundle_cfg(rt, toml), &skip).await?;
    let hs = hub_state_for(&rs, toml);
    let current: BTreeMap<String, (String, &rb::Record)> = records.iter().map(|r| (format!("{}/{}", r.kind, r.id), (content_hash(&r.body), r))).collect();
    let mut rep = HubSyncReport { root: t.root.clone(), project: t.project.clone(), repo_id: toml.repo_id.clone(), hub: toml.hub.clone(), dry_run, cursor: hs.cursor, ..Default::default() };
    let upserts: Vec<RecordIn> = current
        .iter()
        .filter(|(k, (h, _))| full || hs.pushed.get(*k) != Some(h))
        .map(|(_, (h, r))| RecordIn { kind: r.kind.to_string(), rid: r.id.clone(), content_hash: h.clone(), body: r.body.clone() })
        .collect();
    let withdraws: Vec<RecordRef> = hs.pushed.keys().filter(|k| !current.contains_key(*k)).filter_map(|k| k.split_once('/')).map(|(kind, rid)| RecordRef { kind: kind.into(), rid: rid.into() }).collect();
    rep.unchanged = current.len().saturating_sub(upserts.len()) as u32;
    if dry_run {
        rep.preview = upserts
            .iter()
            .map(|u| {
                let b = &u.body;
                let text = match u.kind.as_str() {
                    "decisions" => format!("{} -> {}", b["situation"].as_str().unwrap_or(""), b["action"].as_str().unwrap_or("")),
                    "facts" => b["statement"].as_str().unwrap_or("").to_string(),
                    _ => b["title"].as_str().unwrap_or("").to_string(),
                };
                format!("{}: {text}", u.kind)
            })
            .chain(withdraws.iter().map(|w| format!("{} (dicabut): {}", w.kind, w.rid)))
            .collect();
        rep.pushed = upserts.len() as u32;
        rep.withdrawn = withdraws.len() as u32;
        return Ok(rep);
    }
    if upserts.is_empty() && withdraws.is_empty() && !full {
        save_state(rt, &t.root, |r| {
            r.project = t.project.clone();
            r.subject = subject.to_string();
            let mut h = hub_state_for(r, toml);
            h.last_push_at.get_or_insert_with(now_rfc3339);
            h.last_error = None;
            r.hub = Some(h);
        })?;
        return Ok(rep);
    }
    let device = rb::device_name();
    let mut pushed_ok: BTreeMap<String, String> = hs.pushed.clone();
    // Pencabutan ikut permintaan pertama; upsert dipecah per potongan; push penuh ditutup di
    // potongan terakhir dengan daftar `keep` (record yang sudah terkirim di potongan sebelumnya).
    let chunks: Vec<&[RecordIn]> = upserts.chunks(PUSH_CHUNK).collect();
    let n_chunks = chunks.len().max(1);
    let mut sent: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for i in 0..n_chunks {
        let ups: Vec<RecordIn> = chunks.get(i).map(|c| c.to_vec()).unwrap_or_default();
        let last = i + 1 == n_chunks;
        let keep: Vec<RecordRef> = if full && last {
            let in_this: std::collections::BTreeSet<String> = ups.iter().map(|u| format!("{}/{}", u.kind, u.rid)).collect();
            current.iter().filter(|(k, _)| !in_this.contains(*k)).map(|(_, (_, r))| RecordRef { kind: r.kind.to_string(), rid: r.id.clone() }).collect()
        } else {
            vec![]
        };
        let body = PushIn { device_id: device_id.clone(), device_name: device.clone(), full: full && last, upserts: ups.clone(), withdraws: if i == 0 { withdraws.clone() } else { vec![] }, keep };
        let out = client.push(&toml.repo_id, &body).await.map_err(hub_err)?;
        rep.pushed += out.accepted;
        rep.withdrawn += out.withdrawn;
        rep.rejected += out.rejected;
        rep.head_seq = out.head_seq;
        if i == 0 {
            for w in &withdraws {
                pushed_ok.remove(&format!("{}/{}", w.kind, w.rid));
            }
        }
        for u in &ups {
            let k = format!("{}/{}", u.kind, u.rid);
            pushed_ok.insert(k.clone(), u.content_hash.clone());
            sent.insert(k);
        }
    }
    if full {
        pushed_ok.retain(|k, _| current.contains_key(k));
    }
    rep.unchanged = current.len().saturating_sub(rep.pushed as usize) as u32;
    if rep.rejected > 0 {
        rep.warnings.push(format!("{} record ditolak hub (terlalu besar atau memuat penanda rahasia)", rep.rejected));
    }
    save_state(rt, &t.root, |r| {
        r.project = t.project.clone();
        r.subject = subject.to_string();
        let mut h = hub_state_for(r, toml);
        h.pushed = pushed_ok;
        h.last_push_at = Some(now_rfc3339());
        h.last_error = None;
        r.hub = Some(h);
    })?;
    Ok(rep)
}

// ---------------------------------------------------------------- pull

/// Tarik memori rekan dari hub (sejak cursor terakhir) dan gabungkan ke vault.
pub async fn pull_repo(rt: &Runtime, client: &HubClient, subject: &str, repo: &Path) -> Result<HubSyncReport> {
    let _g = RUN_LOCK.lock().await;
    pull_unlocked(rt, client, subject, repo).await
}

async fn pull_unlocked(rt: &Runtime, client: &HubClient, subject: &str, repo: &Path) -> Result<HubSyncReport> {
    let root = git_root(repo)?;
    let toml = require_toml(&root)?;
    let res = pull_locked(rt, client, subject, &root, &toml).await;
    if let Err(e) = &res {
        note_error(rt, &root, &toml, e);
    }
    res
}

async fn pull_locked(rt: &Runtime, client: &HubClient, subject: &str, root: &Path, toml: &RepoToml) -> Result<HubSyncReport> {
    let t = rb::resolve_target(root, Some(&toml.project))?;
    rt.pipeline.ensure_subject(subject, None).await?;
    let (device_id, mut rs) = state_of(rt, &t.root)?;
    let mut hs = hub_state_for(&rs, toml);
    let mut rep = HubSyncReport { root: t.root.clone(), project: t.project.clone(), repo_id: toml.repo_id.clone(), hub: toml.hub.clone(), ..Default::default() };
    let mut imp = BundleReport { dir: t.dir.clone(), project: t.project.clone(), ..Default::default() };
    let mut authors: BTreeMap<String, String> = BTreeMap::new();
    loop {
        let page = client.changes(&toml.repo_id, hs.cursor, PULL_PAGE).await.map_err(hub_err)?;
        rep.head_seq = page.head;
        for r in &page.records {
            hs.cursor = hs.cursor.max(r.seq);
            // Record perangkat ini sendiri (sudah ada di vault) dilewati.
            if r.device_id == device_id {
                continue;
            }
            let who = Author { key: format!("hub:{}:{}", r.user_id, r.device_id), name: r.body["author"].as_str().filter(|a| !a.trim().is_empty()).unwrap_or(&r.author).to_string(), device: r.device.clone() };
            let mut row = r.body.clone();
            if let Some(o) = row.as_object_mut()
                && r.status == "withdrawn"
            {
                o.insert("status".into(), json!("withdrawn"));
            }
            match rb::apply_record(rt, subject, &t.project, &who, &r.kind, &r.rid, &row, &mut rs, &mut imp).await {
                Ok(()) => {
                    rep.pulled += 1;
                    authors.entry(who.name.clone()).or_insert_with(|| r.device.clone());
                }
                Err(e) => imp.warnings.push(format!("{}/{}: {e}", r.kind, r.rid)),
            }
        }
        // Disimpan per halaman: tarikan yang terputus melanjutkan dari halaman terakhir.
        rs.project = t.project.clone();
        rs.subject = subject.to_string();
        hs.last_pull_at = Some(now_rfc3339());
        hs.last_error = None;
        rs.hub = Some(hs.clone());
        let snapshot = rs.clone();
        save_state(rt, &t.root, move |r| *r = snapshot)?;
        if page.next.is_none() || page.records.is_empty() {
            break;
        }
    }
    imp.members = authors.into_iter().map(|(a, d)| if d.is_empty() { a } else { format!("{a} ({d})") }).collect();
    rep.cursor = hs.cursor;
    rep.warnings.extend(imp.warnings.iter().cloned());
    rep.import = Some(imp);
    Ok(rep)
}

// ---------------------------------------------------------------- init / join / migrate

/// Hubungkan repo ke hub: buat repo bersama, tulis `.engram/repo.toml`, kirim memori pertama.
/// Repo yang sudah punya `repo.toml` tidak dibuat ulang (cukup push penuh + pull).
pub async fn init_repo(rt: &Runtime, client: &HubClient, subject: &str, repo: &Path, project: Option<&str>) -> Result<HubSyncReport> {
    let _g = RUN_LOCK.lock().await;
    init_unlocked(rt, client, subject, repo, project).await
}

async fn init_unlocked(rt: &Runtime, client: &HubClient, subject: &str, repo: &Path, project: Option<&str>) -> Result<HubSyncReport> {
    let t = rb::resolve_target(repo, project)?;
    if rb::git_dir_of(&t.root).is_none() {
        return Err(JobError::Invalid(format!("{} bukan repositori git", t.root.display())));
    }
    let toml = match read_repo_toml(&t.root) {
        Some(x) => x,
        None => {
            let legacy = rb::load_bundle_config(&t.dir);
            let share = ShareSettings {
                kinds: memory_hub::ids::RECORD_KINDS.iter().filter(|k| **k == "episodes" || legacy.shares(k)).map(|k| k.to_string()).collect(),
                max_records: legacy.max_records,
                exclude: legacy.exclude.clone(),
            };
            let out = client
                .create_repo(&RepoCreateIn { name: t.project.clone(), project: t.project.clone(), remote: git_remote(&t.root), github_full_name: None, share: Some(share.clone()), require_github_verify: None })
                .await
                .map_err(hub_err)?;
            let toml = RepoToml { format: REPO_TOML_FORMAT, repo_id: out.repo.id, hub: client.base().to_string(), join_key: out.join_key, project: t.project.clone(), share };
            write_repo_toml(&t.root, &toml)?;
            save_state(rt, &t.root, |r| {
                let mut h = hub_state_for(r, &toml);
                h.role = Some("owner".into());
                r.hub = Some(h);
            })?;
            toml
        }
    };
    save_state(rt, &t.root, |r| {
        r.project = toml.project.clone();
        r.subject = subject.to_string();
        let h = hub_state_for(r, &toml);
        r.hub = Some(h);
    })?;
    let mut rep = push_unlocked(rt, client, subject, &t.root, true, false).await?;
    let pulled = pull_unlocked(rt, client, subject, &t.root).await?;
    rep.pulled = pulled.pulled;
    rep.cursor = pulled.cursor;
    rep.head_seq = rep.head_seq.max(pulled.head_seq);
    rep.import = pulled.import;
    rep.role = state_of(rt, &t.root)?.1.hub.and_then(|h| h.role);
    rep.commit_hint = Some("git add .engram/repo.toml && git commit -m \"engram: hubungkan memori tim ke hub\"".into());
    if t.dir.join(rb::MEMBERS_DIR).is_dir() {
        rep.warnings.push("folder .engram/members/ format lama masih ada; jalankan `engram repo migrate` untuk memindahkannya ke hub".into());
    }
    Ok(rep)
}

/// Bergabung ke memori repo yang sudah punya `repo.toml` (join key dari berkas itu, atau
/// verifikasi izin GitHub), lalu tarik memori rekan.
pub async fn join_repo(rt: &Runtime, client: &HubClient, subject: &str, repo: &Path, github_token: Option<String>) -> Result<HubSyncReport> {
    let _g = RUN_LOCK.lock().await;
    join_unlocked(rt, client, subject, repo, github_token).await
}

async fn join_unlocked(rt: &Runtime, client: &HubClient, subject: &str, repo: &Path, github_token: Option<String>) -> Result<HubSyncReport> {
    let root = git_root(repo)?;
    let toml = require_toml(&root)?;
    let body = JoinIn { join_key: Some(toml.join_key.clone()).filter(|k| !k.trim().is_empty()), github_token, device_name: Some(rb::device_name()) };
    let out = match client.join(&toml.repo_id, &body).await {
        Ok(o) => o,
        Err(e) => {
            let e = hub_err(e);
            note_error(rt, &root, &toml, &e);
            return Err(e);
        }
    };
    save_state(rt, &root, |r| {
        r.project = toml.project.clone();
        r.subject = subject.to_string();
        let mut h = hub_state_for(r, &toml);
        h.role = Some(out.role.clone());
        h.last_error = None;
        r.hub = Some(h);
    })?;
    let mut rep = pull_unlocked(rt, client, subject, &root).await?;
    rep.role = Some(out.role);
    Ok(rep)
}

/// Pindahkan bundel `.engram/members/*` lama ke hub lalu bersihkan repo: hanya `repo.toml` yang
/// tersisa dan git hook engram dicabut. Record anggota lain dikirim atas nama pemigrasi dengan
/// perangkat `legacy:<folder>`; nama penulis asli disimpan di field `author` record.
pub async fn migrate_repo(rt: &Runtime, client: &HubClient, subject: &str, repo: &Path, project: Option<&str>) -> Result<HubSyncReport> {
    let _g = RUN_LOCK.lock().await;
    let t = rb::resolve_target(repo, project)?;
    let mut rep = init_unlocked(rt, client, subject, &t.root, project).await?;
    rep.warnings.retain(|w| !w.contains("engram repo migrate"));
    let toml = require_toml(&t.root)?;
    let dir = t.dir.clone();
    let members: Vec<rb::MemberDir> = tokio::task::spawn_blocking(move || rb::list_member_ids(&dir).into_iter().map(|id| rb::read_member(&dir.join(rb::MEMBERS_DIR).join(&id), &id)).collect())
        .await
        .map_err(|e| JobError::Invalid(format!("membaca bundel lama: {e}")))?;
    let mut migrated = 0u32;
    for m in &members {
        let author = if m.info.name.trim().is_empty() { m.id.clone() } else { m.info.name.trim().to_string() };
        let mut ups: Vec<RecordIn> = Vec::new();
        for (rel, body) in &m.records {
            let Some((kind, file)) = rel.split_once('/') else { continue };
            let Some(rid) = rb::record_id_of(file) else { continue };
            if body["status"] == "withdrawn" || !memory_hub::ids::is_record_kind(kind) {
                continue;
            }
            let mut b = body.clone();
            if let Some(o) = b.as_object_mut() {
                o.insert("author".into(), json!(author));
            }
            ups.push(RecordIn { kind: kind.into(), rid: rid.into(), content_hash: content_hash(&b), body: b });
        }
        for chunk in ups.chunks(PUSH_CHUNK) {
            let out = client
                .push(&toml.repo_id, &PushIn { device_id: format!("legacy:{}", m.id), device_name: format!("{author} (bundel git)"), full: false, upserts: chunk.to_vec(), withdraws: vec![], keep: vec![] })
                .await
                .map_err(hub_err)?;
            migrated += out.accepted;
            rep.head_seq = out.head_seq;
        }
    }
    rep.pushed += migrated;
    // Bersihkan bundel lama; hanya repo.toml yang tersisa.
    let dir = t.dir.clone();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let members = dir.join(rb::MEMBERS_DIR);
        if members.is_dir() {
            std::fs::remove_dir_all(&members)?;
        }
        for f in [rb::MANIFEST, "README.md", ".gitattributes", rb::CONFIG_FILE] {
            let p = dir.join(f);
            if p.is_file() {
                std::fs::remove_file(&p)?;
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| JobError::Invalid(format!("membersihkan bundel lama: {e}")))??;
    let hooks = rb::git_hooks(&t.root, true)?;
    if !hooks.removed.is_empty() {
        rep.warnings.push(format!("git hook engram dicabut: {}", hooks.removed.join(", ")));
    }
    save_state(rt, &t.root, |r| {
        r.fingerprint = None;
        r.pulled.clear();
    })?;
    rep.commit_hint = Some("git rm -r --cached --ignore-unmatch .engram && git add .engram/repo.toml && git commit -m \"engram: pindahkan memori tim ke hub\"".into());
    Ok(rep)
}

/// Rotasi join key (maintainer+): key lama langsung tidak berlaku, `repo.toml` ditulis ulang.
pub async fn rotate_key(client: &HubClient, repo: &Path) -> Result<HubSyncReport> {
    let root = git_root(repo)?;
    let mut toml = require_toml(&root)?;
    let out = client.rotate_key(&toml.repo_id).await.map_err(hub_err)?;
    toml.join_key = out.join_key;
    write_repo_toml(&root, &toml)?;
    Ok(HubSyncReport {
        root,
        project: toml.project,
        repo_id: toml.repo_id,
        hub: toml.hub,
        commit_hint: Some(format!("git add .engram/repo.toml && git commit -m \"engram: rotasi join key (v{})\"", out.version)),
        ..Default::default()
    })
}

/// Lepas repo dari hub di vault ini (memori yang sudah ditarik tetap ada; `repo.toml` tidak disentuh).
pub fn disconnect(rt: &Runtime, repo: &Path) -> Result<()> {
    let root = git_root(repo)?;
    save_state(rt, &root, |r| r.hub = None)
}

// ---------------------------------------------------------------- status

pub async fn status(rt: &Runtime, repo: &Path, project: Option<&str>) -> Result<HubShareStatus> {
    // Nama proyek: argumen, lalu `repo.toml`, lalu akar git / nama folder.
    let toml = read_repo_toml(&git_root(repo)?);
    let t = match rb::resolve_target(repo, project.or(toml.as_ref().map(|x| x.project.as_str()).filter(|p| !p.is_empty()))) {
        Ok(t) => t,
        Err(_) => rb::resolve_target(repo, Some("_"))?,
    };
    let (_, rs) = state_of(rt, &t.root)?;
    let creds = memory_hub::client::HubCreds::load();
    let sc = &rt.cfg.repo_share;
    let legacy_bundle = t.dir.join(rb::MEMBERS_DIR).is_dir();
    let hs = toml.as_ref().map(|x| hub_state_for(&rs, x));
    let root = t.root.clone();
    let legacy_hooks = tokio::task::spawn_blocking(move || rb::installed_hooks(&root)).await.unwrap_or_default();
    Ok(HubShareStatus {
        mode: quick_mode(&t.root).into(),
        project: toml.as_ref().map(|x| x.project.clone()).filter(|p| !p.is_empty()).unwrap_or(t.project),
        repo_id: toml.as_ref().map(|x| x.repo_id.clone()),
        hub: toml.as_ref().map(|x| x.hub.clone()),
        joined: hs.as_ref().is_some_and(|h| h.role.is_some()),
        role: hs.as_ref().and_then(|h| h.role.clone()),
        cursor: hs.as_ref().map(|h| h.cursor).unwrap_or(0),
        pushed: hs.as_ref().map(|h| h.pushed.len()).unwrap_or(0),
        imported: rs.imported.len(),
        last_push_at: hs.as_ref().and_then(|h| h.last_push_at.clone()),
        last_pull_at: hs.as_ref().and_then(|h| h.last_pull_at.clone()),
        last_error: hs.as_ref().and_then(|h| h.last_error.clone()),
        logged_in: creds.is_some() || std::env::var("ENGRAM_HUB_TOKEN").is_ok_and(|v| !v.trim().is_empty()),
        login_hub: creds.map(|c| c.hub),
        legacy_bundle,
        legacy_hooks,
        enabled: sc.enabled,
        auto_pull: sc.auto_import,
        auto_push: sc.auto_export,
        root: t.root,
    })
}

/// Mode berbagi repo tanpa query DB: `hub` / `legacy` / `none` (kartu beranda).
pub fn quick_mode(root: &Path) -> &'static str {
    let dir = root.join(rb::BUNDLE_DIR);
    if dir.join(REPO_TOML).is_file() {
        "hub"
    } else if dir.join(rb::MANIFEST).is_file() || dir.join(rb::MEMBERS_DIR).is_dir() {
        "legacy"
    } else {
        "none"
    }
}

// ---------------------------------------------------------------- otomatisasi

async fn enqueue(rt: &Runtime, kind: &str, subject: &str, root: &Path, every_secs: i64) -> Result<bool> {
    let payload = json!({"subject": subject, "root": root});
    let rk = blake3::hash(rb::key_of(root).as_bytes()).to_hex()[..16].to_string();
    let bucket = chrono::Utc::now().timestamp() / every_secs.max(1);
    let mut job = Job::new(kind, payload, format!("{kind}:{rk}:{bucket}"), None);
    job.max_attempts = 6;
    let (_, created) = rt.store.enqueue(&job).await?;
    Ok(created)
}

/// Dipanggil saat sesi agent dimulai (`context`) / berakhir (`session-end`) dan dari git hook
/// lama: repo ber-`repo.toml` diantre pull (dan push saat sesi berakhir); repo yang hanya punya
/// bundel lama diimpor read-only. Tidak pernah memproses langsung (hook harus cepat).
pub async fn on_repo_event(rt: &Runtime, subject: &str, cwd: &Path, project: Option<&str>, session_end: bool) -> Result<Value> {
    let sc = &rt.cfg.repo_share;
    if !sc.enabled {
        return Ok(json!({"enabled": false}));
    }
    let cwd2 = cwd.to_path_buf();
    let found = tokio::task::spawn_blocking(move || rb::repo_root_of(&cwd2)).await.map_err(|e| JobError::Invalid(e.to_string()))?;
    let Some((root, _)) = found else { return Ok(json!({"repo": null})) };
    let Some(toml) = read_repo_toml(&root) else {
        if sc.legacy_import {
            return rb::on_repo_event_import_only(rt, subject, cwd, project).await;
        }
        return Ok(json!({"repo": root, "hub": false}));
    };
    save_state(rt, &root, |r| {
        r.project = toml.project.clone();
        r.subject = subject.to_string();
        let h = hub_state_for(r, &toml);
        r.hub = Some(h);
    })?;
    let pull = sc.auto_import && enqueue(rt, REPO_PULL, subject, &root, 300).await?;
    let push = sc.auto_export && session_end && enqueue(rt, REPO_PUSH, subject, &root, 600).await?;
    Ok(json!({"repo": root, "hub": true, "pull_queued": pull, "push_queued": push}))
}

/// Pemeriksaan berkala: pull + push semua repo hub yang dikenal; repo bundel lama hanya impor.
pub async fn schedule_due(rt: &Runtime) -> Result<u32> {
    let sc = &rt.cfg.repo_share;
    if !sc.enabled {
        return Ok(0);
    }
    let repos = rb::with_state(&rt.vault, |st| st.repos.clone())?;
    let mut n = 0;
    for (root, rs) in repos {
        let root = PathBuf::from(root);
        if rs.subject.is_empty() || !root.join(".git").exists() || read_repo_toml(&root).is_none() {
            continue;
        }
        n += u32::from(sc.auto_import && enqueue(rt, REPO_PULL, &rs.subject, &root, 300).await?);
        n += u32::from(sc.auto_export && enqueue(rt, REPO_PUSH, &rs.subject, &root, 600).await?);
    }
    if sc.legacy_import {
        n += rb::schedule_due_import_only(rt).await?;
    }
    Ok(n)
}

/// Klien hub untuk job: PAT dari env atau kredensial `hub.json` untuk hub di `repo.toml`.
pub fn client_for(toml: &RepoToml) -> std::result::Result<HubClient, HubClientError> {
    HubClient::from_env_or_creds(Some(&toml.hub))
}

/// Jalankan job `repo_push` / `repo_pull` dengan kredensial pengguna.
pub async fn run_job(rt: &Runtime, kind: &str, payload: &Value) -> Result<Value> {
    let root = PathBuf::from(payload["root"].as_str().ok_or_else(|| JobError::Invalid("payload tanpa root".into()))?);
    let Some(toml) = read_repo_toml(&root) else { return Ok(json!({"skipped": "repo belum terhubung ke hub"})) };
    match client_for(&toml) {
        Ok(c) => run_job_with(rt, &c, kind, payload).await,
        Err(e) => {
            // Belum login: dicatat di status repo, tidak diulang.
            let msg = e.to_string();
            let _ = save_state(rt, &root, |r| {
                let mut h = hub_state_for(r, &toml);
                h.last_error = Some(msg.clone());
                r.hub = Some(h);
            });
            Ok(json!({"skipped": msg}))
        }
    }
}

/// Seperti [`run_job`] dengan klien tertentu (uji). Repo yang belum di-join otomatis bergabung
/// dengan join key dari `repo.toml` (nol-sentuh bagi rekan tim). Galat sementara (jaringan, 5xx)
/// dikembalikan agar antrean mengulang dengan backoff; galat permanen hanya dicatat.
pub async fn run_job_with(rt: &Runtime, client: &HubClient, kind: &str, payload: &Value) -> Result<Value> {
    let subject = payload["subject"].as_str().ok_or_else(|| JobError::Invalid("payload tanpa subject".into()))?;
    let root = PathBuf::from(payload["root"].as_str().ok_or_else(|| JobError::Invalid("payload tanpa root".into()))?);
    if !root.is_dir() {
        return Ok(json!({"skipped": "repo tidak ditemukan"}));
    }
    let Some(toml) = read_repo_toml(&root) else { return Ok(json!({"skipped": "repo belum terhubung ke hub"})) };
    let (_, rs) = state_of(rt, &root)?;
    let joined = rs.hub.as_ref().is_some_and(|h| h.repo_id == toml.repo_id && h.role.is_some());
    let res = if !joined {
        join_repo(rt, client, subject, &root, None).await
    } else if kind == REPO_PUSH {
        push_repo(rt, client, subject, &root, false, false).await
    } else {
        pull_repo(rt, client, subject, &root).await
    };
    match res {
        Ok(r) => Ok(json!(r)),
        Err(e @ JobError::Hub { retry: true, .. }) => Err(e),
        Err(e) => Ok(json!({"skipped": e.to_string()})),
    }
}

// ---------------------------------------------------------------- perintah CLI / UI

/// Klien hub untuk repo `root`: hub dari `repo.toml`, selain itu `hub_override`, lalu
/// `[repo_share] hub_url`, lalu hub tempat pengguna sedang login.
pub fn client_for_repo(rt: &Runtime, root: &Path, hub_override: Option<&str>) -> Result<HubClient> {
    let hint = read_repo_toml(root).map(|t| t.hub).or_else(|| hub_override.map(str::trim).filter(|h| !h.is_empty()).map(str::to_string)).or_else(|| rt.cfg.repo_share.hub_url.clone().filter(|h| !h.trim().is_empty()));
    HubClient::from_env_or_creds(hint.as_deref()).map_err(hub_err)
}

/// Satu aksi `engram repo <aksi>` (`options.action`): dipakai CLI langsung maupun server
/// (`/api/hooks/cli/repo-cli`) agar keduanya berperilaku sama.
pub async fn cli_action(rt: &Runtime, subject: &str, path: &Path, project: Option<&str>, options: &Value) -> Result<Value> {
    let opt = |k: &str| options[k].as_bool().unwrap_or(false);
    let ostr = |k: &str| options[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let root = rb::resolve_target(path, project.or(Some("_")))?.root;
    let action = options["action"].as_str().unwrap_or("status");
    Ok(match action {
        "init" => json!(init_repo(rt, &client_for_repo(rt, &root, ostr("hub").as_deref())?, subject, &root, project).await?),
        "migrate" => json!(migrate_repo(rt, &client_for_repo(rt, &root, ostr("hub").as_deref())?, subject, &root, project).await?),
        "join" => json!(join_repo(rt, &client_for_repo(rt, &root, None)?, subject, &root, ostr("github_token")).await?),
        "push" => json!(push_repo(rt, &client_for_repo(rt, &root, None)?, subject, &root, opt("full"), opt("dry_run")).await?),
        "pull" => json!(pull_repo(rt, &client_for_repo(rt, &root, None)?, subject, &root).await?),
        "rotate-key" => json!(rotate_key(&client_for_repo(rt, &root, None)?, &root).await?),
        "disconnect" => {
            disconnect(rt, &root)?;
            json!({"ok": true})
        }
        "status" => json!(status(rt, &root, project).await?),
        // Jalur bundel lama (dipertahankan satu rilis untuk repo yang belum migrasi).
        "export" => json!(rb::export_with(rt, subject, &root, project, opt("dry_run")).await?),
        "import" => json!(rb::import_repo(rt, subject, &root, project).await?),
        "hooks" => {
            if opt("remove") {
                json!(rb::git_hooks(&root, true)?)
            } else {
                json!({"removed": [], "installed": [], "skipped": ["git hook tidak dipakai lagi: memori tim disinkronkan lewat hub saat sesi agent dan berkala"]})
            }
        }
        other => return Err(JobError::Invalid(format!("aksi repo tidak dikenal: {other}"))),
    })
}
