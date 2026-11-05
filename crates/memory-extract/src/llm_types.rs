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
    /// Hanya untuk observasi sesi agent: jenis observasi
    /// (decision | bugfix | discovery | feature | refactor | change | session_summary).
    #[serde(default)]
    pub obs_type: Option<String>,
    /// Tag konsep singkat (pola, trade-off, nama fitur/modul).
    #[serde(default)]
    pub concepts: Vec<String>,
    /// Berkas yang dibaca / diubah (path relatif akar proyek).
    #[serde(default)]
    pub files_read: Vec<String>,
    #[serde(default)]
    pub files_modified: Vec<String>,
    /// Pelajaran proyek (konvensi, koreksi pengguna, jebakan) yang layak diingat.
    #[serde(default)]
    pub lessons: Vec<LessonOut>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LessonOut {
    /// correction | convention | pitfall | discovery
    #[serde(default)]
    pub kind: String,
    /// Satu kalimat imperatif singkat, stabil (dipakai sebagai kunci dedup).
    pub title: String,
    #[serde(default)]
    pub detail: String,
    /// Disalin persis dari teks.
    #[serde(default)]
    pub quote: String,
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
    /// person | place | org | project | component | tool | event | object | concept
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CompactEpisodesOut {
    #[serde(default)]
    pub rollups: Vec<EpisodeRollupOut>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct EpisodeRollupOut {
    pub episode_ids: Vec<String>,
    pub title: String,
    pub summary: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CompactFactsOut {
    #[serde(default)]
    pub merges: Vec<FactMergeOut>,
    #[serde(default)]
    pub closures: Vec<FactClosureOut>,
    #[serde(default)]
    pub rejections: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FactMergeOut {
    pub keep_id: String,
    pub absorb_ids: Vec<String>,
    #[serde(default)]
    pub predicate: Option<String>,
    #[serde(default)]
    pub object_text: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FactClosureOut {
    pub fact_id: String,
    #[serde(default)]
    pub valid_to: Option<String>,
    #[serde(default)]
    pub superseded_by: Option<String>,
}

/// Keluaran ekstraksi satu giliran sesi agent: ringkasan sesi + observasi bertipe.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SessionExtraction {
    /// Tidak ada yang layak diingat (basa-basi, navigasi, pembacaan tanpa hasil).
    #[serde(default)]
    pub skip: bool,
    #[serde(default)]
    pub skip_reason: Option<String>,
    #[serde(default)]
    pub summary: Option<SummaryOut>,
    #[serde(default)]
    pub observations: Vec<ObservationOut>,
}

/// Ringkasan giliran: apa yang diminta, diselidiki, dipelajari, selesai, dan langkah berikut.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SummaryOut {
    #[serde(default)]
    pub request: String,
    #[serde(default)]
    pub investigated: String,
    #[serde(default)]
    pub learned: String,
    #[serde(default)]
    pub completed: String,
    #[serde(default)]
    pub next_steps: String,
}

/// Satu observasi: hal yang mahal ditemukan ulang (keputusan, bugfix, temuan, fitur, refactor, perubahan).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ObservationOut {
    /// decision | bugfix | discovery | feature | refactor | change
    #[serde(default)]
    pub obs_type: String,
    pub title: String,
    /// Narasi 2-6 kalimat yang berdiri sendiri (tanpa perlu membaca sesi aslinya).
    #[serde(default)]
    pub narrative: String,
    /// Fakta atomik tentang kode/proyek (teks bebas) untuk ringkasan.
    #[serde(default)]
    pub facts_text: Vec<String>,
    #[serde(default)]
    pub concepts: Vec<String>,
    #[serde(default)]
    pub files_read: Vec<String>,
    #[serde(default)]
    pub files_modified: Vec<String>,
    #[serde(default)]
    pub entities: Vec<EntityOut>,
    #[serde(default)]
    pub facts: Vec<FactOut>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub lessons: Vec<LessonOut>,
    /// Kutipan pendukung dari teks sesi (disalin persis).
    #[serde(default)]
    pub quotes: Vec<String>,
}
