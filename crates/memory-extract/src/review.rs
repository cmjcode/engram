//! Menerapkan keputusan review manusia. Semua keputusan reversibel di
//! tingkat data: tidak ada record yang dihapus, hanya status yang berubah.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use memory_core::ids::now_rfc3339;
use memory_core::model::*;
use memory_core::text::are_distinct_technical_names;

use crate::{ExtractError, Pipeline, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approve,
    Reject,
}

fn day_before(iso: &str) -> Option<String> {
    let d = chrono::NaiveDate::parse_from_str(iso.get(..10)?, "%Y-%m-%d").ok()?;
    d.pred_opt().map(|p| p.format("%Y-%m-%d").to_string())
}

impl Pipeline {
    pub async fn resolve_review(&self, review_id: &str, decision: Decision, note: Option<String>) -> Result<ReviewItem> {
        let mut r = self.store.get_review(review_id).await?.ok_or_else(|| ExtractError::Invalid(format!("review {review_id} tidak ada")))?;
        if r.status != ReviewStatus::Open {
            return Ok(r);
        }
        let s = |k: &str| r.payload[k].as_str().map(str::to_string);
        match (r.kind, decision) {
            (ReviewKind::GrayZone, Decision::Approve) => {
                if let Some(item) = s("stream_item") {
                    self.promote_stream_item(&item).await?;
                }
            }
            (ReviewKind::GrayZone, Decision::Reject) => {}
            (ReviewKind::Conflict, d) => {
                let (Some(old_id), Some(new_id)) = (s("existing"), s("new")) else { return Err(ExtractError::Invalid("payload konflik rusak".into())) };
                let mut old: Fact = self.store.require("fact", &old_id).await?;
                let mut new: Fact = self.store.require("fact", &new_id).await?;
                old.status = FactStatus::Active;
                if d == Decision::Approve {
                    // Fakta baru menggantikan yang lama mulai awal berlakunya
                    // (atau sejak diceritakan bila awalnya tidak diketahui).
                    let start = new.valid_from.clone().unwrap_or_else(|| new.observed_at.chars().take(10).collect());
                    if new.valid_from.is_none() {
                        new.valid_from = Some(start.clone());
                    }
                    old.valid_to = day_before(&start).or(Some(start));
                    old.superseded_by = Some(new.id.clone());
                    new.supersedes = Some(old.id.clone());
                    new.status = FactStatus::Active;
                } else {
                    new.status = FactStatus::Rejected;
                }
                self.store.put_fact(&old).await?;
                self.store.put_fact(&new).await?;
            }
            (ReviewKind::EntityMerge, Decision::Approve) => {
                let (Some(from), Some(into)) = (s("new"), s("existing")) else { return Err(ExtractError::Invalid("payload merge rusak".into())) };
                self.merge_entities(&from, &into).await?;
            }
            (ReviewKind::EntityMerge, Decision::Reject) => {
                // Dikonfirmasi sebagai entitas berbeda.
                if let Some(id) = s("new") {
                    self.store.patch("entity", &id, json!({"status": "confirmed"})).await?;
                }
            }
            (ReviewKind::Quarantine, d) => {
                if let Some(id) = s("fact") {
                    let status = if d == Decision::Approve { "active" } else { "rejected" };
                    self.store.patch("fact", &id, json!({"status": status})).await?;
                }
            }
            (ReviewKind::Reflective, d) => {
                if let (Some(table), Some(id)) = (s("table"), s("id")) {
                    if !matches!(table.as_str(), "principle" | "disposition") {
                        return Err(ExtractError::Invalid(format!("tabel reflektif tidak dikenal: {table}")));
                    }
                    let status = if d == Decision::Approve { "active" } else { "rejected" };
                    self.store.patch(&table, &id, json!({"status": status})).await?;
                }
            }
        }
        r.status = if decision == Decision::Approve { ReviewStatus::Approved } else { ReviewStatus::Rejected };
        r.note = note;
        r.resolved_at = Some(now_rfc3339());
        self.store.put_review(&r).await?;
        Ok(r)
    }

    /// Gabungkan entitas `from` ke `into`: alias, sebutan, fakta, kekerabatan.
    pub async fn merge_entities(&self, from: &str, into: &str) -> Result<()> {
        if from == into {
            return Ok(());
        }
        let lock = self.subject_lock("__merge__").await;
        let _g = lock.lock().await;
        let mut a: Entity = self.store.get_entity(from).await?.ok_or_else(|| ExtractError::Invalid(format!("entitas {from} tidak ada")))?;
        let mut b: Entity = self.store.get_entity(into).await?.ok_or_else(|| ExtractError::Invalid(format!("entitas {into} tidak ada")))?;
        for n in std::iter::once(a.name.clone()).chain(a.aliases.clone()) {
            if n != b.name && !b.aliases.contains(&n) {
                b.aliases.push(n);
            }
        }
        for e in &a.episode_ids {
            if !b.episode_ids.contains(e) {
                b.episode_ids.push(e.clone());
            }
        }
        b.mention_count += a.mention_count;
        b.status = EntityStatus::Confirmed;
        if b.gender.is_none() {
            b.gender = a.gender;
        }
        if b.description.is_empty() {
            b.description = a.description.clone();
        }
        a.status = EntityStatus::Merged;
        a.merged_into = Some(b.id.clone());
        let emb = self.embedder.embed_one(&crate::resolve::entity_embed_text(&b)).await.ok();
        self.store.put_entity(&b, emb.as_deref()).await?;
        self.store.put_entity(&a, None).await?;
        self.store.repoint_entity(from, into).await?;
        Ok(())
    }

    /// Perbaiki otomatis entitas teknis (`Project`/`Component`) yang sempat tersimpan
    /// sebagai `Person` karena fallback lama, dan tolak otomatis item review `EntityMerge`
    /// yang membandingkan dua identifier teknis/microservice/berkas berbeda.
    pub async fn sanitize_technical_entities(&self, subject: &str) -> Result<(u32, u32)> {
        let mut reclassified = 0u32;
        let persons = self
            .store
            .list_entities(subject, Some(EntityKind::Person), 2000, 0)
            .await?;
        for e in persons {
            if e.is_self {
                continue;
            }
            if let Some(new_kind) = EntityKind::infer_from_name(&e.name)
                && new_kind != EntityKind::Person
            {
                self.store
                    .patch("entity", &e.id, json!({"kind": new_kind}))
                    .await?;
                reclassified += 1;
            }
        }

        let mut dismissed = 0u32;
        let open_merges = self
            .store
            .list_reviews(subject, ReviewStatus::Open, Some(ReviewKind::EntityMerge), 500)
            .await?;
        for r in open_merges {
            let new_id = r.payload["new"]
                .as_str()
                .or_else(|| r.targets.first().map(String::as_str))
                .unwrap_or("");
            let existing_id = r.payload["existing"]
                .as_str()
                .or_else(|| r.targets.get(1).map(String::as_str))
                .unwrap_or("");
            if new_id.is_empty() || existing_id.is_empty() {
                continue;
            }
            let a = self.store.get_entity(new_id).await?;
            let b = self.store.get_entity(existing_id).await?;
            let should_reject = match (a, b) {
                (Some(ea), Some(eb)) => {
                    ea.kind != eb.kind || are_distinct_technical_names(&ea.name, &eb.name)
                }
                _ => false,
            };
            if should_reject {
                let _ = self
                    .resolve_review(
                        &r.id,
                        Decision::Reject,
                        Some("Ditolak otomatis: entitas teknis/proyek/berkas berbeda".into()),
                    )
                    .await?;
                dismissed += 1;
            }
        }
        Ok((reclassified, dismissed))
    }
}
