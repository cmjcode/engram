//! Orkestrasi per sumber:
//!
//! Fase A (paralel, boleh diulang): Segment → embed → Score → Extract.
//! Hasilnya disimpan sebagai *staging* di record sumber, sehingga job yang
//! gagal di tengah tidak mengulang panggilan LLM.
//!
//! Fase B (berurutan per subjek): Resolve → Reconcile → Merge → Commit, satu
//! transaksi per episode dengan ID deterministik (idempoten saat diulang).

use std::collections::HashMap;

use chrono::NaiveDate;
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;

use memory_core::kin::normalize_kin_term;
use memory_core::model::*;
use memory_core::score::{Route, ScoreInput, score};
use memory_core::text::{normalize, quote_supported, truncate_chars};
use memory_core::time::{TimeRef, standard_phase_for_age};
use memory_core::ids::now_rfc3339;
use memory_llm::{ModelRole, complete_json};
use memory_store::{VectorIndex, entity_doc};

use crate::llm_types::ExtractionOut;
use crate::reconcile::{Action, reconcile};
use crate::resolve::{Resolver, entity_embed_text};
use crate::segment::Segment;
use crate::{ExtractError, Pipeline, Result, det_id, prompts};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Staged {
    seg: Segment,
    route: Route,
    importance: f32,
    #[serde(default)]
    back_refs: u32,
    about: bool,
    extraction: Option<ExtractionOut>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Staging {
    items: Vec<Staged>,
}

/// Konteks sumber minimal untuk commit (juga dipakai promosi stream item).
#[derive(Debug, Clone)]
pub struct SourceCtx {
    pub id: String,
    pub kind: SourceKind,
    pub occurred_from: Option<String>,
    pub project: Option<String>,
    pub created_at: String,
    /// Sesi agent asal (sumber dari hook `engram hook`).
    pub session_id: Option<String>,
    pub prompt_number: Option<u32>,
}

impl From<&Source> for SourceCtx {
    fn from(s: &Source) -> Self {
        let project = s.project.clone().or_else(|| {
            s.meta["source"]
                .as_str()
                .and_then(|raw| parse_source_tag(raw).1)
                .or_else(|| {
                    s.title
                        .strip_prefix("Diingat agent: ")
                        .and_then(|raw| parse_source_tag(raw).1)
                })
        });
        Self {
            id: s.id.clone(),
            kind: s.kind,
            occurred_from: s.occurred_from.clone(),
            project,
            created_at: s.created_at.clone(),
            session_id: s.meta["session_id"].as_str().map(str::to_string),
            prompt_number: s.meta["prompt_number"].as_u64().map(|n| n as u32),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EpisodeCommit {
    pub episode_id: String,
    pub created: bool,
    pub retelling: bool,
    pub facts: u32,
    pub rejected: u32,
    pub quarantined: u32,
    pub lessons: u32,
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

fn iso(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn category_key(category: &str, predicate: &str) -> String {
    let c = normalize(if category.trim().is_empty() { predicate } else { category });
    c.replace(' ', "_")
}

impl Pipeline {
    async fn progress(&self, job_id: Option<&str>, stage: &str, done: usize, total: usize) {
        if let Some(j) = job_id {
            let _ = self.store.job_progress(j, stage, done as u32, total as u32).await;
        }
    }

    /// Proses satu sumber dari awal sampai commit. Idempoten.
    pub async fn process_source(&self, source_id: &str, job_id: Option<&str>) -> Result<SourceStats> {
        let source = self.store.get_source(source_id).await?.ok_or_else(|| ExtractError::Invalid(format!("sumber {source_id} tidak ada")))?;
        if matches!(source.status, SourceStatus::Done | SourceStatus::Skipped) {
            return Ok(source.stats);
        }
        self.store.set_source_status(source_id, SourceStatus::Processing, None).await?;
        match self.process_inner(&source, job_id).await {
            Ok(stats) => {
                self.store.set_source_stats(source_id, &stats).await?;
                let status = if stats.episodes + stats.stream_items == 0 { SourceStatus::Skipped } else { SourceStatus::Done };
                self.store.set_source_status(source_id, status, None).await?;
                self.store.patch("source", source_id, json!({"meta": {"staging": null}})).await?;
                Ok(stats)
            }
            Err(e) => {
                self.store.set_source_status(source_id, SourceStatus::Failed, Some(e.to_string())).await?;
                Err(e)
            }
        }
    }

    async fn process_inner(&self, source: &Source, job_id: Option<&str>) -> Result<SourceStats> {
        let subject = self.ensure_subject(&source.subject, None).await?;
        let staging = match source.meta.get("staging").filter(|v| !v.is_null()) {
            Some(v) => serde_json::from_value::<Staging>(v.clone()).map_err(|e| ExtractError::Invalid(e.to_string()))?,
            None => {
                let st = self.stage(source, &subject, job_id).await?;
                self.store.patch("source", &source.id, json!({"meta": {"staging": st}})).await?;
                st
            }
        };

        let lock = self.subject_lock(&subject.id).await;
        let _guard = lock.lock().await;
        // Muat ulang subjek di dalam kunci (self entity mungkin baru dibuat).
        let subject = self.store.get_subject(&subject.id).await?.unwrap_or(subject);
        let ctx = SourceCtx::from(source);
        let mut stats = SourceStats { segments: staging.items.len() as u32, ..Default::default() };
        let total = staging.items.len();
        for (i, it) in staging.items.iter().enumerate() {
            self.progress(job_id, "commit", i, total).await;
            if !it.about {
                stats.not_about_subject += 1;
                continue;
            }
            match it.route {
                Route::Drop => stats.dropped += 1,
                Route::Stream | Route::GrayZone => {
                    let gray = it.route == Route::GrayZone;
                    self.commit_stream(&ctx, &subject.id, &it.seg, it.importance, it.back_refs, gray).await?;
                    stats.stream_items += 1;
                    if gray {
                        stats.gray_zone += 1;
                    }
                }
                Route::Episode => {
                    let ext = it.extraction.clone().unwrap_or_default();
                    let ep_id = det_id(&[&source.id, "ep", &it.seg.start.to_string(), &it.seg.end.to_string()]);
                    let c = self.commit_episode(&ctx, &subject, &it.seg, it.importance, &ext, &ep_id).await?;
                    if c.created {
                        stats.episodes += 1;
                    }
                    if c.retelling {
                        stats.retellings += 1;
                    }
                    stats.facts += c.facts;
                    stats.facts_rejected += c.rejected;
                    stats.quarantined += c.quarantined;
                }
            }
        }
        self.progress(job_id, "selesai", total, total).await;
        if let Some(sid) = &ctx.session_id
            && stats.episodes > 0
        {
            self.bump_session_episodes(sid, stats.episodes).await?;
        }
        Ok(stats)
    }

    /// Fase A: segment, score, extract.
    async fn stage(&self, source: &Source, subject: &Subject, job_id: Option<&str>) -> Result<Staging> {
        if source.kind == SourceKind::AgentSession {
            return self.stage_session(source, subject, job_id).await;
        }
        let text = self.vault.read_rel(&source.path)?;
        self.progress(job_id, "segment", 0, 1).await;
        let examples = self.few_shot_examples(&subject.id).await;
        let segments = self.segment(&subject.name, &text, &examples).await;
        if segments.is_empty() {
            return Ok(Staging::default());
        }
        self.progress(job_id, "score", 0, segments.len()).await;
        let texts: Vec<String> = segments.iter().map(|s| s.text.clone()).collect();
        let embs: Vec<Option<Vec<f32>>> = match self.embedder.embed(&texts).await {
            Ok(v) => v.into_iter().map(Some).collect(),
            Err(e) => {
                tracing::warn!(error = %e, "embedding segmen gagal; lanjut tanpa pencarian kemiripan vektor");
                vec![None; texts.len()]
            }
        };
        let alias_keys = self.known_alias_keys(&subject.id).await?;
        let index = self.store.vector_index();
        let mut items = Vec::with_capacity(segments.len());
        for (seg, emb) in segments.into_iter().zip(embs) {
            let hits = match &emb {
                Some(v) => index.search("episode", &subject.id, v, 1, "status = 'active'").await?,
                None => vec![],
            };
            let top = hits.first().map(|h| h.similarity).unwrap_or(0.0);
            let score_text = crate::segment::strip_chat_prefix(&seg.text);
            let padded = format!(" {} ", normalize(&score_text));
            let back_refs = alias_keys.iter().filter(|k| padded.contains(&format!(" {k} "))).count() as u32;
            let r = score(
                &ScoreInput { text: &score_text, kind: source.kind, novelty: 1.0 - top, back_refs, significance: seg.significance },
                &self.cfg.score,
            );
            // Cerita yang diceritakan ulang selalu ditautkan, bukan dibuang.
            let route = if top >= self.cfg.pipeline.merge_similarity && r.route != Route::Drop { Route::Episode } else { r.route };
            items.push(Staged { about: seg.about_subject, seg, route, importance: r.importance, back_refs, extraction: None });
        }

        let todo: Vec<usize> = items.iter().enumerate().filter(|(_, it)| it.about && it.route == Route::Episode).map(|(i, _)| i).collect();
        let known = self.known_entities_prompt(&subject.id).await?;
        let birth = subject.anchors.birth.map(iso);
        let observed_default = source.occurred_from.clone().unwrap_or_else(|| source.created_at.chars().take(10).collect());
        let total = todo.len();
        self.progress(job_id, "extract", 0, total).await;
        let results: Vec<(usize, Result<ExtractionOut>)> = stream::iter(todo.into_iter().map(|i| {
            let seg = items[i].seg.clone();
            let observed = seg.date.clone().unwrap_or_else(|| observed_default.clone());
            let (known, birth, name) = (known.clone(), birth.clone(), subject.name.clone());
            async move { (i, self.extract(&name, &observed, birth.as_deref(), &known, &seg.text).await) }
        }))
        .buffer_unordered(3)
        .collect()
        .await;
        for (i, r) in results {
            let ext = match r {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "ekstraksi LLM gagal; memakai fallback segmen");
                    let clean = crate::segment::strip_chat_prefix(&items[i].seg.text);
                    ExtractionOut {
                        title: if items[i].seg.topic.trim().is_empty() {
                            truncate_chars(clean.lines().next().unwrap_or(&clean), 72)
                        } else {
                            items[i].seg.topic.clone()
                        },
                        summary: truncate_chars(&clean, 300),
                        ..Default::default()
                    }
                }
            };
            items[i].extraction = Some(ext);
        }
        Ok(Staging { items })
    }

    /// Fase A untuk giliran sesi agent: satu panggilan LLM menghasilkan observasi bertipe
    /// (keputusan, bugfix, temuan, ...) dan ringkasan giliran; tanpa segmentasi.
    async fn stage_session(&self, source: &Source, subject: &Subject, job_id: Option<&str>) -> Result<Staging> {
        use crate::llm_types::SessionExtraction;
        let text = self.vault.read_rel(&source.path)?;
        self.progress(job_id, "session", 0, 1).await;
        let known = self.known_entities_prompt(&subject.id).await?;
        let observed = source.occurred_from.clone().unwrap_or_else(|| source.created_at.chars().take(10).collect());
        let project = source.project.clone().or_else(|| source.meta["project"].as_str().map(str::to_string));
        let client = source.meta["client"].as_str().unwrap_or("agent").to_string();
        let user = prompts::session_user(&subject.name, project.as_deref(), &client, &observed, &known, &text);
        let ext: SessionExtraction = complete_json(self.llm.as_ref(), ModelRole::Extract, "session", prompts::SESSION_SYSTEM, &user, self.cfg.llm.max_retries).await?;
        if ext.skip {
            tracing::info!(source = %source.id, reason = ext.skip_reason.as_deref().unwrap_or(""), "giliran sesi dilewati");
            return Ok(Staging::default());
        }
        let meta_files = |k: &str| -> Vec<String> { source.meta[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default() };
        let mut items = Vec::new();
        for (i, o) in ext.observations.iter().enumerate().take(8) {
            if o.title.trim().is_empty() && o.narrative.trim().is_empty() {
                continue;
            }
            // Kutipan pendukung yang benar-benar ada di dokumen giliran ikut ke teks episode,
            // sehingga verifikasi kutipan fakta/pelajaran tetap berjalan seperti biasa.
            let mut quotes: Vec<String> = Vec::new();
            for q in o.quotes.iter().chain(o.facts.iter().map(|f| &f.quote)).chain(o.lessons.iter().map(|l| &l.quote)) {
                if !q.trim().is_empty() && quote_supported(q, &text) && !quotes.contains(q) {
                    quotes.push(q.clone());
                }
            }
            let mut body = o.narrative.trim().to_string();
            if body.is_empty() {
                body = o.title.trim().to_string();
            }
            if !o.facts_text.is_empty() {
                body.push_str("\n\nFakta:\n");
                for f in o.facts_text.iter().take(12) {
                    body.push_str(&format!("- {}\n", f.trim()));
                }
            }
            if !quotes.is_empty() {
                body.push_str("\nBukti:\n");
                for q in quotes.iter().take(12) {
                    body.push_str(&format!("- {}\n", q.trim()));
                }
            }
            let obs_type = match o.obs_type.trim().to_lowercase().as_str() {
                t @ ("decision" | "bugfix" | "discovery" | "feature" | "refactor" | "change") => t.to_string(),
                _ => "change".to_string(),
            };
            let importance = match obs_type.as_str() {
                "decision" | "bugfix" => 0.85,
                "discovery" | "feature" => 0.75,
                _ => 0.65,
            };
            let extraction = ExtractionOut {
                title: truncate_chars(o.title.trim(), 120),
                summary: truncate_chars(o.narrative.trim(), 400),
                entities: o.entities.clone(),
                facts: o.facts.iter().filter(|f| quote_supported(&f.quote, &text)).cloned().collect(),
                decisions: o.decisions.clone(),
                quotes: quotes.clone(),
                obs_type: Some(obs_type),
                concepts: o.concepts.clone(),
                files_read: o.files_read.clone(),
                files_modified: o.files_modified.clone(),
                lessons: o.lessons.iter().filter(|l| l.quote.trim().is_empty() || quote_supported(&l.quote, &text)).cloned().collect(),
                ..Default::default()
            };
            let seg = Segment { start: i, end: i, text: body, topic: o.title.trim().to_string(), about_subject: true, speaker: None, significance: Some(0.9), date: Some(observed.clone()) };
            items.push(Staged { seg, route: Route::Episode, importance, back_refs: 0, about: true, extraction: Some(extraction) });
        }
        if let Some(sum) = &ext.summary {
            let fields = [("Permintaan", &sum.request), ("Diselidiki", &sum.investigated), ("Dipelajari", &sum.learned), ("Selesai", &sum.completed), ("Langkah berikut", &sum.next_steps)];
            let body: String = fields.iter().filter(|(_, v)| !v.trim().is_empty()).map(|(k, v)| format!("{k}: {}\n", v.trim())).collect();
            if !body.trim().is_empty() {
                let title = if sum.request.trim().is_empty() { "Ringkasan sesi".to_string() } else { truncate_chars(sum.request.trim(), 100) };
                let summary = if sum.completed.trim().is_empty() { truncate_chars(&body, 300) } else { truncate_chars(sum.completed.trim(), 300) };
                let extraction = ExtractionOut {
                    title,
                    summary,
                    obs_type: Some("session_summary".into()),
                    files_read: meta_files("files_read"),
                    files_modified: meta_files("files_modified"),
                    ..Default::default()
                };
                let n = items.len();
                let seg = Segment { start: n, end: n, text: body, topic: "ringkasan sesi".into(), about_subject: true, speaker: None, significance: Some(0.7), date: Some(observed.clone()) };
                items.push(Staged { seg, route: Route::Episode, importance: 0.6, back_refs: 0, about: true, extraction: Some(extraction) });
            }
        }
        self.progress(job_id, "session", 1, 1).await;
        Ok(Staging { items })
    }

    pub async fn extract(&self, subject_name: &str, observed: &str, birth: Option<&str>, known: &str, text: &str) -> Result<ExtractionOut> {
        let user = prompts::extract_user(subject_name, observed, birth, known, text);
        Ok(complete_json(self.llm.as_ref(), ModelRole::Extract, "extract", prompts::EXTRACT_SYSTEM, &user, self.cfg.llm.max_retries).await?)
    }

    async fn few_shot_examples(&self, subject: &str) -> String {
        let Ok(items) = self.store.review_examples(subject, ReviewKind::GrayZone, self.cfg.pipeline.few_shot_examples).await else {
            return String::new();
        };
        items
            .iter()
            .map(|r| {
                let text = r.payload["text"].as_str().unwrap_or(&r.summary);
                let verdict = if r.status == ReviewStatus::Approved { "PENTING (significance tinggi)" } else { "NOISE (significance rendah)" };
                format!("- \"{}\" => {verdict}", truncate_chars(text, 160))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) async fn known_alias_keys(&self, subject: &str) -> Result<Vec<String>> {
        let ents = self.store.list_entities(subject, None, 1000, 0).await?;
        let mut keys: Vec<String> = ents
            .iter()
            .filter(|e| e.mention_count >= 2 && !e.is_self)
            .flat_map(|e| memory_store::alias_keys(&e.name, &e.aliases))
            .filter(|k| k.chars().count() >= 3 && normalize_kin_term(k).is_none())
            .collect();
        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    pub(crate) async fn known_entities_prompt(&self, subject: &str) -> Result<String> {
        let ents = self.store.list_entities(subject, None, 40, 0).await?;
        Ok(ents
            .iter()
            .map(|e| {
                let alias = if e.aliases.is_empty() { String::new() } else { format!(" alias: {}", e.aliases.join(", ")) };
                format!("- {} ({:?}){alias}", e.name, e.kind)
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    async fn commit_stream(&self, src: &SourceCtx, subject: &str, seg: &Segment, importance: f32, back_refs: u32, gray: bool) -> Result<()> {
        let id = det_id(&[&src.id, "stream", &seg.start.to_string(), &seg.end.to_string()]);
        if self.store.get::<StreamItem>("stream_item", &id).await?.is_some() {
            return Ok(());
        }
        let now = chrono::Utc::now();
        let item = StreamItem {
            id: id.clone(),
            subject: subject.to_string(),
            source_id: src.id.clone(),
            text: seg.text.clone(),
            speaker: seg.speaker.clone(),
            importance,
            back_refs,
            created_at: now_rfc3339(),
            expires_at: (now + chrono::Duration::days(self.cfg.score.stream_ttl_days)).format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            promoted_to: None,
            occurred_at: seg.date.clone(),
        };
        let mut tx = self.store.tx();
        tx.upsert("stream_item", &id, &item, None)?;
        if gray {
            let r = ReviewItem {
                id: det_id(&["gray", &id]),
                subject: subject.to_string(),
                kind: ReviewKind::GrayZone,
                summary: format!("Layak jadi episode? (skor {importance:.2}) \"{}\"", truncate_chars(&seg.text, 140)),
                targets: vec![id.clone()],
                payload: json!({"stream_item": id, "text": truncate_chars(&seg.text, 600), "importance": importance}),
                status: ReviewStatus::Open,
                note: None,
                created_at: now_rfc3339(),
                resolved_at: None,
            };
            tx.upsert("review_item", &r.id, &r, None)?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Fase B untuk satu episode: resolve, reconcile, merge, commit (atomik).
    pub async fn commit_episode(&self, src: &SourceCtx, subject: &Subject, seg: &Segment, importance: f32, ext: &ExtractionOut, ep_id: &str) -> Result<EpisodeCommit> {
        let mut out = EpisodeCommit { episode_id: ep_id.to_string(), ..Default::default() };
        if self.store.get_episode(ep_id).await?.is_some() {
            return Ok(out);
        }
        let now = now_rfc3339();
        let observed = seg
            .date
            .clone()
            .or_else(|| src.occurred_from.clone())
            .unwrap_or_else(|| src.created_at.chars().take(10).collect());
        let observed_date = parse_date(&observed).unwrap_or_else(|| chrono::Utc::now().date_naive());
        let anchors = &subject.anchors;

        // Waktu episode.
        let mut time = ext.time.to_time_ref();
        let mut rt = time.resolve(anchors, observed_date);
        if !rt.is_known() && time == TimeRef::Unknown {
            let fallback_date = seg
                .date
                .as_deref()
                .and_then(parse_date)
                .or_else(|| src.occurred_from.as_deref().and_then(parse_date))
                .or_else(|| src.kind.defaults_to_observed_date().then_some(observed_date));
            if (!src.kind.is_biographic() || src.kind.defaults_to_observed_date() || src.occurred_from.is_some())
                && let Some(d) = fallback_date
            {
                time = TimeRef::Exact { date: d };
                rt = time.resolve(anchors, observed_date);
            }
        }
        let life_phase = rt
            .start
            .and_then(|s| anchors.age_at(s))
            .map(|a| standard_phase_for_age(a).to_string())
            .or_else(|| ext.life_phase.clone().filter(|p| !p.trim().is_empty()));

        // Merge: deteksi penceritaan ulang terhadap keadaan terbaru.
        let emb = match self.embedder.embed_one(&seg.text).await {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!(error = %e, "embedding episode gagal; disimpan tanpa vektor");
                None
            }
        };
        let hits = match &emb {
            Some(v) => self.store.vector_index().search("episode", &subject.id, v, 1, "status = 'active'").await?,
            None => vec![],
        };
        let retelling_of = hits.first().filter(|h| h.similarity >= self.cfg.pipeline.merge_similarity && h.id != ep_id).map(|h| h.id.clone());
        out.retelling = retelling_of.is_some();
        let conf_factor = if out.retelling { self.cfg.pipeline.retelling_confidence_factor } else { 1.0 };

        // Resolve entitas.
        let mut res = Resolver::new(self, subject).await?;
        let kind_of = |name: &str| -> EntityKind {
            ext.entities
                .iter()
                .find(|e| normalize(&e.name) == normalize(name) || e.aliases.iter().any(|a| normalize(a) == normalize(name)))
                .map(|e| EntityKind::parse_with_name(&e.kind, &e.name))
                .or_else(|| EntityKind::infer_from_name(name))
                .unwrap_or(EntityKind::Person)
        };
        for e in &ext.entities {
            res.resolve(&e.name, EntityKind::parse_with_name(&e.kind, &e.name), &e.aliases, &e.description, e.is_self, ep_id).await?;
        }

        // Kekerabatan.
        let mut kin_out: Vec<Kin> = Vec::new();
        for k in &ext.kin {
            if !quote_supported(&k.quote, &seg.text) {
                out.rejected += 1;
                continue;
            }
            let Some(term) = normalize_kin_term(&k.term) else { continue };
            let person = res.resolve(&k.person, EntityKind::Person, &[], "", false, ep_id).await?;
            let of = match k.of.as_deref().map(str::trim) {
                Some(o) if !o.is_empty() => res.resolve(o, EntityKind::Person, &[], "", false, ep_id).await?,
                _ => res.self_id().to_string(),
            };
            if person == of {
                continue;
            }
            if let (Some(g), Some(t)) = (term.gender, res.touched.get_mut(&person))
                && t.entity.gender.is_none() {
                    t.entity.gender = Some(g);
                    t.changed = true;
                }
            let rel = serde_json::to_value(term.relation).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let kid = det_id(&["kin", &person, &of, &rel]);
            let ev = Evidence { episode_id: ep_id.to_string(), quote: k.quote.clone() };
            let mut kin = match kin_out.iter().position(|x| x.id == kid) {
                Some(i) => kin_out.remove(i),
                None => match self.store.get::<Kin>("kin", &kid).await? {
                    Some(existing) => existing,
                    None => Kin {
                        id: kid.clone(),
                        subject: subject.id.clone(),
                        from: person.clone(),
                        to: of.clone(),
                        relation: term.relation,
                        gender: term.gender,
                        older: term.older,
                        term: k.term.clone(),
                        lineage: None,
                        inferred: false,
                        confidence: k.confidence.unwrap_or(0.8).clamp(0.0, 1.0),
                        evidence: vec![],
                        created_at: now.clone(),
                    },
                },
            };
            if !kin.evidence.iter().any(|e| e.episode_id == ev.episode_id) {
                kin.evidence.push(ev);
            }
            res.note_kin(kin.clone());
            kin_out.push(kin);
        }

        // Fakta + rekonsiliasi.
        let mut actions: Vec<Action> = Vec::new();
        let mut overlay: HashMap<(String, String), Vec<Fact>> = HashMap::new();
        for (fi, fo) in ext.facts.iter().enumerate() {
            if fo.quote.trim().is_empty() || !quote_supported(&fo.quote, &seg.text) {
                out.rejected += 1;
                continue;
            }
            let ent = res.resolve(&fo.subject, kind_of(&fo.subject), &[], "", false, ep_id).await?;
            let obj = if fo.object_is_entity {
                let k = match kind_of(&fo.object) {
                    EntityKind::Person if fo.category.contains("politik") => EntityKind::Org,
                    k => k,
                };
                Some(res.resolve(&fo.object, k, &[], "", false, ep_id).await?)
            } else {
                None
            };
            let kind = if fo.kind.trim().eq_ignore_ascii_case("preference") { FactKind::Preference } else { FactKind::Fact };
            let mut ftime = fo.time.to_time_ref();
            if ftime == TimeRef::Unknown {
                ftime = time.clone();
            }
            let frt = ftime.resolve(anchors, observed_date);
            // Pernyataan preferensi pada satu titik waktu ("umur 20 saya dukung A")
            // berarti "sejak saat itu"; akhirnya ditentukan fakta yang menggantikannya.
            let point_preference = kind == FactKind::Preference && !matches!(ftime, TimeRef::Range { .. } | TimeRef::Past);
            let (valid_to, ongoing) = if point_preference || frt.ongoing { (None, frt.start.is_some()) } else { (frt.end.map(iso), false) };
            let confidence = (fo.confidence.unwrap_or(0.7).clamp(0.0, 1.0) * conf_factor).clamp(0.0, 1.0);
            let quarantined = confidence < self.cfg.pipeline.min_fact_confidence;
            let fact = Fact {
                id: det_id(&[ep_id, "fact", &fi.to_string()]),
                subject: subject.id.clone(),
                entity_id: ent.clone(),
                predicate: fo.predicate.trim().to_string(),
                category: category_key(&fo.category, &fo.predicate),
                object_entity: obj,
                object_text: fo.object.trim().to_string(),
                kind,
                exclusive: fo.exclusive,
                time: ftime,
                valid_from: frt.start.map(iso),
                valid_to,
                ongoing,
                time_confidence: frt.confidence,
                observed_at: observed.clone(),
                confidence,
                evidence: vec![Evidence { episode_id: ep_id.to_string(), quote: fo.quote.clone() }],
                status: if quarantined { FactStatus::Quarantined } else { FactStatus::Active },
                superseded_by: None,
                supersedes: None,
                created_at: now.clone(),
            };
            if quarantined {
                out.quarantined += 1;
                actions.push(Action::Review(Box::new(ReviewItem {
                    id: det_id(&["quarantine", &fact.id]),
                    subject: subject.id.clone(),
                    kind: ReviewKind::Quarantine,
                    summary: format!("Fakta ragu ({:.2}): {} {} {}", confidence, fo.subject, fact.predicate, fact.object_text),
                    targets: vec![fact.id.clone()],
                    payload: json!({"fact": fact.id, "quote": fo.quote}),
                    status: ReviewStatus::Open,
                    note: None,
                    created_at: now.clone(),
                    resolved_at: None,
                })));
                actions.push(Action::Insert(Box::new(fact)));
                continue;
            }
            let key = (fact.entity_id.clone(), fact.category.clone());
            if !overlay.contains_key(&key) {
                let existing = self.store.facts_in_category(&key.0, &key.1).await?;
                overlay.insert(key.clone(), existing);
            }
            let list = overlay.get_mut(&key).expect("baru diisi");
            let acts = reconcile(fact, list, &now);
            out.facts += acts.iter().filter(|a| matches!(a, Action::Insert(_))).count() as u32;
            actions.extend(acts);
        }

        // Keputusan, kutipan, sinyal sifat: hanya kutipan yang ada di teks.
        let decisions: Vec<Decision> = ext
            .decisions
            .iter()
            .cloned()
            .map(|mut d| {
                if d.quote.as_deref().is_some_and(|q| !quote_supported(q, &seg.text)) {
                    d.quote = None;
                }
                d
            })
            .filter(|d| !d.situation.trim().is_empty() && !d.action.trim().is_empty())
            .collect();
        let quotes: Vec<String> = ext.quotes.iter().filter(|q| quote_supported(q, &seg.text)).cloned().collect();
        let trait_signals: Vec<TraitSignal> = ext
            .trait_signals
            .iter()
            .filter(|t| !t.label.trim().is_empty() && t.quote.as_deref().is_some_and(|q| quote_supported(q, &seg.text)))
            .cloned()
            .collect();

        res.record_mentions(ep_id);
        let self_id = res.self_id().to_string();
        let mut entity_ids: Vec<String> = res.touched.keys().filter(|k| **k != self_id).cloned().collect();
        entity_ids.sort();

        let episode = Episode {
            id: ep_id.to_string(),
            subject: subject.id.clone(),
            source_id: src.id.clone(),
            text: seg.text.clone(),
            summary: ext.summary.trim().to_string(),
            title: if ext.title.trim().is_empty() { truncate_chars(&seg.topic, 80) } else { ext.title.trim().to_string() },
            time,
            time_start: rt.start.map(iso),
            time_end: rt.end.map(iso),
            time_ongoing: rt.ongoing,
            time_confidence: rt.confidence,
            importance,
            emotions: ext.emotions.iter().map(|e| Emotion { label: e.label.clone(), intensity: e.intensity.clamp(0.0, 1.0) }).collect(),
            languages: memory_core::text::detect_languages(&seg.text),
            life_phase,
            project: src.project.clone(),
            speaker: seg.speaker.clone(),
            status: if out.retelling { EpisodeStatus::Retelling } else { EpisodeStatus::Active },
            retelling_of: retelling_of.clone(),
            retell_count: 0,
            decisions,
            quotes,
            trait_signals,
            entity_ids,
            obs_type: ext.obs_type.clone().map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()),
            concepts: ext.concepts.iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).take(12).collect(),
            files_read: ext.files_read.iter().map(|f| f.trim().replace('\\', "/")).filter(|f| !f.is_empty()).take(40).collect(),
            files_modified: ext.files_modified.iter().map(|f| f.trim().replace('\\', "/")).filter(|f| !f.is_empty()).take(40).collect(),
            session_id: src.session_id.clone(),
            prompt_number: src.prompt_number,
            observed_at: observed,
            created_at: now.clone(),
        };

        // Pelajaran proyek: dedup per (subjek, proyek, judul ternormalisasi); kemunculan bertambah.
        let mut lessons_out: Vec<Lesson> = Vec::new();
        for lo in ext.lessons.iter().filter(|l| !l.title.trim().is_empty()) {
            if !lo.quote.trim().is_empty() && !quote_supported(&lo.quote, &seg.text) {
                continue;
            }
            let key = Lesson::title_key_of(&lo.title);
            if key.is_empty() {
                continue;
            }
            let ev = Evidence { episode_id: ep_id.to_string(), quote: lo.quote.clone() };
            let mut lesson = match self.store.lesson_by_key(&subject.id, src.project.as_deref(), &key).await? {
                Some(mut l) => {
                    if !l.evidence.iter().any(|e| e.episode_id == ev.episode_id) {
                        l.occurrences += 1;
                        l.evidence.push(ev);
                    }
                    if l.detail.trim().is_empty() {
                        l.detail = lo.detail.trim().to_string();
                    }
                    l
                }
                None => Lesson {
                    id: det_id(&["lesson", &subject.id, src.project.as_deref().unwrap_or(""), &key]),
                    subject: subject.id.clone(),
                    project: src.project.clone(),
                    kind: LessonKind::parse(&lo.kind),
                    title: lo.title.trim().to_string(),
                    title_key: key,
                    detail: lo.detail.trim().to_string(),
                    occurrences: 1,
                    evidence: vec![ev],
                    status: "active".into(),
                    promoted_to: None,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                },
            };
            lesson.updated_at = now.clone();
            lessons_out.push(lesson);
        }
        out.lessons = lessons_out.len() as u32;

        // Commit atomik.
        let mut tx = self.store.tx();
        tx.upsert("episode", ep_id, &episode, emb.as_deref())?;
        for t in res.touched.values() {
            if t.is_new || t.changed {
                let e_emb = self.embedder.embed_one(&entity_embed_text(&t.entity)).await.ok();
                tx.upsert("entity", &t.entity.id, &entity_doc(&t.entity), e_emb.as_deref())?;
            }
        }
        for a in &actions {
            match a {
                Action::Insert(f) | Action::Update(f) => {
                    tx.upsert("fact", &f.id, f.as_ref(), None)?;
                }
                Action::Review(r) => {
                    tx.upsert("review_item", &r.id, r.as_ref(), None)?;
                }
            }
        }
        for k in &kin_out {
            tx.upsert("kin", &k.id, k, None)?;
        }
        for l in &lessons_out {
            tx.upsert("lesson", &l.id, l, None)?;
        }
        for r in &res.reviews {
            tx.upsert("review_item", &r.id, r, None)?;
        }
        if let Some(cid) = &retelling_of {
            tx.raw(
                "UPDATE type::record('episode', $retell_cid) SET retell_count += 1, importance = math::min([importance + 0.05, 1]) RETURN NONE;",
                memory_store::binds! {"retell_cid" => cid},
            );
        }
        tx.commit().await?;
        out.created = true;
        Ok(out)
    }

    /// Angkat stream item menjadi episode (setelah review "approve" atau
    /// konsolidasi menemukan item itu ternyata penting).
    pub async fn promote_stream_item(&self, item_id: &str) -> Result<Option<String>> {
        let Some(item) = self.store.get::<StreamItem>("stream_item", item_id).await? else { return Ok(None) };
        if item.promoted_to.is_some() {
            return Ok(item.promoted_to);
        }
        let source = self.store.get_source(&item.source_id).await?;
        let subject = self.ensure_subject(&item.subject, None).await?;
        let ctx = match &source {
            Some(s) => SourceCtx::from(s),
            None => SourceCtx { id: item.source_id.clone(), kind: SourceKind::Lifelog, occurred_from: None, project: None, created_at: item.created_at.clone(), session_id: None, prompt_number: None },
        };
        let seg = Segment { start: 0, end: 0, text: item.text.clone(), topic: String::new(), about_subject: true, speaker: item.speaker.clone(), significance: None, date: item.occurred_at.clone() };
        let observed = seg.date.clone().unwrap_or_else(|| item.created_at.chars().take(10).collect());
        let known = self.known_entities_prompt(&subject.id).await?;
        let ext = match self.extract(&subject.name, &observed, subject.anchors.birth.map(iso).as_deref(), &known, &seg.text).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "ekstraksi promosi stream item gagal; memakai fallback");
                let clean = crate::segment::strip_chat_prefix(&seg.text);
                ExtractionOut {
                    title: truncate_chars(clean.lines().next().unwrap_or(&clean), 72),
                    summary: truncate_chars(&clean, 300),
                    ..Default::default()
                }
            }
        };
        let lock = self.subject_lock(&subject.id).await;
        let _g = lock.lock().await;
        let subject = self.store.get_subject(&subject.id).await?.unwrap_or(subject);
        let ep_id = det_id(&[&item.id, "ep"]);
        self.commit_episode(&ctx, &subject, &seg, item.importance.max(self.cfg.score.promote_threshold), &ext, &ep_id).await?;
        self.store.patch("stream_item", &item.id, json!({"promoted_to": ep_id})).await?;
        Ok(Some(ep_id))
    }
}
