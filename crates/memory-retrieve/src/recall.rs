//! Recall hybrid: vektor + BM25 + ekspansi graph entitas, difusi RRF,
//! filter temporal, lalu fakta/prinsip/disposisi yang relevan.

use std::collections::{HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use memory_core::model::*;
use memory_core::text::{normalize, tokens, truncate_chars};
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
    /// Entitas yang disebut di query: n-gram (1..3 kata) dicocokkan ke alias.
    pub async fn entities_in_query(&self, subject: &str, query: &str) -> Result<Vec<Entity>> {
        let toks = tokens(query);
        let mut out: Vec<Entity> = Vec::new();
        let mut tried = HashSet::new();
        for n in (1..=3).rev() {
            for w in toks.windows(n) {
                let gram = w.join(" ");
                if gram.chars().count() < 3 || tried.len() > 40 || !tried.insert(gram.clone()) {
                    continue;
                }
                for e in self.store.entities_by_alias(subject, &gram).await? {
                    if !out.iter().any(|x| x.id == e.id) {
                        out.push(e);
                    }
                }
            }
        }
        Ok(out)
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

        // 1. Kandidat episode dari tiga jalur.
        let vhits = match &qvec {
            Some(v) => index.search("episode", &q.subject, v, rc.candidate_k, "status = 'active'").await?,
            None => vec![],
        };
        let best_sim = vhits.first().map(|h| h.similarity).unwrap_or(0.0);
        let vlist: Vec<String> = vhits.iter().filter(|h| h.similarity >= rc.min_similarity * 0.8).map(|h| h.id.clone()).collect();
        let flist: Vec<String> = self.store.fulltext_episodes(&q.subject, &q.query, rc.candidate_k).await?.into_iter().map(|(id, _)| id).collect();
        let mut entities = self.entities_in_query(&q.subject, &q.query).await?;
        let want_project = q.project.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let mut project_names: Vec<String> = Vec::new();
        if let Some(proj) = want_project {
            for e in self.store.entities_by_alias(&q.subject, proj).await? {
                if !entities.iter().any(|x| x.id == e.id) {
                    entities.push(e);
                }
            }
            project_names = self.store.project_names(&q.subject, proj).await?;
        }
        if let Some(v) = &qvec {
            for h in index.search("entity", &q.subject, v, 5, "status != 'merged'").await? {
                if h.similarity >= 0.6 && !entities.iter().any(|e| e.id == h.id)
                    && let Some(e) = self.store.get_entity(&h.id).await? {
                        entities.push(e);
                    }
            }
        }
        // Ekspansi graph: episode yang menyebut entitas + tetangga kekerabatan (hop 2).
        let mut glist: Vec<String> = Vec::new();
        let mut hop2: Vec<String> = Vec::new();
        for e in entities.iter().filter(|e| !e.is_self) {
            for ep in self.store.episodes_mentioning(&e.id, 20).await? {
                if !glist.contains(&ep.id) {
                    glist.push(ep.id);
                }
            }
            if rc.graph_hops >= 2 {
                for k in self.store.kin_touching(&e.id).await? {
                    let other = if k.from == e.id { k.to } else { k.from };
                    for ep in self.store.episodes_mentioning(&other, 5).await? {
                        if !hop2.contains(&ep.id) && !glist.contains(&ep.id) {
                            hop2.push(ep.id);
                        }
                    }
                }
            }
        }
        let mut plist: Vec<String> = Vec::new();
        if let Some(proj) = want_project {
            for ep in self
                .store
                .list_episodes(
                    &q.subject,
                    &memory_store::EpisodeFilter {
                        status: Some(EpisodeStatus::Active),
                        project: Some(proj.to_string()),
                        projects: project_names.clone(),
                        limit: 20,
                        newest_first: true,
                        ..Default::default()
                    },
                )
                .await?
            {
                if !plist.contains(&ep.id) {
                    plist.push(ep.id);
                }
            }
        }
        let fused = rrf(&[vlist.clone(), flist.clone(), glist.clone(), hop2.clone(), plist.clone()], rc.rrf_k);

        // 2. Muat & saring temporal.
        let ids: Vec<String> = fused.iter().take(rc.candidate_k).map(|(id, _)| id.clone()).collect();
        let eps = self.store.episodes_by_ids(&ids).await?;
        let by_id: HashMap<&str, &Episode> = eps.iter().map(|e| (e.id.as_str(), e)).collect();
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
            let proj_match = want_project.is_some_and(|wp| {
                let nwp = normalize(wp);
                plist.contains(id)
                    || e.project.as_deref().is_some_and(|ep| memory_core::project::same_project(ep, wp) || project_names.iter().any(|n| memory_core::project::same_project(ep, n)))
                    || (!nwp.is_empty() && normalize(&e.text).contains(&nwp))
            });
            if proj_match {
                via.push("project".to_string());
            }
            let proj_boost = if proj_match { 1.35 } else { 1.0 };
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
                score: *score * (1.0 + 0.05 * e.retell_count.min(10) as f32) * (0.75 + 0.5 * e.importance) * proj_boost,
                via,
            });
        }
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(limit);

        // 3. Fakta: dari entitas yang disebut, diri subjek, entitas di episode teratas.
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
            let top_eps: HashSet<&str> = hits.iter().map(|h| h.id.as_str()).collect();
            let named: HashSet<&str> = entities.iter().map(|e| e.id.as_str()).collect();
            for eid in ent_ids.iter().take(30) {
                for f in self.store.facts_for_entity(eid).await? {
                    if matches!(f.status, FactStatus::Rejected | FactStatus::Quarantined) || facts.iter().any(|(x, _)| x.id == f.id) {
                        continue;
                    }
                    let text = format!("{} {} {}", f.predicate, f.object_text, f.category.replace('_', " "));
                    let mut s = overlap(&qtoks, &text);
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
            }
            facts.sort_by(|a, b| b.1.total_cmp(&a.1));
            facts.truncate(limit * 2);
            // Seluruh riwayat kategori yang sama ikut agar perubahan terlihat utuh.
            let cats: HashSet<(String, String)> = facts.iter().map(|(f, _)| (f.entity_id.clone(), f.category.clone())).collect();
            for (eid, cat) in cats {
                for f in self.store.facts_in_category(&eid, &cat).await? {
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
