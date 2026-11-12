//! Tipe wire HTTP hub (dipakai server dan klien). Semua tanggal RFC3339 UTC.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct UserOut {
    pub id: String,
    pub email: String,
    pub name: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TokenOut {
    pub access_token: String,
    pub refresh_token: String,
    /// Umur access token (detik).
    pub expires_in: i64,
    pub user: UserOut,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PollOut {
    /// `pending` | `completed` | `error`.
    pub status: String,
    pub token: Option<TokenOut>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct InfoOut {
    pub enabled: bool,
    pub name: String,
    pub providers: Vec<String>,
    pub version: String,
    pub open_signup: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct IdentityOut {
    pub provider: String,
    pub email: String,
    pub last_login_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RepoBrief {
    pub id: String,
    pub name: String,
    pub project: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MeOut {
    pub user: UserOut,
    pub identities: Vec<IdentityOut>,
    pub repos: Vec<RepoBrief>,
}

/// Apa yang dibagikan per repo (`.engram/repo.toml [share]` dan pengaturan repo di hub).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ShareSettings {
    pub kinds: Vec<String>,
    pub max_records: usize,
    pub exclude: Vec<String>,
}

impl Default for ShareSettings {
    fn default() -> Self {
        Self { kinds: crate::ids::RECORD_KINDS.iter().map(|k| k.to_string()).collect(), max_records: 2000, exclude: vec![] }
    }
}

impl ShareSettings {
    pub fn shares(&self, kind: &str) -> bool {
        self.kinds.iter().any(|k| k.trim().eq_ignore_ascii_case(kind))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RepoCreateIn {
    pub name: String,
    pub project: String,
    pub remote: Option<String>,
    pub github_full_name: Option<String>,
    pub share: Option<ShareSettings>,
    pub require_github_verify: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RepoPatchIn {
    pub name: Option<String>,
    pub share: Option<ShareSettings>,
    pub join_enabled: Option<bool>,
    pub require_github_verify: Option<bool>,
    pub archived: Option<bool>,
    pub github_full_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RepoOut {
    pub id: String,
    pub name: String,
    pub project: String,
    pub remote: Option<String>,
    pub github_full_name: Option<String>,
    /// Peran pemanggil di repo ini.
    pub role: String,
    pub head_seq: u64,
    pub members: u64,
    /// Jumlah record aktif per jenis.
    pub counts: BTreeMap<String, u64>,
    pub share: ShareSettings,
    pub join_enabled: bool,
    pub join_key_version: u32,
    pub require_github_verify: bool,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RepoCreateOut {
    pub repo: RepoOut,
    /// Hanya ditampilkan sekali; hub menyimpan hash-nya saja.
    pub join_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct JoinIn {
    pub join_key: Option<String>,
    /// Token GitHub pengguna (dipakai sekali untuk memeriksa izin repo, tidak disimpan).
    pub github_token: Option<String>,
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct JoinOut {
    pub role: String,
    pub already_member: bool,
    pub repo: RepoOut,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RotateOut {
    pub join_key: String,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MemberOut {
    pub user_id: String,
    pub name: String,
    /// Hanya untuk maintainer ke atas.
    pub email: Option<String>,
    pub role: String,
    pub joined_via: String,
    pub github_verified_at: Option<String>,
    pub devices: Vec<String>,
    pub last_push_at: Option<String>,
    pub joined_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RoleIn {
    pub role: String,
}

/// Satu record yang dikirim klien.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RecordIn {
    pub kind: String,
    pub rid: String,
    pub content_hash: String,
    pub body: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RecordRef {
    pub kind: String,
    pub rid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PushIn {
    pub device_id: String,
    pub device_name: String,
    /// `true`: record aktif perangkat ini yang tidak ada di `upserts` ikut dicabut.
    pub full: bool,
    pub upserts: Vec<RecordIn>,
    pub withdraws: Vec<RecordRef>,
    /// Dengan `full`: record yang tetap dipertahankan walau tidak ada di `upserts` (sudah dikirim
    /// di push sebelumnya). Memungkinkan push penuh dipecah menjadi beberapa permintaan.
    pub keep: Vec<RecordRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PushOut {
    pub accepted: u32,
    pub skipped: u32,
    pub withdrawn: u32,
    /// Record yang ditolak (ukuran, jenis, atau masih memuat penanda redaksi).
    pub rejected: u32,
    pub head_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RecordOut {
    pub seq: u64,
    pub user_id: String,
    pub author: String,
    pub device_id: String,
    pub device: String,
    pub kind: String,
    pub rid: String,
    /// `active` | `withdrawn`.
    pub status: String,
    pub body: Value,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ChangesOut {
    /// `seq` terakhir di repo (cursor setelah semua halaman habis).
    pub head: u64,
    /// Cursor untuk halaman berikutnya; `None` bila sudah habis.
    pub next: Option<u64>,
    pub records: Vec<RecordOut>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LastPush {
    pub user_id: String,
    pub author: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StatusOut {
    pub head_seq: u64,
    pub members: u64,
    pub per_kind: BTreeMap<String, u64>,
    pub last_push: Option<LastPush>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TokenCreateIn {
    pub name: String,
    /// `repo:read`, `repo:write`, `mcp`.
    pub scopes: Vec<String>,
    /// Kosong = semua repo tempat pengguna menjadi anggota.
    pub repo_ids: Vec<String>,
    pub expires_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TokenOutRow {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    pub repo_ids: Vec<String>,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TokenCreateOut {
    pub token: TokenOutRow,
    /// Hanya ditampilkan sekali.
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AuditOut {
    pub at: String,
    pub actor: String,
    pub actor_name: String,
    pub action: String,
    pub detail: Value,
}
