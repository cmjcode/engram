//! Urusan per-provider: URL otorisasi, pertukaran kode, pengambilan identitas. Kedua provider
//! mengembalikan [`ProviderIdentity`] yang sama bentuknya. Ditulis tangan di atas `reqwest`
//! (tanpa crate oauth2), mengikuti rastrum-server; tanpa PKCE karena client secret hanya
//! dipakai di server.

use async_trait::async_trait;
use serde::Deserialize;

use super::super::{HubError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Google,
    GitHub,
}

impl Provider {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "google" => Some(Provider::Google),
            "github" => Some(Provider::GitHub),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Google => "google",
            Provider::GitHub => "github",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Google => "Sign in with Google",
            Provider::GitHub => "Sign in with GitHub",
        }
    }
}

/// Identitas seragam dari provider. `provider_id` (klaim `sub` Google / id numerik GitHub)
/// adalah kunci identitas; email hanya untuk menautkan ke akun yang sudah ada.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderIdentity {
    pub provider_id: String,
    pub email: String,
    pub email_verified: bool,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
}

/// Kredensial aplikasi OAuth untuk satu pertukaran kode.
#[derive(Debug, Clone)]
pub struct ExchangeCtx {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

/// Dipisah sebagai trait agar uji bisa memakai provider tiruan tanpa jaringan.
#[async_trait]
pub trait ProviderClient: Send + Sync {
    async fn exchange(&self, provider: Provider, code: &str, ctx: &ExchangeCtx) -> Result<ProviderIdentity>;
}

/// URL otorisasi provider, lengkap dengan nonce `state`.
pub fn authorize_url(provider: Provider, client_id: &str, redirect_uri: &str, state: &str) -> String {
    match provider {
        Provider::Google => format!(
            "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope=openid%20email%20profile&state={}&prompt=select_account",
            urlencode(client_id),
            urlencode(redirect_uri),
            state
        ),
        Provider::GitHub => format!("https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope=read:user%20user:email&state={}", urlencode(client_id), urlencode(redirect_uri), state),
    }
}

/// Percent-encoding per byte (bukan per `char`, agar karakter non-ASCII tidak terpotong).
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub struct ReqwestProviderClient {
    http: reqwest::Client,
}

impl ReqwestProviderClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }
}

#[async_trait]
impl ProviderClient for ReqwestProviderClient {
    async fn exchange(&self, provider: Provider, code: &str, ctx: &ExchangeCtx) -> Result<ProviderIdentity> {
        match provider {
            Provider::Google => exchange_google(&self.http, code, ctx).await,
            Provider::GitHub => exchange_github(&self.http, code, ctx).await,
        }
    }
}

// ─── Google ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GoogleToken {
    access_token: String,
}

#[derive(Deserialize)]
struct GoogleUserInfo {
    sub: String,
    email: Option<String>,
    #[serde(default)]
    email_verified: bool,
    name: Option<String>,
    picture: Option<String>,
}

async fn exchange_google(http: &reqwest::Client, code: &str, ctx: &ExchangeCtx) -> Result<ProviderIdentity> {
    let token: GoogleToken = http
        .post("https://oauth2.googleapis.com/token")
        .form(&[("client_id", ctx.client_id.as_str()), ("client_secret", ctx.client_secret.as_str()), ("redirect_uri", ctx.redirect_uri.as_str()), ("code", code), ("grant_type", "authorization_code")])
        .send()
        .await
        .map_err(|e| HubError::OAuth(format!("Google tidak bisa dihubungi: {e}")))?
        .json()
        .await
        .map_err(|e| HubError::OAuth(format!("balasan token Google tidak terbaca: {e}")))?;
    let info: GoogleUserInfo = http
        .get("https://www.googleapis.com/oauth2/v3/userinfo")
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|e| HubError::OAuth(format!("profil Google tidak bisa diambil: {e}")))?
        .json()
        .await
        .map_err(|e| HubError::OAuth(format!("profil Google tidak terbaca: {e}")))?;
    Ok(ProviderIdentity { provider_id: info.sub, email: info.email.unwrap_or_default(), email_verified: info.email_verified, name: info.name, avatar_url: info.picture })
}

// ─── GitHub ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GitHubToken {
    access_token: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct GitHubUser {
    id: i64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct GitHubEmail {
    pub email: String,
    pub primary: bool,
    pub verified: bool,
}

async fn exchange_github(http: &reqwest::Client, code: &str, ctx: &ExchangeCtx) -> Result<ProviderIdentity> {
    let token: GitHubToken = http
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[("client_id", ctx.client_id.as_str()), ("client_secret", ctx.client_secret.as_str()), ("redirect_uri", ctx.redirect_uri.as_str()), ("code", code)])
        .send()
        .await
        .map_err(|e| HubError::OAuth(format!("GitHub tidak bisa dihubungi: {e}")))?
        .json()
        .await
        .map_err(|e| HubError::OAuth(format!("balasan token GitHub tidak terbaca: {e}")))?;
    let access = token.access_token.ok_or_else(|| HubError::OAuth(format!("GitHub menolak kode login: {}", token.error_description.unwrap_or_else(|| "tanpa keterangan".into()))))?;
    let gh = |url: &str| http.get(url).bearer_auth(&access).header("Accept", "application/vnd.github+json").header("X-GitHub-Api-Version", "2022-11-28");
    let user: GitHubUser = gh("https://api.github.com/user")
        .send()
        .await
        .map_err(|e| HubError::OAuth(format!("profil GitHub tidak bisa diambil: {e}")))?
        .json()
        .await
        .map_err(|e| HubError::OAuth(format!("profil GitHub tidak terbaca: {e}")))?;
    // `/user` tidak memuat email bila disembunyikan; alamat terverifikasi diambil terpisah.
    let emails: Vec<GitHubEmail> = gh("https://api.github.com/user/emails")
        .send()
        .await
        .map_err(|e| HubError::OAuth(format!("email GitHub tidak bisa diambil: {e}")))?
        .json()
        .await
        .map_err(|e| HubError::OAuth(format!("email GitHub tidak terbaca: {e}")))?;
    let email = pick_github_email(emails).ok_or_else(|| HubError::OAuth("akun GitHub ini tidak punya alamat email terverifikasi".into()))?;
    Ok(ProviderIdentity { provider_id: user.id.to_string(), email, email_verified: true, name: user.name.or(Some(user.login)), avatar_url: user.avatar_url })
}

/// Alamat utama + terverifikasi, kalau tidak ada: yang terverifikasi mana pun. Alamat yang belum
/// terverifikasi tidak pernah dipakai (siapa pun bisa menambahkan alamat orang lain ke akunnya).
pub(crate) fn pick_github_email(emails: Vec<GitHubEmail>) -> Option<String> {
    if let Some(primary) = emails.iter().find(|e| e.primary && e.verified).map(|e| e.email.clone()) {
        return Some(primary);
    }
    emails.into_iter().find(|e| e.verified).map(|e| e.email)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_email_choice() {
        let u = authorize_url(Provider::Google, "cid", "https://h.x/hub/auth/callback/google", "st");
        assert!(u.starts_with("https://accounts.google.com/") && u.contains("redirect_uri=https%3A%2F%2Fh.x%2Fhub%2Fauth%2Fcallback%2Fgoogle") && u.contains("state=st"));
        let g = authorize_url(Provider::GitHub, "cid", "https://h.x/cb", "st");
        assert!(g.contains("scope=read:user%20user:email"));
        assert_eq!(urlencode("a b/é"), "a%20b%2F%C3%A9");
        let e = |email: &str, primary, verified| GitHubEmail { email: email.into(), primary, verified };
        assert_eq!(pick_github_email(vec![e("x@a", false, true), e("p@a", true, true)]).as_deref(), Some("p@a"));
        assert_eq!(pick_github_email(vec![e("p@a", true, false), e("v@a", false, true)]).as_deref(), Some("v@a"));
        assert!(pick_github_email(vec![e("p@a", true, false)]).is_none());
        assert_eq!(Provider::parse("GitHub"), Some(Provider::GitHub));
        assert!(Provider::parse("apple").is_none());
    }
}
