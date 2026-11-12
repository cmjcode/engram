//! Server lokal: HTTP API (dipakai UI web), MCP untuk agent AI, dan UI statis.
//!
//! Default hanya mendengarkan di 127.0.0.1. Bila env var token (default
//! `MEMORY_TOKEN`) diisi, semua `/api` dan `/mcp` wajib `Authorization: Bearer`.
//!
//! Isolasi tenant: env var `MEMORY_SUBJECT_TOKENS` (`subjekA=tokenA,subjekB=tokenB`)
//! memberi token yang hanya berlaku untuk satu subjek. Token global tetap bisa
//! semua; token subjek hanya boleh menyentuh `/api/subjects/{s}/...` dengan `s`
//! yang sama, `/api/subjects` (POST id yang sama, GET hanya subjeknya), `/mcp`
//! (argumen `subject` dipaksa), serta `/api/reviews/{id}` dan `/api/sources/{id}`
//! milik subjek itu. Request lain (settings, storage, update, services, dll.)
//! ditolak 403.

pub mod api;
pub mod hooks;
#[cfg(feature = "hub")]
pub mod hub_local;
pub mod install;
pub mod mcp;
pub mod settings;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
#[cfg(feature = "ui")]
use axum::http::HeaderValue;
use axum::http::{StatusCode, header};
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
    /// Token per subjek (isolasi tenant): token → id subjek. Lihat [`parse_subject_tokens`].
    pub subject_tokens: Arc<std::collections::BTreeMap<String, String>>,
    /// Alamat MCP HTTP server ini (mis. `http://127.0.0.1:7878/mcp`). Bila diisi, UI bisa
    /// memasang server ke config agent AI di komputer ini (lihat [`install`]).
    pub mcp_url: Option<String>,
    /// Pengendali layanan latar (worker, scheduler, inbox, server). Hanya diisi
    /// aplikasi desktop; `engram serve` membiarkannya kosong.
    pub services: Option<Arc<dyn ServiceControl>>,
    /// Berkas `memory.toml` yang dipakai. Bila diisi, UI bisa mengubah pengaturan
    /// (berlaku setelah dijalankan ulang). Kosong = pengaturan hanya bisa dilihat.
    pub config_path: Option<std::path::PathBuf>,
    /// Event hook terakhir per client (untuk panel kesehatan integrasi), hanya di memori.
    pub hook_stats: Arc<std::sync::Mutex<std::collections::BTreeMap<String, serde_json::Value>>>,
    /// Mode hub tim (`engram serve --hub` / `[hub] enabled`): rute `/hub/*`, akun Google/GitHub,
    /// repo bersama. Kosong = server pribadi biasa.
    #[cfg(feature = "hub")]
    pub hub: Option<memory_hub::HubState>,
    /// Alur login hub yang sedang ditunggu (UI lokal/desktop): tiket -> status.
    #[cfg(feature = "hub")]
    pub hub_logins: Arc<std::sync::Mutex<std::collections::BTreeMap<String, serde_json::Value>>>,
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
    /// Jalankan semua layanan latar yang sedang berhenti; kembalikan daftar ID layanan yang baru dijalankan.
    fn start_all(&self) -> Result<Vec<String>, String> {
        let mut started = Vec::new();
        for s in self.list() {
            if !s.running {
                self.start(&s.id)?;
                started.push(s.id);
            }
        }
        Ok(started)
    }
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
    /// Cek update lewat updater aplikasi (memblokir). `None` = tidak didukung;
    /// server lalu memeriksa rilis GitHub sendiri.
    fn check_update(&self) -> Option<Result<settings::UpdateInfo, String>> {
        None
    }
    /// Unduh & pasang update lalu buka ulang aplikasi (berjalan di latar).
    fn install_update(&self) -> Result<String, String> {
        Err("pasang update hanya tersedia di aplikasi desktop; unduh rilis baru dari GitHub".into())
    }
    /// Kemajuan pengunduhan & pemasangan update saat ini.
    fn update_progress(&self) -> Option<settings::UpdateProgress> {
        None
    }
    /// Cek update otomatis aktif? `None` = tidak ada pengaturan ini.
    fn auto_update(&self) -> Option<bool> {
        None
    }
    fn set_auto_update(&self, _on: bool) -> Result<(), String> {
        Err("pengaturan ini hanya ada di aplikasi desktop".into())
    }
    /// Buka ulang aplikasi (agar pengaturan baru berlaku).
    fn restart(&self) -> Result<String, String> {
        Err("jalankan ulang `engram serve` agar pengaturan baru berlaku".into())
    }
    /// Buka folder/berkas di pengelola berkas, atau URL di browser.
    fn open(&self, _target: &str) -> Result<(), String> {
        Err("hanya tersedia di aplikasi desktop".into())
    }
    /// Bahasa UI aplikasi (kode seperti "en"/"id"). `None` = tidak ada pengaturan ini.
    fn language(&self) -> Option<String> {
        None
    }
    /// Simpan bahasa pilihan pengguna agar menu & dialog bawaan aplikasi ikut berganti.
    fn set_language(&self, _lang: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Subjek yang terikat ke token request saat ini (dipasang middleware [`auth`] untuk
/// token per subjek; tidak ada untuk token global / tanpa token).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundSubject(pub String);

/// Urai isi env `MEMORY_SUBJECT_TOKENS`: `subjekA=tokenA,subjekB=tokenB` (spasi di
/// sekitar koma/`=` diabaikan; pasangan tanpa `=`, subjek tidak valid, atau token
/// kosong dilewati dengan peringatan). Hasil: token → subjek.
pub fn parse_subject_tokens(raw: &str) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    for pair in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let Some((subject, token)) = pair.split_once('=').map(|(s, t)| (s.trim(), t.trim())) else {
            tracing::warn!(pair, "MEMORY_SUBJECT_TOKENS: pasangan tanpa '=' dilewati");
            continue;
        };
        if !memory_core::ids::is_valid_id(subject) || token.is_empty() {
            tracing::warn!(subject, "MEMORY_SUBJECT_TOKENS: subjek tidak valid atau token kosong, dilewati");
            continue;
        }
        if map.insert(token.to_string(), subject.to_string()).is_some() {
            tracing::warn!(subject, "MEMORY_SUBJECT_TOKENS: token yang sama dipakai dua subjek; yang terakhir menang");
        }
    }
    map
}

impl AppState {
    pub fn new(rt: Runtime, importer: Importer) -> Self {
        let token = rt.cfg.server.token_env.as_deref().and_then(|k| std::env::var(k).ok()).filter(|t| !t.is_empty()).map(Arc::new);
        let subject_tokens = rt.cfg.server.subject_tokens_env.as_deref().and_then(|k| std::env::var(k).ok()).map(|v| parse_subject_tokens(&v)).unwrap_or_default();
        Self {
            rt,
            importer,
            token,
            subject_tokens: Arc::new(subject_tokens),
            mcp_url: None,
            services: None,
            config_path: None,
            hook_stats: Arc::default(),
            #[cfg(feature = "hub")]
            hub: None,
            #[cfg(feature = "hub")]
            hub_logins: Arc::default(),
        }
    }

    /// State untuk aplikasi yang meng-embed engram dan memanggil [`router`]
    /// in-process (tanpa socket): auth token dimatikan karena tidak ada
    /// jaringan yang dilewati; isolasi subjek menjadi tanggung jawab pemanggil.
    pub fn embedded(rt: Runtime, importer: Importer) -> Self {
        let mut st = Self::new(rt, importer);
        st.token = None;
        st.subject_tokens = Arc::default();
        st
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
    /// Token per subjek mencoba menyentuh subjek/record lain.
    #[error("{0}")]
    Forbidden(String),
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
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "galat API");
        }
        (status, axum::Json(json!({"error": self.to_string()}))).into_response()
    }
}

pub type ApiResult<T> = std::result::Result<axum::Json<T>, ApiError>;

/// Apakah path boleh disentuh token yang terikat ke `subject`. Path `/api/subjects`,
/// `/mcp`, `/api/reviews/{id}` dan `/api/sources/{id}...` diloloskan ke handler, yang
/// memeriksa sendiri lewat [`BoundSubject`] (id body / argumen MCP / pemilik record).
fn subject_token_allows(path: &str, subject: &str) -> bool {
    if path == "/mcp" || path == "/api/subjects" {
        return true;
    }
    if let Some(rest) = path.strip_prefix("/api/subjects/") {
        return rest.split('/').next() == Some(subject);
    }
    let is_record = |prefix: &str| path.strip_prefix(prefix).is_some_and(|rest| !rest.is_empty() && rest.split('/').next().is_some_and(|id| !id.is_empty()));
    is_record("/api/reviews/") || is_record("/api/sources/")
}

async fn auth(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    if st.token.is_none() && st.subject_tokens.is_empty() {
        return next.run(req).await;
    }
    let bearer = req.headers().get(header::AUTHORIZATION).and_then(|h| h.to_str().ok()).and_then(|h| h.strip_prefix("Bearer "));
    let Some(bearer) = bearer else {
        return (StatusCode::UNAUTHORIZED, axum::Json(json!({"error": "token tidak valid"}))).into_response();
    };
    if st.token.as_deref().is_some_and(|t| t == bearer) {
        return next.run(req).await;
    }
    let Some(subject) = st.subject_tokens.get(bearer).cloned() else {
        return (StatusCode::UNAUTHORIZED, axum::Json(json!({"error": "token tidak valid"}))).into_response();
    };
    if !subject_token_allows(req.uri().path(), &subject) {
        return (StatusCode::FORBIDDEN, axum::Json(json!({"error": format!("token ini hanya berlaku untuk subjek {subject}")}))).into_response();
    }
    req.extensions_mut().insert(BoundSubject(subject));
    next.run(req).await
}

#[cfg(feature = "ui")]
const INDEX_HTML: &str = include_str!("../ui/index.html");
#[cfg(feature = "ui")]
const APP_JS: &str = include_str!("../ui/app.js");
#[cfg(feature = "ui")]
const APP_CSS: &str = include_str!("../ui/app.css");
#[cfg(feature = "ui")]
const I18N_JS: &str = include_str!("../ui/i18n.js");
#[cfg(feature = "ui")]
const LIQUID_GLASS_JS: &str = include_str!("../ui/liquid-glass.js");
#[cfg(feature = "ui")]
const GRAPH_JS: &str = include_str!("../ui/graph.js");
#[cfg(feature = "ui")]
const HUB_JS: &str = include_str!("../ui/hub.js");
#[cfg(all(feature = "ui", feature = "hub"))]
const HUB_HTML: &str = include_str!("../ui/hub.html");
#[cfg(feature = "ui")]
const LOGO_PNG: &[u8] = include_bytes!("../ui/logo.png");

#[cfg(feature = "ui")]
fn asset(body: &'static str, ctype: &'static str) -> Response {
    let mut r = body.into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(ctype));
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    r
}

pub fn router(state: AppState) -> Router {
    let max = state.rt.cfg.server.max_upload_mb * 1024 * 1024;
    let protected = api::routes().merge(hooks::routes());
    #[cfg(feature = "hub")]
    let protected = protected.merge(hub_local::routes());
    let protected = protected
        .route("/mcp", post(mcp::http_handler).get(mcp::http_get))
        .layer(middleware::from_fn_with_state(state.clone(), auth))
        .layer(axum::extract::DefaultBodyLimit::max(max));
    let r: Router<AppState> = Router::new()
        .route("/api/health", get(|| async { axum::Json(json!({"ok": true, "version": env!("CARGO_PKG_VERSION")})) }));
    #[cfg(feature = "ui")]
    let r = r
        .route("/", get(|| async { asset(INDEX_HTML, "text/html; charset=utf-8") }))
        .route("/app.js", get(|| async { asset(APP_JS, "text/javascript; charset=utf-8") }))
        .route("/app.css", get(|| async { asset(APP_CSS, "text/css; charset=utf-8") }))
        .route("/i18n.js", get(|| async { asset(I18N_JS, "text/javascript; charset=utf-8") }))
        .route("/liquid-glass.js", get(|| async { asset(LIQUID_GLASS_JS, "text/javascript; charset=utf-8") }))
        .route("/graph.js", get(|| async { asset(GRAPH_JS, "text/javascript; charset=utf-8") }))
        .route(
            "/logo.png",
            get(|| async { ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "public, max-age=86400")], LOGO_PNG) }),
        )
        .route("/hub.js", get(|| async { asset(HUB_JS, "text/javascript; charset=utf-8") }));
    let r = r.merge(protected);
    #[cfg(feature = "hub")]
    let r = match state.hub.clone() {
        // Rute hub di luar middleware `MEMORY_TOKEN`: tiap handler memeriksa JWT/PAT sendiri.
        Some(hub) => r.nest("/hub", memory_hub::router(hub, hub_html())),
        None => r,
    };
    r.with_state(state)
}

#[cfg(all(feature = "hub", feature = "ui"))]
fn hub_html() -> &'static str {
    HUB_HTML
}

#[cfg(all(feature = "hub", not(feature = "ui")))]
fn hub_html() -> &'static str {
    "<!doctype html><title>engram hub</title><p>UI hub tidak dibangun (fitur `ui` mati).</p>"
}

/// Hub tim di atas store vault ini (tabel `hub_*`, migrasi sendiri). Rahasia dari env.
#[cfg(feature = "hub")]
pub async fn hub_state(rt: &Runtime, cfg: memory_core::config::HubConfig) -> Result<memory_hub::HubState, memory_hub::server::HubError> {
    memory_hub::HubState::init(rt.store.clone(), cfg).await
}

/// Jalankan server sampai `shutdown` bernilai true.
pub async fn serve(state: AppState, bind: &str, shutdown: tokio::sync::watch::Receiver<bool>) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    serve_listener(state, listener, shutdown).await
}

/// Seperti [`serve`], tetapi dengan listener yang sudah dibuka (dipakai aplikasi desktop).
pub async fn serve_listener(state: AppState, listener: tokio::net::TcpListener, mut shutdown: tokio::sync::watch::Receiver<bool>) -> std::io::Result<()> {
    tracing::info!("UI & API: http://{}", listener.local_addr()?);
    // ConnectInfo: IP klien untuk pembatas laju & audit hub.
    axum::serve(listener, router(state).into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(async move {
            let _ = shutdown.changed().await;
        })
        .await
}

#[cfg(test)]
mod tests;
