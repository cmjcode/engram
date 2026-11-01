//! Record domain yang disimpan. Semua field opsional memakai `#[serde(default)]`
//! agar data lama tetap terbaca ketika skema berevolusi selama puluhan tahun.
//!
//! Tanggal disimpan sebagai string ISO (`YYYY-MM-DD`) dan waktu sebagai
//! RFC3339 UTC, sehingga perbandingan string di database setara dengan
//! perbandingan waktu.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::kin::{Gender, KinRelation};
use crate::time::{LifeAnchors, TimeRef};

/// Orang yang memorinya disimpan. Satu vault bisa berisi banyak subjek.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Subject {
    /// Slug, mis. "bapak".
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub anchors: LifeAnchors,
    #[serde(default)]
    pub gender: Option<Gender>,
    /// Entitas "diri" subjek di knowledge graph.
    #[serde(default)]
    pub self_entity: Option<String>,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Interview,
    Memoir,
    #[default]
    Note,
    Chat,
    Lifelog,
    Email,
    Document,
    Photo,
    Audio,
    AgentConversation,
}

impl SourceKind {
    /// Sumber biografis (wawancara, memoar, catatan, percakapan agent) diperlakukan sebagai
    /// sinyal kuat; sumber volume tinggi (chat, lifelog) harus lolos skor.
    pub fn is_biographic(self) -> bool {
        matches!(
            self,
            SourceKind::Interview
                | SourceKind::Memoir
                | SourceKind::Note
                | SourceKind::Document
                | SourceKind::Audio
                | SourceKind::Photo
                | SourceKind::AgentConversation
        )
    }

    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_lowercase())).ok()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Interview => "interview",
            SourceKind::Memoir => "memoir",
            SourceKind::Note => "note",
            SourceKind::Chat => "chat",
            SourceKind::Lifelog => "lifelog",
            SourceKind::Email => "email",
            SourceKind::Document => "document",
            SourceKind::Photo => "photo",
            SourceKind::Audio => "audio",
            SourceKind::AgentConversation => "agent_conversation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    #[default]
    Pending,
    Processing,
    Done,
    /// Tidak tentang subjek atau seluruhnya noise: disimpan, tidak dipecah.
    Skipped,
    Failed,
}

/// Lapisan 0: sumber asli, immutable. File aslinya ada di `vault/sources/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Source {
    pub id: String,
    pub subject: String,
    pub kind: SourceKind,
    #[serde(default)]
    pub title: String,
    /// Path relatif terhadap root vault.
    pub path: String,
    /// blake3 hex dari teks yang diekstrak (kunci idempoten).
    pub hash: String,
    #[serde(default)]
    pub original_name: Option<String>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub chars: u64,
    #[serde(default)]
    pub status: SourceStatus,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub batch_id: Option<String>,
    /// Kapan isi sumber terjadi (bila diketahui adapter), ISO date.
    #[serde(default)]
    pub occurred_from: Option<String>,
    #[serde(default)]
    pub occurred_to: Option<String>,
    #[serde(default)]
    pub stats: SourceStats,
    #[serde(default)]
    pub meta: serde_json::Value,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceStats {
    #[serde(default)]
    pub segments: u32,
    #[serde(default)]
    pub episodes: u32,
    #[serde(default)]
    pub stream_items: u32,
    #[serde(default)]
    pub not_about_subject: u32,
    /// Noise murni yang tidak disimpan sebagai item apa pun.
    #[serde(default)]
    pub dropped: u32,
    #[serde(default)]
    pub gray_zone: u32,
    #[serde(default)]
    pub retellings: u32,
    #[serde(default)]
    pub facts: u32,
    #[serde(default)]
    pub facts_rejected: u32,
    #[serde(default)]
    pub quarantined: u32,
}

/// Working memory: item lifelog yang belum layak jadi episode. Luruh lewat
/// decay dan TTL; sumber aslinya tetap ada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StreamItem {
    pub id: String,
    pub subject: String,
    pub source_id: String,
    pub text: String,
    #[serde(default)]
    pub speaker: Option<String>,
    pub importance: f32,
    #[serde(default)]
    pub back_refs: u32,
    pub created_at: String,
    pub expires_at: String,
    #[serde(default)]
    pub promoted_to: Option<String>,
    #[serde(default)]
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Emotion {
    pub label: String,
    /// 0..1
    #[serde(default)]
    pub intensity: f32,
}

/// Sinyal keputusan dalam cerita: bahan Wisdom Layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Decision {
    pub situation: String,
    pub action: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub quote: Option<String>,
}

/// Sinyal bahwa cerita menunjukkan (polarity 1) atau membantah/meninggalkan
/// (polarity -1) suatu sifat atau nilai.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TraitSignal {
    pub label: String,
    #[serde(default)]
    pub kind: DispositionKind,
    #[serde(default = "one")]
    pub polarity: i8,
    #[serde(default)]
    pub quote: Option<String>,
}

fn one() -> i8 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeStatus {
    #[default]
    Active,
    /// Penceritaan ulang dari episode kanonik; disembunyikan dari recall default.
    Retelling,
    Quarantined,
}

/// Lapisan 1: satu pengalaman/cerita.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Episode {
    pub id: String,
    pub subject: String,
    pub source_id: String,
    pub text: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub time: TimeRef,
    #[serde(default)]
    pub time_start: Option<String>,
    #[serde(default)]
    pub time_end: Option<String>,
    #[serde(default)]
    pub time_ongoing: bool,
    #[serde(default)]
    pub time_confidence: f32,
    #[serde(default)]
    pub importance: f32,
    #[serde(default)]
    pub emotions: Vec<Emotion>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub life_phase: Option<String>,
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub status: EpisodeStatus,
    #[serde(default)]
    pub retelling_of: Option<String>,
    #[serde(default)]
    pub retell_count: u32,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub quotes: Vec<String>,
    /// Sinyal sifat/nilai dari cerita ini (bahan disposisi; bukan emosi sesaat).
    #[serde(default)]
    pub trait_signals: Vec<TraitSignal>,
    #[serde(default)]
    pub entity_ids: Vec<String>,
    /// Kapan diceritakan/direkam (ISO date).
    #[serde(default)]
    pub observed_at: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    #[default]
    Person,
    Place,
    Org,
    Event,
    Object,
    Concept,
}

impl EntityKind {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "place" | "location" | "tempat" | "lokasi" => EntityKind::Place,
            "org" | "organization" | "organisasi" | "company" | "party" | "partai" => EntityKind::Org,
            "event" | "peristiwa" => EntityKind::Event,
            "object" | "thing" | "benda" => EntityKind::Object,
            "concept" | "idea" | "konsep" => EntityKind::Concept,
            _ => EntityKind::Person,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum EntityStatus {
    /// Baru muncul sekali / belum terkonfirmasi. Tidak dipakai untuk auto-merge.
    #[default]
    Provisional,
    Confirmed,
    /// Sudah digabung ke entitas lain (`merged_into`).
    Merged,
}

/// Lapisan 2: entitas knowledge graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Entity {
    pub id: String,
    pub subject: String,
    pub kind: EntityKind,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: EntityStatus,
    #[serde(default)]
    pub merged_into: Option<String>,
    #[serde(default)]
    pub is_self: bool,
    #[serde(default)]
    pub gender: Option<Gender>,
    #[serde(default)]
    pub mention_count: u32,
    /// Episode tempat entitas disebut (untuk syarat "bukti kedua").
    #[serde(default)]
    pub episode_ids: Vec<String>,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum FactKind {
    /// Fakta peristiwa/atribut: "lahir di Klaten", "bekerja di Pertamina".
    #[default]
    Fact,
    /// Preferensi/afiliasi yang dinyatakan: "mendukung Partai A".
    Preference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum FactStatus {
    /// Tepercaya (bisa masih berlaku atau sudah ditutup `valid_to`).
    #[default]
    Active,
    Quarantined,
    /// Bertentangan dengan fakta lain di periode yang sama; menunggu review.
    Conflict,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Evidence {
    pub episode_id: String,
    pub quote: String,
}

/// Lapisan 2: fakta bitemporal. Tidak pernah dihapus atau ditimpa; fakta
/// yang berubah ditutup dengan `valid_to` dan `superseded_by`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Fact {
    pub id: String,
    pub subject: String,
    /// Entitas pelaku fakta.
    pub entity_id: String,
    pub predicate: String,
    /// Kelompok saling-eksklusif, mis. "afiliasi_politik", "pekerjaan", "tempat_tinggal".
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub object_entity: Option<String>,
    pub object_text: String,
    #[serde(default)]
    pub kind: FactKind,
    /// Hanya satu objek boleh berlaku pada satu waktu untuk (entity, category).
    #[serde(default)]
    pub exclusive: bool,
    #[serde(default)]
    pub time: TimeRef,
    /// Waktu valid (dunia nyata).
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_to: Option<String>,
    #[serde(default)]
    pub ongoing: bool,
    #[serde(default)]
    pub time_confidence: f32,
    /// Waktu transaksi: kapan sistem mengetahui fakta ini.
    pub observed_at: String,
    pub confidence: f32,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub status: FactStatus,
    #[serde(default)]
    pub superseded_by: Option<String>,
    #[serde(default)]
    pub supersedes: Option<String>,
    #[serde(default)]
    pub created_at: String,
}

impl Fact {
    /// Masih berlaku sekarang (belum ditutup).
    pub fn is_current(&self) -> bool {
        self.status == FactStatus::Active && self.valid_to.is_none()
    }

    /// Berlaku pada tanggal ISO `date` (batas tidak diketahui = terbuka).
    pub fn valid_at(&self, date: &str) -> bool {
        let after_start = self.valid_from.as_deref().is_none_or(|f| f <= date);
        let before_end = self.valid_to.as_deref().is_none_or(|t| t >= date);
        after_start && before_end
    }

    pub fn statement(&self, entity_name: &str) -> String {
        format!("{entity_name} {} {}", self.predicate, self.object_text)
    }
}

/// Edge kekerabatan: `from` adalah `relation` dari `to`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Kin {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub relation: KinRelation,
    #[serde(default)]
    pub gender: Option<Gender>,
    #[serde(default)]
    pub older: Option<bool>,
    /// Istilah asli yang diucapkan ("pakde", "uwa").
    #[serde(default)]
    pub term: String,
    #[serde(default)]
    pub lineage: Option<String>,
    #[serde(default)]
    pub inferred: bool,
    pub confidence: f32,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReflectiveStatus {
    #[default]
    Active,
    PendingReview,
    Rejected,
}

/// Lapisan 3: prinsip / pola keputusan, wajib didukung beberapa episode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Principle {
    pub id: String,
    pub subject: String,
    pub statement: String,
    #[serde(default)]
    pub domain: String,
    /// Asal pengaruh, mis. "didikan ibu".
    #[serde(default)]
    pub origin: Option<String>,
    pub confidence: f32,
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_to: Option<String>,
    #[serde(default)]
    pub status: ReflectiveStatus,
    #[serde(default)]
    pub derived_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum DispositionKind {
    /// Sifat: sabar, pemarah, pemalu.
    #[default]
    Trait,
    /// Nilai hidup: kejujuran, keluarga dulu.
    Value,
}

/// Lapisan 3: sifat/nilai jangka panjang dengan masa berlaku. Berbeda dengan
/// `Emotion` (sesaat, per episode) yang tidak pernah otomatis menjadi disposisi.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Disposition {
    pub id: String,
    pub subject: String,
    pub kind: DispositionKind,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_to: Option<String>,
    pub confidence: f32,
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub superseded_by: Option<String>,
    #[serde(default)]
    pub status: ReflectiveStatus,
    #[serde(default)]
    pub derived_at: String,
}

/// Ringkasan satu fase hidup hasil konsolidasi.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LifePhase {
    pub id: String,
    pub subject: String,
    pub name: String,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub episode_ids: Vec<String>,
    #[serde(default)]
    pub updated_at: String,
}

/// Lapisan 4: gaya bertutur subjek.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceProfile {
    pub subject: String,
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub idioms: Vec<(String, u32)>,
    #[serde(default)]
    pub language_mix: BTreeMap<String, f32>,
    #[serde(default)]
    pub sample_quotes: Vec<String>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    #[default]
    Queued,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Job {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(default)]
    pub status: JobStatus,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    pub idempotency_key: String,
    #[serde(default)]
    pub progress_done: u32,
    #[serde(default)]
    pub progress_total: u32,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub batch_id: Option<String>,
    #[serde(default)]
    pub run_after: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn default_max_attempts() -> u32 {
    3
}

/// Satu sesi import massal: progres untuk UI dan CLI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportBatch {
    pub id: String,
    pub subject: String,
    pub label: String,
    #[serde(default)]
    pub total: u32,
    #[serde(default)]
    pub done: u32,
    #[serde(default)]
    pub failed: u32,
    #[serde(default)]
    pub duplicates: u32,
    #[serde(default)]
    pub failures: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewKind {
    /// Skor kepentingan di zona abu-abu.
    GrayZone,
    /// Dua fakta eksklusif sama-sama berlaku di periode yang sama.
    Conflict,
    /// Kemungkinan dua entitas adalah orang yang sama.
    EntityMerge,
    /// Fakta/episode confidence rendah.
    Quarantine,
    /// Prinsip/disposisi yang lolos verifikasi tapi butuh konfirmasi.
    Reflective,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    #[default]
    Open,
    Approved,
    Rejected,
}

/// Antrean review manusia. Keputusan dipakai sebagai contoh few-shot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewItem {
    pub id: String,
    pub subject: String,
    pub kind: ReviewKind,
    pub summary: String,
    /// ID record yang terlibat (fakta, entitas, stream_item, ...).
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(default)]
    pub status: ReviewStatus,
    #[serde(default)]
    pub note: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub resolved_at: Option<String>,
}

impl Job {
    /// Job baru yang siap dijalankan sekarang.
    pub fn new(kind: &str, payload: serde_json::Value, idempotency_key: String, batch_id: Option<String>) -> Self {
        let now = crate::ids::now_rfc3339();
        Self {
            id: crate::new_id(),
            kind: kind.to_string(),
            payload,
            status: JobStatus::Queued,
            attempts: 0,
            max_attempts: 3,
            idempotency_key,
            progress_done: 0,
            progress_total: 0,
            stage: None,
            error: None,
            batch_id,
            run_after: now.clone(),
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(from: Option<&str>, to: Option<&str>) -> Fact {
        Fact {
            id: "f".into(),
            subject: "s".into(),
            entity_id: "e".into(),
            predicate: "mendukung".into(),
            category: "afiliasi_politik".into(),
            object_entity: None,
            object_text: "Partai A".into(),
            kind: FactKind::Preference,
            exclusive: true,
            time: TimeRef::Unknown,
            valid_from: from.map(Into::into),
            valid_to: to.map(Into::into),
            ongoing: false,
            time_confidence: 1.0,
            observed_at: "2024-01-01".into(),
            confidence: 0.9,
            evidence: vec![],
            status: FactStatus::Active,
            superseded_by: None,
            supersedes: None,
            created_at: String::new(),
        }
    }

    #[test]
    fn fact_validity_window() {
        let f = fact(Some("1999-01-01"), Some("2015-12-31"));
        assert!(f.valid_at("2005-06-01"));
        assert!(!f.valid_at("2020-01-01"));
        assert!(!f.is_current());
        assert!(fact(Some("2019-01-01"), None).is_current());
    }

    #[test]
    fn old_records_without_new_fields_still_deserialize() {
        let json = serde_json::json!({
            "id": "e1", "subject": "s", "kind": "person", "name": "Karno"
        });
        let e: Entity = serde_json::from_value(json).unwrap();
        assert_eq!(e.status, EntityStatus::Provisional);
    }

    #[test]
    fn source_kind_roundtrip() {
        assert_eq!(SourceKind::parse("Interview"), Some(SourceKind::Interview));
        assert_eq!(SourceKind::parse("agent_conversation"), Some(SourceKind::AgentConversation));
        assert!(SourceKind::Interview.is_biographic());
        assert!(!SourceKind::Lifelog.is_biographic());
    }
}
