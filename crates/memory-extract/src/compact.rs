//! Pemadatan memori non-destruktif (*semantic compaction*):
//! - **Episode Roll-up**: menggabungkan beberapa episode mikro yang bertopik
//!   sama (terutama dari `AgentConversation`/lifelog) menjadi satu episode
//!   kanonik yang padat, lalu menandai episode anggotanya sebagai
//!   [`EpisodeStatus::Retelling`] agar `recall` tetap ringkas tanpa menghapus
//!   teks asli maupun memutus rantai [`Evidence`].
//! - **Fact Compaction**: menggabungkan fakta aktif yang duplikat/tumpang tindih
//!   (bukti digabung ke fakta kanonik), menutup fakta lama yang sudah lewat
//!   (`valid_to`), dan membuang log status teknis sesaat (`Rejected`).
//! - Dapat dijalankan otomatis lewat LLM pipeline (`compact`) atau dikurasi
//!   langsung oleh agent eksternal seperti `agy` lewat MCP (`compact_candidates`
//!   + `compact_apply`).

use std::collections::{BTreeMap, HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use memory_core::ids::now_rfc3339;
use memory_core::model::*;
use memory_core::text::truncate_chars;
use memory_llm::{ModelRole, complete_json};
use memory_store::EpisodeFilter;

use crate::distill::greedy_clusters;
use crate::llm_types::{
    CompactEpisodesOut, CompactFactsOut, EpisodeRollupOut, FactClosureOut, FactMergeOut,
};
use crate::review::Decision as ReviewDecision;
use crate::{Pipeline, Result, det_id, prompts, today_iso};

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CompactReport {
    pub episode_clusters: u32,
    pub episodes_compacted: u32,
    pub canonical_episodes_created: u32,
    pub facts_merged: u32,
    pub facts_closed: u32,
    pub facts_rejected: u32,
    pub reviews_resolved: u32,
    pub phases_updated: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EpisodeCandidate {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub text: String,
    pub time: String,
    pub time_start: Option<String>,
    pub source_title: String,
    pub source_kind: SourceKind,
    pub importance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EpisodeClusterCandidate {
    pub reason: String,
    pub episodes: Vec<EpisodeCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FactCandidate {
    pub id: String,
    pub predicate: String,
    pub object_text: String,
    pub category: String,
    pub kind: FactKind,
    pub valid_from: Option<String>,
    pub observed_at: String,
    pub confidence: f32,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EntityFactCandidate {
    pub entity_id: String,
    pub entity_name: String,
    pub facts: Vec<FactCandidate>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CompactCandidates {
    pub subject: String,
    pub episode_clusters: Vec<EpisodeClusterCandidate>,
    pub entity_fact_groups: Vec<EntityFactCandidate>,
    pub open_reviews: Vec<ReviewItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReviewResolution {
    pub review_id: String,
    pub decision: ReviewDecision,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CompactPlan {
    #[serde(default)]
    pub episode_rollups: Vec<EpisodeRollupOut>,
    #[serde(default)]
    pub fact_merges: Vec<FactMergeOut>,
    #[serde(default)]
    pub fact_closures: Vec<FactClosureOut>,
    #[serde(default)]
    pub fact_rejections: Vec<String>,
    #[serde(default)]
    pub review_decisions: Vec<ReviewResolution>,
}

impl Pipeline {
    /// Kumpulkan kandidat episode yang mirip/serumpun, fakta aktif yang berpotensi
    /// redundan per entitas, serta antrean review terbuka.
    pub async fn compact_candidates(&self, subject: &str) -> Result<CompactCandidates> {
        let _ = self.sanitize_technical_entities(subject).await;
        let eps = self
            .store
            .list_episodes(
                subject,
                &EpisodeFilter {
                    status: Some(EpisodeStatus::Active),
                    limit: 5000,
                    ..Default::default()
                },
            )
            .await?;
        let sources: HashMap<String, Source> = self
            .store
            .list_sources(subject, None, 5000, 0)
            .await?
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect();

        let min_eps = self.cfg.pipeline.compact_min_episodes.max(2);
        let mut episode_clusters: Vec<EpisodeClusterCandidate> = Vec::new();
        let mut assigned: HashSet<String> = HashSet::new();

        // 1. Klasterisasi berdasar kemiripan vektor embedding.
        if eps.len() >= min_eps {
            let mut vecs: Vec<Vec<f32>> = Vec::with_capacity(eps.len());
            let mut valid_eps: Vec<&Episode> = Vec::with_capacity(eps.len());
            for ep in &eps {
                let emb = match self.store.embedding_of("episode", &ep.id).await? {
                    Some(v) if !v.is_empty() => Some(v),
                    _ => self.embedder.embed_one(&ep.text).await.ok(),
                };
                if let Some(v) = emb {
                    vecs.push(v);
                    valid_eps.push(ep);
                }
            }
            for members in greedy_clusters(&vecs, self.cfg.pipeline.compact_similarity) {
                if members.len() < min_eps {
                    continue;
                }
                for chunk in members.chunks(12) {
                    if chunk.len() < min_eps {
                        continue;
                    }
                    let mut c_eps = Vec::with_capacity(chunk.len());
                    for &idx in chunk {
                        let e = valid_eps[idx];
                        assigned.insert(e.id.clone());
                        c_eps.push(to_ep_candidate(e, &sources));
                    }
                    episode_clusters.push(EpisodeClusterCandidate {
                        reason: format!(
                            "kemiripan semantik >= {:.2}",
                            self.cfg.pipeline.compact_similarity
                        ),
                        episodes: c_eps,
                    });
                }
            }
        }

        // 2. Klasterisasi tambahan untuk percakapan agent/chat dari sumber/proyek yang sama.
        let mut by_source_title: BTreeMap<String, Vec<&Episode>> = BTreeMap::new();
        for ep in &eps {
            if assigned.contains(&ep.id) {
                continue;
            }
            if let Some(src) = sources.get(&ep.source_id)
                && matches!(
                    src.kind,
                    SourceKind::AgentConversation | SourceKind::Chat | SourceKind::Lifelog
                )
                && !src.title.trim().is_empty()
            {
                by_source_title
                    .entry(src.title.clone())
                    .or_default()
                    .push(ep);
            }
        }
        for (title, group) in by_source_title {
            if group.len() < min_eps {
                continue;
            }
            for chunk in group.chunks(12) {
                if chunk.len() < min_eps {
                    continue;
                }
                let c_eps = chunk.iter().map(|e| to_ep_candidate(e, &sources)).collect();
                episode_clusters.push(EpisodeClusterCandidate {
                    reason: format!("sumber/topik sama: {title}"),
                    episodes: c_eps,
                });
            }
        }

        // 3. Kelompokkan fakta aktif yang masih berlaku (`is_current`) per entitas.
        let active_facts = self
            .store
            .list_facts(subject, Some(FactStatus::Active), 5000)
            .await?;
        let entities: HashMap<String, String> = self
            .store
            .list_entities(subject, None, 2000, 0)
            .await?
            .into_iter()
            .map(|e| (e.id, e.name))
            .collect();
        let mut facts_by_entity: BTreeMap<String, Vec<Fact>> = BTreeMap::new();
        for f in active_facts.into_iter().filter(Fact::is_current) {
            facts_by_entity
                .entry(f.entity_id.clone())
                .or_default()
                .push(f);
        }
        let mut entity_fact_groups = Vec::new();
        for (entity_id, mut list) in facts_by_entity {
            if list.len() < 2 {
                continue;
            }
            list.sort_by(|a, b| {
                a.category
                    .cmp(&b.category)
                    .then_with(|| a.predicate.cmp(&b.predicate))
                    .then_with(|| a.created_at.cmp(&b.created_at))
            });
            let entity_name = entities
                .get(&entity_id)
                .cloned()
                .unwrap_or_else(|| entity_id.clone());
            let facts = list
                .into_iter()
                .map(|f| FactCandidate {
                    id: f.id,
                    predicate: f.predicate,
                    object_text: f.object_text,
                    category: f.category,
                    kind: f.kind,
                    valid_from: f.valid_from,
                    observed_at: f.observed_at,
                    confidence: f.confidence,
                    evidence_count: f.evidence.len(),
                })
                .collect();
            entity_fact_groups.push(EntityFactCandidate {
                entity_id,
                entity_name,
                facts,
            });
        }

        let open_reviews = self
            .store
            .list_reviews(subject, ReviewStatus::Open, None, 200)
            .await?;
        Ok(CompactCandidates {
            subject: subject.to_string(),
            episode_clusters,
            entity_fact_groups,
            open_reviews,
        })
    }

    /// Terapkan rencana pemadatan memori secara non-destruktif dan idempoten.
    pub async fn compact_apply(&self, subject: &str, plan: CompactPlan) -> Result<CompactReport> {
        let mut report = CompactReport::default();

        for rollup in plan.episode_rollups {
            let n = self
                .rollup_episodes(
                    subject,
                    &rollup.episode_ids,
                    &rollup.title,
                    &rollup.summary,
                    &rollup.text,
                )
                .await?;
            if n > 0 {
                report.canonical_episodes_created += 1;
                report.episodes_compacted += n;
            }
        }

        {
            let lock = self.subject_lock(subject).await;
            let _g = lock.lock().await;

            let mut touched_facts: HashSet<String> = HashSet::new();

            // 1. Gabung fakta duplikat ke `keep_id`.
            for m in plan.fact_merges {
                if m.absorb_ids.is_empty() || touched_facts.contains(&m.keep_id) {
                    continue;
                }
                let Some(mut keep) = self.store.get_fact(&m.keep_id).await? else {
                    continue;
                };
                if keep.subject != subject || keep.status == FactStatus::Rejected {
                    continue;
                }
                let mut absorbed_any = false;
                for aid in &m.absorb_ids {
                    if aid == &m.keep_id || touched_facts.contains(aid) {
                        continue;
                    }
                    let Some(mut abs) = self.store.get_fact(aid).await? else {
                        continue;
                    };
                    if abs.subject != subject || abs.status == FactStatus::Rejected {
                        continue;
                    }
                    for ev in &abs.evidence {
                        if !keep
                            .evidence
                            .iter()
                            .any(|x| x.episode_id == ev.episode_id && x.quote == ev.quote)
                        {
                            keep.evidence.push(ev.clone());
                        }
                    }
                    keep.confidence = keep.confidence.max(abs.confidence);
                    if let Some(af) = &abs.valid_from
                        && keep.valid_from.as_ref().is_none_or(|kf| af < kf)
                    {
                        keep.valid_from = Some(af.clone());
                    }
                    abs.status = FactStatus::Rejected;
                    abs.superseded_by = Some(keep.id.clone());
                    abs.ongoing = false;
                    self.store.put_fact(&abs).await?;
                    touched_facts.insert(abs.id);
                    report.facts_merged += 1;
                    absorbed_any = true;
                }
                if absorbed_any {
                    if let Some(pred) = m
                        .predicate
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        keep.predicate = pred.to_string();
                    }
                    if let Some(obj) = m
                        .object_text
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        keep.object_text = obj.to_string();
                    }
                    self.store.put_fact(&keep).await?;
                    touched_facts.insert(keep.id);
                }
            }

            // 2. Tutup fakta yang sudah berakhir masa berlakunya (menjadi riwayat 'dulu').
            for c in plan.fact_closures {
                if touched_facts.contains(&c.fact_id) {
                    continue;
                }
                let Some(mut f) = self.store.get_fact(&c.fact_id).await? else {
                    continue;
                };
                if f.subject != subject || f.status != FactStatus::Active || f.valid_to.is_some() {
                    continue;
                }
                let end = c
                    .valid_to
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| {
                        (!f.observed_at.is_empty())
                            .then(|| f.observed_at.chars().take(10).collect())
                    })
                    .unwrap_or_else(today_iso);
                f.valid_to = Some(end);
                f.ongoing = false;
                if let Some(by) = c.superseded_by.filter(|s| !s.trim().is_empty()) {
                    f.superseded_by = Some(by);
                }
                self.store.put_fact(&f).await?;
                touched_facts.insert(f.id);
                report.facts_closed += 1;
            }

            // 3. Tolak fakta sesaat/noise operasional.
            for fid in plan.fact_rejections {
                if touched_facts.contains(&fid) {
                    continue;
                }
                let Some(mut f) = self.store.get_fact(&fid).await? else {
                    continue;
                };
                if f.subject != subject || f.status == FactStatus::Rejected {
                    continue;
                }
                f.status = FactStatus::Rejected;
                f.ongoing = false;
                self.store.put_fact(&f).await?;
                touched_facts.insert(f.id);
                report.facts_rejected += 1;
            }
        }

        // 4. Selesaikan review item bila ada.
        for rev in plan.review_decisions {
            if self
                .resolve_review(&rev.review_id, rev.decision, rev.note)
                .await
                .is_ok()
            {
                report.reviews_resolved += 1;
            }
        }

        if report.canonical_episodes_created > 0 {
            report.phases_updated = self.update_life_phases(subject).await?;
        }
        Ok(report)
    }

    /// Gabungkan beberapa episode aktif menjadi 1 episode kanonik, dan tandai
    /// episode anggota sebagai `Retelling` dari episode kanonik tersebut.
    pub async fn rollup_episodes(
        &self,
        subject: &str,
        episode_ids: &[String],
        title: &str,
        summary: &str,
        text: &str,
    ) -> Result<u32> {
        if text.trim().is_empty() {
            return Ok(0);
        }
        let lock = self.subject_lock(subject).await;
        let _g = lock.lock().await;

        let mut uniq_ids: Vec<String> = Vec::new();
        for id in episode_ids {
            let clean = id.trim().trim_start_matches("ep:").to_string();
            if !clean.is_empty() && !uniq_ids.contains(&clean) {
                uniq_ids.push(clean);
            }
        }
        if uniq_ids.len() < 2 {
            return Ok(0);
        }

        let loaded = self.store.episodes_by_ids(&uniq_ids).await?;
        let mut members: Vec<Episode> = loaded
            .into_iter()
            .filter(|e| e.subject == subject && e.status == EpisodeStatus::Active)
            .collect();
        if members.len() < 2 {
            return Ok(0);
        }
        members.sort_by(|a, b| {
            match (&a.time_start, &b.time_start) {
                (Some(ta), Some(tb)) => ta.cmp(tb),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.id.cmp(&b.id))
        });

        let mut sorted_ids: Vec<&str> = members.iter().map(|e| e.id.as_str()).collect();
        sorted_ids.sort();
        let mut key_parts = vec![subject, "compact_ep"];
        key_parts.extend(sorted_ids);
        let canon_id = det_id(&key_parts);

        let now = now_rfc3339();
        let first = &members[0];
        let parse_iso_date = |s: &str| -> Option<chrono::NaiveDate> {
            chrono::NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
        };
        let obs_dates: Vec<String> = members
            .iter()
            .filter_map(|e| {
                parse_iso_date(&e.observed_at).map(|d| d.format("%Y-%m-%d").to_string())
            })
            .collect();
        let time_start = members
            .iter()
            .filter_map(|e| e.time_start.clone())
            .min()
            .or_else(|| obs_dates.iter().min().cloned());
        let time_end = members
            .iter()
            .filter_map(|e| e.time_end.clone())
            .max()
            .or_else(|| obs_dates.iter().max().cloned())
            .or_else(|| time_start.clone());
        let time_ongoing = members.iter().any(|e| e.time_ongoing);
        let time_confidence = members
            .iter()
            .map(|e| e.time_confidence)
            .fold(0.0_f32, f32::max)
            .max(if time_start.is_some() { 0.8 } else { 0.0 });
        let canon_time = match members
            .iter()
            .find(|e| e.time != memory_core::TimeRef::Unknown)
        {
            Some(e) => e.time.clone(),
            None => match (
                time_start.as_deref().and_then(parse_iso_date),
                time_end.as_deref().and_then(parse_iso_date),
            ) {
                (Some(s), Some(e)) if s == e && !time_ongoing => {
                    memory_core::TimeRef::Exact { date: s }
                }
                (Some(s), e) => memory_core::TimeRef::Range {
                    start: Some(s),
                    end: e,
                    ongoing: time_ongoing,
                },
                _ => first.time.clone(),
            },
        };
        let importance =
            (members.iter().map(|e| e.importance).fold(0.0_f32, f32::max) + 0.05).min(1.0);
        let life_phase = members
            .iter()
            .find_map(|e| e.life_phase.clone().filter(|p| !p.trim().is_empty()));
        let project = members
            .iter()
            .find_map(|e| e.project.clone().filter(|p| !p.trim().is_empty()));
        let observed_at = members
            .iter()
            .filter(|e| !e.observed_at.is_empty()).map(|e| e.observed_at.clone())
            .max()
            .unwrap_or_else(today_iso);
        let retell_count: u32 = members
            .iter()
            .map(|e| e.retell_count + 1)
            .sum::<u32>()
            .saturating_sub(1);

        let mut emotions: Vec<Emotion> = Vec::new();
        let mut languages: Vec<String> = Vec::new();
        let mut decisions: Vec<Decision> = Vec::new();
        let mut quotes: Vec<String> = Vec::new();
        let mut trait_signals: Vec<TraitSignal> = Vec::new();
        let mut entity_ids: Vec<String> = Vec::new();

        for m in &members {
            for em in &m.emotions {
                if let Some(ex) = emotions.iter_mut().find(|x| x.label == em.label) {
                    ex.intensity = ex.intensity.max(em.intensity);
                } else {
                    emotions.push(em.clone());
                }
            }
            for lang in &m.languages {
                if !languages.contains(lang) {
                    languages.push(lang.clone());
                }
            }
            for d in &m.decisions {
                if !decisions
                    .iter()
                    .any(|x| x.situation == d.situation && x.action == d.action)
                {
                    decisions.push(d.clone());
                }
            }
            for q in &m.quotes {
                if !quotes.contains(q) {
                    quotes.push(q.clone());
                }
            }
            for ts in &m.trait_signals {
                if !trait_signals
                    .iter()
                    .any(|x| x.label == ts.label && x.polarity == ts.polarity)
                {
                    trait_signals.push(ts.clone());
                }
            }
            for eid in &m.entity_ids {
                if !entity_ids.contains(eid) {
                    entity_ids.push(eid.clone());
                }
            }
        }
        if languages.is_empty() {
            languages = memory_core::text::detect_languages(text);
        }
        entity_ids.sort();

        let clean_title = if title.trim().is_empty() {
            first.title.clone()
        } else {
            title.trim().to_string()
        };
        let clean_summary = if summary.trim().is_empty() {
            truncate_chars(text.trim(), 300)
        } else {
            summary.trim().to_string()
        };

        let canonical = Episode {
            id: canon_id.clone(),
            subject: subject.to_string(),
            source_id: first.source_id.clone(),
            text: text.trim().to_string(),
            summary: clean_summary,
            title: clean_title,
            time: canon_time,
            time_start,
            time_end,
            time_ongoing,
            time_confidence,
            importance,
            emotions,
            languages,
            life_phase,
            project,
            speaker: first.speaker.clone(),
            status: EpisodeStatus::Active,
            retelling_of: None,
            retell_count,
            decisions,
            quotes,
            trait_signals,
            entity_ids: entity_ids.clone(),
            obs_type: members.iter().find_map(|m| m.obs_type.clone()),
            concepts: {
                let mut c: Vec<String> = members.iter().flat_map(|m| m.concepts.iter().cloned()).collect();
                c.sort();
                c.dedup();
                c.truncate(12);
                c
            },
            files_read: {
                let mut f: Vec<String> = members.iter().flat_map(|m| m.files_read.iter().cloned()).collect();
                f.sort();
                f.dedup();
                f
            },
            files_modified: {
                let mut f: Vec<String> = members.iter().flat_map(|m| m.files_modified.iter().cloned()).collect();
                f.sort();
                f.dedup();
                f
            },
            session_id: None,
            prompt_number: None,
            observed_at,
            created_at: now,
        };

        let emb = self.embedder.embed_one(&canonical.text).await.ok();
        let member_ids: Vec<String> = members
            .iter()
            .filter(|m| m.id != canon_id)
            .map(|m| m.id.clone())
            .collect();

        let mut tx = self.store.tx();
        tx.upsert("episode", &canon_id, &canonical, emb.as_deref())?;
        for mut m in members {
            if m.id == canon_id {
                continue;
            }
            m.status = EpisodeStatus::Retelling;
            m.retelling_of = Some(canon_id.clone());
            tx.upsert("episode", &m.id, &m, None)?;
        }
        if !member_ids.is_empty() {
            tx.raw(
                "UPDATE episode SET retelling_of = $canon WHERE subject = $s AND retelling_of IN $mids RETURN NONE;",
                memory_store::binds! {"canon" => canon_id.clone(), "s" => subject, "mids" => member_ids.clone()},
            );
        }
        tx.commit().await?;

        Ok(member_ids.len() as u32)
    }

    /// Jalankan pemadatan memori otomatis memakai LLM untuk satu subjek.
    pub async fn compact(&self, subject: &str) -> Result<CompactReport> {
        let candidates = self.compact_candidates(subject).await?;
        let mut plan = CompactPlan::default();
        let mut cluster_count = 0u32;

        for cluster in &candidates.episode_clusters {
            cluster_count += 1;
            let allowed: HashSet<&str> = cluster.episodes.iter().map(|e| e.id.as_str()).collect();
            let listing: Vec<String> = cluster
                .episodes
                .iter()
                .map(|e| {
                    format!(
                        "[{}] ({} | {}) Judul: {}\nRingkasan: {}\nTeks: {}",
                        e.id,
                        e.time,
                        e.source_title,
                        e.title,
                        e.summary,
                        truncate_chars(&e.text, 800)
                    )
                })
                .collect();
            let user = format!(
                "SUBJEK: {subject}\nALASAN KLASTER: {}\n\nEPISODE:\n{}",
                cluster.reason,
                listing.join("\n\n")
            );
            match complete_json::<CompactEpisodesOut>(
                self.llm.as_ref(),
                ModelRole::Extract,
                "compact_episodes",
                prompts::COMPACT_EPISODES_SYSTEM,
                &user,
                self.cfg.llm.max_retries,
            )
            .await
            {
                Ok(out) => {
                    let mut used: HashSet<String> = HashSet::new();
                    for mut r in out.rollups {
                        r.episode_ids.retain(|id| {
                            let clean = id.trim().trim_start_matches("ep:");
                            allowed.contains(clean) && !used.contains(clean)
                        });
                        if r.episode_ids.len() >= 2 && !r.text.trim().is_empty() {
                            for id in &r.episode_ids {
                                used.insert(id.trim().trim_start_matches("ep:").to_string());
                            }
                            plan.episode_rollups.push(r);
                        }
                    }
                }
                Err(e) => tracing::warn!(error = %e, "rangkuman klaster episode gagal; dilewati"),
            }
        }

        for eg in &candidates.entity_fact_groups {
            let allowed: HashSet<&str> = eg.facts.iter().map(|f| f.id.as_str()).collect();
            let lines: Vec<String> = eg
                .facts
                .iter()
                .map(|f| {
                    format!(
                        "- [{}] ({}, sejak {}, dicatat {}) {} {} {}",
                        f.id,
                        if f.category.is_empty() {
                            "umum"
                        } else {
                            &f.category
                        },
                        f.valid_from.as_deref().unwrap_or("?"),
                        f.observed_at,
                        eg.entity_name,
                        f.predicate,
                        f.object_text
                    )
                })
                .collect();
            let user = format!(
                "ENTITAS: {}\nFAKTA AKTIF:\n{}",
                eg.entity_name,
                lines.join("\n")
            );
            match complete_json::<CompactFactsOut>(
                self.llm.as_ref(),
                ModelRole::Extract,
                "compact_facts",
                prompts::COMPACT_FACTS_SYSTEM,
                &user,
                self.cfg.llm.max_retries,
            )
            .await
            {
                Ok(out) => {
                    for mut m in out.merges {
                        let keep = m.keep_id.trim().to_string();
                        m.absorb_ids
                            .retain(|id| allowed.contains(id.trim()) && id.trim() != keep);
                        if allowed.contains(keep.as_str()) && !m.absorb_ids.is_empty() {
                            m.keep_id = keep;
                            plan.fact_merges.push(m);
                        }
                    }
                    for c in out.closures {
                        if allowed.contains(c.fact_id.trim()) {
                            plan.fact_closures.push(c);
                        }
                    }
                    for r in out.rejections {
                        let clean = r.trim().to_string();
                        if allowed.contains(clean.as_str()) {
                            plan.fact_rejections.push(clean);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(entity = %eg.entity_name, error = %e, "pemadatan fakta entitas gagal; dilewati")
                }
            }
        }

        let mut report = self.compact_apply(subject, plan).await?;
        report.episode_clusters = cluster_count;
        Ok(report)
    }
}

fn to_ep_candidate(e: &Episode, sources: &HashMap<String, Source>) -> EpisodeCandidate {
    let (source_title, source_kind) = sources
        .get(&e.source_id)
        .map(|s| (s.title.clone(), s.kind))
        .unwrap_or_else(|| (String::new(), SourceKind::Note));
    EpisodeCandidate {
        id: e.id.clone(),
        title: e.title.clone(),
        summary: e.summary.clone(),
        text: e.text.clone(),
        time: e.time.label(),
        time_start: e.time_start.clone(),
        source_title,
        source_kind,
        importance: e.importance,
    }
}
