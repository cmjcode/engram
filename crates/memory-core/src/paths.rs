//! Lokasi berkas per pengguna di luar vault: folder data aplikasi (`id.engram.ai`) yang
//! dipakai bersama oleh CLI, hook agent, aplikasi desktop, dan klien hub.

use std::path::PathBuf;

/// Folder data aplikasi: `ENGRAM_DATA_DIR`, lalu lokasi standar OS
/// (macOS `~/Library/Application Support/id.engram.ai`, Windows `%APPDATA%\id.engram.ai`,
/// Linux `$XDG_DATA_HOME/id.engram.ai` atau `~/.local/share/id.engram.ai`).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("ENGRAM_DATA_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support").join("id.engram.ai")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join("id.engram.ai")
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local").join("share")).join("id.engram.ai")
    }
}

/// Berkas kredensial hub tim (`hub.json`, mode 0600).
pub fn hub_credentials_path() -> PathBuf {
    data_dir().join("hub.json")
}
