//! Orkestrator memori: pipeline ekstraksi dan pekerjaan reflektif.
//!
//! Alur per sumber (lihat [`process`]):
//! Ingest → Segment → Score → Extract → Resolve → Reconcile → Merge → Commit.
//! Pekerjaan terjadwal: [`consolidate`] (working memory, fase hidup),
//! [`distill`] (prinsip & disposisi dengan verifikasi), [`voice`] (gaya tutur).

pub mod compact;
pub mod consolidate;
pub mod distill;
pub mod ingest;
pub mod interview;
pub mod llm_types;
pub mod process;
pub mod prompts;
pub mod reconcile;
pub mod resolve;
pub mod review;
pub mod segment;
pub mod session;
pub mod vault;
pub mod voice;

use std::collections::HashMap;
use std::sync::Arc;

use memory_core::model::{Entity, EntityKind, EntityStatus, Subject};
use memory_core::{MemoryConfig, new_id};
use memory_llm::{Embedder, LlmProvider};
use memory_store::Store;
use tokio::sync::Mutex;

pub use vault::Vault;

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error(transparent)]
    Store(#[from] memory_store::StoreError),
    #[error(transparent)]
    Llm(#[from] memory_llm::LlmError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, ExtractError>;

/// Konteks bersama semua tahap pipeline.
#[derive(Clone)]
pub struct Pipeline {
    pub store: Store,
    pub llm: Arc<dyn LlmProvider>,
    pub embedder: Arc<dyn Embedder>,
    pub cfg: Arc<MemoryConfig>,
    pub vault: Vault,
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl Pipeline {
    pub fn new(store: Store, llm: Arc<dyn LlmProvider>, embedder: Arc<dyn Embedder>, cfg: Arc<MemoryConfig>, vault: Vault) -> Self {
        Self { store, llm, embedder, cfg, vault, locks: Arc::default() }
    }

    /// Kunci per subjek: resolusi entitas & commit untuk satu subjek berjalan
    /// berurutan agar tidak lahir entitas kembar dari dua job paralel.
    pub async fn subject_lock(&self, subject: &str) -> Arc<Mutex<()>> {
        let mut map = self.locks.lock().await;
        map.entry(subject.to_string()).or_default().clone()
    }

    /// Pastikan subjek dan entitas "diri"-nya ada.
    pub async fn ensure_subject(&self, id: &str, name: Option<&str>) -> Result<Subject> {
        if !memory_core::ids::is_valid_id(id) {
            return Err(ExtractError::Invalid(format!("id subjek tidak valid: {id}")));
        }
        let mut subject = match self.store.get_subject(id).await? {
            Some(s) => s,
            None => Subject {
                id: id.to_string(),
                name: name.unwrap_or(id).to_string(),
                anchors: Default::default(),
                gender: None,
                self_entity: None,
                created_at: memory_core::ids::now_rfc3339(),
            },
        };
        if let Some(n) = name {
            subject.name = n.to_string();
        }
        if subject.self_entity.is_none() {
            let e = Entity {
                id: new_id(),
                subject: id.to_string(),
                kind: EntityKind::Person,
                name: subject.name.clone(),
                aliases: vec![],
                description: "subjek memori (diri sendiri)".into(),
                status: EntityStatus::Confirmed,
                merged_into: None,
                is_self: true,
                gender: subject.gender,
                mention_count: 0,
                episode_ids: vec![],
                created_at: memory_core::ids::now_rfc3339(),
            };
            // Membuat subjek tidak boleh gagal hanya karena server embedding mati.
            // Tanpa vektor, entitas tetap tersimpan; `memory reembed` melengkapinya nanti.
            let emb = match self.embedder.embed_one(&resolve::entity_embed_text(&e)).await {
                Ok(v) => Some(v),
                Err(err) => {
                    tracing::warn!(subject = %id, error = %err, "embedding entitas diri gagal; disimpan tanpa embedding");
                    None
                }
            };
            self.store.put_entity(&e, emb.as_deref()).await?;
            subject.self_entity = Some(e.id);
        }
        self.store.put_subject(&subject).await?;
        Ok(subject)
    }
}

/// ID deterministik dari beberapa bagian (untuk idempotensi saat job diulang).
pub fn det_id(parts: &[&str]) -> String {
    let h = blake3::hash(parts.join("\u{1f}").as_bytes());
    h.to_hex()[..26].to_string()
}

pub fn today_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

#[cfg(test)]
pub(crate) mod testkit;
#[cfg(test)]
mod pipeline_tests;
