//! Klien HTTP hub. Memakai PAT dari env (`ENGRAM_HUB_TOKEN` + `ENGRAM_HUB_URL`, untuk CI) atau
//! kredensial pengguna di `hub.json` yang di-refresh otomatis (rotasi refresh token).

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::creds::{HubCreds, with_lock};
use crate::types::*;

#[derive(Debug, thiserror::Error)]
pub enum HubClientError {
    #[error("belum login ke hub; jalankan `engram login --hub <url>` atau set ENGRAM_HUB_TOKEN")]
    NotLoggedIn,
    #[error("hub menolak kredensial: {0}")]
    Unauthorized(String),
    #[error("hub tidak bisa dihubungi: {0}")]
    Network(String),
    #[error("hub menjawab {status}: {message}")]
    Api { status: u16, message: String },
    #[error("{0}")]
    Invalid(String),
}

impl HubClientError {
    /// Layak diulang oleh antrean job (jaringan, 5xx, 429).
    pub fn is_retryable(&self) -> bool {
        match self {
            HubClientError::Network(_) => true,
            HubClientError::Api { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, HubClientError>;

#[derive(Debug, Clone)]
pub enum Auth {
    /// Tanpa kredensial (login, info).
    None,
    /// Personal access token (CI).
    Pat(String),
    /// Kredensial pengguna di berkas (di-refresh otomatis).
    Creds(PathBuf),
}

#[derive(Clone)]
pub struct HubClient {
    base: String,
    http: reqwest::Client,
    auth: Auth,
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("engram/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

/// URL hub wajib https kecuali loopback (uji & pengembangan lokal).
pub fn check_hub_url(url: &str) -> Result<String> {
    let u = url.trim().trim_end_matches('/').to_string();
    if u.starts_with("https://") {
        return Ok(u);
    }
    if let Some(rest) = u.strip_prefix("http://") {
        let host = rest.split(['/', ':']).next().unwrap_or("");
        if host == "127.0.0.1" || host == "localhost" || host == "[::1]" || host.starts_with("127.") {
            return Ok(u);
        }
        return Err(HubClientError::Invalid(format!("hub {u} harus memakai https (http hanya untuk 127.0.0.1)")));
    }
    Err(HubClientError::Invalid(format!("URL hub tidak valid: {url}")))
}

impl HubClient {
    pub fn new(base: &str, auth: Auth) -> Result<Self> {
        Ok(Self { base: check_hub_url(base)?, http: http_client(), auth })
    }

    /// Klien tanpa kredensial (alur login, `/hub/api/info`).
    pub fn anonymous(base: &str) -> Result<Self> {
        Self::new(base, Auth::None)
    }

    /// PAT dari env `ENGRAM_HUB_TOKEN` (+ `ENGRAM_HUB_URL`), selain itu `hub.json` pengguna.
    /// `hub_hint` = URL dari `repo.toml`/config bila env tidak menyebut URL.
    pub fn from_env_or_creds(hub_hint: Option<&str>) -> Result<Self> {
        let env = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        if let Some(tok) = env("ENGRAM_HUB_TOKEN") {
            let base = env("ENGRAM_HUB_URL").or_else(|| hub_hint.map(str::to_string)).ok_or(HubClientError::NotLoggedIn)?;
            return Self::new(&base, Auth::Pat(tok));
        }
        let path = HubCreds::path();
        let creds = HubCreds::load_from(&path).ok_or(HubClientError::NotLoggedIn)?;
        if let Some(h) = hub_hint
            && !h.trim().is_empty()
            && h.trim().trim_end_matches('/') != creds.hub
        {
            return Err(HubClientError::Invalid(format!("login saat ini ke {}, sedangkan repo memakai hub {h}; jalankan `engram login --hub {h}`", creds.hub)));
        }
        Self::new(&creds.hub, Auth::Creds(path))
    }

    /// Seperti [`Self::from_env_or_creds`] dengan berkas kredensial di lokasi lain (uji).
    pub fn with_creds_file(path: PathBuf) -> Result<Self> {
        let creds = HubCreds::load_from(&path).ok_or(HubClientError::NotLoggedIn)?;
        Self::new(&creds.hub, Auth::Creds(path))
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn is_authenticated(&self) -> bool {
        !matches!(self.auth, Auth::None)
    }

    /// Pengguna yang sedang login (dari berkas kredensial), tanpa panggilan jaringan.
    pub fn local_user(&self) -> Option<UserOut> {
        match &self.auth {
            Auth::Creds(p) => HubCreds::load_from(p).map(|c| c.user),
            _ => None,
        }
    }

    async fn bearer(&self) -> Result<Option<String>> {
        match &self.auth {
            Auth::None => Ok(None),
            Auth::Pat(t) => Ok(Some(t.clone())),
            Auth::Creds(path) => {
                let mut creds = HubCreds::load_from(path).ok_or(HubClientError::NotLoggedIn)?;
                if !creds.needs_refresh() {
                    return Ok(Some(creds.access_token));
                }
                // Box::pin memutus rekursi async send -> bearer -> refresh -> send (E0733).
                match Box::pin(self.refresh(&creds.refresh_token)).await {
                    Ok(t) => {
                        creds.apply(&t);
                        let c2 = creds.clone();
                        let p2 = path.clone();
                        with_lock(path, move || c2.save_to(&p2)).map_err(|e| HubClientError::Invalid(format!("menyimpan kredensial: {e}")))?;
                        Ok(Some(creds.access_token))
                    }
                    Err(HubClientError::Unauthorized(m)) => {
                        // Refresh token dicabut/kedaluwarsa: kredensial dibuang agar pengguna login ulang.
                        let _ = HubCreds::clear_at(path);
                        Err(HubClientError::Unauthorized(format!("{m}; jalankan `engram login`")))
                    }
                    Err(HubClientError::Network(_)) if creds.access_exp > chrono::Utc::now().timestamp() => Ok(Some(creds.access_token)),
                    Err(e) => Err(e),
                }
            }
        }
    }

    async fn send<T: DeserializeOwned, B: Serialize + ?Sized>(&self, method: reqwest::Method, path: &str, body: Option<&B>, auth: bool) -> Result<T> {
        let mut req = self.http.request(method, format!("{}{path}", self.base));
        if auth && let Some(tok) = self.bearer().await? {
            req = req.bearer_auth(tok);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(|e| HubClientError::Network(e.to_string()))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| HubClientError::Network(e.to_string()))?;
        if !status.is_success() {
            let message = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["error"].as_str().map(str::to_string)).unwrap_or_else(|| text.chars().take(200).collect());
            return Err(match status.as_u16() {
                401 => HubClientError::Unauthorized(message),
                s => HubClientError::Api { status: s, message },
            });
        }
        serde_json::from_str(&text).map_err(|e| HubClientError::Invalid(format!("balasan hub tidak terbaca: {e}")))
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.send::<T, ()>(reqwest::Method::GET, path, None, true).await
    }

    async fn post<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Result<T> {
        self.send(reqwest::Method::POST, path, Some(body), true).await
    }

    // ------------------------------------------------------------------ publik

    pub async fn info(&self) -> Result<InfoOut> {
        self.send::<InfoOut, ()>(reqwest::Method::GET, "/hub/api/info", None, false).await
    }

    pub async fn poll_ticket(&self, ticket: &str) -> Result<PollOut> {
        self.send(reqwest::Method::POST, "/hub/auth/ticket/poll", Some(&serde_json::json!({"ticket": ticket})), false).await
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenOut> {
        self.send(reqwest::Method::POST, "/hub/auth/refresh", Some(&serde_json::json!({"refresh_token": refresh_token})), false).await
    }

    pub async fn logout(&self, refresh_token: &str) -> Result<()> {
        let _: Value = self.send(reqwest::Method::POST, "/hub/auth/logout", Some(&serde_json::json!({"refresh_token": refresh_token})), false).await?;
        Ok(())
    }

    // -------------------------------------------------------------- terautentikasi

    pub async fn me(&self) -> Result<MeOut> {
        self.get("/hub/api/me").await
    }

    pub async fn create_repo(&self, body: &RepoCreateIn) -> Result<RepoCreateOut> {
        self.post("/hub/api/repos", body).await
    }

    pub async fn list_repos(&self) -> Result<Vec<RepoOut>> {
        self.get("/hub/api/repos").await
    }

    pub async fn get_repo(&self, id: &str) -> Result<RepoOut> {
        self.get(&format!("/hub/api/repos/{id}")).await
    }

    pub async fn join(&self, id: &str, body: &JoinIn) -> Result<JoinOut> {
        self.post(&format!("/hub/api/repos/{id}/join"), body).await
    }

    pub async fn rotate_key(&self, id: &str) -> Result<RotateOut> {
        self.post(&format!("/hub/api/repos/{id}/key/rotate"), &serde_json::json!({})).await
    }

    pub async fn push(&self, id: &str, body: &PushIn) -> Result<PushOut> {
        self.post(&format!("/hub/api/repos/{id}/push"), body).await
    }

    pub async fn changes(&self, id: &str, since: u64, limit: usize) -> Result<ChangesOut> {
        self.get(&format!("/hub/api/repos/{id}/changes?since={since}&limit={limit}")).await
    }

    pub async fn status(&self, id: &str) -> Result<StatusOut> {
        self.get(&format!("/hub/api/repos/{id}/status")).await
    }

    pub async fn members(&self, id: &str) -> Result<Vec<MemberOut>> {
        self.get(&format!("/hub/api/repos/{id}/members")).await
    }

    pub async fn leave(&self, id: &str, user_id: &str) -> Result<()> {
        let _: Value = self.send::<Value, ()>(reqwest::Method::DELETE, &format!("/hub/api/repos/{id}/members/{user_id}"), None, true).await?;
        Ok(())
    }
}
