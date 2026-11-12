//! Sinkronisasi antar komputer lewat folder bersama (Google Drive for desktop,
//! atau layanan lain yang menyediakan folder lokal).
//!
//! Database (SurrealKV) TIDAK ditaruh langsung di folder Drive: Drive menyalin
//! berkas setengah jadi dan dua komputer bisa menulis bersamaan. Sebagai gantinya
//! tiap komputer menulis snapshot miliknya sendiri:
//!
//! ```text
//! <folder sync>/
//!   devices/<id-perangkat>/manifest.json          nama perangkat, waktu, hash isi
//!   devices/<id-perangkat>/snapshot.jsonl.gz.age  semua record (terenkripsi kata sandi)
//!   sources/..                                    berkas sumber asli (dialamatkan hash, dipakai bersama)
//! ```
//!
//! Satu kali sync = tarik snapshot perangkat lain lalu gabungkan per ID record,
//! kemudian tulis snapshot sendiri. Aturan gabung:
//! - record yang belum ada ditambahkan; record yang sama persis dilewati;
//! - bila kedua versi punya `updated_at`, yang lebih baru menang; selain itu versi
//!   tarikan menang, tetapi daftar ID (mis. episode milik satu tokoh) digabung;
//! - sumber yang belum selesai diproses di perangkat asalnya belum diambil
//!   (agar tidak diproses dua kali); ikut di sync berikutnya setelah selesai;
//! - antrean job & batch import tetap lokal; penghapusan tidak ikut tersinkron.
//!
//! Embedding tidak dikirim; record yang berubah dihitung ulang embedding-nya di
//! komputer penerima (ditunda bila server embedding belum hidup).
//!
//! Berkas lokal di vault: `sync.json` (pengaturan), `sync_state.json` (status),
//! `sync.key` (kata sandi, hanya-pemilik).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use age::secrecy::SecretString;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use memory_core::ids::{new_id, now_rfc3339};
use memory_extract::Vault;
use memory_store::{TABLES, VECTOR_TABLES};

use crate::{JobError, Result, Runtime};

pub const FORMAT: &str = "engram-sync";
pub const FORMAT_VERSION: u32 = 1;
/// Env var kata sandi sinkronisasi (mengalahkan `sync.key`).
pub const PASSPHRASE_ENV: &str = "ENGRAM_SYNC_PASSPHRASE";
/// Subfolder yang dipakai bila folder pilihan pengguna sudah berisi berkas lain.
pub const SYNC_DIR_NAME: &str = "engram-sync";
/// Tabel yang hanya berarti di komputer ini.
const LOCAL_TABLES: &[&str] = &["job", "import_batch"];
const PAGE: usize = 1000;
const SETTINGS_FILE: &str = "sync.json";
const STATE_FILE: &str = "sync_state.json";
const KEY_FILE: &str = "sync.key";
const SNAPSHOT: &str = "snapshot.jsonl.gz";
const MANIFEST: &str = "manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncSettings {
    /// Folder sync (mis. `.../Google Drive/My Drive/engram-sync`). Kosong = sync mati.
    pub dir: Option<PathBuf>,
    /// Sync otomatis tiap sekian menit (0 = hanya manual).
    pub interval_minutes: u64,
    /// Nama komputer ini di daftar perangkat (default: nama komputer).
    pub device_name: Option<String>,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self { dir: None, interval_minutes: 10, device_name: None }
    }
}

/// Status lokal; tidak ikut disinkron.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncState {
    pub device_id: String,
    /// Sidik komputer; bila vault disalin ke komputer lain, ID perangkat dibuat baru.
    pub machine: String,
    pub last_push_hash: Option<String>,
    /// Sidik kata sandi saat push terakhir (ganti kata sandi -> push ulang).
    pub last_push_key: Option<String>,
    pub last_push_at: Option<String>,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    pub last_warnings: Vec<String>,
    /// Hash snapshot terakhir yang sudah digabung, per perangkat.
    pub pulled: BTreeMap<String, String>,
    /// Record (tabel, id) yang embedding-nya belum dihitung.
    pub pending_embed: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub schema_version: u32,
    pub device_id: String,
    pub device_name: String,
    pub created_at: String,
    /// blake3 isi snapshot (sebelum kompresi/enkripsi).
    pub hash: String,
    pub file: String,
    pub encrypted: bool,
    pub rows: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncReport {
    /// Nama perangkat yang datanya digabung kali ini.
    pub pulled_from: Vec<String>,
    pub added: u64,
    pub updated: u64,
    /// Versi lokal lebih baru, dipertahankan.
    pub kept_local: u64,
    /// Sumber yang belum selesai diproses di perangkat asalnya.
    pub deferred: u64,
    pub source_files_in: u64,
    pub source_files_out: u64,
    pub pushed: bool,
    pub embedded: u64,
    pub pending_embed: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub updated_at: String,
    pub rows: u64,
    pub encrypted: bool,
    pub this_device: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncStatus {
    pub configured: bool,
    pub settings: SyncSettings,
    /// Folder sync bisa diakses (Google Drive berjalan).
    pub dir_ok: bool,
    pub device_id: String,
    pub device_name: String,
    pub encrypted: bool,
    pub last_sync_at: Option<String>,
    pub last_push_at: Option<String>,
    pub last_error: Option<String>,
    pub last_warnings: Vec<String>,
    pub pending_embed: usize,
    pub devices: Vec<DeviceInfo>,
    /// Folder Google Drive yang terdeteksi di komputer ini.
    pub suggestions: Vec<PathBuf>,
}

// ---------------------------------------------------------------- berkas lokal

fn read_json<T: DeserializeOwned + Default>(p: &Path) -> T {
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_atomic(p: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = tmp_path(p);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}

fn write_json<T: Serialize>(p: &Path, v: &T) -> Result<()> {
    write_atomic(p, &serde_json::to_vec_pretty(v).map_err(|e| JobError::Invalid(e.to_string()))?)
}

/// Berkas sementara di folder yang sama (`.nama.tmp`): diabaikan pembaca dan rename-nya atomik.
fn tmp_path(p: &Path) -> PathBuf {
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    p.with_file_name(format!(".{name}.tmp"))
}

pub fn load_settings(vault: &Vault) -> SyncSettings {
    read_json(&vault.root().join(SETTINGS_FILE))
}

/// Simpan pengaturan. Folder sync diperiksa dan dibuat; mengembalikan pengaturan
/// final (folder bisa mendapat subfolder [`SYNC_DIR_NAME`]).
pub fn save_settings(vault: &Vault, mut s: SyncSettings) -> Result<SyncSettings> {
    if let Some(dir) = s.dir.take() {
        s.dir = Some(prepare_sync_dir(vault.root(), &dir)?);
    }
    s.device_name = s.device_name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    write_json(&vault.root().join(SETTINGS_FILE), &s)?;
    Ok(s)
}

/// Periksa & siapkan folder sync. Folder yang sudah berisi berkas lain (dan bukan
/// folder sync engram) mendapat subfolder [`SYNC_DIR_NAME`].
pub fn prepare_sync_dir(vault_root: &Path, dir: &Path) -> Result<PathBuf> {
    if !dir.is_absolute() {
        return Err(JobError::Invalid("folder sinkronisasi harus berupa path lengkap (absolut)".into()));
    }
    let mut dir = dir.to_path_buf();
    if dir.is_dir() && !dir.join("devices").is_dir() && std::fs::read_dir(&dir)?.next().is_some() {
        dir = dir.join(SYNC_DIR_NAME);
    }
    // Induk harus ada: bila Google Drive belum berjalan, jangan buat folder palsu di disk lokal.
    if !dir.parent().is_some_and(Path::is_dir) {
        return Err(JobError::Invalid(format!("folder {} tidak ditemukan; pastikan Google Drive berjalan", dir.parent().unwrap_or(&dir).display())));
    }
    let vault = vault_root.canonicalize().unwrap_or_else(|_| vault_root.to_path_buf());
    let probe = dir.canonicalize().unwrap_or_else(|_| dir.parent().and_then(|p| p.canonicalize().ok()).map(|p| p.join(dir.file_name().unwrap_or_default())).unwrap_or_else(|| dir.clone()));
    if probe.starts_with(&vault) || vault.starts_with(&probe) {
        return Err(JobError::Invalid("folder sinkronisasi tidak boleh berada di dalam vault (atau sebaliknya)".into()));
    }
    std::fs::create_dir_all(dir.join("devices"))?;
    Ok(dir)
}

/// Simpan atau hapus (`None`/kosong) kata sandi sinkronisasi.
pub fn set_passphrase(vault: &Vault, pass: Option<&str>) -> Result<()> {
    let p = vault.root().join(KEY_FILE);
    match pass.map(str::trim).filter(|s| !s.is_empty()) {
        None => {
            if p.exists() {
                std::fs::remove_file(&p)?;
            }
        }
        Some(s) => {
            if s.chars().count() < 8 {
                return Err(JobError::Invalid("kata sandi minimal 8 karakter".into()));
            }
            write_atomic(&p, s.as_bytes())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
            }
        }
    }
    Ok(())
}

fn passphrase(vault: &Vault) -> Option<String> {
    std::env::var(PASSPHRASE_ENV)
        .ok()
        .or_else(|| std::fs::read_to_string(vault.root().join(KEY_FILE)).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn key_fingerprint(pass: Option<&str>) -> String {
    match pass {
        Some(p) => blake3::hash(format!("engram-sync-key:{p}").as_bytes()).to_hex()[..16].to_string(),
        None => "none".into(),
    }
}

fn load_state(vault: &Vault) -> SyncState {
    let mut st: SyncState = read_json(&vault.root().join(STATE_FILE));
    let machine = machine_id();
    if st.device_id.is_empty() || st.machine != machine {
        // Vault baru, atau disalin dari komputer lain: perangkat ini perlu ID sendiri.
        st = SyncState { device_id: new_id(), machine, ..Default::default() };
    }
    st
}

fn save_state(vault: &Vault, st: &SyncState) -> Result<()> {
    write_json(&vault.root().join(STATE_FILE), st)
}

fn device_name(s: &SyncSettings) -> String {
    s.device_name.clone().unwrap_or_else(host_name)
}

// ---------------------------------------------------------------- status

pub fn status(rt: &Runtime) -> SyncStatus {
    let settings = load_settings(&rt.vault);
    let st = load_state(&rt.vault);
    let dir_ok = settings.dir.as_ref().is_some_and(|d| d.join("devices").is_dir());
    let devices = match &settings.dir {
        Some(d) if dir_ok => list_devices(d)
            .into_iter()
            .map(|m| DeviceInfo { this_device: m.device_id == st.device_id, id: m.device_id, name: m.device_name, updated_at: m.created_at, rows: m.rows, encrypted: m.encrypted })
            .collect(),
        _ => vec![],
    };
    SyncStatus {
        configured: settings.dir.is_some(),
        dir_ok,
        device_id: st.device_id.clone(),
        device_name: device_name(&settings),
        encrypted: passphrase(&rt.vault).is_some(),
        last_sync_at: st.last_sync_at,
        last_push_at: st.last_push_at,
        last_error: st.last_error,
        last_warnings: st.last_warnings,
        pending_embed: st.pending_embed.len(),
        devices,
        suggestions: suggest_dirs(),
        settings,
    }
}

fn list_devices(dir: &Path) -> Vec<Manifest> {
    let Ok(rd) = std::fs::read_dir(dir.join("devices")) else { return vec![] };
    let mut out: Vec<Manifest> = rd
        .flatten()
        .filter_map(|e| std::fs::read(e.path().join(MANIFEST)).ok())
        .filter_map(|b| serde_json::from_slice::<Manifest>(&b).ok())
        .filter(|m| m.format == FORMAT)
        .collect();
    out.sort_by(|a, b| a.device_name.cmp(&b.device_name));
    out
}

// ---------------------------------------------------------------- sync

/// Tarik data perangkat lain, gabungkan, lalu tulis snapshot komputer ini.
pub async fn sync_now(rt: &Runtime) -> Result<SyncReport> {
    let _guard = rt.sync_lock.try_lock().map_err(|_| JobError::Invalid("sinkronisasi sedang berjalan".into()))?;
    let settings = load_settings(&rt.vault);
    let dir = settings.dir.clone().ok_or_else(|| JobError::Invalid("folder sinkronisasi belum diatur".into()))?;
    let mut st = load_state(&rt.vault);
    let res = if dir.join("devices").is_dir() {
        run(rt, &dir, &settings, &mut st).await
    } else {
        Err(JobError::Invalid(format!("folder sinkronisasi {} tidak bisa diakses; pastikan Google Drive berjalan", dir.display())))
    };
    match &res {
        Ok(r) => {
            st.last_error = None;
            st.last_warnings = r.warnings.clone();
            st.last_sync_at = Some(now_rfc3339());
        }
        Err(e) => st.last_error = Some(e.to_string()),
    }
    save_state(&rt.vault, &st)?;
    res
}

async fn run(rt: &Runtime, dir: &Path, settings: &SyncSettings, st: &mut SyncState) -> Result<SyncReport> {
    let pass = passphrase(&rt.vault);
    let mut rep = SyncReport::default();
    for m in list_devices(dir) {
        if m.device_id == st.device_id || st.pulled.get(&m.device_id) == Some(&m.hash) {
            continue;
        }
        if m.schema_version > memory_core::SCHEMA_VERSION || m.version > FORMAT_VERSION {
            rep.warnings.push(format!("{}: dibuat engram versi lebih baru; perbarui aplikasi di komputer ini", m.device_name));
            continue;
        }
        match pull_device(rt, dir, &m, pass.as_deref(), st, &mut rep).await {
            Ok(true) => {
                st.pulled.insert(m.device_id.clone(), m.hash.clone());
                rep.pulled_from.push(m.device_name.clone());
            }
            // Snapshot belum lengkap tersinkron oleh Drive; record yang terbaca tetap valid.
            Ok(false) => rep.warnings.push(format!("{}: data belum lengkap terunduh dari Drive, dicoba lagi nanti", m.device_name)),
            Err(e) => rep.warnings.push(format!("{}: {e}", m.device_name)),
        }
    }
    rep.source_files_in = copy_missing(&dir.join("sources"), &rt.vault.sources_dir())?;
    embed_pending(rt, st, &mut rep).await;
    push(rt, dir, settings, pass.as_deref(), st, &mut rep).await?;
    rep.source_files_out = copy_missing(&rt.vault.sources_dir(), &dir.join("sources"))?;
    rep.pending_embed = st.pending_embed.len() as u64;
    Ok(rep)
}

/// Gabungkan snapshot satu perangkat. `Ok(false)` bila isinya tidak cocok dengan manifest.
async fn pull_device(rt: &Runtime, dir: &Path, m: &Manifest, pass: Option<&str>, st: &mut SyncState, rep: &mut SyncReport) -> Result<bool> {
    if m.file.contains(['/', '\\']) || m.file.starts_with('.') || m.device_id.contains(['/', '\\', '.']) {
        return Err(JobError::Invalid("manifest tidak valid".into()));
    }
    let path = dir.join("devices").join(&m.device_id).join(&m.file);
    if !path.exists() {
        return Ok(false);
    }
    let reader = open_snapshot(&path, pass)?;
    let mut hasher = blake3::Hasher::new();
    for line in BufReader::new(reader).lines() {
        let line = line.map_err(|e| JobError::Invalid(format!("snapshot rusak/belum lengkap: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
        let v: Value = serde_json::from_str(&line).map_err(|e| JobError::Invalid(format!("snapshot rusak: {e}")))?;
        let (Some(table), Some(row)) = (v["t"].as_str(), v.get("r")) else { continue };
        if !TABLES.contains(&table) || LOCAL_TABLES.contains(&table) {
            continue;
        }
        apply_row(rt, table, row, st, rep).await?;
    }
    Ok(hasher.finalize().to_hex().to_string() == m.hash)
}

fn open_snapshot(path: &Path, pass: Option<&str>) -> Result<Box<dyn Read + Send>> {
    let file = std::fs::File::open(path)?;
    let raw: Box<dyn Read + Send> = if path.extension().is_some_and(|e| e == "age") {
        let pass = pass.ok_or_else(|| JobError::Invalid("data terenkripsi: isi kata sandi sinkronisasi yang sama dengan komputer lain".into()))?;
        let dec = age::Decryptor::new(BufReader::new(file)).map_err(|e| JobError::Invalid(format!("berkas terenkripsi rusak: {e}")))?;
        let id = age::scrypt::Identity::new(SecretString::from(pass.to_string()));
        Box::new(
            dec.decrypt(std::iter::once(&id as &dyn age::Identity))
                .map_err(|_| JobError::Invalid("kata sandi sinkronisasi tidak cocok dengan komputer lain".into()))?,
        )
    } else {
        Box::new(file)
    };
    Ok(Box::new(flate2::read::GzDecoder::new(raw)))
}

/// Teks yang di-embed untuk record tabel vektor.
pub(crate) fn embed_text(table: &str, row: &Value) -> String {
    match table {
        "episode" => row["text"].as_str().unwrap_or("").to_string(),
        _ => serde_json::from_value::<memory_core::model::Entity>(row.clone()).map(|e| memory_extract::resolve::entity_embed_text(&e)).unwrap_or_default(),
    }
}

async fn apply_row(rt: &Runtime, table: &str, remote: &Value, st: &mut SyncState, rep: &mut SyncReport) -> Result<()> {
    let Some(id) = remote["id"].as_str() else { return Ok(()) };
    if table == "source" && !matches!(remote["status"].as_str(), Some("done" | "skipped")) {
        rep.deferred += 1;
        return Ok(());
    }
    let local: Option<Value> = rt.store.get(table, id).await?;
    let mut row = remote.clone();
    if let Some(l) = &local {
        if same(l, remote) {
            return Ok(());
        }
        let newer_local = matches!((l["updated_at"].as_str(), remote["updated_at"].as_str()), (Some(a), Some(b)) if !a.is_empty() && a > b);
        // Sumber yang sedang diproses di sini jangan ditimpa di tengah jalan.
        let busy = table == "source" && l["status"].as_str() == Some("processing");
        if newer_local || busy {
            rep.kept_local += 1;
            return Ok(());
        }
        row = merge(l, remote);
        if same(l, &row) {
            // Versi lokal sudah mencakup semuanya; dikirim balik saat push.
            rep.kept_local += 1;
            return Ok(());
        }
    }
    let is_vec = VECTOR_TABLES.contains(&table);
    // Embedding lama tetap dipakai bila teks yang di-embed tidak berubah.
    let keep = match &local {
        Some(l) if is_vec && embed_text(table, l) == embed_text(table, &row) => rt.store.embedding_of(table, id).await?,
        _ => None,
    };
    rt.store.upsert(table, id, &row, keep.as_deref()).await?;
    if is_vec && keep.is_none() {
        st.pending_embed.push((table.to_string(), id.to_string()));
    }
    if local.is_some() {
        rep.updated += 1;
    } else {
        rep.added += 1;
    }
    Ok(())
}

/// Bentuk baku untuk perbandingan: field `null` = tidak ada, daftar nilai
/// sederhana dibandingkan sebagai himpunan (urutan bisa beda antar komputer).
fn canon(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(o.iter().filter(|(_, x)| !x.is_null()).map(|(k, x)| (k.clone(), canon(x))).collect()),
        Value::Array(a) if a.iter().all(is_scalar) => {
            let mut items: Vec<String> = a.iter().map(Value::to_string).collect();
            items.sort();
            items.dedup();
            Value::Array(items.into_iter().map(Value::String).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(canon).collect()),
        x => x.clone(),
    }
}

fn is_scalar(v: &Value) -> bool {
    matches!(v, Value::String(_) | Value::Number(_) | Value::Bool(_))
}

pub(crate) fn same(a: &Value, b: &Value) -> bool {
    canon(a) == canon(b)
}

/// Gabung record yang diubah di dua komputer (tanpa `updated_at` penentu):
/// versi tarikan sebagai dasar, daftar nilai sederhana (mis. `episode_ids`,
/// `aliases`) digabung, dan penghitung `*_count` mengambil nilai terbesar.
/// Hasilnya sama di kedua komputer, jadi sync berikutnya tidak bolak-balik.
pub(crate) fn merge(local: &Value, remote: &Value) -> Value {
    let mut out = remote.clone();
    if let (Some(o), Some(l)) = (out.as_object_mut(), local.as_object()) {
        for (k, lv) in l {
            match (o.get_mut(k), lv) {
                (Some(Value::Array(ra)), Value::Array(la)) if ra.iter().chain(la).all(is_scalar) => {
                    for x in la {
                        if !ra.contains(x) {
                            ra.push(x.clone());
                        }
                    }
                }
                (Some(rv), lv) if k.ends_with("_count") => {
                    if let (Some(a), Some(b)) = (rv.as_u64(), lv.as_u64())
                        && b > a
                    {
                        *rv = lv.clone();
                    }
                }
                _ => {}
            }
        }
    }
    out
}

async fn embed_pending(rt: &Runtime, st: &mut SyncState, rep: &mut SyncReport) {
    let mut items = std::mem::take(&mut st.pending_embed);
    items.sort();
    items.dedup();
    let batch = rt.cfg.embedding.batch_size.max(1);
    let mut left = Vec::new();
    let mut chunks = items.chunks(batch);
    for chunk in chunks.by_ref() {
        let mut rows = Vec::new();
        for (t, id) in chunk {
            match rt.store.get::<Value>(t, id).await {
                Ok(Some(r)) => rows.push((t.clone(), id.clone(), embed_text(t, &r))),
                Ok(None) => {}
                Err(_) => left.push((t.clone(), id.clone())),
            }
        }
        if rows.is_empty() {
            continue;
        }
        let texts: Vec<String> = rows.iter().map(|r| r.2.clone()).collect();
        match rt.pipeline.embedder.embed(&texts).await {
            Ok(vecs) => {
                for ((t, id, _), v) in rows.iter().zip(vecs) {
                    match rt.store.patch(t, id, json!({"embedding": v})).await {
                        Ok(()) => rep.embedded += 1,
                        Err(_) => left.push((t.clone(), id.clone())),
                    }
                }
            }
            Err(e) => {
                rep.warnings.push(format!("embedding ditunda, server embedding belum bisa dihubungi: {e}"));
                left.extend(rows.into_iter().map(|(t, id, _)| (t, id)));
                break;
            }
        }
    }
    for rest in chunks {
        left.extend(rest.iter().cloned());
    }
    st.pending_embed = left;
}

async fn push(rt: &Runtime, dir: &Path, settings: &SyncSettings, pass: Option<&str>, st: &mut SyncState, rep: &mut SyncReport) -> Result<()> {
    // Teks polos ditulis di vault lokal dulu, jangan sampai terunggah ke Drive.
    std::fs::create_dir_all(rt.vault.exports_dir())?;
    let plain = rt.vault.exports_dir().join(".sync-snapshot.gz");
    let (hash, rows) = write_snapshot(rt, &plain).await?;
    let key = key_fingerprint(pass);
    let dev_dir = dir.join("devices").join(&st.device_id);
    let file = if pass.is_some() { format!("{SNAPSHOT}.age") } else { SNAPSHOT.to_string() };
    let unchanged = st.last_push_hash.as_deref() == Some(hash.as_str()) && st.last_push_key.as_deref() == Some(key.as_str()) && dev_dir.join(&file).exists();
    if unchanged {
        let _ = std::fs::remove_file(&plain);
        return Ok(());
    }
    std::fs::create_dir_all(&dev_dir)?;
    let target = dev_dir.join(&file);
    let tmp = tmp_path(&target);
    match pass {
        Some(p) => {
            let enc = age::Encryptor::with_user_passphrase(SecretString::from(p.to_string()));
            let mut w = enc.wrap_output(std::fs::File::create(&tmp)?)?;
            std::io::copy(&mut std::fs::File::open(&plain)?, &mut w)?;
            w.finish()?.sync_all()?;
        }
        None => {
            std::fs::copy(&plain, &tmp)?;
        }
    }
    std::fs::rename(&tmp, &target)?;
    let _ = std::fs::remove_file(&plain);
    // Snapshot versi lain (mis. sebelum enkripsi diaktifkan) dihapus.
    for other in [SNAPSHOT.to_string(), format!("{SNAPSHOT}.age")] {
        if other != file {
            let _ = std::fs::remove_file(dev_dir.join(other));
        }
    }
    let manifest = Manifest {
        format: FORMAT.into(),
        version: FORMAT_VERSION,
        schema_version: memory_core::SCHEMA_VERSION,
        device_id: st.device_id.clone(),
        device_name: device_name(settings),
        created_at: now_rfc3339(),
        hash: hash.clone(),
        file,
        encrypted: pass.is_some(),
        rows,
    };
    write_json(&dev_dir.join(MANIFEST), &manifest)?;
    st.last_push_hash = Some(hash);
    st.last_push_key = Some(key);
    st.last_push_at = Some(manifest.created_at);
    rep.pushed = true;
    Ok(())
}

/// Tulis semua record yang disinkron ke `path` (jsonl.gz). Mengembalikan (hash isi, jumlah record).
async fn write_snapshot(rt: &Runtime, path: &Path) -> Result<(String, u64)> {
    let mut gz = flate2::write::GzEncoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), flate2::Compression::default());
    let mut hasher = blake3::Hasher::new();
    let mut n = 0u64;
    for table in TABLES.iter().filter(|t| !LOCAL_TABLES.contains(t)) {
        let mut offset = 0;
        loop {
            let rows = rt.store.dump_table(table, false, PAGE, offset).await?;
            for r in &rows {
                let mut line = serde_json::to_vec(&json!({"t": table, "r": r})).map_err(|e| JobError::Invalid(e.to_string()))?;
                line.push(b'\n');
                hasher.update(&line);
                gz.write_all(&line)?;
                n += 1;
            }
            if rows.len() < PAGE {
                break;
            }
            offset += PAGE;
        }
    }
    gz.finish()?.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    Ok((hasher.finalize().to_hex().to_string(), n))
}

/// Salin berkas di `from` yang belum ada di `to` (path relatif sama). Berkas
/// sumber dialamatkan hash, jadi berkas yang sudah ada tidak perlu dibandingkan.
fn copy_missing(from: &Path, to: &Path) -> Result<u64> {
    if !from.is_dir() {
        return Ok(0);
    }
    let mut n = 0;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().to_string();
        // Berkas sementara/tersembunyi (milik kita atau Google Drive) dilewati.
        if name.starts_with('.') || name.starts_with('~') || name.ends_with(".tmp") {
            continue;
        }
        let ty = e.file_type()?;
        let dst = to.join(&name);
        if ty.is_dir() {
            n += copy_missing(&e.path(), &dst)?;
        } else if ty.is_file() && !dst.exists() {
            std::fs::create_dir_all(to)?;
            let tmp = tmp_path(&dst);
            std::fs::copy(e.path(), &tmp)?;
            std::fs::rename(&tmp, &dst)?;
            n += 1;
        }
    }
    Ok(n)
}

// ---------------------------------------------------------------- komputer ini

/// Folder Google Drive for desktop yang terdeteksi, sebagai saran folder sync.
pub fn suggest_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let drive_roots = ["My Drive", "Drive Saya"];
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from) {
        // macOS: ~/Library/CloudStorage/GoogleDrive-<email>/My Drive
        if let Ok(rd) = std::fs::read_dir(home.join("Library/CloudStorage")) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with("GoogleDrive") {
                    out.extend(drive_roots.iter().map(|r| e.path().join(r)).filter(|p| p.is_dir()));
                }
            }
        }
        // Versi lama Google Drive / Backup and Sync.
        for p in [home.join("Google Drive/My Drive"), home.join("Google Drive")] {
            if p.is_dir() {
                out.push(p);
                break;
            }
        }
    }
    // Windows: Google Drive dipasang sebagai drive G: (atau huruf lain).
    if cfg!(windows) {
        for letter in 'D'..='Z' {
            for r in drive_roots {
                let p = PathBuf::from(format!("{letter}:\\{r}"));
                if p.is_dir() {
                    out.push(p);
                }
            }
        }
    }
    out.into_iter().map(|p| p.join(SYNC_DIR_NAME)).collect()
}

fn command_output(cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = std::process::Command::new(cmd);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: jangan munculkan jendela konsol
    }
    let out = c.output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

/// Nama komputer yang ramah dibaca.
pub fn host_name() -> String {
    if cfg!(target_os = "macos")
        && let Some(n) = command_output("scutil", &["--get", "ComputerName"])
    {
        return n;
    }
    std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| command_output("hostname", &[]))
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "komputer".into())
}

/// Sidik komputer yang stabil (di-hash; ID mesin asli tidak disimpan).
pub(crate) fn machine_id() -> String {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(|| {
        let raw = machine_raw().unwrap_or_else(host_name);
        blake3::hash(format!("engram-machine:{raw}").as_bytes()).to_hex()[..16].to_string()
    })
    .clone()
}

fn machine_raw() -> Option<String> {
    if cfg!(target_os = "macos") {
        let out = command_output("ioreg", &["-rd1", "-c", "IOPlatformExpertDevice"])?;
        let line = out.lines().find(|l| l.contains("IOPlatformUUID"))?;
        return line.split('"').nth(3).map(str::to_string);
    }
    if cfg!(windows) {
        let out = command_output("reg", &["query", r"HKLM\SOFTWARE\Microsoft\Cryptography", "/v", "MachineGuid"])?;
        return out.lines().find(|l| l.contains("MachineGuid"))?.split_whitespace().last().map(str::to_string);
    }
    ["/etc/machine-id", "/var/lib/dbus/machine-id"].iter().find_map(|p| std::fs::read_to_string(p).ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
