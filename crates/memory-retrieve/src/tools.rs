//! Tool baca tingkat tinggi untuk agent dan UI.

use std::collections::{HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use memory_core::kin::{KinEdge, infer_kin};
use memory_core::model::*;
use memory_core::text::truncate_chars;
use memory_llm::{ChatRequest, Message, ModelRole, cosine};
use memory_store::{EpisodeFilter, VectorIndex};

use crate::recall::{FactView, RecallQuery, RecallResult, fact_view};
use crate::{Result, Retriever};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EpisodeRef {
    pub id: String,
    pub title: String,
    pub time: String,
    pub time_start: Option<String>,
    pub excerpt: String,
    #[serde(default)]
    pub obs_type: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    /// Perkiraan token bila teks penuh diambil.
    #[serde(default)]
    pub approx_tokens: usize,
}

impl From<&Episode> for EpisodeRef {
    fn from(e: &Episode) -> Self {
        Self {
            id: e.id.clone(),
            title: e.title.clone(),
            time: e.time_label(),
            time_start: e.time_start.clone(),
            excerpt: if e.summary.is_empty() { e.text.chars().take(300).collect() } else { e.summary.clone() },
            obs_type: e.obs_type.clone(),
            project: e.project.clone(),
            approx_tokens: e.approx_tokens(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KinView {
    pub person_id: String,
    pub name: String,
    /// "pakde", "cucu", ... relatif terhadap entitas yang ditanyakan.
    pub relation: String,
    pub term: String,
    pub inferred: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WhoIs {
    pub entity: Entity,
    /// Hubungan dengan subjek memori, mis. "pakde".
    pub relation_to_subject: Option<String>,
    pub current_facts: Vec<FactView>,
    pub history: Vec<FactView>,
    pub kin: Vec<KinView>,
    pub episodes: Vec<EpisodeRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TreeNode {
    pub id: String,
    pub name: String,
    pub gender: Option<memory_core::Gender>,
    pub is_self: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TreeEdge {
    pub from: String,
    pub to: String,
    pub relation: memory_core::KinRelation,
    pub label: String,
    pub inferred: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FamilyTree {
    pub nodes: Vec<TreeNode>,
    pub edges: Vec<TreeEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TimelineItem {
    /// episode | fact_start | fact_end
    pub kind: String,
    pub date: Option<String>,
    pub label: String,
    pub id: String,
    pub life_phase: Option<String>,
    pub time: String,
    #[serde(default)]
    pub obs_type: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub approx_tokens: usize,
    /// Episode jangkar (mode `timeline(anchor)`).
    #[serde(default)]
    pub anchor: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Timeline {
    pub items: Vec<TimelineItem>,
    pub phases: Vec<LifePhase>,
    /// Episode tanpa waktu yang bisa diurutkan.
    pub undated: Vec<EpisodeRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PrincipleView {
    pub principle: Principle,
    pub evidence: Vec<EpisodeRef>,
    pub relevance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PersonaContext {
    pub system_prompt: String,
    pub age_at_reference: Option<u32>,
    pub found: bool,
    pub recall: RecallResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectContext {
    pub project: String,
    pub found: bool,
    pub entity: Option<Entity>,
    pub current_facts: Vec<FactView>,
    pub history_facts: Vec<FactView>,
    pub decisions: Vec<Decision>,
    pub principles: Vec<PrincipleView>,
    pub episodes: Vec<EpisodeRef>,
    /// Konvensi, koreksi, dan jebakan proyek yang berulang.
    #[serde(default)]
    pub lessons: Vec<Lesson>,
    /// Metadata pengguna: repositori, catatan dengan tautan `[[...]]`, tag grup.
    #[serde(default)]
    pub meta: Option<ProjectMeta>,
    /// Remote/nama repositori git (isian pengguna, atau terdeteksi dari `.git/config`).
    #[serde(default)]
    pub repo: Option<String>,
    /// Proyek terkait: tautan `[[...]]` dari catatan proyek ini lalu backlink dari proyek lain.
    #[serde(default)]
    pub related: Vec<String>,
    /// Ringkasan konteks siap pakai untuk AI coding agent.
    pub context: String,
}

/// Opsi tool baca (anggaran & cakupan).
#[derive(Debug, Clone, Default)]
pub struct ToolOpts {
    /// Perkiraan token untuk teks konteks; `None` = `retrieval.tool_budget_tokens`.
    pub budget_tokens: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Answer {
    pub answer: String,
    pub found: bool,
    pub citations: Vec<String>,
    pub context: String,
}

fn rel_label(k: &Kin, reverse: bool, gender: Option<memory_core::Gender>) -> String {
    let rel = if reverse { k.relation.inverse() } else { k.relation };
    let older = if reverse { k.older.map(|o| !o) } else { k.older };
    rel.label_id(gender, older).to_string()
}

/// Ambil `[ep:ID]` dari teks jawaban.
pub fn citations(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("[ep:") {
        let after = &rest[i + 4..];
        if let Some(j) = after.find(']') {
            let id = after[..j].trim().to_string();
            if !id.is_empty() && !out.contains(&id) {
                out.push(id);
            }
            rest = &after[j..];
        } else {
            break;
        }
    }
    out
}

impl Retriever {
    pub async fn find_entity(&self, subject: &str, name: &str) -> Result<Option<Entity>> {
        let mut c = self.store.entities_by_alias(subject, name).await?;
        c.sort_by_key(|e| std::cmp::Reverse(e.mention_count));
        if let Some(e) = c.into_iter().next() {
            return Ok(Some(e));
        }
        if let Ok(v) = self.embedder.embed_one(name).await {
            for h in self.store.vector_index().search("entity", subject, &v, 1, "status != 'merged'").await? {
                if h.similarity >= 0.6 {
                    return Ok(self.store.get_entity(&h.id).await?);
                }
            }
        }
        Ok(None)
    }

    pub async fn who_is(&self, subject: &str, name: &str) -> Result<Option<WhoIs>> {
        let Some(entity) = self.find_entity(subject, name).await? else { return Ok(None) };
        self.who_is_entity(subject, entity).await.map(Some)
    }

    pub async fn who_is_id(&self, subject: &str, entity_id: &str) -> Result<Option<WhoIs>> {
        match self.store.resolve_entity(entity_id).await? {
            Some(e) if e.subject == subject => self.who_is_entity(subject, e).await.map(Some),
            _ => Ok(None),
        }
    }

    async fn who_is_entity(&self, subject: &str, entity: Entity) -> Result<WhoIs> {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let mut names: HashMap<String, String> = HashMap::new();
        names.insert(entity.id.clone(), entity.name.clone());
        let mut current = Vec::new();
        let mut history = Vec::new();
        for f in self.store.facts_for_entity(&entity.id).await? {
            if matches!(f.status, FactStatus::Rejected | FactStatus::Quarantined) {
                continue;
            }
            let ename = match names.get(&f.entity_id) {
                Some(n) => n.clone(),
                None => {
                    let n = self.store.get_entity(&f.entity_id).await?.map(|e| e.name).unwrap_or_default();
                    names.insert(f.entity_id.clone(), n.clone());
                    n
                }
            };
            let v = fact_view(&f, &ename, &today);
            if v.current { current.push(v) } else { history.push(v) }
        }
        let tree = self.family_tree(subject).await?;
        let self_id = tree.nodes.iter().find(|n| n.is_self).map(|n| n.id.clone());
        let relation_to_subject =
            self_id.as_ref().and_then(|sid| tree.edges.iter().find(|e| e.from == entity.id && &e.to == sid).map(|e| e.label.clone()));
        let node_name: HashMap<&str, &str> = tree.nodes.iter().map(|n| (n.id.as_str(), n.name.as_str())).collect();
        let kin = tree
            .edges
            .iter()
            .filter(|e| e.to == entity.id)
            .map(|e| KinView {
                person_id: e.from.clone(),
                name: node_name.get(e.from.as_str()).copied().unwrap_or("?").to_string(),
                relation: e.label.clone(),
                term: String::new(),
                inferred: e.inferred,
            })
            .collect();
        let episodes = self.store.episodes_mentioning(&entity.id, 20).await?.iter().map(EpisodeRef::from).collect();
        Ok(WhoIs { entity, relation_to_subject, current_facts: current, history, kin, episodes })
    }

    /// Silsilah: edge langsung plus relasi turunan (kakek, sepupu, mertua, ...).
    pub async fn family_tree(&self, subject: &str) -> Result<FamilyTree> {
        let kin = self.store.kin_for_subject(subject).await?;
        let mut ids: HashSet<String> = kin.iter().flat_map(|k| [k.from.clone(), k.to.clone()]).collect();
        if let Some(s) = self.store.get_subject(subject).await?.and_then(|s| s.self_entity) {
            ids.insert(s);
        }
        let mut nodes = Vec::new();
        let mut gender: HashMap<String, Option<memory_core::Gender>> = HashMap::new();
        for id in &ids {
            if let Some(e) = self.store.resolve_entity(id).await? {
                gender.insert(id.clone(), e.gender);
                nodes.push(TreeNode { id: id.clone(), name: e.name, gender: e.gender, is_self: e.is_self });
            }
        }
        nodes.sort_by(|a, b| b.is_self.cmp(&a.is_self).then(a.name.cmp(&b.name)));
        let mut edges: Vec<TreeEdge> = Vec::new();
        for k in &kin {
            let g = gender.get(&k.from).copied().flatten().or(k.gender);
            edges.push(TreeEdge {
                from: k.from.clone(),
                to: k.to.clone(),
                relation: k.relation,
                label: rel_label(k, false, g),
                inferred: false,
            });
            let g2 = gender.get(&k.to).copied().flatten();
            edges.push(TreeEdge {
                from: k.to.clone(),
                to: k.from.clone(),
                relation: k.relation.inverse(),
                label: rel_label(k, true, g2),
                inferred: false,
            });
        }
        let base: Vec<KinEdge> = kin.iter().map(|k| KinEdge { from: k.from.clone(), to: k.to.clone(), relation: k.relation }).collect();
        for e in infer_kin(&base) {
            if edges.iter().any(|x| x.from == e.from && x.to == e.to) {
                continue;
            }
            let g = gender.get(&e.from).copied().flatten();
            edges.push(TreeEdge {
                label: e.relation.label_id(g, None).to_string(),
                from: e.from,
                to: e.to,
                relation: e.relation,
                inferred: true,
            });
        }
        Ok(FamilyTree { nodes, edges })
    }

    pub async fn timeline(
        &self,
        subject: &str,
        entity: Option<&str>,
        phase: Option<&str>,
        from: Option<&str>,
        to: Option<&str>,
        limit: usize,
    ) -> Result<Timeline> {
        let limit = limit.clamp(1, 2000);
        let mut eps: Vec<Episode> = match entity {
            Some(name) => match self.find_entity(subject, name).await? {
                Some(e) => self.store.episodes_mentioning(&e.id, limit).await?,
                None => return Ok(Timeline::default()),
            },
            None => {
                self.store
                    .list_episodes(
                        subject,
                        &EpisodeFilter {
                            status: Some(EpisodeStatus::Active),
                            life_phase: phase.map(str::to_string),
                            from: from.map(str::to_string),
                            to: to.map(str::to_string),
                            limit,
                            ..Default::default()
                        },
                    )
                    .await?
            }
        };
        eps.retain(|e| e.status == EpisodeStatus::Active);
        let mut items = Vec::new();
        let mut undated = Vec::new();
        for e in &eps {
            if e.time_start.is_none() && e.time_end.is_none() {
                undated.push(EpisodeRef::from(e));
                continue;
            }
            items.push(TimelineItem {
                kind: "episode".into(),
                date: e.time_start.clone().or(e.time_end.clone()),
                label: if e.title.is_empty() { e.text.chars().take(80).collect() } else { e.title.clone() },
                id: e.id.clone(),
                life_phase: e.life_phase.clone(),
                time: e.time_label(),
                obs_type: e.obs_type.clone(),
                project: e.project.clone(),
                approx_tokens: e.approx_tokens(),
                anchor: false,
            });
        }
        if let Some(name) = entity
            && let Some(ent) = self.find_entity(subject, name).await?
        {
            for f in self.store.facts_for_entity(&ent.id).await? {
                if f.status != FactStatus::Active {
                    continue;
                }
                let st = f.statement(&ent.name);
                if let Some(d) = &f.valid_from {
                    items.push(TimelineItem {
                        kind: "fact_start".into(),
                        date: Some(d.clone()),
                        label: format!("mulai: {st}"),
                        id: f.id.clone(),
                        life_phase: None,
                        time: f.time.label(),
                        obs_type: None,
                        project: None,
                        approx_tokens: 0,
                        anchor: false,
                    });
                }
                if let Some(d) = &f.valid_to {
                    items.push(TimelineItem {
                        kind: "fact_end".into(),
                        date: Some(d.clone()),
                        label: format!("berakhir: {st}"),
                        id: f.id.clone(),
                        life_phase: None,
                        time: f.time.label(),
                        obs_type: None,
                        project: None,
                        approx_tokens: 0,
                        anchor: false,
                    });
                }
            }
        }
        items.sort_by(|a, b| a.date.cmp(&b.date));
        Ok(Timeline { items, phases: self.store.list_life_phases(subject).await?, undated })
    }

    /// Kronologi di sekitar satu episode jangkar (urutan dibuat), dalam proyek yang sama
    /// bila jangkar punya proyek. `anchor` boleh `ep:<id>`.
    pub async fn timeline_around(
        &self,
        subject: &str,
        anchor: &str,
        before: usize,
        after: usize,
        project: Option<&str>,
    ) -> Result<Timeline> {
        let id = anchor.trim().trim_start_matches("ep:");
        let Some(a) = self.store.get_episode(id).await?.filter(|e| e.subject == subject) else {
            return Err(crate::RetrieveError::NotFound(format!("episode jangkar '{anchor}' tidak ada")));
        };
        let proj = project.map(str::to_string).or_else(|| a.project.clone());
        let names = match &proj {
            Some(p) => self.store.project_names(subject, p).await?,
            None => vec![],
        };
        let eps = self.store.episodes_around(subject, &a, &names, before.min(50), after.min(50)).await?;
        let items = eps
            .iter()
            .map(|e| TimelineItem {
                kind: "episode".into(),
                date: Some(e.created_at.chars().take(10).collect()),
                label: if e.title.is_empty() { e.text.chars().take(80).collect() } else { e.title.clone() },
                id: e.id.clone(),
                life_phase: e.life_phase.clone(),
                time: e.time_label(),
                obs_type: e.obs_type.clone(),
                project: e.project.clone(),
                approx_tokens: e.approx_tokens(),
                anchor: e.id == a.id,
            })
            .collect();
        Ok(Timeline { items, phases: vec![], undated: vec![] })
    }

    pub async fn principles(&self, subject: &str, situation: Option<&str>) -> Result<Vec<PrincipleView>> {
        let all = self.store.list_principles(subject, Some(ReflectiveStatus::Active)).await?;
        let rel: Vec<f32> = match situation.filter(|s| !s.trim().is_empty()) {
            Some(s) if !all.is_empty() => {
                if let Ok(q) = self.embedder.embed_one(s).await
                    && let Ok(vs) = self.embedder.embed(&all.iter().map(|p| p.statement.clone()).collect::<Vec<_>>()).await
                {
                    vs.iter().map(|v| cosine(v, &q)).collect()
                } else {
                    all.iter().map(|p| p.confidence).collect()
                }
            }
            _ => all.iter().map(|p| p.confidence).collect(),
        };
        let mut out = Vec::new();
        for (p, r) in all.into_iter().zip(rel) {
            let ids: Vec<String> = p.evidence.iter().map(|e| e.episode_id.clone()).collect();
            let evidence = self.store.episodes_by_ids(&ids).await?.iter().map(EpisodeRef::from).collect();
            out.push(PrincipleView { principle: p, evidence, relevance: r });
        }
        out.sort_by(|a, b| b.relevance.total_cmp(&a.relevance));
        Ok(out)
    }

    /// System prompt persona: gaya tutur + prinsip + memori relevan + guardrail.
    pub async fn persona_context(&self, subject: &str, question: &str, as_of: Option<&str>) -> Result<PersonaContext> {
        let subj = self.store.get_subject(subject).await?;
        let name = subj.as_ref().map(|s| s.name.clone()).unwrap_or_else(|| subject.to_string());
        let reference = as_of.map(str::to_string).unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
        let age =
            subj.as_ref().and_then(|s| chrono::NaiveDate::parse_from_str(&reference, "%Y-%m-%d").ok().and_then(|d| s.anchors.age_at(d)));
        let recall = self.recall(&RecallQuery { as_of: as_of.map(str::to_string), ..RecallQuery::new(subject, question) }).await?;
        let voice = self.store.get_voice_profile(subject).await?.unwrap_or_default();
        let mut p = format!("Kamu berbicara sebagai {name}");
        match (as_of, age) {
            (Some(d), Some(a)) => p.push_str(&format!(", versi dirinya pada {d} (umur {a})")),
            (Some(d), None) => p.push_str(&format!(", versi dirinya pada {d}")),
            (None, Some(a)) => p.push_str(&format!(" (umur {a})")),
            _ => {}
        }
        p.push_str(". Gunakan sudut pandang orang pertama.\n");
        if !voice.style.is_empty() {
            p.push_str(&format!("Gaya bertutur: {}\n", voice.style));
        }
        if !voice.idioms.is_empty() {
            let id: Vec<String> = voice.idioms.iter().take(8).map(|(k, _)| format!("\"{k}\"")).collect();
            p.push_str(&format!("Ungkapan khas (pakai secukupnya, jangan dipaksakan): {}\n", id.join(", ")));
        }
        if !voice.language_mix.is_empty() {
            let mix: Vec<String> = voice.language_mix.iter().map(|(k, v)| format!("{k} {:.0}%", v * 100.0)).collect();
            p.push_str(&format!("Campuran bahasa: {}\n", mix.join(", ")));
        }
        p.push_str("\nATURAN:\n");
        p.push_str("1. Jawab HANYA berdasarkan MEMORI di bawah. Jangan mengarang peristiwa, pendapat, atau nama.\n");
        p.push_str("2. Bila memori tidak memuat jawabannya, katakan terus terang bahwa kamu tidak ingat atau tidak ada catatannya.\n");
        p.push_str("3. Bedakan yang dulu dan yang sekarang sesuai status fakta; jangan mencampur keduanya.\n");
        p.push_str("4. Fakta berstatus BERTENTANGAN disampaikan sebagai hal yang belum pasti.\n");
        if let Some(d) = as_of {
            p.push_str(&format!("5. Kamu TIDAK tahu apa pun yang terjadi setelah {d}.\n"));
        }
        p.push_str("6. Akhiri kalimat yang memakai memori dengan sitasi [ep:ID].\n\nMEMORI:\n");
        p.push_str(if recall.found { &recall.context } else { "(tidak ada memori yang relevan)" });
        Ok(PersonaContext { system_prompt: p, age_at_reference: age, found: recall.found, recall })
    }

    /// Konteks terpadu untuk satu proyek/workspace ditambah standar kerja & prinsip subjek
    /// lintas AI tool (Claude Code, Antigravity, Cursor, Gemini CLI).
    pub async fn project_context(&self, subject: &str, project: &str, task: Option<&str>) -> Result<ProjectContext> {
        self.project_context_with(subject, project, task, &ToolOpts::default()).await
    }

    pub async fn project_context_with(&self, subject: &str, project: &str, task: Option<&str>, opts: &ToolOpts) -> Result<ProjectContext> {
        let proj = project.trim();
        let budget_tokens = opts.budget_tokens.unwrap_or(self.cfg.retrieval.tool_budget_tokens).clamp(200, 50_000);
        let project_names = self.store.project_names(subject, proj).await?;
        let query_str = match task.map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => format!("{proj} {t}"),
            None => format!("{proj} arsitektur tech stack konvensi keputusan aturan bug"),
        };
        let rq = RecallQuery {
            subject: subject.to_string(),
            query: query_str,
            project: Some(proj.to_string()),
            limit: Some(15),
            ..Default::default()
        };

        // Jalankan pencarian recall, entitas proyek, daftar episode proyek, prinsip, pelajaran, dan sesi secara paralel.
        let ep_fut = async {
            Ok::<_, crate::RetrieveError>(
                self.store
                    .list_episodes(
                        subject,
                        &EpisodeFilter {
                            status: Some(EpisodeStatus::Active),
                            project: Some(proj.to_string()),
                            projects: project_names.clone(),
                            limit: 25,
                            newest_first: true,
                            ..Default::default()
                        },
                    )
                    .await?,
            )
        };
        let lessons_fut = async {
            Ok::<_, crate::RetrieveError>(self.store.list_lessons(subject, &project_names, Some("active"), 16).await?)
        };
        let sessions_fut = async {
            Ok::<_, crate::RetrieveError>(self.store.list_sessions(subject, &project_names, 5).await?)
        };
        let metas_fut = async { Ok::<_, crate::RetrieveError>(self.store.list_project_meta(subject).await?) };
        let (recall, entity, mut ep_list, principles_raw, mut lessons, sessions, metas) = tokio::try_join!(
            self.recall(&rq),
            self.find_entity(subject, proj),
            ep_fut,
            self.principles(subject, task.or(Some(proj))),
            lessons_fut,
            sessions_fut,
            metas_fut,
        )?;

        if let Some(ent) = &entity {
            for ep in self.store.episodes_mentioning(&ent.id, 20).await? {
                if !ep_list.iter().any(|x| x.id == ep.id) {
                    ep_list.push(ep);
                }
            }
        }

        let matches_proj_name = |name: &str| -> bool {
            memory_core::project::same_project(name, proj) || project_names.iter().any(|n| memory_core::project::same_project(name, n))
        };
        let text_mentions_proj = |text: &str| -> bool {
            let nt = memory_core::text::normalize(text);
            project_names.iter().any(|n| {
                let nn = memory_core::text::normalize(n);
                nn.chars().count() >= 3 && nt.contains(&nn)
            })
        };

        let recall_ep_ids: Vec<String> =
            recall.episodes.iter().filter(|h| !ep_list.iter().any(|x| x.id == h.id)).map(|h| h.id.clone()).collect();
        for ep in self.store.episodes_by_ids(&recall_ep_ids).await? {
            if ep_list.iter().any(|x| x.id == ep.id) {
                continue;
            }
            // Cegah episode dari proyek lain bocor ke konteks proyek ini:
            let belongs = match ep.project.as_deref() {
                Some(ep_proj) => matches_proj_name(ep_proj),
                None => {
                    entity.as_ref().is_some_and(|ent| ep.entity_ids.contains(&ent.id))
                        || text_mentions_proj(&ep.text)
                        || text_mentions_proj(&ep.title)
                }
            };
            if belongs {
                ep_list.push(ep);
            }
        }

        let proj_ep_ids: HashSet<String> = ep_list.iter().map(|e| e.id.clone()).collect();
        let mut current_facts: Vec<FactView> = Vec::new();
        let mut history_facts: Vec<FactView> = Vec::new();
        if let Some(ent) = &entity
            && let Ok(Some(w)) = self.who_is_id(subject, &ent.id).await
        {
            current_facts = w.current_facts;
            history_facts = w.history;
        }
        for fv in &recall.facts {
            // Hanya masukkan fakta dari recall bila entitasnya adalah proyek ini, pernyataannya menyebut proyek ini,
            // atau bukti episodenya berasal dari proyek ini.
            let from_proj_ep = fv.evidence.iter().any(|eid| proj_ep_ids.contains(eid));
            let is_proj_entity =
                entity.as_ref().is_some_and(|ent| fv.entity.eq_ignore_ascii_case(&ent.name)) || matches_proj_name(&fv.entity);
            if !from_proj_ep && !is_proj_entity && !text_mentions_proj(&fv.statement) {
                continue;
            }
            if fv.current {
                if !current_facts.iter().any(|x| x.id == fv.id) {
                    current_facts.push(fv.clone());
                }
            } else if !history_facts.iter().any(|x| x.id == fv.id) {
                history_facts.push(fv.clone());
            }
        }

        let mut decisions: Vec<Decision> = Vec::new();
        for ep in &ep_list {
            for d in &ep.decisions {
                if !decisions.iter().any(|x| x.situation == d.situation && x.action == d.action) {
                    decisions.push(d.clone());
                }
            }
        }

        // Bila ada `task`, prioritaskan pelajaran (`lessons`) dan fakta yang paling relevan secara semantik dengan tugas.
        if let Some(t) = task.map(str::trim).filter(|t| !t.is_empty()) {
            lessons.sort_by(|a, b| {
                let sa =
                    (a.occurrences as f32 * 0.25) + memory_core::text::semantic_text_similarity(t, &format!("{} {}", a.title, a.detail));
                let sb =
                    (b.occurrences as f32 * 0.25) + memory_core::text::semantic_text_similarity(t, &format!("{} {}", b.title, b.detail));
                sb.total_cmp(&sa)
            });
        }
        lessons.truncate(12);

        // Keputusan & fakta: utamakan yang bukan "peristiwa" sekali-pakai, lalu batasi jumlah.
        current_facts.sort_by_key(|f| crate::is_event_fact(&f.category));
        current_facts.truncate(40);
        history_facts.truncate(20);
        decisions.truncate(15);
        let episodes: Vec<EpisodeRef> = ep_list.iter().take(15).map(EpisodeRef::from).collect();
        let principles: Vec<PrincipleView> = principles_raw.into_iter().take(8).collect();
        let meta = metas
            .iter()
            .find(|m| memory_core::project::project_key(&m.project) == memory_core::project::project_key(proj))
            .or_else(|| metas.iter().find(|m| matches_proj_name(&m.project)))
            .cloned();
        let mut related: Vec<String> = meta.as_ref().map(|m| m.links.clone()).unwrap_or_default();
        for b in memory_store::Store::project_backlinks(&metas, meta.as_ref().map(|m| m.project.as_str()).unwrap_or(proj)) {
            if !related.iter().any(|r| memory_core::project::same_project(r, &b)) {
                related.push(b);
            }
        }
        let found = meta.is_some()
            || entity.is_some()
            || !current_facts.is_empty()
            || !decisions.is_empty()
            || !episodes.is_empty()
            || !principles.is_empty()
            || !lessons.is_empty();

        // Deteksi metadata Git & berkas yang sudah dihapus/dipindah di disk (bila akar repo lokal diketahui dari sesi).
        let repo_root = sessions
            .iter()
            .filter_map(|s| s.cwd.as_deref())
            .find_map(|c| memory_core::project::detect_project(std::path::Path::new(c)))
            .filter(|p| matches_proj_name(&p.name))
            .map(|p| p.root);
        let git_info = repo_root.as_deref().and_then(memory_core::project::detect_git_info);
        let repo = meta
            .as_ref()
            .and_then(|m| m.repo.clone())
            .or_else(|| repo_root.as_deref().and_then(memory_core::project::detect_git_remote));
        let stale_files = match &repo_root {
            Some(root) => {
                let mut touched: Vec<String> = Vec::new();
                for ep in ep_list.iter().take(15) {
                    for f in ep.files_modified.iter().chain(ep.files_read.iter()) {
                        if !touched.contains(f) {
                            touched.push(f.clone());
                        }
                    }
                }
                memory_core::project::check_stale_files(root, &touched)
            }
            None => vec![],
        };

        let mut ctx = String::new();
        let push = |line: &str, out: &mut String| -> bool {
            if crate::approx_tokens(out) + crate::approx_tokens(line) > budget_tokens {
                return false;
            }
            out.push_str(line);
            out.push('\n');
            true
        };
        push(&format!("# Konteks Proyek & Memori Kerja: {proj}"), &mut ctx);
        if let Some(ent) = &entity {
            let mut l = format!("Entitas: {} ({})", ent.name, ent.kind.label_id());
            if !ent.aliases.is_empty() {
                l.push_str(&format!(" | alias: {}", ent.aliases.join(", ")));
            }
            if let Some(g) = &git_info
                && (g.branch.is_some() || g.commit.is_some())
            {
                let br = g.branch.as_deref().unwrap_or("detached");
                let cm = g.commit.as_deref().unwrap_or("-");
                l.push_str(&format!(" | git: {br}@{cm}"));
            }
            if !ent.description.is_empty() {
                l.push_str(&format!("\n{}", truncate_chars(&ent.description, 300)));
            }
            push(&l, &mut ctx);
        } else if let Some(g) = &git_info
            && (g.branch.is_some() || g.commit.is_some())
        {
            let br = g.branch.as_deref().unwrap_or("detached");
            let cm = g.commit.as_deref().unwrap_or("-");
            push(&format!("Git: {br}@{cm}"), &mut ctx);
        }
        if let Some(r) = &repo {
            let short = memory_core::project::repo_short_name(r);
            let l = if short == *r { format!("Repositori: {r}") } else { format!("Repositori: {short} ({r})") };
            push(&l, &mut ctx);
        }
        if let Some(m) = meta.as_ref().filter(|m| !m.tags.is_empty()) {
            push(&format!("Grup/tag: {}", m.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ")), &mut ctx);
        }
        if !related.is_empty() {
            push(&format!("Proyek terkait: {}", related.iter().map(|r| format!("[[{r}]]")).collect::<Vec<_>>().join(", ")), &mut ctx);
        }
        if let Some(m) = meta.as_ref().filter(|m| !m.notes.trim().is_empty()) {
            push(&format!("Catatan proyek dari pengguna: {}", truncate_chars(m.notes.trim(), 600)), &mut ctx);
        }
        if !stale_files.is_empty() {
            let listed: Vec<&str> = stale_files.iter().take(6).map(String::as_str).collect();
            push(&format!("Catatan: berkas di memori lama yang sudah tidak ada di disk: {}", listed.join(", ")), &mut ctx);
        }
        if !lessons.is_empty() {
            push("\n## Konvensi & jebakan proyek (berulang)", &mut ctx);
            for l in &lessons {
                let n = if l.occurrences > 1 { format!(" (x{})", l.occurrences) } else { String::new() };
                if !push(&format!("- [{}] {}{n}", l.kind.label_id(), l.title), &mut ctx) {
                    break;
                }
            }
        }
        if !current_facts.is_empty() {
            push("\n## Fakta & arsitektur aktif (berlaku sekarang)", &mut ctx);
            for f in current_facts.iter().filter(|f| !crate::is_event_fact(&f.category)).take(25) {
                let cite = f.evidence.first().map(|e| format!(" [ep:{e}]")).unwrap_or_default();
                if !push(&format!("- [{}] {}{cite}", if f.category.is_empty() { "umum" } else { &f.category }, f.statement), &mut ctx) {
                    break;
                }
            }
        }
        if !decisions.is_empty() {
            push("\n## Keputusan teknis & penyelesaian masalah", &mut ctx);
            for d in decisions.iter() {
                let reason = d.reason.as_deref().filter(|r| !r.is_empty()).map(|r| format!(" (alasan: {r})")).unwrap_or_default();
                if !push(&format!("- {} -> {}{}", d.situation, d.action, reason), &mut ctx) {
                    break;
                }
            }
        }
        if !principles.is_empty() {
            push("\n## Prinsip kerja subjek (lintas proyek)", &mut ctx);
            for p in principles.iter().take(5) {
                if !push(&format!("- {}", p.principle.statement), &mut ctx) {
                    break;
                }
            }
        }
        if !episodes.is_empty() {
            push("\n## Indeks sesi & riwayat (ambil detail: get_episodes(ids=[...]))", &mut ctx);
            for ep in &episodes {
                let date = ep.time_start.as_deref().unwrap_or(&ep.time);
                if !push(&crate::index_line(&ep.id, date, ep.obs_type.as_deref(), None, &ep.title, ep.approx_tokens), &mut ctx) {
                    break;
                }
            }
        }
        if !history_facts.is_empty() {
            push("\n## Riwayat fakta lama (sudah digantikan)", &mut ctx);
            for f in history_facts.iter().take(8) {
                if !push(
                    &format!("- {} ({}–{})", f.statement, f.valid_from.as_deref().unwrap_or("?"), f.valid_to.as_deref().unwrap_or("?")),
                    &mut ctx,
                ) {
                    break;
                }
            }
        }
        if !found {
            push("\n(Belum ada catatan memori untuk proyek ini.)", &mut ctx);
        }

        Ok(ProjectContext {
            project: proj.to_string(),
            found,
            entity,
            current_facts,
            history_facts,
            decisions,
            principles,
            episodes,
            lessons,
            meta,
            repo,
            related,
            context: ctx,
        })
    }

    /// Jawab pertanyaan memakai memori (mode biasa atau persona).
    pub async fn ask(&self, subject: &str, question: &str, persona: bool, as_of: Option<&str>) -> Result<Answer> {
        let (system, recall) = if persona {
            let pc = self.persona_context(subject, question, as_of).await?;
            (pc.system_prompt, pc.recall)
        } else {
            let r = self.recall(&RecallQuery { as_of: as_of.map(str::to_string), ..RecallQuery::new(subject, question) }).await?;
            let sys = format!(
                "Kamu asisten memori AI terpadu (mencakup pengetahuan proyek koding/engineering, pekerjaan profesional, dan arsip personal). Jawab dalam bahasa Indonesia HANYA dari MEMORI berikut, bedakan fakta yang masih berlaku dan riwayat, dan cantumkan sitasi [ep:ID]. Bila tidak ada, katakan tidak ada catatan.\n\nMEMORI:\n{}",
                r.context
            );
            (sys, r)
        };
        if !recall.found {
            return Ok(Answer {
                answer: recall.note.clone().unwrap_or_else(|| "Tidak ada catatan.".into()),
                found: false,
                citations: vec![],
                context: recall.context,
            });
        }
        let raw = self
            .llm
            .chat(ChatRequest {
                role: ModelRole::Extract,
                messages: vec![Message::system(system), Message::user(question)],
                json_schema: None,
                temperature: Some(0.3),
                max_tokens: Some(1200),
                task: "ask".into(),
            })
            .await?;
        let answer = match raw.rfind("</think>") {
            Some(i) => raw[i + 8..].trim().to_string(),
            None => raw.trim().to_string(),
        };
        let known: HashSet<&str> = recall
            .episodes
            .iter()
            .map(|e| e.id.as_str())
            .chain(recall.facts.iter().flat_map(|f| f.evidence.iter().map(String::as_str)))
            .collect();
        let cites = citations(&answer).into_iter().filter(|c| known.contains(c.as_str())).collect();
        Ok(Answer { answer, found: true, citations: cites, context: truncate_chars(&recall.context, 20_000) })
    }
}
