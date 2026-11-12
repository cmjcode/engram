//! Akun hub tim dari sisi server lokal (UI web lokal & aplikasi desktop). Browser tidak
//! memanggil hub langsung (beda origin); server lokal menjalankan alur login lewat tiket,
//! menyimpan kredensial di `<data_dir>/hub.json`, dan membagikannya dengan CLI & job sinkron.
//!
//! - `GET  /api/hub/account`           akun yang sedang login (tanpa jaringan)
//! - `POST /api/hub/login {provider, hub_url}` -> `{url, ticket}`; buka `url` di browser
//! - `GET  /api/hub/login/{ticket}`    status login (`pending` | `completed` | `error`)
//! - `POST /api/hub/logout`

use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::{Value, json};

use memory_hub::client::{HubClient, HubCreds, LoginFlow};

use crate::{ApiError, ApiResult, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/hub/account", get(account))
        .route("/api/hub/login", post(login))
        .route("/api/hub/login/{ticket}", get(login_status))
        .route("/api/hub/logout", post(logout))
}

async fn account(State(st): State<AppState>) -> ApiResult<Value> {
    let creds = HubCreds::load();
    Ok(Json(json!({
        "logged_in": creds.is_some(),
        "hub": creds.as_ref().map(|c| c.hub.clone()).or_else(|| st.rt.cfg.repo_share.hub_url.clone()),
        "user": creds.map(|c| c.user),
        "pat_env": std::env::var("ENGRAM_HUB_TOKEN").is_ok_and(|v| !v.trim().is_empty()),
        "serving_hub": st.hub.is_some(),
    })))
}

#[derive(Deserialize)]
struct LoginIn {
    provider: String,
    hub_url: Option<String>,
}

/// Mulai login: kembalikan URL yang harus dibuka di browser; penunggu latar menyimpan token.
async fn login(State(st): State<AppState>, Json(b): Json<LoginIn>) -> ApiResult<Value> {
    let hub = b
        .hub_url
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(str::to_string)
        .or_else(|| st.rt.cfg.repo_share.hub_url.clone())
        .or_else(|| HubCreds::load().map(|c| c.hub))
        .ok_or_else(|| ApiError::BadRequest("isi URL hub tim".into()))?;
    let flow = LoginFlow::start(&hub, b.provider.trim()).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    // Hub harus bisa dihubungi sebelum browser dibuka.
    HubClient::anonymous(&hub).map_err(|e| ApiError::BadRequest(e.to_string()))?.info().await.map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let (url, ticket) = (flow.url.clone(), flow.ticket.clone());
    if let Ok(mut m) = st.hub_logins.lock() {
        m.retain(|_, v| v["status"] == "pending");
        m.insert(ticket.clone(), json!({"status": "pending"}));
    }
    let logins = st.hub_logins.clone();
    let t2 = ticket.clone();
    tokio::spawn(async move {
        let v = match flow.wait(Duration::from_secs(300)).await {
            Ok(c) => json!({"status": "completed", "user": c.user, "hub": c.hub}),
            Err(e) => json!({"status": "error", "error": e.to_string()}),
        };
        if let Ok(mut m) = logins.lock() {
            m.insert(t2, v);
        }
    });
    // Aplikasi desktop membuka browser sistem; UI web membuka tab sendiri dari `url`.
    let opened = st.services.as_ref().is_some_and(|sv| sv.open(&url).is_ok());
    Ok(Json(json!({"url": url, "ticket": ticket, "hub": hub, "opened": opened})))
}

async fn login_status(State(st): State<AppState>, Path(ticket): Path<String>) -> ApiResult<Value> {
    let m = st.hub_logins.lock().map_err(|_| ApiError::Internal("kunci login".into()))?;
    Ok(Json(m.get(&ticket).cloned().unwrap_or_else(|| json!({"status": "unknown"}))))
}

async fn logout(State(_st): State<AppState>) -> ApiResult<Value> {
    if let Some(c) = HubCreds::load() {
        if let Ok(client) = HubClient::anonymous(&c.hub) {
            let _ = client.logout(&c.refresh_token).await;
        }
        HubCreds::clear().map_err(|e| ApiError::Internal(e.to_string()))?;
    }
    Ok(Json(json!({"ok": true})))
}
