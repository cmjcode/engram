//! Server sync vault: komputer lain (aplikasi desktop dengan URL server ini) menarik snapshot
//! vault server dan mengirim snapshot miliknya untuk digabung. Lihat `memory_jobs::sync_server`.
//!
//! - `GET  /api/vault-sync/info`             siapa yang login, versi skema
//! - `GET  /api/vault-sync/snapshot`         jsonl.gz vault ini (`If-None-Match` -> 304)
//! - `POST /api/vault-sync/snapshot`         gabungkan jsonl.gz kiriman klien
//! - `GET  /api/vault-sync/sources`          daftar berkas sumber
//! - `GET|PUT /api/vault-sync/sources/{*p}`  unduh / unggah satu berkas sumber (tidak ditimpa)
//!
//! Auth (di luar middleware `MEMORY_TOKEN` biasa): token global, atau login hub (JWT/PAT
//! dengan scope `vault:sync`) dari email di `[server] sync_owners` / `ENGRAM_SYNC_OWNERS`.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::json;

use memory_jobs::sync_server as ss;

use crate::{ApiError, AppState};

/// Akun yang lolos auth sync (email, atau `token` untuk token global).
#[derive(Clone)]
struct SyncUser(String);

pub fn routes(state: AppState) -> Router<AppState> {
    let max = state.rt.cfg.server.max_upload_mb * 1024 * 1024;
    Router::new()
        .route("/api/vault-sync/info", get(info))
        .route("/api/vault-sync/snapshot", get(snapshot_get).post(snapshot_post))
        .route("/api/vault-sync/sources", get(sources))
        .route("/api/vault-sync/sources/{*path}", get(source_get).put(source_put))
        .layer(middleware::from_fn_with_state(state, guard))
        .layer(axum::extract::DefaultBodyLimit::max(max))
}

fn deny(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, axum::Json(json!({"error": msg.into()}))).into_response()
}

async fn guard(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let bearer = req.headers().get(header::AUTHORIZATION).and_then(|h| h.to_str().ok()).and_then(|h| h.strip_prefix("Bearer ").or_else(|| h.strip_prefix("bearer "))).map(str::trim).map(str::to_string);
    let Some(bearer) = bearer.filter(|b| !b.is_empty()) else {
        return deny(StatusCode::UNAUTHORIZED, "butuh login (Authorization: Bearer)");
    };
    if st.token.as_deref().is_some_and(|t| *t == bearer) {
        let mut req = req;
        req.extensions_mut().insert(SyncUser("token".into()));
        return next.run(req).await;
    }
    #[cfg(feature = "hub")]
    if let Some(hub) = st.hub.clone() {
        use axum::extract::FromRequestParts;
        let (mut parts, body) = req.into_parts();
        let user = match memory_hub::server::auth::extract::HubUser::from_request_parts(&mut parts, &hub).await {
            Ok(u) => u,
            Err(e) => return deny(StatusCode::UNAUTHORIZED, format!("login tidak valid: {e}")),
        };
        if user.is_pat() && !user.has_scope("vault:sync") {
            return deny(StatusCode::FORBIDDEN, "token akses ini tidak punya scope vault:sync");
        }
        let owners = st.rt.cfg.server.sync_owner_emails();
        if !owners.iter().any(|o| o.eq_ignore_ascii_case(&user.email)) {
            tracing::warn!(email = %user.email, "sync vault ditolak: bukan pemilik");
            return deny(StatusCode::FORBIDDEN, format!("akun {} tidak diizinkan sync dengan server ini; pemilik server perlu menambahkannya ke ENGRAM_SYNC_OWNERS", user.email));
        }
        let mut req = Request::from_parts(parts, body);
        req.extensions_mut().insert(SyncUser(user.email));
        return next.run(req).await;
    }
    deny(StatusCode::UNAUTHORIZED, "token tidak valid")
}

fn job_err(e: memory_jobs::JobError) -> ApiError {
    match e {
        memory_jobs::JobError::Invalid(m) => ApiError::BadRequest(m),
        e => ApiError::Internal(e.to_string()),
    }
}

async fn info(State(st): State<AppState>, axum::Extension(u): axum::Extension<SyncUser>) -> axum::Json<serde_json::Value> {
    axum::Json(json!({
        "ok": true,
        "account": u.0,
        "version": env!("CARGO_PKG_VERSION"),
        "schema_version": memory_core::SCHEMA_VERSION,
        "subject": st.rt.cfg.vault.default_subject,
    }))
}

async fn snapshot_get(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    let snap = ss::export_snapshot(&st.rt).await.map_err(job_err)?;
    let mut h = HeaderMap::new();
    let hv = |s: String| HeaderValue::from_str(&s).unwrap_or(HeaderValue::from_static(""));
    h.insert(ss::H_HASH, hv(snap.hash.clone()));
    h.insert(ss::H_ROWS, hv(snap.rows.to_string()));
    h.insert(ss::H_SCHEMA, hv(memory_core::SCHEMA_VERSION.to_string()));
    h.insert(header::ETAG, hv(snap.hash.clone()));
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()).is_some_and(|v| v.trim_matches('"') == snap.hash) {
        return Ok((StatusCode::NOT_MODIFIED, h).into_response());
    }
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/gzip"));
    Ok((StatusCode::OK, h, snap.bytes).into_response())
}

async fn snapshot_post(State(st): State<AppState>, axum::Extension(u): axum::Extension<SyncUser>, headers: HeaderMap, body: Bytes) -> Result<axum::Json<memory_jobs::sync::SyncReport>, ApiError> {
    let get = |k: &str| headers.get(k).and_then(|v| v.to_str().ok()).map(str::to_string);
    let schema: u32 = get(ss::H_SCHEMA).and_then(|v| v.parse().ok()).unwrap_or(0);
    if schema > memory_core::SCHEMA_VERSION {
        return Err(ApiError::BadRequest("komputer ini memakai engram versi lebih baru dari server; perbarui engram di server".into()));
    }
    let device = get(ss::H_DEVICE).map(|d| d.chars().take(80).collect::<String>()).unwrap_or_else(|| "komputer".into());
    let label = format!("{device} ({})", u.0);
    ss::import_snapshot(&st.rt, &body, &label, get(ss::H_HASH).as_deref()).await.map(axum::Json).map_err(job_err)
}

async fn sources(State(st): State<AppState>) -> Result<axum::Json<serde_json::Value>, ApiError> {
    let root = st.rt.vault.sources_dir();
    let files = tokio::task::spawn_blocking(move || ss::list_sources(&root)).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(axum::Json(json!({"files": files})))
}

async fn source_get(State(st): State<AppState>, Path(path): Path<String>) -> Result<Response, ApiError> {
    match ss::read_source(&st.rt.vault, &path).map_err(job_err)? {
        Some(bytes) => Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response()),
        None => Err(ApiError::NotFound(format!("berkas sumber {path} tidak ada"))),
    }
}

async fn source_put(State(st): State<AppState>, Path(path): Path<String>, body: Bytes) -> Result<axum::Json<serde_json::Value>, ApiError> {
    let dst = ss::source_path(&st.rt.vault.sources_dir(), &path).map_err(job_err)?;
    let created = ss::write_new(&dst, &body).map_err(job_err)?;
    Ok(axum::Json(json!({"ok": true, "created": created})))
}
