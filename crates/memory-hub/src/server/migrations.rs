//! Skema tabel `hub_*` (SCHEMALESS + indeks), berversi di `_meta:hub_schema`, terpisah dari
//! ledger skema vault (`_meta:schema`) agar hub bisa dinyalakan di vault yang sudah ada.

use serde_json::json;

use memory_store::{Store, binds};

use super::Result;

pub const HUB_SCHEMA_VERSION: u32 = 1;

/// Semua tabel hub (untuk ekspor/backup).
pub const HUB_TABLES: &[&str] = &["hub_user", "hub_identity", "hub_session", "hub_oauth_state", "hub_ticket", "hub_token", "hub_repo", "hub_member", "hub_invite", "hub_record", "hub_audit"];

const V1: &str = r#"
DEFINE TABLE IF NOT EXISTS hub_user SCHEMALESS;
DEFINE INDEX IF NOT EXISTS hub_user_email ON hub_user FIELDS email UNIQUE;

DEFINE TABLE IF NOT EXISTS hub_identity SCHEMALESS;
DEFINE INDEX IF NOT EXISTS hub_identity_user ON hub_identity FIELDS user_id;

DEFINE TABLE IF NOT EXISTS hub_session SCHEMALESS;
DEFINE INDEX IF NOT EXISTS hub_session_hash ON hub_session FIELDS refresh_hash UNIQUE;
DEFINE INDEX IF NOT EXISTS hub_session_user ON hub_session FIELDS user_id;
DEFINE INDEX IF NOT EXISTS hub_session_exp ON hub_session FIELDS expires_at;

DEFINE TABLE IF NOT EXISTS hub_oauth_state SCHEMALESS;
DEFINE INDEX IF NOT EXISTS hub_oauth_state_exp ON hub_oauth_state FIELDS expires_at;

DEFINE TABLE IF NOT EXISTS hub_ticket SCHEMALESS;
DEFINE INDEX IF NOT EXISTS hub_ticket_exp ON hub_ticket FIELDS expires_at;

DEFINE TABLE IF NOT EXISTS hub_token SCHEMALESS;
DEFINE INDEX IF NOT EXISTS hub_token_hash ON hub_token FIELDS hash UNIQUE;
DEFINE INDEX IF NOT EXISTS hub_token_user ON hub_token FIELDS user_id;

DEFINE TABLE IF NOT EXISTS hub_repo SCHEMALESS;
DEFINE INDEX IF NOT EXISTS hub_repo_github ON hub_repo FIELDS github_full_name;

DEFINE TABLE IF NOT EXISTS hub_member SCHEMALESS;
DEFINE INDEX IF NOT EXISTS hub_member_repo ON hub_member FIELDS repo_id;
DEFINE INDEX IF NOT EXISTS hub_member_user ON hub_member FIELDS user_id;

DEFINE TABLE IF NOT EXISTS hub_invite SCHEMALESS;
DEFINE INDEX IF NOT EXISTS hub_invite_repo ON hub_invite FIELDS repo_id;
DEFINE INDEX IF NOT EXISTS hub_invite_hash ON hub_invite FIELDS token_hash UNIQUE;

DEFINE TABLE IF NOT EXISTS hub_record SCHEMALESS;
DEFINE INDEX IF NOT EXISTS hub_record_seq ON hub_record FIELDS repo_id, seq;
DEFINE INDEX IF NOT EXISTS hub_record_owner ON hub_record FIELDS repo_id, user_id, device_id;
DEFINE INDEX IF NOT EXISTS hub_record_kind ON hub_record FIELDS repo_id, kind, status;

DEFINE TABLE IF NOT EXISTS hub_audit SCHEMALESS;
DEFINE INDEX IF NOT EXISTS hub_audit_repo ON hub_audit FIELDS repo_id, at;
DEFINE INDEX IF NOT EXISTS hub_audit_actor ON hub_audit FIELDS actor, at;

DEFINE TABLE IF NOT EXISTS _meta SCHEMALESS;
"#;

pub async fn run(store: &Store) -> Result<()> {
    let meta = store.rows("SELECT * FROM _meta:hub_schema;", binds! {}).await.unwrap_or_default();
    let current = meta.first().and_then(|m| m["version"].as_u64()).unwrap_or(0) as u32;
    if current < 1 {
        store.execute(V1, binds! {}).await?;
    }
    if current < HUB_SCHEMA_VERSION {
        store
            .execute("UPSERT _meta:hub_schema CONTENT $c RETURN NONE;", binds! {"c" => json!({"version": HUB_SCHEMA_VERSION, "migrated_at": memory_core::ids::now_rfc3339()})})
            .await?;
        tracing::info!(from = current, to = HUB_SCHEMA_VERSION, "migrasi skema hub selesai");
    }
    Ok(())
}
