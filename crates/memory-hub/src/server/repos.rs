//! Repo bersama: buat, lihat, ubah, bergabung (join key / verifikasi GitHub), rotasi key,
//! anggota & peran, status, dan aktivitas.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use memory_core::ids::{new_id, now_rfc3339};
use memory_store::binds;

use super::auth::extract::{ClientIp, HubUser};
use super::auth::github::full_name_of;
use super::auth::handlers::memberships_of;
use super::auth::users;
use super::authz::{self, Role};
use super::models::{MemberRow, RepoRow};
use super::{Api, HubError, HubState, Result, audit, sync};
use crate::ids::{constant_time_eq, det_id, hash_hex, new_join_key};
use crate::types::*;

fn clean(s: &str, what: &str, max: usize) -> Result<String> {
    let t: String = s.trim().chars().take(max).collect();
    if t.is_empty() {
        return Err(HubError::BadRequest(format!("{what} wajib diisi")));
    }
    Ok(t)
}

fn clean_share(s: ShareSettings) -> Result<ShareSettings> {
    let kinds: Vec<String> = s.kinds.iter().map(|k| k.trim().to_ascii_lowercase()).filter(|k| !k.is_empty()).collect();
    if let Some(bad) = kinds.iter().find(|k| !crate::ids::is_record_kind(k)) {
        return Err(HubError::BadRequest(format!("jenis record tidak dikenal: {bad}")));
    }
    Ok(ShareSettings { kinds, max_records: s.max_records.clamp(1, 20_000), exclude: s.exclude.into_iter().map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).take(200).collect() })
}

pub(crate) async fn counts(st: &HubState, repo_id: &str) -> Result<BTreeMap<String, u64>> {
    let rows = st.store.rows("SELECT kind, count() AS n FROM hub_record WHERE repo_id = $r AND status = 'active' GROUP BY kind;", binds! {"r" => repo_id}).await?;
    let mut out: BTreeMap<String, u64> = crate::ids::RECORD_KINDS.iter().map(|k| (k.to_string(), 0)).collect();
    for r in rows {
        if let (Some(k), Some(n)) = (r["kind"].as_str(), r["n"].as_u64()) {
            out.insert(k.to_string(), n);
        }
    }
    Ok(out)
}

pub(crate) async fn repo_out(st: &HubState, repo: &RepoRow, role: Role) -> Result<RepoOut> {
    let members = authz::members_of(st, &repo.id).await?.len() as u64;
    Ok(RepoOut {
        id: repo.id.clone(),
        name: repo.name.clone(),
        project: repo.project.clone(),
        remote: repo.remote.clone(),
        github_full_name: repo.github_full_name.clone(),
        role: role.as_str().into(),
        head_seq: repo.seq,
        members,
        counts: counts(st, &repo.id).await?,
        share: repo.settings.clone(),
        join_enabled: repo.join_enabled,
        join_key_version: repo.join_key_version,
        require_github_verify: repo.require_github_verify,
        archived: repo.archived,
        created_at: repo.created_at.clone(),
        updated_at: repo.updated_at.clone(),
    })
}

/// `POST /hub/api/repos`: pembuat menjadi owner; join key hanya ditampilkan sekali.
pub async fn create(State(st): State<HubState>, user: HubUser, ip: ClientIp, Json(b): Json<RepoCreateIn>) -> Api<RepoCreateOut> {
    if user.is_pat() {
        return Err(HubError::Forbidden("buat repo lewat sesi login, bukan token".into()));
    }
    let project = clean(&b.project, "nama proyek", 120)?;
    let name = if b.name.trim().is_empty() { project.clone() } else { clean(&b.name, "nama repo", 120)? };
    let remote = b.remote.as_deref().map(str::trim).filter(|r| !r.is_empty()).map(|r| r.chars().take(300).collect::<String>());
    let github_full_name = b.github_full_name.as_deref().map(str::trim).filter(|g| !g.is_empty()).map(str::to_string).or_else(|| remote.as_deref().and_then(full_name_of));
    if let Some(g) = &github_full_name
        && !super::auth::github::valid_full_name(g)
    {
        return Err(HubError::BadRequest(format!("nama repo GitHub tidak valid: {g}")));
    }
    let key = new_join_key();
    let now = now_rfc3339();
    let repo = RepoRow {
        id: new_id(),
        name,
        project,
        remote,
        github_full_name,
        created_by: user.id.clone(),
        join_key_hash: hash_hex(&key),
        join_key_version: 1,
        join_enabled: true,
        require_github_verify: b.require_github_verify.unwrap_or(st.cfg.require_github_verify),
        seq: 0,
        settings: clean_share(b.share.unwrap_or_default())?,
        archived: false,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    st.store.upsert("hub_repo", &repo.id, &repo, None).await?;
    let m = MemberRow { id: det_id(&[&repo.id, &user.id]), repo_id: repo.id.clone(), user_id: user.id.clone(), role: Role::Owner.as_str().into(), joined_via: "owner".into(), github_verified_at: None, invited_by: None, created_at: now, revoked_at: None, last_push_at: None, devices: vec![] };
    st.store.upsert("hub_member", &m.id, &m, None).await?;
    audit::log(&st, &user.id, &user.name, Some(&repo.id), "repo.create", json!({"name": repo.name, "project": repo.project}), Some(&ip.0)).await;
    Ok(Json(RepoCreateOut { repo: repo_out(&st, &repo, Role::Owner).await?, join_key: key }))
}

pub async fn list(State(st): State<HubState>, user: HubUser) -> Api<Vec<RepoOut>> {
    let mut out = Vec::new();
    for m in memberships_of(&st, &user.id).await? {
        if let Ok(repo) = authz::load_repo(&st, &m.repo_id).await {
            out.push(repo_out(&st, &repo, Role::parse(&m.role).unwrap_or(Role::Reader)).await?);
        }
    }
    Ok(Json(out))
}

pub async fn get_one(State(st): State<HubState>, user: HubUser, Path(id): Path<String>) -> Api<RepoOut> {
    let a = authz::require(&st, &id, &user, Role::Reader, false).await?;
    Ok(Json(repo_out(&st, &a.repo, a.role).await?))
}

pub async fn patch(State(st): State<HubState>, user: HubUser, ip: ClientIp, Path(id): Path<String>, Json(b): Json<RepoPatchIn>) -> Api<RepoOut> {
    let a = authz::require(&st, &id, &user, Role::Maintainer, false).await?;
    let mut p = serde_json::Map::new();
    if let Some(n) = b.name {
        p.insert("name".into(), json!(clean(&n, "nama repo", 120)?));
    }
    if let Some(s) = b.share {
        p.insert("settings".into(), json!(clean_share(s)?));
    }
    if let Some(v) = b.join_enabled {
        p.insert("join_enabled".into(), json!(v));
    }
    if let Some(v) = b.require_github_verify {
        p.insert("require_github_verify".into(), json!(v));
    }
    if let Some(v) = b.archived {
        p.insert("archived".into(), json!(v));
    }
    if let Some(g) = b.github_full_name {
        let g = g.trim().to_string();
        if !g.is_empty() && !super::auth::github::valid_full_name(&g) {
            return Err(HubError::BadRequest(format!("nama repo GitHub tidak valid: {g}")));
        }
        p.insert("github_full_name".into(), if g.is_empty() { Value::Null } else { json!(g) });
    }
    if p.is_empty() {
        return Err(HubError::BadRequest("tidak ada yang diubah".into()));
    }
    p.insert("updated_at".into(), json!(now_rfc3339()));
    st.store.patch("hub_repo", &id, Value::Object(p.clone())).await?;
    audit::log(&st, &user.id, &user.name, Some(&id), "repo.update", json!({"fields": p.keys().collect::<Vec<_>>()}), Some(&ip.0)).await;
    let repo = authz::load_repo(&st, &id).await?;
    Ok(Json(repo_out(&st, &repo, a.role).await?))
}

/// `POST /hub/api/repos/{id}/join`: join key -> member; token GitHub -> peran sesuai izin repo.
pub async fn join(State(st): State<HubState>, user: HubUser, ip: ClientIp, Path(id): Path<String>, Json(b): Json<JoinIn>) -> Api<JoinOut> {
    let repo = authz::load_repo(&st, &id).await?;
    if let Some(m) = authz::member_of(&st, &id, &user.id).await? {
        let role = Role::parse(&m.role).unwrap_or(Role::Reader);
        return Ok(Json(JoinOut { role: role.as_str().into(), already_member: true, repo: repo_out(&st, &repo, role).await? }));
    }
    if repo.archived {
        return Err(HubError::Forbidden("repo diarsipkan".into()));
    }
    let lock_key = format!("join:{id}:{}", ip.0);
    if st.limiter.blocked(&lock_key, 10, 900) {
        return Err(HubError::TooMany("terlalu banyak percobaan join key yang salah; coba lagi 15 menit lagi".into()));
    }
    let now = now_rfc3339();
    let mut role: Option<Role> = None;
    let mut via = String::new();
    if let Some(k) = b.join_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        if !repo.join_enabled {
            return Err(HubError::Forbidden("bergabung dengan join key dimatikan untuk repo ini; minta undangan dari maintainer".into()));
        }
        if constant_time_eq(&hash_hex(k), &repo.join_key_hash) {
            role = Some(Role::Member);
            via.push_str("key");
        } else {
            st.limiter.hit(&lock_key, 10, 900);
            audit::log(&st, &user.id, &user.name, Some(&id), "join.fail", json!({"reason": "join key salah"}), Some(&ip.0)).await;
            return Err(HubError::Forbidden("join key salah (mungkin sudah dirotasi; tarik repo.toml terbaru)".into()));
        }
    }
    let mut gh_verified = None;
    if let Some(tok) = b.github_token.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let full = repo.github_full_name.clone().ok_or_else(|| HubError::BadRequest("repo ini tidak terhubung ke repo GitHub; maintainer bisa mengisinya lewat PATCH github_full_name".into()))?;
        match st.github.repo_role(tok, &full).await? {
            Some(r) => {
                role = Some(role.map_or(r, |x| x.max(r)));
                if !via.is_empty() {
                    via.push('+');
                }
                via.push_str("github");
                gh_verified = Some(now.clone());
            }
            None => {
                audit::log(&st, &user.id, &user.name, Some(&id), "join.fail", json!({"reason": "github menolak"}), Some(&ip.0)).await;
                return Err(HubError::Forbidden(format!("GitHub tidak memberi Anda akses ke {full}")));
            }
        }
    }
    if repo.require_github_verify && gh_verified.is_none() {
        return Err(HubError::Forbidden("repo ini mewajibkan verifikasi GitHub: jalankan `engram repo join --github`".into()));
    }
    let role = role.ok_or_else(|| HubError::BadRequest("sertakan join_key (dari .engram/repo.toml) atau github_token".into()))?;
    let m = MemberRow {
        id: det_id(&[&id, &user.id]),
        repo_id: id.clone(),
        user_id: user.id.clone(),
        role: role.as_str().into(),
        joined_via: via,
        github_verified_at: gh_verified,
        invited_by: None,
        created_at: now,
        revoked_at: None,
        last_push_at: None,
        devices: b.device_name.into_iter().map(|d| d.chars().take(64).collect()).filter(|d: &String| !d.is_empty()).collect(),
    };
    st.store.upsert("hub_member", &m.id, &m, None).await?;
    audit::log(&st, &user.id, &user.name, Some(&id), "join", json!({"role": role.as_str(), "via": m.joined_via}), Some(&ip.0)).await;
    Ok(Json(JoinOut { role: role.as_str().into(), already_member: false, repo: repo_out(&st, &repo, role).await? }))
}

pub async fn rotate_key(State(st): State<HubState>, user: HubUser, ip: ClientIp, Path(id): Path<String>) -> Api<RotateOut> {
    let a = authz::require(&st, &id, &user, Role::Maintainer, true).await?;
    let key = new_join_key();
    let version = a.repo.join_key_version + 1;
    st.store.patch("hub_repo", &id, json!({"join_key_hash": hash_hex(&key), "join_key_version": version, "updated_at": now_rfc3339()})).await?;
    audit::log(&st, &user.id, &user.name, Some(&id), "key.rotate", json!({"version": version}), Some(&ip.0)).await;
    Ok(Json(RotateOut { join_key: key, version }))
}

pub async fn members(State(st): State<HubState>, user: HubUser, Path(id): Path<String>) -> Api<Vec<MemberOut>> {
    let a = authz::require(&st, &id, &user, Role::Reader, false).await?;
    let show_email = a.role >= Role::Maintainer;
    let mut out = Vec::new();
    for m in authz::members_of(&st, &id).await? {
        let u = users::find_user(&st, &m.user_id).await?;
        out.push(MemberOut {
            user_id: m.user_id.clone(),
            name: u.as_ref().map(|u| u.name.clone()).unwrap_or_default(),
            email: if show_email { u.map(|u| u.email) } else { None },
            role: m.role,
            joined_via: m.joined_via,
            github_verified_at: m.github_verified_at,
            devices: m.devices,
            last_push_at: m.last_push_at,
            joined_at: m.created_at,
        });
    }
    Ok(Json(out))
}

async fn owners(st: &HubState, repo_id: &str) -> Result<usize> {
    Ok(authz::members_of(st, repo_id).await?.iter().filter(|m| m.role == Role::Owner.as_str()).count())
}

/// `PUT /hub/api/repos/{id}/members/{uid} {role}`: maintainer mengatur reader/member/maintainer;
/// hanya owner yang boleh memberi/mencabut owner; owner terakhir tidak bisa diturunkan.
pub async fn set_role(State(st): State<HubState>, user: HubUser, ip: ClientIp, Path((id, uid)): Path<(String, String)>, Json(b): Json<RoleIn>) -> Api<Value> {
    let a = authz::require(&st, &id, &user, Role::Maintainer, true).await?;
    let target = authz::member_of(&st, &id, &uid).await?.ok_or_else(|| HubError::NotFound("anggota tidak ditemukan".into()))?;
    let new_role = Role::parse(&b.role).ok_or_else(|| HubError::BadRequest("peran tidak dikenal (reader, member, maintainer, owner)".into()))?;
    let cur = Role::parse(&target.role).unwrap_or(Role::Reader);
    if (new_role == Role::Owner || cur == Role::Owner) && a.role < Role::Owner {
        return Err(HubError::Forbidden("hanya owner yang boleh mengubah peran owner".into()));
    }
    if cur == Role::Owner && new_role != Role::Owner && owners(&st, &id).await? <= 1 {
        return Err(HubError::Forbidden("owner terakhir tidak bisa diturunkan; angkat owner lain dulu".into()));
    }
    st.store.patch("hub_member", &target.id, json!({"role": new_role.as_str()})).await?;
    audit::log(&st, &user.id, &user.name, Some(&id), "role.set", json!({"user_id": uid, "from": cur.as_str(), "to": new_role.as_str()}), Some(&ip.0)).await;
    Ok(Json(json!({"ok": true, "role": new_role.as_str()})))
}

/// Mencabut keanggotaan (maintainer+) atau keluar sendiri. Record milik anggota itu ditandai
/// dicabut sehingga rekan mengkarantinanya.
pub async fn revoke(State(st): State<HubState>, user: HubUser, ip: ClientIp, Path((id, uid)): Path<(String, String)>) -> Api<Value> {
    let a = if uid == user.id { authz::require(&st, &id, &user, Role::Reader, false).await? } else { authz::require(&st, &id, &user, Role::Maintainer, true).await? };
    let target = authz::member_of(&st, &id, &uid).await?.ok_or_else(|| HubError::NotFound("anggota tidak ditemukan".into()))?;
    let cur = Role::parse(&target.role).unwrap_or(Role::Reader);
    if cur == Role::Owner {
        if a.role < Role::Owner {
            return Err(HubError::Forbidden("hanya owner yang boleh mencabut owner".into()));
        }
        if owners(&st, &id).await? <= 1 {
            return Err(HubError::Forbidden("owner terakhir tidak bisa keluar; alihkan kepemilikan dulu".into()));
        }
    }
    st.store.patch("hub_member", &target.id, json!({"revoked_at": now_rfc3339()})).await?;
    let withdrawn = sync::withdraw_all(&st, &id, &uid).await?;
    audit::log(&st, &user.id, &user.name, Some(&id), if uid == user.id { "member.leave" } else { "member.revoke" }, json!({"user_id": uid, "withdrawn": withdrawn}), Some(&ip.0)).await;
    Ok(Json(json!({"ok": true, "withdrawn": withdrawn})))
}

pub async fn status(State(st): State<HubState>, user: HubUser, Path(id): Path<String>) -> Api<StatusOut> {
    let a = authz::require(&st, &id, &user, Role::Reader, false).await?;
    let last = st.store.rows("SELECT user_id, author, updated_at FROM hub_record WHERE repo_id = $r ORDER BY seq DESC LIMIT 1;", binds! {"r" => id}).await?;
    let last_push = last.first().map(|r| LastPush { user_id: r["user_id"].as_str().unwrap_or("").into(), author: r["author"].as_str().unwrap_or("").into(), at: r["updated_at"].as_str().unwrap_or("").into() });
    Ok(Json(StatusOut { head_seq: a.repo.seq, members: authz::members_of(&st, &id).await?.len() as u64, per_kind: counts(&st, &id).await?, last_push }))
}

#[derive(Deserialize, Default)]
pub struct LimitQuery {
    pub limit: Option<usize>,
}

pub async fn activity(State(st): State<HubState>, user: HubUser, Path(id): Path<String>, Query(q): Query<LimitQuery>) -> Api<Vec<AuditOut>> {
    authz::require(&st, &id, &user, Role::Reader, false).await?;
    let rows = audit::list(&st, &id, q.limit.unwrap_or(100)).await?;
    Ok(Json(rows.into_iter().map(|r| AuditOut { at: r.at, actor: r.actor, actor_name: r.actor_name, action: r.action, detail: r.detail }).collect()))
}
