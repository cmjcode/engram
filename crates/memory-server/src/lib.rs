//! Server lokal: HTTP API (dipakai UI web), MCP untuk agent AI, dan UI statis.
//!
//! Default hanya mendengarkan di 127.0.0.1. Bila env var token (default
//! `MEMORY_TOKEN`) diisi, semua `/api` dan `/mcp` wajib `Authorization: Bearer`.

pub mod api;
pub mod install;
pub mod mcp;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::json;

use memory_import::Importer;
use memory_jobs::Runtime;

#[derive(Clone)]
pub struct AppState {
    pub rt: Runtime,
    pub importer: Importer,
    pub token: Option<Arc<String>>,
    /// Alamat MCP HTTP server ini (mis. `http://127.0.0.1:7878/mcp`). Bila diisi, UI bisa
    /// memasang server ke config agent AI di komputer ini (lihat [`install`]).
    pub mcp_url: Option<String>,
    /// Pengendali layanan latar (worker, scheduler, inbox, server). Hanya diisi
    /// aplikasi desktop; `engram serve` membiarkannya kosong.
    pub services: Option<Arc<dyn ServiceControl>>,
}

/// Status satu layanan latar untuk ditampilkan di UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ServiceInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub running: bool,
}

/// Dipasang oleh aplikasi yang menjalankan layanan agar UI bisa melihat status
/// dan menyalakan ulang layanan yang berhenti.
pub trait ServiceControl: Send + Sync {
    fn list(&self) -> Vec<ServiceInfo>;
    /// Jalankan layanan `id` bila sedang berhenti. Tidak berbuat apa-apa bila sudah jalan.
    fn start(&self, id: &str) -> Result<(), String>;
    /// Bisa membuka dialog pilih folder & memindah vault (aplikasi desktop).
    fn can_manage_storage(&self) -> bool {
        false
    }
    /// Dialog pilih folder bawaan sistem (memblokir sampai pengguna memilih).
    /// `None` bila dibatalkan atau tidak didukung.
    fn pick_folder(&self, _title: &str) -> Option<std::path::PathBuf> {
        None
    }
    /// Pindahkan vault ke `target` (sudah diperiksa [`memory_jobs::storage::plan_move`]).
    /// Aplikasi menutup database, menyalin vault, lalu dibuka ulang.
    fn move_vault(&self, _target: &std::path::Path) -> Result<String, String> {
        Err("pemindahan lokasi data hanya tersedia di aplikasi desktop; pakai `engram vault move`".into())
    }
}

impl AppState {
    pub fn new(rt: Runtime, importer: Importer) -> Self {
        let token = rt.cfg.server.token_env.as_deref().and_then(|k| std::env::var(k).ok()).filter(|t| !t.is_empty()).map(Arc::new);
        Self { rt, importer, token, mcp_url: None, services: None }
    }

    pub fn default_subject(&self) -> String {
        self.rt.cfg.vault.default_subject.clone()
    }

    pub async fn resolve_subject(&self, requested: Option<&str>) -> String {
        if let Some(s) = requested.filter(|s| !s.trim().is_empty()) {
            return s.to_string();
        }
        let def = self.default_subject();
        if def != "default" {
            return def;
        }
        if let Ok(subs) = self.rt.store.list_subjects().await {
            if subs.iter().any(|s| s.id == "default") {
                return def;
            }
            if let Some(first) = subs.first() {
                return first.id.clone();
            }
        }
        def
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Internal(String),
}

macro_rules! from_err {
    ($($t:ty),*) => {$(
        impl From<$t> for ApiError {
            fn from(e: $t) -> Self {
                ApiError::Internal(e.to_string())
            }
        }
    )*};
}
from_err!(memory_store::StoreError, memory_extract::ExtractError, memory_retrieve::RetrieveError, memory_jobs::JobError, memory_import::ImportError, std::io::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "galat API");
        }
        (status, axum::Json(json!({"error": self.to_string()}))).into_response()
    }
}

pub type ApiResult<T> = std::result::Result<axum::Json<T>, ApiError>;

async fn auth(State(st): State<AppState>, req: Request, next: Next) -> Response {
    if let Some(token) = &st.token {
        let ok = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .is_some_and(|t| t == token.as_str());
        if !ok {
            return (StatusCode::UNAUTHORIZED, axum::Json(json!({"error": "token tidak valid"}))).into_response();
        }
    }
    next.run(req).await
}

const INDEX_HTML: &str = include_str!("../ui/index.html");
const APP_JS: &str = include_str!("../ui/app.js");
const APP_CSS: &str = include_str!("../ui/app.css");

fn asset(body: &'static str, ctype: &'static str) -> Response {
    let mut r = body.into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(ctype));
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    r
}

pub fn router(state: AppState) -> Router {
    let max = state.rt.cfg.server.max_upload_mb * 1024 * 1024;
    let protected = api::routes()
        .route("/mcp", post(mcp::http_handler))
        .layer(middleware::from_fn_with_state(state.clone(), auth))
        .layer(axum::extract::DefaultBodyLimit::max(max));
    Router::new()
        .route("/", get(|| async { asset(INDEX_HTML, "text/html; charset=utf-8") }))
        .route("/app.js", get(|| async { asset(APP_JS, "text/javascript; charset=utf-8") }))
        .route("/app.css", get(|| async { asset(APP_CSS, "text/css; charset=utf-8") }))
        .route("/api/health", get(|| async { axum::Json(json!({"ok": true, "version": env!("CARGO_PKG_VERSION")})) }))
        .merge(protected)
        .with_state(state)
}

/// Jalankan server sampai `shutdown` bernilai true.
pub async fn serve(state: AppState, bind: &str, shutdown: tokio::sync::watch::Receiver<bool>) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    serve_listener(state, listener, shutdown).await
}

/// Seperti [`serve`], tetapi dengan listener yang sudah dibuka (dipakai aplikasi desktop).
pub async fn serve_listener(state: AppState, listener: tokio::net::TcpListener, mut shutdown: tokio::sync::watch::Receiver<bool>) -> std::io::Result<()> {
    tracing::info!("UI & API: http://{}", listener.local_addr()?);
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            let _ = shutdown.changed().await;
        })
        .await
}

#[cfg(test)]
mod tests;
