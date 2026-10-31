//! Distilasi Wisdom Layer: prinsip (pola keputusan) dan disposisi (sifat/nilai)
//! dengan syarat bukti ketat:
//! - minimal `min_evidence` episode berbeda,
//! - tersebar di minimal `min_distinct_periods` periode (tahun/fase hidup),
//! - lolos verifikasi LLM kedua yang hanya melihat teks bukti.
//!
//! Emosi sesaat tidak pernah dipakai sebagai bahan disposisi.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::json;

use memory_core::ids::now_rfc3339;
use memory_core::model::*;
use memory_core::text::{normalize, truncate_chars};
use memory_llm::{ModelRole, complete_json, cosine};
use memory_store::EpisodeFilter;

use crate::llm_types::{PrincipleProposal, TextOut, Verification};
use crate::{Pipeline, Result, det_id, prompts};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DistillReport {
    pub clusters: u32,
    pub principles_new: u32,
    pub principles_updated: u32,
    pub dispositions: u32,
    pub pending_review: u32,
    pub rejected: u32,
}

fn period(e: &Episode) -> Option<String> {
    e.time_start.as_ref().map(|d| d.chars().take(4).collect()).or_else(|| e.life_phase.clone())
}

/// Pengelompokan rakus berdasarkan kosinus ke centroid.
pub fn greedy_clusters(vectors: &[Vec<f32>], threshold: f32) -> Vec<Vec<usize>> {
    let mut clusters: Vec<(Vec<f32>, Vec<usize>)> = Vec::new();
    for (i, v) in vectors.iter().enumerate() {
        let best = clusters
            .iter()
            .enumerate()
            .map(|(ci, (c, _))| (ci, cosine(c, v)))
            .filter(|(_, s)| *s >= threshold)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((ci, _)) => {
                let (c, members) = &mut clusters[ci];
                members.push(i);
                let n = members.len() as f32;
                for (x, y) in c.iter_mut().zip(v) {
                    *x = (*x * (n - 1.0) + y) / n;
                }
            }
            None => clusters.push((v.clone(), vec![i])),
        }
    }
    clusters.into_iter().map(|(_, m)| m).collect()
}

fn spread_ok(eps: &[&Episode], min_evidence: usize, min_periods: usize) -> bool {
    let ids: HashSet<&str> = eps.iter().map(|e| e.id.as_str()).collect();
    let periods: HashSet<String> = eps.iter().filter_map(|e| period(e)).collect();
    ids.len() >= min_evidence && periods.len() >= min_periods
}

fn unique<'a>(eps: &[&'a Episode]) -> Vec<&'a Episode> {
    let mut out: Vec<&Episode> = Vec::new();
    for e in eps {
        if !out.iter().any(|u| u.id == e.id) {
            out.push(e);
        }
    }
    out
}

impl Pipeline {
    pub async fn distill(&self, subject: &str) -> Result<DistillReport> {
        let eps = self
            .store
            .list_episodes(subject, &EpisodeFilter { status: Some(EpisodeStatus::Active), limit: 20_000, ..Default::default() })
            .await?;
        let mut report = DistillReport::default();
        self.distill_principles(subject, &eps, &mut report).await?;
        self.distill_dispositions(subject, &eps, &mut report).await?;
        Ok(report)
    }

    async fn verify(&self, statement: &str, evidence: &[&Episode]) -> Result<Verification> {
        let body: Vec<String> = evidence.iter().map(|e| format!("[{}] {}", e.id, truncate_chars(&e.text, 1200))).collect();
        let user = format!("PERNYATAAN: {statement}\n\nEPISODE:\n{}", body.join("\n\n"));
        Ok(complete_json(self.llm.as_ref(), ModelRole::Extract, "verify", prompts::VERIFY_SYSTEM, &user, 1).await?)
    }

    async fn review_reflective(&self, subject: &str, table: &str, id: &str, text: &str, confidence: f32) -> Result<()> {
        let r = ReviewItem {
            id: det_id(&["reflective", table, id]),
            subject: subject.to_string(),
            kind: ReviewKind::Reflective,
            summary: format!("Konfirmasi {table} (keyakinan {confidence:.2}): {text}"),
            targets: vec![id.to_string()],
            payload: json!({"table": table, "id": id}),
            status: ReviewStatus::Open,
            note: None,
            created_at: now_rfc3339(),
            resolved_at: None,
        };
        if self.store.get_review(&r.id).await?.is_none() {
            self.store.put_review(&r).await?;
        }
        Ok(())
    }

    async fn distill_principles(&self, subject: &str, eps: &[Episode], report: &mut DistillReport) -> Result<()> {
        let dc = &self.cfg.distill;
        let items: Vec<(&Episode, &Decision)> = eps.iter().flat_map(|e| e.decisions.iter().map(move |d| (e, d))).collect();
        if items.len() < dc.min_evidence {
            return Ok(());
        }
        let texts: Vec<String> = items
            .iter()
            .map(|(_, d)| format!("situasi: {} | tindakan: {} | alasan: {}", d.situation, d.action, d.reason.clone().unwrap_or_default()))
            .collect();
        let vecs = self.embedder.embed(&texts).await?;
        let existing = self.store.list_principles(subject, None).await?;
        let existing_vecs = if existing.is_empty() {
            vec![]
        } else {
            self.embedder.embed(&existing.iter().map(|p| p.statement.clone()).collect::<Vec<_>>()).await?
        };
        for members in greedy_clusters(&vecs, dc.cluster_similarity) {
            let cluster_eps: Vec<&Episode> = members.iter().map(|&i| items[i].0).collect();
            if !spread_ok(&cluster_eps, dc.min_evidence, dc.min_distinct_periods) {
                continue;
            }
            report.clusters += 1;
            let listing: Vec<String> = members
                .iter()
                .map(|&i| format!("[{}] ({}) {}", items[i].0.id, items[i].0.time.label(), texts[i]))
                .collect();
            let proposal: PrincipleProposal = match complete_json(
                self.llm.as_ref(),
                ModelRole::Extract,
                "principle",
                prompts::PRINCIPLE_SYSTEM,
                &format!("KEPUTUSAN:\n{}", listing.join("\n")),
                1,
            )
            .await
            {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, "usulan prinsip gagal");
                    continue;
                }
            };
            if proposal.statement.trim().is_empty() {
                continue;
            }
            let uniq = unique(&cluster_eps);
            let v = self.verify(&proposal.statement, &uniq).await?;
            let supported: Vec<&Episode> = uniq.iter().copied().filter(|e| v.supported_ids.iter().any(|s| s == &e.id)).collect();
            if !spread_ok(&supported, dc.min_evidence, dc.min_distinct_periods) {
                report.rejected += 1;
                continue;
            }
            let evidence: Vec<Evidence> = supported
                .iter()
                .map(|e| {
                    let d = e.decisions.first();
                    Evidence { episode_id: e.id.clone(), quote: d.and_then(|d| d.quote.clone()).unwrap_or_else(|| d.map(|d| d.action.clone()).unwrap_or_default()) }
                })
                .collect();
            let confidence = v.confidence.clamp(0.0, 1.0);
            let status = if confidence >= dc.auto_accept_confidence { ReflectiveStatus::Active } else { ReflectiveStatus::PendingReview };
            let stmt_vec = self.embedder.embed_one(&proposal.statement).await?;
            let dup = existing.iter().zip(&existing_vecs).find(|(_, ev)| cosine(ev, &stmt_vec) >= 0.85).map(|(p, _)| p.clone());
            let principle = match dup {
                Some(mut p) => {
                    for ev in evidence {
                        if !p.evidence.iter().any(|x| x.episode_id == ev.episode_id) {
                            p.evidence.push(ev);
                        }
                    }
                    p.confidence = p.confidence.max(confidence);
                    p.derived_at = now_rfc3339();
                    report.principles_updated += 1;
                    p
                }
                None => {
                    report.principles_new += 1;
                    Principle {
                        id: det_id(&[subject, "principle", &normalize(&proposal.statement)]),
                        subject: subject.to_string(),
                        statement: proposal.statement.trim().to_string(),
                        domain: proposal.domain.clone(),
                        origin: proposal.origin.clone().filter(|o| !o.trim().is_empty()),
                        confidence,
                        valid_from: supported.iter().filter_map(|e| e.time_start.clone()).min(),
                        valid_to: None,
                        evidence,
                        status,
                        derived_at: now_rfc3339(),
                    }
                }
            };
            if principle.status == ReflectiveStatus::PendingReview {
                report.pending_review += 1;
                self.review_reflective(subject, "principle", &principle.id, &principle.statement, principle.confidence).await?;
            }
            self.store.put_principle(&principle).await?;
        }
        Ok(())
    }

    async fn distill_dispositions(&self, subject: &str, eps: &[Episode], report: &mut DistillReport) -> Result<()> {
        let dc = &self.cfg.distill;
        let signals: Vec<(&Episode, &TraitSignal)> = eps.iter().flat_map(|e| e.trait_signals.iter().map(move |t| (e, t))).collect();
        if signals.is_empty() {
            return Ok(());
        }
        // Kelompokkan label: sama setelah normalisasi, atau embedding sangat mirip.
        let mut labels: Vec<String> = signals.iter().map(|(_, t)| normalize(&t.label)).collect();
        labels.sort();
        labels.dedup();
        let lvecs = self.embedder.embed(&labels).await?;
        let mut canon: BTreeMap<String, String> = BTreeMap::new();
        for g in greedy_clusters(&lvecs, 0.88) {
            let head = labels[g[0]].clone();
            for i in g {
                canon.insert(labels[i].clone(), head.clone());
            }
        }
        let mut by_label: BTreeMap<String, Vec<(&Episode, &TraitSignal)>> = BTreeMap::new();
        for (e, t) in &signals {
            let key = canon.get(&normalize(&t.label)).cloned().unwrap_or_else(|| normalize(&t.label));
            by_label.entry(key).or_default().push((e, t));
        }
        for (label, sigs) in by_label {
            let positive: Vec<&Episode> = sigs.iter().filter(|(_, t)| t.polarity >= 0).map(|(e, _)| *e).collect();
            if !spread_ok(&positive, dc.min_evidence, dc.min_distinct_periods) {
                continue;
            }
            let kind = sigs[0].1.kind;
            let statement = match kind {
                DispositionKind::Trait => format!("Orang ini memiliki sifat: {label}"),
                DispositionKind::Value => format!("Orang ini memegang nilai hidup: {label}"),
            };
            let uniq = unique(&positive);
            let v = self.verify(&statement, &uniq).await?;
            let supported: Vec<&Episode> = uniq.iter().copied().filter(|e| v.supported_ids.iter().any(|s| s == &e.id)).collect();
            if !spread_ok(&supported, dc.min_evidence, dc.min_distinct_periods) {
                report.rejected += 1;
                continue;
            }
            // Berlaku sejak bukti positif paling awal; berakhir bila ada bukti
            // negatif (sifat ditinggalkan) setelah bukti positif terakhir.
            let first_pos = supported.iter().filter_map(|e| e.time_start.clone()).min();
            let last_pos = supported.iter().filter_map(|e| e.time_start.clone()).max();
            let valid_to = sigs
                .iter()
                .filter(|(_, t)| t.polarity < 0)
                .filter_map(|(e, _)| e.time_start.clone())
                .filter(|d| last_pos.as_ref().is_some_and(|lp| d > lp))
                .min();
            let quotes: Vec<String> = sigs
                .iter()
                .filter_map(|(e, t)| t.quote.as_ref().map(|q| format!("({}{}) {q}", e.time.label(), if t.polarity < 0 { ", berlawanan" } else { "" })))
                .collect();
            let description = complete_json::<TextOut>(
                self.llm.as_ref(),
                ModelRole::Light,
                "disposition",
                prompts::DISPOSITION_SYSTEM,
                &format!("{statement}\nBUKTI:\n{}", quotes.join("\n")),
                1,
            )
            .await
            .map(|t| t.text)
            .unwrap_or_default();
            let confidence = v.confidence.clamp(0.0, 1.0);
            let id = det_id(&[subject, "disposition", &label]);
            let status = match self.store.get::<Disposition>("disposition", &id).await? {
                Some(old) if old.status != ReflectiveStatus::PendingReview => old.status,
                _ if confidence >= dc.auto_accept_confidence => ReflectiveStatus::Active,
                _ => ReflectiveStatus::PendingReview,
            };
            let d = Disposition {
                id: id.clone(),
                subject: subject.to_string(),
                kind,
                label: label.clone(),
                description,
                valid_from: first_pos,
                valid_to,
                confidence,
                evidence: supported
                    .iter()
                    .map(|e| Evidence {
                        episode_id: e.id.clone(),
                        quote: sigs.iter().find(|(x, _)| x.id == e.id).and_then(|(_, t)| t.quote.clone()).unwrap_or_default(),
                    })
                    .collect(),
                superseded_by: None,
                status,
                derived_at: now_rfc3339(),
            };
            if d.status == ReflectiveStatus::PendingReview {
                report.pending_review += 1;
                self.review_reflective(subject, "disposition", &id, &label, confidence).await?;
            }
            self.store.put_disposition(&d).await?;
            report.dispositions += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_group_similar_vectors() {
        let v = vec![vec![1.0, 0.0], vec![0.98, 0.05], vec![0.0, 1.0], vec![0.05, 0.99]];
        assert_eq!(greedy_clusters(&v, 0.9), vec![vec![0, 1], vec![2, 3]]);
    }
}
