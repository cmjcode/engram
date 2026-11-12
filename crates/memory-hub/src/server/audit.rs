//! Jejak audit (login, join, perubahan peran, rotasi key, push). Best-effort: kegagalan tulis
//! hanya dicatat di log, tidak menggagalkan aksinya.

use serde_json::Value;

use memory_core::ids::{new_id, now_rfc3339};
use memory_store::binds;

use super::models::AuditRow;
use super::{HubState, Result};

pub async fn log(st: &HubState, actor: &str, actor_name: &str, repo_id: Option<&str>, action: &str, detail: Value, ip: Option<&str>) {
    let row = AuditRow {
        id: new_id(),
        at: now_rfc3339(),
        actor: actor.to_string(),
        actor_name: actor_name.to_string(),
        repo_id: repo_id.map(str::to_string),
        action: action.to_string(),
        detail,
        ip: ip.map(str::to_string),
    };
    if let Err(e) = st.store.upsert("hub_audit", &row.id, &row, None).await {
        tracing::warn!(error = %e, action, "audit hub gagal ditulis");
    }
}

pub async fn list(st: &HubState, repo_id: &str, limit: usize) -> Result<Vec<AuditRow>> {
    let limit = limit.clamp(1, 500);
    Ok(st.store.rows_as(&format!("SELECT * FROM hub_audit WHERE repo_id = $r ORDER BY at DESC, id LIMIT {limit};"), binds! {"r" => repo_id}).await?)
}
