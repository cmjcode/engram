//! Verifikasi izin repo GitHub: token GitHub milik pengguna dipakai SEKALI untuk
//! `GET /repos/{owner}/{repo}` lalu dibuang; hub tidak pernah menyimpannya.

use async_trait::async_trait;
use serde::Deserialize;

use super::super::authz::Role;
use super::super::{HubError, Result};

#[async_trait]
pub trait GithubApi: Send + Sync {
    /// Peran yang pantas dari izin GitHub (`admin`/`maintain` -> maintainer, `push` -> member,
    /// `pull` -> reader); `None` bila repo tidak terlihat oleh token itu.
    async fn repo_role(&self, token: &str, full_name: &str) -> Result<Option<Role>>;
}

pub struct ReqwestGithub {
    http: reqwest::Client,
}

impl ReqwestGithub {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }
}

#[derive(Deserialize, Default)]
struct Perms {
    #[serde(default)]
    admin: bool,
    #[serde(default)]
    maintain: bool,
    #[serde(default)]
    push: bool,
    #[serde(default)]
    pull: bool,
}

#[derive(Deserialize)]
struct RepoInfo {
    #[serde(default)]
    permissions: Perms,
}

#[async_trait]
impl GithubApi for ReqwestGithub {
    async fn repo_role(&self, token: &str, full_name: &str) -> Result<Option<Role>> {
        if !valid_full_name(full_name) {
            return Err(HubError::BadRequest(format!("nama repo GitHub tidak valid: {full_name}")));
        }
        let resp = self
            .http
            .get(format!("https://api.github.com/repos/{full_name}"))
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| HubError::OAuth(format!("GitHub tidak bisa dihubungi: {e}")))?;
        match resp.status().as_u16() {
            200 => {}
            401 => return Err(HubError::Forbidden("token GitHub ditolak".into())),
            403 | 404 => return Ok(None),
            s => return Err(HubError::OAuth(format!("GitHub menjawab {s}"))),
        }
        let info: RepoInfo = resp.json().await.map_err(|e| HubError::OAuth(format!("balasan GitHub tidak terbaca: {e}")))?;
        Ok(role_of(&info.permissions))
    }
}

fn role_of(p: &Perms) -> Option<Role> {
    if p.admin || p.maintain {
        Some(Role::Maintainer)
    } else if p.push {
        Some(Role::Member)
    } else if p.pull {
        Some(Role::Reader)
    } else {
        None
    }
}

pub fn valid_full_name(s: &str) -> bool {
    let Some((o, r)) = s.split_once('/') else { return false };
    let ok = |p: &str| !p.is_empty() && p.len() <= 100 && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    ok(o) && ok(r) && !r.contains('/')
}

/// `owner/repo` dari URL remote GitHub (https, ssh, atau scp-like); `None` untuk host lain.
pub fn full_name_of(remote: &str) -> Option<String> {
    let r = remote.trim().trim_end_matches('/');
    let rest = if let Some(x) = r.strip_prefix("git@github.com:") {
        x
    } else if let Some(x) = r.strip_prefix("ssh://git@github.com/") {
        x
    } else {
        r.strip_prefix("https://github.com/").or_else(|| r.strip_prefix("http://github.com/"))?
    };
    let name = rest.trim_end_matches(".git").trim_matches('/');
    valid_full_name(name).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remotes_and_maps_roles() {
        assert_eq!(full_name_of("https://github.com/cmjcode/engram.git").as_deref(), Some("cmjcode/engram"));
        assert_eq!(full_name_of("git@github.com:cmjcode/engram.git").as_deref(), Some("cmjcode/engram"));
        assert_eq!(full_name_of("ssh://git@github.com/cmjcode/engram").as_deref(), Some("cmjcode/engram"));
        assert!(full_name_of("https://gitlab.com/a/b").is_none());
        assert!(full_name_of("https://github.com/a/b/c").is_none());
        assert_eq!(role_of(&Perms { admin: true, ..Default::default() }), Some(Role::Maintainer));
        assert_eq!(role_of(&Perms { push: true, pull: true, ..Default::default() }), Some(Role::Member));
        assert_eq!(role_of(&Perms { pull: true, ..Default::default() }), Some(Role::Reader));
        assert_eq!(role_of(&Perms::default()), None);
    }
}
