//! Baris tabel `hub_*` (dokumen SurrealDB). Field `Option` yang `None` tidak ditulis
//! (lihat `content_of` di memory-store), karenanya semua struct `#[serde(default)]`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{ShareSettings, UserOut};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UserRow {
    pub id: String,
    /// Huruf kecil; unik.
    pub email: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub disabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl UserRow {
    pub fn out(&self) -> UserOut {
        UserOut { id: self.id.clone(), email: self.email.clone(), name: self.name.clone(), avatar_url: self.avatar_url.clone() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct IdentityRow {
    /// `det_id([provider, provider_id])`.
    pub id: String,
    pub provider: String,
    pub provider_id: String,
    pub user_id: String,
    pub email: String,
    pub email_verified: bool,
    pub last_login_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SessionRow {
    pub id: String,
    pub user_id: String,
    /// blake3 heksa refresh token; token polosnya tidak pernah disimpan.
    pub refresh_hash: String,
    pub label: String,
    pub expires_at: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct OauthStateRow {
    /// Nonce `state` (64 heksa).
    pub id: String,
    pub provider: String,
    pub ticket: Option<String>,
    /// `login` (bawaan) atau `verify-github`.
    pub purpose: String,
    pub repo_id: Option<String>,
    pub expires_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TicketRow {
    pub id: String,
    /// `pending` | `ready` | `error` | `consumed`.
    pub status: String,
    /// JSON token (ready) atau pesan galat (error).
    pub payload: Option<String>,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PatRow {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub prefix: String,
    pub hash: String,
    pub scopes: Vec<String>,
    pub repo_ids: Vec<String>,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RepoRow {
    pub id: String,
    pub name: String,
    pub project: String,
    pub remote: Option<String>,
    pub github_full_name: Option<String>,
    pub created_by: String,
    pub join_key_hash: String,
    pub join_key_version: u32,
    pub join_enabled: bool,
    pub require_github_verify: bool,
    /// Counter change log; naik satu per mutasi record.
    pub seq: u64,
    pub settings: ShareSettings,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MemberRow {
    /// `det_id([repo_id, user_id])`.
    pub id: String,
    pub repo_id: String,
    pub user_id: String,
    pub role: String,
    /// `owner` | `key` | `github` | `invite`.
    pub joined_via: String,
    pub github_verified_at: Option<String>,
    pub invited_by: Option<String>,
    pub created_at: String,
    pub revoked_at: Option<String>,
    pub last_push_at: Option<String>,
    pub devices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RecordRow {
    /// `det_id([repo_id, user_id, device_id, kind, rid])`.
    pub id: String,
    pub repo_id: String,
    pub user_id: String,
    pub device_id: String,
    pub device: String,
    pub author: String,
    pub kind: String,
    pub rid: String,
    pub body: Value,
    pub content_hash: String,
    /// `active` | `withdrawn`.
    pub status: String,
    pub seq: u64,
    pub created_at: String,
    pub updated_at: String,
    pub withdrawn_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AuditRow {
    pub id: String,
    pub at: String,
    /// `user_id` atau `system`.
    pub actor: String,
    pub actor_name: String,
    pub repo_id: Option<String>,
    pub action: String,
    pub detail: Value,
    pub ip: Option<String>,
}
