//! Skema keluaran LLM (dipakai untuk guided JSON). Dibuat longgar dengan
//! default agar model lokal tidak gagal hanya karena field opsional.

use memory_core::model::{Decision, TraitSignal};
use memory_core::time::RawTime;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SegmentPlan {
    pub segments: Vec<SegmentOut>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SegmentOut {
    /// Indeks unit pertama (inklusif).
    pub start: usize,
    /// Indeks unit terakhir (inklusif).
    pub end: usize,
    #[serde(default)]
    pub topic: String,
    /// Tentang kehidupan/pemikiran subjek atau keluarganya.
    #[serde(default = "yes")]
    pub about_subject: bool,
    #[serde(default)]
    pub speaker: Option<String>,
    /// 0 = basa-basi/logistik, 1 = peristiwa hidup penting/keputusan/nilai.
    #[serde(default)]
    pub significance: Option<f32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ExtractionOut {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub time: RawTime,
    #[serde(default)]
    pub life_phase: Option<String>,
    #[serde(default)]
    pub emotions: Vec<EmotionOut>,
    #[serde(default)]
    pub entities: Vec<EntityOut>,
    #[serde(default)]
    pub facts: Vec<FactOut>,
    #[serde(default)]
    pub kin: Vec<KinOut>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub quotes: Vec<String>,
    #[serde(default)]
    pub trait_signals: Vec<TraitSignal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmotionOut {
    pub label: String,
    #[serde(default)]
    pub intensity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EntityOut {
    pub name: String,
    /// person | place | org | event | object | concept
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_self: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FactOut {
    /// Nama entitas pelaku (sama dengan salah satu `entities.name`).
    pub subject: String,
    pub predicate: String,
    pub object: String,
    #[serde(default)]
    pub object_is_entity: bool,
    /// snake_case stabil: afiliasi_politik, tempat_tinggal, pekerjaan, ...
    #[serde(default)]
    pub category: String,
    /// "fact" atau "preference".
    #[serde(default)]
    pub kind: String,
    /// Hanya satu objek bisa berlaku pada satu waktu.
    #[serde(default)]
    pub exclusive: bool,
    #[serde(default)]
    pub time: RawTime,
    /// Disalin persis dari teks.
    pub quote: String,
    #[serde(default)]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KinOut {
    /// Orang yang perannya disebut.
    pub person: String,
    /// Istilah apa adanya: pakde, bulik, mbah kakung, uwa, teteh, ...
    pub term: String,
    /// Relatif terhadap siapa (default: subjek).
    #[serde(default)]
    pub of: Option<String>,
    pub quote: String,
    #[serde(default)]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SameEntityVerdict {
    pub same: bool,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PrincipleProposal {
    /// Kosong bila bukti tidak menunjukkan pola yang konsisten.
    #[serde(default)]
    pub statement: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Verification {
    /// ID episode yang benar-benar mendukung pernyataan.
    #[serde(default)]
    pub supported_ids: Vec<String>,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TextOut {
    pub text: String,
}
