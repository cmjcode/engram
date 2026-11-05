//! Resolusi entitas: sebutan di teks -> entitas di knowledge graph.
//!
//! Urutan: kata ganti diri -> istilah kekerabatan (pakai edge kin subjek) ->
//! alias persis -> kemiripan embedding (gabung otomatis hanya ke entitas
//! confirmed) -> adjudikasi LLM untuk zona ragu -> entitas provisional baru.
//! Sebutan yang ragu tidak pernah digabung diam-diam: lahir entitas baru
//! plus item review `EntityMerge`.

use std::collections::HashMap;

use memory_core::kin::normalize_kin_term;
use memory_core::model::*;
use memory_core::text::{are_distinct_technical_names, normalize};
use memory_core::{ids::now_rfc3339, new_id};
use memory_llm::{ModelRole, complete_json};
use memory_store::VectorIndex;
use serde_json::json;

use crate::llm_types::SameEntityVerdict;
use crate::{Pipeline, Result, prompts};

const SELF_WORDS: &[&str] = &["saya", "aku", "abdi", "kula", "dalem", "gue", "gua", "ane", "i", "me", "kulo", "ingsun"];

pub fn entity_embed_text(e: &Entity) -> String {
    let mut s = format!("{} ({:?})", e.name, e.kind);
    if !e.aliases.is_empty() {
        s.push_str(&format!(" alias: {}", e.aliases.join(", ")));
    }
    if !e.description.is_empty() {
        s.push_str(&format!(" — {}", e.description));
    }
    s
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Adjudication {
    Same,
    Different,
    Uncertain,
}

/// Entitas yang disentuh dalam satu episode.
#[derive(Debug, Clone)]
pub struct Touched {
    pub entity: Entity,
    pub is_new: bool,
    pub changed: bool,
}

pub struct Resolver<'a> {
    p: &'a Pipeline,
    subject: &'a Subject,
    self_id: String,
    cache: HashMap<String, String>,
    pub touched: HashMap<String, Touched>,
    pub reviews: Vec<ReviewItem>,
    kin: Vec<Kin>,
}

impl<'a> Resolver<'a> {
    pub async fn new(p: &'a Pipeline, subject: &'a Subject) -> Result<Self> {
        let self_id = subject.self_entity.clone().unwrap_or_default();
        let kin = p.store.kin_for_subject(&subject.id).await?;
        Ok(Self { p, subject, self_id, cache: HashMap::new(), touched: HashMap::new(), reviews: vec![], kin })
    }

    pub fn self_id(&self) -> &str {
        &self.self_id
    }

    /// Tambahkan edge kin yang baru dibuat agar sebutan berikutnya bisa memakainya.
    pub fn note_kin(&mut self, k: Kin) {
        self.kin.push(k);
    }

    fn is_self_name(&self, key: &str) -> bool {
        SELF_WORDS.contains(&key) || key == normalize(&self.subject.name)
    }

    async fn load(&mut self, id: &str) -> Result<Option<Entity>> {
        if let Some(t) = self.touched.get(id) {
            return Ok(Some(t.entity.clone()));
        }
        Ok(self.p.store.resolve_entity(id).await?)
    }

    fn touch(&mut self, entity: Entity, is_new: bool, changed: bool) -> String {
        let id = entity.id.clone();
        let entry = self.touched.entry(id.clone()).or_insert(Touched { entity: entity.clone(), is_new, changed });
        entry.entity = entity;
        entry.is_new |= is_new;
        entry.changed |= changed;
        id
    }

    fn add_aliases(e: &mut Entity, names: &[&str]) -> bool {
        let mut changed = false;
        for n in names {
            let n = n.trim();
            if n.is_empty() || normalize(n) == normalize(&e.name) {
                continue;
            }
            if !e.aliases.iter().any(|a| normalize(a) == normalize(n)) {
                e.aliases.push(n.to_string());
                changed = true;
            }
        }
        changed
    }

    /// Cari entitas lewat edge kin ke subjek ("Pakde" -> satu-satunya pakde).
    fn by_kin_term(&self, name: &str) -> Option<String> {
        let term = normalize_kin_term(name)?;
        // Hanya bila seluruh sebutan adalah istilah kekerabatan (tanpa nama diri).
        let toks: Vec<String> = normalize(name).split_whitespace().map(str::to_string).collect();
        if toks.len() > 2 {
            return None;
        }
        let matches: Vec<&Kin> = self
            .kin
            .iter()
            .filter(|k| k.to == self.self_id && k.relation == term.relation)
            .filter(|k| term.gender.is_none() || k.gender.is_none() || k.gender == term.gender)
            .filter(|k| term.older.is_none() || k.older.is_none() || k.older == term.older)
            .collect();
        let mut ids: Vec<&str> = matches.iter().map(|k| k.from.as_str()).collect();
        ids.sort();
        ids.dedup();
        (ids.len() == 1).then(|| ids[0].to_string())
    }

    /// Resolusi satu sebutan. `episode_id` dicatat sebagai bukti sebutan.
    pub async fn resolve(&mut self, name: &str, kind: EntityKind, aliases: &[String], description: &str, is_self: bool, episode_id: &str) -> Result<String> {
        let key = normalize(name);
        if key.is_empty() {
            return Ok(self.self_id.clone());
        }
        if let Some(id) = self.cache.get(&key) {
            return Ok(id.clone());
        }
        let id = self.resolve_uncached(name, kind, aliases, description, is_self, episode_id).await?;
        self.cache.insert(key, id.clone());
        for a in aliases {
            self.cache.insert(normalize(a), id.clone());
        }
        Ok(id)
    }

    async fn resolve_uncached(&mut self, name: &str, kind: EntityKind, aliases: &[String], description: &str, is_self: bool, episode_id: &str) -> Result<String> {
        let key = normalize(name);
        let mut names: Vec<&str> = vec![name];
        names.extend(aliases.iter().map(String::as_str));

        // 1. Diri sendiri.
        if is_self || self.is_self_name(&key) {
            let id = self.self_id.clone();
            if let Some(mut e) = self.load(&id).await? {
                let changed = !is_self_word(&key) && Self::add_aliases(&mut e, &names);
                self.touch(e, false, changed);
            }
            return Ok(id);
        }

        // 2. Istilah kekerabatan.
        if kind == EntityKind::Person
            && let Some(id) = self.by_kin_term(name)
                && let Some(e) = self.load(&id).await? {
                    return Ok(self.touch(e, false, false));
                }

        // 3. Alias persis (termasuk entitas yang baru dibuat di episode ini).
        for t in self.touched.values() {
            let keys = memory_store::alias_keys(&t.entity.name, &t.entity.aliases);
            if names.iter().any(|n| keys.contains(&normalize(n))) && t.entity.kind == kind {
                let mut e = t.entity.clone();
                let changed = Self::add_aliases(&mut e, &names);
                return Ok(self.touch(e, false, changed));
            }
        }
        let mut candidates: Vec<Entity> = Vec::new();
        for n in &names {
            for e in self.p.store.entities_by_alias(&self.subject.id, n).await? {
                if !candidates.iter().any(|c| c.id == e.id) {
                    candidates.push(e);
                }
            }
        }
        candidates.retain(|c| c.kind == kind && !c.is_self);
        if candidates.len() == 1 {
            let mut e = candidates.remove(0);
            let changed = Self::add_aliases(&mut e, &names);
            return Ok(self.touch(e, false, changed));
        }

        // 4. Embedding.
        let probe = Entity {
            id: new_id(),
            subject: self.subject.id.clone(),
            kind,
            name: name.trim().to_string(),
            aliases: aliases.iter().filter(|a| normalize(a) != key).cloned().collect(),
            description: description.to_string(),
            status: EntityStatus::Provisional,
            merged_into: None,
            is_self: false,
            gender: normalize_kin_term(name).and_then(|t| t.gender),
            mention_count: 0,
            episode_ids: vec![],
            created_at: now_rfc3339(),
        };
        let hits = match self.p.embedder.embed_one(&entity_embed_text(&probe)).await {
            Ok(emb) => self.p.store.vector_index().search("entity", &self.subject.id, &emb, 3, "status != 'merged' AND is_self != true").await?,
            Err(e) => {
                tracing::warn!(error = %e, "embedding entitas probe gagal; dilewati");
                vec![]
            }
        };
        let pc = &self.p.cfg.pipeline;
        for hit in hits {
            if hit.similarity < pc.entity_review {
                break;
            }
            let Some(cand) = self.load(&hit.id).await? else { continue };
            if cand.kind != kind {
                continue;
            }
            if !candidates.is_empty() && !candidates.iter().any(|c| c.id == cand.id) {
                continue;
            }
            // Guardrail teknis: jangan pernah menggabungkan atau meminta review untuk dua
            // microservice/repo/file/komponen yang secara struktural berbeda.
            if are_distinct_technical_names(&probe.name, &cand.name) {
                continue;
            }
            let auto = hit.similarity >= pc.entity_auto_merge
                && cand.status == EntityStatus::Confirmed
                && !kind.is_technical();
            let verdict = if auto {
                Adjudication::Same
            } else {
                self.adjudicate(&probe, &cand).await
            };
            match verdict {
                Adjudication::Same => {
                    let mut e = cand;
                    let changed = Self::add_aliases(&mut e, &names);
                    return Ok(self.touch(e, false, changed));
                }
                Adjudication::Different => {
                    // LLM yakin keduanya berbeda -> jangan kotori antrean review manusia.
                    continue;
                }
                Adjudication::Uncertain => {
                    // Ragu: buat entitas baru, minta manusia memutuskan.
                    let summary = if kind == EntityKind::Person {
                        format!(
                            "Apakah \"{}\" sama dengan \"{}\"? (kemiripan {:.2})",
                            probe.name, cand.name, hit.similarity
                        )
                    } else {
                        format!(
                            "Apakah {} \"{}\" sama dengan \"{}\"? (kemiripan {:.2})",
                            kind.merge_prompt_noun(),
                            probe.name,
                            cand.name,
                            hit.similarity
                        )
                    };
                    self.reviews.push(ReviewItem {
                        id: crate::det_id(&["merge", &probe.name, &cand.id, episode_id]),
                        subject: self.subject.id.clone(),
                        kind: ReviewKind::EntityMerge,
                        summary,
                        targets: vec![probe.id.clone(), cand.id.clone()],
                        payload: json!({
                            "new": probe.id,
                            "existing": cand.id,
                            "similarity": hit.similarity,
                            "entity_kind": kind,
                        }),
                        status: ReviewStatus::Open,
                        note: None,
                        created_at: now_rfc3339(),
                        resolved_at: None,
                    });
                    break;
                }
            }
        }
        // Bila ada >1 kandidat alias tapi tidak ada yang cocok embedding, ambil
        // yang paling sering disebut (alias persis tetap sinyal kuat).
        if let Some(best) = candidates.into_iter().max_by_key(|c| c.mention_count) {
            return Ok(self.touch(best, false, false));
        }
        Ok(self.touch(probe, true, true))
    }

    async fn adjudicate(&self, a: &Entity, b: &Entity) -> Adjudication {
        let user = format!(
            "Subjek: {}\nSebutan A: {}\nSebutan B: {}",
            self.subject.name,
            entity_embed_text(a),
            entity_embed_text(b)
        );
        match complete_json::<SameEntityVerdict>(
            self.p.llm.as_ref(),
            ModelRole::Light,
            "same_entity",
            prompts::SAME_ENTITY_SYSTEM,
            &user,
            1,
        )
        .await
        {
            Ok(v) if v.same && v.confidence >= 0.8 => Adjudication::Same,
            Ok(v) if !v.same && v.confidence >= 0.75 => Adjudication::Different,
            Ok(_) => Adjudication::Uncertain,
            Err(e) => {
                tracing::warn!(error = %e, "adjudikasi entitas gagal; masuk antrean review");
                Adjudication::Uncertain
            }
        }
    }

    /// Catat sebutan di episode: mention_count, episode_ids, dan konfirmasi
    /// otomatis entitas provisional yang muncul di episode kedua.
    pub fn record_mentions(&mut self, episode_id: &str) {
        for t in self.touched.values_mut() {
            if !t.entity.episode_ids.iter().any(|e| e == episode_id) {
                t.entity.episode_ids.push(episode_id.to_string());
                t.entity.mention_count += 1;
                t.changed = true;
            }
            if t.entity.status == EntityStatus::Provisional && t.entity.episode_ids.len() >= 2 {
                t.entity.status = EntityStatus::Confirmed;
            }
        }
    }
}

fn is_self_word(key: &str) -> bool {
    SELF_WORDS.contains(&key)
}
