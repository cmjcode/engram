//! Login dari terminal/aplikasi desktop lewat tiket: buka browser ke
//! `{hub}/hub/auth/login/{provider}?ticket=…`, lalu poll `/hub/auth/ticket/poll` sampai hub
//! menaruh token di tiket itu (pola yang sama dengan rastrum-gui; tanpa listener loopback
//! karena diblokir App Sandbox macOS).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::creds::{HubCreds, with_lock};
use super::http::{HubClient, HubClientError, Result};

pub fn generate_ticket() -> String {
    crate::ids::random_hex(16)
}

pub fn login_url(hub: &str, provider: &str, ticket: &str) -> String {
    format!("{}/hub/auth/login/{provider}?ticket={ticket}", hub.trim_end_matches('/'))
}

/// Buka URL di browser bawaan. `false` bila tidak ada cara (mis. server tanpa desktop):
/// pemanggil lalu mencetak URL-nya.
pub fn open_browser(url: &str) -> bool {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("open", &[])]
    } else if cfg!(target_os = "windows") {
        &[("cmd", &["/C", "start", ""])]
    } else {
        &[("xdg-open", &[]), ("sensible-browser", &[]), ("x-www-browser", &[])]
    };
    for (bin, args) in candidates {
        let ok = std::process::Command::new(bin).args(*args).arg(url).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().is_ok();
        if ok {
            return true;
        }
    }
    false
}

/// Satu alur login yang sedang berjalan.
pub struct LoginFlow {
    pub url: String,
    pub ticket: String,
    client: HubClient,
    creds_path: PathBuf,
}

impl LoginFlow {
    pub fn start(hub: &str, provider: &str) -> Result<Self> {
        if !matches!(provider, "google" | "github") {
            return Err(HubClientError::Invalid(format!("provider tidak dikenal: {provider} (pakai google atau github)")));
        }
        let client = HubClient::anonymous(hub)?;
        let ticket = generate_ticket();
        Ok(Self { url: login_url(client.base(), provider, &ticket), ticket, client, creds_path: HubCreds::path() })
    }

    pub fn with_creds_path(mut self, p: PathBuf) -> Self {
        self.creds_path = p;
        self
    }

    /// Poll sekali: `Ok(Some)` bila token sudah ada (dan sudah disimpan), `Ok(None)` bila masih
    /// menunggu. Galat tiket (provider belum dikonfigurasi, pengguna membatalkan) jadi `Err`.
    pub async fn poll_once(&self) -> Result<Option<HubCreds>> {
        let out = self.client.poll_ticket(&self.ticket).await?;
        match out.status.as_str() {
            "completed" => {
                let t = out.token.ok_or_else(|| HubClientError::Invalid("tiket selesai tanpa token".into()))?;
                let creds = HubCreds::from_token(self.client.base(), &t);
                let (c2, p2) = (creds.clone(), self.creds_path.clone());
                with_lock(&self.creds_path, move || c2.save_to(&p2)).map_err(|e| HubClientError::Invalid(format!("menyimpan kredensial: {e}")))?;
                Ok(Some(creds))
            }
            "error" => Err(HubClientError::Api { status: 400, message: out.error.unwrap_or_else(|| "login gagal di hub".into()) }),
            _ => Ok(None),
        }
    }

    /// Tunggu sampai login selesai (poll tiap 1,5 detik). Galat jaringan dihitung masih menunggu.
    pub async fn wait(&self, timeout: Duration) -> Result<HubCreds> {
        let start = Instant::now();
        loop {
            match self.poll_once().await {
                Ok(Some(c)) => return Ok(c),
                Ok(None) | Err(HubClientError::Network(_)) => {}
                Err(e) => return Err(e),
            }
            if start.elapsed() >= timeout {
                return Err(HubClientError::Invalid("login tidak selesai dalam batas waktu; coba lagi".into()));
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_and_url() {
        let t = generate_ticket();
        assert_eq!(t.len(), 32);
        assert_eq!(login_url("https://h.x/", "google", "abc"), "https://h.x/hub/auth/login/google?ticket=abc");
        assert!(LoginFlow::start("https://h.x", "apple").is_err());
        assert!(LoginFlow::start("http://hub.tim.id", "google").is_err(), "http non-loopback ditolak");
        assert!(LoginFlow::start("http://127.0.0.1:7878", "github").is_ok());
    }
}
