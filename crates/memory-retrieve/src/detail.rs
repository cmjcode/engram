//! Lapis detail pengambilan bertahap: `get_episodes(ids)` mengembalikan teks penuh,
//! fakta, keputusan, berkas, dan asal sumber untuk episode yang dipilih agent dari
//! indeks `recall`/`timeline`/konteks awal sesi.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use memory_core::model::*;

use crate::recall::{FactView, fact_view};
use crate::{Result, Retriever};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EpisodeDetail {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub text: String,
    pub time: String,
    pub time_start: Option<String>,
    pub project: Option<String>,
    pub obs_type: Option<String>,
    pub concepts: Vec<String>,
    pub files_read: Vec<String>,
    pub files_modified: Vec<String>,
    pub decisions: Vec<Decision>,
    pub quotes: Vec<String>,
    pub facts: Vec<FactView>,
    pub lessons: Vec<Lesson>,
    pub entities: Vec<String>,
    pub session_id: Option<String>,
    pub prompt_number: Option<u32>,
    pub source_id: String,
    pub source_title: String,
    pub source_kind: Option<SourceKind>,
    pub retell_count: u32,
    pub status: EpisodeStatus,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct EpisodesDetail {
    pub episodes: Vec<EpisodeDetail>,
    /// ID yang diminta tetapi tidak ada / bukan milik subjek ini.
    pub missing: Vec<String>,
    /// Teks siap pakai untuk agent.
    pub context: String,
}

impl Retriever {
    pub async fn get_episodes(&self, subject: &str, ids: &[String], budget_tokens: usize) -> Result<EpisodesDetail> {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let ids: Vec<String> = ids.iter().map(|i| i.trim().trim_start_matches("ep:").to_string()).filter(|i| memory_core::ids::is_valid_id(i)).take(25).collect();
        let eps = self.store.episodes_by_ids(&ids).await?;
        let mut out = EpisodesDetail::default();
        let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for id in &ids {
            if !eps.iter().any(|e| &e.id == id && e.subject == subject) {
                out.missing.push(id.clone());
            }
        }
        for e in eps.into_iter().filter(|e| e.subject == subject) {
            let mut facts = Vec::new();
            for f in self.store.facts_by_episode(&e.id).await? {
                if matches!(f.status, FactStatus::Rejected) {
                    continue;
                }
                let n = match names.get(&f.entity_id) {
                    Some(n) => n.clone(),
                    None => {
                        let n = self.store.get_entity(&f.entity_id).await?.map(|x| x.name).unwrap_or_default();
                        names.insert(f.entity_id.clone(), n.clone());
                        n
                    }
                };
                facts.push(fact_view(&f, &n, &today));
            }
            let mut entities = Vec::new();
            for eid in e.entity_ids.iter().take(20) {
                if let Some(en) = self.store.resolve_entity(eid).await? {
                    entities.push(en.name);
                }
            }
            let lessons: Vec<Lesson> = self
                .store
                .list_lessons(subject, &[], Some("active"), 200)
                .await?
                .into_iter()
                .filter(|l| l.evidence.iter().any(|ev| ev.episode_id == e.id))
                .collect();
            let src = self.store.get_source(&e.source_id).await?;
            out.episodes.push(EpisodeDetail {
                id: e.id.clone(),
                title: e.title.clone(),
                summary: e.summary.clone(),
                text: e.text.clone(),
                time: e.time_label(),
                time_start: e.time_start.clone(),
                project: e.project.clone(),
                obs_type: e.obs_type.clone(),
                concepts: e.concepts.clone(),
                files_read: e.files_read.clone(),
                files_modified: e.files_modified.clone(),
                decisions: e.decisions.clone(),
                quotes: e.quotes.clone(),
                facts,
                lessons,
                entities,
                session_id: e.session_id.clone(),
                prompt_number: e.prompt_number,
                source_id: e.source_id.clone(),
                source_title: src.as_ref().map(|s| s.title.clone()).unwrap_or_default(),
                source_kind: src.as_ref().map(|s| s.kind),
                retell_count: e.retell_count,
                status: e.status,
                created_at: e.created_at.clone(),
            });
        }
        out.context = render_details(&out, budget_tokens);
        Ok(out)
    }
}

fn render_details(d: &EpisodesDetail, budget_tokens: usize) -> String {
    let mut out = String::new();
    let per = if d.episodes.is_empty() { budget_tokens } else { (budget_tokens / d.episodes.len()).max(150) };
    for e in &d.episodes {
        let mut block = format!("## [ep:{}] {} ({})\n", e.id, if e.title.is_empty() { "(tanpa judul)" } else { &e.title }, e.time);
        let mut meta = Vec::new();
        if let Some(p) = &e.project {
            meta.push(format!("proyek: {p}"));
        }
        if let Some(t) = &e.obs_type {
            meta.push(format!("jenis: {}", obs_type_label(Some(t))));
        }
        if !e.concepts.is_empty() {
            meta.push(format!("konsep: {}", e.concepts.join(", ")));
        }
        if e.retell_count > 0 {
            meta.push(format!("diceritakan {}x", e.retell_count + 1));
        }
        if !meta.is_empty() {
            block.push_str(&format!("{}\n", meta.join(" | ")));
        }
        if !e.summary.is_empty() {
            block.push_str(&format!("Ringkasan: {}\n", e.summary));
        }
        if !e.files_modified.is_empty() {
            block.push_str(&format!("Berkas diubah: {}\n", e.files_modified.join(", ")));
        }
        if !e.files_read.is_empty() {
            block.push_str(&format!("Berkas dibaca: {}\n", e.files_read.iter().take(10).cloned().collect::<Vec<_>>().join(", ")));
        }
        for dcs in &e.decisions {
            let reason = dcs.reason.as_deref().filter(|r| !r.is_empty()).map(|r| format!(" (alasan: {r})")).unwrap_or_default();
            block.push_str(&format!("Keputusan: {} -> {}{reason}\n", dcs.situation, dcs.action));
        }
        for f in &e.facts {
            block.push_str(&format!("Fakta: {} ({})\n", f.statement, if f.current { "berlaku sekarang" } else { "riwayat" }));
        }
        for l in &e.lessons {
            block.push_str(&format!("Pelajaran [{}]: {}\n", l.kind.label_id(), l.title));
        }
        block.push_str("Teks:\n");
        let used = block.chars().count() / 4;
        let room = per.saturating_sub(used).max(60) * 4;
        block.push_str(&memory_core::text::truncate_chars(&e.text, room));
        block.push_str("\n\n");
        out.push_str(&block);
    }
    if !d.missing.is_empty() {
        out.push_str(&format!("(tidak ditemukan: {})\n", d.missing.join(", ")));
    }
    if out.trim().is_empty() {
        out.push_str("Tidak ada episode dengan ID tersebut.");
    }
    out
}
