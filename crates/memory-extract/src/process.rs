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
    pub created_at: String,
}

impl From<&Source> for SourceCtx {
    fn from(s: &Source) -> Self {
        Self { id: s.id.clone(), kind: s.kind, occurred_from: s.occurred_from.clone(), created_at: s.created_at.clone() }
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
        Ok(stats)
    }

    /// Fase A: segment, score, extract.
    async fn stage(&self, source: &Source, subject: &Subject, job_id: Option<&str>) -> Result<Staging> {
        let text = self.vault.read_rel(&source.path)?;
        self.progress(job_id, "segment", 0, 1).await;
        let examples = self.few_shot_examples(&subject.id).await;
        let segments = self.segment(&subject.name, &text, &examples).await;
        if segments.is_empty() {
            return Ok(Staging::default());
        }
        self.progress(job_id, "score", 0, segments.len()).await;
        let texts: Vec<String> = segments.iter().map(|s| s.text.clone()).collect();
        let embs = self.embedder.embed(&texts).await?;
        let alias_keys = self.known_alias_keys(&subject.id).await?;
        let index = self.store.vector_index();
        let mut items = Vec::with_capacity(segments.len());
        for (seg, emb) in segments.into_iter().zip(embs) {
            let hits = index.search("episode", &subject.id, &emb, 1, "status = 'active'").await?;
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
            items[i].extraction = Some(r?);
        }
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
        if !rt.is_known() && !src.kind.is_biographic()
            && let Some(d) = seg.date.as_deref().and_then(parse_date) {
                time = TimeRef::Exact { date: d };
                rt = time.resolve(anchors, observed_date);
            }
        let life_phase = rt
            .start
            .and_then(|s| anchors.age_at(s))
            .map(|a| standard_phase_for_age(a).to_string())
            .or_else(|| ext.life_phase.clone().filter(|p| !p.trim().is_empty()));

        // Merge: deteksi penceritaan ulang terhadap keadaan terbaru.
        let emb = self.embedder.embed_one(&seg.text).await?;
        let hits = self.store.vector_index().search("episode", &subject.id, &emb, 1, "status = 'active'").await?;
        let retelling_of = hits.first().filter(|h| h.similarity >= self.cfg.pipeline.merge_similarity && h.id != ep_id).map(|h| h.id.clone());
        out.retelling = retelling_of.is_some();
        let conf_factor = if out.retelling { self.cfg.pipeline.retelling_confidence_factor } else { 1.0 };

        // Resolve entitas.
        let mut res = Resolver::new(self, subject).await?;
        let kind_of = |name: &str| -> EntityKind {
            ext.entities
                .iter()
                .find(|e| normalize(&e.name) == normalize(name) || e.aliases.iter().any(|a| normalize(a) == normalize(name)))
                .map(|e| EntityKind::parse(&e.kind))
                .unwrap_or(EntityKind::Person)
        };
        for e in &ext.entities {
            res.resolve(&e.name, EntityKind::parse(&e.kind), &e.aliases, &e.description, e.is_self, ep_id).await?;
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
            speaker: seg.speaker.clone(),
            status: if out.retelling { EpisodeStatus::Retelling } else { EpisodeStatus::Active },
            retelling_of: retelling_of.clone(),
            retell_count: 0,
            decisions,
            quotes,
            trait_signals,
            entity_ids,
            observed_at: observed,
            created_at: now.clone(),
        };

        // Commit atomik.
        let mut tx = self.store.tx();
        tx.upsert("episode", ep_id, &episode, Some(&emb))?;
        for t in res.touched.values() {
            if t.is_new || t.changed {
                let e_emb = self.embedder.embed_one(&entity_embed_text(&t.entity)).await?;
                tx.upsert("entity", &t.entity.id, &entity_doc(&t.entity), Some(&e_emb))?;
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
            None => SourceCtx { id: item.source_id.clone(), kind: SourceKind::Lifelog, occurred_from: None, created_at: item.created_at.clone() },
        };
        let seg = Segment { start: 0, end: 0, text: item.text.clone(), topic: String::new(), about_subject: true, speaker: item.speaker.clone(), significance: None, date: item.occurred_at.clone() };
        let observed = seg.date.clone().unwrap_or_else(|| item.created_at.chars().take(10).collect());
        let known = self.known_entities_prompt(&subject.id).await?;
        let ext = self.extract(&subject.name, &observed, subject.anchors.birth.map(iso).as_deref(), &known, &seg.text).await?;
        let lock = self.subject_lock(&subject.id).await;
        let _g = lock.lock().await;
        let subject = self.store.get_subject(&subject.id).await?.unwrap_or(subject);
        let ep_id = det_id(&[&item.id, "ep"]);
        self.commit_episode(&ctx, &subject, &seg, item.importance.max(self.cfg.score.promote_threshold), &ext, &ep_id).await?;
        self.store.patch("stream_item", &item.id, json!({"promoted_to": ep_id})).await?;
        Ok(Some(ep_id))
    }
}
