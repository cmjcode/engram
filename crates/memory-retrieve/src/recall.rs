//! Recall hybrid: vektor + BM25 + ekspansi graph entitas, difusi RRF,
//! filter temporal, lalu fakta/prinsip/disposisi yang relevan.

use std::collections::{HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use memory_core::model::*;
use memory_core::text::{code_symbol_bonus, normalize, semantic_text_similarity, tokens, truncate_chars};
use memory_llm::cosine;
use memory_store::VectorIndex;

use crate::{Result, Retriever};

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct RecallQuery {
    /// Diisi dari URL/argumen bila kosong.
    #[serde(default)]
    pub subject: String,
    pub query: String,
    /// Filter/prioritas proyek atau workspace tertentu (mis. "LLM-MEMORY").
    #[serde(default)]
    pub project: Option<String>,
    /// Jendela waktu ISO (YYYY-MM-DD), opsional.
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    /// Lihat memori "per tanggal" ini (persona versi masa lalu).
    #[serde(default)]
    pub as_of: Option<String>,
    /// Batasi jenis: episode, fact, principle, disposition. Kosong = semua.
    #[serde(default)]
    pub kinds: Vec<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    /// `index` (bawaan untuk agent: satu baris per hasil + biaya token) atau `full`.
    #[serde(default)]
    pub detail: Option<String>,
    /// Anggaran teks konteks (perkiraan token); bawaan `retrieval.context_budget_tokens`.
    #[serde(default)]
    pub budget: Option<usize>,
}

impl RecallQuery {
    pub fn index_mode(&self) -> bool {
        self.detail.as_deref().is_some_and(|d| d.eq_ignore_ascii_case("index"))
    }
    pub fn new(subject: &str, query: &str) -> Self {
        Self { subject: subject.into(), query: query.into(), ..Default::default() }
    }
    fn wants(&self, kind: &str) -> bool {
        self.kinds.is_empty() || self.kinds.iter().any(|k| k == kind)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EpisodeHit {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub excerpt: String,
    pub time: String,
    pub time_start: Option<String>,
    pub life_phase: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub obs_type: Option<String>,
    pub retell_count: u32,
    pub score: f32,
    /// Jalur yang menemukan: vector, fulltext, graph, project.
    pub via: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FactView {
    pub id: String,
    pub entity: String,
    pub statement: String,
    pub category: String,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    /// Berlaku pada titik acuan (sekarang atau as_of).
    pub current: bool,
    pub status: FactStatus,
    pub confidence: f32,
    pub time: String,
    pub evidence: Vec<String>,
    pub superseded_by: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct RecallResult {
    pub found: bool,
    pub note: Option<String>,
    pub episodes: Vec<EpisodeHit>,
    pub facts: Vec<FactView>,
    pub principles: Vec<Principle>,
    pub dispositions: Vec<Disposition>,
    /// Konteks siap pakai untuk LLM, dengan sitasi [ep:ID].
    pub context: String,
}

fn rrf(lists: &[Vec<String>], k: f32) -> Vec<(String, f32)> {
    let mut scores: HashMap<String, f32> = HashMap::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            *scores.entry(id.clone()).or_default() += 1.0 / (k + rank as f32 + 1.0);
        }
    }
    let mut v: Vec<(String, f32)> = scores.into_iter().collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

/// Rentang episode memotong jendela? Waktu tak diketahui = tidak ikut bila ada jendela.
pub fn episode_in_window(e: &Episode, from: Option<&str>, to: Option<&str>) -> bool {
    if from.is_none() && to.is_none() {
        return true;
    }
    let Some(start) = e.time_start.as_deref().or(e.time_end.as_deref()) else { return false };
    let end = if e.time_ongoing { None } else { e.time_end.as_deref().or(Some(start)) };
    let starts_ok = to.is_none_or(|t| start <= t);
    let ends_ok = match (end, from) {
        (Some(en), Some(f)) => en >= f,
        _ => true,
    };
    starts_ok && ends_ok
}

pub fn fact_view(f: &Fact, entity_name: &str, reference: &str) -> FactView {
    FactView {
        id: f.id.clone(),
        entity: entity_name.to_string(),
        statement: f.statement(entity_name),
        category: f.category.clone(),
        valid_from: f.valid_from.clone(),
        valid_to: f.valid_to.clone(),
        current: f.status == FactStatus::Active && f.valid_at(reference),
        status: f.status,
        confidence: f.confidence,
        time: f.time.label(),
        evidence: f.evidence.iter().map(|e| e.episode_id.clone()).collect(),
        superseded_by: f.superseded_by.clone(),
    }
}

/// Kecocokan kata sederhana antara query dan teks (0..1).
fn overlap(query_toks: &HashSet<String>, text: &str) -> f32 {
    if query_toks.is_empty() {
        return 0.0;
    }
    let t: HashSet<String> = tokens(text).into_iter().filter(|w| w.chars().count() > 2).collect();
    query_toks.iter().filter(|q| t.contains(*q)).count() as f32 / query_toks.len() as f32
}

impl Retriever {
    /// Entitas yang disebut di query: n-gram (1..3 kata) dicocokkan ke alias dalam satu batch query.
    pub async fn entities_in_query(&self, subject: &str, query: &str) -> Result<Vec<Entity>> {
        let toks = tokens(query);
        let mut grams: Vec<String> = Vec::new();
        let mut tried = HashSet::new();
        for n in (1..=3).rev() {
            for w in toks.windows(n) {
                let gram = w.join(" ");
                if gram.chars().count() < 3 || tried.len() > 40 || !tried.insert(gram.clone()) {
                    continue;
                }
                grams.push(gram);
            }
        }
        if grams.is_empty() {
            return Ok(vec![]);
        }
        Ok(self.store.entities_by_aliases(subject, &grams).await?)
    }

    pub async fn recall(&self, q: &RecallQuery) -> Result<RecallResult> {
        let rc = &self.cfg.retrieval;
        let limit = q.limit.unwrap_or(rc.top_k).clamp(1, 50);
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let reference = q.as_of.clone().unwrap_or(today);
        let to = match (&q.to, &q.as_of) {
            (Some(t), Some(a)) => Some(t.clone().min(a.clone())),
            (t, a) => t.clone().or(a.clone()),
        };
        let qvec = match self.embedder.embed_one(&q.query).await {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!(error = %e, "embedding query recall gagal, lanjut dengan pencarian fulltext dan graph");
                None
            }
        };
        let index = self.store.vector_index();
        let want_project = q.project.as_deref().map(str::trim).filter(|s| !s.is_empty());

        // 1. Kandidat episode & entitas dari tiga jalur secara paralel.
        let v_fut = async {
            match &qvec {
                Some(v) => Ok::<_, crate::RetrieveError>(index.search("episode", &q.subject, v, rc.candidate_k, "status = 'active'").await?),
                None => Ok(vec![]),
            }
        };
        let ev_fut = async {
            match &qvec {
                Some(v) => Ok::<_, crate::RetrieveError>(index.search("entity", &q.subject, v, 5, "status != 'merged'").await?),
                None => Ok(vec![]),
            }
        };
        let fts_fut = async { Ok::<_, crate::RetrieveError>(self.store.fulltext_episodes(&q.subject, &q.query, rc.candidate_k).await?) };
        let ent_fut = self.entities_in_query(&q.subject, &q.query);
        let proj_fut = async {
            match want_project {
                Some(proj) => {
                    let (ents, names) = tokio::try_join!(
                        self.store.entities_by_alias(&q.subject, proj),
                        self.store.project_names(&q.subject, proj),
                    )?;
                    Ok::<_, crate::RetrieveError>((ents, names))
                }
                None => Ok((vec![], vec![])),
            }
        };

        let (vhits, ehits, fts_hits, mut entities, (proj_ents, project_names)) =
            tokio::try_join!(v_fut, ev_fut, fts_fut, ent_fut, proj_fut)?;

        let best_sim = vhits.first().map(|h| h.similarity).unwrap_or(0.0);
        let vlist: Vec<String> = vhits.iter().filter(|h| h.similarity >= rc.min_similarity * 0.8).map(|h| h.id.clone()).collect();
        let flist: Vec<String> = fts_hits.into_iter().map(|(id, _)| id).collect();

        let mut proj_ent_ids: HashSet<String> = HashSet::new();
        for e in proj_ents {
            proj_ent_ids.insert(e.id.clone());
            if !entities.iter().any(|x| x.id == e.id) {
                entities.push(e);
            }
        }
        for h in ehits {
            if h.similarity >= 0.6 && !entities.iter().any(|e| e.id == h.id)
                && let Some(e) = self.store.get_entity(&h.id).await?
            {
                entities.push(e);
            }
        }

        // Ekspansi graph (batch): episode yang menyebut entitas + tetangga kekerabatan (hop 2) + episode proyek.
        let non_self_ids: Vec<String> = entities.iter().filter(|e| !e.is_self).map(|e| e.id.clone()).collect();
        let graph_fut = self.store.episodes_mentioning_any(&non_self_ids, (non_self_ids.len() * 20).clamp(20, 100));
        let kin_fut = async {
            if rc.graph_hops >= 2 && !non_self_ids.is_empty() {
                self.store.kin_touching_any(&non_self_ids).await
            } else {
                Ok(vec![])
            }
        };
        let plist_fut = async {
            match want_project {
                Some(proj) => {
                    self.store
                        .list_episodes(
                            &q.subject,
                            &memory_store::EpisodeFilter {
                                status: Some(EpisodeStatus::Active),
                                project: Some(proj.to_string()),
                                projects: project_names.clone(),
                                limit: 40,
                                newest_first: true,
                                ..Default::default()
                            },
                        )
                        .await
                }
                None => Ok(vec![]),
            }
        };

        let (graph_eps, kin_edges, proj_eps) = tokio::try_join!(graph_fut, kin_fut, plist_fut)?;
        let mut glist: Vec<String> = Vec::new();
        for ep in graph_eps {
            if !glist.contains(&ep.id) {
                glist.push(ep.id);
            }
        }
        let mut hop2: Vec<String> = Vec::new();
        if !kin_edges.is_empty() {
            let mut neighbor_ids: Vec<String> = Vec::new();
            for k in kin_edges {
                let other = if non_self_ids.contains(&k.from) { k.to } else { k.from };
                if !non_self_ids.contains(&other) && !neighbor_ids.contains(&other) {
                    neighbor_ids.push(other);
                }
            }
            for ep in self.store.episodes_mentioning_any(&neighbor_ids, 20).await? {
                if !hop2.contains(&ep.id) && !glist.contains(&ep.id) {
                    hop2.push(ep.id);
                }
            }
        }
        let mut plist: Vec<String> = Vec::new();
        for ep in proj_eps {
            if !plist.contains(&ep.id) {
                plist.push(ep.id);
            }
        }
        let fused = rrf(&[vlist.clone(), flist.clone(), glist.clone(), hop2.clone(), plist.clone()], rc.rrf_k);

        // 2. Muat, saring temporal & rerank lintas-sinyal (RRF + simbol kode + kemiripan semantik judul/konsep + konsensus jalur).
        let ids: Vec<String> = fused.iter().take(rc.candidate_k).map(|(id, _)| id.clone()).collect();
        let eps = self.store.episodes_by_ids(&ids).await?;
        let by_id: HashMap<&str, &Episode> = eps.iter().map(|e| (e.id.as_str(), e)).collect();
        let ep_belongs_to_project = |e: &Episode, wp: &str| -> bool {
            let nwp = normalize(wp);
            plist.contains(&e.id)
                || e.project.as_deref().is_some_and(|ep| {
                    memory_core::project::same_project(ep, wp)
                        || project_names.iter().any(|n| memory_core::project::same_project(ep, n))
                })
                || e.entity_ids.iter().any(|eid| proj_ent_ids.contains(eid))
                || (e.project.is_none() && !nwp.is_empty() && normalize(&e.text).contains(&nwp))
        };
        let mut hits: Vec<EpisodeHit> = Vec::new();
        for (id, score) in &fused {
            let Some(e) = by_id.get(id.as_str()) else { continue };
            if !episode_in_window(e, q.from.as_deref(), to.as_deref()) {
                continue;
            }
            let mut via = vec![];
            if vlist.contains(id) {
                via.push("vector".to_string());
            }
            if flist.contains(id) {
                via.push("fulltext".to_string());
            }
            if glist.contains(id) || hop2.contains(id) {
                via.push("graph".to_string());
            }
            let proj_match = want_project.is_some_and(|wp| ep_belongs_to_project(e, wp));
            if proj_match {
                via.push("project".to_string());
            }
            let proj_boost = match want_project {
                Some(_) if proj_match => 1.35,
                Some(_) if e.project.is_some() => 0.65,
                _ => 1.0,
            };
            // Reranker lintas-sinyal: konsensus multi-jalur + simbol kode/berkas + keselarasan judul/konsep
            let consensus_boost = 1.0 + 0.08 * via.len().saturating_sub(1) as f32;
            let mut touched_files = e.files_modified.clone();
            touched_files.extend(e.files_read.iter().cloned());
            let sym_bonus = code_symbol_bonus(&q.query, &e.title, &e.text, &touched_files);
            let meta_text = format!("{} {} {}", e.title, e.summary, e.concepts.join(" "));
            let align_bonus = 0.25 * semantic_text_similarity(&q.query, &meta_text);
            let rerank_mult = consensus_boost * (1.0 + sym_bonus + align_bonus);

            hits.push(EpisodeHit {
                id: e.id.clone(),
                title: e.title.clone(),
                summary: e.summary.clone(),
                excerpt: truncate_chars(&e.text, 600),
                time: e.time_label(),
                time_start: e.time_start.clone(),
                life_phase: e.life_phase.clone(),
                project: e.project.clone(),
                obs_type: e.obs_type.clone(),
                retell_count: e.retell_count,
                score: *score * (1.0 + 0.05 * e.retell_count.min(10) as f32) * (0.75 + 0.5 * e.importance) * proj_boost * rerank_mult,
                via,
            });
        }
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(limit);

        // 3. Fakta: ambil secara batch, skor hibrida (stem/trigram + bukti episode), dan isolasi per proyek bila diminta.
        let qtoks: HashSet<String> = tokens(&q.query).into_iter().filter(|w| w.chars().count() > 2).collect();
        let mut facts: Vec<(Fact, f32)> = Vec::new();
        if q.wants("fact") {
            let self_id = self.store.get_subject(&q.subject).await?.and_then(|s| s.self_entity);
            let mut ent_ids: Vec<String> = entities.iter().map(|e| e.id.clone()).collect();
            ent_ids.extend(self_id.clone());
            for h in hits.iter().take(5) {
                if let Some(e) = by_id.get(h.id.as_str()) {
                    ent_ids.extend(e.entity_ids.iter().cloned());
                }
            }
            ent_ids.sort();
            ent_ids.dedup();
            ent_ids.truncate(30);

            let top_eps: HashSet<&str> = hits.iter().map(|h| h.id.as_str()).collect();
            let named: HashSet<&str> = entities.iter().map(|e| e.id.as_str()).collect();
            let raw_facts = self.store.facts_for_entities(&ent_ids).await?;

            // Bila difilter berdasarkan proyek (`want_project`), muat episode bukti fakta yang belum ada di `by_id`
            // dalam satu batch query agar kita dapat memverifikasi atribusi proyek tiap fakta secara akurat.
            let mut ev_eps_map: HashMap<String, Episode> = eps.iter().map(|e| (e.id.clone(), e.clone())).collect();
            if want_project.is_some() {
                let mut missing_ev_ids: Vec<String> = raw_facts
                    .iter()
                    .flat_map(|f| f.evidence.iter().map(|ev| ev.episode_id.clone()))
                    .filter(|id| !ev_eps_map.contains_key(id))
                    .collect();
                missing_ev_ids.sort();
                missing_ev_ids.dedup();
                for ep in self.store.episodes_by_ids(&missing_ev_ids).await? {
                    ev_eps_map.insert(ep.id.clone(), ep);
                }
            }

            let fact_belongs_to_project = |f: &Fact, wp: &str| -> bool {
                if proj_ent_ids.contains(&f.entity_id)
                    || f.object_entity.as_deref().is_some_and(|o| proj_ent_ids.contains(o))
                {
                    return true;
                }
                let nwp = normalize(wp);
                if !nwp.is_empty()
                    && (normalize(&f.object_text).contains(&nwp) || normalize(&f.predicate).contains(&nwp))
                {
                    return true;
                }
                f.evidence.iter().any(|ev| {
                    plist.contains(&ev.episode_id)
                        || ev_eps_map
                            .get(&ev.episode_id)
                            .is_some_and(|ep| ep_belongs_to_project(ep, wp))
                })
            };

            for f in raw_facts {
                if matches!(f.status, FactStatus::Rejected | FactStatus::Quarantined) || facts.iter().any(|(x, _)| x.id == f.id) {
                    continue;
                }
                if let Some(wp) = want_project
                    && !fact_belongs_to_project(&f, wp)
                {
                    continue;
                }
                let text = format!("{} {} {}", f.predicate, f.object_text, f.category.replace('_', " "));
                let mut s = overlap(&qtoks, &text).max(semantic_text_similarity(&q.query, &text));
                if f.evidence.iter().any(|e| top_eps.contains(e.episode_id.as_str())) {
                    s += 0.5;
                }
                if named.contains(f.entity_id.as_str()) || f.object_entity.as_deref().is_some_and(|o| named.contains(o)) {
                    s += 0.3;
                }
                if s >= 0.3 {
                    facts.push((f, s));
                }
            }
            facts.sort_by(|a, b| b.1.total_cmp(&a.1));
            facts.truncate(limit * 2);
            // Riwayat kategori yang sama ikut untuk kategori eksklusif (atau saat tidak dibatasi proyek)
            // agar perubahan masa lalu terlihat utuh tanpa membocorkan fakta non-eksklusif proyek lain.
            let cats: HashSet<(String, String)> = facts
                .iter()
                .filter(|(f, _)| f.exclusive || f.supersedes.is_some() || f.superseded_by.is_some() || want_project.is_none())
                .map(|(f, _)| (f.entity_id.clone(), f.category.clone()))
                .collect();
            for (eid, cat) in cats {
                for f in self.store.facts_in_category(&eid, &cat).await? {
                    if let Some(wp) = want_project
                        && !fact_belongs_to_project(&f, wp)
                    {
                        continue;
                    }
                    if !facts.iter().any(|(x, _)| x.id == f.id) {
                        facts.push((f, 0.1));
                    }
                }
            }
            // Jendela waktu & as_of: buang yang tidak memotong jendela / belum terjadi.
            facts.retain(|(f, _)| {
                let after_from = q.from.as_deref().is_none_or(|fr| f.valid_to.as_deref().is_none_or(|t| t >= fr));
                let before_to = to.as_deref().is_none_or(|t| f.valid_from.as_deref().is_none_or(|v| v <= t));
                after_from && before_to
            });
        }
        let mut names: HashMap<String, String> = HashMap::new();
        for (f, _) in &facts {
            if !names.contains_key(&f.entity_id) {
                let n = self.store.get_entity(&f.entity_id).await?.map(|e| e.name).unwrap_or_else(|| f.entity_id.clone());
                names.insert(f.entity_id.clone(), n);
            }
        }
        let mut fviews: Vec<FactView> = facts.iter().map(|(f, _)| fact_view(f, &names[&f.entity_id], &reference)).collect();
        fviews.sort_by(|a, b| (a.entity.as_str(), a.category.as_str(), a.valid_from.as_deref()).cmp(&(b.entity.as_str(), b.category.as_str(), b.valid_from.as_deref())));

        // 4. Prinsip & disposisi yang relevan dan sudah ada pada titik acuan.
        let mut principles = Vec::new();
        if q.wants("principle") {
            let all: Vec<Principle> = self
                .store
                .list_principles(&q.subject, Some(ReflectiveStatus::Active))
                .await?
                .into_iter()
                .filter(|p| p.valid_from.as_deref().is_none_or(|v| v <= reference.as_str()))
                .collect();
            if !all.is_empty() {
                if let Some(qv) = &qvec
                    && let Ok(vecs) = self.embedder.embed(&all.iter().map(|p| p.statement.clone()).collect::<Vec<_>>()).await
                {
                    let mut scored: Vec<(Principle, f32)> = all.into_iter().zip(vecs).map(|(p, v)| (p, cosine(&v, qv))).filter(|(_, s)| *s >= 0.3).collect();
                    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
                    principles = scored.into_iter().take(3).map(|(p, _)| p).collect();
                } else {
                    let mut scored: Vec<(Principle, f32)> = all.into_iter().map(|p| {
                        let s = overlap(&qtoks, &format!("{} {}", p.statement, p.domain));
                        (p, s)
                    }).filter(|(_, s)| *s > 0.0).collect();
                    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
                    principles = scored.into_iter().take(3).map(|(p, _)| p).collect();
                }
            }
        }
        let mut dispositions = Vec::new();
        if q.wants("disposition") {
            for d in self.store.list_dispositions(&q.subject).await? {
                let active = d.status == ReflectiveStatus::Active && d.valid_from.as_deref().is_none_or(|v| v <= reference.as_str());
                if active && (overlap(&qtoks, &format!("{} {}", d.label, d.description)) > 0.0 || normalize(&q.query).contains(&normalize(&d.label))) {
                    dispositions.push(d);
                }
            }
        }

        if !q.wants("episode") {
            hits.clear();
        }
        let any = !hits.is_empty() || !fviews.is_empty() || !principles.is_empty() || !dispositions.is_empty();
        let weak = best_sim < rc.min_similarity && flist.is_empty() && plist.is_empty() && entities.iter().all(|e| e.is_self) && fviews.is_empty();
        let mut result = RecallResult { found: any && !weak, note: None, episodes: hits, facts: fviews, principles, dispositions, context: String::new() };
        if !result.found {
            result.episodes.clear();
            result.note = Some("Tidak ada catatan memori yang relevan untuk pertanyaan ini.".into());
        }
        let budget = q.budget.unwrap_or(rc.context_budget_tokens).clamp(200, 50_000);
        result.context = if q.index_mode() {
            // Fakta "peristiwa" sekali-pakai hanya tampil bila agent memang meminta fakta.
            let explicit_facts = q.kinds.iter().any(|k| k == "fact");
            crate::assemble_index(&result, &reference, q.as_of.is_some(), budget, explicit_facts)
        } else {
            crate::assemble_context(&result, &reference, q.as_of.is_some(), budget)
        };
        Ok(result)
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn rrf_rewards_agreement() {
        let fused = rrf(&[vec!["a".into(), "b".into()], vec!["b".into(), "c".into()]], 60.0);
        assert_eq!(fused[0].0, "b");
    }
}
