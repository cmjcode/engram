//! Konsolidasi ("tidur"): working memory meluruh atau naik jadi episode bila
//! kemudian terbukti penting; episode dikelompokkan menjadi ringkasan fase hidup.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use memory_core::ids::now_rfc3339;
use memory_core::model::{Episode, EpisodeStatus, LifePhase};
use memory_core::score::decayed_importance;
use memory_core::text::{normalize, truncate_chars};
use memory_llm::{ModelRole, complete_json};
use memory_store::EpisodeFilter;

use crate::llm_types::TextOut;
use crate::{Pipeline, Result, det_id, prompts};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsolidateReport {
    pub expired: u64,
    pub promoted: u32,
    pub phases_updated: u32,
}

impl Pipeline {
    pub async fn consolidate(&self, subject: &str) -> Result<ConsolidateReport> {
        let mut report = ConsolidateReport { expired: self.store.delete_expired_stream(&now_rfc3339()).await?, ..Default::default() };
        let items = self.store.stream_items(subject, true, 5000).await?;
        let keys = self.known_alias_keys(subject).await?;
        let now = chrono::Utc::now();
        for item in items {
            let padded = format!(" {} ", normalize(&item.text));
            let refs = keys.iter().filter(|k| padded.contains(&format!(" {k} "))).count() as u32;
            let new_refs = refs.saturating_sub(item.back_refs);
            let age_days = chrono::DateTime::parse_from_rfc3339(&item.created_at)
                .map(|t| (now - t.with_timezone(&chrono::Utc)).num_hours() as f64 / 24.0)
                .unwrap_or(0.0);
            let imp = decayed_importance(item.importance, age_days, self.cfg.score.half_life_days, new_refs);
            if new_refs > 0 && imp >= self.cfg.score.promote_threshold {
                match self.promote_stream_item(&item.id).await {
                    Ok(Some(_)) => report.promoted += 1,
                    Ok(None) => {}
                    Err(e) => tracing::warn!(item = %item.id, error = %e, "promosi stream item gagal"),
                }
            }
        }
        report.phases_updated = self.update_life_phases(subject).await?;
        Ok(report)
    }

    /// Ringkas ulang fase hidup yang daftar episodenya berubah.
    pub async fn update_life_phases(&self, subject: &str) -> Result<u32> {
        let eps = self
            .store
            .list_episodes(subject, &EpisodeFilter { status: Some(EpisodeStatus::Active), limit: 20_000, ..Default::default() })
            .await?;
        let mut groups: BTreeMap<String, Vec<Episode>> = BTreeMap::new();
        for e in eps {
            if let Some(p) = e.life_phase.clone().filter(|p| !p.trim().is_empty()) {
                groups.entry(p).or_default().push(e);
            }
        }
        let existing = self.store.list_life_phases(subject).await?;
        let mut updated = 0;
        for (name, mut list) in groups {
            let mut ids: Vec<String> = list.iter().map(|e| e.id.clone()).collect();
            ids.sort();
            let unchanged = existing.iter().any(|p| {
                let mut x = p.episode_ids.clone();
                x.sort();
                p.name == name && x == ids
            });
            if unchanged {
                continue;
            }
            list.sort_by(|a, b| a.time_start.cmp(&b.time_start));
            let mut ranked = list.clone();
            ranked.sort_by(|a, b| b.importance.total_cmp(&a.importance));
            ranked.truncate(60);
            ranked.sort_by(|a, b| a.time_start.cmp(&b.time_start));
            let lines: Vec<String> = ranked
                .iter()
                .map(|e| format!("- [{}] {}: {}", e.time.label(), e.title, truncate_chars(if e.summary.is_empty() { &e.text } else { &e.summary }, 300)))
                .collect();
            let user = format!("Fase hidup: {name}\nEpisode:\n{}", lines.join("\n"));
            let summary = match complete_json::<TextOut>(self.llm.as_ref(), ModelRole::Extract, "life_phase", prompts::LIFE_PHASE_SYSTEM, &user, 1).await {
                Ok(t) => t.text,
                Err(e) => {
                    tracing::warn!(error = %e, phase = %name, "ringkasan fase gagal; memakai daftar judul");
                    ranked.iter().map(|e| e.title.clone()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("; ")
                }
            };
            let phase = LifePhase {
                id: det_id(&[subject, "phase", &name]),
                subject: subject.to_string(),
                start: list.iter().filter_map(|e| e.time_start.clone()).min(),
                end: list.iter().filter_map(|e| e.time_end.clone()).max(),
                name,
                summary,
                episode_ids: ids,
                updated_at: now_rfc3339(),
            };
            self.store.put_life_phase(&phase).await?;
            updated += 1;
        }
        Ok(updated)
    }
}
