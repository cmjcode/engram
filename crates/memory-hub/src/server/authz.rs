//! Peran anggota repo dan satu pintu pemeriksaan izin (`require`), termasuk scope PAT.

use memory_store::binds;

use super::auth::extract::{AuthVia, HubUser};
use super::models::{MemberRow, RepoRow};
use super::{HubError, HubState, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Reader,
    Member,
    Maintainer,
    Owner,
}

impl Role {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "reader" => Some(Role::Reader),
            "member" => Some(Role::Member),
            "maintainer" => Some(Role::Maintainer),
            "owner" => Some(Role::Owner),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Reader => "reader",
            Role::Member => "member",
            Role::Maintainer => "maintainer",
            Role::Owner => "owner",
        }
    }
}

/// Keanggotaan aktif (belum dicabut) `user_id` di `repo_id`.
pub async fn member_of(st: &HubState, repo_id: &str, user_id: &str) -> Result<Option<MemberRow>> {
    let id = crate::ids::det_id(&[repo_id, user_id]);
    let m: Option<MemberRow> = st.store.get("hub_member", &id).await?;
    Ok(m.filter(|m| m.revoked_at.is_none()))
}

pub async fn load_repo(st: &HubState, repo_id: &str) -> Result<RepoRow> {
    if !crate::ids::is_valid_key(repo_id) {
        return Err(HubError::BadRequest("id repo tidak valid".into()));
    }
    st.store.get::<RepoRow>("hub_repo", repo_id).await?.ok_or_else(|| HubError::NotFound("repo tidak ditemukan".into()))
}

pub async fn members_of(st: &HubState, repo_id: &str) -> Result<Vec<MemberRow>> {
    let rows: Vec<MemberRow> = st.store.rows_as("SELECT * FROM hub_member WHERE repo_id = $r ORDER BY created_at ASC;", binds! {"r" => repo_id}).await?;
    Ok(rows.into_iter().filter(|m| m.revoked_at.is_none()).collect())
}

pub struct Access {
    pub repo: RepoRow,
    pub member: MemberRow,
    pub role: Role,
}

/// Repo harus ada, pemanggil anggota aktif dengan peran >= `min`; PAT harus punya scope yang
/// sesuai (`repo:write` untuk tulis, `repo:read`/`repo:write` untuk baca) dan, bila dibatasi,
/// mencantumkan repo ini. Repo terarsip menolak tulis.
pub async fn require(st: &HubState, repo_id: &str, user: &HubUser, min: Role, write: bool) -> Result<Access> {
    let repo = load_repo(st, repo_id).await?;
    let member = member_of(st, repo_id, &user.id).await?.ok_or_else(|| HubError::Forbidden("Anda bukan anggota repo ini".into()))?;
    let role = Role::parse(&member.role).unwrap_or(Role::Reader);
    if role < min {
        return Err(HubError::Forbidden(format!("butuh peran {} (peran Anda: {})", min.as_str(), role.as_str())));
    }
    if write && repo.archived {
        return Err(HubError::Forbidden("repo diarsipkan; hanya bisa dibaca".into()));
    }
    if let AuthVia::Pat { scopes, repo_ids } = &user.via {
        if !repo_ids.is_empty() && !repo_ids.iter().any(|r| r == repo_id) {
            return Err(HubError::Forbidden("token ini tidak berlaku untuk repo tersebut".into()));
        }
        let ok = if write { scopes.iter().any(|s| s == "repo:write") } else { scopes.iter().any(|s| s == "repo:write" || s == "repo:read") };
        if !ok {
            return Err(HubError::Forbidden(format!("token tidak punya scope {}", if write { "repo:write" } else { "repo:read" })));
        }
    }
    Ok(Access { repo, member, role })
}
