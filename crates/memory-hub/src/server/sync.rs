//! Change log per repo: push idempoten (per pengguna+perangkat) dan pull berbasis cursor `seq`.
//! Record disimpan apa adanya (sudah terdistilasi & disaring privasi di klien); hub menolak
//! body yang masih memuat penanda redaksi sebagai pertahanan kedua.

use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::json;

use memory_core::ids::now_rfc3339;
use memory_store::binds;

use super::auth::extract::{ClientIp, HubUser};
use super::authz::{self, Role};
use super::models::{RecordRow, RepoRow};
use super::{Api, HubError, HubState, Result, audit};
use crate::ids::{det_id, is_record_kind, is_valid_key, is_valid_rid};
use crate::types::*;

/// Naikkan counter repo sebanyak `n` secara atomik; mengembalikan `seq` terakhir yang dipesan.
pub(crate) async fn reserve_seq(st: &HubState, repo_id: &str, n: u64) -> Result<u64> {
    let rows: Vec<RepoRow> = st
        .store
        .rows_as("UPDATE type::record('hub_repo', $id) SET seq += $n, updated_at = $now RETURN AFTER;", binds! {"id" => repo_id, "n" => n, "now" => now_rfc3339()})
        .await?;
    rows.first().map(|r| r.seq).ok_or_else(|| HubError::NotFound("repo tidak ditemukan".into()))
}

fn record_out(r: RecordRow) -> RecordOut {
    RecordOut { seq: r.seq, user_id: r.user_id, author: r.author, device_id: r.device_id, device: r.device, kind: r.kind, rid: r.rid, status: r.status, body: r.body, updated_at: r.updated_at }
}

enum Change {
    Upsert(RecordIn),
    Withdraw(String),
}

/// `POST /hub/api/repos/{id}/push`
pub async fn push(State(st): State<HubState>, user: HubUser, ip: ClientIp, Path(id): Path<String>, Json(b): Json<PushIn>) -> Api<PushOut> {
    let a = authz::require(&st, &id, &user, Role::Member, true).await?;
    let device_id = b.device_id.trim().to_string();
    if !is_valid_key(&device_id) {
        return Err(HubError::BadRequest("device_id tidak valid".into()));
    }
    let device: String = b.device_name.trim().chars().take(64).collect();
    if b.upserts.len() + b.withdraws.len() > st.cfg.max_records_per_push {
        return Err(HubError::BadRequest(format!("terlalu banyak record dalam satu push (maks {})", st.cfg.max_records_per_push)));
    }
    let lock = st.repo_lock(&id);
    let _g = lock.lock().await;

    let existing: Vec<RecordRow> = st.store.rows_as("SELECT * FROM hub_record WHERE repo_id = $r AND user_id = $u AND device_id = $d;", binds! {"r" => id, "u" => user.id, "d" => device_id}).await?;
    let by_key: HashMap<String, RecordRow> = existing.into_iter().map(|r| (format!("{}/{}", r.kind, r.rid), r)).collect();
    let mut changes: Vec<Change> = Vec::new();
    let mut out = PushOut { head_seq: a.repo.seq, ..Default::default() };
    let mut seen: HashSet<String> = HashSet::new();
    for u in b.upserts {
        let key = format!("{}/{}", u.kind, u.rid);
        if !is_record_kind(&u.kind) || !is_valid_rid(&u.rid) || !u.body.is_object() || !seen.insert(key.clone()) {
            out.rejected += 1;
            continue;
        }
        let bytes = serde_json::to_vec(&u.body).map(|v| v.len()).unwrap_or(usize::MAX);
        if bytes > st.cfg.max_record_bytes || memory_core::privacy::has_redaction_marker(&u.body.to_string()) {
            out.rejected += 1;
            continue;
        }
        if by_key.get(&key).is_some_and(|e| e.status == "active" && e.content_hash == u.content_hash) {
            out.skipped += 1;
            continue;
        }
        changes.push(Change::Upsert(u));
    }
    let mut withdraw_keys: HashSet<String> = HashSet::new();
    for w in &b.withdraws {
        let key = format!("{}/{}", w.kind, w.rid);
        if by_key.get(&key).is_some_and(|e| e.status == "active") && !seen.contains(&key) && withdraw_keys.insert(key.clone()) {
            changes.push(Change::Withdraw(key));
        } else {
            out.skipped += 1;
        }
    }
    if b.full {
        if b.keep.len() > st.cfg.max_records_per_member {
            return Err(HubError::BadRequest("daftar keep terlalu panjang".into()));
        }
        let keep: HashSet<String> = b.keep.iter().map(|k| format!("{}/{}", k.kind, k.rid)).collect();
        for (key, e) in &by_key {
            if e.status == "active" && !seen.contains(key) && !keep.contains(key) && withdraw_keys.insert(key.clone()) {
                changes.push(Change::Withdraw(key.clone()));
            }
        }
    }
    let active_now = by_key.values().filter(|e| e.status == "active").count();
    let new_rows = changes.iter().filter(|c| matches!(c, Change::Upsert(u) if by_key.get(&format!("{}/{}", u.kind, u.rid)).is_none_or(|e| e.status != "active"))).count();
    if active_now + new_rows > st.cfg.max_records_per_member {
        return Err(HubError::BadRequest(format!("melebihi batas {} record per anggota", st.cfg.max_records_per_member)));
    }
    if changes.is_empty() {
        return Ok(Json(out));
    }
    let n = changes.len() as u64;
    let head = reserve_seq(&st, &id, n).await?;
    let base = head - n;
    let now = now_rfc3339();
    let mut tx = st.store.tx();
    for (i, ch) in changes.into_iter().enumerate() {
        let seq = base + i as u64 + 1;
        match ch {
            Change::Upsert(u) => {
                let key = format!("{}/{}", u.kind, u.rid);
                let prev = by_key.get(&key);
                let row = RecordRow {
                    id: det_id(&[&id, &user.id, &device_id, &u.kind, &u.rid]),
                    repo_id: id.clone(),
                    user_id: user.id.clone(),
                    device_id: device_id.clone(),
                    device: device.clone(),
                    author: user.name.clone(),
                    kind: u.kind,
                    rid: u.rid,
                    body: u.body,
                    content_hash: u.content_hash,
                    status: "active".into(),
                    seq,
                    created_at: prev.map(|p| p.created_at.clone()).unwrap_or_else(|| now.clone()),
                    updated_at: now.clone(),
                    withdrawn_at: None,
                };
                tx.upsert("hub_record", &row.id, &row, None)?;
                out.accepted += 1;
            }
            Change::Withdraw(key) => {
                if let Some(e) = by_key.get(&key) {
                    tx.patch("hub_record", &e.id, json!({"status": "withdrawn", "withdrawn_at": now, "updated_at": now, "seq": seq}));
                    out.withdrawn += 1;
                }
            }
        }
    }
    tx.commit().await?;
    out.head_seq = head;
    let mut devices = a.member.devices.clone();
    if !device.is_empty() && !devices.contains(&device) {
        devices.push(device.clone());
    }
    st.store.patch("hub_member", &a.member.id, json!({"last_push_at": now, "devices": devices})).await?;
    audit::log(&st, &user.id, &user.name, Some(&id), "push", json!({"accepted": out.accepted, "withdrawn": out.withdrawn, "rejected": out.rejected, "device": device}), Some(&ip.0)).await;
    Ok(Json(out))
}

/// Tandai semua record aktif milik `user_id` di repo sebagai dicabut (anggota keluar/dicabut).
pub(crate) async fn withdraw_all(st: &HubState, repo_id: &str, user_id: &str) -> Result<u32> {
    let lock = st.repo_lock(repo_id);
    let _g = lock.lock().await;
    let rows: Vec<RecordRow> = st.store.rows_as("SELECT * FROM hub_record WHERE repo_id = $r AND user_id = $u AND status = 'active';", binds! {"r" => repo_id, "u" => user_id}).await?;
    if rows.is_empty() {
        return Ok(0);
    }
    let n = rows.len() as u64;
    let head = reserve_seq(st, repo_id, n).await?;
    let base = head - n;
    let now = now_rfc3339();
    let mut tx = st.store.tx();
    for (i, r) in rows.iter().enumerate() {
        tx.patch("hub_record", &r.id, json!({"status": "withdrawn", "withdrawn_at": now, "updated_at": now, "seq": base + i as u64 + 1}));
    }
    tx.commit().await?;
    Ok(n as u32)
}

#[derive(Deserialize, Default)]
pub struct ChangesQuery {
    pub since: Option<u64>,
    pub limit: Option<usize>,
}

/// `GET /hub/api/repos/{id}/changes?since=&limit=`: record dengan `seq > since`, urut naik.
pub async fn changes(State(st): State<HubState>, user: HubUser, Path(id): Path<String>, Query(q): Query<ChangesQuery>) -> Api<ChangesOut> {
    authz::require(&st, &id, &user, Role::Reader, false).await?;
    let since = q.since.unwrap_or(0);
    let limit = q.limit.unwrap_or(500).clamp(1, 1000);
    let mut rows: Vec<RecordRow> = st
        .store
        .rows_as(&format!("SELECT * FROM hub_record WHERE repo_id = $r AND seq > $s ORDER BY seq ASC LIMIT {};", limit + 1), binds! {"r" => id, "s" => since})
        .await?;
    let next = if rows.len() > limit {
        rows.truncate(limit);
        rows.last().map(|r| r.seq)
    } else {
        None
    };
    // Head dibaca setelah halaman agar cursor klien tidak melompati seq yang belum terbaca.
    let head = authz::load_repo(&st, &id).await?.seq;
    Ok(Json(ChangesOut { head, next, records: rows.into_iter().map(record_out).collect() }))
}

#[derive(Deserialize, Default)]
pub struct RecordsQuery {
    pub kind: Option<String>,
    pub user: Option<String>,
    pub limit: Option<usize>,
}

/// `GET /hub/api/repos/{id}/records`: jelajah record aktif (UI).
pub async fn records(State(st): State<HubState>, user: HubUser, Path(id): Path<String>, Query(q): Query<RecordsQuery>) -> Api<Vec<RecordOut>> {
    authz::require(&st, &id, &user, Role::Reader, false).await?;
    let mut cond = String::from("repo_id = $r AND status = 'active'");
    let mut b = binds! {"r" => id};
    if let Some(k) = q.kind.as_deref().filter(|k| is_record_kind(k)) {
        cond.push_str(" AND kind = $k");
        b.insert("k".into(), json!(k));
    }
    if let Some(u) = q.user.as_deref().filter(|u| is_valid_key(u)) {
        cond.push_str(" AND user_id = $u");
        b.insert("u".into(), json!(u));
    }
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let rows: Vec<RecordRow> = st.store.rows_as(&format!("SELECT * FROM hub_record WHERE {cond} ORDER BY seq DESC LIMIT {limit};"), b).await?;
    Ok(Json(rows.into_iter().map(record_out).collect()))
}
