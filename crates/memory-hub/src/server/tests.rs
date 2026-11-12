use std::sync::Arc;

use async_trait::async_trait;
use memory_core::config::{HubConfig, IndexConfig};
use memory_store::Store;
use serde_json::{Value, json};

use super::auth::github::GithubApi;
use super::auth::providers::{ExchangeCtx, Provider, ProviderClient, ProviderIdentity};
use super::authz::Role;
use super::{HubError, HubSecrets, HubState, Result, router};
use crate::types::*;

/// Kode = `<provider_id>|<email>|<verified 1/0>|<nama>`.
struct MockProvider;

#[async_trait]
impl ProviderClient for MockProvider {
    async fn exchange(&self, p: Provider, code: &str, ctx: &ExchangeCtx) -> Result<ProviderIdentity> {
        assert!(ctx.redirect_uri.ends_with(&format!("/hub/auth/callback/{}", p.as_str())));
        if code == "bad" {
            return Err(HubError::OAuth("kode ditolak provider".into()));
        }
        let parts: Vec<&str> = code.split('|').collect();
        Ok(ProviderIdentity { provider_id: parts[0].into(), email: parts[1].into(), email_verified: parts[2] == "1", name: Some(parts[3].into()), avatar_url: None })
    }
}

struct MockGithub;

#[async_trait]
impl GithubApi for MockGithub {
    async fn repo_role(&self, token: &str, _full: &str) -> Result<Option<Role>> {
        Ok(match token {
            "gh-admin" => Some(Role::Maintainer),
            "gh-push" => Some(Role::Member),
            "gh-none" => None,
            _ => return Err(HubError::Forbidden("token GitHub ditolak".into())),
        })
    }
}

async fn spawn() -> (String, HubState) {
    let store = Store::memory(16, &IndexConfig::default()).await.unwrap();
    let cfg = HubConfig { enabled: true, base_url: "http://127.0.0.1:1".into(), google_client_id: "gid".into(), github_client_id: "ghid".into(), ..Default::default() };
    let st = HubState::with_parts(store, cfg, HubSecrets::for_tests(), reqwest::Client::new(), Arc::new(MockProvider), Arc::new(MockGithub)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = axum::Router::new().nest("/hub", router(st.clone(), "<html>hub</html>"));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), st)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap()
}

/// Login lewat tiket seperti CLI: mulai, ikuti `state` dari Location, callback, poll.
async fn login(c: &reqwest::Client, base: &str, provider: &str, code: &str) -> std::result::Result<TokenOut, String> {
    let ticket = crate::ids::random_hex(16);
    let r = c.get(format!("{base}/hub/auth/login/{provider}?ticket={ticket}")).send().await.unwrap();
    assert_eq!(r.status(), 307, "{}", r.text().await.unwrap_or_default());
    let loc = r.headers()["location"].to_str().unwrap().to_string();
    let state = loc.split("state=").nth(1).unwrap().split('&').next().unwrap().to_string();
    let cb = c.get(format!("{base}/hub/auth/callback/{provider}?code={}&state={state}", urlenc(code))).send().await.unwrap();
    let cb_status = cb.status();
    let html = cb.text().await.unwrap();
    let p: PollOut = c.post(format!("{base}/hub/auth/ticket/poll")).json(&json!({"ticket": ticket})).send().await.unwrap().json().await.unwrap();
    match p.status.as_str() {
        "completed" => {
            assert!(cb_status.is_success() && html.contains("return to the terminal"), "{cb_status} {html}");
            // Token hanya bisa dijemput sekali.
            let again: PollOut = c.post(format!("{base}/hub/auth/ticket/poll")).json(&json!({"ticket": ticket})).send().await.unwrap().json().await.unwrap();
            assert_eq!(again.status, "pending");
            Ok(p.token.unwrap())
        }
        _ => Err(p.error.unwrap_or_else(|| format!("status {}", p.status))),
    }
}

fn urlenc(s: &str) -> String {
    super::auth::providers::urlencode(s)
}

async fn status_of(r: reqwest::Response) -> (u16, Value) {
    let s = r.status().as_u16();
    (s, r.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn oauth_login_merges_identities_refreshes_and_pats() {
    let (base, _st) = spawn().await;
    let c = client();
    let info: InfoOut = c.get(format!("{base}/hub/api/info")).send().await.unwrap().json().await.unwrap();
    assert_eq!(info.providers, vec!["google", "github"]);
    assert!(c.get(format!("{base}/hub")).send().await.unwrap().text().await.unwrap().contains("hub"));
    assert_eq!(c.get(format!("{base}/hub/auth/login/apple")).send().await.unwrap().status(), 400);

    // Google -> akun baru; GitHub dengan email terverifikasi yang sama -> akun yang sama.
    let ana = login(&c, &base, "google", "g1|Ana@Example.com|1|Ana").await.unwrap();
    assert_eq!((ana.user.email.as_str(), ana.user.name.as_str()), ("ana@example.com", "Ana"));
    let ana2 = login(&c, &base, "github", "gh1|ana@example.com|1|Ana GH").await.unwrap();
    assert_eq!(ana2.user.id, ana.user.id);
    let me: MeOut = c.get(format!("{base}/hub/api/me")).bearer_auth(&ana2.access_token).send().await.unwrap().json().await.unwrap();
    assert_eq!(me.identities.len(), 2);
    assert_eq!(me.user.name, "Ana", "nama tidak ditimpa identitas kedua");
    // Email yang belum terverifikasi ditolak; kode buruk sampai ke tiket sebagai galat.
    assert!(login(&c, &base, "github", "gh2|bob@example.com|0|Bob").await.unwrap_err().contains("belum terverifikasi"));
    assert!(login(&c, &base, "google", "bad").await.unwrap_err().contains("kode ditolak"));
    let bob = login(&c, &base, "google", "g3|bob@example.com|1|Bob").await.unwrap();
    assert_ne!(bob.user.id, ana.user.id);

    // Refresh memutar token; refresh lama mati; access lama masih valid sampai kedaluwarsa.
    let (s, v) = status_of(c.post(format!("{base}/hub/auth/refresh")).json(&json!({"refresh_token": ana.refresh_token})).send().await.unwrap()).await;
    assert_eq!(s, 200, "{v}");
    let new_refresh = v["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(new_refresh, ana.refresh_token);
    assert_eq!(c.post(format!("{base}/hub/auth/refresh")).json(&json!({"refresh_token": ana.refresh_token})).send().await.unwrap().status(), 401);
    assert_eq!(c.get(format!("{base}/hub/api/me")).bearer_auth(&ana.access_token).send().await.unwrap().status(), 200);
    assert_eq!(c.get(format!("{base}/hub/api/me")).send().await.unwrap().status(), 401);
    assert_eq!(c.get(format!("{base}/hub/api/me")).bearer_auth("token.palsu.x").send().await.unwrap().status(), 401);

    // PAT: dibuat lewat JWT, dipakai untuk /me, tidak bisa membuat PAT lain, lalu dicabut.
    let (s, v) = status_of(c.post(format!("{base}/hub/api/me/tokens")).bearer_auth(&ana.access_token).json(&json!({"name": "ci", "scopes": ["repo:read"]})).send().await.unwrap()).await;
    assert_eq!(s, 200, "{v}");
    let pat = v["secret"].as_str().unwrap().to_string();
    assert!(pat.starts_with("engram_pat_"));
    let me_pat: MeOut = c.get(format!("{base}/hub/api/me")).bearer_auth(&pat).send().await.unwrap().json().await.unwrap();
    assert_eq!(me_pat.user.id, ana.user.id);
    assert_eq!(c.post(format!("{base}/hub/api/me/tokens")).bearer_auth(&pat).json(&json!({"name": "x"})).send().await.unwrap().status(), 403);
    let list: Vec<TokenOutRow> = c.get(format!("{base}/hub/api/me/tokens")).bearer_auth(&ana.access_token).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(c.delete(format!("{base}/hub/api/me/tokens/{}", list[0].id)).bearer_auth(&ana.access_token).send().await.unwrap().status(), 200);
    assert_eq!(c.get(format!("{base}/hub/api/me")).bearer_auth(&pat).send().await.unwrap().status(), 401);

    // Logout mengakhiri sesi refresh.
    assert_eq!(c.post(format!("{base}/hub/auth/logout")).json(&json!({"refresh_token": new_refresh})).send().await.unwrap().status(), 200);
    assert_eq!(c.post(format!("{base}/hub/auth/refresh")).json(&json!({"refresh_token": new_refresh})).send().await.unwrap().status(), 401);
}

fn rec(kind: &str, rid: &str, body: Value) -> Value {
    json!({"kind": kind, "rid": rid, "content_hash": crate::ids::hash_hex(&body.to_string()), "body": body})
}

fn lesson_a() -> Value {
    rec("lessons", "aaa111", json!({"title": "Jalankan migrasi sebelum tes", "title_key": "jalankan migrasi sebelum tes", "occurrences": 2, "status": "active"}))
}

#[tokio::test]
async fn repos_join_push_changes_and_roles() {
    let (base, _st) = spawn().await;
    let c = client();
    let ana = login(&c, &base, "google", "g1|ana@example.com|1|Ana").await.unwrap();
    let budi = login(&c, &base, "github", "gh2|budi@example.com|1|Budi").await.unwrap();
    let cici = login(&c, &base, "google", "g4|cici@example.com|1|Cici").await.unwrap();
    let auth = |t: &TokenOut| t.access_token.clone();

    // Ana membuat repo; join key hanya tampil sekali.
    let (s, v) = status_of(c.post(format!("{base}/hub/api/repos")).bearer_auth(auth(&ana)).json(&json!({"name": "api-a", "project": "api-a", "remote": "git@github.com:tim/api-a.git"})).send().await.unwrap()).await;
    assert_eq!(s, 200, "{v}");
    let repo_id = v["repo"]["id"].as_str().unwrap().to_string();
    let key = v["join_key"].as_str().unwrap().to_string();
    assert!(key.starts_with("ek1_") && v["repo"]["github_full_name"] == "tim/api-a" && v["repo"]["role"] == "owner");

    // Budi: key salah -> 403, key benar -> member, idempoten.
    let (s, _) = status_of(c.post(format!("{base}/hub/api/repos/{repo_id}/join")).bearer_auth(auth(&budi)).json(&json!({"join_key": "ek1_salah"})).send().await.unwrap()).await;
    assert_eq!(s, 403);
    let (s, v) = status_of(c.post(format!("{base}/hub/api/repos/{repo_id}/join")).bearer_auth(auth(&budi)).json(&json!({"join_key": key, "device_name": "mbp-budi"})).send().await.unwrap()).await;
    assert_eq!((s, v["role"].as_str()), (200, Some("member")), "{v}");
    let (_, v) = status_of(c.post(format!("{base}/hub/api/repos/{repo_id}/join")).bearer_auth(auth(&budi)).json(&json!({"join_key": key})).send().await.unwrap()).await;
    assert_eq!(v["already_member"], true);
    // Cici lewat verifikasi GitHub (izin admin -> maintainer); token tanpa akses ditolak.
    let (s, _) = status_of(c.post(format!("{base}/hub/api/repos/{repo_id}/join")).bearer_auth(auth(&cici)).json(&json!({"github_token": "gh-none"})).send().await.unwrap()).await;
    assert_eq!(s, 403);
    let (s, v) = status_of(c.post(format!("{base}/hub/api/repos/{repo_id}/join")).bearer_auth(auth(&cici)).json(&json!({"github_token": "gh-admin"})).send().await.unwrap()).await;
    assert_eq!((s, v["role"].as_str()), (200, Some("maintainer")), "{v}");

    // Ana push 3 record (full) -> seq 1..3; push ulang identik -> tidak ada perubahan.
    let push = |who: &TokenOut, body: Value| {
        let (c, url, tok) = (c.clone(), format!("{base}/hub/api/repos/{repo_id}/push"), auth(who));
        async move { status_of(c.post(url).bearer_auth(tok).json(&body).send().await.unwrap()).await }
    };
    let l2 = rec("lessons", "bbb222", json!({"title": "Pakai cargo test -p", "status": "active"}));
    let f1 = rec("facts", "ccc333", json!({"statement": "api-a memakai Redis", "predicate": "memakai", "object": "Redis", "status": "active"}));
    let (s, v) = push(&ana, json!({"device_id": "dev-ana", "device_name": "mbp-ana", "full": true, "upserts": [lesson_a(), l2.clone(), f1.clone()]})).await;
    assert_eq!((s, v["accepted"].as_u64(), v["head_seq"].as_u64()), (200, Some(3), Some(3)), "{v}");
    let (_, v) = push(&ana, json!({"device_id": "dev-ana", "device_name": "mbp-ana", "full": true, "upserts": [lesson_a(), l2, f1]})).await;
    assert_eq!((v["accepted"].as_u64(), v["skipped"].as_u64(), v["head_seq"].as_u64()), (Some(0), Some(3), Some(3)), "{v}");
    // Record yang masih memuat penanda redaksi ditolak di hub.
    let (_, v) = push(&ana, json!({"device_id": "dev-ana", "device_name": "mbp-ana", "upserts": [rec("facts", "ddd444", json!({"statement": "token <redacted type='api_key'/> bocor", "status": "active"}))]})).await;
    assert_eq!((v["rejected"].as_u64(), v["accepted"].as_u64()), (Some(1), Some(0)), "{v}");

    // Budi menarik perubahan: 3 record, paging 2+1, head 3.
    let changes = |who: &TokenOut, since: u64, limit: usize| {
        let (c, url, tok) = (c.clone(), format!("{base}/hub/api/repos/{repo_id}/changes?since={since}&limit={limit}"), auth(who));
        async move { c.get(url).bearer_auth(tok).send().await.unwrap().json::<ChangesOut>().await.unwrap() }
    };
    let all = changes(&budi, 0, 500).await;
    assert_eq!((all.records.len(), all.head, all.next), (3, 3, None));
    assert!(all.records.iter().all(|r| r.author == "Ana" && r.device == "mbp-ana" && r.status == "active"));
    let p1 = changes(&budi, 0, 2).await;
    assert_eq!((p1.records.len(), p1.next), (2, Some(2)));
    let p2 = changes(&budi, 2, 2).await;
    assert_eq!((p2.records.len(), p2.next, p2.records[0].rid.as_str()), (1, None, "ccc333"));

    // Push full dengan satu record tersisa -> dua lainnya dicabut (seq 4,5) dan terlihat oleh Budi.
    let (_, v) = push(&ana, json!({"device_id": "dev-ana", "device_name": "mbp-ana", "full": true, "upserts": [lesson_a()]})).await;
    assert_eq!((v["withdrawn"].as_u64(), v["head_seq"].as_u64()), (Some(2), Some(5)), "{v}");
    let w = changes(&budi, 3, 500).await;
    assert_eq!(w.records.len(), 2);
    assert!(w.records.iter().all(|r| r.status == "withdrawn"));
    // Withdraw eksplisit lalu record muncul lagi (reaktivasi) dengan seq baru.
    let (_, v) = push(&ana, json!({"device_id": "dev-ana", "device_name": "mbp-ana", "withdraws": [{"kind": "lessons", "rid": "aaa111"}]})).await;
    assert_eq!(v["withdrawn"].as_u64(), Some(1));
    let (_, v) = push(&ana, json!({"device_id": "dev-ana", "device_name": "mbp-ana", "upserts": [lesson_a()]})).await;
    assert_eq!((v["accepted"].as_u64(), v["head_seq"].as_u64()), (Some(1), Some(7)), "{v}");

    // Status & anggota; email anggota hanya untuk maintainer ke atas.
    let st: StatusOut = c.get(format!("{base}/hub/api/repos/{repo_id}/status")).bearer_auth(auth(&budi)).send().await.unwrap().json().await.unwrap();
    assert_eq!((st.head_seq, st.members, st.per_kind["lessons"], st.per_kind["facts"]), (7, 3, 1, 0));
    assert_eq!(st.last_push.unwrap().author, "Ana");
    let ms: Vec<MemberOut> = c.get(format!("{base}/hub/api/repos/{repo_id}/members")).bearer_auth(auth(&budi)).send().await.unwrap().json().await.unwrap();
    assert_eq!(ms.len(), 3);
    assert!(ms.iter().all(|m| m.email.is_none()));
    let ms: Vec<MemberOut> = c.get(format!("{base}/hub/api/repos/{repo_id}/members")).bearer_auth(auth(&cici)).send().await.unwrap().json().await.unwrap();
    assert!(ms.iter().all(|m| m.email.is_some()));
    assert!(ms.iter().any(|m| m.devices == vec!["mbp-ana".to_string()]));

    // Peran: Budi (member) tidak boleh mengatur peran; Cici (maintainer) menjadikan Budi reader
    // -> push Budi ditolak; maintainer tidak boleh menyentuh owner; owner terakhir tidak bisa keluar.
    let put_role = |who: &TokenOut, uid: &str, role: &str| {
        let (c, url, tok) = (c.clone(), format!("{base}/hub/api/repos/{repo_id}/members/{uid}"), auth(who));
        let body = json!({"role": role});
        async move { c.put(url).bearer_auth(tok).json(&body).send().await.unwrap().status().as_u16() }
    };
    assert_eq!(put_role(&budi, &ana.user.id, "reader").await, 403);
    assert_eq!(put_role(&cici, &budi.user.id, "reader").await, 200);
    assert_eq!(put_role(&cici, &ana.user.id, "member").await, 403);
    assert_eq!(put_role(&cici, &budi.user.id, "owner").await, 403);
    let (s, _) = push(&budi, json!({"device_id": "dev-budi", "upserts": [rec("lessons", "eee555", json!({"title": "x", "status": "active"}))]})).await;
    assert_eq!(s, 403);
    assert_eq!(c.delete(format!("{base}/hub/api/repos/{repo_id}/members/{}", ana.user.id)).bearer_auth(auth(&ana)).send().await.unwrap().status(), 403);
    assert_eq!(put_role(&ana, &budi.user.id, "owner").await, 200);
    assert_eq!(c.delete(format!("{base}/hub/api/repos/{repo_id}/members/{}", ana.user.id)).bearer_auth(auth(&ana)).send().await.unwrap().status(), 200, "owner kedua ada, Ana boleh keluar");

    // Cici (maintainer) push lalu dicabut Budi (owner): record Cici tampak dicabut bagi penarik.
    let (s, _) = push(&cici, json!({"device_id": "dev-cici", "device_name": "pc-cici", "upserts": [rec("decisions", "fff666", json!({"situation": "s", "action": "a", "status": "active"}))]})).await;
    assert_eq!(s, 200);
    let (s, v) = status_of(c.delete(format!("{base}/hub/api/repos/{repo_id}/members/{}", cici.user.id)).bearer_auth(auth(&budi)).send().await.unwrap()).await;
    assert_eq!((s, v["withdrawn"].as_u64()), (200, Some(1)), "{v}");
    let after = changes(&budi, 0, 500).await;
    assert!(after.records.iter().filter(|r| r.user_id == cici.user.id).all(|r| r.status == "withdrawn"));
    assert_eq!(c.get(format!("{base}/hub/api/repos/{repo_id}/changes")).bearer_auth(auth(&cici)).send().await.unwrap().status(), 403);
    // Ana yang sudah keluar: record-nya juga dicabut, dan ia tidak lagi bisa membaca.
    assert!(after.records.iter().filter(|r| r.user_id == ana.user.id).all(|r| r.status == "withdrawn"));
    assert_eq!(c.get(format!("{base}/hub/api/repos/{repo_id}/changes")).bearer_auth(auth(&ana)).send().await.unwrap().status(), 403);

    // Rotasi key: key lama ditolak, key baru diterima (Ana bergabung lagi).
    let (s, v) = status_of(c.post(format!("{base}/hub/api/repos/{repo_id}/key/rotate")).bearer_auth(auth(&budi)).send().await.unwrap()).await;
    assert_eq!((s, v["version"].as_u64()), (200, Some(2)), "{v}");
    let key2 = v["join_key"].as_str().unwrap().to_string();
    let (s, _) = status_of(c.post(format!("{base}/hub/api/repos/{repo_id}/join")).bearer_auth(auth(&ana)).json(&json!({"join_key": key})).send().await.unwrap()).await;
    assert_eq!(s, 403);
    let (s, v) = status_of(c.post(format!("{base}/hub/api/repos/{repo_id}/join")).bearer_auth(auth(&ana)).json(&json!({"join_key": key2})).send().await.unwrap()).await;
    assert_eq!((s, v["role"].as_str()), (200, Some("member")), "{v}");

    // PAT hanya-baca: boleh menarik, tidak boleh push; PAT terbatas repo lain ditolak.
    let (_, v) = status_of(c.post(format!("{base}/hub/api/me/tokens")).bearer_auth(auth(&budi)).json(&json!({"name": "ro", "scopes": ["repo:read"]})).send().await.unwrap()).await;
    let ro = v["secret"].as_str().unwrap().to_string();
    assert_eq!(c.get(format!("{base}/hub/api/repos/{repo_id}/changes")).bearer_auth(&ro).send().await.unwrap().status(), 200);
    assert_eq!(c.post(format!("{base}/hub/api/repos/{repo_id}/push")).bearer_auth(&ro).json(&json!({"device_id": "ci", "upserts": []})).send().await.unwrap().status(), 403);
    let (_, v) = status_of(c.post(format!("{base}/hub/api/me/tokens")).bearer_auth(auth(&budi)).json(&json!({"name": "lain", "repo_ids": ["01lain"]})).send().await.unwrap()).await;
    assert_eq!(c.get(format!("{base}/hub/api/repos/{repo_id}/changes")).bearer_auth(v["secret"].as_str().unwrap()).send().await.unwrap().status(), 403);

    // Aktivitas tercatat; daftar repo milik Budi.
    let act: Vec<AuditOut> = c.get(format!("{base}/hub/api/repos/{repo_id}/activity")).bearer_auth(auth(&budi)).send().await.unwrap().json().await.unwrap();
    assert!(act.iter().any(|a| a.action == "push") && act.iter().any(|a| a.action == "key.rotate") && act.iter().any(|a| a.action == "join.fail"));
    let mine: Vec<RepoOut> = c.get(format!("{base}/hub/api/repos")).bearer_auth(auth(&budi)).send().await.unwrap().json().await.unwrap();
    assert_eq!((mine.len(), mine[0].role.as_str(), mine[0].join_key_version), (1, "owner", 2));

    // Lockout: 10 key salah dari IP ini mengunci join repo lain selama 15 menit.
    let (_, v) = status_of(c.post(format!("{base}/hub/api/repos")).bearer_auth(auth(&budi)).json(&json!({"project": "api-b"})).send().await.unwrap()).await;
    let r2 = v["repo"]["id"].as_str().unwrap().to_string();
    let k2 = v["join_key"].as_str().unwrap().to_string();
    for _ in 0..10 {
        assert_eq!(c.post(format!("{base}/hub/api/repos/{r2}/join")).bearer_auth(auth(&cici)).json(&json!({"join_key": "ek1_salah"})).send().await.unwrap().status(), 403);
    }
    assert_eq!(c.post(format!("{base}/hub/api/repos/{r2}/join")).bearer_auth(auth(&cici)).json(&json!({"join_key": k2})).send().await.unwrap().status(), 429);
}
