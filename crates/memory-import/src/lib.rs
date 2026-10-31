//! Import memori dari berbagai format. Semua jalur bermuara ke tahap Ingest
//! yang sama, sehingga dedup (hash) dan penyaringan noise berlaku seragam.
//!
//! - [`parse_bytes`]: deteksi format & ubah ke dokumen teks ([`Imported`]).
//! - [`Importer::import_paths`]: import massal ke satu batch dengan progres.
//! - [`Importer::scan_inbox`]: folder `vault/inbox/` (subfolder = subjek).

pub mod adapters;
mod importer;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use memory_core::model::SourceKind;

pub use adapters::{detect_format, parse_bytes, Format};
pub use importer::{ImportSummary, Importer, InboxReport};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("format tidak didukung: {0}")]
    Unsupported(String),
    #[error("berkas rusak: {0}")]
    Parse(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Extract(#[from] memory_extract::ExtractError),
    #[error(transparent)]
    Store(#[from] memory_store::StoreError),
    #[error(transparent)]
    Llm(#[from] memory_llm::LlmError),
}

pub type Result<T> = std::result::Result<T, ImportError>;

/// Satu dokumen hasil adapter, siap di-ingest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Imported {
    pub kind: SourceKind,
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub occurred_from: Option<String>,
    #[serde(default)]
    pub occurred_to: Option<String>,
    #[serde(default)]
    pub meta: Value,
}
