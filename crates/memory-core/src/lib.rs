//! Tipe domain murni untuk platform engram.
//!
//! Crate ini sengaja tidak bergantung pada database maupun LLM, sehingga
//! aturan inti (waktu kabur, kekerabatan, skor kepentingan, verifikasi kutipan)
//! bisa diuji tanpa infrastruktur.

pub mod config;
pub mod ids;
pub mod kin;
pub mod model;
pub mod score;
pub mod text;
pub mod time;

pub use config::MemoryConfig;
pub use ids::new_id;
pub use kin::{Gender, KinRelation, KinTerm};
pub use model::*;
pub use time::{LifeAnchors, RawTime, ResolvedTime, TimeRef};

/// Versi skema penyimpanan dan format ekspor. Dinaikkan setiap ada migrasi.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("nilai tidak valid: {0}")]
    Invalid(String),
}
