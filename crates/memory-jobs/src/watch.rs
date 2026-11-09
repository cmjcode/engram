//! Pemantau "Folder Penting" (Watched Folders): membaca keseluruhan isi folder
//! yang dipilih pengguna, mengambil konteksnya menggunakan `agy` CLI, lalu
//! menjadikannya memori di Engram. Setiap kali ada perubahan berkas di dalam
//! folder tersebut, Engram mendeteksi diff-nya (berkas baru, diubah, dihapus)
//! dan memanggil `agy` CLI untuk meng-update memori secara incremental.
//!
//! Pengaturan & status disimpan di `vault/watched_folders.json` sehingga
//! perubahan dari UI maupun CLI langsung berlaku tanpa membuka ulang server.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::json;

use memory_core::ids::{is_valid_id, now_rfc3339};
use memory_core::model::{Fact, ReviewItem, ReviewKind, ReviewStatus, SourceKind};
use memory_core::text::truncate_chars;
use memory_extract::Vault;
use memory_extract::det_id;
use memory_extract::ingest::IngestDoc;
use memory_extract::reconcile::is_non_exclusive_category;
use memory_extract::review::Decision as ReviewDecision;

use crate::{JobError, Result, Runtime};

const WATCH_FILE: &str = "watched_folders.json";
const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_PROMPT_EXCERPT_CHARS: usize = 16_000;
const AGY_TIMEOUT_SECS: u64 = 300;

/// Direktori yang selalu diabaikan saat memindai folder penting.
const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    "__pycache__",
    ".venv",
    "venv",
    "vendor",
    ".cache",
    "coverage",
    ".gemini",
    ".claude",
    ".vscode",
    ".idea",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".gradle",
    ".cargo",
    "Pods",
    "cargo-vendor",
    "flatpak-export",
    "DerivedData",
    "cmake-build-debug",
    "cmake-build-release",
];

/// Berkas lock/sementara yang diabaikan agar perubahan otomatisnya tidak memicu scan ulang.
const IGNORED_FILES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
    "bun.lock",
    "poetry.lock",
    "Gemfile.lock",
    "composer.lock",
    "go.sum",
    ".DS_Store",
    "Thumbs.db",
];

/// Ekstensi biner/media/database yang tidak dibaca sebagai teks konteks.
const IGNORED_EXTS: &[&str] = &[
    "png",
    "jpg",
    "jpeg",
    "gif",
    "webp",
    "ico",
    "bmp",
    "tiff",
    "mp3",
    "mp4",
    "wav",
    "ogg",
    "flac",
    "m4a",
    "mov",
    "avi",
    "mkv",
    "webm",
    "zip",
    "tar",
    "gz",
    "tgz",
    "bz2",
    "xz",
    "7z",
    "rar",
    "age",
    "pdf",
    "doc",
    "docx",
    "xls",
    "xlsx",
    "ppt",
    "pptx",
    "exe",
    "dll",
    "so",
    "dylib",
    "wasm",
    "rlib",
    "rmeta",
    "o",
    "a",
    "obj",
    "class",
    "pyc",
    "pyo",
    "jar",
    "db",
    "sqlite",
    "sqlite3",
    "sst",
    "ldb",
    "log",
    "woff",
    "woff2",
    "ttf",
    "otf",
    "eot",
    "part",
    "tmp",
    "bak",
    "swp",
    "crdownload",
    "download",
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchedFile {
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchedFolder {
    pub id: String,
    pub subject: String,
    pub path: PathBuf,
    pub label: String,
    pub enabled: bool,
    pub created_at: String,
    pub last_scan_at: Option<String>,
    pub last_change_at: Option<String>,
    pub last_tree_hash: Option<String>,
    pub file_count: usize,
    /// Peta `path_relatif -> { hash, size }` dari scan berhasil terakhir.
    pub files: BTreeMap<String, WatchedFile>,
    /// Ringkasan konteks terakhir (dipakai sebagai pembanding saat ada perubahan).
    pub last_summary: Option<String>,
    /// ID `Source` terakhir yang di-ingest ke memori dari folder ini.
    pub last_source_id: Option<String>,
    pub last_error: Option<String>,
    /// Ringkasan konteks terakhir per repo git di dalam folder (kunci: path relatif repo).
    pub repo_summaries: BTreeMap<String, String>,
}

impl Default for WatchedFolder {
    fn default() -> Self {
        Self {
            id: String::new(),
            subject: "default".into(),
            path: PathBuf::new(),
            label: String::new(),
            enabled: true,
            created_at: String::new(),
            last_scan_at: None,
            last_change_at: None,
            last_tree_hash: None,
            file_count: 0,
            files: BTreeMap::new(),
            last_summary: None,
            last_source_id: None,
            last_error: None,
            repo_summaries: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct WatchStore {
    folders: Vec<WatchedFolder>,
}

/// Snapshot pohon berkas saat ini di disk.
#[derive(Debug, Clone, Default)]
pub struct TreeSnapshot {
    pub tree_hash: String,
    pub files: BTreeMap<String, WatchedFile>,
    /// Ada berkas yang baru saja dimodifikasi (< `min_age_secs`); tunda ke putaran berikutnya.
    pub settling: bool,
}

/// Perbedaan antara snapshot terakhir dan kondisi folder saat ini.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeDiff {
    pub initial: bool,
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub removed: Vec<String>,
}

impl TreeDiff {
    pub fn has_changes(&self) -> bool {
        self.initial
            || !self.added.is_empty()
            || !self.modified.is_empty()
            || !self.removed.is_empty()
    }
}

/// Laporan hasil pemindaian satu folder penting.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FolderScanOutcome {
    pub folder_id: String,
    pub label: String,
    pub path: PathBuf,
    pub changed: bool,
    pub settling: bool,
    pub files: usize,
    pub added: usize,
    pub modified: usize,
    pub removed: usize,
    pub source_id: Option<String>,
    pub job_id: Option<String>,
    pub error: Option<String>,
    /// Proyek (repo) yang memorinya diperbarui pada pemindaian ini.
    #[serde(default)]
    pub projects: Vec<String>,
}

/// Status keseluruhan pemantau folder penting untuk UI & CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchStatus {
    pub agy_available: bool,
    pub agy_path: Option<String>,
    pub poll_secs: u64,
    pub folders: Vec<WatchedFolder>,
}

// ---------------------------------------------------------------- berkas state di vault

fn watch_file_path(vault: &Vault) -> PathBuf {
    vault.root().join(WATCH_FILE)
}

fn load_store(vault: &Vault) -> WatchStore {
    std::fs::read(watch_file_path(vault))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_store(vault: &Vault, st: &WatchStore) -> Result<()> {
    let p = watch_file_path(vault);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = p.with_file_name(format!(".{WATCH_FILE}.tmp"));
    let bytes = serde_json::to_vec_pretty(st).map_err(|e| JobError::Invalid(e.to_string()))?;
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

/// Daftar semua folder penting (opsional disaring per subjek).
pub fn list_folders(vault: &Vault, subject: Option<&str>) -> Vec<WatchedFolder> {
    let st = load_store(vault);
    st.folders
        .into_iter()
        .filter(|f| subject.is_none_or(|s| f.subject == s))
        .collect()
}

/// Status folder penting untuk satu subjek (atau semua bila `None`) beserta deteksi `agy` CLI.
pub fn status(rt: &Runtime, subject: Option<&str>) -> WatchStatus {
    let agy = find_agy_cli();
    WatchStatus {
        agy_available: agy.is_some(),
        agy_path: agy.map(|p| p.display().to_string()),
        poll_secs: rt.cfg.schedule.watch_poll_secs,
        folders: list_folders(&rt.vault, subject),
    }
}

/// Tambahkan folder penting baru untuk `subject`. Bila path yang sama sudah terdaftar
/// untuk subjek tersebut, mengembalikan entri yang sudah ada (dan mengaktifkannya kembali).
pub fn add_folder(
    vault: &Vault,
    subject: &str,
    path: &Path,
    label: Option<&str>,
) -> Result<WatchedFolder> {
    if !is_valid_id(subject) {
        return Err(JobError::Invalid(format!(
            "id subjek tidak valid: {subject}"
        )));
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::path::absolute(path)?
    };
    if !abs.exists() {
        return Err(JobError::Invalid(format!(
            "folder tidak ditemukan: {}",
            abs.display()
        )));
    }
    if !abs.is_dir() {
        return Err(JobError::Invalid(format!(
            "bukan folder: {}",
            abs.display()
        )));
    }
    let canon = abs.canonicalize().unwrap_or(abs);
    let vault_canon = vault
        .root()
        .canonicalize()
        .unwrap_or_else(|_| vault.root().to_path_buf());
    if canon.starts_with(&vault_canon) || vault_canon.starts_with(&canon) {
        return Err(JobError::Invalid(
            "folder penting tidak boleh berada di dalam vault (atau sebaliknya)".into(),
        ));
    }

    let id = det_id(&["watch", subject, &canon.to_string_lossy()]);
    let default_label = canon
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| canon.display().to_string());
    let chosen_label = label
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&default_label)
        .to_string();

    let mut st = load_store(vault);
    if let Some(existing) = st
        .folders
        .iter_mut()
        .find(|f| f.id == id || (f.subject == subject && f.path == canon))
    {
        existing.enabled = true;
        if label.is_some_and(|l| !l.trim().is_empty()) {
            existing.label = chosen_label;
        }
        let out = existing.clone();
        save_store(vault, &st)?;
        return Ok(out);
    }

    let folder = WatchedFolder {
        id,
        subject: subject.to_string(),
        path: canon,
        label: chosen_label,
        enabled: true,
        created_at: now_rfc3339(),
        ..Default::default()
    };
    st.folders.push(folder.clone());
    save_store(vault, &st)?;
    Ok(folder)
}

/// Hapus folder penting berdasarkan `id` atau path-nya. Mengembalikan `true` bila ditemukan.
pub fn remove_folder(vault: &Vault, id_or_path: &str) -> Result<bool> {
    let mut st = load_store(vault);
    let probe_path = Path::new(id_or_path);
    let canon = if probe_path.exists() {
        probe_path.canonicalize().ok()
    } else {
        std::path::absolute(probe_path).ok()
    };
    let before = st.folders.len();
    st.folders.retain(|f| {
        f.id != id_or_path
            && f.path.to_string_lossy() != id_or_path
            && canon.as_ref().is_none_or(|c| &f.path != c)
    });
    let removed = st.folders.len() < before;
    if removed {
        save_store(vault, &st)?;
    }
    Ok(removed)
}

pub fn get_folder(vault: &Vault, id_or_path: &str) -> Option<WatchedFolder> {
    let st = load_store(vault);
    let probe_path = Path::new(id_or_path);
    let canon = if probe_path.exists() {
        probe_path.canonicalize().ok()
    } else {
        std::path::absolute(probe_path).ok()
    };
    st.folders.into_iter().find(|f| {
        f.id == id_or_path
            || f.path.to_string_lossy() == id_or_path
            || canon.as_ref().is_some_and(|c| &f.path == c)
    })
}

fn update_folder(vault: &Vault, folder: &WatchedFolder) -> Result<()> {
    let mut st = load_store(vault);
    if let Some(slot) = st.folders.iter_mut().find(|f| f.id == folder.id) {
        *slot = folder.clone();
        save_store(vault, &st)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- pemindaian pohon berkas

fn is_ignored_dir(name: &str) -> bool {
    name.starts_with('.')
        || name.starts_with('~')
        || name.ends_with("-vendor")
        || IGNORED_DIRS.contains(&name)
}

fn is_ignored_file(path: &Path, name: &str) -> bool {
    if name.starts_with('.') || name.starts_with('~') || IGNORED_FILES.contains(&name) {
        return true;
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    IGNORED_EXTS.contains(&ext.as_str())
}

/// Pindai seluruh berkas teks/kode/dokumen yang relevan di bawah `root`.
pub fn scan_folder_tree(root: &Path, min_age_secs: u64) -> Result<TreeSnapshot> {
    if !root.is_dir() {
        return Err(JobError::Invalid(format!(
            "folder tidak bisa diakses: {}",
            root.display()
        )));
    }
    let mut snap = TreeSnapshot::default();
    let now = SystemTime::now();
    walk_dir(root, root, min_age_secs, now, &mut snap)?;
    snap.tree_hash = tree_hash_of(&snap.files);
    Ok(snap)
}

fn tree_hash_of(files: &BTreeMap<String, WatchedFile>) -> String {
    let mut hasher = blake3::Hasher::new();
    for (rel, f) in files {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update(f.hash.as_bytes());
        hasher.update(b"\n");
    }
    hasher.finalize().to_hex().to_string()
}

/// Repo git di dalam `root` (path relatif, `""` bila `root` sendiri repo), dicari sampai
/// kedalaman terbatas tanpa masuk ke folder yang diabaikan pemindai.
pub fn find_repo_roots(root: &Path) -> Vec<String> {
    fn walk(root: &Path, cur: &Path, depth: usize, out: &mut Vec<String>) {
        if cur.join(".git").exists()
            && let Ok(rel) = cur.strip_prefix(root)
        {
            out.push(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"));
        }
        if depth >= 6 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(cur) else { return };
        let mut dirs: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| !is_ignored_dir(&e.file_name().to_string_lossy()))
            .map(|e| e.path())
            .collect();
        dirs.sort();
        for d in dirs {
            walk(root, &d, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(root, root, 0, &mut out);
    out
}

/// Repo terdalam yang memuat berkas `rel` (`""` = di luar repo mana pun / akar folder).
fn unit_of<'a>(rel: &str, roots: &'a [String]) -> &'a str {
    roots
        .iter()
        .filter(|r| !r.is_empty() && (rel == r.as_str() || rel.starts_with(&format!("{r}/"))))
        .max_by_key(|r| r.len())
        .map(String::as_str)
        .unwrap_or("")
}

fn strip_unit<'a>(rel: &'a str, unit: &str) -> &'a str {
    if unit.is_empty() { rel } else { rel.strip_prefix(unit).map(|r| r.trim_start_matches('/')).unwrap_or(rel) }
}

fn walk_dir(
    root: &Path,
    cur: &Path,
    min_age_secs: u64,
    now: SystemTime,
    snap: &mut TreeSnapshot,
) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(cur)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for p in entries {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if !is_ignored_dir(&name) {
                walk_dir(root, &p, min_age_secs, now, snap)?;
            }
        } else if meta.is_file() {
            if is_ignored_file(&p, &name) || meta.len() > MAX_FILE_BYTES || meta.len() == 0 {
                continue;
            }
            if min_age_secs > 0
                && meta
                    .modified()
                    .ok()
                    .and_then(|m| now.duration_since(m).ok())
                    .is_some_and(|age| age.as_secs() < min_age_secs)
            {
                snap.settling = true;
            }
            let Ok(bytes) = std::fs::read(&p) else {
                continue;
            };
            // Lewati berkas biner (mengandung byte NUL di 8 KB pertama).
            if bytes.iter().take(8192).any(|&b| b == 0) {
                continue;
            }
            let Ok(rel_path) = p.strip_prefix(root) else {
                continue;
            };
            let rel = rel_path
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let hash = blake3::hash(&bytes).to_hex().to_string();
            snap.files.insert(
                rel,
                WatchedFile {
                    hash,
                    size: meta.len(),
                },
            );
        }
    }
    Ok(())
}

/// Hitung perbedaan antara berkas yang tersimpan di `prev` dan snapshot `cur`.
pub fn diff_trees(
    prev_hash: Option<&str>,
    prev: &BTreeMap<String, WatchedFile>,
    cur: &TreeSnapshot,
) -> TreeDiff {
    if prev_hash.is_none() {
        return TreeDiff {
            initial: true,
            added: cur.files.keys().cloned().collect(),
            modified: vec![],
            removed: vec![],
        };
    }
    if prev_hash == Some(cur.tree_hash.as_str()) {
        return TreeDiff::default();
    }
    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut removed = Vec::new();
    for (k, cf) in &cur.files {
        match prev.get(k) {
            None => added.push(k.clone()),
            Some(pf) if pf.hash != cf.hash => modified.push(k.clone()),
            Some(_) => {}
        }
    }
    for k in prev.keys() {
        if !cur.files.contains_key(k) {
            removed.push(k.clone());
        }
    }
    TreeDiff {
        initial: false,
        added,
        modified,
        removed,
    }
}

// ---------------------------------------------------------------- agy CLI

fn search_paths() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    {
        v.extend([
            home.join(".local/bin"),
            home.join(".antigravity/bin"),
            home.join(".gemini/antigravity/bin"),
            home.join(".npm-global/bin"),
            home.join(".bun/bin"),
            home.join(".cargo/bin"),
        ]);
    }
    v.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    v
}

/// Temukan lokasi executable `agy` CLI.
/// Bisa dioverride lewat env `ENGRAM_AGY_BIN` (berguna untuk konfigurasi khusus atau pengujian).
pub fn find_agy_cli() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("ENGRAM_AGY_BIN") {
        let p = PathBuf::from(custom.trim());
        if p.is_file() {
            return Some(p);
        }
    }
    let names: &[&str] = if cfg!(windows) {
        &["agy.exe", "agy.cmd"]
    } else {
        &["agy"]
    };
    search_paths()
        .into_iter()
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| p.is_file())
}

/// Susun cuplikan berkas utama atau berkas yang berubah untuk disertakan di prompt `agy`.
fn collect_file_excerpts(root: &Path, rel_paths: &[String], max_chars: usize) -> String {
    let mut prioritized: Vec<&String> = rel_paths.iter().collect();
    // Utamakan README, dokumentasi, dan berkas konfigurasi utama bila membaca keseluruhan.
    prioritized.sort_by_key(|p| {
        let low = p.to_lowercase();
        if low.starts_with("readme") || low.ends_with("/readme.md") {
            0
        } else if low.ends_with(".md")
            || low.ends_with(".txt")
            || low.ends_with(".toml")
            || low.ends_with("package.json")
        {
            1
        } else {
            2
        }
    });

    let mut out = String::new();
    for rel in prioritized {
        if out.chars().count() >= max_chars {
            break;
        }
        let full = root.join(rel);
        let Ok(content) = std::fs::read_to_string(&full) else {
            continue;
        };
        let clean = content.trim();
        if clean.is_empty() {
            continue;
        }
        let remaining = max_chars.saturating_sub(out.chars().count());
        let snippet = truncate_chars(clean, remaining.min(2500));
        out.push_str(&format!("\n--- Berkas: {rel} ---\n{snippet}\n"));
    }
    out
}

fn build_agy_prompt(
    subject_name: &str,
    folder: &WatchedFolder,
    snap: &TreeSnapshot,
    diff: &TreeDiff,
) -> String {
    let file_list: Vec<String> = snap.files.keys().take(200).cloned().collect();
    let tree_preview = file_list.join("\n- ");
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    if diff.initial || folder.last_summary.is_none() {
        let all_keys: Vec<String> = snap.files.keys().cloned().collect();
        let excerpts = collect_file_excerpts(&folder.path, &all_keys, MAX_PROMPT_EXCERPT_CHARS);
        format!(
            "Kamu adalah agen pembaca konteks untuk sistem memori pribadi & proyek bernama Engram.\n\
            Tugasmu adalah membaca seluruh isi folder penting \"{label}\" (lokasi: `{path}`) milik subjek bernama **{subject_name}** (tanggal pengamatan: {today}), \
            memahami konteks keseluruhannya (baik berupa proyek kode, arsip kerja, maupun catatan/dokumen pribadi), \
            lalu menuliskannya sebagai dokumen memori naratif dalam bahasa Indonesia yang kaya fakta.\n\n\
            ATURAN PENULISAN OUTPUT (WAJIB DIIKUTI):\n\
            1. Tulis HANYA teks narasi memori dalam bahasa Indonesia (tanpa pembuka basa-basi seperti 'Berikut adalah...', tanpa blok kode panjang). Bagi menjadi beberapa paragraf bertopik jelas.\n\
            2. Di setiap paragraf, sebutkan nama **{subject_name}** secara eksplisit sebagai pelaku/pemilik (misal: \"{subject_name} mengembangkan/memiliki/mencatat...\") agar pipeline Engram mengenali bahwa semua segmen ini adalah tentang {subject_name}.\n\
            3. Paragraf 1 - Gambaran Umum & Tujuan: Jelaskan apa isi folder \"{label}\" secara keseluruhan, tujuan utamanya, serta peran {subject_name}.\n\
            4. Paragraf 2 - Fakta & Entitas Penting: Nyatakan fakta-fakta spesifik yang berlaku saat ini (teknologi/arsitektur yang digunakan bila folder proyek, atau tokoh/tempat/organisasi/informasi penting bila folder dokumen/catatan).\n\
            5. Paragraf 3 - Keputusan & Prinsip Kerja: Jelaskan keputusan penting atau preferensi yang diambil {subject_name} di dalam folder ini beserta alasannya (situasi -> tindakan -> alasan).\n\
            6. Gunakan alat baca berkasmu bila perlu memeriksa berkas lain di workspace untuk melengkapi konteks.\n\n\
            Daftar berkas di folder \"{label}\" ({total} berkas):\n- {tree_preview}\n\n\
            Cuplikan isi berkas:\n{excerpts}",
            label = folder.label,
            path = folder.path.display(),
            total = snap.files.len(),
        )
    } else {
        let mut changed_files: Vec<String> = Vec::new();
        changed_files.extend(diff.added.iter().cloned());
        changed_files.extend(diff.modified.iter().cloned());
        if changed_files.is_empty() {
            changed_files.extend(snap.files.keys().take(20).cloned());
        }
        let excerpts =
            collect_file_excerpts(&folder.path, &changed_files, MAX_PROMPT_EXCERPT_CHARS);
        let prev_summary = folder.last_summary.as_deref().unwrap_or("-");
        format!(
            "Kamu adalah agen pembaru konteks untuk sistem memori Engram.\n\
            Folder penting \"{label}\" (lokasi: `{path}`) milik subjek **{subject_name}** baru saja mengalami perubahan pada {today}.\n\
            Tugasmu adalah membaca perubahan terbaru sekaligus memperbarui konteks keseluruhan folder tersebut menjadi narasi memori bahasa Indonesia.\n\n\
            RINGKASAN KONTEKS SEBELUMNYA:\n{prev_summary}\n\n\
            PERUBAHAN BERKAS YANG TERDETEKSI:\n\
            - Berkas baru ditambahkan ({n_add}): {added}\n\
            - Berkas diubah ({n_mod}): {modified}\n\
            - Berkas dihapus ({n_rem}): {removed}\n\n\
            ATURAN PENULISAN OUTPUT (WAJIB DIIKUTI):\n\
            1. Tulis HANYA teks narasi memori dalam bahasa Indonesia (tanpa basa-basi pembuka). Bagi menjadi beberapa paragraf yang jelas.\n\
            2. Sebutkan nama **{subject_name}** secara eksplisit di setiap paragraf sebagai pelaku.\n\
            3. Paragraf 1 - Pembaruan Terbaru ({today}): Jelaskan secara konkret apa yang baru saja ditambahkan, diubah, atau dihapus oleh {subject_name} di folder \"{label}\" dan mengapa perubahan itu penting.\n\
            4. Paragraf 2 - Fakta yang Berubah / Diperbarui: Bila ada fakta, konfigurasi, fitur, keputusan, atau isi catatan yang berubah dari keadaan sebelumnya, nyatakan dengan tegas keadaan barunya saat ini (misal: \"Saat ini pada {today}, {subject_name} mengubah/menggunakan...\") agar fakta lama dapat digantikan (di-supersede).\n\
            5. Paragraf 3 - Konteks Keseluruhan Terkini Folder \"{label}\": Ringkas keadaan utuh folder \"{label}\" saat ini beserta fakta dan keputusan kunci yang masih berlaku.\n\
            6. Gunakan alat baca berkasmu bila perlu melihat berkas lain di folder `{path}`.\n\n\
            Daftar berkas saat ini ({total} berkas):\n- {tree_preview}\n\n\
            Cuplikan berkas yang berubah:\n{excerpts}",
            label = folder.label,
            path = folder.path.display(),
            n_add = diff.added.len(),
            added = if diff.added.is_empty() {
                "-".into()
            } else {
                diff.added.join(", ")
            },
            n_mod = diff.modified.len(),
            modified = if diff.modified.is_empty() {
                "-".into()
            } else {
                diff.modified.join(", ")
            },
            n_rem = diff.removed.len(),
            removed = if diff.removed.is_empty() {
                "-".into()
            } else {
                diff.removed.join(", ")
            },
            total = snap.files.len(),
        )
    }
}

/// Panggil `agy` CLI untuk membaca folder dan menghasilkan teks konteks memori.
pub async fn run_agy_context(
    subject_name: &str,
    folder: &WatchedFolder,
    snap: &TreeSnapshot,
    diff: &TreeDiff,
) -> Result<String> {
    let bin = find_agy_cli().ok_or_else(|| {
        JobError::Invalid(
            "CLI `agy` tidak ditemukan. Pastikan Antigravity CLI (`agy`) terpasang di PATH atau ~/.local/bin/agy".into(),
        )
    })?;
    let prompt = build_agy_prompt(subject_name, folder, snap, diff);
    let print_arg = format!("--print={prompt}");
    let path_env =
        std::env::join_paths(search_paths()).map_err(|e| JobError::Invalid(e.to_string()))?;

    let mut cmd = tokio::process::Command::new(&bin);
    cmd.current_dir(&folder.path)
        .args([
            "--add-dir",
            &folder.path.to_string_lossy(),
            "--disable-slash-commands",
            "--dangerously-skip-permissions",
            &print_arg,
        ])
        .env("PATH", &path_env)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let out = tokio::time::timeout(Duration::from_secs(AGY_TIMEOUT_SECS), cmd.output())
        .await
        .map_err(|_| {
            JobError::Invalid(format!(
                "agy CLI tidak selesai dalam {AGY_TIMEOUT_SECS} detik"
            ))
        })?
        .map_err(JobError::Io)?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(JobError::Invalid(format!(
            "agy CLI gagal ({}): {}",
            out.status,
            truncate_chars(&detail, 400)
        )));
    }

    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        return Err(JobError::Invalid(
            "agy CLI mengembalikan keluaran kosong".into(),
        ));
    }
    Ok(text)
}

// ---------------------------------------------------------------- orkestrasi scan & ingest

/// Periksa satu folder penting: bila ada perubahan (atau `force == true`), ambil konteksnya
/// lewat `agy` CLI lalu ingest ke pipeline memori Engram.
pub async fn scan_one(
    rt: &Runtime,
    id_or_path: &str,
    force: bool,
    min_age_secs: u64,
) -> Result<FolderScanOutcome> {
    let _guard = rt
        .watch_lock
        .try_lock()
        .map_err(|_| JobError::Invalid("pemindaian folder penting sedang berjalan".into()))?;
    scan_one_locked(rt, id_or_path, force, min_age_secs).await
}

async fn scan_one_locked(
    rt: &Runtime,
    id_or_path: &str,
    force: bool,
    min_age_secs: u64,
) -> Result<FolderScanOutcome> {
    let mut folder = get_folder(&rt.vault, id_or_path).ok_or_else(|| {
        JobError::Invalid(format!("folder penting tidak ditemukan: {id_or_path}"))
    })?;

    let mut outcome = FolderScanOutcome {
        folder_id: folder.id.clone(),
        label: folder.label.clone(),
        path: folder.path.clone(),
        ..Default::default()
    };

    let folder_path = folder.path.clone();
    let min_age = if force { 0 } else { min_age_secs };
    let snap_res = tokio::task::spawn_blocking(move || scan_folder_tree(&folder_path, min_age))
        .await
        .map_err(|e| JobError::Invalid(format!("scan task gagal: {e}")))?;
    let snap = match snap_res {
        Ok(s) => s,
        Err(e) => {
            folder.last_scan_at = Some(now_rfc3339());
            folder.last_error = Some(e.to_string());
            update_folder(&rt.vault, &folder)?;
            outcome.error = Some(e.to_string());
            return Ok(outcome);
        }
    };

    outcome.files = snap.files.len();
    if snap.files.is_empty() {
        folder.last_scan_at = Some(now_rfc3339());
        folder.file_count = 0;
        folder.last_error = None;
        update_folder(&rt.vault, &folder)?;
        return Ok(outcome);
    }

    if snap.settling && !force {
        outcome.settling = true;
        return Ok(outcome);
    }

    let mut diff = diff_trees(folder.last_tree_hash.as_deref(), &folder.files, &snap);
    if force && !diff.has_changes() {
        diff.initial = true;
        diff.added = snap.files.keys().cloned().collect();
    }

    folder.last_scan_at = Some(now_rfc3339());
    folder.file_count = snap.files.len();

    if !diff.has_changes() {
        folder.last_error = None;
        update_folder(&rt.vault, &folder)?;
        return Ok(outcome);
    }

    outcome.changed = true;
    outcome.added = diff.added.len();
    outcome.modified = diff.modified.len();
    outcome.removed = diff.removed.len();

    let subject = rt.pipeline.ensure_subject(&folder.subject, None).await?;

    // Satu unit memori per repo git di dalam folder; berkas di luar repo mana pun menjadi unit
    // akar (`""`) bernama label folder. Folder yang memuat banyak repo (mis. FOX/, VOLTUNES/)
    // dengan begitu terpecah per repo, dan label folder dicatat sebagai tag grup repo-repo itu.
    let folder_path = folder.path.clone();
    let mut roots = tokio::task::spawn_blocking(move || find_repo_roots(&folder_path))
        .await
        .map_err(|e| JobError::Invalid(format!("scan task gagal: {e}")))?;
    if roots.is_empty() || !roots.iter().any(String::is_empty) {
        roots.push(String::new());
    }
    let root_project = memory_core::project::detect_project(&folder.path)
        .filter(|p| p.from_git)
        .map(|p| p.name)
        .unwrap_or_else(|| folder.label.clone());
    let multi_repo = roots.iter().any(|r| !r.is_empty());

    let mut units: BTreeMap<String, (TreeSnapshot, TreeDiff)> = BTreeMap::new();
    for (rel, f) in &snap.files {
        let u = unit_of(rel, &roots);
        units.entry(u.to_string()).or_default().0.files.insert(strip_unit(rel, u).to_string(), f.clone());
    }
    let mut push_diff = |rel: &String, pick: fn(&mut TreeDiff) -> &mut Vec<String>| {
        let u = unit_of(rel, &roots).to_string();
        let entry = units.entry(u.clone()).or_default();
        pick(&mut entry.1).push(strip_unit(rel, &u).to_string());
    };
    for r in &diff.added {
        push_diff(r, |d| &mut d.added);
    }
    for r in &diff.modified {
        push_diff(r, |d| &mut d.modified);
    }
    for r in &diff.removed {
        push_diff(r, |d| &mut d.removed);
    }
    for (u, (sub_snap, sub_diff)) in units.iter_mut() {
        sub_snap.tree_hash = tree_hash_of(&sub_snap.files);
        let seen_before = if u.is_empty() { folder.last_summary.is_some() } else { folder.repo_summaries.contains_key(u) };
        // Repo yang belum pernah diringkas sendiri dibaca utuh.
        sub_diff.initial = diff.initial || (multi_repo && !seen_before && !sub_snap.files.is_empty());
    }

    let now_iso = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let stamp = chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string();
    let mut failed_units: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for (u, (sub_snap, sub_diff)) in &units {
        // Unit tanpa berkas (mis. repo yang sudah dihapus) tidak punya apa pun untuk diringkas.
        if !sub_diff.has_changes() || sub_snap.files.is_empty() {
            continue;
        }
        let project = if u.is_empty() {
            root_project.clone()
        } else {
            u.rsplit('/').next().unwrap_or(u).to_string()
        };
        let unit_folder = WatchedFolder {
            path: folder.path.join(u),
            label: if u.is_empty() { folder.label.clone() } else { project.clone() },
            last_summary: if u.is_empty() { folder.last_summary.clone() } else { folder.repo_summaries.get(u).cloned() },
            ..folder.clone()
        };
        let context_text = match run_agy_context(&subject.name, &unit_folder, sub_snap, sub_diff).await {
            Ok(t) => t,
            Err(e) => {
                failed_units.push(u.clone());
                errors.push(if u.is_empty() { e.to_string() } else { format!("{u}: {e}") });
                continue;
            }
        };
        let title = match (u.is_empty(), sub_diff.initial) {
            (true, true) => format!("Folder penting: {} (konteks menyeluruh)", folder.label),
            (true, false) => format!("Folder penting: {} (pembaruan {})", folder.label, stamp),
            (false, true) => format!("Repo {project} di folder penting {} (konteks menyeluruh)", folder.label),
            (false, false) => format!("Repo {project} di folder penting {} (pembaruan {})", folder.label, stamp),
        };
        let ingest_res = rt
            .pipeline
            .ingest(IngestDoc {
                subject: folder.subject.clone(),
                kind: SourceKind::Document,
                title,
                text: context_text.clone(),
                original_name: Some(project.clone()),
                occurred_from: Some(now_iso.clone()),
                project: Some(project.clone()),
                meta: json!({
                    "via": "watched_folder",
                    "extractor": "agy_cli",
                    "folder_id": folder.id,
                    "folder_label": folder.label,
                    "folder_path": folder.path,
                    "repo_path": u,
                    "scan_kind": if sub_diff.initial { "initial" } else { "update" },
                    "added": sub_diff.added,
                    "modified": sub_diff.modified,
                    "removed": sub_diff.removed,
                    "tree_hash": sub_snap.tree_hash,
                }),
                ..Default::default()
            })
            .await?;
        if u.is_empty() {
            folder.last_summary = Some(truncate_chars(&context_text, 4000));
        } else {
            folder.repo_summaries.insert(u.clone(), truncate_chars(&context_text, 4000));
            tag_project_group(rt, &folder.subject, &project, &folder.label).await;
        }
        outcome.projects.push(project);
        outcome.source_id.get_or_insert_with(|| ingest_res.source.id.clone());
        if outcome.job_id.is_none() {
            outcome.job_id = ingest_res.job_id.clone();
        }
        folder.last_source_id = Some(ingest_res.source.id);
    }

    // Unit yang gagal mempertahankan keadaan berkas lamanya agar dicoba lagi di putaran berikut.
    let mut files = snap.files.clone();
    if !failed_units.is_empty() {
        files.retain(|rel, _| !failed_units.iter().any(|u| unit_of(rel, &roots) == u));
        for (rel, f) in &folder.files {
            if failed_units.iter().any(|u| unit_of(rel, &roots) == u) {
                files.insert(rel.clone(), f.clone());
            }
        }
    }
    if !outcome.projects.is_empty() {
        folder.last_change_at = Some(now_rfc3339());
    }
    folder.last_tree_hash = Some(if failed_units.is_empty() { snap.tree_hash } else { tree_hash_of(&files) });
    folder.files = files;
    folder.last_error = (!errors.is_empty()).then(|| errors.join("; "));
    update_folder(&rt.vault, &folder)?;
    outcome.error = folder.last_error.clone();
    Ok(outcome)
}

/// Catat label folder penting sebagai tag grup di metadata proyek repo (tanpa menimpa isian lain).
async fn tag_project_group(rt: &Runtime, subject: &str, project: &str, group: &str) {
    let Some(tag) = memory_core::project::normalize_tag(group) else { return };
    if memory_core::project::same_project(project, group) {
        return;
    }
    let prev = match rt.store.get_project_meta(subject, project).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(project, error = %e, "gagal membaca metadata proyek");
            return;
        }
    };
    if prev.as_ref().is_some_and(|m| m.tags.iter().any(|t| t.eq_ignore_ascii_case(&tag))) {
        return;
    }
    let mut tags = prev.as_ref().map(|m| m.tags.clone()).unwrap_or_default();
    tags.push(tag);
    let name = prev.as_ref().map(|m| m.project.clone()).unwrap_or_else(|| project.to_string());
    let m = memory_core::model::ProjectMeta::build(
        subject,
        &name,
        prev.as_ref().and_then(|m| m.repo.as_deref()),
        prev.as_ref().map(|m| m.notes.as_str()).unwrap_or(""),
        &tags,
        prev.as_ref(),
    );
    if let Err(e) = rt.store.put_project_meta(&m).await {
        tracing::warn!(project, error = %e, "gagal menyimpan tag grup proyek");
    }
}

/// Pindai semua folder penting yang aktif (opsional disaring per subjek).
pub async fn scan_all(
    rt: &Runtime,
    subject: Option<&str>,
    force: bool,
    min_age_secs: u64,
) -> Result<Vec<FolderScanOutcome>> {
    let _guard = match rt.watch_lock.try_lock() {
        Ok(g) => g,
        Err(_) => return Ok(vec![]),
    };
    let folders = list_folders(&rt.vault, subject);
    let mut out = Vec::new();
    for f in folders.into_iter().filter(|f| f.enabled) {
        match scan_one_locked(rt, &f.id, force, min_age_secs).await {
            Ok(rep) => out.push(rep),
            Err(e) => {
                tracing::warn!(folder = %f.label, error = %e, "gagal memindai folder penting");
                out.push(FolderScanOutcome {
                    folder_id: f.id,
                    label: f.label,
                    path: f.path,
                    error: Some(e.to_string()),
                    ..Default::default()
                });
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- review otomatis via agy CLI

const AGY_REVIEW_BATCH_SIZE: usize = 20;
const AGY_REVIEW_TIMEOUT_SECS: u64 = 180;

/// Satu keputusan hasil review otomatis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoReviewDecisionItem {
    pub review_id: String,
    pub kind: String,
    pub summary: String,
    pub decision: ReviewDecision,
    pub note: String,
}

/// Laporan hasil review otomatis via `agy` CLI (dengan fallback heuristik/LLM).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoReviewReport {
    pub subject: String,
    pub total_open: u32,
    pub resolved: u32,
    pub approved: u32,
    pub rejected: u32,
    pub kept_both: u32,
    pub agy_used: bool,
    pub agy_path: Option<String>,
    pub decisions: Vec<AutoReviewDecisionItem>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AgyReviewDecisionRaw {
    review_id: String,
    decision: String,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AgyReviewOutRaw {
    #[serde(default)]
    decisions: Vec<AgyReviewDecisionRaw>,
}

fn parse_decision_str(raw: &str) -> Option<ReviewDecision> {
    match raw.trim().to_lowercase().as_str() {
        "approve" | "approved" | "setuju" | "accept" => Some(ReviewDecision::Approve),
        "reject" | "rejected" | "tolak" | "drop" => Some(ReviewDecision::Reject),
        "keep_both" | "both" | "keduanya" | "coexist" => Some(ReviewDecision::KeepBoth),
        _ => None,
    }
}

fn parse_agy_review_json(raw: &str) -> Vec<AgyReviewDecisionRaw> {
    let trimmed = raw.trim();
    if let Ok(wrapped) = serde_json::from_str::<AgyReviewOutRaw>(trimmed)
        && !wrapped.decisions.is_empty()
    {
        return wrapped.decisions;
    }
    if let Ok(arr) = serde_json::from_str::<Vec<AgyReviewDecisionRaw>>(trimmed) {
        return arr;
    }
    // Ekstrak blok JSON {...} atau [...] bila dibungkus markdown code fence atau teks pengantar.
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}'))
        && start < end
        && let Ok(wrapped) = serde_json::from_str::<AgyReviewOutRaw>(&trimmed[start..=end])
        && !wrapped.decisions.is_empty()
    {
        return wrapped.decisions;
    }
    if let (Some(start), Some(end)) = (trimmed.find('['), trimmed.rfind(']'))
        && start < end
        && let Ok(arr) = serde_json::from_str::<Vec<AgyReviewDecisionRaw>>(&trimmed[start..=end])
    {
        return arr;
    }
    vec![]
}

async fn run_agy_review(subject_name: &str, items_block: &str, work_dir: &Path) -> Result<Vec<AgyReviewDecisionRaw>> {
    let bin = find_agy_cli().ok_or_else(|| {
        JobError::Invalid("CLI `agy` tidak ditemukan di PATH atau ~/.local/bin/agy".into())
    })?;
    let prompt = format!(
        "Kamu adalah kurator memori otomatis untuk sistem Engram milik subjek **{subject_name}**.\n\
        Tugasmu adalah meninjau dan memutuskan daftar item Review berikut secara akurat.\n\n\
        ATURAN KEPUTUSAN PER JENIS REVIEW:\n\
        1. `conflict` (Dua fakta dianggap bertentangan):\n\
           - Pilih `\"keep_both\"` bila kedua fakta SEBENARNYA TIDAK SALING MENGGANTIKAN dan dapat berlaku bersamaan \
             (misal: dua fakta arsitektur, infrastruktur, komponen, fitur, konvensi, konfigurasi berbeda, atau path/proyek yang berbeda).\n\
           - Pilih `\"approve\"` bila Fakta Baru merupakan pembaruan yang sah dan menggantikan Fakta Lama yang sudah usang \
             (misal: pindah tempat tinggal, ganti versi/status dari atribut tunggal yang sama).\n\
           - Pilih `\"reject\"` bila Fakta Baru adalah noise, status teknis sesaat, atau salah ekstrak.\n\
        2. `entity_merge` (Apakah dua entitas sama?):\n\
           - Pilih `\"approve\"` HANYA bila keduanya benar-benar merujuk orang, proyek, atau komponen yang persis sama (misal nama pendek vs nama lengkap).\n\
           - Pilih `\"reject\"` bila keduanya adalah repositori, microservice, berkas, komponen, atau orang yang berbeda.\n\
        3. `quarantine` (Fakta meragukan):\n\
           - Pilih `\"approve\"` bila fakta didukung kutipan bukti dan bernilai jangka panjang.\n\
           - Pilih `\"reject\"` bila fakta mengarang, terlalu kabur, atau hanya proses sesaat.\n\
        4. `gray_zone` (Layak jadi episode cerita?):\n\
           - Pilih `\"approve\"` bila berisi keputusan teknis, bugfix, arsitektur, atau cerita hidup yang bermakna.\n\
           - Pilih `\"reject\"` bila hanya basa-basi, log kompilasi mentah, atau rutinitas tanpa nilai memori.\n\
        5. `reflective` (Konfirmasi prinsip/sifat):\n\
           - Pilih `\"approve\"` bila pernyataan prinsip/sifat konsisten dan akurat; `\"reject\"` bila meragukan.\n\n\
        KELUARKAN HANYA JSON MURNI (tanpa penjelasan di luar JSON, tanpa blok markdown) dengan format persis:\n\
        {{\"decisions\": [{{\"review_id\": \"<id>\", \"decision\": \"approve\" | \"reject\" | \"keep_both\", \"note\": \"[agy] <alasan singkat 1 kalimat bahasa Indonesia>\"}}]}}\n\n\
        DAFTAR ITEM REVIEW:\n{items_block}"
    );
    let print_arg = format!("--print={prompt}");
    let path_env =
        std::env::join_paths(search_paths()).map_err(|e| JobError::Invalid(e.to_string()))?;

    let mut cmd = tokio::process::Command::new(&bin);
    cmd.current_dir(work_dir)
        .args([
            "--disable-slash-commands",
            "--dangerously-skip-permissions",
            &print_arg,
        ])
        .env("PATH", &path_env)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }

    let out = tokio::time::timeout(Duration::from_secs(AGY_REVIEW_TIMEOUT_SECS), cmd.output())
        .await
        .map_err(|_| {
            JobError::Invalid(format!(
                "agy CLI review tidak selesai dalam {AGY_REVIEW_TIMEOUT_SECS} detik"
            ))
        })?
        .map_err(JobError::Io)?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(JobError::Invalid(format!(
            "agy CLI review gagal ({}): {}",
            out.status,
            truncate_chars(&detail, 300)
        )));
    }

    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let parsed = parse_agy_review_json(&text);
    if parsed.is_empty() {
        return Err(JobError::Invalid(format!(
            "agy CLI tidak mengembalikan JSON keputusan yang valid: {}",
            truncate_chars(&text, 240)
        )));
    }
    Ok(parsed)
}

fn fallback_decision(
    r: &ReviewItem,
    old_fact: Option<&Fact>,
    new_fact: Option<&Fact>,
    agy_tag: bool,
) -> (ReviewDecision, String) {
    let prefix = if agy_tag { "[agy]" } else { "[auto]" };
    match r.kind {
        ReviewKind::Conflict => {
            if let (Some(old), Some(new)) = (old_fact, new_fact) {
                if is_non_exclusive_category(&old.category)
                    || is_non_exclusive_category(&new.category)
                    || old.predicate != new.predicate
                {
                    return (
                        ReviewDecision::KeepBoth,
                        format!(
                            "{prefix} Kedua fakta '{}' dapat berlaku berdampingan (bukan atribut tunggal eksklusif)",
                            old.category
                        ),
                    );
                }
                if new.confidence >= old.confidence {
                    return (
                        ReviewDecision::Approve,
                        format!("{prefix} Fakta baru memperbarui fakta lama"),
                    );
                }
                return (
                    ReviewDecision::Reject,
                    format!("{prefix} Fakta lama dipertahankan karena keyakinan lebih tinggi"),
                );
            }
            (
                ReviewDecision::KeepBoth,
                format!("{prefix} Kedua fakta disimpan berdampingan"),
            )
        }
        ReviewKind::EntityMerge => (
            ReviewDecision::Reject,
            format!("{prefix} Dikonfirmasi sebagai dua entitas terpisah"),
        ),
        ReviewKind::Quarantine => {
            let quote = r.payload["quote"].as_str().unwrap_or("").trim();
            if !quote.is_empty() {
                (
                    ReviewDecision::Approve,
                    format!("{prefix} Fakta didukung kutipan bukti"),
                )
            } else {
                (
                    ReviewDecision::Reject,
                    format!("{prefix} Fakta tanpa kutipan bukti ditolak"),
                )
            }
        }
        ReviewKind::GrayZone => {
            let imp = r.payload["importance"].as_f64().unwrap_or(0.0);
            if imp >= 0.48 {
                (
                    ReviewDecision::Approve,
                    format!("{prefix} Segmen cukup bermakna untuk diangkat menjadi episode"),
                )
            } else {
                (
                    ReviewDecision::Reject,
                    format!("{prefix} Segmen dibiarkan di working memory"),
                )
            }
        }
        ReviewKind::Reflective => (
            ReviewDecision::Approve,
            format!("{prefix} Prinsip/disposisi dikonfirmasi aktif"),
        ),
    }
}

/// Jalankan review otomatis terhadap semua item `ReviewStatus::Open` milik `subject`
/// menggunakan `agy` CLI (serta aturan sanitasi entitas teknis & fallback cerdas).
pub async fn auto_review(rt: &Runtime, subject: &str) -> Result<AutoReviewReport> {
    let _guard = rt
        .review_lock
        .try_lock()
        .map_err(|_| JobError::Invalid("review otomatis sedang berjalan".into()))?;

    let agy_bin = find_agy_cli();
    let mut report = AutoReviewReport {
        subject: subject.to_string(),
        agy_used: agy_bin.is_some(),
        agy_path: agy_bin.map(|p| p.display().to_string()),
        ..Default::default()
    };

    let (_, dismissed) = rt
        .pipeline
        .sanitize_technical_entities(subject)
        .await
        .unwrap_or((0, 0));
    if dismissed > 0 {
        report.total_open += dismissed;
        report.resolved += dismissed;
        report.rejected += dismissed;
    }

    let open = rt
        .store
        .list_reviews(subject, ReviewStatus::Open, None, 500)
        .await?;
    report.total_open += open.len() as u32;
    if open.is_empty() {
        return Ok(report);
    }

    let subject_obj = rt.pipeline.ensure_subject(subject, None).await?;
    let mut entity_names: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    struct PreparedReview {
        item: ReviewItem,
        old_fact: Option<Fact>,
        new_fact: Option<Fact>,
        prompt_desc: String,
    }

    let mut prepared: Vec<PreparedReview> = Vec::with_capacity(open.len());
    for mut r in open {
        match r.kind {
            ReviewKind::Conflict => {
                let old_id = r.payload["existing"]
                    .as_str()
                    .or_else(|| r.payload["old"].as_str())
                    .unwrap_or("")
                    .to_string();
                let new_id = r.payload["new"]
                    .as_str()
                    .or_else(|| r.payload["incoming"].as_str())
                    .unwrap_or("")
                    .to_string();
                let old_f = if !old_id.is_empty() {
                    rt.store.get_fact(&old_id).await?
                } else {
                    None
                };
                let new_f = if !new_id.is_empty() {
                    rt.store.get_fact(&new_id).await?
                } else {
                    None
                };
                let (Some(old), Some(new)) = (old_f, new_f) else {
                    // Fakta sudah tidak ada; tutup item review yatim.
                    r.status = ReviewStatus::Rejected;
                    r.note = Some("[auto] Ditutup karena salah satu fakta sudah tidak ada".into());
                    r.resolved_at = Some(now_rfc3339());
                    rt.store.put_review(&r).await?;
                    report.resolved += 1;
                    report.rejected += 1;
                    continue;
                };
                let ent_name = match entity_names.get(&old.entity_id) {
                    Some(n) => n.clone(),
                    None => {
                        let n = rt
                            .store
                            .resolve_entity(&old.entity_id)
                            .await?
                            .map(|e| e.name)
                            .unwrap_or_else(|| subject_obj.name.clone());
                        entity_names.insert(old.entity_id.clone(), n.clone());
                        n
                    }
                };
                let old_quote = old
                    .evidence
                    .first()
                    .map(|e| truncate_chars(&e.quote, 220))
                    .unwrap_or_default();
                let new_quote = new
                    .evidence
                    .first()
                    .map(|e| truncate_chars(&e.quote, 220))
                    .unwrap_or_default();
                let desc = format!(
                    "- review_id: {}\n  jenis: conflict (entitas: \"{}\", kategori: \"{}\")\n  ringkasan: {}\n  fakta_lama: \"{} {} {}\" (waktu: {}, sejak: {}, bukti: \"{}\")\n  fakta_baru: \"{} {} {}\" (waktu: {}, sejak: {}, bukti: \"{}\")",
                    r.id,
                    ent_name,
                    old.category,
                    r.summary,
                    ent_name,
                    old.predicate,
                    old.object_text,
                    old.time.label(),
                    old.valid_from.as_deref().unwrap_or("?"),
                    old_quote,
                    ent_name,
                    new.predicate,
                    new.object_text,
                    new.time.label(),
                    new.valid_from.as_deref().unwrap_or("?"),
                    new_quote,
                );
                prepared.push(PreparedReview {
                    item: r,
                    old_fact: Some(old),
                    new_fact: Some(new),
                    prompt_desc: desc,
                });
            }
            ReviewKind::EntityMerge => {
                let new_id = r.payload["new"].as_str().unwrap_or("");
                let ex_id = r.payload["existing"].as_str().unwrap_or("");
                let ea = if !new_id.is_empty() {
                    rt.store.get_entity(new_id).await?
                } else {
                    None
                };
                let eb = if !ex_id.is_empty() {
                    rt.store.get_entity(ex_id).await?
                } else {
                    None
                };
                let (Some(a), Some(b)) = (ea, eb) else {
                    r.status = ReviewStatus::Rejected;
                    r.note = Some("[auto] Ditutup karena salah satu entitas sudah tidak ada".into());
                    r.resolved_at = Some(now_rfc3339());
                    rt.store.put_review(&r).await?;
                    report.resolved += 1;
                    report.rejected += 1;
                    continue;
                };
                let desc = format!(
                    "- review_id: {}\n  jenis: entity_merge\n  ringkasan: {}\n  entitas_baru: \"{}\" (jenis: {:?}, alias: {:?}, deskripsi: \"{}\")\n  entitas_lama: \"{}\" (jenis: {:?}, alias: {:?}, deskripsi: \"{}\")",
                    r.id, r.summary, a.name, a.kind, a.aliases, a.description, b.name, b.kind, b.aliases, b.description
                );
                prepared.push(PreparedReview {
                    item: r,
                    old_fact: None,
                    new_fact: None,
                    prompt_desc: desc,
                });
            }
            ReviewKind::Quarantine => {
                let quote = r.payload["quote"].as_str().unwrap_or("");
                let desc = format!(
                    "- review_id: {}\n  jenis: quarantine\n  ringkasan: {}\n  kutipan_bukti: \"{}\"",
                    r.id,
                    r.summary,
                    truncate_chars(quote, 300)
                );
                prepared.push(PreparedReview {
                    item: r,
                    old_fact: None,
                    new_fact: None,
                    prompt_desc: desc,
                });
            }
            ReviewKind::GrayZone => {
                let txt = r.payload["text"].as_str().unwrap_or("");
                let desc = format!(
                    "- review_id: {}\n  jenis: gray_zone\n  ringkasan: {}\n  teks_segmen: \"{}\"",
                    r.id,
                    r.summary,
                    truncate_chars(txt, 400)
                );
                prepared.push(PreparedReview {
                    item: r,
                    old_fact: None,
                    new_fact: None,
                    prompt_desc: desc,
                });
            }
            ReviewKind::Reflective => {
                let desc = format!(
                    "- review_id: {}\n  jenis: reflective\n  ringkasan: {}",
                    r.id, r.summary
                );
                prepared.push(PreparedReview {
                    item: r,
                    old_fact: None,
                    new_fact: None,
                    prompt_desc: desc,
                });
            }
        }
    }

    for chunk in prepared.chunks(AGY_REVIEW_BATCH_SIZE) {
        let block = chunk
            .iter()
            .map(|p| p.prompt_desc.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let mut agy_map: std::collections::HashMap<String, (ReviewDecision, String)> =
            std::collections::HashMap::new();

        if report.agy_used {
            match run_agy_review(&subject_obj.name, &block, rt.vault.root()).await {
                Ok(raw_list) => {
                    for d in raw_list {
                        if let Some(parsed_dec) = parse_decision_str(&d.decision) {
                            let mut note = d
                                .note
                                .unwrap_or_else(|| "Direview otomatis oleh agy CLI".into())
                                .trim()
                                .to_string();
                            if !note.starts_with("[agy]") {
                                note = format!("[agy] {note}");
                            }
                            agy_map.insert(d.review_id.trim().to_string(), (parsed_dec, note));
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "review via agy CLI gagal pada batch; memakai fallback");
                    report.error = Some(e.to_string());
                }
            }
        }

        for p in chunk {
            let (dec, note) = agy_map.remove(&p.item.id).unwrap_or_else(|| {
                fallback_decision(
                    &p.item,
                    p.old_fact.as_ref(),
                    p.new_fact.as_ref(),
                    report.agy_used && report.error.is_none(),
                )
            });
            match rt
                .pipeline
                .resolve_review(&p.item.id, dec, Some(note.clone()))
                .await
            {
                Ok(_) => {
                    report.resolved += 1;
                    match dec {
                        ReviewDecision::Approve => report.approved += 1,
                        ReviewDecision::Reject => report.rejected += 1,
                        ReviewDecision::KeepBoth => report.kept_both += 1,
                    }
                    report.decisions.push(AutoReviewDecisionItem {
                        review_id: p.item.id.clone(),
                        kind: format!("{:?}", p.item.kind).to_lowercase(),
                        summary: p.item.summary.clone(),
                        decision: dec,
                        note,
                    });
                }
                Err(e) => {
                    tracing::warn!(review = %p.item.id, error = %e, "gagal menyelesaikan item review");
                }
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory_core::model::{Evidence, Fact, FactKind, FactStatus, ReviewItem, ReviewKind, ReviewStatus};
    use memory_core::time::TimeRef;
    use memory_core::MemoryConfig;
    use memory_llm::HashEmbedder;
    use memory_llm::mock::ScriptedLlm;
    use memory_store::Store;
    use std::sync::Arc;

    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[test]
    fn tree_scan_ignores_noise_and_detects_diffs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/config"), "ignored").unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::write(root.join("node_modules/pkg/index.js"), "ignored").unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("target/debug/bin"), [0u8, 1, 2]).unwrap();
        std::fs::write(root.join("Cargo.lock"), "ignored lock").unwrap();
        std::fs::write(root.join("gambar.png"), [0x89u8, 0x50, 0x00]).unwrap();

        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("README.md"),
            "# Proyek Penting\nCatatan arsitektur.",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn halo() {}").unwrap();

        let snap1 = scan_folder_tree(root, 0).unwrap();
        assert_eq!(snap1.files.len(), 2);
        assert!(snap1.files.contains_key("README.md"));
        assert!(snap1.files.contains_key("src/lib.rs"));

        let d0 = diff_trees(None, &BTreeMap::new(), &snap1);
        assert!(d0.initial);
        assert_eq!(d0.added.len(), 2);

        // Tanpa perubahan -> diff kosong.
        let d_same = diff_trees(Some(&snap1.tree_hash), &snap1.files, &snap1);
        assert!(!d_same.has_changes());

        // Ubah satu berkas, tambah satu berkas, hapus satu berkas.
        std::fs::write(
            root.join("README.md"),
            "# Proyek Penting v2\nFitur baru ditambahkan.",
        )
        .unwrap();
        std::fs::write(root.join("catatan.txt"), "Keputusan penting hari ini.").unwrap();
        std::fs::remove_file(root.join("src/lib.rs")).unwrap();

        let snap2 = scan_folder_tree(root, 0).unwrap();
        let d2 = diff_trees(Some(&snap1.tree_hash), &snap1.files, &snap2);
        assert!(!d2.initial);
        assert_eq!(d2.added, vec!["catatan.txt".to_string()]);
        assert_eq!(d2.modified, vec!["README.md".to_string()]);
        assert_eq!(d2.removed, vec!["src/lib.rs".to_string()]);
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn watch_folder_initial_and_incremental_ingest_via_agy_cli() {
        use std::os::unix::fs::PermissionsExt;
        let _env_guard = ENV_LOCK.lock().await;

        let tmp = tempfile::tempdir().unwrap();
        let vault_dir = tmp.path().join("vault");
        let important_dir = tmp.path().join("proyek-penting");
        std::fs::create_dir_all(&important_dir).unwrap();
        std::fs::write(
            important_dir.join("README.md"),
            "Proyek ini dikerjakan Budi untuk otomasi memori keluarga.",
        )
        .unwrap();

        // Buat script tiruan `agy` CLI untuk memverifikasi alur pemanggilan CLI & ingest.
        let fake_agy = tmp.path().join("fake-agy");
        std::fs::write(
            &fake_agy,
            "#!/bin/sh\n\
             for arg in \"$@\"; do\n\
               case \"$arg\" in\n\
                 --print=*) has_print=1 ;;\n\
               esac\n\
             done\n\
             if [ -z \"$has_print\" ]; then echo \"missing --print=<prompt>\" >&2; exit 2; fi\n\
             echo \"Budi mengelola folder penting proyek-penting untuk menyimpan rancangan arsitektur keluarga. Saat ini Budi menggunakan Rust dan SurrealDB sebagai fondasi penyimpanan.\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_agy, std::fs::Permissions::from_mode(0o755)).unwrap();
        // SAFETY: hanya di unit test ini.
        unsafe {
            std::env::set_var("ENGRAM_AGY_BIN", &fake_agy);
        }

        let mut cfg = MemoryConfig::for_tests(64);
        cfg.vault.path = vault_dir.clone();
        let store = Store::memory(64, &cfg.index).await.unwrap();
        let vault = Vault::new(&vault_dir);
        vault.init().unwrap();
        let llm = Arc::new(ScriptedLlm::new(|_| Ok("{}".into())));
        let emb = Arc::new(HashEmbedder::new(64));
        let rt = Runtime::from_parts(cfg, store, llm, emb, vault);

        let f = add_folder(&rt.vault, "budi", &important_dir, Some("Proyek Penting")).unwrap();
        assert_eq!(f.label, "Proyek Penting");

        // Scan pertama (initial).
        let out1 = scan_one(&rt, &f.id, false, 0).await.unwrap();
        assert!(out1.changed, "{out1:?}");
        assert_eq!(out1.files, 1);
        assert!(out1.source_id.is_some());

        // Scan kedua tanpa perubahan berkas -> tidak memanggil ulang.
        let out_noop = scan_one(&rt, &f.id, false, 0).await.unwrap();
        assert!(!out_noop.changed);

        // Ada perubahan di folder -> otomatis terdeteksi & meng-update memori.
        std::fs::write(
            important_dir.join("keputusan.md"),
            "Budi memutuskan memakai agy CLI untuk mengekstrak konteks folder secara otomatis.",
        )
        .unwrap();
        std::fs::write(
            &fake_agy,
            "#!/bin/sh\n\
             echo \"Budi memperbarui folder proyek-penting dengan berkas keputusan.md. Budi memutuskan memakai agy CLI untuk mengekstrak konteks folder secara otomatis.\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_agy, std::fs::Permissions::from_mode(0o755)).unwrap();

        let out2 = scan_one(&rt, &f.id, false, 0).await.unwrap();
        assert!(out2.changed);
        assert_eq!(out2.added, 1);
        assert_ne!(out2.source_id, out1.source_id);

        let sources = rt.store.list_sources("budi", None, 10, 0).await.unwrap();
        assert_eq!(sources.len(), 2);
        assert!(remove_folder(&rt.vault, &f.id).unwrap());
        assert!(list_folders(&rt.vault, Some("budi")).is_empty());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn watch_folder_with_many_repos_is_split_per_repo() {
        use std::os::unix::fs::PermissionsExt;
        let _env_guard = ENV_LOCK.lock().await;

        let tmp = tempfile::tempdir().unwrap();
        let vault_dir = tmp.path().join("vault");
        let group = tmp.path().join("VOLTUNES");
        for (repo, file) in [("BACKEND/api-trans-panen", "main.go"), ("BACKEND/api-master-kandang", "main.go"), ("FRONTEND/chick-mobile-app", "pubspec.yaml")] {
            std::fs::create_dir_all(group.join(repo).join(".git")).unwrap();
            std::fs::write(group.join(repo).join(".git/config"), "[core]").unwrap();
            std::fs::write(group.join(repo).join(file), format!("// {repo}")).unwrap();
        }
        std::fs::create_dir_all(group.join("AI")).unwrap();
        std::fs::write(group.join("AI/catatan.md"), "Catatan riset lintas repo.").unwrap();

        // agy tiruan menulis folder kerjanya, sehingga tiap unit terbukti dipanggil di repo-nya sendiri.
        let fake_agy = tmp.path().join("fake-agy");
        std::fs::write(&fake_agy, "#!/bin/sh\necho \"Budi mengerjakan $(basename \"$PWD\") dengan Go dan Flutter sebagai bagian ekosistem.\"\n").unwrap();
        std::fs::set_permissions(&fake_agy, std::fs::Permissions::from_mode(0o755)).unwrap();
        // SAFETY: hanya di unit test ini (dijaga ENV_LOCK).
        unsafe {
            std::env::set_var("ENGRAM_AGY_BIN", &fake_agy);
        }

        let mut cfg = MemoryConfig::for_tests(64);
        cfg.vault.path = vault_dir.clone();
        let store = Store::memory(64, &cfg.index).await.unwrap();
        let vault = Vault::new(&vault_dir);
        vault.init().unwrap();
        let rt = Runtime::from_parts(cfg, store, Arc::new(ScriptedLlm::new(|_| Ok("{}".into()))), Arc::new(HashEmbedder::new(64)), vault);

        let f = add_folder(&rt.vault, "budi", &group, Some("VOLTUNES")).unwrap();
        let out = scan_one(&rt, &f.id, false, 0).await.unwrap();
        assert!(out.error.is_none(), "{out:?}");
        let mut projects = out.projects.clone();
        projects.sort();
        assert_eq!(projects, vec!["VOLTUNES", "api-master-kandang", "api-trans-panen", "chick-mobile-app"]);

        let sources = rt.store.list_sources("budi", None, 20, 0).await.unwrap();
        let panen = sources.iter().find(|s| s.project.as_deref() == Some("api-trans-panen")).expect("sumber per repo");
        assert!(panen.title.contains("Repo api-trans-panen"), "{}", panen.title);
        let meta = rt.store.get_project_meta("budi", "api-trans-panen").await.unwrap().expect("tag grup tercatat");
        assert_eq!(meta.tags, vec!["VOLTUNES".to_string()]);
        assert!(rt.store.get_project_meta("budi", "VOLTUNES").await.unwrap().is_none(), "unit akar tidak memberi tag ke dirinya sendiri");

        // Perubahan di satu repo hanya memperbarui repo itu.
        std::fs::write(group.join("BACKEND/api-trans-panen/panen.go"), "// harga kontrak").unwrap();
        let out2 = scan_one(&rt, &f.id, false, 0).await.unwrap();
        assert_eq!(out2.projects, vec!["api-trans-panen".to_string()]);
        assert_eq!(out2.added, 1);

        // agy gagal: keadaan berkas repo itu tidak maju, jadi dicoba lagi pada putaran berikutnya.
        std::fs::write(group.join("BACKEND/api-master-kandang/stok.go"), "// stok").unwrap();
        std::fs::write(&fake_agy, "#!/bin/sh\necho gagal >&2\nexit 1\n").unwrap();
        let out3 = scan_one(&rt, &f.id, false, 0).await.unwrap();
        assert!(out3.error.as_deref().is_some_and(|e| e.contains("api-master-kandang")), "{out3:?}");
        std::fs::write(&fake_agy, "#!/bin/sh\necho \"Budi menambah modul stok di api-master-kandang.\"\n").unwrap();
        let out4 = scan_one(&rt, &f.id, false, 0).await.unwrap();
        assert_eq!(out4.projects, vec!["api-master-kandang".to_string()]);
        assert!(out4.error.is_none());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn auto_review_via_agy_cli_resolves_conflicts_and_quarantines() {
        use std::os::unix::fs::PermissionsExt;
        let _env_guard = ENV_LOCK.lock().await;

        let tmp = tempfile::tempdir().unwrap();
        let vault_dir = tmp.path().join("vault");
        let mut cfg = MemoryConfig::for_tests(64);
        cfg.vault.path = vault_dir.clone();
        let store = Store::memory(64, &cfg.index).await.unwrap();
        let vault = Vault::new(&vault_dir);
        vault.init().unwrap();
        let llm = Arc::new(ScriptedLlm::new(|_| Ok("{}".into())));
        let emb = Arc::new(HashEmbedder::new(64));
        let rt = Runtime::from_parts(cfg, store, llm, emb, vault);

        rt.pipeline.ensure_subject("budi", Some("Budi")).await.unwrap();

        let f_old = Fact {
            id: "f_old_1".into(),
            subject: "budi".into(),
            entity_id: "ent1".into(),
            predicate: "terletak di".into(),
            category: "infrastruktur".into(),
            object_entity: None,
            object_text: "/Users/jayuda/PROJECT/FOX".into(),
            kind: FactKind::Preference,
            exclusive: true,
            time: TimeRef::Unknown,
            valid_from: None,
            valid_to: None,
            ongoing: true,
            time_confidence: 0.9,
            observed_at: "2026-11-06".into(),
            confidence: 0.9,
            evidence: vec![Evidence {
                episode_id: "ep_1".into(),
                quote: "Direktori lokal FOX".into(),
            }],
            status: FactStatus::Conflict,
            superseded_by: None,
            supersedes: None,
            created_at: "2026-11-06T00:00:00Z".into(),
        };
        let f_new = Fact {
            id: "f_new_1".into(),
            subject: "budi".into(),
            entity_id: "ent1".into(),
            predicate: "terletak di".into(),
            category: "infrastruktur".into(),
            object_entity: None,
            object_text: "mqtt.foxlogger.app".into(),
            kind: FactKind::Preference,
            exclusive: true,
            time: TimeRef::Unknown,
            valid_from: None,
            valid_to: None,
            ongoing: true,
            time_confidence: 0.9,
            observed_at: "2026-11-06".into(),
            confidence: 0.9,
            evidence: vec![Evidence {
                episode_id: "ep_2".into(),
                quote: "Endpoint broker MQTT".into(),
            }],
            status: FactStatus::Conflict,
            superseded_by: None,
            supersedes: None,
            created_at: "2026-11-06T00:00:00Z".into(),
        };
        rt.store.put_fact(&f_old).await.unwrap();
        rt.store.put_fact(&f_new).await.unwrap();

        let rev_conflict = ReviewItem {
            id: "rev_conf_1".into(),
            subject: "budi".into(),
            kind: ReviewKind::Conflict,
            status: ReviewStatus::Open,
            summary: "Dua fakta 'infrastruktur' bertentangan".into(),
            payload: serde_json::json!({
                "existing": f_old.id,
                "new": f_new.id,
                "category": "infrastruktur",
            }),
            targets: vec![],
            created_at: "2026-11-06T00:00:00Z".into(),
            resolved_at: None,
            note: None,
        };
        rt.store.put_review(&rev_conflict).await.unwrap();

        // Buat fake agy yang mengembalikan JSON array berisi keputusan untuk rev_conf_1.
        let fake_agy = tmp.path().join("fake-agy-review");
        std::fs::write(
            &fake_agy,
            "#!/bin/sh\n\
             echo '[{\"review_id\":\"rev_conf_1\",\"decision\":\"keep_both\",\"note\":\"Endpoint MQTT dan direktori lokal keduanya berlaku bersamaan\"}]'\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_agy, std::fs::Permissions::from_mode(0o755)).unwrap();
        unsafe {
            std::env::set_var("ENGRAM_AGY_BIN", &fake_agy);
        }

        let rep = auto_review(&rt, "budi").await.unwrap();
        assert_eq!(rep.total_open, 1);
        assert_eq!(rep.resolved, 1);
        assert_eq!(rep.kept_both, 1);

        let f_old_after = rt.store.get_fact(&f_old.id).await.unwrap().unwrap();
        let f_new_after = rt.store.get_fact(&f_new.id).await.unwrap().unwrap();
        assert_eq!(f_old_after.status, FactStatus::Active);
        assert_eq!(f_new_after.status, FactStatus::Active);
        assert!(!f_old_after.exclusive);
        assert!(!f_new_after.exclusive);
    }
}
