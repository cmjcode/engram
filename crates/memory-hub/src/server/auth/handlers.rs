//! Handler alur login: mulai OAuth, callback, relai tiket (CLI/desktop), refresh, logout,
//! info publik, dan `me`.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use memory_core::ids::now_rfc3339;

use super::super::models::{MemberRow, OauthStateRow, TicketRow};
use super::super::{Api, HubError, HubState, Result, audit, authz};
use super::extract::{ClientIp, HubUser};
use super::providers::{ExchangeCtx, Provider, authorize_url};
use super::users;
use crate::ids::random_hex;
use crate::types::*;

/// Umur nonce `state` dan tiket sejak login dimulai.
const LOGIN_WINDOW_MIN: i64 = 10;
/// Umur tiket setelah tokennya siap dijemput (klien memolling tiap 1,5 detik).
const TICKET_READY_MIN: i64 = 5;

fn minutes_from_now(m: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::minutes(m)).format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn valid_ticket(t: &str) -> bool {
    (8..=64).contains(&t.len()) && t.chars().all(|c| c.is_ascii_alphanumeric())
}

#[derive(Deserialize, Default)]
pub struct LoginQuery {
    pub ticket: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Deserialize)]
pub struct TicketIn {
    pub ticket: String,
}

#[derive(Deserialize)]
pub struct RefreshIn {
    pub refresh_token: String,
}

/// `GET /hub/api/info`: provider yang tersedia (UI membuat tombol dari sini).
pub async fn info(State(st): State<HubState>) -> Json<InfoOut> {
    Json(InfoOut { enabled: true, name: st.cfg.name.clone(), providers: st.providers().iter().map(|p| p.to_string()).collect(), version: env!("CARGO_PKG_VERSION").into(), open_signup: st.cfg.open_signup })
}

/// `GET /hub/auth/login/{provider}?ticket=…`
pub async fn login(State(st): State<HubState>, Path(provider): Path<String>, Query(q): Query<LoginQuery>) -> Result<Response> {
    let p = Provider::parse(&provider).ok_or_else(|| HubError::BadRequest(format!("provider login tidak dikenal: {provider}")))?;
    let ticket = match q.ticket.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) if valid_ticket(t) => Some(t.to_string()),
        Some(_) => return Err(HubError::BadRequest("tiket tidak valid".into())),
        None => None,
    };
    if !st.providers().contains(&p.as_str()) {
        // Pesan ini yang dibaca CLI/desktop lewat tiket, jadi menyebut yang kurang.
        let msg = format!("{} belum dikonfigurasi di hub ini (isi [hub] {}_client_id)", p.label(), p.as_str());
        if let Some(t) = &ticket {
            set_ticket(&st, t, "error", Some(&msg), TICKET_READY_MIN).await;
        }
        return Err(HubError::BadRequest(msg));
    }
    let state = random_hex(32);
    let row = OauthStateRow { id: state.clone(), provider: p.as_str().into(), ticket: ticket.clone(), purpose: "login".into(), repo_id: None, expires_at: minutes_from_now(LOGIN_WINDOW_MIN), created_at: now_rfc3339() };
    st.store.upsert("hub_oauth_state", &row.id, &row, None).await?;
    if let Some(t) = &ticket {
        set_ticket(&st, t, "pending", None, LOGIN_WINDOW_MIN).await;
    }
    let client_id = match p {
        Provider::Google => &st.cfg.google_client_id,
        Provider::GitHub => &st.cfg.github_client_id,
    };
    Ok(Redirect::temporary(&authorize_url(p, client_id, &st.redirect_uri(p), &state)).into_response())
}

/// `GET /hub/auth/callback/{provider}?code=…&state=…`
pub async fn callback(State(st): State<HubState>, Path(provider): Path<String>, Query(q): Query<CallbackQuery>, ip: ClientIp) -> Result<Response> {
    let p = Provider::parse(&provider).ok_or_else(|| HubError::BadRequest(format!("provider login tidak dikenal: {provider}")))?;
    let state = q.state.as_deref().map(str::trim).filter(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())).ok_or_else(|| HubError::BadRequest("sesi login tidak valid".into()))?;
    let row = consume_state(&st, p, state).await?;
    if let Some(err) = q.error.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
        let msg = format!("{} dibatalkan: {err}{}", p.label(), q.error_description.as_deref().map(|d| format!(" ({d})")).unwrap_or_default());
        if let Some(t) = &row.ticket {
            set_ticket(&st, t, "error", Some(&msg), TICKET_READY_MIN).await;
        }
        return Ok(error_page(&msg));
    }
    let code = q.code.as_deref().map(str::trim).filter(|c| !c.is_empty()).ok_or_else(|| HubError::BadRequest("callback tanpa kode otorisasi".into()))?;
    match finish(&st, p, code, row.ticket.is_some(), &ip.0).await {
        Ok(token) => {
            if let Some(t) = &row.ticket {
                let payload = serde_json::to_string(&token)?;
                set_ticket(&st, t, "ready", Some(&payload), TICKET_READY_MIN).await;
                Ok(Html(success_page(&st.cfg.name, None)).into_response())
            } else {
                Ok(Html(success_page(&st.cfg.name, Some(&serde_json::to_string(&token)?))).into_response())
            }
        }
        Err(e) => {
            if let Some(t) = &row.ticket {
                set_ticket(&st, t, "error", Some(&e.to_string()), TICKET_READY_MIN).await;
            }
            Ok(error_page(&e.to_string()))
        }
    }
}

async fn finish(st: &HubState, p: Provider, code: &str, via_ticket: bool, ip: &str) -> Result<TokenOut> {
    let (client_id, client_secret) = match p {
        Provider::Google => (st.cfg.google_client_id.clone(), st.secrets.google_client_secret.clone()),
        Provider::GitHub => (st.cfg.github_client_id.clone(), st.secrets.github_client_secret.clone()),
    };
    let ctx = ExchangeCtx { client_id, client_secret, redirect_uri: st.redirect_uri(p) };
    let ident = st.provider.exchange(p, code, &ctx).await?;
    let user = users::login_identity(st, p, &ident).await?;
    let token = users::issue_tokens(st, &user, &format!("{} via {}", if via_ticket { "cli" } else { "browser" }, p.as_str())).await?;
    audit::log(st, &user.id, &user.name, None, "login", json!({"provider": p.as_str(), "ticket": via_ticket}), Some(ip)).await;
    Ok(token)
}

/// `POST /hub/auth/ticket/poll {ticket}`. Tiket tak dikenal/kedaluwarsa dijawab `pending`
/// (klien memolling sebelum baris tiket ada). `ready`/`error` langsung ditandai `consumed`.
pub async fn poll_ticket(State(st): State<HubState>, Json(b): Json<TicketIn>) -> Api<PollOut> {
    let t = b.ticket.trim();
    if !valid_ticket(t) {
        return Err(HubError::BadRequest("tiket tidak valid".into()));
    }
    let now = now_rfc3339();
    let Some(row) = st.store.get::<TicketRow>("hub_ticket", t).await?.filter(|r| r.expires_at > now) else {
        return Ok(Json(PollOut { status: "pending".into(), ..Default::default() }));
    };
    let out = match row.status.as_str() {
        "error" => PollOut { status: "error".into(), error: Some(row.payload.clone().unwrap_or_else(|| "login gagal di hub".into())), token: None },
        "ready" => match row.payload.as_deref().and_then(|p| serde_json::from_str::<TokenOut>(p).ok()) {
            Some(token) => PollOut { status: "completed".into(), token: Some(token), error: None },
            None => return Ok(Json(PollOut { status: "pending".into(), ..Default::default() })),
        },
        _ => return Ok(Json(PollOut { status: "pending".into(), ..Default::default() })),
    };
    // Token hanya bisa dijemput sekali dan tidak tertinggal di basis data.
    st.store.patch("hub_ticket", t, json!({"status": "consumed", "payload": null})).await?;
    Ok(Json(out))
}

pub async fn refresh(State(st): State<HubState>, Json(b): Json<RefreshIn>) -> Api<TokenOut> {
    Ok(Json(users::refresh(&st, &b.refresh_token).await?))
}

pub async fn logout(State(st): State<HubState>, ip: ClientIp, Json(b): Json<RefreshIn>) -> Api<Value> {
    if let Some(uid) = users::logout(&st, &b.refresh_token).await? {
        audit::log(&st, &uid, "", None, "logout", json!({}), Some(&ip.0)).await;
    }
    Ok(Json(json!({"ok": true})))
}

pub async fn me(State(st): State<HubState>, user: HubUser) -> Api<MeOut> {
    let row = users::find_user(&st, &user.id).await?.ok_or_else(|| HubError::Unauthorized("akun tidak ditemukan".into()))?;
    let identities = users::identities_of(&st, &user.id).await?.into_iter().map(|i| IdentityOut { provider: i.provider, email: i.email, last_login_at: i.last_login_at }).collect();
    let mut repos = Vec::new();
    for m in memberships_of(&st, &user.id).await? {
        if let Ok(repo) = authz::load_repo(&st, &m.repo_id).await {
            repos.push(RepoBrief { id: repo.id, name: repo.name, project: repo.project, role: m.role });
        }
    }
    Ok(Json(MeOut { user: row.out(), identities, repos }))
}

/// Keanggotaan aktif seorang pengguna di semua repo.
pub(crate) async fn memberships_of(st: &HubState, user_id: &str) -> Result<Vec<MemberRow>> {
    let rows: Vec<MemberRow> = st.store.rows_as("SELECT * FROM hub_member WHERE user_id = $u ORDER BY created_at ASC;", memory_store::binds! {"u" => user_id}).await?;
    Ok(rows.into_iter().filter(|m| m.revoked_at.is_none()).collect())
}

// ─── pembantu ───────────────────────────────────────────────────────────────

/// Ambil baris `state` sekaligus menghapusnya: satu callback tidak bisa diputar ulang.
async fn consume_state(st: &HubState, p: Provider, state: &str) -> Result<OauthStateRow> {
    let now = now_rfc3339();
    let row = st.store.get::<OauthStateRow>("hub_oauth_state", state).await?.filter(|r| r.provider == p.as_str() && r.expires_at > now).ok_or_else(|| HubError::BadRequest("sesi login tidak valid atau sudah kedaluwarsa; ulangi login".into()))?;
    st.store.delete("hub_oauth_state", state).await?;
    Ok(row)
}

/// Tulis status tiket (best-effort: klien jatuh ke timeout-nya bila gagal).
async fn set_ticket(st: &HubState, ticket: &str, status: &str, payload: Option<&str>, minutes: i64) {
    let row = TicketRow { id: ticket.to_string(), status: status.into(), payload: payload.map(str::to_string), expires_at: minutes_from_now(minutes) };
    if let Err(e) = st.store.upsert("hub_ticket", ticket, &row, None).await {
        tracing::warn!(error = %e, "gagal menulis tiket login");
    }
}

fn page(title: &str, body: &str, script: &str) -> String {
    format!(
        r#"<!DOCTYPE html><html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{title}</title>
<style>body{{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;background:#0f1115;color:#e8e8ea;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0;padding:24px;box-sizing:border-box}}
.card{{background:#171a21;border:1px solid #2a2f3a;border-radius:14px;padding:2.2rem;max-width:460px;text-align:center}}h1{{font-size:20px;margin:0 0 8px}}p{{color:#a0a6b3;font-size:15px;line-height:1.5;margin:0}}</style></head>
<body><div class="card">{body}</div>{script}</body></html>"#
    )
}

/// Halaman sukses: lewat tiket -> suruh kembali ke terminal/aplikasi; lewat browser -> simpan
/// token di `localStorage` lalu buka UI hub (token tidak pernah lewat URL).
fn success_page(hub_name: &str, browser_token_json: Option<&str>) -> String {
    match browser_token_json {
        None => page("Signed in", &format!("<h1>Signed in to {}</h1><p>You can close this tab and return to the terminal or the engram app.</p>", html_escape(hub_name)), ""),
        Some(json) => page(
            "Signed in",
            &format!("<h1>Signed in to {}</h1><p>Opening the hub…</p>", html_escape(hub_name)),
            &format!(
                "<script>(function(){{var t={};try{{localStorage.setItem('hub_token',t.access_token);localStorage.setItem('hub_refresh',t.refresh_token);localStorage.setItem('hub_user',JSON.stringify(t.user));}}catch(e){{}}location.replace('/hub#hub/repos');}})();</script>",
                escape_for_script(json)
            ),
        ),
    }
}

fn error_page(msg: &str) -> Response {
    (axum::http::StatusCode::BAD_REQUEST, Html(page("Sign-in failed", &format!("<h1>Sign-in failed</h1><p>{}</p>", html_escape(msg)), ""))).into_response()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `serde_json` tidak meng-escape `<`; nama tampilan yang memuat `</script>` bisa menutup blok
/// skrip lebih awal. `\u003c` dkk. tetap JSON & JavaScript yang sah.
fn escape_for_script(json: &str) -> String {
    json.replace('<', "\\u003c").replace('>', "\\u003e").replace('&', "\\u0026")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_escape_untrusted_text() {
        assert!(success_page("T", Some(r#"{"user":{"name":"</script><img src=x>"}}"#)).contains("\\u003c/script\\u003e"));
        assert!(valid_ticket(&random_hex(16)) && !valid_ticket("../x") && !valid_ticket("abc"));
        assert_eq!(html_escape("<b>&"), "&lt;b&gt;&amp;");
    }
}
