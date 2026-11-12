//! Akun pengguna, penautan identitas provider, sesi refresh token.
//!
//! Aturan merge: identitas `(provider, provider_id)` yang sudah dikenal -> user-nya; identitas
//! baru dengan email terverifikasi yang sama dengan user yang ada -> ditautkan ke user itu;
//! selain itu user baru. Email yang belum terverifikasi di provider ditolak sama sekali, supaya
//! tidak ada akun yang bisa "memesan" alamat orang lain lalu kemasukan login sahnya.

use memory_core::ids::{new_id, now_rfc3339};
use memory_store::binds;

use super::super::models::{IdentityRow, SessionRow, UserRow};
use super::super::{HubError, HubState, Result};
use super::providers::{Provider, ProviderIdentity};
use crate::ids::{det_id, hash_hex, random_hex};
use crate::types::TokenOut;

pub async fn find_user(st: &HubState, id: &str) -> Result<Option<UserRow>> {
    if !crate::ids::is_valid_key(id) {
        return Ok(None);
    }
    Ok(st.store.get("hub_user", id).await?)
}

pub async fn user_by_email(st: &HubState, email: &str) -> Result<Option<UserRow>> {
    let mut rows: Vec<UserRow> = st.store.rows_as("SELECT * FROM hub_user WHERE email = $e LIMIT 1;", binds! {"e" => email}).await?;
    Ok(rows.pop())
}

pub async fn identities_of(st: &HubState, user_id: &str) -> Result<Vec<IdentityRow>> {
    Ok(st.store.rows_as("SELECT * FROM hub_identity WHERE user_id = $u ORDER BY created_at ASC;", binds! {"u" => user_id}).await?)
}

fn domain_allowed(st: &HubState, email: &str) -> bool {
    let allowed = &st.cfg.allowed_email_domains;
    if allowed.is_empty() {
        return true;
    }
    let dom = email.rsplit('@').next().unwrap_or("").to_ascii_lowercase();
    allowed.iter().any(|d| d.trim().trim_start_matches('@').eq_ignore_ascii_case(&dom))
}

fn default_name(ident: &ProviderIdentity, email: &str) -> String {
    ident.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| email.split('@').next().unwrap_or("pengguna").to_string())
}

/// Login/daftar lewat identitas provider; mengembalikan user-nya.
pub async fn login_identity(st: &HubState, provider: Provider, ident: &ProviderIdentity) -> Result<UserRow> {
    let email = ident.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(HubError::OAuth(format!("{} tidak memberikan alamat email", provider.label())));
    }
    if !ident.email_verified {
        return Err(HubError::Forbidden(format!("alamat {email} belum terverifikasi di {}; verifikasi dulu lalu coba lagi", provider.as_str())));
    }
    if !domain_allowed(st, &email) {
        return Err(HubError::Forbidden(format!("domain email {email} tidak diizinkan di hub ini")));
    }
    let now = now_rfc3339();
    let iid = det_id(&[provider.as_str(), &ident.provider_id]);
    if let Some(existing) = st.store.get::<IdentityRow>("hub_identity", &iid).await?
        && let Some(mut user) = find_user(st, &existing.user_id).await?
    {
        if user.disabled {
            return Err(HubError::Forbidden("akun dinonaktifkan oleh admin hub".into()));
        }
        let mut patch = serde_json::Map::new();
        if user.name.trim().is_empty() {
            user.name = default_name(ident, &email);
            patch.insert("name".into(), serde_json::json!(user.name));
        }
        if user.avatar_url.is_none() && ident.avatar_url.is_some() {
            user.avatar_url = ident.avatar_url.clone();
            patch.insert("avatar_url".into(), serde_json::json!(user.avatar_url));
        }
        if !patch.is_empty() {
            patch.insert("updated_at".into(), serde_json::json!(now));
            st.store.patch("hub_user", &user.id, serde_json::Value::Object(patch)).await?;
        }
        st.store.patch("hub_identity", &iid, serde_json::json!({"last_login_at": now, "email": email, "email_verified": true})).await?;
        return Ok(user);
    }
    // Identitas baru: tautkan ke user ber-email sama, atau buat user baru.
    let user = match user_by_email(st, &email).await? {
        Some(u) => u,
        None => {
            if !st.cfg.open_signup {
                return Err(HubError::Forbidden("pendaftaran ditutup; minta undangan dari pemilik repo".into()));
            }
            let u = UserRow { id: new_id(), email: email.clone(), name: default_name(ident, &email), avatar_url: ident.avatar_url.clone(), disabled: false, created_at: now.clone(), updated_at: now.clone() };
            st.store.upsert("hub_user", &u.id, &u, None).await?;
            u
        }
    };
    if user.disabled {
        return Err(HubError::Forbidden("akun dinonaktifkan oleh admin hub".into()));
    }
    let row = IdentityRow { id: iid.clone(), provider: provider.as_str().into(), provider_id: ident.provider_id.clone(), user_id: user.id.clone(), email, email_verified: true, last_login_at: Some(now.clone()), created_at: now };
    st.store.upsert("hub_identity", &iid, &row, None).await?;
    Ok(user)
}

fn refresh_expiry(st: &HubState) -> String {
    (chrono::Utc::now() + chrono::Duration::days(st.cfg.refresh_days.max(1))).format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Terbitkan access token + sesi refresh baru.
pub async fn issue_tokens(st: &HubState, user: &UserRow, label: &str) -> Result<TokenOut> {
    let access = super::jwt::issue(user, &st.secrets.jwt, st.cfg.access_minutes)?;
    let refresh = random_hex(48);
    let row = SessionRow { id: new_id(), user_id: user.id.clone(), refresh_hash: hash_hex(&refresh), label: label.chars().take(64).collect(), expires_at: refresh_expiry(st), created_at: now_rfc3339(), last_used_at: None };
    st.store.upsert("hub_session", &row.id, &row, None).await?;
    Ok(TokenOut { access_token: access, refresh_token: refresh, expires_in: st.cfg.access_minutes.max(1) * 60, user: user.out() })
}

async fn session_by_token(st: &HubState, refresh_token: &str) -> Result<Option<SessionRow>> {
    if refresh_token.trim().is_empty() || refresh_token.len() > 256 {
        return Ok(None);
    }
    let mut rows: Vec<SessionRow> = st.store.rows_as("SELECT * FROM hub_session WHERE refresh_hash = $h LIMIT 1;", binds! {"h" => hash_hex(refresh_token.trim())}).await?;
    Ok(rows.pop())
}

/// Putar refresh token (yang lama langsung mati) dan terbitkan access token baru.
pub async fn refresh(st: &HubState, refresh_token: &str) -> Result<TokenOut> {
    let now = now_rfc3339();
    let sess = session_by_token(st, refresh_token).await?.filter(|s| s.expires_at > now).ok_or_else(|| HubError::Unauthorized("sesi tidak dikenal atau sudah kedaluwarsa".into()))?;
    let user = find_user(st, &sess.user_id).await?.filter(|u| !u.disabled).ok_or_else(|| HubError::Unauthorized("akun tidak aktif".into()))?;
    let new_refresh = random_hex(48);
    st.store.patch("hub_session", &sess.id, serde_json::json!({"refresh_hash": hash_hex(&new_refresh), "expires_at": refresh_expiry(st), "last_used_at": now})).await?;
    let access = super::jwt::issue(&user, &st.secrets.jwt, st.cfg.access_minutes)?;
    Ok(TokenOut { access_token: access, refresh_token: new_refresh, expires_in: st.cfg.access_minutes.max(1) * 60, user: user.out() })
}

/// Akhiri sesi. Selalu sukses: token yang tak dikenal berarti sesinya memang sudah tidak ada.
pub async fn logout(st: &HubState, refresh_token: &str) -> Result<Option<String>> {
    let Some(sess) = session_by_token(st, refresh_token).await? else { return Ok(None) };
    st.store.delete("hub_session", &sess.id).await?;
    Ok(Some(sess.user_id))
}
