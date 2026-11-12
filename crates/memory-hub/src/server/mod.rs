//! Sisi server hub: router axum yang dipasang memory-server di `/hub` bila `[hub] enabled`.
//!
//! Semua data hub tinggal di `Store` (SurrealDB) yang sama dengan vault server, dalam tabel
//! berprefiks `hub_*` dengan ledger migrasi sendiri (`_meta:hub_schema`). Rute `/hub/**` tidak
//! lewat middleware `MEMORY_TOKEN`; tiap handler memakai extractor [`HubUser`] (JWT atau PAT).

pub mod audit;
pub mod auth;
pub mod authz;
pub mod error;
pub mod migrations;
pub mod models;
pub mod ratelimit;
pub mod repos;
pub mod sweep;
pub mod sync;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, header};
use axum::middleware;
use axum::response::Html;
use axum::routing::{delete, get, post, put};

use memory_core::config::HubConfig;
use memory_store::Store;

pub use auth::extract::{AuthVia, ClientIp, HubUser};
pub use auth::github::{GithubApi, ReqwestGithub};
pub use auth::providers::{Provider, ProviderClient, ProviderIdentity, ReqwestProviderClient};
pub use error::{Api, HubError, Result};
pub use migrations::HUB_TABLES;

/// Rahasia yang hanya dibaca dari env (nama env ada di [`HubConfig`]).
#[derive(Debug, Clone)]
pub struct HubSecrets {
    pub jwt: String,
    pub google_client_secret: String,
    pub github_client_secret: String,
}

impl HubSecrets {
    pub fn from_env(cfg: &HubConfig) -> Result<Self> {
        let read = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let jwt = read(&cfg.jwt_secret_env).ok_or_else(|| HubError::Internal(format!("env {} (rahasia JWT hub) kosong; isi dengan `openssl rand -hex 32`", cfg.jwt_secret_env)))?;
        if jwt.len() < 32 {
            return Err(HubError::Internal(format!("rahasia JWT di env {} terlalu pendek (minimal 32 karakter)", cfg.jwt_secret_env)));
        }
        Ok(Self { jwt, google_client_secret: read(&cfg.google_client_secret_env).unwrap_or_default(), github_client_secret: read(&cfg.github_client_secret_env).unwrap_or_default() })
    }

    pub fn for_tests() -> Self {
        Self { jwt: "rahasia-uji-yang-panjangnya-lebih-dari-32-karakter".into(), google_client_secret: "gsecret".into(), github_client_secret: "ghsecret".into() }
    }
}

#[derive(Clone)]
pub struct HubState {
    pub store: Store,
    pub cfg: Arc<HubConfig>,
    pub secrets: Arc<HubSecrets>,
    pub http: reqwest::Client,
    pub provider: Arc<dyn ProviderClient>,
    pub github: Arc<dyn GithubApi>,
    pub limiter: Arc<ratelimit::RateLimiter>,
    /// Satu push pada satu waktu per repo: menjamin urutan `seq` sesuai urutan tulis.
    push_locks: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

impl HubState {
    /// Mode produksi: rahasia dari env, provider OAuth & GitHub lewat HTTP sungguhan.
    pub async fn init(store: Store, cfg: HubConfig) -> Result<Self> {
        if cfg.base_url.trim().is_empty() {
            return Err(HubError::Internal("[hub] base_url wajib diisi (URL publik hub, mis. https://engram.tim.id)".into()));
        }
        if cfg.google_client_id.trim().is_empty() && cfg.github_client_id.trim().is_empty() {
            tracing::warn!("hub aktif tanpa provider login: isi [hub] google_client_id dan/atau github_client_id");
        }
        let secrets = HubSecrets::from_env(&cfg)?;
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent(concat!("engram-hub/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| HubError::Internal(format!("klien HTTP: {e}")))?;
        let provider: Arc<dyn ProviderClient> = Arc::new(ReqwestProviderClient::new(http.clone()));
        let github: Arc<dyn GithubApi> = Arc::new(ReqwestGithub::new(http.clone()));
        Self::with_parts(store, cfg, secrets, http, provider, github).await
    }

    /// Rakitan manual (uji: provider tiruan).
    pub async fn with_parts(store: Store, cfg: HubConfig, secrets: HubSecrets, http: reqwest::Client, provider: Arc<dyn ProviderClient>, github: Arc<dyn GithubApi>) -> Result<Self> {
        migrations::run(&store).await?;
        Ok(Self {
            store,
            cfg: Arc::new(cfg),
            secrets: Arc::new(secrets),
            http,
            provider,
            github,
            limiter: Arc::new(ratelimit::RateLimiter::default()),
            push_locks: Arc::default(),
        })
    }

    pub fn repo_lock(&self, repo_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut m = self.push_locks.lock().unwrap_or_else(|e| e.into_inner());
        m.entry(repo_id.to_string()).or_default().clone()
    }

    /// Provider login yang punya client id.
    pub fn providers(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if !self.cfg.google_client_id.trim().is_empty() {
            v.push("google");
        }
        if !self.cfg.github_client_id.trim().is_empty() {
            v.push("github");
        }
        v
    }

    pub fn redirect_uri(&self, p: Provider) -> String {
        format!("{}/hub/auth/callback/{}", self.cfg.redirect_base(), p.as_str())
    }

    /// Pembersih berkala (state OAuth/tiket/sesi kedaluwarsa, record lama yang dicabut).
    pub fn spawn_sweeper(&self) -> tokio::task::JoinHandle<()> {
        sweep::spawn(self.clone())
    }
}

/// Router hub dengan path relatif (dipasang pemanggil di `/hub`). `index_html` = UI web yang
/// sama dengan server lokal; rute JS `#hub/...` menangani halamannya.
pub fn router<S: Clone + Send + Sync + 'static>(state: HubState, index_html: &'static str) -> Router<S> {
    let public = Router::new()
        .route("/auth/login/{provider}", get(auth::handlers::login))
        .route("/auth/callback/{provider}", get(auth::handlers::callback))
        .route("/auth/ticket/poll", post(auth::handlers::poll_ticket))
        .route("/auth/refresh", post(auth::handlers::refresh))
        .route("/auth/logout", post(auth::handlers::logout))
        .layer(middleware::from_fn_with_state(state.clone(), ratelimit::auth_limit));
    let api = Router::new()
        .route("/api/info", get(auth::handlers::info))
        .route("/api/me", get(auth::handlers::me))
        .route("/api/me/tokens", get(auth::pat::list).post(auth::pat::create))
        .route("/api/me/tokens/{id}", delete(auth::pat::revoke))
        .route("/api/repos", get(repos::list).post(repos::create))
        .route("/api/repos/{id}", get(repos::get_one).patch(repos::patch))
        .route("/api/repos/{id}/join", post(repos::join))
        .route("/api/repos/{id}/key/rotate", post(repos::rotate_key))
        .route("/api/repos/{id}/members", get(repos::members))
        .route("/api/repos/{id}/members/{uid}", put(repos::set_role).delete(repos::revoke))
        .route("/api/repos/{id}/push", post(sync::push))
        .route("/api/repos/{id}/changes", get(sync::changes))
        .route("/api/repos/{id}/records", get(sync::records))
        .route("/api/repos/{id}/activity", get(repos::activity))
        .route("/api/repos/{id}/status", get(repos::status));
    let mut r = Router::new()
        .route("/", get(move || async move { Html(index_html) }))
        .merge(public)
        .merge(api)
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024));
    let origins: Vec<HeaderValue> = state.cfg.allowed_origins.iter().filter_map(|o| o.parse().ok()).collect();
    if !origins.is_empty() {
        use tower_http::cors::{Any, CorsLayer};
        r = r.layer(CorsLayer::new().allow_origin(origins).allow_methods(Any).allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]));
    }
    r.with_state(state)
}
