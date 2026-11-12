//! Kredensial hub milik pengguna di `<data_dir>/hub.json` (mode 0600, tulis atomik).
//! Dibaca CLI, server lokal (`engram serve`), dan aplikasi desktop; karena beberapa proses
//! bisa me-refresh token bersamaan, pembaruan dilindungi berkas kunci `hub.json.lock`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::types::{TokenOut, UserOut};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct HubCreds {
    /// URL dasar hub tanpa garis miring akhir (mis. `https://engram.tim.id`).
    pub hub: String,
    pub user: UserOut,
    pub access_token: String,
    /// Kedaluwarsa access token (epoch detik).
    pub access_exp: i64,
    pub refresh_token: String,
}

impl HubCreds {
    pub fn path() -> PathBuf {
        memory_core::paths::hub_credentials_path()
    }

    pub fn from_token(hub: &str, t: &TokenOut) -> Self {
        Self {
            hub: hub.trim_end_matches('/').to_string(),
            user: t.user.clone(),
            access_token: t.access_token.clone(),
            access_exp: chrono::Utc::now().timestamp() + t.expires_in,
            refresh_token: t.refresh_token.clone(),
        }
    }

    pub fn load() -> Option<Self> {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &Path) -> Option<Self> {
        let s = std::fs::read_to_string(path).ok()?;
        serde_json::from_str::<Self>(&s).ok().filter(|c| !c.hub.is_empty() && !c.refresh_token.is_empty())
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts.open(&tmp)?;
            use std::io::Write;
            f.write_all(&body)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn clear() -> std::io::Result<()> {
        Self::clear_at(&Self::path())
    }

    /// Buang berkas kredensial (logout); tidak ada berkas = sudah bersih.
    pub fn clear_at(path: &Path) -> std::io::Result<()> {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }

    /// Access token habis (atau habis dalam 60 detik).
    pub fn needs_refresh(&self) -> bool {
        self.access_exp - 60 < chrono::Utc::now().timestamp()
    }

    pub fn apply(&mut self, t: &TokenOut) {
        self.user = t.user.clone();
        self.access_token = t.access_token.clone();
        self.access_exp = chrono::Utc::now().timestamp() + t.expires_in;
        self.refresh_token = t.refresh_token.clone();
    }
}

/// Jalankan `f` sambil memegang kunci `<path>.lock` (maks. tunggu 2 detik; kunci basi > 30 detik
/// dianggap milik proses yang mati dan diambil alih).
pub fn with_lock<R>(path: &Path, f: impl FnOnce() -> R) -> R {
    let lock = path.with_extension("json.lock");
    let start = Instant::now();
    let mut held = false;
    while start.elapsed() < Duration::from_secs(2) {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
            Ok(_) => {
                held = true;
                break;
            }
            Err(_) => {
                if std::fs::metadata(&lock).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > Duration::from_secs(30)) {
                    let _ = std::fs::remove_file(&lock);
                    continue;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    let out = f();
    if held {
        let _ = std::fs::remove_file(&lock);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_refresh_window() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("hub.json");
        let t = TokenOut { access_token: "a".into(), refresh_token: "r".into(), expires_in: 3600, user: UserOut { id: "u1".into(), email: "a@b.c".into(), name: "A".into(), avatar_url: None } };
        let c = HubCreds::from_token("https://hub.example/", &t);
        assert_eq!(c.hub, "https://hub.example");
        assert!(!c.needs_refresh());
        with_lock(&p, || c.save_to(&p).unwrap());
        assert_eq!(HubCreds::load_from(&p).unwrap(), c);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let mut e = c.clone();
        e.access_exp = chrono::Utc::now().timestamp() + 30;
        assert!(e.needs_refresh());
        HubCreds::clear_at(&p).unwrap();
        assert!(HubCreds::load_from(&p).is_none());
        HubCreds::clear_at(&p).unwrap();
    }
}
