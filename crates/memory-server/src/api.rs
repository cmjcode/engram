//! HTTP API JSON untuk UI web dan integrasi. Semua route di bawah `/api`.

use std::path::PathBuf;

use axum::Json;
use axum::Router;
use axum::extract::{Multipart, Path, Query, State};
use axum::routing::{get, post};
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
        .route("/api/subjects/{s}/episodes/by-ids", post(episodes_by_ids))
        .route("/api/subjects/{s}/lessons", get(lessons))
        .route("/api/subjects/{s}/context/inject", get(context_inject))
        .route("/api/subjects/{s}/sessions", get(sessions))
        .route("/api/subjects/{s}/projects", get(projects_overview))
        .route("/api/subjects/{s}/projects/files", get(project_files))
        .route("/api/subjects/{s}/projects/graph", get(projects_graph))
        .route("/api/subjects/{s}/projects/meta", get(project_meta_get).put(project_meta_put))
        .route("/api/subjects/{s}/projects/share", get(project_share_get).post(project_share_post))
        .route("/api/sessions/{id}", get(session_detail))
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
        .route(
            "/api/subjects/{s}/project-context",
            get(project_context).post(project_context_post),
        )
        .route("/api/subjects/{s}/ask", post(ask))
        .route("/api/subjects/{s}/reviews", get(reviews))
        .route("/api/subjects/{s}/reviews/auto", post(reviews_auto))
        .route("/api/reviews/{id}", post(resolve_review))
        .route("/api/subjects/{s}/sources", get(sources))
        .route("/api/sources/{id}", get(source))
        .route("/api/sources/{id}/retry", post(retry_source))
        .route("/api/jobs/retry-failed", post(retry_failed))
        .route("/api/subjects/{s}/import", post(import))
        .route(
            "/api/subjects/{s}/watched-folders",
            get(watched_folders_list).post(watched_folders_add),
        )
        .route(
            "/api/subjects/{s}/watched-folders/scan",
            post(watched_folders_scan_all),
        )
        .route("/api/watched-folders/{id}/scan", post(watched_folder_scan))
        .route(
            "/api/watched-folders/{id}/remove",
            post(watched_folder_remove),
        )
        .route("/api/subjects/{s}/batches", get(batches))
        .route("/api/batches/{id}", get(batch))
        .route("/api/jobs", get(jobs))
        .route("/api/subjects/{s}/remember", post(remember))
        .route("/api/subjects/{s}/interview/next", get(interview_next))
        .route("/api/subjects/{s}/interview/answer", post(interview_answer))
        .route("/api/subjects/{s}/maintenance/{kind}", post(maintenance))
        .route(
            "/api/subjects/{s}/compact/candidates",
            get(compact_candidates),
        )
        .route("/api/subjects/{s}/compact/apply", post(compact_apply))
        .route("/api/mcp/clients", get(mcp_clients))
        .route("/api/mcp/clients/{id}/install", post(mcp_install))
        .route("/api/mcp/clients/{id}/uninstall", post(mcp_uninstall))
        .route("/api/services", get(services))
        .route("/api/services/start-all", post(services_start_all))
        .route("/api/services/{id}/start", post(service_start))
        .route("/api/storage", get(storage))
        .route("/api/storage/move", post(storage_move))
        .route("/api/pick-folder", post(pick_folder))
        .route("/api/sync", get(sync_status).put(sync_settings))
        .route("/api/sync/run", post(sync_run))
        .route("/api/about", get(about))
        .route("/api/update/check", post(update_check))
        .route("/api/update/install", post(update_install))
        .route("/api/update/progress", get(update_progress))
        .route("/api/update/auto", post(update_auto))
        .route("/api/settings", get(settings_get).put(settings_put))
        .route("/api/settings/test", post(settings_test))
        .route("/api/restart", post(restart))
        .route("/api/open", post(open_target))
}

fn check(s: &str) -> Result<(), ApiError> {
    if is_valid_id(s) {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!("id tidak valid: {s}")))
    }
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

async fn upsert_subject(
    State(st): State<AppState>,
    Json(b): Json<SubjectIn>,
) -> ApiResult<Subject> {
    check(&b.id)?;
    let mut s = st.rt.pipeline.ensure_subject(&b.id, Some(&b.name)).await?;
    let parse = |d: &str| {
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|_| ApiError::BadRequest(format!("tanggal tidak valid: {d}")))
    };
    if let Some(bd) = b.birth.as_deref().filter(|d| !d.is_empty()) {
        s.anchors.birth = Some(parse(bd)?);
    }
    if let Some(ev) = b.events {
        let mut a = LifeAnchors {
            birth: s.anchors.birth,
            phases: s.anchors.phases.clone(),
            ..Default::default()
        };
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
    let _ = st.rt.pipeline.sanitize_technical_entities(&s).await;
    let stats = st.rt.store.stats(&s).await?;
    let subject = st.rt.store.get_subject(&s).await?;
    let pending = st.rt.store.pending_job_count().await?;
    Ok(Json(
        json!({"subject": subject, "stats": stats, "pending_jobs": pending}),
    ))
}

#[derive(Deserialize, Default)]
struct TimelineQ {
    from: Option<String>,
    to: Option<String>,
    phase: Option<String>,
    entity: Option<String>,
    limit: Option<usize>,
}

async fn timeline(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Query(q): Query<TimelineQ>,
) -> ApiResult<memory_retrieve::Timeline> {
    check(&s)?;
    Ok(Json(
        st.rt
            .retriever
            .timeline(
                &s,
                q.entity.as_deref(),
                q.phase.as_deref(),
                q.from.as_deref(),
                q.to.as_deref(),
                q.limit.unwrap_or(500),
            )
            .await?,
    ))
}

async fn episodes(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Query(p): Query<Page>,
) -> ApiResult<Vec<Episode>> {
    check(&s)?;
    let status = match p.status.as_deref() {
        Some("retelling") => Some(EpisodeStatus::Retelling),
        Some("quarantined") => Some(EpisodeStatus::Quarantined),
        Some("all") => None,
        _ => Some(EpisodeStatus::Active),
    };
    let projects = match p.project.as_deref().filter(|x| !x.trim().is_empty()) {
        Some(pr) => st.rt.store.project_names(&s, pr).await?,
        None => vec![],
    };
    Ok(Json(
        st.rt
            .store
            .list_episodes(
                &s,
                &EpisodeFilter {
                    status,
                    projects,
                    obs_type: p.obs_type.clone().filter(|x| !x.is_empty()),
                    session_id: p.session_id.clone().filter(|x| !x.is_empty()),
                    newest_first: p.newest_first.unwrap_or(false),
                    limit: p.limit.unwrap_or(500).min(2000),
                    offset: p.offset.unwrap_or(0),
                    ..Default::default()
                },
            )
            .await?,
    ))
}

async fn stream_items(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Query(p): Query<Page>,
) -> ApiResult<Vec<StreamItem>> {
    check(&s)?;
    let only_open = p.status.as_deref() == Some("open");
    Ok(Json(
        st.rt
            .store
            .stream_items(&s, only_open, p.limit.unwrap_or(500).min(2000))
            .await?,
    ))
}

async fn episode(
    State(st): State<AppState>,
    Path((s, id)): Path<(String, String)>,
) -> ApiResult<Value> {
    check(&s)?;
    let e = st
        .rt
        .store
        .get_episode(&id)
        .await?
        .filter(|e| e.subject == s)
        .ok_or_else(|| ApiError::NotFound("episode tidak ada".into()))?;
    let facts = st.rt.store.facts_by_episode(&id).await?;
    let source = st.rt.store.get_source(&e.source_id).await?;
    let mut entities = Vec::new();
    for eid in &e.entity_ids {
        if let Some(en) = st.rt.store.get_entity(eid).await? {
            entities.push(json!({"id": en.id, "name": en.name, "kind": en.kind}));
        }
    }
    let retellings = st
        .rt
        .store
        .rows_as::<Episode>(
            "SELECT * OMIT embedding FROM episode WHERE retelling_of = $id ORDER BY created_at;",
            memory_store::binds! {"id" => id},
        )
        .await?;
    Ok(Json(
        json!({"episode": e, "facts": facts, "source": source, "entities": entities, "retellings": retellings}),
    ))
}

#[derive(Deserialize, Default)]
struct Page {
    kind: Option<String>,
    status: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    obs_type: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    newest_first: Option<bool>,
}

async fn entities(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Query(p): Query<Page>,
) -> ApiResult<Vec<Entity>> {
    check(&s)?;
    let _ = st.rt.pipeline.sanitize_technical_entities(&s).await;
    let kind = p
        .kind
        .as_deref()
        .filter(|k| !k.is_empty())
        .map(EntityKind::parse);
    Ok(Json(
        st.rt
            .store
            .list_entities(
                &s,
                kind,
                p.limit.unwrap_or(200).min(1000),
                p.offset.unwrap_or(0),
            )
            .await?,
    ))
}

async fn entity(
    State(st): State<AppState>,
    Path((s, id)): Path<(String, String)>,
) -> ApiResult<memory_retrieve::WhoIs> {
    check(&s)?;
    st.rt
        .retriever
        .who_is_id(&s, &id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("entitas tidak ada".into()))
}

async fn facts(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Query(p): Query<Page>,
) -> ApiResult<Vec<Value>> {
    check(&s)?;
    let status = match p.status.as_deref() {
        Some("all") => None,
        Some("conflict") => Some(FactStatus::Conflict),
        Some("quarantined") => Some(FactStatus::Quarantined),
        Some("rejected") => Some(FactStatus::Rejected),
        _ => Some(FactStatus::Active),
    };
    let rows = st
        .rt
        .store
        .list_facts(&s, status, p.limit.unwrap_or(500).min(2000))
        .await?;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(rows.len());
    for f in rows {
        let entity_name = match names.get(&f.entity_id) {
            Some(n) => n.clone(),
            None => {
                let n = st
                    .rt
                    .store
                    .resolve_entity(&f.entity_id)
                    .await?
                    .map(|e| e.name)
                    .unwrap_or_default();
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
        let from_name = ents
            .get(&k.from)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| "?".into());
        let to_name = ents
            .get(&k.to)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| "?".into());
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

async fn family(
    State(st): State<AppState>,
    Path(s): Path<String>,
) -> ApiResult<memory_retrieve::FamilyTree> {
    check(&s)?;
    Ok(Json(st.rt.retriever.family_tree(&s).await?))
}

#[derive(Deserialize, Default)]
struct SituationQ {
    situation: Option<String>,
}

async fn principles(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Query(q): Query<SituationQ>,
) -> ApiResult<Vec<memory_retrieve::PrincipleView>> {
    check(&s)?;
    Ok(Json(
        st.rt
            .retriever
            .principles(&s, q.situation.as_deref())
            .await?,
    ))
}

async fn dispositions(
    State(st): State<AppState>,
    Path(s): Path<String>,
) -> ApiResult<Vec<Disposition>> {
    check(&s)?;
    Ok(Json(st.rt.store.list_dispositions(&s).await?))
}

async fn phases(State(st): State<AppState>, Path(s): Path<String>) -> ApiResult<Vec<LifePhase>> {
    check(&s)?;
    Ok(Json(st.rt.store.list_life_phases(&s).await?))
}

async fn voice(
    State(st): State<AppState>,
    Path(s): Path<String>,
) -> ApiResult<Option<VoiceProfile>> {
    check(&s)?;
    Ok(Json(st.rt.store.get_voice_profile(&s).await?))
}

async fn recall(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Json(mut q): Json<RecallQuery>,
) -> ApiResult<memory_retrieve::RecallResult> {
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

#[derive(Deserialize, Default)]
struct ProjectContextQ {
    project: String,
    #[serde(default)]
    task: Option<String>,
}

async fn project_context(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Query(q): Query<ProjectContextQ>,
) -> ApiResult<memory_retrieve::ProjectContext> {
    check(&s)?;
    if q.project.trim().is_empty() {
        return Err(ApiError::BadRequest("project kosong".into()));
    }
    Ok(Json(
        st.rt
            .retriever
            .project_context(&s, &q.project, q.task.as_deref())
            .await?,
    ))
}

async fn project_context_post(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Json(q): Json<ProjectContextQ>,
) -> ApiResult<memory_retrieve::ProjectContext> {
    check(&s)?;
    if q.project.trim().is_empty() {
        return Err(ApiError::BadRequest("project kosong".into()));
    }
    Ok(Json(
        st.rt
            .retriever
            .project_context(&s, &q.project, q.task.as_deref())
            .await?,
    ))
}

async fn ask(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Json(b): Json<AskIn>,
) -> ApiResult<memory_retrieve::Answer> {
    check(&s)?;
    Ok(Json(
        st.rt
            .retriever
            .ask(
                &s,
                &b.question,
                b.persona,
                b.as_of.as_deref().filter(|d| !d.is_empty()),
            )
            .await?,
    ))
}

async fn reviews(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Query(p): Query<Page>,
) -> ApiResult<Vec<ReviewItem>> {
    check(&s)?;
    let _ = st.rt.pipeline.sanitize_technical_entities(&s).await;
    let status = match p.status.as_deref() {
        Some("approved") => ReviewStatus::Approved,
        Some("rejected") => ReviewStatus::Rejected,
        _ => ReviewStatus::Open,
    };
    let kind = p
        .kind
        .as_deref()
        .and_then(|k| serde_json::from_value(json!(k)).ok());
    let mut list = st
        .rt
        .store
        .list_reviews(&s, status, kind, p.limit.unwrap_or(100).min(500))
        .await?;
    let mut ent_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for r in &mut list {
        match r.kind {
            ReviewKind::Conflict => {
                for (id_key, out_key) in [("existing", "fact_old"), ("new", "fact_new")] {
                    let Some(fid) = r.payload[id_key].as_str().map(str::to_string) else {
                        continue;
                    };
                    if let Ok(Some(f)) = st.rt.store.get_fact(&fid).await {
                        let ename = match ent_names.get(&f.entity_id) {
                            Some(n) => n.clone(),
                            None => {
                                let n = st
                                    .rt
                                    .store
                                    .resolve_entity(&f.entity_id)
                                    .await
                                    .ok()
                                    .flatten()
                                    .map(|e| e.name)
                                    .unwrap_or_default();
                                ent_names.insert(f.entity_id.clone(), n.clone());
                                n
                            }
                        };
                        r.payload[out_key] = json!({
                            "id": f.id,
                            "entity_name": ename,
                            "predicate": f.predicate,
                            "object_text": f.object_text,
                            "category": f.category,
                            "time": f.time.label(),
                            "valid_from": f.valid_from,
                            "valid_to": f.valid_to,
                            "statement": f.statement(&ename),
                            "quote": f.evidence.first().map(|e| e.quote.as_str()).unwrap_or(""),
                        });
                    }
                }
            }
            ReviewKind::EntityMerge => {
                for (id_key, out_key) in [("existing", "fact_old"), ("new", "fact_new")] {
                    let Some(eid) = r.payload[id_key].as_str().map(str::to_string) else {
                        continue;
                    };
                    if let Ok(Some(e)) = st.rt.store.get_entity(&eid).await {
                        let alias = if e.aliases.is_empty() {
                            String::new()
                        } else {
                            format!(" (alias: {})", e.aliases.join(", "))
                        };
                        r.payload[out_key] = json!({
                            "id": e.id,
                            "statement": format!("{} [{}]{}", e.name, e.kind.label_id(), alias),
                            "quote": e.description,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(Json(list))
}

async fn reviews_auto(
    State(st): State<AppState>,
    Path(s): Path<String>,
) -> ApiResult<memory_jobs::watch::AutoReviewReport> {
    check(&s)?;
    Ok(Json(memory_jobs::watch::auto_review(&st.rt, &s).await?))
}

#[derive(Deserialize)]
struct ReviewIn {
    decision: Decision,
    #[serde(default)]
    note: Option<String>,
}

async fn resolve_review(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<ReviewIn>,
) -> ApiResult<ReviewItem> {
    Ok(Json(
        st.rt
            .pipeline
            .resolve_review(&id, b.decision, b.note)
            .await?,
    ))
}

async fn sources(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Query(p): Query<Page>,
) -> ApiResult<Vec<Source>> {
    check(&s)?;
    let status = p
        .status
        .as_deref()
        .and_then(|x| serde_json::from_value(json!(x)).ok());
    Ok(Json(
        st.rt
            .store
            .list_sources(
                &s,
                status,
                p.limit.unwrap_or(100).min(1000),
                p.offset.unwrap_or(0),
            )
            .await?,
    ))
}

async fn source(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let src = st
        .rt
        .store
        .get_source(&id)
        .await?
        .ok_or_else(|| ApiError::NotFound("sumber tidak ada".into()))?;
    let text = st.rt.vault.read_rel(&src.path).unwrap_or_default();
    let episodes = st
        .rt
        .store
        .list_episodes(
            &src.subject,
            &EpisodeFilter {
                source_id: Some(id.clone()),
                limit: 1000,
                ..Default::default()
            },
        )
        .await?;
    let used_quotes: Vec<String> = {
        let mut q = Vec::new();
        for e in &episodes {
            for f in st.rt.store.facts_by_episode(&e.id).await? {
                q.extend(
                    f.evidence
                        .into_iter()
                        .filter(|ev| ev.episode_id == e.id)
                        .map(|ev| ev.quote),
                );
            }
        }
        q
    };
    Ok(Json(
        json!({"source": src, "text": text, "episodes": episodes, "quotes": used_quotes}),
    ))
}

async fn import(
    State(st): State<AppState>,
    Path(s): Path<String>,
    mut mp: Multipart,
) -> ApiResult<memory_import::ImportSummary> {
    check(&s)?;
    let upload = st
        .rt
        .vault
        .exports_dir()
        .join(".uploads")
        .join(memory_core::new_id());
    std::fs::create_dir_all(&upload)?;
    let mut kind: Option<SourceKind> = None;
    let mut label = String::from("upload");
    let mut paths: Vec<PathBuf> = Vec::new();
    while let Some(field) = mp
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "kind" => {
                kind = SourceKind::parse(
                    &field
                        .text()
                        .await
                        .map_err(|e| ApiError::BadRequest(e.to_string()))?,
                )
            }
            "label" => {
                label = field
                    .text()
                    .await
                    .map_err(|e| ApiError::BadRequest(e.to_string()))?
            }
            _ => {
                let fname = field.file_name().map(|f| {
                    std::path::Path::new(f)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                });
                let Some(fname) = fname.filter(|f| !f.is_empty() && !f.starts_with('.')) else {
                    continue;
                };
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
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
    Ok(Json(
        st.importer
            .import_paths(&s, &[upload], &label, kind)
            .await?,
    ))
}

async fn batches(State(st): State<AppState>, Path(s): Path<String>) -> ApiResult<Vec<ImportBatch>> {
    check(&s)?;
    Ok(Json(st.rt.store.list_batches(&s, 50).await?))
}

async fn batch(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let b = st
        .rt
        .store
        .get_batch(&id)
        .await?
        .ok_or_else(|| ApiError::NotFound("batch tidak ada".into()))?;
    let sources = st.rt.store.sources_for_batch(&id).await?;
    let jobs = st.rt.store.jobs_for_batch(&id).await?;
    Ok(Json(json!({"batch": b, "sources": sources, "jobs": jobs})))
}

async fn jobs(State(st): State<AppState>, Query(p): Query<Page>) -> ApiResult<Value> {
    let status = p
        .status
        .as_deref()
        .and_then(|x| serde_json::from_value(json!(x)).ok());
    let jobs = st
        .rt
        .store
        .list_jobs(status, p.limit.unwrap_or(100).min(500))
        .await?;
    Ok(Json(
        json!({"jobs": jobs, "summary": st.rt.job_summary().await?}),
    ))
}

#[derive(Deserialize)]
struct IdsIn {
    ids: Vec<String>,
    #[serde(default)]
    budget: Option<usize>,
}

async fn episodes_by_ids(State(st): State<AppState>, Path(s): Path<String>, Json(b): Json<IdsIn>) -> ApiResult<memory_retrieve::EpisodesDetail> {
    check(&s)?;
    let budget = b.budget.unwrap_or(st.rt.cfg.retrieval.tool_budget_tokens);
    Ok(Json(st.rt.retriever.get_episodes(&s, &b.ids, budget).await?))
}

#[derive(Deserialize, Default)]
struct InjectQ {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    client: Option<String>,
    #[serde(default)]
    max_chars: Option<usize>,
}

async fn context_inject(State(st): State<AppState>, Path(s): Path<String>, Query(q): Query<InjectQ>) -> ApiResult<memory_retrieve::InjectResult> {
    check(&s)?;
    let opts = memory_retrieve::InjectOpts { project: q.project, client: q.client, max_chars: q.max_chars, ..Default::default() };
    Ok(Json(st.rt.retriever.inject_context(&s, &opts).await?))
}

#[derive(Deserialize, Default)]
struct ProjectQ {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn lessons(State(st): State<AppState>, Path(s): Path<String>, Query(q): Query<ProjectQ>) -> ApiResult<Vec<Lesson>> {
    check(&s)?;
    let names = match q.project.as_deref().filter(|p| !p.trim().is_empty()) {
        Some(p) => st.rt.store.project_names(&s, p).await?,
        None => vec![],
    };
    Ok(Json(st.rt.store.list_lessons(&s, &names, None, q.limit.unwrap_or(200).min(1000)).await?))
}

async fn sessions(State(st): State<AppState>, Path(s): Path<String>, Query(q): Query<ProjectQ>) -> ApiResult<Vec<Session>> {
    check(&s)?;
    let names = match q.project.as_deref().filter(|p| !p.trim().is_empty()) {
        Some(p) => st.rt.store.project_names(&s, p).await?,
        None => vec![],
    };
    Ok(Json(st.rt.store.list_sessions(&s, &names, q.limit.unwrap_or(100).min(500)).await?))
}

/// Ringkasan per proyek untuk beranda: aktivitas terakhir, observasi 7 hari, sesi terakhir,
/// langkah berikut dari ringkasan sesi terakhir, jumlah pelajaran.
async fn projects_overview(State(st): State<AppState>, Path(s): Path<String>) -> ApiResult<Value> {
    check(&s)?;
    let eps = st
        .rt
        .store
        .list_episodes(&s, &EpisodeFilter { status: Some(EpisodeStatus::Active), newest_first: true, limit: 3000, ..Default::default() })
        .await?;
    let sessions = st.rt.store.list_sessions(&s, &[], 500).await?;
    let lessons = st.rt.store.list_lessons(&s, &[], Some("active"), 5000).await?;
    let week_ago = (chrono::Utc::now() - chrono::Duration::days(7)).format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let key = |p: &str| memory_core::project::project_key(p);
    let mut map: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
    let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for e in &eps {
        let Some(pr) = e.project.as_deref().filter(|p| !p.trim().is_empty()) else { continue };
        let k = key(pr);
        names.entry(k.clone()).or_insert_with(|| pr.to_string());
        let v = map.entry(k).or_insert_with(|| json!({"episodes": 0, "last_week": 0, "by_type": {}, "last_activity": "", "sessions": 0, "lessons": 0}));
        v["episodes"] = json!(v["episodes"].as_u64().unwrap_or(0) + 1);
        if e.created_at >= week_ago {
            v["last_week"] = json!(v["last_week"].as_u64().unwrap_or(0) + 1);
        }
        if v["last_activity"].as_str().unwrap_or("") < e.created_at.as_str() {
            v["last_activity"] = json!(e.created_at);
        }
        let t = e.obs_type.clone().unwrap_or_else(|| "cerita".into());
        let n = v["by_type"][&t].as_u64().unwrap_or(0) + 1;
        v["by_type"][&t] = json!(n);
        if e.obs_type.as_deref() == Some("session_summary") && v.get("last_summary").is_none() {
            let next = e.text.lines().find_map(|l| l.strip_prefix("Langkah berikut:")).map(str::trim).unwrap_or("").to_string();
            v["last_summary"] = json!({"id": e.id, "title": e.title, "at": e.created_at, "next_steps": next, "summary": e.summary});
        }
        if v.get("latest").is_none() {
            v["latest"] = json!({"id": e.id, "title": e.title, "obs_type": e.obs_type, "at": e.created_at});
        }
    }
    for se in &sessions {
        let Some(pr) = se.project.as_deref() else { continue };
        let k = key(pr);
        names.entry(k.clone()).or_insert_with(|| pr.to_string());
        let v = map.entry(k).or_insert_with(|| json!({"episodes": 0, "last_week": 0, "by_type": {}, "last_activity": "", "sessions": 0, "lessons": 0}));
        v["sessions"] = json!(v["sessions"].as_u64().unwrap_or(0) + 1);
        if v.get("last_session").is_none() {
            v["last_session"] = json!({"id": se.id, "client": se.client, "at": se.last_activity_at, "status": se.status, "first_prompt": se.first_prompt, "tool_uses": se.tool_uses});
        }
        if v["last_activity"].as_str().unwrap_or("") < se.last_activity_at.as_str() {
            v["last_activity"] = json!(se.last_activity_at);
        }
    }
    for l in &lessons {
        let Some(pr) = l.project.as_deref() else { continue };
        let k = key(pr);
        names.entry(k.clone()).or_insert_with(|| pr.to_string());
        let v = map.entry(k).or_insert_with(|| json!({"episodes": 0, "last_week": 0, "by_type": {}, "last_activity": "", "sessions": 0, "lessons": 0}));
        v["lessons"] = json!(v["lessons"].as_u64().unwrap_or(0) + 1);
    }
    // Metadata pengguna (repo, tag, tautan); proyek yang hanya punya metadata tetap tampil.
    let metas = st.rt.store.list_project_meta(&s).await?;
    // Tautan `[[...]]` yang ditulis di episode (memori agent/sesi) untuk backlink.
    let link_rows: Vec<memory_store::EpisodeLinkRow> = eps
        .iter()
        .filter(|e| !e.links.is_empty())
        .map(|e| memory_store::EpisodeLinkRow { project: e.project.clone(), entity_ids: vec![], links: e.links.clone() })
        .collect();
    for m in &metas {
        let k = key(&m.project);
        names.entry(k.clone()).or_insert_with(|| m.project.clone());
        map.entry(k).or_insert_with(|| json!({"episodes": 0, "last_week": 0, "by_type": {}, "last_activity": m.updated_at, "sessions": 0, "lessons": 0}));
    }
    let mut out: Vec<Value> = map
        .into_iter()
        .map(|(k, mut v)| {
            let name = names.get(&k).cloned().unwrap_or(k);
            let meta = metas.iter().find(|m| memory_core::project::project_key(&m.project) == memory_core::project::project_key(&name));
            let repo = meta.and_then(|m| m.repo.clone()).or_else(|| {
                memory_core::project::remote_for_project(&name, sessions.iter().filter(|se| se.project.as_deref().is_some_and(|p| key(p) == key(&name))).filter_map(|se| se.cwd.as_deref()))
            });
            v["tags"] = json!(meta.map(|m| m.tags.clone()).unwrap_or_default());
            v["links"] = json!(meta.map(|m| m.links.clone()).unwrap_or_default());
            v["backlinks"] = json!(memory_store::Store::project_backlinks(&metas, &name));
            v["episode_backlinks"] = json!(memory_store::Store::episode_backlinks(&link_rows, &name));
            v["repo"] = json!(repo);
            v["repo_name"] = json!(repo.as_deref().map(memory_core::project::repo_short_name));
            v["repo_url"] = json!(repo.as_deref().and_then(memory_core::project::repo_web_url));
            v["project"] = json!(name);
            v
        })
        .collect();
    // Keadaan `.engram/` per proyek (akar repo + aktif/tidak) untuk tombol setup di kartu.
    let known = memory_jobs::repo_bundle::known_roots(&st.rt).unwrap_or_default();
    let jobs: Vec<(String, Vec<String>)> = out
        .iter()
        .map(|v| {
            let name = v["project"].as_str().unwrap_or("").to_string();
            let cwds = sessions.iter().filter(|se| se.project.as_deref().is_some_and(|p| key(p) == key(&name))).filter_map(|se| se.cwd.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
            (name, cwds)
        })
        .collect();
    let shares = tokio::task::spawn_blocking(move || jobs.iter().map(|(n, c)| memory_jobs::repo_bundle::quick_share_state(&known, n, c)).collect::<Vec<_>>()).await.unwrap_or_default();
    for (v, sh) in out.iter_mut().zip(shares) {
        v["engram"] = sh.map_or(Value::Null, |(root, state)| json!({"root": root, "state": state}));
    }
    out.sort_by(|a, b| b["last_activity"].as_str().unwrap_or("").cmp(a["last_activity"].as_str().unwrap_or("")));
    let no_project = eps.iter().filter(|e| e.project.is_none()).count();
    Ok(Json(json!({"projects": out, "episodes_without_project": no_project})))
}

#[derive(Deserialize)]
struct GraphQ {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    depth: Option<usize>,
    #[serde(default)]
    tags: Option<bool>,
    #[serde(default)]
    entities: Option<bool>,
    #[serde(default)]
    max_nodes: Option<usize>,
}

/// Graf proyek bergaya Obsidian (global, atau lokal bila `project` diisi): simpul proyek/tag/entitas,
/// sisi `link` (tautan `[[...]]`), `mention`, `tag`, `entity`.
async fn projects_graph(State(st): State<AppState>, Path(s): Path<String>, Query(q): Query<GraphQ>) -> ApiResult<memory_retrieve::ProjectGraph> {
    check(&s)?;
    let project = q.project.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(str::to_string);
    if project.as_deref().is_some_and(|p| p.chars().count() > 200) {
        return Err(ApiError::BadRequest("nama proyek terlalu panjang".into()));
    }
    let opts = memory_retrieve::GraphOpts {
        project,
        depth: q.depth.unwrap_or(1).clamp(1, 3),
        include_tags: q.tags.unwrap_or(true),
        include_entities: q.entities.unwrap_or(true),
        max_nodes: q.max_nodes.unwrap_or(memory_retrieve::GRAPH_MAX_NODES).clamp(10, 2000),
    };
    Ok(Json(st.rt.retriever.project_graph(&s, &opts).await?))
}

#[derive(Deserialize)]
struct ProjectMetaIn {
    project: String,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    tags: Vec<String>,
}

/// Metadata satu proyek beserta backlink, remote git terdeteksi, dan daftar tag yang ada
/// (untuk saran). `meta` bernilai `null` bila belum pernah diisi.
async fn project_meta_get(State(st): State<AppState>, Path(s): Path<String>, Query(q): Query<ProjectQ>) -> ApiResult<Value> {
    check(&s)?;
    let name = q.project.as_deref().map(str::trim).filter(|p| !p.is_empty()).ok_or_else(|| ApiError::BadRequest("project wajib diisi".into()))?;
    let (all, link_rows) = tokio::try_join!(st.rt.store.list_project_meta(&s), st.rt.store.episode_link_rows(&s))?;
    let meta = st.rt.store.get_project_meta(&s, name).await?;
    let names = st.rt.store.project_names(&s, name).await?;
    let sessions = st.rt.store.list_sessions(&s, &names, 50).await?;
    let detected = memory_core::project::remote_for_project(name, sessions.iter().filter_map(|se| se.cwd.as_deref()));
    let repo = meta.as_ref().and_then(|m| m.repo.clone()).or_else(|| detected.clone());
    let mut tags: Vec<String> = Vec::new();
    for t in all.iter().flat_map(|m| m.tags.iter()) {
        if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
            tags.push(t.clone());
        }
    }
    tags.sort_by_key(|t| t.to_lowercase());
    Ok(Json(json!({
        "project": name,
        "meta": meta,
        "backlinks": memory_store::Store::project_backlinks(&all, name),
        "episode_backlinks": memory_store::Store::episode_backlinks(&link_rows, meta.as_ref().map(|m| m.project.as_str()).unwrap_or(name)),
        "repo_detected": detected,
        "repo_name": repo.as_deref().map(memory_core::project::repo_short_name),
        "repo_url": repo.as_deref().and_then(memory_core::project::repo_web_url),
        "all_tags": tags,
        "all_projects": all.iter().map(|m| m.project.clone()).collect::<Vec<_>>(),
    })))
}

/// Simpan metadata proyek. Semua kolom kosong = metadata dihapus.
async fn project_meta_put(State(st): State<AppState>, Path(s): Path<String>, Json(b): Json<ProjectMetaIn>) -> ApiResult<Value> {
    check(&s)?;
    let name = b.project.trim();
    if name.is_empty() || name.chars().count() > 200 {
        return Err(ApiError::BadRequest("nama proyek wajib diisi (maks. 200 karakter)".into()));
    }
    if b.notes.chars().count() > 50_000 {
        return Err(ApiError::BadRequest("catatan terlalu panjang (maks. 50.000 karakter)".into()));
    }
    if b.repo.as_deref().is_some_and(|r| r.chars().count() > 500) {
        return Err(ApiError::BadRequest("repositori terlalu panjang".into()));
    }
    if b.tags.len() > 50 {
        return Err(ApiError::BadRequest("tag terlalu banyak (maks. 50)".into()));
    }
    let prev = st.rt.store.get_project_meta(&s, name).await?;
    // Pertahankan nama kanonik yang sudah tersimpan (mis. `BACKEND/api-x` saat dibuka sebagai `api-x`).
    let canonical = prev.as_ref().map(|p| p.project.clone()).unwrap_or_else(|| name.to_string());
    let m = ProjectMeta::build(&s, &canonical, b.repo.as_deref(), &b.notes, &b.tags, prev.as_ref());
    if m.repo.is_none() && m.notes.trim().is_empty() && m.tags.is_empty() {
        if let Some(p) = &prev {
            st.rt.store.delete_project_meta(&p.id).await?;
        }
        return Ok(Json(json!({"meta": null, "deleted": prev.is_some()})));
    }
    st.rt.store.put_project_meta(&m).await?;
    Ok(Json(json!({"meta": m})))
}

#[derive(Deserialize)]
struct ProjectShareIn {
    project: String,
    /// `init` | `export` | `import` | `hooks` | `unhooks`
    action: String,
    /// Akar repo bila belum dikenal dari sesi kerja (path absolut).
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    dry_run: bool,
}

async fn share_root(st: &AppState, s: &str, project: &str, root: Option<&str>) -> Result<Option<PathBuf>, ApiError> {
    if let Some(r) = root.map(str::trim).filter(|r| !r.is_empty()) {
        let p = PathBuf::from(r);
        if !p.is_absolute() || !p.is_dir() {
            return Err(ApiError::BadRequest("folder repo harus path absolut yang ada".into()));
        }
        return Ok(Some(p));
    }
    Ok(memory_jobs::repo_bundle::root_for_project(&st.rt, s, project).await?)
}

/// Status berbagi memori proyek lewat repo git (`.engram/`). `found: false` bila akar repo
/// proyek belum diketahui (belum ada sesi kerja di repo itu).
async fn project_share_get(State(st): State<AppState>, Path(s): Path<String>, Query(q): Query<ProjectQ>) -> ApiResult<Value> {
    check(&s)?;
    let name = q.project.as_deref().map(str::trim).filter(|p| !p.is_empty()).ok_or_else(|| ApiError::BadRequest("project wajib diisi".into()))?;
    let Some(root) = share_root(&st, &s, name, None).await? else {
        return Ok(Json(json!({"found": false, "enabled": st.rt.cfg.repo_share.enabled})));
    };
    let status = memory_jobs::repo_bundle::status(&st.rt, &s, &root, Some(name)).await?;
    Ok(Json(json!({"found": true, "status": status})))
}

async fn project_share_post(State(st): State<AppState>, Path(s): Path<String>, Json(b): Json<ProjectShareIn>) -> ApiResult<Value> {
    use memory_jobs::repo_bundle as rb;
    check(&s)?;
    let name = b.project.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("project wajib diisi".into()));
    }
    let root = share_root(&st, &s, name, b.root.as_deref()).await?.ok_or_else(|| ApiError::BadRequest("folder repo proyek ini belum diketahui; isi path repo".into()))?;
    let result = match b.action.as_str() {
        "init" => json!(rb::init_repo(&st.rt, &s, &root, Some(name), false).await?),
        "export" => json!(rb::export_with(&st.rt, &s, &root, Some(name), b.dry_run).await?),
        "import" => json!(rb::import_repo(&st.rt, &s, &root, Some(name)).await?),
        "hooks" | "unhooks" => json!(rb::git_hooks(&root, b.action == "unhooks")?),
        other => return Err(ApiError::BadRequest(format!("aksi tidak dikenal: {other}"))),
    };
    let status = rb::status(&st.rt, &s, &root, Some(name)).await?;
    Ok(Json(json!({"result": result, "status": status})))
}

/// Peta berkas -> observasi yang menyentuhnya, untuk tab Berkas halaman proyek.
async fn project_files(State(st): State<AppState>, Path(s): Path<String>, Query(q): Query<ProjectQ>) -> ApiResult<Value> {
    check(&s)?;
    let names = match q.project.as_deref().filter(|p| !p.trim().is_empty()) {
        Some(p) => st.rt.store.project_names(&s, p).await?,
        None => return Err(ApiError::BadRequest("project wajib diisi".into())),
    };
    let eps = st
        .rt
        .store
        .list_episodes(&s, &EpisodeFilter { status: Some(EpisodeStatus::Active), projects: names, newest_first: true, limit: 3000, ..Default::default() })
        .await?;
    let mut files: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
    for e in &eps {
        for (f, modified) in e.files_modified.iter().map(|f| (f, true)).chain(e.files_read.iter().map(|f| (f, false))) {
            let v = files.entry(f.clone()).or_insert_with(|| json!({"path": f, "modified": 0, "read": 0, "episodes": []}));
            let k = if modified { "modified" } else { "read" };
            v[k] = json!(v[k].as_u64().unwrap_or(0) + 1);
            if v["episodes"].as_array().is_some_and(|a| a.len() < 8 && !a.iter().any(|x| x["id"] == e.id)) {
                v["episodes"].as_array_mut().unwrap().push(json!({"id": e.id, "title": e.title, "obs_type": e.obs_type, "at": e.created_at.chars().take(10).collect::<String>(), "modified": modified}));
            }
        }
    }
    let mut out: Vec<Value> = files.into_values().collect();
    out.sort_by(|a, b| (b["modified"].as_u64().unwrap_or(0) * 3 + b["read"].as_u64().unwrap_or(0)).cmp(&(a["modified"].as_u64().unwrap_or(0) * 3 + a["read"].as_u64().unwrap_or(0))));
    Ok(Json(json!({"files": out})))
}

/// Detail sesi: record sesi, episode yang lahir darinya, dan giliran mentah dari spool.
async fn session_detail(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let sid = memory_extract::session::clean_session_id(&id);
    let session = st.rt.store.get_session(&sid).await?.ok_or_else(|| ApiError::NotFound("sesi tidak ada".into()))?;
    let episodes = st
        .rt
        .store
        .list_episodes(&session.subject, &EpisodeFilter { status: None, session_id: Some(sid.clone()), newest_first: false, limit: 500, ..Default::default() })
        .await?;
    let turns = st.rt.pipeline.session_turns(&sid);
    Ok(Json(json!({"session": session, "episodes": episodes, "turns": turns})))
}

#[derive(Deserialize)]
struct RememberIn {
    text: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    project: Option<String>,
}

async fn remember(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Json(b): Json<RememberIn>,
) -> ApiResult<Value> {
    check(&s)?;
    let out =
        remember_text_with_project(&st, &s, &b.text, b.source.as_deref(), b.project.as_deref())
            .await?;
    Ok(Json(out))
}

#[allow(dead_code)]
pub(crate) async fn remember_text(
    st: &AppState,
    subject: &str,
    text: &str,
    source: Option<&str>,
) -> Result<Value, ApiError> {
    remember_text_with_project(st, subject, text, source, None).await
}

pub(crate) async fn remember_text_with_project(
    st: &AppState,
    subject: &str,
    text: &str,
    source: Option<&str>,
    project: Option<&str>,
) -> Result<Value, ApiError> {
    if text.trim().is_empty() {
        return Err(ApiError::BadRequest("teks kosong".into()));
    }
    if memory_core::privacy::sanitize(text, &st.rt.cfg.privacy).is_none() {
        return Err(ApiError::BadRequest("teks kosong setelah blok <private> dibuang; tidak ada yang disimpan".into()));
    }
    let out = st
        .rt
        .pipeline
        .ingest(IngestDoc {
            subject: subject.to_string(),
            kind: SourceKind::AgentConversation,
            title: format!("Diingat agent: {}", source.unwrap_or("percakapan")),
            text: text.to_string(),
            project: project.map(str::to_string),
            occurred_from: Some(chrono::Utc::now().format("%Y-%m-%d").to_string()),
            meta: json!({"via": "remember", "source": source, "project": project}),
            ..Default::default()
        })
        .await?;
    Ok(
        json!({"source_id": out.source.id, "duplicate": out.duplicate, "job_id": out.job_id,
              "note": "Disimpan sebagai sumber; pipeline akan menyaring basa-basi sebelum menjadi episode."}),
    )
}

async fn interview_next(
    State(st): State<AppState>,
    Path(s): Path<String>,
) -> ApiResult<memory_extract::interview::InterviewQuestion> {
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

async fn interview_answer(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Json(b): Json<AnswerIn>,
) -> ApiResult<Value> {
    check(&s)?;
    if b.answer.trim().is_empty() {
        return Err(ApiError::BadRequest("jawaban kosong".into()));
    }
    let out = st
        .rt
        .pipeline
        .answer_question(&s, &b.question, &b.answer)
        .await?;
    Ok(Json(
        json!({"source_id": out.source.id, "job_id": out.job_id, "duplicate": out.duplicate}),
    ))
}

async fn maintenance(
    State(st): State<AppState>,
    Path((s, kind)): Path<(String, String)>,
) -> ApiResult<Value> {
    check(&s)?;
    let kind = match kind.as_str() {
        "consolidate" => memory_jobs::CONSOLIDATE,
        "distill" => memory_jobs::DISTILL,
        "voice" => memory_jobs::VOICE,
        "compact" => memory_jobs::COMPACT,
        "review" | "auto_review" => memory_jobs::AUTO_REVIEW,
        other => {
            return Err(ApiError::BadRequest(format!(
                "jenis tidak dikenal: {other}"
            )));
        }
    };
    let job = st
        .rt
        .enqueue(
            kind,
            json!({"subject": s}),
            format!("{kind}:{s}:manual:{}", memory_core::new_id()),
        )
        .await?;
    Ok(Json(json!({"job_id": job.id})))
}

async fn compact_candidates(
    State(st): State<AppState>,
    Path(s): Path<String>,
) -> ApiResult<memory_extract::compact::CompactCandidates> {
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
    let url = st.mcp_url.clone().ok_or_else(|| {
        ApiError::BadRequest(
            "pemasangan otomatis hanya tersedia di aplikasi desktop atau server lokal".into(),
        )
    })?;
    let dirs = crate::install::Dirs::from_env()
        .ok_or_else(|| ApiError::Internal("folder home pengguna tidak diketahui".into()))?;
    Ok((url, dirs))
}

fn mcp_client(id: &str) -> Result<crate::install::Client, ApiError> {
    crate::install::Client::parse(id)
        .ok_or_else(|| ApiError::NotFound(format!("client tidak dikenal: {id}")))
}

async fn mcp_clients(State(st): State<AppState>) -> ApiResult<Value> {
    let Ok((url, dirs)) = mcp_target(&st) else {
        return Ok(Json(json!({"enabled": false, "clients": []})));
    };
    let token = st.token.as_deref().map(String::as_str);
    let clients: Vec<_> = crate::install::Client::ALL
        .into_iter()
        .map(|c| crate::install::status(&dirs, c, &url, token))
        .collect();
    Ok(Json(
        json!({"enabled": true, "url": url, "name": crate::install::SERVER_NAME, "clients": clients}),
    ))
}

async fn mcp_install(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let c = mcp_client(&id)?;
    let (url, dirs) = mcp_target(&st)?;
    let token = st.token.as_deref().map(String::as_str);
    let msg = crate::install::install(&dirs, c, &url, token)
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(
        json!({"ok": true, "message": msg, "status": crate::install::status(&dirs, c, &url, token)}),
    ))
}

async fn mcp_uninstall(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let c = mcp_client(&id)?;
    let (url, dirs) = mcp_target(&st)?;
    let token = st.token.as_deref().map(String::as_str);
    let msg = crate::install::uninstall(&dirs, c)
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(
        json!({"ok": true, "message": msg, "status": crate::install::status(&dirs, c, &url, token)}),
    ))
}

async fn services(State(st): State<AppState>) -> ApiResult<Value> {
    let Some(c) = &st.services else {
        return Ok(Json(json!({"enabled": false, "services": []})));
    };
    Ok(Json(
        json!({"enabled": true, "services": service_list(&st, c.as_ref()).await, "update_progress": c.update_progress()}),
    ))
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
    let light_separate = cfg
        .light_base_url
        .as_deref()
        .is_some_and(|u| u.trim_end_matches('/') != cfg.base_url.trim_end_matches('/'));
    let mut targets = vec![(
        "llm",
        "Server LLM",
        ModelRole::Extract,
        cfg.base_url.clone(),
        if light_separate {
            cfg.extract_model.clone()
        } else {
            format!("{}, {}", cfg.extract_model, cfg.light_model)
        },
    )];
    if light_separate {
        targets.push((
            "llm-light",
            "Server LLM ringan",
            ModelRole::Light,
            cfg.light_base_url.clone().unwrap_or_default(),
            cfg.light_model.clone(),
        ));
    }
    let probe = |role| async move {
        match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            st.rt.pipeline.llm.health(role),
        )
        .await
        {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e.to_string()),
            Err(_) => Some("tidak menjawab dalam 3 detik".to_string()),
        }
    };
    let (main_err, light_err) = tokio::join!(probe(ModelRole::Extract), async {
        if light_separate {
            probe(ModelRole::Light).await
        } else {
            None
        }
    });
    for ((id, name, _, url, models), error) in targets.into_iter().zip([main_err, light_err]) {
        let desc = format!(
            "Server di luar engram ({url} · {models}). Dibutuhkan untuk ekstraksi memori dan jawaban; nyalakan di mesinnya bila berhenti."
        );
        out.push(external_service(id, name, desc, error));
    }
    out
}

/// Server embedding berjalan di luar engram (mis. TEI di mesin GPU): cek dengan
/// satu permintaan embedding sungguhan. Tidak bisa dijalankan dari sini.
async fn embedding_status(st: &AppState) -> Value {
    let cfg = &st.rt.cfg.embedding;
    let target = if cfg.provider == "hash" {
        "hash (tanpa server)".to_string()
    } else {
        format!("{} · {}", cfg.base_url, cfg.model)
    };
    let probe = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        st.rt.pipeline.embedder.embed_one("ping"),
    )
    .await;
    let error = match probe {
        Ok(Ok(_)) => None,
        Ok(Err(e)) => Some(e.to_string()),
        Err(_) => Some("tidak menjawab dalam 3 detik".to_string()),
    };
    external_service(
        "embedding",
        "Server embedding",
        format!(
            "Server di luar engram ({target}). Dibutuhkan untuk memproses memori baru; nyalakan di mesinnya bila berhenti."
        ),
        error,
    )
}

async fn service_start(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let c = st
        .services
        .as_ref()
        .ok_or_else(|| ApiError::BadRequest("pengendali layanan tidak tersedia".into()))?;
    c.start(&id).map_err(ApiError::BadRequest)?;
    Ok(Json(
        json!({"ok": true, "services": service_list(&st, c.as_ref()).await}),
    ))
}

async fn services_start_all(State(st): State<AppState>) -> ApiResult<Value> {
    let started = match &st.services {
        Some(c) => c.start_all().map_err(ApiError::BadRequest)?,
        None => Vec::new(),
    };
    let requeued_stale = st.rt.store.requeue_stale_jobs().await.unwrap_or(0);
    let retry = st.rt.retry_failed(false).await.unwrap_or_default();
    let list = match &st.services {
        Some(c) => service_list(&st, c.as_ref()).await,
        None => {
            let (emb, llm) = tokio::join!(embedding_status(&st), llm_status(&st));
            let mut arr = vec![emb];
            arr.extend(llm);
            json!(arr)
        }
    };
    Ok(Json(json!({
        "ok": true,
        "enabled": st.services.is_some(),
        "started": started,
        "requeued_stale": requeued_stale,
        "retried_sources": retry.requeued,
        "backend_error": retry.backend_error,
        "services": list,
    })))
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
    let plan = memory_jobs::storage::plan_move(st.rt.vault.root(), &to)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let msg = c.move_vault(&plan.target).map_err(ApiError::BadRequest)?;
    Ok(Json(
        json!({"ok": true, "target": plan.target, "use_existing": plan.use_existing, "message": msg}),
    ))
}

#[derive(Deserialize)]
struct PickIn {
    #[serde(default)]
    title: Option<String>,
}

async fn pick_folder(State(st): State<AppState>, Json(b): Json<PickIn>) -> ApiResult<Value> {
    let c = st
        .services
        .clone()
        .filter(|c| c.can_manage_storage())
        .ok_or_else(|| {
            ApiError::BadRequest(
                "dialog folder hanya tersedia di aplikasi desktop; ketik path folder".into(),
            )
        })?;
    let title = b.title.unwrap_or_else(|| "Pilih folder".into());
    let path = tokio::task::spawn_blocking(move || c.pick_folder(&title))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(json!({"path": path})))
}

async fn sync_status(State(st): State<AppState>) -> ApiResult<memory_jobs::sync::SyncStatus> {
    let rt = st.rt.clone();
    // Membaca folder Drive bisa lambat (berkas online-only); jangan blok runtime.
    Ok(Json(
        tokio::task::spawn_blocking(move || memory_jobs::sync::status(&rt))
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?,
    ))
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

async fn sync_settings(
    State(st): State<AppState>,
    Json(b): Json<SyncIn>,
) -> ApiResult<memory_jobs::sync::SyncStatus> {
    use memory_jobs::sync;
    let mut s = sync::load_settings(&st.rt.vault);
    s.dir = b
        .dir
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    if let Some(m) = b.interval_minutes {
        s.interval_minutes = m;
    }
    if b.device_name.is_some() {
        s.device_name = b.device_name;
    }
    if let Some(p) = &b.passphrase {
        sync::set_passphrase(&st.rt.vault, Some(p.as_str()))
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    }
    sync::save_settings(&st.rt.vault, s).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    sync_status(State(st)).await
}

async fn sync_run(State(st): State<AppState>) -> ApiResult<memory_jobs::sync::SyncReport> {
    memory_jobs::sync::sync_now(&st.rt)
        .await
        .map(Json)
        .map_err(|e| ApiError::BadRequest(e.to_string()))
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
        let res = tokio::task::spawn_blocking(move || c.check_update())
            .await
            .map_err(blocking_err)?;
        if let Some(r) = res {
            return r.map(Json).map_err(ApiError::BadRequest);
        }
    }
    crate::settings::check_github()
        .await
        .map(Json)
        .map_err(ApiError::BadRequest)
}

async fn update_install(State(st): State<AppState>) -> ApiResult<Value> {
    let c = st.services.as_ref().ok_or_else(|| {
        ApiError::BadRequest(
            "pasang update hanya tersedia di aplikasi desktop; unduh rilis baru dari GitHub".into(),
        )
    })?;
    let msg = c.install_update().map_err(ApiError::BadRequest)?;
    Ok(Json(
        json!({"ok": true, "message": msg, "progress": c.update_progress()}),
    ))
}

async fn update_progress(State(st): State<AppState>) -> ApiResult<crate::settings::UpdateProgress> {
    let p = st
        .services
        .as_ref()
        .and_then(|c| c.update_progress())
        .unwrap_or_default();
    Ok(Json(p))
}

#[derive(Deserialize)]
struct OnIn {
    enabled: bool,
}

async fn update_auto(State(st): State<AppState>, Json(b): Json<OnIn>) -> ApiResult<Value> {
    let c = st.services.as_ref().ok_or_else(|| {
        ApiError::BadRequest("pengaturan ini hanya ada di aplikasi desktop".into())
    })?;
    c.set_auto_update(b.enabled).map_err(ApiError::BadRequest)?;
    Ok(Json(json!({"ok": true, "auto_update": c.auto_update()})))
}

/// Nilai pengaturan dari memory.toml, dan apakah berbeda dari yang sedang dipakai.
fn settings_view(st: &AppState) -> Result<Value, ApiError> {
    use crate::settings::{editable_values, preview};
    let file =
        preview(st.config_path.as_deref(), &st.rt.cfg, None).map_err(ApiError::BadRequest)?;
    let (saved, running) = (editable_values(&file), editable_values(&st.rt.cfg));
    // Rahasia hanya terlihat sebagai *_set; bandingkan nilai aslinya juga.
    let secret_changed = file.llm.api_key != st.rt.cfg.llm.api_key
        || file.embedding.api_key != st.rt.cfg.embedding.api_key;
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
    let path = st.config_path.clone().ok_or_else(|| {
        ApiError::BadRequest("lokasi memory.toml tidak diketahui; ubah berkasnya langsung".into())
    })?;
    tokio::task::spawn_blocking(move || crate::settings::save(&path, &b.values))
        .await
        .map_err(blocking_err)?
        .map_err(ApiError::BadRequest)?;
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
    let cfg = settings::preview(st.config_path.as_deref(), &st.rt.cfg, b.values.as_ref())
        .map_err(ApiError::BadRequest)?;
    Ok(Json(match b.kind.as_str() {
        "llm" => settings::test_llm(&cfg).await,
        "embedding" => settings::test_embedding(&cfg).await,
        "transcription" => settings::test_transcription(&cfg).await,
        k => {
            return Err(ApiError::BadRequest(format!(
                "jenis tes tidak dikenal: {k}"
            )));
        }
    }))
}

async fn restart(State(st): State<AppState>) -> ApiResult<Value> {
    let c = st.services.as_ref().ok_or_else(|| {
        ApiError::BadRequest("jalankan ulang `engram serve` agar pengaturan baru berlaku".into())
    })?;
    let msg = c.restart().map_err(ApiError::BadRequest)?;
    Ok(Json(json!({"ok": true, "message": msg})))
}

#[derive(Deserialize)]
struct OpenIn {
    target: String,
    /// Untuk `project-repo`: subjek & proyek yang halaman repositorinya dibuka.
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    project: Option<String>,
}

/// Hanya target yang dikenal; UI tidak bisa membuka path atau URL sembarang.
async fn open_target(State(st): State<AppState>, Json(b): Json<OpenIn>) -> ApiResult<Value> {
    let c = st
        .services
        .as_ref()
        .ok_or_else(|| ApiError::BadRequest("hanya tersedia di aplikasi desktop".into()))?;
    let repo = crate::settings::REPO_URL;
    let vault = st.rt.vault.root();
    let target = match b.target.as_str() {
        "vault" => vault.display().to_string(),
        "inbox" => {
            let dir = vault.join("inbox");
            std::fs::create_dir_all(&dir)?;
            dir.display().to_string()
        }
        "config" => st
            .config_path
            .as_ref()
            .and_then(|p| p.parent())
            .ok_or_else(|| ApiError::BadRequest("lokasi memory.toml tidak diketahui".into()))?
            .display()
            .to_string(),
        "repo" => repo.to_string(),
        // URL diturunkan server dari metadata tersimpan / `.git/config`, bukan dari UI.
        "project-repo" => {
            let (Some(s), Some(p)) = (b.subject.as_deref(), b.project.as_deref().map(str::trim).filter(|p| !p.is_empty())) else {
                return Err(ApiError::BadRequest("subject dan project wajib diisi".into()));
            };
            check(s)?;
            let stored = st.rt.store.get_project_meta(s, p).await?.and_then(|m| m.repo);
            let remote = match stored {
                Some(r) => Some(r),
                None => {
                    let names = st.rt.store.project_names(s, p).await?;
                    let sessions = st.rt.store.list_sessions(s, &names, 50).await?;
                    memory_core::project::remote_for_project(p, sessions.iter().filter_map(|se| se.cwd.as_deref()))
                }
            };
            remote
                .as_deref()
                .and_then(memory_core::project::repo_web_url)
                .filter(|u| u.starts_with("https://"))
                .ok_or_else(|| ApiError::BadRequest("repositori proyek belum diketahui atau bukan URL web".into()))?
        }
        "releases" => format!("{repo}/releases"),
        "issues" => format!("{repo}/issues"),
        t => return Err(ApiError::BadRequest(format!("target tidak dikenal: {t}"))),
    };
    c.open(&target).map_err(ApiError::BadRequest)?;
    Ok(Json(json!({"ok": true})))
}

// ---------------------------------------------------------------- folder penting (watched folders via agy CLI)

async fn watched_folders_list(
    State(st): State<AppState>,
    Path(s): Path<String>,
) -> ApiResult<memory_jobs::watch::WatchStatus> {
    check(&s)?;
    let rt = st.rt.clone();
    Ok(Json(
        tokio::task::spawn_blocking(move || memory_jobs::watch::status(&rt, Some(&s)))
            .await
            .map_err(blocking_err)?,
    ))
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

async fn watched_folders_add(
    State(st): State<AppState>,
    Path(s): Path<String>,
    Json(b): Json<WatchedFolderIn>,
) -> ApiResult<Value> {
    check(&s)?;
    let raw_path = b.path.trim().to_string();
    if raw_path.is_empty() {
        return Err(ApiError::BadRequest("path folder kosong".into()));
    }
    st.rt.pipeline.ensure_subject(&s, None).await?;
    let folder = memory_jobs::watch::add_folder(
        &st.rt.vault,
        &s,
        std::path::Path::new(&raw_path),
        b.label.as_deref(),
    )
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let scan = if b.scan.unwrap_or(false) {
        Some(
            memory_jobs::watch::scan_one(&st.rt, &folder.id, false, 0)
                .await
                .map_err(|e| ApiError::BadRequest(e.to_string()))?,
        )
    } else {
        None
    };
    let status = memory_jobs::watch::status(&st.rt, Some(&s));
    Ok(Json(
        json!({"folder": folder, "scan": scan, "status": status}),
    ))
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

async fn watched_folder_remove(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    let removed = memory_jobs::watch::remove_folder(&st.rt.vault, &id)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if !removed {
        return Err(ApiError::NotFound("folder penting tidak ditemukan".into()));
    }
    Ok(Json(json!({"ok": true})))
}
