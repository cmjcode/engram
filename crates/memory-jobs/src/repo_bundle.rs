//! Berbagi memori proyek lewat repositori git: siapa pun yang punya akses ke repo ikut
//! berbagi pelajaran, keputusan, dan fakta proyek.
//!
//! SurrealDB di vault tiap anggota tetap menjadi penyimpanan & mesin query. Folder
//! `.engram/` di tree repo hanya jalur kirim-terima berisi memori terdistilasi, tanpa
//! embedding dan tanpa episode mentah:
//!
//! ```text
//! <repo>/.engram/
//!   config.toml               apa yang dibagikan; adanya berkas ini = repo ikut berbagi
//!   manifest.json             format, versi, nama proyek
//!   README.md                 ringkasan untuk manusia (dibuat ulang tiap ekspor)
//!   .gitattributes            README tanpa diff
//!   members/<nama>-<hash>/    satu folder per anggota per perangkat
//!     member.json             nama penulis, perangkat, hash isi
//!     lessons/<id>.json       satu berkas JSON per record
//!     decisions/<id>.json
//!     facts/<id>.json
//!     episodes/<id>.json      judul + ringkasan episode proyek
//! ```
//!
//! Tiap perangkat hanya menulis foldernya sendiri, jadi `git merge` antar anggota tidak
//! pernah konflik. Impor membaca folder anggota lain dan menggabungkan berdasarkan kunci
//! stabil; tidak pernah menghapus. Record yang ditarik penulisnya ditulis ulang sebagai
//! tombstone (`"status": "withdrawn"`); penerima mengarsipkan pelajarannya, mengkarantina
//! episodenya, dan menutup faktanya dengan `valid_to`.
//!
//! Status lokal (repo yang dikenal, hash terakhir, peta record hasil impor) ada di
//! `<vault>/repo_share.json` dan tidak ikut dibagikan. engram tidak pernah `git commit`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use memory_core::ids::{new_id, now_rfc3339};
use memory_core::model::{EpisodeStatus, Job, Lesson, LessonKind};
use memory_core::project::{detect_project, same_project};
use memory_core::text::normalize;
use memory_extract::ingest::DistilledDoc;
use memory_extract::{Vault, det_id};
use memory_store::EpisodeFilter;

use crate::{JobError, Result, Runtime};

pub const BUNDLE_DIR: &str = ".engram";
pub const FORMAT: &str = "engram-repo-bundle";
pub const FORMAT_VERSION: u32 = 2;
pub const CONFIG_FILE: &str = "config.toml";
/// Konsep penanda episode hasil impor bundel: tidak diekspor ulang ke repo.
pub const SHARED_CONCEPT: &str = "repo-share";
pub const REPO_IMPORT: &str = "repo_import";
pub const REPO_EXPORT: &str = "repo_export";

pub(crate) const MEMBERS_DIR: &str = "members";
pub(crate) const MEMBER_FILE: &str = "member.json";
pub(crate) const MANIFEST: &str = "manifest.json";
const STATE_FILE: &str = "repo_share.json";
const KINDS: [&str; 4] = ["lessons", "decisions", "facts", "episodes"];
const MAX_RECORD_BYTES: u64 = 64 * 1024;
const MAX_FILES_PER_MEMBER: usize = 20_000;
const HOOK_START: &str = "# >>> engram repo share >>>";
const HOOK_END: &str = "# <<< engram repo share <<<";
const HOOK_NAMES: [&str; 3] = ["post-merge", "post-checkout", "pre-commit"];

const CONFIG_TEMPLATE: &str = r#"# Memori proyek bersama (engram). Berkas ini ikut di-commit: adanya berkas ini berarti
# setiap anggota tim yang memakai engram otomatis berbagi memori proyek lewat repo ini.
# Yang dibagikan hanya memori terdistilasi tentang proyek (tanpa episode mentah,
# transkrip, atau fakta pribadi); teks yang memuat rahasia/blok <private> dilewati.

# Jenis memori yang dibagikan: "lessons" (konvensi & jebakan), "decisions", "facts".
share = ["lessons", "decisions", "facts"]

# Batas record per jenis per anggota.
max_records = 2000

# Record yang memuat salah satu frasa ini (tanpa beda huruf besar-kecil) tidak diekspor.
exclude = []
"#;

// ---------------------------------------------------------------- tipe publik

#[derive(Debug, Clone, Default, Serialize)]
pub struct BundleReport {
    pub dir: PathBuf,
    pub project: String,
    /// Folder anggota milik perangkat ini.
    pub member: String,
    pub lessons: u32,
    pub decisions: u32,
    pub facts: u32,
    /// Ringkasan episode proyek (tanpa teks mentah).
    pub episodes: u32,
    /// Hanya saat impor.
    pub lessons_new: u32,
    pub lessons_merged: u32,
    pub decisions_new: u32,
    pub decisions_duplicate: u32,
    pub facts_new: u32,
    pub episodes_new: u32,
    /// Record yang ditarik penulisnya (impor) atau ditandai ditarik (ekspor).
    pub withdrawn: u32,
    /// Anggota yang datanya digabung (impor).
    pub members: Vec<String>,
    /// Ekspor: jumlah berkas yang ditulis; `unchanged` = isi sama dengan sebelumnya.
    pub written: u32,
    pub unchanged: bool,
    pub dry_run: bool,
    /// Ekspor `--dry-run`: daftar record yang akan ditulis.
    pub preview: Vec<String>,
    pub warnings: Vec<String>,
}

/// Apa yang dibagikan (`.engram/config.toml`, ditulis `engram repo init`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BundleConfig {
    pub share: Vec<String>,
    pub max_records: usize,
    pub exclude: Vec<String>,
}

impl Default for BundleConfig {
    fn default() -> Self {
        Self { share: KINDS.iter().map(|k| k.to_string()).collect(), max_records: 2000, exclude: vec![] }
    }
}

impl BundleConfig {
    pub(crate) fn shares(&self, kind: &str) -> bool {
        self.share.iter().any(|s| s.trim().eq_ignore_ascii_case(kind))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MemberInfo {
    pub format: String,
    pub version: u32,
    pub id: String,
    pub name: String,
    pub device: String,
    pub content_hash: String,
    pub counts: BTreeMap<String, u64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemberSummary {
    pub id: String,
    pub name: String,
    pub device: String,
    pub own: bool,
    pub counts: BTreeMap<String, u64>,
    pub updated_at: String,
}

/// Status berbagi satu repo untuk UI & CLI.
#[derive(Debug, Clone, Serialize)]
pub struct ShareStatus {
    pub root: PathBuf,
    pub project: String,
    /// `.engram/config.toml` ada: repo ikut berbagi (ekspor otomatis).
    pub active: bool,
    /// `.engram/manifest.json` ada.
    pub bundle: bool,
    pub member: String,
    pub members: Vec<MemberSummary>,
    pub last_import_at: Option<String>,
    pub last_export_at: Option<String>,
    pub last_error: Option<String>,
    /// Record rekan tim yang sudah digabung ke vault ini.
    pub imported: usize,
    /// Git hook engram yang terpasang.
    pub hooks: Vec<String>,
    /// Ada perubahan di `.engram/` yang belum digabung.
    pub pending_import: bool,
    pub enabled: bool,
    pub auto_import: bool,
    pub auto_export: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct HooksReport {
    pub installed: Vec<String>,
    pub removed: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InitReport {
    pub root: PathBuf,
    pub project: String,
    pub config_created: bool,
    pub export: BundleReport,
    pub import: Option<BundleReport>,
    pub hooks: Option<HooksReport>,
}

// ---------------------------------------------------------------- status lokal

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ShareState {
    pub(crate) device_id: String,
    pub(crate) machine: String,
    /// Kunci: path absolut akar repo.
    pub(crate) repos: BTreeMap<String, RepoState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct RepoState {
    pub(crate) project: String,
    pub(crate) subject: String,
    /// Sidik `.engram/` (path, ukuran, waktu ubah) saat impor terakhir.
    pub(crate) fingerprint: Option<String>,
    /// Hash isi folder anggota lain yang sudah digabung.
    pub(crate) pulled: BTreeMap<String, String>,
    /// `<penulis>/<jenis>/<id record>` -> ID lokal yang dibuat impor.
    pub(crate) imported: BTreeMap<String, String>,
    /// Record rekan yang menguatkan pelajaran milik sendiri (dihitung sekali).
    pub(crate) confirmed: BTreeSet<String>,
    pub(crate) last_import_at: Option<String>,
    pub(crate) last_export_at: Option<String>,
    pub(crate) last_error: Option<String>,
    /// Sinkronisasi lewat hub tim (`.engram/repo.toml`).
    pub(crate) hub: Option<crate::repo_hub::HubRepoState>,
}

/// Satu penulisan berkas status pada satu waktu.
static STATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Satu impor/ekspor pada satu waktu (worker paralel, hook, UI).
pub(crate) static RUN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn state_path(v: &Vault) -> PathBuf {
    v.root().join(STATE_FILE)
}

pub(crate) fn with_state<R>(v: &Vault, f: impl FnOnce(&mut ShareState) -> R) -> Result<R> {
    let _g = STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = state_path(v);
    let raw = std::fs::read(&path).ok();
    let mut st: ShareState = raw.as_deref().and_then(|b| serde_json::from_slice(b).ok()).unwrap_or_default();
    let machine = crate::sync::machine_id();
    if st.device_id.is_empty() || st.machine != machine {
        // Vault baru atau disalin ke komputer lain: perangkat ini menulis folder anggota sendiri.
        st.device_id = new_id();
        st.machine = machine;
    }
    let r = f(&mut st);
    let out = serde_json::to_vec_pretty(&st).map_err(|e| JobError::Invalid(e.to_string()))?;
    if raw.as_deref() != Some(out.as_slice()) {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &out)?;
        std::fs::rename(&tmp, &path)?;
    }
    Ok(r)
}

pub(crate) fn key_of(root: &Path) -> String {
    root.to_string_lossy().to_string()
}

fn short_hash(s: &str) -> String {
    blake3::hash(s.as_bytes()).to_hex()[..16].to_string()
}

// ---------------------------------------------------------------- repo & identitas

pub(crate) struct Target {
    pub(crate) root: PathBuf,
    pub(crate) dir: PathBuf,
    pub(crate) project: String,
}

/// Akar repo, folder bundel, dan nama proyek. Nama lokal (akar git) diutamakan agar memori
/// impor menyatu dengan sesi kerja di komputer ini; `manifest.json` hanya cadangan.
pub(crate) fn resolve_target(repo: &Path, project: Option<&str>) -> Result<Target> {
    let repo = std::path::absolute(repo).unwrap_or_else(|_| repo.to_path_buf());
    let base = if repo.file_name().is_some_and(|n| n == BUNDLE_DIR) { repo.parent().map(Path::to_path_buf).unwrap_or_else(|| repo.clone()) } else { repo.clone() };
    let detected = detect_project(&base).filter(|p| p.from_git);
    let root = detected.as_ref().map(|p| p.root.clone()).unwrap_or(base);
    let dir = root.join(BUNDLE_DIR);
    let project = project
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .or_else(|| detected.map(|p| p.name))
        .or_else(|| read_manifest(&dir).ok().and_then(|m| m["project"].as_str().map(str::to_string)))
        .or_else(|| detect_project(&root).map(|p| p.name))
        .ok_or_else(|| JobError::Invalid("nama proyek tidak bisa ditentukan; beri --project".into()))?;
    Ok(Target { root, dir, project })
}

fn read_manifest(dir: &Path) -> Result<Value> {
    let v: Value = serde_json::from_slice(&std::fs::read(dir.join(MANIFEST))?).map_err(|e| JobError::Invalid(format!("manifest tidak terbaca: {e}")))?;
    if v["format"] != FORMAT {
        return Err(JobError::Invalid("bukan bundel engram".into()));
    }
    Ok(v)
}

pub fn load_bundle_config(dir: &Path) -> BundleConfig {
    std::fs::read_to_string(dir.join(CONFIG_FILE)).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
}

/// Folder git (common dir untuk worktree) dari akar repo.
pub(crate) fn git_dir_of(root: &Path) -> Option<PathBuf> {
    let git = root.join(".git");
    if git.is_dir() {
        return Some(git);
    }
    let s = std::fs::read_to_string(&git).ok()?;
    let line = s.lines().find_map(|l| l.trim().strip_prefix("gitdir:"))?.trim();
    let gd = if Path::new(line).is_relative() { root.join(line) } else { PathBuf::from(line) };
    let common = std::fs::read_to_string(gd.join("commondir")).ok().map(|c| {
        let p = PathBuf::from(c.trim());
        if p.is_relative() { gd.join(p) } else { p }
    });
    Some(common.unwrap_or(gd))
}

/// Nilai terakhir `key` di bagian `[section]` (tanpa subsection) sebuah berkas config git.
fn git_config_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut cur = false;
    let mut found = None;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            let inner = l.trim_start_matches('[').trim_end_matches(']').trim();
            cur = inner.eq_ignore_ascii_case(section);
            continue;
        }
        if !cur || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=')
            && k.trim().eq_ignore_ascii_case(key)
        {
            let v = v.trim().trim_matches('"').trim().to_string();
            if !v.is_empty() {
                found = Some(v);
            }
        }
    }
    found
}

/// Berkas config git yang berlaku untuk repo, dari global ke lokal (yang terakhir menang).
fn git_config_texts(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    if let Some(h) = std::env::var_os("HOME").map(PathBuf::from) {
        files.push(h.join(".config").join("git").join("config"));
        files.push(h.join(".gitconfig"));
    }
    if let Some(g) = git_dir_of(root) {
        files.push(g.join("config"));
    }
    files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect()
}

fn git_setting(root: &Path, section: &str, key: &str) -> Option<String> {
    git_config_texts(root).iter().rev().find_map(|t| git_config_value(t, section, key))
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 24 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "anggota".into() } else { out }
}

fn valid_member_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub(crate) fn record_id_of(file_name: &str) -> Option<&str> {
    let id = file_name.strip_suffix(".json")?;
    (!id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric())).then_some(id)
}

struct Me {
    id: String,
    name: String,
    device: String,
}

pub(crate) fn device_name() -> String {
    static N: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    N.get_or_init(crate::sync::host_name).clone()
}

/// Identitas anggota: nama dari `git config user.name` (cadangan: nama subjek), folder
/// `<nama>-<hash(email, perangkat)>` sehingga tiap perangkat menulis foldernya sendiri.
/// Email tidak ditulis ke repo.
async fn member_identity(rt: &Runtime, subject: &str, root: &Path, device_id: &str) -> Me {
    let name = git_setting(root, "user", "name");
    let email = git_setting(root, "user", "email");
    let name = match name {
        Some(n) => n,
        None => rt.store.get_subject(subject).await.ok().flatten().map(|s| s.name).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| subject.to_string()),
    };
    let basis = email.map(|e| e.trim().to_lowercase()).unwrap_or_else(|| name.to_lowercase());
    let h = blake3::hash(format!("{basis}\u{0}{device_id}").as_bytes()).to_hex()[..8].to_string();
    Me { id: format!("{}-{h}", slug(&name)), name, device: device_name() }
}

// ---------------------------------------------------------------- baca bundel

pub(crate) struct MemberDir {
    pub(crate) id: String,
    pub(crate) info: MemberInfo,
    /// `<jenis>/<id>.json` -> isi.
    pub(crate) records: BTreeMap<String, Value>,
    raw: BTreeMap<String, Vec<u8>>,
    hash: String,
}

fn hash_files(files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut h = blake3::Hasher::new();
    for (rel, b) in files {
        h.update(rel.as_bytes());
        h.update(b"\0");
        h.update(b);
        h.update(b"\n");
    }
    h.finalize().to_hex()[..32].to_string()
}

pub(crate) fn read_member(path: &Path, id: &str) -> MemberDir {
    let info: MemberInfo = std::fs::read(path.join(MEMBER_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut records = BTreeMap::new();
    let mut raw = BTreeMap::new();
    'kinds: for kind in KINDS {
        let Ok(rd) = std::fs::read_dir(path.join(kind)) else { continue };
        let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort();
        for name in names {
            if raw.len() >= MAX_FILES_PER_MEMBER {
                break 'kinds;
            }
            let Some(rid) = record_id_of(&name) else { continue };
            let p = path.join(kind).join(&name);
            // Hanya berkas biasa berukuran wajar (symlink diabaikan).
            if !std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_file() && m.len() <= MAX_RECORD_BYTES) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&p) else { continue };
            let Ok(v) = serde_json::from_slice::<Value>(&bytes) else { continue };
            if !v.is_object() {
                continue;
            }
            let rel = format!("{kind}/{rid}.json");
            records.insert(rel.clone(), v);
            raw.insert(rel, bytes);
        }
    }
    let hash = hash_files(&raw);
    MemberDir { id: id.to_string(), info, records, raw, hash }
}

pub(crate) fn list_member_ids(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir.join(MEMBERS_DIR)) else { return vec![] };
    let mut out: Vec<String> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| valid_member_id(n))
        .collect();
    out.sort();
    out
}

/// Sidik murah folder bundel (path, ukuran, waktu ubah) untuk deteksi perubahan di hook.
fn fingerprint(dir: &Path) -> Option<String> {
    fn walk(base: &Path, cur: &Path, depth: usize, out: &mut Vec<(String, u64, u128)>) {
        let Ok(rd) = std::fs::read_dir(cur) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(m) = std::fs::symlink_metadata(&p) else { continue };
            if m.is_dir() && depth < 4 {
                walk(base, &p, depth + 1, out);
            } else if m.is_file() {
                let rel = p.strip_prefix(base).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                let mtime = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
                out.push((rel, m.len(), mtime));
            }
        }
    }
    if !dir.join(MANIFEST).is_file() {
        return None;
    }
    let mut files = Vec::new();
    walk(dir, dir, 0, &mut files);
    files.sort();
    let mut h = blake3::Hasher::new();
    for (rel, len, mtime) in files {
        h.update(rel.as_bytes());
        h.update(&len.to_le_bytes());
        h.update(&mtime.to_le_bytes());
    }
    Some(h.finalize().to_hex()[..32].to_string())
}

// ---------------------------------------------------------------- ekspor

pub(crate) struct Record {
    pub(crate) kind: &'static str,
    pub(crate) id: String,
    pub(crate) body: Value,
}

pub(crate) fn canonical(v: &Value) -> Vec<u8> {
    let mut b = serde_json::to_vec_pretty(v).unwrap_or_default();
    b.push(b'\n');
    b
}

/// Penyaring teks sebelum keluar dari vault: rahasia/blok privat -> record dilewati,
/// frasa `exclude` -> dilewati, path absolut repo/home dipendekkan.
struct Cleaner<'a> {
    privacy: &'a memory_core::config::PrivacyConfig,
    exclude: Vec<String>,
    root: String,
    home: Option<String>,
}

impl Cleaner<'_> {
    fn clean(&self, s: &str) -> Option<String> {
        let t = s.trim();
        if t.is_empty() {
            return Some(String::new());
        }
        let c = memory_core::privacy::sanitize(t, self.privacy)?;
        if c.redacted > 0 || c.stripped_private || memory_core::privacy::has_redaction_marker(&c.text) {
            return None;
        }
        let low = c.text.to_lowercase();
        if self.exclude.iter().any(|x| low.contains(x.as_str())) {
            return None;
        }
        let mut out = c.text.trim().to_string();
        if !self.root.is_empty() {
            out = out.replace(&format!("{}/", self.root), "").replace(&self.root, ".");
        }
        if let Some(h) = &self.home {
            out = out.replace(h.as_str(), "~");
        }
        Some(out)
    }
}

/// ID lokal pelajaran yang berasal dari impor (untuk proyek ini): tidak diekspor ulang.
pub(crate) fn imported_lessons(st: &ShareState, project: &str) -> BTreeSet<String> {
    st.repos
        .values()
        .filter(|r| same_project(&r.project, project))
        .flat_map(|r| r.imported.iter().filter(|(k, _)| k.contains("/lessons/")).map(|(_, v)| v.clone()))
        .collect()
}

pub(crate) async fn collect(rt: &Runtime, subject: &str, t: &Target, names: &[String], cfg: &BundleConfig, skip_lessons: &BTreeSet<String>) -> Result<Vec<Record>> {
    let cl = Cleaner {
        privacy: &rt.cfg.privacy,
        exclude: cfg.exclude.iter().map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect(),
        root: t.root.to_string_lossy().to_string(),
        home: std::env::var("HOME").ok().filter(|h| h.len() > 1),
    };
    let cap = cfg.max_records.max(1);
    let mut out: Vec<Record> = Vec::new();
    // ID record diturunkan dari ISI (kunci judul, situasi+aksi, entitas+predikat+objek), bukan
    // dari ID lokal, sehingga supersede fakta atau kompaksi episode tidak melahirkan record baru
    // + tombstone. Dua sumber dengan isi sama menghasilkan satu record (yang terbaru menang).
    let mut seen: BTreeSet<String> = BTreeSet::new();

    if cfg.shares("lessons") {
        let mut n = 0;
        for l in rt.store.list_lessons(subject, names, None, 5000).await? {
            if n >= cap || skip_lessons.contains(&l.id) {
                continue;
            }
            let (Some(title), Some(detail)) = (cl.clean(&l.title), cl.clean(&l.detail)) else { continue };
            if title.is_empty() {
                continue;
            }
            let key = if l.title_key.is_empty() { Lesson::title_key_of(&l.title) } else { l.title_key.clone() };
            let id = det_id(&["repo-lesson", &key]);
            if !seen.insert(id.clone()) {
                continue;
            }
            let status = if l.status == "archived" { "archived" } else { "active" };
            out.push(Record {
                kind: "lessons",
                id,
                body: json!({"kind": l.kind, "title": title, "title_key": key, "detail": detail, "occurrences": l.occurrences, "status": status}),
            });
            n += 1;
        }
    }

    let eps = if cfg.shares("decisions") || cfg.shares("episodes") {
        rt.store
            .list_episodes(subject, &EpisodeFilter { status: Some(EpisodeStatus::Active), projects: names.to_vec(), newest_first: true, limit: 5000, ..Default::default() })
            .await?
            .into_iter()
            // Episode hasil impor memori bersama tidak dipantulkan balik.
            .filter(|e| !e.concepts.iter().any(|c| c == SHARED_CONCEPT))
            .collect::<Vec<_>>()
    } else {
        vec![]
    };
    let date_of = |e: &memory_core::model::Episode| -> String { e.time_start.clone().unwrap_or_else(|| e.created_at.clone()).chars().take(10).collect() };

    if cfg.shares("decisions") {
        let mut n = 0;
        'eps: for e in &eps {
            for d in &e.decisions {
                if n >= cap {
                    break 'eps;
                }
                let (Some(sit), Some(act), Some(title)) = (cl.clean(&d.situation), cl.clean(&d.action), cl.clean(&e.title)) else { continue };
                let reason = match d.reason.as_deref() {
                    Some(r) => match cl.clean(r) {
                        Some(r) => r,
                        None => continue,
                    },
                    None => String::new(),
                };
                if sit.is_empty() || act.is_empty() {
                    continue;
                }
                let id = det_id(&["repo-decision", &normalize(&sit), &normalize(&act)]);
                if !seen.insert(id.clone()) {
                    continue;
                }
                out.push(Record {
                    kind: "decisions",
                    id,
                    body: json!({"date": date_of(e), "title": title, "obs_type": e.obs_type, "situation": sit, "action": act, "reason": reason, "status": "active"}),
                });
                n += 1;
            }
        }
    }

    if cfg.shares("episodes") {
        let mut n = 0;
        // Episode yang memuat keputusan sudah dibagikan lewat jenis `decisions`; jangan dobel.
        for e in eps.iter().filter(|e| e.decisions.is_empty() || !cfg.shares("decisions")) {
            if n >= cap {
                break;
            }
            let (Some(title), Some(summary)) = (cl.clean(&e.title), cl.clean(&e.summary)) else { continue };
            if title.is_empty() || summary.is_empty() {
                continue;
            }
            let id = det_id(&["repo-episode", &e.id]);
            if !seen.insert(id.clone()) {
                continue;
            }
            let concepts: Vec<String> = e.concepts.iter().filter_map(|c| cl.clean(c)).filter(|c| !c.is_empty()).take(12).collect();
            out.push(Record {
                kind: "episodes",
                id,
                body: json!({
                    "date": date_of(e), "title": memory_core::text::truncate_chars(&title, 160), "summary": memory_core::text::truncate_chars(&summary, 600),
                    "obs_type": e.obs_type, "concepts": concepts, "status": "active",
                }),
            });
            n += 1;
        }
    }

    if cfg.shares("facts")
        && let Some(ent) = rt.retriever.find_entity(subject, &t.project).await?
    {
        let facts: Vec<_> = rt
            .store
            .facts_for_entity(&ent.id)
            .await?
            .into_iter()
            .filter(|f| f.is_current() && !memory_retrieve::is_event_fact(&f.category) && f.confidence >= rt.cfg.pipeline.min_fact_confidence)
            .collect();
        // Fakta yang seluruh buktinya berasal dari impor bundel milik rekan, bukan milik sendiri.
        let ev_ids: Vec<String> = facts.iter().flat_map(|f| f.evidence.iter().map(|e| e.episode_id.clone())).collect::<BTreeSet<_>>().into_iter().collect();
        let shared: BTreeSet<String> = rt.store.episodes_by_ids(&ev_ids).await?.into_iter().filter(|e| e.concepts.iter().any(|c| c == SHARED_CONCEPT)).map(|e| e.id).collect();
        let mut n = 0;
        for f in &facts {
            if n >= cap {
                break;
            }
            if !f.evidence.is_empty() && f.evidence.iter().all(|e| shared.contains(&e.episode_id)) {
                continue;
            }
            let (Some(pred), Some(obj)) = (cl.clean(&f.predicate), cl.clean(&f.object_text)) else { continue };
            if pred.is_empty() || obj.is_empty() {
                continue;
            }
            let id = det_id(&["repo-fact", &normalize(&ent.name), &normalize(&pred), &normalize(&obj)]);
            if !seen.insert(id.clone()) {
                continue;
            }
            out.push(Record {
                kind: "facts",
                id,
                body: json!({
                    "entity": ent.name, "predicate": pred, "object": obj, "object_is_entity": f.object_entity.is_some(),
                    "category": f.category, "kind": f.kind, "exclusive": f.exclusive, "valid_from": f.valid_from,
                    "statement": format!("{} {pred} {obj}", ent.name), "confidence": (f.confidence * 100.0).round() / 100.0, "status": "active",
                }),
            });
            n += 1;
        }
    }
    Ok(out)
}

#[derive(Default)]
struct WriteOut {
    written: u32,
    withdrawn: u32,
    unchanged: bool,
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> std::io::Result<bool> {
    if std::fs::read(path).ok().as_deref() == Some(bytes) {
        return Ok(false);
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, bytes)?;
    Ok(true)
}

fn write_bundle(dir: &Path, project: &str, me: &Me, records: Vec<Record>) -> Result<WriteOut> {
    let mdir = dir.join(MEMBERS_DIR).join(&me.id);
    let existing = read_member(&mdir, &me.id);
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut counts: BTreeMap<String, u64> = KINDS.iter().map(|k| (k.to_string(), 0)).collect();
    for r in &records {
        *counts.entry(r.kind.to_string()).or_default() += u64::from(r.body["status"] == "active");
        files.insert(format!("{}/{}.json", r.kind, r.id), canonical(&r.body));
    }
    // Record yang hilang sejak ekspor sebelumnya menjadi tombstone agar rekan tim ikut menariknya.
    let mut out = WriteOut::default();
    for (rel, v) in &existing.records {
        if files.contains_key(rel) {
            continue;
        }
        if v["status"] == "withdrawn" {
            files.insert(rel.clone(), existing.raw.get(rel).cloned().unwrap_or_else(|| canonical(v)));
        } else {
            files.insert(rel.clone(), canonical(&json!({"status": "withdrawn", "withdrawn_at": chrono::Utc::now().format("%Y-%m-%d").to_string()})));
            out.withdrawn += 1;
        }
    }
    let hash = hash_files(&files);
    if existing.info.content_hash == hash && mdir.join(MEMBER_FILE).is_file() && dir.join(MANIFEST).is_file() {
        out.unchanged = true;
        return Ok(out);
    }
    for (rel, bytes) in &files {
        out.written += u32::from(write_if_changed(&mdir.join(rel), bytes)?);
    }
    // Berkas tak dikenal di folder sendiri (mis. sisa format lama) dibersihkan.
    for kind in KINDS {
        let Ok(rd) = std::fs::read_dir(mdir.join(kind)) else { continue };
        for e in rd.flatten() {
            let rel = format!("{kind}/{}", e.file_name().to_string_lossy());
            if !files.contains_key(&rel) && e.file_type().is_ok_and(|t| t.is_file()) {
                std::fs::remove_file(e.path())?;
            }
        }
    }
    let info = MemberInfo { format: FORMAT.into(), version: FORMAT_VERSION, id: me.id.clone(), name: me.name.clone(), device: me.device.clone(), content_hash: hash, counts, updated_at: now_rfc3339() };
    write_if_changed(&mdir.join(MEMBER_FILE), &canonical(&serde_json::to_value(&info).unwrap_or_default()))?;
    if !dir.join(MANIFEST).is_file() {
        write_if_changed(&dir.join(MANIFEST), &canonical(&json!({"format": FORMAT, "version": FORMAT_VERSION, "project": project})))?;
    }
    if !dir.join(".gitattributes").is_file() {
        write_if_changed(&dir.join(".gitattributes"), b"README.md -diff linguist-generated=true\n")?;
    }
    write_if_changed(&dir.join("README.md"), render_readme(dir, project).as_bytes())?;
    Ok(out)
}

/// README gabungan semua anggota; deterministik (tanpa cap waktu) agar tidak memicu diff.
fn render_readme(dir: &Path, project: &str) -> String {
    let members: Vec<MemberDir> = list_member_ids(dir).iter().map(|id| read_member(&dir.join(MEMBERS_DIR).join(id), id)).collect();
    let mut lessons: BTreeMap<String, (String, String, BTreeSet<String>)> = BTreeMap::new();
    let mut decisions: Vec<(String, String, String, String)> = Vec::new();
    let mut facts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for m in &members {
        let who = if m.info.name.is_empty() { m.id.clone() } else { m.info.name.clone() };
        for (rel, v) in &m.records {
            if v["status"] != "active" {
                continue;
            }
            let s = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
            if rel.starts_with("lessons/") {
                let e = lessons.entry(s("title_key")).or_insert_with(|| (LessonKind::parse(&s("kind")).label_id().to_string(), s("title"), BTreeSet::new()));
                e.2.insert(who.clone());
            } else if rel.starts_with("decisions/") {
                decisions.push((s("date"), s("situation"), s("action"), who.clone()));
            } else if rel.starts_with("facts/") {
                facts.entry(s("statement")).or_default().insert(who.clone());
            }
        }
    }
    decisions.sort_by(|a, b| b.cmp(a));
    decisions.dedup();
    let by = |w: &BTreeSet<String>| w.iter().cloned().collect::<Vec<_>>().join(", ");
    let mut r = format!(
        "# Memori proyek {project} (engram)\n\nFolder ini ditulis engram dan dibagikan lewat git: siapa pun yang punya akses ke repo ini dan memakai engram otomatis menggabungkan memori di sini ke vault-nya.\nJangan diedit manual; tiap anggota hanya menulis foldernya sendiri di `members/`.\n\n- Aktifkan: `engram repo init` (sekali per repo, lalu commit `.engram/`)\n- Gabungkan sekarang: `engram repo import`\n- Lihat status: `engram repo status`\n\n## Anggota\n\n"
    );
    for m in &members {
        let c = |k: &str| m.info.counts.get(k).copied().unwrap_or(0);
        r.push_str(&format!("- {} ({}): {} pelajaran, {} keputusan, {} fakta\n", if m.info.name.is_empty() { &m.id } else { &m.info.name }, if m.info.device.is_empty() { "?" } else { &m.info.device }, c("lessons"), c("decisions"), c("facts")));
    }
    if !lessons.is_empty() {
        r.push_str("\n## Konvensi & jebakan\n\n");
        let mut ls: Vec<_> = lessons.values().collect();
        ls.sort_by(|a, b| (&a.0, a.1.to_lowercase()).cmp(&(&b.0, b.1.to_lowercase())));
        for (kind, title, who) in ls {
            r.push_str(&format!("- [{kind}] {title} ({})\n", by(who)));
        }
    }
    if !decisions.is_empty() {
        r.push_str("\n## Keputusan\n\n");
        for (date, sit, act, who) in decisions.iter().take(150) {
            r.push_str(&format!("- {}: {sit} -> {act} ({who})\n", if date.is_empty() { "?" } else { date }));
        }
    }
    if !facts.is_empty() {
        r.push_str("\n## Fakta\n\n");
        for (st, who) in facts.iter().take(300) {
            r.push_str(&format!("- {st} ({})\n", by(who)));
        }
    }
    r
}

/// Tulis `<repo>/.engram/members/<anggota ini>/` dari memori proyek.
pub async fn export_repo(rt: &Runtime, subject: &str, repo: &Path, project: Option<&str>) -> Result<BundleReport> {
    export_with(rt, subject, repo, project, false).await
}

pub async fn export_with(rt: &Runtime, subject: &str, repo: &Path, project: Option<&str>, dry_run: bool) -> Result<BundleReport> {
    let _g = RUN_LOCK.lock().await;
    let t = resolve_target(repo, project)?;
    let res = export_locked(rt, subject, &t, dry_run).await;
    note_error(rt, &t, &res);
    res
}

async fn export_locked(rt: &Runtime, subject: &str, t: &Target, dry_run: bool) -> Result<BundleReport> {
    let cfg = load_bundle_config(&t.dir);
    let names = rt.store.project_names(subject, &t.project).await?;
    let (device_id, skip) = with_state(&rt.vault, |st| (st.device_id.clone(), imported_lessons(st, &t.project)))?;
    let me = member_identity(rt, subject, &t.root, &device_id).await;
    let records = collect(rt, subject, t, &names, &cfg, &skip).await?;
    let count = |k: &str| records.iter().filter(|r| r.kind == k).count() as u32;
    let mut rep = BundleReport { dir: t.dir.clone(), project: t.project.clone(), member: me.id.clone(), lessons: count("lessons"), decisions: count("decisions"), facts: count("facts"), episodes: count("episodes"), dry_run, ..Default::default() };
    if dry_run {
        rep.preview = records
            .iter()
            .map(|r| {
                let text = match r.kind {
                    "lessons" | "episodes" => r.body["title"].as_str().unwrap_or("").to_string(),
                    "decisions" => format!("{} -> {}", r.body["situation"].as_str().unwrap_or(""), r.body["action"].as_str().unwrap_or("")),
                    _ => r.body["statement"].as_str().unwrap_or("").to_string(),
                };
                format!("{}: {text}", r.kind)
            })
            .collect();
        return Ok(rep);
    }
    let dir = t.dir.clone();
    let project = t.project.clone();
    let out = tokio::task::spawn_blocking(move || write_bundle(&dir, &project, &me, records)).await.map_err(|e| JobError::Invalid(format!("tugas ekspor gagal: {e}")))??;
    rep.written = out.written;
    rep.withdrawn = out.withdrawn;
    rep.unchanged = out.unchanged;
    let key = key_of(&t.root);
    with_state(&rt.vault, |st| {
        let r = st.repos.entry(key).or_default();
        r.project = t.project.clone();
        r.subject = subject.to_string();
        if !out.unchanged || r.last_export_at.is_none() {
            r.last_export_at = Some(now_rfc3339());
        }
        r.last_error = None;
    })?;
    Ok(rep)
}

fn note_error<T>(rt: &Runtime, t: &Target, res: &Result<T>) {
    if let Err(e) = res {
        let msg = e.to_string();
        let _ = with_state(&rt.vault, |st| {
            if let Some(r) = st.repos.get_mut(&key_of(&t.root)) {
                r.last_error = Some(msg);
            }
        });
    }
}

// ---------------------------------------------------------------- impor

/// Gabungkan bundel rekan tim di `<repo>/.engram/` ke vault ini (tidak menghapus apa pun).
/// Perintah manual selalu membaca ulang semua anggota.
pub async fn import_repo(rt: &Runtime, subject: &str, repo: &Path, project: Option<&str>) -> Result<BundleReport> {
    import_with(rt, subject, repo, project, true).await
}

pub async fn import_with(rt: &Runtime, subject: &str, repo: &Path, project: Option<&str>, force: bool) -> Result<BundleReport> {
    let _g = RUN_LOCK.lock().await;
    let t = resolve_target(repo, project)?;
    let res = import_locked(rt, subject, &t, force).await;
    note_error(rt, &t, &res);
    res
}

async fn import_locked(rt: &Runtime, subject: &str, t: &Target, force: bool) -> Result<BundleReport> {
    let manifest = read_manifest(&t.dir)?;
    if !t.dir.join(MEMBERS_DIR).is_dir() && t.dir.join("lessons.jsonl").is_file() {
        return import_v1(rt, subject, &t.dir, &t.project).await;
    }
    if manifest["version"].as_u64().unwrap_or(1) > u64::from(FORMAT_VERSION) {
        return Err(JobError::Invalid("bundel dibuat engram versi lebih baru; perbarui engram di komputer ini".into()));
    }
    rt.pipeline.ensure_subject(subject, None).await?;
    let key = key_of(&t.root);
    let (device_id, mut rs) = with_state(&rt.vault, |st| (st.device_id.clone(), st.repos.get(&key).cloned().unwrap_or_default()))?;
    let me = member_identity(rt, subject, &t.root, &device_id).await;
    let dir = t.dir.clone();
    let own = me.id.clone();
    let (members, fp) = tokio::task::spawn_blocking(move || {
        let fp = fingerprint(&dir);
        let ms: Vec<MemberDir> = list_member_ids(&dir).into_iter().filter(|i| *i != own).map(|i| read_member(&dir.join(MEMBERS_DIR).join(&i), &i)).collect();
        (ms, fp)
    })
    .await
    .map_err(|e| JobError::Invalid(format!("tugas impor gagal: {e}")))?;

    let mut rep = BundleReport { dir: t.dir.clone(), project: t.project.clone(), member: me.id.clone(), ..Default::default() };
    for m in &members {
        if m.info.version > FORMAT_VERSION {
            rep.warnings.push(format!("{}: dibuat engram versi lebih baru, dilewati", m.id));
            continue;
        }
        if !force && rs.pulled.get(&m.id) == Some(&m.hash) {
            continue;
        }
        match import_member(rt, subject, &t.project, m, &mut rs, &mut rep).await {
            Ok(()) => {
                rs.pulled.insert(m.id.clone(), m.hash.clone());
                rep.members.push(if m.info.name.is_empty() { m.id.clone() } else { format!("{} ({})", m.info.name, m.info.device) });
            }
            Err(e) => rep.warnings.push(format!("{}: {e}", m.id)),
        }
    }
    rs.project = t.project.clone();
    rs.subject = subject.to_string();
    rs.fingerprint = fp;
    if !rep.members.is_empty() || rs.last_import_at.is_none() {
        rs.last_import_at = Some(now_rfc3339());
    }
    rs.last_error = (!rep.warnings.is_empty()).then(|| rep.warnings.join("; "));
    with_state(&rt.vault, |st| {
        st.repos.insert(key, rs);
    })?;
    Ok(rep)
}

async fn import_member(rt: &Runtime, subject: &str, project: &str, m: &MemberDir, rs: &mut RepoState, rep: &mut BundleReport) -> Result<()> {
    let who = Author { key: m.id.clone(), name: m.info.name.clone(), device: m.info.device.clone() };
    for (rel, row) in &m.records {
        let Some((kind, file)) = rel.split_once('/') else { continue };
        let Some(rid) = record_id_of(file) else { continue };
        apply_record(rt, subject, project, &who, kind, rid, row, rs, rep).await?;
    }
    Ok(())
}

/// Penulis record bersama: folder anggota bundel legacy (`<nama>-<hash>`) atau pengguna hub
/// (`hub:<user_id>:<device_id>`). Kuncinya menjadi prefiks peta `RepoState.imported`.
pub(crate) struct Author {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) device: String,
}

impl Author {
    fn display(&self) -> String {
        if self.name.trim().is_empty() { self.key.clone() } else { self.name.trim().to_string() }
    }
    fn is_hub(&self) -> bool {
        self.key.starts_with("hub:")
    }
    /// Keterangan jalur di teks episode hasil impor.
    fn channel(&self) -> &'static str {
        if self.is_hub() { "lewat hub tim" } else { "lewat repositori git" }
    }
    fn via(&self) -> &'static str {
        if self.is_hub() { "hub" } else { "repo-bundle" }
    }
    fn origin(&self) -> String {
        if self.is_hub() { self.key.clone() } else { format!("{BUNDLE_DIR}/{MEMBERS_DIR}/{}", self.key) }
    }
}

/// Terapkan satu record rekan (bundel legacy atau hub) ke vault ini: pelajaran digabung per
/// kunci judul, keputusan/fakta/episode menjadi episode terdistilasi bertanda `repo-share`,
/// tombstone (`status: withdrawn`) mengarsipkan/mengkarantina/menutup record lokalnya.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn apply_record(rt: &Runtime, subject: &str, project: &str, who: &Author, kind: &str, rid: &str, row: &Value, rs: &mut RepoState, rep: &mut BundleReport) -> Result<()> {
    let now = now_rfc3339();
    let rkey = format!("{}/{kind}/{rid}", who.key);
    let s = |k: &str| row[k].as_str().map(str::trim).unwrap_or("").to_string();
    if s("status") == "withdrawn" {
        if let Some(local) = rs.imported.get(&rkey).cloned()
            && withdraw(rt, kind, &local).await?
        {
            rep.withdrawn += 1;
        }
        return Ok(());
    }
    match kind {
        "lessons" => {
            let title = s("title");
            if title.is_empty() {
                return Ok(());
            }
            rep.lessons += 1;
            let key = Some(s("title_key")).filter(|k| !k.is_empty()).unwrap_or_else(|| Lesson::title_key_of(&title));
            let archived = s("status") == "archived";
            let occ = row["occurrences"].as_u64().unwrap_or(1).clamp(1, 10_000) as u32;
            match rt.store.lesson_by_key(subject, Some(project), &key).await? {
                Some(mut local) => {
                    rep.lessons_merged += 1;
                    let before = local.clone();
                    if rs.imported.get(&rkey) == Some(&local.id) {
                        // Pelajaran milik rekan: ikuti status & hitungan penulisnya.
                        local.status = if archived { "archived".into() } else if local.status == "archived" { "active".into() } else { local.status.clone() };
                        local.occurrences = local.occurrences.max(occ);
                    } else if !archived && rs.confirmed.insert(rkey.clone()) {
                        // Rekan menemukan pelajaran yang sama: dikuatkan sekali.
                        local.occurrences += 1;
                    }
                    if local.detail.trim().is_empty() {
                        local.detail = s("detail");
                    }
                    if local != before {
                        local.updated_at = now.clone();
                        rt.store.put_lesson(&local).await?;
                    }
                }
                None if archived => {}
                None => {
                    let l = Lesson {
                        id: det_id(&["lesson", subject, project, &key]),
                        subject: subject.to_string(),
                        project: Some(project.to_string()),
                        kind: LessonKind::parse(&s("kind")),
                        title,
                        title_key: key,
                        detail: s("detail"),
                        occurrences: occ,
                        evidence: vec![],
                        status: "active".into(),
                        promoted_to: None,
                        created_at: now.clone(),
                        updated_at: now.clone(),
                    };
                    rt.store.put_lesson(&l).await?;
                    rs.imported.insert(rkey, l.id);
                    rep.lessons_new += 1;
                }
            }
        }
        "decisions" => {
            let (sit, act) = (s("situation"), s("action"));
            if sit.is_empty() || act.is_empty() {
                return Ok(());
            }
            rep.decisions += 1;
            if let Some(local) = rs.imported.get(&rkey) {
                reactivate(rt, local).await?;
                rep.decisions_duplicate += 1;
                return Ok(());
            }
            let date = Some(s("date")).filter(|d| !d.is_empty()).unwrap_or_else(|| "tanggal tidak diketahui".into());
            let reason = Some(s("reason")).filter(|r| !r.is_empty());
            let text = format!(
                "Keputusan proyek {project} ({date}), dibagikan {} {}.\nSituasi: {sit}\nKeputusan: {act}{}",
                who.display(),
                who.channel(),
                reason.as_deref().map(|r| format!("\nAlasan: {r}")).unwrap_or_default()
            );
            let title = Some(s("title")).filter(|x| !x.is_empty()).unwrap_or_else(|| memory_core::text::truncate_chars(&sit, 100));
            let extraction = json!({
                "obs_type": "decision",
                "concepts": [SHARED_CONCEPT],
                "decisions": [{"situation": sit, "action": act, "reason": reason}],
            });
            let (ep, dup) = ingest_shared(rt, subject, project, &title, &act, &text, who, rid, extraction).await?;
            rs.imported.insert(rkey, ep);
            if dup {
                rep.decisions_duplicate += 1;
            } else {
                rep.decisions_new += 1;
            }
        }
        "facts" => {
            let (statement, pred, obj) = (s("statement"), s("predicate"), s("object"));
            if statement.is_empty() || pred.is_empty() || obj.is_empty() {
                return Ok(());
            }
            rep.facts += 1;
            if let Some(local) = rs.imported.get(&rkey) {
                reactivate(rt, local).await?;
                return Ok(());
            }
            let entity = Some(s("entity")).filter(|e| !e.is_empty()).unwrap_or_else(|| project.to_string());
            let since = Some(s("valid_from")).filter(|v| !v.is_empty()).map(|v| format!(" (berlaku sejak {v})")).unwrap_or_default();
            let text = format!("Fakta proyek {project}, dibagikan {} {}: {statement}.{since}", who.display(), who.channel());
            let aliases: Vec<String> = if same_project(&entity, project) { vec![] } else { vec![project.to_string()] };
            let extraction = json!({
                "obs_type": "discovery",
                "concepts": [SHARED_CONCEPT],
                "entities": [{"name": entity, "kind": "project", "aliases": aliases}],
                "facts": [{
                    "subject": entity, "predicate": pred, "object": obj, "object_is_entity": row["object_is_entity"].as_bool().unwrap_or(false),
                    "category": s("category"), "kind": s("kind"), "exclusive": row["exclusive"].as_bool().unwrap_or(false),
                    "time": {"kind": "current"}, "quote": statement, "confidence": row["confidence"].as_f64().map(|c| c.clamp(0.0, 1.0)),
                }],
            });
            let (ep, dup) = ingest_shared(rt, subject, project, &memory_core::text::truncate_chars(&statement, 100), &statement, &text, who, rid, extraction).await?;
            rs.imported.insert(rkey, ep);
            if !dup {
                rep.facts_new += 1;
            }
        }
        "episodes" => {
            let (title, summary) = (s("title"), s("summary"));
            if title.is_empty() || summary.is_empty() {
                return Ok(());
            }
            rep.episodes += 1;
            if let Some(local) = rs.imported.get(&rkey) {
                reactivate(rt, local).await?;
                return Ok(());
            }
            let date = Some(s("date")).filter(|d| !d.is_empty()).unwrap_or_else(|| "tanggal tidak diketahui".into());
            let text = format!("Observasi proyek {project} ({date}), dibagikan {} {}.\n{title}\n{summary}", who.display(), who.channel());
            let mut concepts = vec![SHARED_CONCEPT.to_string()];
            concepts.extend(row["concepts"].as_array().into_iter().flatten().filter_map(|c| c.as_str()).map(str::trim).filter(|c| !c.is_empty() && *c != SHARED_CONCEPT).take(12).map(str::to_string));
            let obs_type = Some(s("obs_type")).filter(|o| !o.is_empty()).unwrap_or_else(|| "discovery".into());
            let extraction = json!({"obs_type": obs_type, "concepts": concepts});
            let (ep, dup) = ingest_shared(rt, subject, project, &title, &summary, &text, who, rid, extraction).await?;
            rs.imported.insert(rkey, ep);
            if !dup {
                rep.episodes_new += 1;
            }
        }
        _ => {}
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn ingest_shared(rt: &Runtime, subject: &str, project: &str, title: &str, summary: &str, text: &str, who: &Author, rid: &str, extraction: Value) -> Result<(String, bool)> {
    let out = rt
        .pipeline
        .ingest_distilled(DistilledDoc {
            subject: subject.to_string(),
            title: title.to_string(),
            summary: memory_core::text::truncate_chars(summary, 300),
            text: text.to_string(),
            project: Some(project.to_string()),
            original_name: Some(who.origin()),
            meta: json!({"via": who.via(), "member": who.key, "author": who.name, "device": who.device, "record": rid, "extraction": extraction}),
        })
        .await?;
    let ep = out.episode_id.unwrap_or_else(|| det_id(&[&out.source_id, "ep", "0", "0"]));
    Ok((ep, out.duplicate))
}

/// Tarik record yang dicabut penulisnya. `true` bila ada yang berubah.
async fn withdraw(rt: &Runtime, kind: &str, local: &str) -> Result<bool> {
    if kind == "lessons" {
        if let Some(mut l) = rt.store.get_lesson(local).await?
            && l.status == "active"
        {
            l.status = "archived".into();
            l.updated_at = now_rfc3339();
            rt.store.put_lesson(&l).await?;
            return Ok(true);
        }
        return Ok(false);
    }
    let mut changed = false;
    if let Some(e) = rt.store.get_episode(local).await?
        && e.status == EpisodeStatus::Active
    {
        rt.store.patch("episode", local, json!({"status": EpisodeStatus::Quarantined})).await?;
        changed = true;
    }
    // Fakta yang hanya bersandar pada episode impor ini ditutup (bitemporal, tidak dihapus).
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    for f in rt.store.facts_by_episode(local).await? {
        if f.is_current() && f.evidence.iter().all(|e| e.episode_id == local) {
            rt.store.patch("fact", &f.id, json!({"valid_to": today})).await?;
            changed = true;
        }
    }
    Ok(changed)
}

/// Record yang dipulihkan penulisnya setelah sempat ditarik.
async fn reactivate(rt: &Runtime, local: &str) -> Result<()> {
    if let Some(e) = rt.store.get_episode(local).await?
        && e.status == EpisodeStatus::Quarantined
    {
        rt.store.patch("episode", local, json!({"status": EpisodeStatus::Active})).await?;
    }
    Ok(())
}

/// Bundel format lama (v1: `lessons.jsonl`, `decisions.jsonl` di akar `.engram/`).
async fn import_v1(rt: &Runtime, subject: &str, dir: &Path, project: &str) -> Result<BundleReport> {
    fn read_jsonl(path: &Path) -> Vec<Value> {
        std::fs::read_to_string(path).map(|s| s.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()).unwrap_or_default()
    }
    let mut rep = BundleReport { dir: dir.to_path_buf(), project: project.to_string(), ..Default::default() };
    let now = now_rfc3339();
    for row in read_jsonl(&dir.join("lessons.jsonl")) {
        let Ok(mut l) = serde_json::from_value::<Lesson>(row) else { continue };
        rep.lessons += 1;
        l.subject = subject.to_string();
        l.project = Some(project.to_string());
        if l.title_key.is_empty() {
            l.title_key = Lesson::title_key_of(&l.title);
        }
        match rt.store.lesson_by_key(subject, Some(project), &l.title_key).await? {
            Some(mut local) => {
                if local.detail.is_empty() {
                    local.detail = l.detail.clone();
                }
                local.updated_at = now.clone();
                rt.store.put_lesson(&local).await?;
                rep.lessons_merged += 1;
            }
            None => {
                l.id = det_id(&["lesson", subject, project, &l.title_key]);
                l.promoted_to = None;
                l.evidence.clear();
                l.status = "active".into();
                l.updated_at = now.clone();
                rt.store.put_lesson(&l).await?;
                rep.lessons_new += 1;
            }
        }
    }
    for row in read_jsonl(&dir.join("decisions.jsonl")) {
        let (sit, act) = (row["situation"].as_str().unwrap_or("").trim(), row["action"].as_str().unwrap_or("").trim());
        if sit.is_empty() || act.is_empty() {
            continue;
        }
        rep.decisions += 1;
        let reason = row["reason"].as_str().map(str::trim).filter(|r| !r.is_empty());
        let text = format!("Keputusan proyek {project} ({}).\nSituasi: {sit}\nKeputusan: {act}{}", row["date"].as_str().unwrap_or("tanggal tidak diketahui"), reason.map(|r| format!("\nAlasan: {r}")).unwrap_or_default());
        let out = rt
            .pipeline
            .ingest_distilled(DistilledDoc {
                subject: subject.to_string(),
                title: memory_core::text::truncate_chars(sit, 100),
                summary: memory_core::text::truncate_chars(act, 300),
                text,
                project: Some(project.to_string()),
                original_name: Some("decisions.jsonl".into()),
                meta: json!({"via": "repo-bundle", "extraction": {"obs_type": "decision", "concepts": [SHARED_CONCEPT], "decisions": [{"situation": sit, "action": act, "reason": reason}]}}),
            })
            .await?;
        if out.duplicate {
            rep.decisions_duplicate += 1;
        } else {
            rep.decisions_new += 1;
        }
    }
    rep.facts = read_jsonl(&dir.join("facts.jsonl")).len() as u32;
    rep.warnings.push("bundel format lama (v1); jalankan `engram repo init` untuk beralih ke format per anggota".into());
    Ok(rep)
}

// ---------------------------------------------------------------- otomatisasi

pub(crate) fn repo_root_of(cwd: &Path) -> Option<(PathBuf, String)> {
    let p = detect_project(cwd)?;
    p.from_git.then_some((p.root, p.name))
}

/// Dipanggil saat sesi agent dimulai/berakhir dan dari git hook (`post-merge`,
/// `post-checkout`): mendaftarkan repo dan mengantre impor (bila `.engram/` berubah)
/// serta ekspor (bila repo sudah diaktifkan). Tidak pernah memproses langsung.
pub async fn on_repo_event(rt: &Runtime, subject: &str, cwd: &Path, project: Option<&str>) -> Result<Value> {
    on_repo_event_with(rt, subject, cwd, project, true).await
}

/// Seperti [`on_repo_event`] tanpa ekspor: dipakai saat memori tim sudah lewat hub, sehingga
/// bundel lama rekan yang belum migrasi masih terbaca tetapi folder `.engram/` tidak ditulis.
pub async fn on_repo_event_import_only(rt: &Runtime, subject: &str, cwd: &Path, project: Option<&str>) -> Result<Value> {
    on_repo_event_with(rt, subject, cwd, project, false).await
}

async fn on_repo_event_with(rt: &Runtime, subject: &str, cwd: &Path, project: Option<&str>, export: bool) -> Result<Value> {
    if !rt.cfg.repo_share.enabled {
        return Ok(json!({"enabled": false}));
    }
    let cwd = cwd.to_path_buf();
    let found = tokio::task::spawn_blocking(move || {
        repo_root_of(&cwd).map(|(root, name)| {
            let dir = root.join(BUNDLE_DIR);
            let fp = fingerprint(&dir);
            let active = dir.join(CONFIG_FILE).is_file();
            (root, name, fp, active)
        })
    })
    .await
    .map_err(|e| JobError::Invalid(format!("deteksi repo gagal: {e}")))?;
    let Some((root, name, fp, active)) = found else { return Ok(json!({"repo": null})) };
    let project = project.map(str::trim).filter(|p| !p.is_empty()).map(str::to_string).unwrap_or(name);
    // Repo tanpa `.engram/` tidak dicatat (hemat tulis berkas status) kecuali sudah dikenal.
    let key = key_of(&root);
    let prev = with_state(&rt.vault, |st| {
        if fp.is_none() && !st.repos.contains_key(&key) {
            return None;
        }
        let r = st.repos.entry(key.clone()).or_default();
        if r.project != project || r.subject != subject {
            r.project = project.clone();
            r.subject = subject.to_string();
        }
        Some(r.fingerprint.clone())
    })?;
    let Some(prev) = prev else { return Ok(json!({"repo": root, "bundle": false})) };
    queue_for(rt, subject, &root, &project, fp.as_deref(), prev.as_deref(), active && export).await
}

async fn queue_for(rt: &Runtime, subject: &str, root: &Path, project: &str, fp: Option<&str>, prev: Option<&str>, active: bool) -> Result<Value> {
    let sc = &rt.cfg.repo_share;
    let payload = json!({"subject": subject, "root": root, "project": project});
    let rk = short_hash(&key_of(root));
    let mut import = false;
    let mut export = false;
    if sc.auto_import
        && let Some(f) = fp
        && prev != Some(f)
    {
        let (_, created) = rt.store.enqueue(&Job::new(REPO_IMPORT, payload.clone(), format!("{REPO_IMPORT}:{rk}:{}", &f[..16.min(f.len())]), None)).await?;
        import = created;
    }
    if sc.auto_export && active {
        let bucket = chrono::Utc::now().timestamp() / 600;
        let (_, created) = rt.store.enqueue(&Job::new(REPO_EXPORT, payload, format!("{REPO_EXPORT}:{rk}:{bucket}"), None)).await?;
        export = created;
    }
    Ok(json!({"repo": root, "project": project, "bundle": fp.is_some(), "active": active, "import_queued": import, "export_queued": export}))
}

/// Ekspor langsung untuk git hook `pre-commit` (berkas harus sudah ada sebelum commit).
pub async fn export_for_commit(rt: &Runtime, subject: &str, cwd: &Path, project: Option<&str>) -> Result<Value> {
    if !rt.cfg.repo_share.enabled {
        return Ok(json!({"enabled": false}));
    }
    let Some((root, _)) = repo_root_of(cwd) else { return Ok(json!({"repo": null})) };
    if !root.join(BUNDLE_DIR).join(CONFIG_FILE).is_file() {
        return Ok(json!({"repo": root, "active": false}));
    }
    let r = export_with(rt, subject, &root, project, false).await?;
    Ok(json!({"repo": root, "active": true, "written": r.written, "unchanged": r.unchanged}))
}

/// Jalankan job `repo_import` / `repo_export` dari antrean.
pub async fn run_job(rt: &Runtime, kind: &str, payload: &Value) -> Result<Value> {
    let subject = payload["subject"].as_str().ok_or_else(|| JobError::Invalid("payload tanpa subject".into()))?;
    let root = PathBuf::from(payload["root"].as_str().ok_or_else(|| JobError::Invalid("payload tanpa root".into()))?);
    let project = payload["project"].as_str();
    if !root.is_dir() {
        return Ok(json!({"skipped": "repo tidak ditemukan"}));
    }
    match kind {
        REPO_IMPORT => {
            if !root.join(BUNDLE_DIR).join(MANIFEST).is_file() {
                return Ok(json!({"skipped": "tanpa bundel"}));
            }
            Ok(json!(import_with(rt, subject, &root, project, false).await?))
        }
        _ => {
            if !root.join(BUNDLE_DIR).join(CONFIG_FILE).is_file() {
                return Ok(json!({"skipped": "repo belum diaktifkan"}));
            }
            Ok(json!(export_with(rt, subject, &root, project, false).await?))
        }
    }
}

/// Pemeriksaan berkala semua repo yang dikenal (menangkap `git pull` di luar sesi agent).
pub async fn schedule_due(rt: &Runtime) -> Result<u32> {
    schedule_due_with(rt, true).await
}

/// Pemeriksaan berkala bundel lama tanpa ekspor (repo yang belum pindah ke hub tetap terbaca).
pub async fn schedule_due_import_only(rt: &Runtime) -> Result<u32> {
    schedule_due_with(rt, false).await
}

async fn schedule_due_with(rt: &Runtime, export: bool) -> Result<u32> {
    if !rt.cfg.repo_share.enabled {
        return Ok(0);
    }
    let repos = with_state(&rt.vault, |st| st.repos.clone())?;
    let mut n = 0;
    for (root, rs) in repos {
        let root = PathBuf::from(root);
        if rs.subject.is_empty() || !root.join(".git").exists() || root.join(BUNDLE_DIR).join(crate::repo_hub::REPO_TOML).is_file() {
            continue;
        }
        let dir = root.join(BUNDLE_DIR);
        let (fp, active) = tokio::task::spawn_blocking(move || (fingerprint(&dir), dir.join(CONFIG_FILE).is_file())).await.map_err(|e| JobError::Invalid(e.to_string()))?;
        let v = queue_for(rt, &rs.subject, &root, &rs.project, fp.as_deref(), rs.fingerprint.as_deref(), active && export).await?;
        n += u32::from(v["import_queued"] == true) + u32::from(v["export_queued"] == true);
    }
    Ok(n)
}

// ---------------------------------------------------------------- init, status, hook git

/// Aktifkan berbagi di repo: tulis `config.toml`, ekspor pertama, gabungkan rekan tim,
/// dan (opsional) pasang git hook lokal.
pub async fn init_repo(rt: &Runtime, subject: &str, repo: &Path, project: Option<&str>, hooks: bool) -> Result<InitReport> {
    let t = resolve_target(repo, project)?;
    if git_dir_of(&t.root).is_none() {
        return Err(JobError::Invalid(format!("{} bukan repositori git", t.root.display())));
    }
    std::fs::create_dir_all(&t.dir)?;
    let cfg_path = t.dir.join(CONFIG_FILE);
    let config_created = !cfg_path.exists();
    if config_created {
        std::fs::write(&cfg_path, CONFIG_TEMPLATE)?;
    }
    let (export, import) = {
        let _g = RUN_LOCK.lock().await;
        let ex = export_locked(rt, subject, &t, false).await;
        note_error(rt, &t, &ex);
        let ex = ex?;
        let im = if list_member_ids(&t.dir).iter().any(|m| *m != ex.member) { Some(import_locked(rt, subject, &t, false).await?) } else { None };
        (ex, im)
    };
    let hooks = if hooks { Some(git_hooks(&t.root, false)?) } else { None };
    Ok(InitReport { root: t.root, project: t.project, config_created, export, import, hooks })
}

pub async fn status(rt: &Runtime, subject: &str, repo: &Path, project: Option<&str>) -> Result<ShareStatus> {
    let t = resolve_target(repo, project)?;
    let key = key_of(&t.root);
    let (device_id, rs) = with_state(&rt.vault, |st| (st.device_id.clone(), st.repos.get(&key).cloned().unwrap_or_default()))?;
    let me = member_identity(rt, subject, &t.root, &device_id).await;
    let dir = t.dir.clone();
    let root = t.root.clone();
    let own = me.id.clone();
    let (members, fp, hooks) = tokio::task::spawn_blocking(move || {
        let ms: Vec<MemberSummary> = list_member_ids(&dir)
            .into_iter()
            .map(|id| {
                let info: MemberInfo = std::fs::read(dir.join(MEMBERS_DIR).join(&id).join(MEMBER_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
                MemberSummary { own: id == own, name: if info.name.is_empty() { id.clone() } else { info.name }, device: info.device, counts: info.counts, updated_at: info.updated_at, id }
            })
            .collect();
        (ms, fingerprint(&dir), installed_hooks(&root))
    })
    .await
    .map_err(|e| JobError::Invalid(e.to_string()))?;
    let sc = &rt.cfg.repo_share;
    Ok(ShareStatus {
        active: t.dir.join(CONFIG_FILE).is_file(),
        bundle: fp.is_some(),
        member: me.id,
        members,
        last_import_at: rs.last_import_at,
        last_export_at: rs.last_export_at,
        last_error: rs.last_error,
        imported: rs.imported.len(),
        hooks,
        pending_import: fp.is_some() && fp != rs.fingerprint,
        enabled: sc.enabled,
        auto_import: sc.auto_import,
        auto_export: sc.auto_export,
        root: t.root,
        project: t.project,
    })
}

/// Akar repo proyek `name`: dari repo yang sudah dikenal, selain itu dari folder kerja sesi.
pub async fn root_for_project(rt: &Runtime, subject: &str, name: &str) -> Result<Option<PathBuf>> {
    let repos = with_state(&rt.vault, |st| st.repos.clone())?;
    if let Some(root) = repos.iter().filter(|(_, r)| same_project(&r.project, name)).map(|(k, _)| PathBuf::from(k)).find(|p| p.is_dir()) {
        return Ok(Some(root));
    }
    let names = rt.store.project_names(subject, name).await?;
    let sessions = rt.store.list_sessions(subject, &names, 50).await?;
    Ok(sessions.iter().filter_map(|s| s.cwd.as_deref()).filter_map(|c| detect_project(Path::new(c))).find(|p| p.from_git && same_project(&p.name, name)).map(|p| p.root))
}

/// Akar repo yang sudah dikenal (path, nama proyek) untuk [`quick_share_state`].
pub fn known_roots(rt: &Runtime) -> Result<Vec<(PathBuf, String)>> {
    with_state(&rt.vault, |st| st.repos.iter().map(|(k, r)| (PathBuf::from(k), r.project.clone())).collect())
}

/// Keadaan `.engram/` proyek `name` tanpa query DB (untuk kartu beranda): akar repo dari
/// repo yang dikenal atau folder kerja sesi, lalu `active` (config.toml ada), `bundle`
/// (hanya bundel rekan), atau `none`. `None` bila akar repo git belum diketahui.
pub fn quick_share_state(known: &[(PathBuf, String)], name: &str, cwds: &[String]) -> Option<(PathBuf, &'static str)> {
    let root = known
        .iter()
        .filter(|(p, n)| same_project(n, name) && p.is_dir())
        .map(|(p, _)| p.clone())
        .next()
        .or_else(|| cwds.iter().filter_map(|c| detect_project(Path::new(c))).find(|p| p.from_git && same_project(&p.name, name)).map(|p| p.root))?;
    let dir = root.join(BUNDLE_DIR);
    let state = if dir.join(crate::repo_hub::REPO_TOML).is_file() {
        "hub"
    } else if dir.join(CONFIG_FILE).is_file() {
        "active"
    } else if dir.join(MANIFEST).is_file() {
        "bundle"
    } else {
        "none"
    };
    Some((root, state))
}

/// Path binary `engram` untuk git hook: `ENGRAM_BIN`, binary ini bila bernama engram,
/// atau `engram` di PATH / lokasi instalasi umum.
fn engram_bin() -> String {
    if let Some(b) = std::env::var_os("ENGRAM_BIN").filter(|b| !b.is_empty()) {
        return b.to_string_lossy().to_string();
    }
    if let Ok(exe) = std::env::current_exe()
        && exe.file_stem().is_some_and(|n| n == "engram")
    {
        return exe.to_string_lossy().to_string();
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].iter().map(PathBuf::from));
    if let Some(h) = &home {
        dirs.push(h.join(".local").join("bin"));
        dirs.push(h.join(".cargo").join("bin"));
    }
    dirs.into_iter().map(|d| d.join("engram")).find(|p| p.is_file()).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| "engram".into())
}

fn hook_block(name: &str, bin: &str) -> String {
    let bin = bin.replace('\'', "'\\''");
    let find = format!("_engram=\"${{ENGRAM_BIN:-$(command -v engram 2>/dev/null || echo '{bin}')}}\"");
    let run = match name {
        "pre-commit" => "if [ -x \"$_engram\" ] && \"$_engram\" hook git repo-export </dev/null >/dev/null 2>&1; then git add -- .engram >/dev/null 2>&1; fi",
        // Hanya saat pindah branch (argumen ketiga = 1), bukan checkout berkas.
        "post-checkout" => "if [ \"$3\" = \"1\" ] && [ -x \"$_engram\" ]; then \"$_engram\" hook git repo-sync </dev/null >/dev/null 2>&1; fi",
        _ => "if [ -x \"$_engram\" ]; then \"$_engram\" hook git repo-sync </dev/null >/dev/null 2>&1; fi",
    };
    format!("{HOOK_START}\n# Memori proyek bersama engram; cabut dengan `engram repo hooks --remove`.\n{find}\n{run}\ntrue\n{HOOK_END}\n")
}

fn strip_block(text: &str) -> String {
    let (Some(s), Some(e)) = (text.find(HOOK_START), text.find(HOOK_END)) else { return text.to_string() };
    if e < s {
        return text.to_string();
    }
    let end = (e + HOOK_END.len() + 1).min(text.len());
    format!("{}{}", &text[..s], &text[end..])
}

fn hooks_dir(root: &Path) -> std::result::Result<PathBuf, String> {
    if let Some(p) = git_setting(root, "core", "hookspath") {
        return Err(format!("core.hooksPath diatur ke {p} (mis. husky); tambahkan `engram hook git repo-sync` ke hook post-merge di sana secara manual"));
    }
    git_dir_of(root).map(|g| g.join("hooks")).ok_or_else(|| "folder .git tidak ditemukan".into())
}

pub(crate) fn installed_hooks(root: &Path) -> Vec<String> {
    let Ok(dir) = hooks_dir(root) else { return vec![] };
    HOOK_NAMES.iter().filter(|n| std::fs::read_to_string(dir.join(n)).is_ok_and(|t| t.contains(HOOK_START))).map(|n| n.to_string()).collect()
}

/// Pasang/cabut git hook lokal (`post-merge`, `post-checkout`, `pre-commit`). Hook milik
/// pengguna tidak ditimpa: blok engram disisipkan setelah baris shebang skrip shell.
pub fn git_hooks(root: &Path, remove: bool) -> Result<HooksReport> {
    let mut rep = HooksReport::default();
    let dir = match hooks_dir(root) {
        Ok(d) => d,
        Err(e) => {
            rep.skipped.push(e);
            return Ok(rep);
        }
    };
    std::fs::create_dir_all(&dir)?;
    let bin = engram_bin();
    for name in HOOK_NAMES {
        let path = dir.join(name);
        let cur = std::fs::read_to_string(&path).ok();
        if remove {
            let Some(text) = cur.filter(|t| t.contains(HOOK_START)) else { continue };
            let rest = strip_block(&text);
            if rest.lines().all(|l| l.trim().is_empty() || l.starts_with("#!")) {
                std::fs::remove_file(&path)?;
            } else {
                std::fs::write(&path, rest)?;
            }
            rep.removed.push(name.to_string());
            continue;
        }
        let block = hook_block(name, &bin);
        let next = match cur {
            None => format!("#!/bin/sh\n{block}"),
            Some(text) if text.contains(HOOK_START) => insert_after_shebang(&strip_block(&text), &block),
            Some(text) => {
                let first = text.lines().next().unwrap_or("");
                if !(first.starts_with("#!") && (first.ends_with("sh") || first.contains("/sh ") || first.contains("bash") || first.contains("zsh"))) {
                    rep.skipped.push(format!("{name}: hook yang ada bukan skrip shell; tambahkan `engram hook git repo-sync` secara manual"));
                    continue;
                }
                insert_after_shebang(&text, &block)
            }
        };
        std::fs::write(&path, next)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
        rep.installed.push(name.to_string());
    }
    Ok(rep)
}

fn insert_after_shebang(text: &str, block: &str) -> String {
    match text.split_once('\n') {
        Some((first, rest)) if first.starts_with("#!") => format!("{first}\n{block}{rest}"),
        _ if text.starts_with("#!") => format!("{text}\n{block}"),
        _ => format!("#!/bin/sh\n{block}{text}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use memory_core::MemoryConfig;
    use memory_core::model::Evidence;
    use memory_llm::HashEmbedder;
    use memory_llm::mock::ScriptedLlm;

    async fn rt(dir: &Path) -> Runtime {
        let mut cfg = MemoryConfig::for_tests(64);
        cfg.vault.path = dir.to_path_buf();
        Runtime::open_with(cfg, Arc::new(ScriptedLlm::new(|_| Ok("{}".into()))), Arc::new(HashEmbedder::new(64))).await.unwrap()
    }

    /// Klon repo tiruan: `.git/config` dengan identitas penulis sendiri.
    fn clone_repo(base: &Path, name: &str, user: &str) -> PathBuf {
        let r = base.join(name);
        std::fs::create_dir_all(r.join(".git")).unwrap();
        std::fs::write(r.join(".git/config"), format!("[user]\n\tname = {user}\n\temail = {}@example.com\n", user.to_lowercase())).unwrap();
        r
    }

    /// Tiruan `git push` + `git pull`: salin `.engram/` dari satu klon ke klon lain.
    fn sync_bundle(from: &Path, to: &Path) {
        fn copy(a: &Path, b: &Path) {
            std::fs::create_dir_all(b).unwrap();
            for e in std::fs::read_dir(a).unwrap().flatten() {
                let (src, dst) = (e.path(), b.join(e.file_name()));
                if src.is_dir() { copy(&src, &dst) } else { std::fs::copy(&src, &dst).unwrap(); }
            }
        }
        copy(&from.join(BUNDLE_DIR), &to.join(BUNDLE_DIR));
    }

    fn doc(project: &str, title: &str, text: &str, extraction: Value) -> DistilledDoc {
        DistilledDoc { subject: "budi".into(), title: title.into(), summary: String::new(), text: text.into(), project: Some(project.into()), original_name: None, meta: json!({"extraction": extraction}) }
    }

    #[tokio::test]
    async fn team_shares_project_memory_through_the_repo() {
        let base = tempfile::tempdir().unwrap();
        let (repo_a, repo_b) = (clone_repo(base.path(), "a", "Budi"), clone_repo(base.path(), "b", "Sari"));
        let (va, vb) = (base.path().join("vault-a"), base.path().join("vault-b"));
        let (a, b) = (rt(&va).await, rt(&vb).await);
        a.pipeline.ensure_subject("budi", Some("Budi")).await.unwrap();
        b.pipeline.ensure_subject("sari", Some("Sari")).await.unwrap();

        // Memori proyek Budi: pelajaran, keputusan, fakta; satu pelajaran memuat blok privat.
        let now = now_rfc3339();
        let lesson = |title: &str, detail: &str| Lesson {
            id: det_id(&["lesson", "budi", "api-a", &Lesson::title_key_of(title)]),
            subject: "budi".into(),
            project: Some("api-a".into()),
            kind: LessonKind::Pitfall,
            title: title.into(),
            title_key: Lesson::title_key_of(title),
            detail: detail.into(),
            occurrences: 2,
            evidence: vec![Evidence { episode_id: "ep-lokal".into(), quote: String::new() }],
            status: "active".into(),
            promoted_to: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        a.store.put_lesson(&lesson("Jalankan migrasi sebelum tes", "Tes integrasi butuh skema terbaru.")).await.unwrap();
        a.store.put_lesson(&lesson("Kunci server staging", "<private>kata sandi ada di catatan pribadi</private>")).await.unwrap();
        let dec = a
            .pipeline
            .ingest_distilled(doc(
                "api-a",
                "Pilih Postgres",
                "Kami memilih Postgres untuk api-a karena butuh transaksi.",
                json!({"obs_type": "decision", "decisions": [{"situation": "Butuh database transaksional untuk api-a", "action": "Pakai Postgres 16", "reason": "Transaksi dan row locking"}]}),
            ))
            .await
            .unwrap();
        let dec_ep = dec.episode_id.unwrap();
        let fact_text = "api-a memakai Redis untuk cache sesi.";
        a.pipeline
            .ingest_distilled(doc(
                "api-a",
                "Cache api-a",
                fact_text,
                json!({"entities": [{"name": "api-a", "kind": "project"}], "facts": [{"subject": "api-a", "predicate": "memakai", "object": "Redis untuk cache sesi", "category": "teknologi_cache", "time": {"kind": "current"}, "quote": "api-a memakai Redis untuk cache sesi", "confidence": 0.9}]}),
            ))
            .await
            .unwrap();
        let ep = a.store.get_episode(&dec_ep).await.unwrap().unwrap();
        assert_eq!((ep.obs_type.as_deref(), ep.decisions.len()), (Some("decision"), 1), "struktur meta diteruskan ke episode");

        // Ekspor Budi: blok privat tidak keluar; ekspor ulang tanpa perubahan tidak menulis apa pun.
        let ex = export_repo(&a, "budi", &repo_a, Some("api-a")).await.unwrap();
        assert_eq!((ex.lessons, ex.decisions, ex.facts), (1, 1, 1), "{ex:?}");
        assert!(ex.member.starts_with("budi-") && ex.written > 0);
        let member_dir = repo_a.join(BUNDLE_DIR).join(MEMBERS_DIR).join(&ex.member);
        assert!(member_dir.join(MEMBER_FILE).is_file() && member_dir.join("decisions").is_dir());
        let all = std::fs::read_to_string(repo_a.join(BUNDLE_DIR).join("README.md")).unwrap();
        assert!(all.contains("Jalankan migrasi sebelum tes") && all.contains("Pakai Postgres 16") && !all.contains("kata sandi"), "{all}");
        let again = export_repo(&a, "budi", &repo_a, Some("api-a")).await.unwrap();
        assert!(again.unchanged && again.written == 0, "{again:?}");

        // Sari menarik repo lalu menggabungkan: semua jenis memori masuk ke vault-nya.
        sync_bundle(&repo_a, &repo_b);
        let im = import_repo(&b, "sari", &repo_b, Some("api-a")).await.unwrap();
        assert_eq!((im.lessons_new, im.decisions_new, im.facts_new), (1, 1, 1), "{im:?}");
        assert_eq!(im.members.len(), 1);
        let lessons = b.store.list_lessons("sari", &["api-a".into()], None, 10).await.unwrap();
        assert_eq!(lessons.len(), 1);
        assert_eq!(lessons[0].kind, LessonKind::Pitfall);
        let eps = b.store.list_episodes("sari", &EpisodeFilter { projects: vec!["api-a".into()], limit: 10, ..Default::default() }).await.unwrap();
        let shared_dec = eps.iter().find(|e| e.obs_type.as_deref() == Some("decision")).expect("keputusan rekan jadi episode keputusan");
        assert!(shared_dec.decisions[0].action == "Pakai Postgres 16" && shared_dec.concepts.iter().any(|c| c == SHARED_CONCEPT));
        assert!(shared_dec.text.contains("dibagikan Budi"));
        let ent = b.retriever.find_entity("sari", "api-a").await.unwrap().expect("entitas proyek dibuat");
        let facts = b.store.facts_for_entity(&ent.id).await.unwrap();
        assert!(facts.iter().any(|f| f.is_current() && f.object_text == "Redis untuk cache sesi"), "{facts:?}");

        // Memori hasil impor tidak dipantulkan balik ke repo oleh Sari.
        let ex_b = export_repo(&b, "sari", &repo_b, Some("api-a")).await.unwrap();
        assert_eq!((ex_b.lessons, ex_b.decisions, ex_b.facts), (0, 0, 0), "{ex_b:?}");
        // Impor otomatis berikutnya melewati anggota yang isinya tidak berubah.
        let quiet = import_with(&b, "sari", &repo_b, Some("api-a"), false).await.unwrap();
        assert!(quiet.members.is_empty(), "{quiet:?}");

        // Budi mencabut keputusan & fakta: tombstone mengalir, Sari mengkarantina & menutup.
        a.store.patch("episode", &dec_ep, json!({"status": EpisodeStatus::Quarantined})).await.unwrap();
        let a_ent = a.retriever.find_entity("budi", "api-a").await.unwrap().unwrap();
        for f in a.store.facts_for_entity(&a_ent.id).await.unwrap() {
            a.store.patch("fact", &f.id, json!({"valid_to": "2026-11-01"})).await.unwrap();
        }
        let ex2 = export_repo(&a, "budi", &repo_a, Some("api-a")).await.unwrap();
        assert_eq!((ex2.decisions, ex2.facts, ex2.withdrawn), (0, 0, 2), "{ex2:?}");
        sync_bundle(&repo_a, &repo_b);
        let im2 = import_with(&b, "sari", &repo_b, Some("api-a"), false).await.unwrap();
        assert_eq!(im2.withdrawn, 2, "{im2:?}");
        let gone = b.store.get_episode(&shared_dec.id).await.unwrap().unwrap();
        assert_eq!(gone.status, EpisodeStatus::Quarantined);
        let facts = b.store.facts_for_entity(&ent.id).await.unwrap();
        assert!(facts.iter().all(|f| !f.is_current()), "fakta yang dicabut ditutup, tidak dihapus: {facts:?}");
        assert!(!facts.is_empty());
        // Tombstone stabil: ekspor berikutnya tidak menulis ulang.
        assert!(export_repo(&a, "budi", &repo_a, Some("api-a")).await.unwrap().unchanged);

        // Status & sidik: setelah impor tidak ada perubahan tertunda.
        let st = status(&b, "sari", &repo_b, Some("api-a")).await.unwrap();
        assert!(st.bundle && !st.active && !st.pending_import, "{st:?}");
        assert_eq!(st.members.iter().filter(|m| m.own).count(), 1);
        assert!(st.imported >= 3);
    }

    #[tokio::test]
    async fn init_activates_sharing_and_events_queue_jobs() {
        let base = tempfile::tempdir().unwrap();
        let repo = clone_repo(base.path(), "svc", "Budi");
        let r = rt(&base.path().join("vault")).await;
        r.pipeline.ensure_subject("budi", Some("Budi")).await.unwrap();
        assert!(init_repo(&r, "budi", &base.path().join("bukan-repo"), Some("svc"), false).await.is_err());
        let ini = init_repo(&r, "budi", &repo, Some("svc"), false).await.unwrap();
        assert!(ini.config_created && repo.join(BUNDLE_DIR).join(CONFIG_FILE).is_file());
        let cfg = load_bundle_config(&repo.join(BUNDLE_DIR));
        assert!(cfg.shares("lessons") && cfg.shares("facts") && cfg.max_records == 2000);
        // Dry run tidak menulis apa pun.
        let dry = export_with(&r, "budi", &repo, Some("svc"), true).await.unwrap();
        assert!(dry.dry_run && dry.written == 0);
        let jobs = schedule_due(&r).await.unwrap();
        assert!(jobs >= 1, "repo aktif yang dikenal diantre ekspor berkala");
        for j in r.store.list_jobs(None, 20).await.unwrap().iter().filter(|j| j.kind == REPO_EXPORT || j.kind == REPO_IMPORT) {
            run_job(&r, &j.kind, &j.payload).await.unwrap();
        }
    }

    #[tokio::test]
    async fn record_ids_follow_content_not_local_ids() {
        let base = tempfile::tempdir().unwrap();
        let repo = clone_repo(base.path(), "svc", "Budi");
        let r = rt(&base.path().join("vault")).await;
        r.pipeline.ensure_subject("budi", Some("Budi")).await.unwrap();
        // Keputusan yang sama dari dua episode berbeda -> satu record; ID-nya tidak bergantung ID episode.
        let dec = json!({"obs_type": "decision", "decisions": [{"situation": "Butuh antrean job", "action": "Pakai tabel job SurrealDB", "reason": null}]});
        let e1 = r.pipeline.ingest_distilled(doc("svc", "Antrean", "Kami memilih tabel job SurrealDB sebagai antrean.", dec.clone())).await.unwrap();
        let e2 = r.pipeline.ingest_distilled(doc("svc", "Antrean (ulang)", "Diskusi ulang: tetap pakai tabel job SurrealDB untuk antrean.", dec)).await.unwrap();
        assert_ne!(e1.episode_id, e2.episode_id);
        let fact_ep = r
            .pipeline
            .ingest_distilled(doc(
                "svc",
                "Cache",
                "svc memakai Redis untuk cache sesi.",
                json!({"entities": [{"name": "svc", "kind": "project"}], "facts": [{"subject": "svc", "predicate": "memakai", "object": "Redis untuk cache sesi", "category": "teknologi_cache", "time": {"kind": "current"}, "quote": "svc memakai Redis untuk cache sesi", "confidence": 0.9}]}),
            ))
            .await
            .unwrap()
            .episode_id
            .unwrap();
        std::fs::create_dir_all(repo.join(BUNDLE_DIR)).unwrap();
        std::fs::write(repo.join(BUNDLE_DIR).join(CONFIG_FILE), "share = [\"lessons\", \"decisions\", \"facts\", \"episodes\"]\n").unwrap();
        let ex = export_repo(&r, "budi", &repo, Some("svc")).await.unwrap();
        // Dua episode keputusan dibagikan sebagai satu keputusan; ringkasan episode hanya untuk episode lain.
        assert_eq!((ex.decisions, ex.facts, ex.episodes), (1, 1, 1), "{ex:?}");
        let mdir = repo.join(BUNDLE_DIR).join(MEMBERS_DIR).join(&ex.member);
        let dec_id = det_id(&["repo-decision", &normalize("Butuh antrean job"), &normalize("Pakai tabel job SurrealDB")]);
        assert!(mdir.join("decisions").join(format!("{dec_id}.json")).is_file());
        let fact_id = det_id(&["repo-fact", &normalize("svc"), &normalize("memakai"), &normalize("Redis untuk cache sesi")]);
        assert!(mdir.join("facts").join(format!("{fact_id}.json")).is_file());
        assert!(!mdir.join("episodes").join(format!("{}.json", det_id(&["repo-episode", e1.episode_id.as_deref().unwrap()]))).exists());
        let ep_file = mdir.join("episodes").join(format!("{}.json", det_id(&["repo-episode", &fact_ep])));
        let ep: Value = serde_json::from_slice(&std::fs::read(&ep_file).unwrap()).unwrap();
        assert_eq!((ep["title"].as_str(), ep["status"].as_str()), (Some("Cache"), Some("active")));
        assert!(ep["summary"].as_str().is_some_and(|s| !s.is_empty()) && ep.get("text").is_none(), "{ep}");

        // Keyakinan fakta berubah: isi record berubah tetapi ID tetap -> tidak ada tombstone.
        let ent = r.retriever.find_entity("budi", "svc").await.unwrap().unwrap();
        for f in r.store.facts_for_entity(&ent.id).await.unwrap() {
            r.store.patch("fact", &f.id, json!({"confidence": 0.75})).await.unwrap();
        }
        let ex2 = export_repo(&r, "budi", &repo, Some("svc")).await.unwrap();
        assert_eq!((ex2.facts, ex2.withdrawn, ex2.written), (1, 0, 1), "{ex2:?}");

        // Ringkasan episode rekan masuk ke vault lain sebagai episode bertanda repo-share.
        let repo_b = clone_repo(base.path(), "svc-b", "Sari");
        sync_bundle(&repo, &repo_b);
        let b = rt(&base.path().join("vault-b")).await;
        b.pipeline.ensure_subject("sari", Some("Sari")).await.unwrap();
        let im = import_repo(&b, "sari", &repo_b, Some("svc")).await.unwrap();
        assert_eq!((im.decisions_new, im.facts_new, im.episodes_new), (1, 1, 1), "{im:?}");
        let eps = b.store.list_episodes("sari", &EpisodeFilter { projects: vec!["svc".into()], limit: 20, ..Default::default() }).await.unwrap();
        assert_eq!(eps.iter().filter(|e| e.concepts.iter().any(|c| c == SHARED_CONCEPT)).count(), 3, "{}", eps.len());
        assert!(eps.iter().any(|e| e.title == "Antrean" && e.text.contains("dibagikan Budi lewat repositori git")));
    }

    #[test]
    fn git_config_parsing_and_slugs() {
        let cfg = "[core]\n\thooksPath = .husky/_\n[user]\n\tname = \"Budi Santoso\"\n\temail = budi@example.com\n[remote \"origin\"]\n\tname = bukan\n";
        assert_eq!(git_config_value(cfg, "user", "name").as_deref(), Some("Budi Santoso"));
        assert_eq!(git_config_value(cfg, "user", "email").as_deref(), Some("budi@example.com"));
        assert_eq!(git_config_value(cfg, "core", "hookspath").as_deref(), Some(".husky/_"));
        assert_eq!(slug("Budi Santoso"), "budi-santoso");
        assert_eq!(slug("  ***  "), "anggota");
        assert!(valid_member_id("budi-santoso-1a2b3c4d") && !valid_member_id("../x") && !valid_member_id("A"));
        assert_eq!(record_id_of("abc123.json"), Some("abc123"));
        assert!(record_id_of("../x.json").is_none() && record_id_of("a.b.json").is_none());
    }

    #[test]
    fn hook_install_keeps_user_hooks_and_removes_cleanly() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        std::fs::write(root.join(".git/hooks/post-merge"), "#!/bin/sh\necho milik-pengguna\nexit 0\n").unwrap();
        std::fs::write(root.join(".git/hooks/pre-commit"), "#!/usr/bin/env python3\nprint('x')\n").unwrap();
        let r = git_hooks(root, false).unwrap();
        assert_eq!(r.installed, vec!["post-merge", "post-checkout"]);
        assert_eq!(r.skipped.len(), 1, "{r:?}");
        let pm = std::fs::read_to_string(root.join(".git/hooks/post-merge")).unwrap();
        assert!(pm.starts_with("#!/bin/sh\n# >>> engram"), "blok engram dijalankan sebelum `exit 0` milik pengguna: {pm}");
        assert!(pm.contains("echo milik-pengguna") && pm.contains("hook git repo-sync"));
        // Pasang ulang tidak menggandakan blok.
        git_hooks(root, false).unwrap();
        assert_eq!(std::fs::read_to_string(root.join(".git/hooks/post-merge")).unwrap().matches(HOOK_START).count(), 1);
        assert_eq!(installed_hooks(root), vec!["post-merge", "post-checkout"]);
        let r = git_hooks(root, true).unwrap();
        assert_eq!(r.removed, vec!["post-merge", "post-checkout"]);
        assert_eq!(std::fs::read_to_string(root.join(".git/hooks/post-merge")).unwrap(), "#!/bin/sh\necho milik-pengguna\nexit 0\n");
        assert!(!root.join(".git/hooks/post-checkout").exists(), "hook buatan engram sendiri dihapus utuh");

        std::fs::write(root.join(".git/config"), "[core]\n\thooksPath = .husky\n").unwrap();
        let r = git_hooks(root, false).unwrap();
        assert!(r.installed.is_empty() && r.skipped[0].contains("core.hooksPath"));
    }
}
