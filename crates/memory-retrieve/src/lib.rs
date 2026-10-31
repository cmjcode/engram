//! Sisi baca memori. Semua tool yang dipanggil agent (MCP/HTTP) ada di sini:
//! [`Retriever::recall`], [`Retriever::who_is`], [`Retriever::timeline`],
//! [`Retriever::principles`], [`Retriever::persona_context`], [`Retriever::ask`].
//!
//! Logika temporal & guardrail ada di server, bukan di LLM klien:
//! - tanpa jendela waktu: fakta yang masih berlaku ditandai "berlaku sekarang",
//!   fakta yang sudah ditutup ditampilkan sebagai riwayat;
//! - dengan jendela `from..to`: hanya memori yang rentangnya memotong jendela;
//! - dengan `as_of`: hanya yang sudah terjadi/berlaku pada tanggal itu
//!   (persona "versi umur 25" tidak tahu masa depannya);
//! - hasil lemah -> `found = false`, bukan potongan acak.

mod context;
mod recall;
mod tools;

use std::sync::Arc;

use memory_core::MemoryConfig;
use memory_llm::{Embedder, LlmProvider};
use memory_store::Store;

pub use context::{approx_tokens, assemble_context};
pub use recall::*;
pub use tools::*;

#[derive(Debug, thiserror::Error)]
pub enum RetrieveError {
    #[error(transparent)]
    Store(#[from] memory_store::StoreError),
    #[error(transparent)]
    Llm(#[from] memory_llm::LlmError),
    #[error("{0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, RetrieveError>;

#[derive(Clone)]
pub struct Retriever {
    pub store: Store,
    pub embedder: Arc<dyn Embedder>,
    pub llm: Arc<dyn LlmProvider>,
    pub cfg: Arc<MemoryConfig>,
}

impl Retriever {
    pub fn new(store: Store, embedder: Arc<dyn Embedder>, llm: Arc<dyn LlmProvider>, cfg: Arc<MemoryConfig>) -> Self {
        Self { store, embedder, llm, cfg }
    }
}

#[cfg(test)]
mod tests;
