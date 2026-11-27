//! Sinkronisasi vault lewat server engram (mis. `https://engram.operon.id`).
//!
//! Pengguna cukup mengisi URL server lalu klik Sync; login GitHub/Google lewat hub server itu
//! dilakukan sekali (kredensial di `<vault>/sync_server.json`, terpisah dari login hub tim).
//! Server adalah satu "perangkat" bersama: ia menggabungkan snapshot tiap komputer ke vault-nya
//! sendiri dan memberikan snapshot vault-nya, jadi semua komputer bertemu lewat server.
//!
//! Satu kali sync (sisi klien):
//! 1. `GET  /api/vault-sync/snapshot` (`If-None-Match` = hash terakhir) lalu gabung per record
//!    (aturan sama dengan sync folder, lihat [`crate::sync`]);
//! 2. unduh berkas sumber yang belum ada (`/api/vault-sync/sources`);
//! 3. hitung embedding record yang berubah;
//! 4. `POST /api/vault-sync/snapshot` bila snapshot komputer ini berubah; server menggabungkannya;
//! 5. unggah berkas sumber yang belum ada di server.
//!
//! Embedding tidak dikirim (dihitung ulang di penerima); penghapusan tidak ikut tersinkron.

use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use memory_core::ids::new_id;
use memory_extract::Vault;
use memory_hub::client::{HubClient, HubClientError, HubCreds};

use crate::sync::{self, SyncReport, SyncSettings, SyncState};
use crate::{JobError, Result, Runtime};

/// Kredensial login ke server sync (di vault, mode 0600).
pub const CREDS_FILE: &str = "sync_server.json";
/// Awalan pesan galat yang berarti pengguna harus login (lagi); UI memulai login otomatis.
pub const NEED_LOGIN: &str = "perlu login ke server sync";

pub const H_HASH: &str = "x-engram-hash";
pub const H_ROWS: &str = "x-engram-rows";
pub const H_SCHEMA: &str = "x-engram-schema";
pub const H_DEVICE: &str = "x-engram-device";

pub fn creds_path(vault: &Vault) -> PathBuf {
    vault.root().join(CREDS_FILE)
}

/// `engram.operon.id` -> `https://engram.operon.id`; wajib https (kecuali loopback).
pub fn normalize_url(raw: &str) -> Result<String> {
    let raw = raw.trim();
    let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("https://{raw}") };
    memory_hub::client::http::check_hub_url(&with_scheme).map_err(|e| JobError::Invalid(e.to_string()))
}

/// Email akun yang login ke server `url`, bila ada.
pub fn account(vault: &Vault, url: &str) -> Option<String> {
    let c = HubCreds::load_from(&creds_path(vault))?;
    let url = normalize_url(url).ok()?;
    (c.hub == url).then_some(if c.user.email.is_empty() { c.user.name } else { c.user.email })
}

/// Keluar dari server sync (kredensial dihapus; pengaturan URL tetap).
pub fn logout(vault: &Vault) -> Result<()> {
    HubCreds::clear_at(&creds_path(vault))?;
    Ok(())
}

fn need_login(detail: impl std::fmt::Display) -> JobError {
    JobError::Hub { msg: format!("{NEED_LOGIN}: {detail}"), retry: false }
}

fn net(e: impl std::fmt::Display) -> JobError {
    JobError::Hub { msg: format!("server sync tidak bisa dihubungi: {e}"), retry: true }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(15))
        .user_agent(concat!("engram/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

/// Ubah balasan non-2xx jadi galat yang bisa dibaca pengguna.
async fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() || status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(resp);
    }
    let text = resp.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v["error"].as_str().map(str::to_string)).unwrap_or_else(|| text.chars().take(200).collect());
    Err(match status.as_u16() {
        401 => need_login(msg),
        403 => JobError::Hub { msg: format!("server menolak akun ini: {msg}"), retry: false },
        404 => JobError::Hub { msg: "server ini belum mendukung sync (perbarui engram di server)".into(), retry: false },
        s => JobError::Hub { msg: format!("server menjawab {s}: {msg}"), retry: s == 429 || s >= 500 },
    })
}

/// Satu kali sync dengan server `url` (dipanggil dari [`crate::sync::sync_now`]).
pub(crate) async fn run(rt: &Runtime, url: &str, settings: &SyncSettings, st: &mut SyncState, rep: &mut SyncReport) -> Result<()> {
    let url = normalize_url(url)?;
    let hub = HubClient::with_creds_file(creds_path(&rt.vault)).map_err(|_| need_login("belum login"))?;
    if hub.base() != url {
        return Err(need_login(format!("login tersimpan untuk {}", hub.base())));
    }
    let token = match hub.access_token().await {
        Ok(Some(t)) => t,
        Ok(None) | Err(HubClientError::NotLoggedIn) => return Err(need_login("belum login")),
        Err(HubClientError::Unauthorized(m)) => return Err(need_login(m)),
        Err(HubClientError::Network(m)) => return Err(net(m)),
        Err(e) => return Err(JobError::Hub { msg: e.to_string(), retry: false }),
    };
    if st.server_url.as_deref() != Some(url.as_str()) {
        // Server baru: tarik & unggah semuanya.
        st.server_url = Some(url.clone());
        st.server_pulled = None;
        st.server_push_hash = None;
    }
    let c = http();
    let api = |p: &str| format!("{url}/api/vault-sync{p}");
    let host = url.split("://").nth(1).unwrap_or(&url).to_string();

    // 1. tarik snapshot server
    let mut req = c.get(api("/snapshot")).bearer_auth(&token);
    if let Some(h) = &st.server_pulled {
        req = req.header(reqwest::header::IF_NONE_MATCH, h.as_str());
    }
    let resp = check(req.send().await.map_err(net)?).await?;
    if resp.status() != reqwest::StatusCode::NOT_MODIFIED {
        let schema: u32 = resp.headers().get(H_SCHEMA).and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(0);
        let want = resp.headers().get(H_HASH).and_then(|v| v.to_str().ok()).map(str::to_string);
        if schema > memory_core::SCHEMA_VERSION {
            rep.warnings.push(format!("{host}: server memakai engram versi lebih baru; perbarui aplikasi di komputer ini"));
        } else {
            let body = resp.bytes().await.map_err(net)?;
            let got = sync::apply_lines(rt, flate2::read::GzDecoder::new(&body[..]), st, rep).await?;
            if want.as_deref().is_some_and(|w| w != got) {
                rep.warnings.push(format!("{host}: snapshot server tidak utuh, dicoba lagi nanti"));
            } else {
                st.server_pulled = Some(got);
                rep.pulled_from.push(host.clone());
            }
        }
    }

    // 2. berkas sumber dari server
    let remote: Vec<String> = check(c.get(api("/sources")).bearer_auth(&token).send().await.map_err(net)?).await?.json::<serde_json::Value>().await.map_err(net)?["files"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let src = rt.vault.sources_dir();
    for rel in &remote {
        let Ok(dst) = source_path(&src, rel) else { continue };
        if dst.exists() {
            continue;
        }
        let bytes = check(c.get(api(&format!("/sources/{rel}"))).bearer_auth(&token).send().await.map_err(net)?).await?.bytes().await.map_err(net)?;
        write_new(&dst, &bytes)?;
        rep.source_files_in += 1;
    }

    // 3. embedding record yang berubah
    sync::embed_pending(rt, st, rep).await;

    // 4. unggah snapshot komputer ini bila berubah
    std::fs::create_dir_all(rt.vault.exports_dir())?;
    let plain = rt.vault.exports_dir().join(format!(".sync-server-{}.gz", new_id()));
    let snap = sync::write_snapshot(rt, &plain).await;
    let bytes = std::fs::read(&plain);
    let _ = std::fs::remove_file(&plain);
    let (hash, rows) = snap?;
    if st.server_push_hash.as_deref() != Some(hash.as_str()) {
        let resp = c
            .post(api("/snapshot"))
            .bearer_auth(&token)
            .header(H_HASH, &hash)
            .header(H_ROWS, rows)
            .header(H_SCHEMA, memory_core::SCHEMA_VERSION)
            .header(H_DEVICE, sync::device_name(settings))
            .header(reqwest::header::CONTENT_TYPE, "application/gzip")
            .body(bytes?)
            .send()
            .await
            .map_err(net)?;
        check(resp).await?;
        st.server_push_hash = Some(hash);
        rep.pushed = true;
    }

    // 5. berkas sumber yang belum ada di server
    let have: std::collections::BTreeSet<&str> = remote.iter().map(String::as_str).collect();
    for rel in list_sources(&src) {
        if have.contains(rel.as_str()) {
            continue;
        }
        let path = source_path(&src, &rel)?;
        let resp = c.put(api(&format!("/sources/{rel}"))).bearer_auth(&token).body(std::fs::read(&path)?).send().await.map_err(net)?;
        check(resp).await?;
        rep.source_files_out += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------- sisi server

/// Snapshot vault server untuk dikirim ke klien.
pub struct ServerSnapshot {
    /// jsonl.gz
    pub bytes: Vec<u8>,
    pub hash: String,
    pub rows: u64,
}

/// Tulis snapshot vault ini (tanpa tabel lokal & embedding) ke memori.
pub async fn export_snapshot(rt: &Runtime) -> Result<ServerSnapshot> {
    std::fs::create_dir_all(rt.vault.exports_dir())?;
    let tmp = rt.vault.exports_dir().join(format!(".vault-sync-{}.gz", new_id()));
    let res = sync::write_snapshot(rt, &tmp).await;
    let bytes = std::fs::read(&tmp);
    let _ = std::fs::remove_file(&tmp);
    let (hash, rows) = res?;
    Ok(ServerSnapshot { bytes: bytes?, hash, rows })
}

/// Gabungkan snapshot kiriman klien (jsonl.gz) ke vault server.
pub async fn import_snapshot(rt: &Runtime, gz: &[u8], device: &str, expected_hash: Option<&str>) -> Result<SyncReport> {
    // Tunggu sync lain (klien lain / jadwal) selesai; jangan menolak.
    let _guard = rt.sync_lock.lock().await;
    let mut st = sync::load_state(&rt.vault);
    let mut rep = SyncReport::default();
    let got = sync::apply_lines(rt, flate2::read::GzDecoder::new(gz), &mut st, &mut rep).await;
    // Record yang sudah terbaca tetap valid walau snapshot terpotong: embedding tetap dihitung.
    sync::embed_pending(rt, &mut st, &mut rep).await;
    rep.pending_embed = st.pending_embed.len() as u64;
    sync::save_state(&rt.vault, &st)?;
    let got = got?;
    if expected_hash.is_some_and(|h| h != got) {
        return Err(JobError::Invalid("snapshot tidak utuh (hash tidak cocok); kirim ulang".into()));
    }
    rep.pulled_from.push(device.to_string());
    tracing::info!(device, added = rep.added, updated = rep.updated, "sync server: snapshot klien digabung");
    Ok(rep)
}

/// Path relatif berkas sumber (pemisah `/`), dialamatkan hash sehingga tidak pernah berubah.
pub fn list_sources(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if hidden(&name) {
                continue;
            }
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(&e.path(), &rel, out),
                Ok(t) if t.is_file() => out.push(rel),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", &mut out);
    out.sort();
    out
}

fn hidden(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('~') || name.ends_with(".tmp")
}

/// Path berkas sumber `rel` di bawah `root`; menolak `..`, path absolut, dan berkas tersembunyi.
pub fn source_path(root: &Path, rel: &str) -> Result<PathBuf> {
    let bad = || JobError::Invalid(format!("path sumber tidak valid: {rel}"));
    if rel.is_empty() || rel.contains('\\') || rel.len() > 512 {
        return Err(bad());
    }
    let p = Path::new(rel);
    for c in p.components() {
        match c {
            Component::Normal(n) if !hidden(&n.to_string_lossy()) => {}
            _ => return Err(bad()),
        }
    }
    Ok(root.join(p))
}

/// Simpan berkas sumber baru (atomik). `false` bila sudah ada (isi tidak ditimpa).
pub fn write_new(dst: &Path, bytes: &[u8]) -> Result<bool> {
    if dst.exists() {
        return Ok(false);
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = sync::tmp_path(dst);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dst)?;
    Ok(true)
}

/// Baca berkas sumber `rel` di vault (untuk diunduh klien).
pub fn read_source(vault: &Vault, rel: &str) -> Result<Option<Vec<u8>>> {
    let p = source_path(&vault.sources_dir(), rel)?;
    if !p.is_file() {
        return Ok(None);
    }
    let mut f = std::fs::File::open(p)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(Some(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_and_paths() {
        assert_eq!(normalize_url("engram.operon.id").unwrap(), "https://engram.operon.id");
        assert_eq!(normalize_url(" https://engram.operon.id/ ").unwrap(), "https://engram.operon.id");
        assert!(normalize_url("http://engram.operon.id").is_err());
        let root = Path::new("/v/sources");
        assert!(source_path(root, "ab/abcdef.pdf").is_ok());
        for bad in ["../x", "/etc/passwd", "a/../../x", ".hidden", "a/.x.tmp", "", "a\\b"] {
            assert!(source_path(root, bad).is_err(), "{bad}");
        }
    }
}
