//! Personal access token (CI, agent cloud): `engram_pat_<64 heksa>`, hanya hash-nya disimpan,
//! scope `repo:read` / `repo:write` / `mcp`, bisa dibatasi ke repo tertentu.

use axum::Json;
use axum::extract::{Path, State};
use serde_json::{Value, json};

use memory_core::ids::{new_id, now_rfc3339};
use memory_store::binds;

use super::super::models::PatRow;
use super::super::{Api, HubError, HubState, Result, audit};
use super::extract::{AuthVia, HubUser};
use super::users;
use crate::ids::{hash_hex, new_pat};
use crate::types::{TokenCreateIn, TokenCreateOut, TokenOutRow};

const SCOPES: [&str; 3] = ["repo:read", "repo:write", "mcp"];

pub async fn authenticate(st: &HubState, token: &str) -> Result<HubUser> {
    let mut rows: Vec<PatRow> = st.store.rows_as("SELECT * FROM hub_token WHERE hash = $h LIMIT 1;", binds! {"h" => hash_hex(token)}).await?;
    let row = rows.pop().ok_or_else(|| HubError::Unauthorized("token tidak dikenal".into()))?;
    if row.revoked_at.is_some() {
        return Err(HubError::Unauthorized("token sudah dicabut".into()));
    }
    let now = now_rfc3339();
    if row.expires_at.as_deref().is_some_and(|e| e <= now.as_str()) {
        return Err(HubError::Unauthorized("token sudah kedaluwarsa".into()));
    }
    let user = users::find_user(st, &row.user_id).await?.filter(|u| !u.disabled).ok_or_else(|| HubError::Unauthorized("akun token tidak aktif".into()))?;
    let _ = st.store.patch("hub_token", &row.id, json!({"last_used_at": now})).await;
    Ok(HubUser { id: user.id, email: user.email, name: user.name, via: AuthVia::Pat { scopes: row.scopes, repo_ids: row.repo_ids } })
}

fn row_out(r: &PatRow) -> TokenOutRow {
    TokenOutRow { id: r.id.clone(), name: r.name.clone(), prefix: r.prefix.clone(), scopes: r.scopes.clone(), repo_ids: r.repo_ids.clone(), expires_at: r.expires_at.clone(), created_at: r.created_at.clone(), last_used_at: r.last_used_at.clone() }
}

/// `POST /hub/api/me/tokens` (hanya lewat login JWT; PAT tidak boleh membuat PAT lain).
pub async fn create(State(st): State<HubState>, user: HubUser, Json(b): Json<TokenCreateIn>) -> Api<TokenCreateOut> {
    if user.is_pat() {
        return Err(HubError::Forbidden("buat token lewat sesi login, bukan lewat token lain".into()));
    }
    let name: String = b.name.trim().chars().take(64).collect();
    if name.is_empty() {
        return Err(HubError::BadRequest("nama token wajib diisi".into()));
    }
    let mut scopes: Vec<String> = b.scopes.iter().map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()).collect();
    if scopes.is_empty() {
        scopes = vec!["repo:read".into(), "repo:write".into()];
    }
    if let Some(bad) = scopes.iter().find(|s| !SCOPES.contains(&s.as_str())) {
        return Err(HubError::BadRequest(format!("scope tidak dikenal: {bad} (pakai repo:read, repo:write, mcp)")));
    }
    scopes.sort();
    scopes.dedup();
    let repo_ids: Vec<String> = b.repo_ids.iter().map(|r| r.trim().to_string()).filter(|r| crate::ids::is_valid_key(r)).collect();
    let expires_at = b.expires_days.filter(|d| *d > 0).map(|d| (chrono::Utc::now() + chrono::Duration::days(d.min(3650))).format("%Y-%m-%dT%H:%M:%SZ").to_string());
    let secret = new_pat();
    let row = PatRow { id: new_id(), user_id: user.id.clone(), name, prefix: secret.chars().take(16).collect(), hash: hash_hex(&secret), scopes, repo_ids, expires_at, created_at: now_rfc3339(), last_used_at: None, revoked_at: None };
    st.store.upsert("hub_token", &row.id, &row, None).await?;
    audit::log(&st, &user.id, &user.name, None, "pat.create", json!({"token_id": row.id, "scopes": row.scopes}), None).await;
    Ok(Json(TokenCreateOut { token: row_out(&row), secret }))
}

pub async fn list(State(st): State<HubState>, user: HubUser) -> Api<Vec<TokenOutRow>> {
    let rows: Vec<PatRow> = st.store.rows_as("SELECT * FROM hub_token WHERE user_id = $u ORDER BY created_at DESC, id;", binds! {"u" => user.id}).await?;
    Ok(Json(rows.iter().filter(|r| r.revoked_at.is_none()).map(row_out).collect()))
}

pub async fn revoke(State(st): State<HubState>, user: HubUser, Path(id): Path<String>) -> Api<Value> {
    if user.is_pat() {
        return Err(HubError::Forbidden("cabut token lewat sesi login".into()));
    }
    let row: Option<PatRow> = if crate::ids::is_valid_key(&id) { st.store.get("hub_token", &id).await? } else { None };
    let row = row.filter(|r| r.user_id == user.id).ok_or_else(|| HubError::NotFound("token tidak ditemukan".into()))?;
    st.store.patch("hub_token", &row.id, json!({"revoked_at": now_rfc3339()})).await?;
    audit::log(&st, &user.id, &user.name, None, "pat.revoke", json!({"token_id": row.id}), None).await;
    Ok(Json(json!({"ok": true})))
}
