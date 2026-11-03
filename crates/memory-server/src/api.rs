//! HTTP API JSON untuk UI web dan integrasi. Semua route di bawah `/api`.

use std::path::PathBuf;

use axum::Router;
use axum::extract::{Multipart, Path, Query, State};
use axum::routing::{get, post};
use axum::Json;
use serde::Deserialize;
use serde_json::{Value, json};

use memory_core::ids::is_valid_id;
use memory_core::model::*;
use memory_core::time::LifeAnchors;
use memory_extract::ingest::IngestDoc;
use memory_extract::review::Decision;
use memory_retrieve::RecallQuery;
use memory_store::EpisodeFilter;

use crate::{ApiError, ApiResult, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/subjects", get(list_subjects).post(upsert_subject))
        .route("/api/subjects/{s}/stats", get(stats))
        .route("/api/subjects/{s}/timeline", get(timeline))
        .route("/api/subjects/{s}/episodes", get(episodes))
        .route("/api/subjects/{s}/episodes/{id}", get(episode))
        .route("/api/subjects/{s}/stream", get(stream_items))
        .route("/api/subjects/{s}/entities", get(entities))
        .route("/api/subjects/{s}/entities/{id}", get(entity))
        .route("/api/subjects/{s}/facts", get(facts))
        .route("/api/subjects/{s}/kin", get(kin))
        .route("/api/subjects/{s}/family", get(family))
        .route("/api/subjects/{s}/principles", get(principles))
        .route("/api/subjects/{s}/dispositions", get(dispositions))
        .route("/api/subjects/{s}/phases", get(phases))
        .route("/api/subjects/{s}/voice", get(voice))
        .route("/api/subjects/{s}/recall", post(recall))
        .route("/api/subjects/{s}/ask", post(ask))
        .route("/api/subjects/{s}/reviews", get(reviews))
        .route("/api/reviews/{id}", post(resolve_review))
        .route("/api/subjects/{s}/sources", get(sources))
        .route("/api/sources/{id}", get(source))
        .route("/api/sources/{id}/retry", post(retry_source))
        .route("/api/jobs/retry-failed", post(retry_failed))
        .route("/api/subjects/{s}/import", post(import))
        .route("/api/subjects/{s}/watched-folders", get(watched_folders_list).post(watched_folders_add))
        .route("/api/subjects/{s}/watched-folders/scan", post(watched_folders_scan_all))
        .route("/api/watched-folders/{id}/scan", post(watched_folder_scan))
        .route("/api/watched-folders/{id}/remove", post(watched_folder_remove))
        .route("/api/subjects/{s}/batches", get(batches))
        .route("/api/batches/{id}", get(batch))
        .route("/api/jobs", get(jobs))
        .route("/api/subjects/{s}/remember", post(remember))
        .route("/api/subjects/{s}/interview/next", get(interview_next))
        .route("/api/subjects/{s}/interview/answer", post(interview_answer))
        .route("/api/subjects/{s}/maintenance/{kind}", post(maintenance))
        .route("/api/subjects/{s}/compact/candidates", get(compact_candidates))
        .route("/api/subjects/{s}/compact/apply", post(compact_apply))
        .route("/api/mcp/clients", get(mcp_clients))
        .route("/api/mcp/clients/{id}/install", post(mcp_install))
        .route("/api/mcp/clients/{id}/uninstall", post(mcp_uninstall))
        .route("/api/services", get(services))
        .route("/api/services/{id}/start", post(service_start))
        .route("/api/storage", get(storage))
        .route("/api/storage/move", post(storage_move))
        .route("/api/pick-folder", post(pick_folder))
        .route("/api/sync", get(sync_status).put(sync_settings))
        .route("/api/sync/run", post(sync_run))
        .route("/api/about", get(about))
        .route("/api/update/check", post(update_check))
        .route("/api/update/install", post(update_install))
        .route("/api/update/auto", post(update_auto))
        .route("/api/settings", get(settings_get).put(settings_put))
        .route("/api/settings/test", post(settings_test))
        .route("/api/restart", post(restart))
        .route("/api/open", post(open_target))
}

fn check(s: &str) -> Result<(), ApiError> {
    if is_valid_id(s) { Ok(()) } else { Err(ApiError::BadRequest(format!("id tidak valid: {s}"))) }
}

async fn list_subjects(State(st): State<AppState>) -> ApiResult<Value> {
    let subjects = st.rt.store.list_subjects().await?;
    let def = st.resolve_subject(None).await;
    Ok(Json(json!({"subjects": subjects, "default": def})))
}

#[derive(Deserialize)]
struct SubjectIn {
    id: String,
    name: String,
    #[serde(default)]
    birth: Option<String>,
    #[serde(default)]
    gender: Option<memory_core::Gender>,
    #[serde(default)]
    events: Option<std::collections::BTreeMap<String, String>>,
}

async fn upsert_subject(State(st): State<AppState>, Json(b): Json<SubjectIn>) -> ApiResult<Subject> {
    check(&b.id)?;
    let mut s = st.rt.pipeline.ensure_subject(&b.id, Some(&b.name)).await?;
    let parse = |d: &str| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| ApiError::BadRequest(format!("tanggal tidak valid: {d}")));
    if let Some(bd) = b.birth.as_deref().filter(|d| !d.is_empty()) {
        s.anchors.birth = Some(parse(bd)?);
    }
    if let Some(ev) = b.events {
        let mut a = LifeAnchors { birth: s.anchors.birth, phases: s.anchors.phases.clone(), ..Default::default() };
        for (k, v) in ev {
            a.events.insert(k, parse(&v)?);
        }
        s.anchors = a;
    }
    if b.gender.is_some() {
        s.gender = b.gender;
    }
    st.rt.store.put_subject(&s).await?;
    Ok(Json(s))
}

async fn stats(State(st): State<AppState>, Path(s): Path<String>) -> ApiResult<Value> {
    check(&s)?;
    let stats = st.rt.store.stats(&s).await?;
    let subject = st.rt.store.get_subject(&s).await?;
    let pending = st.rt.store.pending_job_count().await?;
    Ok(Json(json!({"subject": subject, "stats": stats, "pending_jobs": pending})))
}

#[derive(Deserialize, Default)]
struct TimelineQ {
    from: Option<String>,
    to: Option<String>,
    phase: Option<String>,
    entity: Option<String>,
    limit: Option<usize>,
}

async fn timeline(State(st): State<AppState>, Path(s): Path<String>, Query(q): Query<TimelineQ>) -> ApiResult<memory_retrieve::Timeline> {
    check(&s)?;
    Ok(Json(st.rt.retriever.timeline(&s, q.entity.as_deref(), q.phase.as_deref(), q.from.as_deref(), q.to.as_deref(), q.limit.unwrap_or(500)).await?))
}

async fn episodes(State(st): State<AppState>, Path(s): Path<String>, Query(p): Query<Page>) -> ApiResult<Vec<Episode>> {
    check(&s)?;
    let status = match p.status.as_deref() {
        Some("retelling") => Some(EpisodeStatus::Retelling),
        Some("quarantined") => Some(EpisodeStatus::Quarantined),
        Some("all") => None,
        _ => Some(EpisodeStatus::Active),
    };
    Ok(Json(
        st.rt
            .store
            .list_episodes(
                &s,
                &EpisodeFilter {
                    status,
                    limit: p.limit.unwrap_or(500).min(2000),
                    offset: p.offset.unwrap_or(0),
                    ..Default::default()
                },
            )
            .await?,
    ))
}

async fn stream_items(State(st): State<AppState>, Path(s): Path<String>, Query(p): Query<Page>) -> ApiResult<Vec<StreamItem>> {
    check(&s)?;
    let only_open = p.status.as_deref() == Some("open");
    Ok(Json(st.rt.store.stream_items(&s, only_open, p.limit.unwrap_or(500).min(2000)).await?))
}

async fn episode(State(st): State<AppState>, Path((s, id)): Path<(String, String)>) -> ApiResult<Value> {
    check(&s)?;
    let e = st.rt.store.get_episode(&id).await?.filter(|e| e.subject == s).ok_or_else(|| ApiError::NotFound("episode tidak ada".into()))?;
    let facts = st.rt.store.facts_by_episode(&id).await?;
    let source = st.rt.store.get_source(&e.source_id).await?;
    let mut entities = Vec::new();
    for eid in &e.entity_ids {
        if let Some(en) = st.rt.store.get_entity(eid).await? {
            entities.push(json!({"id": en.id, "name": en.name, "kind": en.kind}));
        }
    }
    let retellings = st.rt.store.rows_as::<Episode>(
        "SELECT * OMIT embedding FROM episode WHERE retelling_of = $id ORDER BY created_at;",
        memory_store::binds! {"id" => id},
    ).await?;
    Ok(Json(json!({"episode": e, "facts": facts, "source": source, "entities": entities, "retellings": retellings})))
}

#[derive(Deserialize, Default)]
struct Page {
    kind: Option<String>,
    status: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

async fn entities(State(st): State<AppState>, Path(s): Path<String>, Query(p): Query<Page>) -> ApiResult<Vec<Entity>> {
    check(&s)?;
    let kind = p.kind.as_deref().filter(|k| !k.is_empty()).map(EntityKind::parse);
    Ok(Json(st.rt.store.list_entities(&s, kind, p.limit.unwrap_or(200).min(1000), p.offset.unwrap_or(0)).await?))
}

async fn entity(State(st): State<AppState>, Path((s, id)): Path<(String, String)>) -> ApiResult<memory_retrieve::WhoIs> {
    check(&s)?;
    st.rt.retriever.who_is_id(&s, &id).await?.map(Json).ok_or_else(|| ApiError::NotFound("entitas tidak ada".into()))
}

async fn facts(State(st): State<AppState>, Path(s): Path<String>, Query(p): Query<Page>) -> ApiResult<Vec<Value>> {
    check(&s)?;
    let status = match p.status.as_deref() {
        Some("all") => None,
        Some("conflict") => Some(FactStatus::Conflict),
        Some("quarantined") => Some(FactStatus::Quarantined),
        Some("rejected") => Some(FactStatus::Rejected),
        _ => Some(FactStatus::Active),
    };
    let rows = st.rt.store.list_facts(&s, status, p.limit.unwrap_or(500).min(2000)).await?;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(rows.len());
    for f in rows {
        let entity_name = match names.get(&f.entity_id) {
            Some(n) => n.clone(),
            None => {
                let n = st.rt.store.resolve_entity(&f.entity_id).await?.map(|e| e.name).unwrap_or_default();
                names.insert(f.entity_id.clone(), n.clone());
                n
            }
        };
        let mut v = serde_json::to_value(&f).unwrap_or_default();
        v["entity_name"] = json!(entity_name);
        v["statement"] = json!(f.statement(&entity_name));
        v["current"] = json!(f.status == FactStatus::Active && f.valid_at(&today));
        v["time"] = json!(f.time.label());
        out.push(v);
    }
    Ok(Json(out))
}

async fn kin(State(st): State<AppState>, Path(s): Path<String>) -> ApiResult<Vec<Value>> {
    check(&s)?;
    let rows = st.rt.store.kin_for_subject(&s).await?;
    let mut ents: std::collections::HashMap<String, Entity> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(rows.len());
    for k in rows {
        for id in [&k.from, &k.to] {
            if !ents.contains_key(id)
                && let Some(e) = st.rt.store.resolve_entity(id).await?
            {
                ents.insert(id.clone(), e);
            }
        }
        let from_name = ents.get(&k.from).map(|e| e.name.clone()).unwrap_or_else(|| "?".into());
        let to_name = ents.get(&k.to).map(|e| e.name.clone()).unwrap_or_else(|| "?".into());
        let g = ents.get(&k.from).and_then(|e| e.gender).or(k.gender);
        let label = k.relation.label_id(g, k.older).to_string();
        let mut v = serde_json::to_value(&k).unwrap_or_default();
        v["from_name"] = json!(from_name);
        v["to_name"] = json!(to_name);
        v["relation_label"] = json!(label);
        out.push(v);
    }
    Ok(Json(out))
}

async fn family(State(st): State<AppState>, Path(s): Path<String>) -> ApiResult<memory_retrieve::FamilyTree> {
    check(&s)?;
    Ok(Json(st.rt.retriever.family_tree(&s).await?))
}

#[derive(Deserialize, Default)]
struct SituationQ {
    situation: Option<String>,
}

async fn principles(State(st): State<AppState>, Path(s): Path<String>, Query(q): Query<SituationQ>) -> ApiResult<Vec<memory_retrieve::PrincipleView>> {
    check(&s)?;
    Ok(Json(st.rt.retriever.principles(&s, q.situation.as_deref()).await?))
}

async fn dispositions(State(st): State<AppState>, Path(s): Path<String>) -> ApiResult<Vec<Disposition>> {
    check(&s)?;
    Ok(Json(st.rt.store.list_dispositions(&s).await?))
}

async fn phases(State(st): State<AppState>, Path(s): Path<String>) -> ApiResult<Vec<LifePhase>> {
    check(&s)?;
    Ok(Json(st.rt.store.list_life_phases(&s).await?))
}

async fn voice(State(st): State<AppState>, Path(s): Path<String>) -> ApiResult<Option<VoiceProfile>> {
    check(&s)?;
    Ok(Json(st.rt.store.get_voice_profile(&s).await?))
}

async fn recall(State(st): State<AppState>, Path(s): Path<String>, Json(mut q): Json<RecallQuery>) -> ApiResult<memory_retrieve::RecallResult> {
    check(&s)?;
    q.subject = s;
    if q.query.trim().is_empty() {
        return Err(ApiError::BadRequest("query kosong".into()));
    }
    Ok(Json(st.rt.retriever.recall(&q).await?))
}

#[derive(Deserialize)]
struct AskIn {
    question: String,
    #[serde(default)]
    persona: bool,
    #[serde(default)]
    as_of: Option<String>,
}

async fn ask(State(st): State<AppState>, Path(s): Path<String>, Json(b): Json<AskIn>) -> ApiResult<memory_retrieve::Answer> {
    check(&s)?;
    Ok(Json(st.rt.retriever.ask(&s, &b.question, b.persona, b.as_of.as_deref().filter(|d| !d.is_empty())).await?))
}

async fn reviews(State(st): State<AppState>, Path(s): Path<String>, Query(p): Query<Page>) -> ApiResult<Vec<ReviewItem>> {
    check(&s)?;
    let status = match p.status.as_deref() {
        Some("approved") => ReviewStatus::Approved,
        Some("rejected") => ReviewStatus::Rejected,
        _ => ReviewStatus::Open,
    };
    let kind = p.kind.as_deref().and_then(|k| serde_json::from_value(json!(k)).ok());
    Ok(Json(st.rt.store.list_reviews(&s, status, kind, p.limit.unwrap_or(100).min(500)).await?))
}

#[derive(Deserialize)]
struct ReviewIn {
    decision: Decision,
    #[serde(default)]
    note: Option<String>,
}

async fn resolve_review(State(st): State<AppState>, Path(id): Path<String>, Json(b): Json<ReviewIn>) -> ApiResult<ReviewItem> {
    Ok(Json(st.rt.pipeline.resolve_review(&id, b.decision, b.note).await?))
}

async fn sources(State(st): State<AppState>, Path(s): Path<String>, Query(p): Query<Page>) -> ApiResult<Vec<Source>> {
    check(&s)?;
    let status = p.status.as_deref().and_then(|x| serde_json::from_value(json!(x)).ok());
    Ok(Json(st.rt.store.list_sources(&s, status, p.limit.unwrap_or(100).min(1000), p.offset.unwrap_or(0)).await?))
}

async fn source(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let src = st.rt.store.get_source(&id).await?.ok_or_else(|| ApiError::NotFound("sumber tidak ada".into()))?;
    let text = st.rt.vault.read_rel(&src.path).unwrap_or_default();
    let episodes = st.rt.store.list_episodes(&src.subject, &EpisodeFilter { source_id: Some(id.clone()), limit: 1000, ..Default::default() }).await?;
    let used_quotes: Vec<String> = {
        let mut q = Vec::new();
        for e in &episodes {
            for f in st.rt.store.facts_by_episode(&e.id).await? {
                q.extend(f.evidence.into_iter().filter(|ev| ev.episode_id == e.id).map(|ev| ev.quote));
            }
        }
        q
    };
    Ok(Json(json!({"source": src, "text": text, "episodes": episodes, "quotes": used_quotes})))
}

async fn import(State(st): State<AppState>, Path(s): Path<String>, mut mp: Multipart) -> ApiResult<memory_import::ImportSummary> {
    check(&s)?;
    let upload = st.rt.vault.exports_dir().join(".uploads").join(memory_core::new_id());
    std::fs::create_dir_all(&upload)?;
    let mut kind: Option<SourceKind> = None;
    let mut label = String::from("upload");
    let mut paths: Vec<PathBuf> = Vec::new();
    while let Some(field) = mp.next_field().await.map_err(|e| ApiError::BadRequest(e.to_string()))? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "kind" => kind = SourceKind::parse(&field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "label" => label = field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?,
            _ => {
                let fname = field.file_name().map(|f| {
                    std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                });
                let Some(fname) = fname.filter(|f| !f.is_empty() && !f.starts_with('.')) else { continue };
                let bytes = field.bytes().await.map_err(|e| ApiError::BadRequest(e.to_string()))?;
                let p = upload.join(&fname);
                std::fs::write(&p, &bytes)?;
                paths.push(p);
            }
        }
    }
    if paths.is_empty() {
        return Err(ApiError::BadRequest("tidak ada berkas".into()));
    }
    // Import seluruh folder upload sekaligus agar foto + caption berpasangan.
    Ok(Json(st.importer.import_paths(&s, &[upload], &label, kind).await?))
}

async fn batches(State(st): State<AppState>, Path(s): Path<String>) -> ApiResult<Vec<ImportBatch>> {
    check(&s)?;
    Ok(Json(st.rt.store.list_batches(&s, 50).await?))
}

async fn batch(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let b = st.rt.store.get_batch(&id).await?.ok_or_else(|| ApiError::NotFound("batch tidak ada".into()))?;
    let sources = st.rt.store.sources_for_batch(&id).await?;
    let jobs = st.rt.store.jobs_for_batch(&id).await?;
    Ok(Json(json!({"batch": b, "sources": sources, "jobs": jobs})))
}

async fn jobs(State(st): State<AppState>, Query(p): Query<Page>) -> ApiResult<Value> {
    let status = p.status.as_deref().and_then(|x| serde_json::from_value(json!(x)).ok());
    let jobs = st.rt.store.list_jobs(status, p.limit.unwrap_or(100).min(500)).await?;
    Ok(Json(json!({"jobs": jobs, "summary": st.rt.job_summary().await?})))
}

#[derive(Deserialize)]
struct RememberIn {
    text: String,
    #[serde(default)]
    source: Option<String>,
}

async fn remember(State(st): State<AppState>, Path(s): Path<String>, Json(b): Json<RememberIn>) -> ApiResult<Value> {
    check(&s)?;
    let out = remember_text(&st, &s, &b.text, b.source.as_deref()).await?;
    Ok(Json(out))
}

pub(crate) async fn remember_text(st: &AppState, subject: &str, text: &str, source: Option<&str>) -> Result<Value, ApiError> {
    if text.trim().is_empty() {
        return Err(ApiError::BadRequest("teks kosong".into()));
    }
    let out = st
        .rt
        .pipeline
        .ingest(IngestDoc {
            subject: subject.to_string(),
            kind: SourceKind::AgentConversation,
            title: format!("Diingat agent: {}", source.unwrap_or("percakapan")),
            text: text.to_string(),
            occurred_from: Some(chrono::Utc::now().format("%Y-%m-%d").to_string()),
            meta: json!({"via": "remember", "source": source}),
            ..Default::default()
        })
        .await?;
    Ok(json!({"source_id": out.source.id, "duplicate": out.duplicate, "job_id": out.job_id,
              "note": "Disimpan sebagai sumber; pipeline akan menyaring basa-basi sebelum menjadi episode."}))
}

async fn interview_next(State(st): State<AppState>, Path(s): Path<String>) -> ApiResult<memory_extract::interview::InterviewQuestion> {
    check(&s)?;
    Ok(Json(st.rt.pipeline.next_question(&s).await?))
}

async fn retry_source(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let job = st.rt.retry_source(&id).await?;
    Ok(Json(json!({"job_id": job})))
}

async fn retry_failed(State(st): State<AppState>) -> ApiResult<Value> {
    Ok(Json(json!(st.rt.retry_failed(false).await?)))
}

#[derive(Deserialize)]
struct AnswerIn {
    question: String,
    answer: String,
}

async fn interview_answer(State(st): State<AppState>, Path(s): Path<String>, Json(b): Json<AnswerIn>) -> ApiResult<Value> {
    check(&s)?;
    if b.answer.trim().is_empty() {
        return Err(ApiError::BadRequest("jawaban kosong".into()));
    }
    let out = st.rt.pipeline.answer_question(&s, &b.question, &b.answer).await?;
    Ok(Json(json!({"source_id": out.source.id, "job_id": out.job_id, "duplicate": out.duplicate})))
}

async fn maintenance(State(st): State<AppState>, Path((s, kind)): Path<(String, String)>) -> ApiResult<Value> {
    check(&s)?;
    let kind = match kind.as_str() {
        "consolidate" => memory_jobs::CONSOLIDATE,
        "distill" => memory_jobs::DISTILL,
        "voice" => memory_jobs::VOICE,
        "compact" => memory_jobs::COMPACT,
        other => return Err(ApiError::BadRequest(format!("jenis tidak dikenal: {other}"))),
    };
    let job = st.rt.enqueue(kind, json!({"subject": s}), format!("{kind}:{s}:manual:{}", memory_core::new_id())).await?;
    Ok(Json(json!({"job_id": job.id})))
}

async fn compact_candidates(State(st): State<AppState>, Path(s): Path<String>) -> ApiResult<memory_extract::compact::CompactCandidates> {
    check(&s)?;
    Ok(Json(st.rt.pipeline.compact_candidates(&s).await?))
}

async fn compact_apply(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Json(plan): Json<memory_extract::compact::CompactPlan>,
) -> ApiResult<memory_extract::compact::CompactReport> {
    check(&s)?;
    Ok(Json(st.rt.pipeline.compact_apply(&s, plan).await?))
}

// ---------------------------------------------------------------- pasang MCP ke agent AI

/// Alamat MCP & folder pengguna; error bila fitur tidak aktif di server ini.
fn mcp_target(st: &AppState) -> Result<(String, crate::install::Dirs), ApiError> {
    let url = st.mcp_url.clone().ok_or_else(|| ApiError::BadRequest("pemasangan otomatis hanya tersedia di aplikasi desktop atau server lokal".into()))?;
    let dirs = crate::install::Dirs::from_env().ok_or_else(|| ApiError::Internal("folder home pengguna tidak diketahui".into()))?;
    Ok((url, dirs))
}

fn mcp_client(id: &str) -> Result<crate::install::Client, ApiError> {
    crate::install::Client::parse(id).ok_or_else(|| ApiError::NotFound(format!("client tidak dikenal: {id}")))
}

async fn mcp_clients(State(st): State<AppState>) -> ApiResult<Value> {
    let Ok((url, dirs)) = mcp_target(&st) else {
        return Ok(Json(json!({"enabled": false, "clients": []})));
    };
    let token = st.token.as_deref().map(String::as_str);
    let clients: Vec<_> = crate::install::Client::ALL.into_iter().map(|c| crate::install::status(&dirs, c, &url, token)).collect();
    Ok(Json(json!({"enabled": true, "url": url, "name": crate::install::SERVER_NAME, "clients": clients})))
}

async fn mcp_install(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let c = mcp_client(&id)?;
    let (url, dirs) = mcp_target(&st)?;
    let token = st.token.as_deref().map(String::as_str);
    let msg = crate::install::install(&dirs, c, &url, token).await.map_err(ApiError::BadRequest)?;
    Ok(Json(json!({"ok": true, "message": msg, "status": crate::install::status(&dirs, c, &url, token)})))
}

async fn mcp_uninstall(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let c = mcp_client(&id)?;
    let (url, dirs) = mcp_target(&st)?;
    let token = st.token.as_deref().map(String::as_str);
    let msg = crate::install::uninstall(&dirs, c).await.map_err(ApiError::BadRequest)?;
    Ok(Json(json!({"ok": true, "message": msg, "status": crate::install::status(&dirs, c, &url, token)})))
}

async fn services(State(st): State<AppState>) -> ApiResult<Value> {
    let Some(c) = &st.services else {
        return Ok(Json(json!({"enabled": false, "services": []})));
    };
    Ok(Json(json!({"enabled": true, "services": service_list(&st, c.as_ref()).await})))
}

/// Layanan internal + status server embedding.
async fn service_list(st: &AppState, c: &dyn crate::ServiceControl) -> Value {
    let mut list = serde_json::to_value(c.list()).unwrap_or_else(|_| json!([]));
    if let Some(arr) = list.as_array_mut() {
        // Dicek paralel: tiap server bisa butuh beberapa detik untuk timeout.
        let (emb, llm) = tokio::join!(embedding_status(st), llm_status(st));
        arr.push(emb);
        arr.extend(llm);
    }
    list
}

/// Hasil cek layanan luar sebagai entri daftar layanan.
fn external_service(id: &str, name: &str, description: String, error: Option<String>) -> Value {
    json!({"id": id, "name": name, "description": description, "running": error.is_none(), "external": true, "error": error})
}

/// Server LLM (vLLM dkk.) di luar engram; model ringan bisa di server terpisah.
async fn llm_status(st: &AppState) -> Vec<Value> {
    use memory_llm::ModelRole;
    let cfg = &st.rt.cfg.llm;
    let mut out = Vec::new();
    let light_separate = cfg.light_base_url.as_deref().is_some_and(|u| u.trim_end_matches('/') != cfg.base_url.trim_end_matches('/'));
    let mut targets = vec![("llm", "Server LLM", ModelRole::Extract, cfg.base_url.clone(), if light_separate { cfg.extract_model.clone() } else { format!("{}, {}", cfg.extract_model, cfg.light_model) })];
    if light_separate {
        targets.push(("llm-light", "Server LLM ringan", ModelRole::Light, cfg.light_base_url.clone().unwrap_or_default(), cfg.light_model.clone()));
    }
    let probe = |role| async move {
        match tokio::time::timeout(std::time::Duration::from_secs(3), st.rt.pipeline.llm.health(role)).await {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e.to_string()),
            Err(_) => Some("tidak menjawab dalam 3 detik".to_string()),
        }
    };
    let (main_err, light_err) = tokio::join!(probe(ModelRole::Extract), async {
        if light_separate { probe(ModelRole::Light).await } else { None }
    });
    for ((id, name, _, url, models), error) in targets.into_iter().zip([main_err, light_err]) {
        let desc = format!("Server di luar engram ({url} · {models}). Dibutuhkan untuk ekstraksi memori dan jawaban; nyalakan di mesinnya bila berhenti.");
        out.push(external_service(id, name, desc, error));
    }
    out
}

/// Server embedding berjalan di luar engram (mis. TEI di mesin GPU): cek dengan
/// satu permintaan embedding sungguhan. Tidak bisa dijalankan dari sini.
async fn embedding_status(st: &AppState) -> Value {
    let cfg = &st.rt.cfg.embedding;
    let target = if cfg.provider == "hash" { "hash (tanpa server)".to_string() } else { format!("{} · {}", cfg.base_url, cfg.model) };
    let probe = tokio::time::timeout(std::time::Duration::from_secs(3), st.rt.pipeline.embedder.embed_one("ping")).await;
    let error = match probe {
        Ok(Ok(_)) => None,
        Ok(Err(e)) => Some(e.to_string()),
        Err(_) => Some("tidak menjawab dalam 3 detik".to_string()),
    };
    external_service("embedding", "Server embedding", format!("Server di luar engram ({target}). Dibutuhkan untuk memproses memori baru; nyalakan di mesinnya bila berhenti."), error)
}

async fn service_start(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let c = st.services.as_ref().ok_or_else(|| ApiError::BadRequest("pengendali layanan tidak tersedia".into()))?;
    c.start(&id).map_err(ApiError::BadRequest)?;
    Ok(Json(json!({"ok": true, "services": service_list(&st, c.as_ref()).await})))
}

// ---------------------------------------------------------------- lokasi data & sinkronisasi

async fn storage(State(st): State<AppState>) -> ApiResult<Value> {
    let root = st.rt.vault.root();
    Ok(Json(json!({
        "vault": root,
        "can_change": st.services.as_ref().is_some_and(|c| c.can_manage_storage()),
        "in_cloud_folder": memory_jobs::storage::looks_like_cloud_folder(root),
    })))
}

#[derive(Deserialize)]
struct PathIn {
    path: String,
}

async fn storage_move(State(st): State<AppState>, Json(b): Json<PathIn>) -> ApiResult<Value> {
    let c = st.services.clone().filter(|c| c.can_manage_storage()).ok_or_else(|| ApiError::BadRequest("pemindahan lokasi data hanya tersedia di aplikasi desktop; pakai `engram vault move`".into()))?;
    let to = PathBuf::from(b.path.trim());
    let plan = memory_jobs::storage::plan_move(st.rt.vault.root(), &to).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let msg = c.move_vault(&plan.target).map_err(ApiError::BadRequest)?;
    Ok(Json(json!({"ok": true, "target": plan.target, "use_existing": plan.use_existing, "message": msg})))
}

#[derive(Deserialize)]
struct PickIn {
    #[serde(default)]
    title: Option<String>,
}

async fn pick_folder(State(st): State<AppState>, Json(b): Json<PickIn>) -> ApiResult<Value> {
    let c = st.services.clone().filter(|c| c.can_manage_storage()).ok_or_else(|| ApiError::BadRequest("dialog folder hanya tersedia di aplikasi desktop; ketik path folder".into()))?;
    let title = b.title.unwrap_or_else(|| "Pilih folder".into());
    let path = tokio::task::spawn_blocking(move || c.pick_folder(&title)).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(json!({"path": path})))
}

async fn sync_status(State(st): State<AppState>) -> ApiResult<memory_jobs::sync::SyncStatus> {
    let rt = st.rt.clone();
    // Membaca folder Drive bisa lambat (berkas online-only); jangan blok runtime.
    Ok(Json(tokio::task::spawn_blocking(move || memory_jobs::sync::status(&rt)).await.map_err(|e| ApiError::Internal(e.to_string()))?))
}

#[derive(Deserialize)]
struct SyncIn {
    /// Folder sync; kosong/null = sinkronisasi dimatikan.
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    interval_minutes: Option<u64>,
    #[serde(default)]
    device_name: Option<String>,
    /// Kata sandi baru; tidak dikirim = tidak berubah, string kosong = hapus.
    #[serde(default)]
    passphrase: Option<String>,
}

async fn sync_settings(State(st): State<AppState>, Json(b): Json<SyncIn>) -> ApiResult<memory_jobs::sync::SyncStatus> {
    use memory_jobs::sync;
    let mut s = sync::load_settings(&st.rt.vault);
    s.dir = b.dir.map(|d| d.trim().to_string()).filter(|d| !d.is_empty()).map(PathBuf::from);
    if let Some(m) = b.interval_minutes {
        s.interval_minutes = m;
    }
    if b.device_name.is_some() {
        s.device_name = b.device_name;
    }
    if let Some(p) = &b.passphrase {
        sync::set_passphrase(&st.rt.vault, Some(p.as_str())).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    }
    sync::save_settings(&st.rt.vault, s).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    sync_status(State(st)).await
}

async fn sync_run(State(st): State<AppState>) -> ApiResult<memory_jobs::sync::SyncReport> {
    memory_jobs::sync::sync_now(&st.rt).await.map(Json).map_err(|e| ApiError::BadRequest(e.to_string()))
}

// ---------------------------------------------------------------- pengaturan, tentang, update

fn blocking_err(e: tokio::task::JoinError) -> ApiError {
    ApiError::Internal(e.to_string())
}

async fn about(State(st): State<AppState>) -> ApiResult<Value> {
    let c = st.services.as_ref();
    Ok(Json(json!({
        "name": "engram",
        "version": crate::settings::VERSION,
        "license": "AGPL-3.0-or-later",
        "repository": crate::settings::REPO_URL,
        "host": if c.is_some() { "desktop" } else { "server" },
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "vault": st.rt.vault.root(),
        "config_path": st.config_path,
        "auto_update": c.and_then(|c| c.auto_update()),
        "can_open": c.is_some(),
        "can_restart": c.is_some(),
    })))
}

async fn update_check(State(st): State<AppState>) -> ApiResult<crate::settings::UpdateInfo> {
    if let Some(c) = st.services.clone() {
        let res = tokio::task::spawn_blocking(move || c.check_update()).await.map_err(blocking_err)?;
        if let Some(r) = res {
            return r.map(Json).map_err(ApiError::BadRequest);
        }
    }
    crate::settings::check_github().await.map(Json).map_err(ApiError::BadRequest)
}

async fn update_install(State(st): State<AppState>) -> ApiResult<Value> {
    let c = st.services.as_ref().ok_or_else(|| ApiError::BadRequest("pasang update hanya tersedia di aplikasi desktop; unduh rilis baru dari GitHub".into()))?;
    let msg = c.install_update().map_err(ApiError::BadRequest)?;
    Ok(Json(json!({"ok": true, "message": msg})))
}

#[derive(Deserialize)]
struct OnIn {
    enabled: bool,
}

async fn update_auto(State(st): State<AppState>, Json(b): Json<OnIn>) -> ApiResult<Value> {
    let c = st.services.as_ref().ok_or_else(|| ApiError::BadRequest("pengaturan ini hanya ada di aplikasi desktop".into()))?;
    c.set_auto_update(b.enabled).map_err(ApiError::BadRequest)?;
    Ok(Json(json!({"ok": true, "auto_update": c.auto_update()})))
}

/// Nilai pengaturan dari memory.toml, dan apakah berbeda dari yang sedang dipakai.
fn settings_view(st: &AppState) -> Result<Value, ApiError> {
    use crate::settings::{editable_values, preview};
    let file = preview(st.config_path.as_deref(), &st.rt.cfg, None).map_err(ApiError::BadRequest)?;
    let (saved, running) = (editable_values(&file), editable_values(&st.rt.cfg));
    // Rahasia hanya terlihat sebagai *_set; bandingkan nilai aslinya juga.
    let secret_changed = file.llm.api_key != st.rt.cfg.llm.api_key || file.embedding.api_key != st.rt.cfg.embedding.api_key;
    Ok(json!({
        "editable": st.config_path.is_some(),
        "config_path": st.config_path,
        "values": saved,
        "restart_required": saved != running || secret_changed,
    }))
}

async fn settings_get(State(st): State<AppState>) -> ApiResult<Value> {
    settings_view(&st).map(Json)
}

#[derive(Deserialize)]
struct SettingsIn {
    values: Value,
}

async fn settings_put(State(st): State<AppState>, Json(b): Json<SettingsIn>) -> ApiResult<Value> {
    let path = st.config_path.clone().ok_or_else(|| ApiError::BadRequest("lokasi memory.toml tidak diketahui; ubah berkasnya langsung".into()))?;
    tokio::task::spawn_blocking(move || crate::settings::save(&path, &b.values)).await.map_err(blocking_err)?.map_err(ApiError::BadRequest)?;
    settings_view(&st).map(Json)
}

#[derive(Deserialize)]
struct TestIn {
    kind: String,
    /// Perubahan yang belum disimpan, format sama dengan PUT /api/settings.
    #[serde(default)]
    values: Option<Value>,
}

async fn settings_test(State(st): State<AppState>, Json(b): Json<TestIn>) -> ApiResult<Value> {
    use crate::settings;
    let cfg = settings::preview(st.config_path.as_deref(), &st.rt.cfg, b.values.as_ref()).map_err(ApiError::BadRequest)?;
    Ok(Json(match b.kind.as_str() {
        "llm" => settings::test_llm(&cfg).await,
        "embedding" => settings::test_embedding(&cfg).await,
        "transcription" => settings::test_transcription(&cfg).await,
        k => return Err(ApiError::BadRequest(format!("jenis tes tidak dikenal: {k}"))),
    }))
}

async fn restart(State(st): State<AppState>) -> ApiResult<Value> {
    let c = st.services.as_ref().ok_or_else(|| ApiError::BadRequest("jalankan ulang `engram serve` agar pengaturan baru berlaku".into()))?;
    let msg = c.restart().map_err(ApiError::BadRequest)?;
    Ok(Json(json!({"ok": true, "message": msg})))
}

#[derive(Deserialize)]
struct OpenIn {
    target: String,
}

/// Hanya target yang dikenal; UI tidak bisa membuka path atau URL sembarang.
async fn open_target(State(st): State<AppState>, Json(b): Json<OpenIn>) -> ApiResult<Value> {
    let c = st.services.as_ref().ok_or_else(|| ApiError::BadRequest("hanya tersedia di aplikasi desktop".into()))?;
    let repo = crate::settings::REPO_URL;
    let vault = st.rt.vault.root();
    let target = match b.target.as_str() {
        "vault" => vault.display().to_string(),
        "inbox" => {
            let dir = vault.join("inbox");
            std::fs::create_dir_all(&dir)?;
            dir.display().to_string()
        }
        "config" => st.config_path.as_ref().and_then(|p| p.parent()).ok_or_else(|| ApiError::BadRequest("lokasi memory.toml tidak diketahui".into()))?.display().to_string(),
        "repo" => repo.to_string(),
        "releases" => format!("{repo}/releases"),
        "issues" => format!("{repo}/issues"),
        t => return Err(ApiError::BadRequest(format!("target tidak dikenal: {t}"))),
    };
    c.open(&target).map_err(ApiError::BadRequest)?;
    Ok(Json(json!({"ok": true})))
}

// ---------------------------------------------------------------- folder penting (watched folders via agy CLI)

async fn watched_folders_list(State(st): State<AppState>, Path(s): Path<String>) -> ApiResult<memory_jobs::watch::WatchStatus> {
    check(&s)?;
    let rt = st.rt.clone();
    Ok(Json(tokio::task::spawn_blocking(move || memory_jobs::watch::status(&rt, Some(&s))).await.map_err(blocking_err)?))
}

#[derive(Deserialize)]
struct WatchedFolderIn {
    path: String,
    #[serde(default)]
    label: Option<String>,
    /// Langsung pindai & ambil konteks lewat agy CLI sekarang.
    #[serde(default)]
    scan: Option<bool>,
}

async fn watched_folders_add(State(st): State<AppState>, Path(s): Path<String>, Json(b): Json<WatchedFolderIn>) -> ApiResult<Value> {
    check(&s)?;
    let raw_path = b.path.trim().to_string();
    if raw_path.is_empty() {
        return Err(ApiError::BadRequest("path folder kosong".into()));
    }
    st.rt.pipeline.ensure_subject(&s, None).await?;
    let folder = memory_jobs::watch::add_folder(&st.rt.vault, &s, std::path::Path::new(&raw_path), b.label.as_deref())
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let scan = if b.scan.unwrap_or(false) {
        Some(memory_jobs::watch::scan_one(&st.rt, &folder.id, false, 0).await.map_err(|e| ApiError::BadRequest(e.to_string()))?)
    } else {
        None
    };
    let status = memory_jobs::watch::status(&st.rt, Some(&s));
    Ok(Json(json!({"folder": folder, "scan": scan, "status": status})))
}

#[derive(Deserialize, Default)]
struct ScanIn {
    #[serde(default)]
    force: bool,
}

async fn watched_folder_scan(
    State(st): State<AppState>,
    Path(id): Path<String>,
     b: Option<Json<ScanIn>>,
) -> ApiResult<memory_jobs::watch::FolderScanOutcome> {
    let force = b.map(|x| x.force).unwrap_or(false);
    memory_jobs::watch::scan_one(&st.rt, &id, force, 0)
        .await
        .map(Json)
        .map_err(|e| ApiError::BadRequest(e.to_string()))
}

async fn watched_folders_scan_all(
    State(st): State<AppState>,
    Path(s): Path<String>,
    b: Option<Json<ScanIn>>,
) -> ApiResult<Vec<memory_jobs::watch::FolderScanOutcome>> {
    check(&s)?;
    let force = b.map(|x| x.force).unwrap_or(false);
    memory_jobs::watch::scan_all(&st.rt, Some(&s), force, 0)
        .await
        .map(Json)
        .map_err(|e| ApiError::BadRequest(e.to_string()))
}

async fn watched_folder_remove(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let removed = memory_jobs::watch::remove_folder(&st.rt.vault, &id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if !removed {
        return Err(ApiError::NotFound("folder penting tidak ditemukan".into()));
    }
    Ok(Json(json!({"ok": true})))
}
