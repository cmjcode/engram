//! Pengaturan dari UI: info aplikasi, cek update, dan edit `memory.toml`.
//!
//! Hanya kunci di [`FIELDS`] yang bisa diubah lewat UI. Berkas diedit sebagai
//! tabel TOML agar kunci lain tetap ada; perubahan berlaku setelah aplikasi
//! (atau `engram serve`) dijalankan ulang. API key tidak pernah dikirim balik ke UI.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Map, Value, json};

use memory_core::MemoryConfig;

pub const REPO_URL: &str = "https://github.com/cmjcode/engram";
const LATEST_JSON: &str = "https://github.com/cmjcode/engram/releases/latest/download/latest.json";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Hasil pemeriksaan update.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: Option<String>,
    pub available: bool,
    pub notes: Option<String>,
    pub date: Option<String>,
    /// Update bisa dipasang langsung dari UI (aplikasi desktop).
    pub can_install: bool,
    /// Halaman unduhan manual.
    pub url: String,
}

/// Kemajuan pengunduhan & pemasangan update aplikasi desktop.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UpdateProgress {
    /// Pembaruan sedang berjalan (`true` saat menyiapkan, mengunduh, atau memasang).
    pub running: bool,
    /// Tahap: `"idle"`, `"checking"`, `"downloading"`, `"installing"`, `"restarting"`, `"error"`.
    pub stage: String,
    /// Versi yang sedang diunduh/dipasang.
    pub version: Option<String>,
    /// Jumlah byte yang sudah diunduh.
    pub downloaded_bytes: u64,
    /// Total ukuran paket dalam byte (bila tersedia dari server).
    pub total_bytes: Option<u64>,
    /// Persentase kemajuan (0..=100), bila diketahui.
    pub percent: Option<u8>,
    /// Pesan status untuk ditampilkan di UI.
    pub message: Option<String>,
    /// Pesan galat terakhir bila gagal.
    pub error: Option<String>,
}

impl Default for UpdateProgress {
    fn default() -> Self {
        Self {
            running: false,
            stage: "idle".into(),
            version: None,
            downloaded_bytes: 0,
            total_bytes: None,
            percent: None,
            message: None,
            error: None,
        }
    }
}

/// Bandingkan versi "x.y.z" (awalan `v` & akhiran `-pre` diabaikan).
pub fn newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.trim().trim_start_matches('v').split(['-', '+']).next().unwrap_or("").split('.').map(|p| p.parse().unwrap_or(0)).collect()
    };
    let (a, b) = (parse(latest), parse(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// Cek rilis terbaru di GitHub (dipakai `engram serve`; aplikasi desktop memakai updater-nya sendiri).
pub async fn check_github() -> Result<UpdateInfo, String> {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(15)).user_agent(concat!("engram/", env!("CARGO_PKG_VERSION"))).build().map_err(|e| e.to_string())?;
    let resp = http.get(LATEST_JSON).send().await.map_err(|e| format!("tidak bisa menghubungi GitHub: {e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(UpdateInfo {
            current: VERSION.into(),
            latest: Some(VERSION.into()),
            available: false,
            notes: None,
            date: None,
            can_install: false,
            url: format!("{REPO_URL}/releases/latest"),
        });
    }
    if !resp.status().is_success() {
        return Err(format!("GitHub menjawab {}", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| format!("info rilis tidak terbaca: {e}"))?;
    let latest = v["version"].as_str().map(|s| s.trim_start_matches('v').to_string());
    Ok(UpdateInfo {
        current: VERSION.into(),
        available: latest.as_deref().is_some_and(|l| newer(l, VERSION)),
        latest,
        notes: v["notes"].as_str().map(String::from).filter(|s| !s.trim().is_empty()),
        date: v["pub_date"].as_str().map(String::from),
        can_install: false,
        url: format!("{REPO_URL}/releases/latest"),
    })
}

// ---------------------------------------------------------------- memory.toml

#[derive(Clone, Copy)]
enum Ty {
    Str,
    /// Kosong = kunci dihapus (pakai default / tidak dipakai).
    OptStr,
    /// Rahasia: tidak pernah dikirim ke UI; `null` = tidak berubah, "" = hapus.
    Secret,
    Bool,
    Int(i64, i64),
    Float(f64, f64),
    Choice(&'static [&'static str]),
}

/// Kunci yang boleh diubah dari UI.
const FIELDS: &[(&str, &str, Ty)] = &[
    ("vault", "default_subject", Ty::Str),
    ("llm", "base_url", Ty::Str),
    ("llm", "extract_model", Ty::Str),
    ("llm", "light_model", Ty::Str),
    ("llm", "light_base_url", Ty::OptStr),
    ("llm", "api_key_env", Ty::OptStr),
    ("llm", "api_key", Ty::Secret),
    ("llm", "json_mode", Ty::Choice(&["json_schema", "json_object", "none"])),
    ("llm", "disable_thinking", Ty::Bool),
    ("llm", "temperature", Ty::Float(0.0, 2.0)),
    ("llm", "max_tokens", Ty::Int(64, 1_000_000)),
    ("llm", "timeout_secs", Ty::Int(5, 3600)),
    ("llm", "max_retries", Ty::Int(0, 10)),
    ("embedding", "provider", Ty::Choice(&["openai", "hash"])),
    ("embedding", "base_url", Ty::Str),
    ("embedding", "model", Ty::Str),
    ("embedding", "dimension", Ty::Int(8, 16384)),
    ("embedding", "batch_size", Ty::Int(1, 1024)),
    ("embedding", "api_key_env", Ty::OptStr),
    ("embedding", "api_key", Ty::Secret),
    ("transcription", "base_url", Ty::OptStr),
    ("transcription", "model", Ty::Str),
    ("transcription", "language", Ty::OptStr),
    ("pipeline", "workers", Ty::Int(1, 32)),
    ("schedule", "inbox_poll_secs", Ty::Int(1, 3600)),
    ("schedule", "watch_poll_secs", Ty::Int(0, 3600)),
    ("schedule", "retry_failed_minutes", Ty::Int(0, 10080)),
    ("schedule", "retry_failed_max", Ty::Int(0, 10000)),
    ("score", "stream_ttl_days", Ty::Int(1, 36500)),
    ("retrieval", "top_k", Ty::Int(1, 100)),
    ("server", "max_upload_mb", Ty::Int(1, 102_400)),
];

fn field(section: &str, key: &str) -> Option<Ty> {
    FIELDS.iter().find(|(s, k, _)| *s == section && *k == key).map(|f| f.2)
}

/// Satu penulis memory.toml dalam satu waktu.
static WRITE: Mutex<()> = Mutex::new(());

pub fn read_table(path: &Path) -> Result<toml::Table, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => s.parse().map_err(|e| format!("format {}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(e) => Err(format!("membaca {}: {e}", path.display())),
    }
}

fn parse_config(table: &toml::Table) -> Result<MemoryConfig, String> {
    toml::Value::Table(table.clone()).try_into().map_err(|e: toml::de::Error| format!("pengaturan tidak valid: {e}"))
}

/// Nilai kunci yang bisa diubah, per bagian. Rahasia diganti `<key>_set: bool`.
pub fn editable_values(cfg: &MemoryConfig) -> Value {
    let full = serde_json::to_value(cfg).unwrap_or_default();
    let mut out = Map::new();
    for (s, k, ty) in FIELDS {
        let sec = out.entry(s.to_string()).or_insert_with(|| json!({})).as_object_mut().expect("objek");
        let v = full[s][k].clone();
        match ty {
            Ty::Secret => {
                sec.insert(format!("{k}_set"), json!(v.as_str().is_some_and(|x| !x.trim().is_empty())));
            }
            _ => {
                sec.insert(k.to_string(), v);
            }
        }
    }
    Value::Object(out)
}

/// Terapkan perubahan `{bagian: {kunci: nilai}}` ke tabel (belum ditulis).
pub fn apply(table: &mut toml::Table, changes: &Value) -> Result<(), String> {
    let changes = changes.as_object().ok_or("format perubahan harus objek")?;
    for (s, kv) in changes {
        let kv = kv.as_object().ok_or_else(|| format!("[{s}] harus objek"))?;
        for (k, v) in kv {
            let ty = field(s, k).ok_or_else(|| format!("{s}.{k} tidak bisa diubah dari UI"))?;
            let new = to_toml(ty, v).map_err(|e| format!("{s}.{k}: {e}"))?;
            let sec = table.entry(s.clone()).or_insert_with(|| toml::Value::Table(toml::Table::new()));
            let toml::Value::Table(sec) = sec else { return Err(format!("[{s}] di memory.toml bukan tabel")) };
            match new {
                Change::Keep => {}
                Change::Remove => {
                    sec.remove(k);
                }
                Change::Set(x) => {
                    sec.insert(k.clone(), x);
                }
            }
        }
    }
    parse_config(table).map(|_| ())
}

enum Change {
    Keep,
    Remove,
    Set(toml::Value),
}

fn to_toml(ty: Ty, v: &Value) -> Result<Change, String> {
    let text = || v.as_str().map(str::trim).ok_or_else(|| "harus teks".to_string());
    Ok(match ty {
        Ty::Str => {
            let s = text()?;
            if s.is_empty() {
                return Err("tidak boleh kosong".into());
            }
            Change::Set(toml::Value::String(s.into()))
        }
        Ty::OptStr => match v {
            Value::Null => Change::Remove,
            _ if text()?.is_empty() => Change::Remove,
            _ => Change::Set(toml::Value::String(text()?.into())),
        },
        Ty::Secret => match v {
            Value::Null => Change::Keep,
            _ if text()?.is_empty() => Change::Remove,
            _ => Change::Set(toml::Value::String(text()?.into())),
        },
        Ty::Bool => Change::Set(toml::Value::Boolean(v.as_bool().ok_or("harus true/false")?)),
        Ty::Int(lo, hi) => {
            let n = v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).ok_or("harus bilangan bulat")?;
            if !(lo..=hi).contains(&n) {
                return Err(format!("harus antara {lo} dan {hi}"));
            }
            Change::Set(toml::Value::Integer(n))
        }
        Ty::Float(lo, hi) => {
            let n = v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).ok_or("harus angka")?;
            if !(lo..=hi).contains(&n) {
                return Err(format!("harus antara {lo} dan {hi}"));
            }
            Change::Set(toml::Value::Float(n))
        }
        Ty::Choice(opts) => {
            let s = text()?;
            if !opts.contains(&s) {
                return Err(format!("pilihan: {}", opts.join(", ")));
            }
            Change::Set(toml::Value::String(s.into()))
        }
    })
}

/// Terapkan perubahan lalu tulis memory.toml (hanya-pemilik: bisa berisi API key).
pub fn save(path: &Path, changes: &Value) -> Result<(), String> {
    let _g = WRITE.lock().map_err(|_| "kunci pengaturan rusak".to_string())?;
    let mut table = read_table(path)?;
    apply(&mut table, changes)?;
    let body = toml::to_string_pretty(&table).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("menulis {}: {e}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("menulis {}: {e}", path.display()))
}

/// Konfigurasi dari berkas (atau `fallback` bila tidak ada berkas), ditambah perubahan yang belum disimpan.
pub fn preview(path: Option<&Path>, fallback: &MemoryConfig, changes: Option<&Value>) -> Result<MemoryConfig, String> {
    let mut table = match path {
        Some(p) => read_table(p)?,
        None => toml::Value::try_from(fallback).ok().and_then(|v| v.as_table().cloned()).unwrap_or_default(),
    };
    if let Some(c) = changes {
        apply(&mut table, c)?;
    }
    parse_config(&table)
}

// ---------------------------------------------------------------- tes koneksi

fn http(timeout: u64) -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(timeout)).build().unwrap_or_default()
}

fn short(s: &str) -> String {
    let s: String = s.chars().take(300).collect();
    s.trim().to_string()
}

/// Daftar model di `<base>/models`.
async fn list_models(base: &str, key: Option<&str>) -> Result<Vec<String>, String> {
    let url = format!("{}/models", base.trim_end_matches('/'));
    let mut rb = http(10).get(&url);
    if let Some(k) = key {
        rb = rb.bearer_auth(k);
    }
    let resp = rb.send().await.map_err(|e| format!("tidak bisa menghubungi {url}: {e}"))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{url} menjawab {status}: {}", short(&body)));
    }
    let v: Value = serde_json::from_str(&body).map_err(|_| format!("{url} tidak mengembalikan JSON"))?;
    Ok(v["data"].as_array().map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect()).unwrap_or_default())
}

/// Tes server LLM: bisa dihubungi dan model yang dipilih tersedia.
pub async fn test_llm(cfg: &MemoryConfig) -> Value {
    let key = cfg.llm.resolved_api_key();
    let mut warnings = vec![];
    let models = match list_models(&cfg.llm.base_url, key.as_deref()).await {
        Ok(m) => m,
        Err(e) => return json!({"ok": false, "message": e}),
    };
    let mut light_models = models.clone();
    if let Some(lb) = cfg.llm.light_base_url.as_deref().filter(|u| u.trim() != cfg.llm.base_url.trim()) {
        match list_models(lb, key.as_deref()).await {
            Ok(m) => light_models = m,
            Err(e) => return json!({"ok": false, "message": format!("Model ringan: {e}"), "models": models}),
        }
    }
    if !models.is_empty() && !models.contains(&cfg.llm.extract_model) {
        warnings.push(format!("Model utama \"{}\" tidak ada di server.", cfg.llm.extract_model));
    }
    if !light_models.is_empty() && !light_models.contains(&cfg.llm.light_model) {
        warnings.push(format!("Model ringan \"{}\" tidak ada di server.", cfg.llm.light_model));
    }
    let mut all = models;
    for m in light_models {
        if !all.contains(&m) {
            all.push(m);
        }
    }
    json!({"ok": warnings.is_empty(), "message": if warnings.is_empty() { format!("Terhubung. {} model tersedia.", all.len()) } else { warnings.join(" ") }, "models": all})
}

/// Tes server embedding: minta satu vektor dan cocokkan dimensinya.
pub async fn test_embedding(cfg: &MemoryConfig) -> Value {
    let e = &cfg.embedding;
    if e.provider == "hash" {
        return json!({"ok": true, "message": "Mode hash: tanpa server (kualitas pencarian rendah, hanya untuk uji)."});
    }
    let url = format!("{}/embeddings", e.base_url.trim_end_matches('/'));
    let mut rb = http(60).post(&url).json(&json!({"model": e.model, "input": ["tes koneksi engram"]}));
    if let Some(k) = e.resolved_api_key() {
        rb = rb.bearer_auth(k);
    }
    let resp = match rb.send().await {
        Ok(r) => r,
        Err(err) => return json!({"ok": false, "message": format!("tidak bisa menghubungi {url}: {err}")}),
    };
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return json!({"ok": false, "message": format!("{url} menjawab {status}: {}", short(&body))});
    }
    let dim = serde_json::from_str::<Value>(&body).ok().and_then(|v| v["data"][0]["embedding"].as_array().map(Vec::len));
    match dim {
        None => json!({"ok": false, "message": "Jawaban server tidak berisi embedding."}),
        Some(d) if d != e.dimension => json!({"ok": false, "dimension": d,
            "message": format!("Server mengembalikan dimensi {d}, pengaturan {}. Samakan dimensi lalu jalankan `engram reembed`.", e.dimension)}),
        Some(d) => json!({"ok": true, "dimension": d, "message": format!("Terhubung. Dimensi {d} cocok.")}),
    }
}

/// Tes server transkripsi: cukup bisa dihubungi (tidak semua server Whisper punya `/models`).
pub async fn test_transcription(cfg: &MemoryConfig) -> Value {
    let Some(base) = cfg.transcription.base_url.as_deref().filter(|u| !u.trim().is_empty()) else {
        return json!({"ok": true, "message": "Transkripsi audio tidak dipakai (alamat server kosong)."});
    };
    let url = format!("{}/models", base.trim_end_matches('/'));
    match http(10).get(&url).send().await {
        Ok(r) if r.status().is_server_error() => json!({"ok": false, "message": format!("{url} menjawab {}", r.status())}),
        Ok(_) => json!({"ok": true, "message": "Server transkripsi bisa dihubungi."}),
        Err(e) => json!({"ok": false, "message": format!("tidak bisa menghubungi {url}: {e}")}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(newer("0.0.8", "0.0.7"));
        assert!(newer("v0.1.0", "0.0.9"));
        assert!(newer("1.0", "0.9.9"));
        assert!(!newer("0.0.7", "0.0.7"));
        assert!(!newer("0.0.6", "0.0.7"));
        assert!(!newer("0.0.7-beta", "0.0.7"));
    }

    #[test]
    fn apply_validates_and_keeps_other_keys() {
        let mut t: toml::Table = "[llm]\nbase_url = \"http://a/v1\"\napi_key = \"rahasia\"\n[index]\nhnsw_m = 32\n".parse().unwrap();
        apply(&mut t, &json!({"llm": {"extract_model": "qwen", "temperature": 0.3, "api_key": null, "light_base_url": ""}})).unwrap();
        assert_eq!(t["llm"]["extract_model"].as_str(), Some("qwen"));
        assert_eq!(t["llm"]["api_key"].as_str(), Some("rahasia"), "null = rahasia tidak berubah");
        assert_eq!(t["index"]["hnsw_m"].as_integer(), Some(32), "kunci lain tetap");
        apply(&mut t, &json!({"llm": {"api_key": ""}})).unwrap();
        assert!(t["llm"].get("api_key").is_none());

        assert!(apply(&mut t, &json!({"index": {"hnsw_m": 8}})).unwrap_err().contains("tidak bisa diubah"));
        assert!(apply(&mut t, &json!({"pipeline": {"workers": 0}})).is_err());
        assert!(apply(&mut t, &json!({"llm": {"json_mode": "xml"}})).is_err());
        assert!(apply(&mut t, &json!({"llm": {"base_url": "  "}})).is_err());
    }

    #[test]
    fn secrets_are_never_returned() {
        let mut cfg = MemoryConfig::default();
        cfg.llm.api_key = Some("sk-rahasia".into());
        let v = editable_values(&cfg);
        assert_eq!(v["llm"]["api_key_set"], true);
        assert_eq!(v["embedding"]["api_key_set"], false);
        assert!(!v.to_string().contains("sk-rahasia"));
    }
}
