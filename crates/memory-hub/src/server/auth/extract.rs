//! Extractor axum: pengguna hub dari `Authorization: Bearer` (JWT akses atau PAT) dan IP klien.

use std::convert::Infallible;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;

use super::super::ratelimit::client_ip;
use super::super::{HubError, HubState, Result};
use crate::ids::PAT_PREFIX;

#[derive(Debug, Clone)]
pub enum AuthVia {
    Jwt,
    Pat { scopes: Vec<String>, repo_ids: Vec<String> },
}

#[derive(Debug, Clone)]
pub struct HubUser {
    pub id: String,
    pub email: String,
    pub name: String,
    pub via: AuthVia,
}

impl HubUser {
    pub fn is_pat(&self) -> bool {
        matches!(self.via, AuthVia::Pat { .. })
    }

    /// JWT punya semua scope; PAT hanya yang diberikan (`repo:write` mencakup `repo:read`).
    pub fn has_scope(&self, scope: &str) -> bool {
        match &self.via {
            AuthVia::Jwt => true,
            AuthVia::Pat { scopes, .. } => scopes.iter().any(|s| s == scope || (scope == "repo:read" && s == "repo:write")),
        }
    }
}

impl FromRequestParts<HubState> for HubUser {
    type Rejection = HubError;

    async fn from_request_parts(parts: &mut Parts, st: &HubState) -> Result<Self> {
        let raw = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer ").or_else(|| h.strip_prefix("bearer ")))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| HubError::Unauthorized("butuh header Authorization: Bearer <token>".into()))?;
        if raw.starts_with(PAT_PREFIX) {
            return super::pat::authenticate(st, raw).await;
        }
        let c = super::jwt::verify(raw, &st.secrets.jwt)?;
        Ok(HubUser { id: c.sub, email: c.email, name: c.name, via: AuthVia::Jwt })
    }
}

/// IP klien untuk audit & pembatas laju (lihat [`client_ip`]).
#[derive(Debug, Clone)]
pub struct ClientIp(pub String);

impl FromRequestParts<HubState> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, st: &HubState) -> std::result::Result<Self, Infallible> {
        let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
        Ok(ClientIp(client_ip(&parts.headers, peer, st.cfg.trust_proxy)))
    }
}
