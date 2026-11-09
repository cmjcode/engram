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
    /// Satu giliran sesi kerja agent AI (prompt pengguna + pemanggilan tool + jawaban),
    /// dari hook `engram hook`; diekstrak menjadi observasi bertipe + ringkasan sesi.
    AgentSession,
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
                | SourceKind::AgentSession
        )
    }

    /// Sumber yang isinya umumnya terjadi pada tanggal sumber direkam/diamati
    /// bila teks tidak menyebut waktu masa lalu secara eksplisit.
    pub fn defaults_to_observed_date(self) -> bool {
        matches!(
            self,
            SourceKind::AgentConversation
                | SourceKind::AgentSession
                | SourceKind::Document
                | SourceKind::Chat
                | SourceKind::Lifelog
                | SourceKind::Email
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
            SourceKind::AgentSession => "agent_session",
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
    /// Nama proyek/workspace asal sumber bila ada (mis. dari `antigravity:LLM-MEMORY`).
    #[serde(default)]
    pub project: Option<String>,
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
    /// Proyek/workspace terkait episode ini (bila berasal dari sesi kerja/coding).
    #[serde(default)]
    pub project: Option<String>,
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
    /// Jenis observasi untuk episode dari sesi kerja agent:
    /// `bugfix | feature | decision | refactor | discovery | change | session_summary`.
    /// Kosong untuk cerita/fakta biasa.
    #[serde(default)]
    pub obs_type: Option<String>,
    /// Tag konsep (pola, trade-off, nama fitur) untuk pengindeksan.
    #[serde(default)]
    pub concepts: Vec<String>,
    /// Berkas yang dibaca/diubah dalam observasi (relatif akar proyek bila bisa).
    #[serde(default)]
    pub files_read: Vec<String>,
    #[serde(default)]
    pub files_modified: Vec<String>,
    /// Sesi agent & nomor prompt asal observasi.
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub prompt_number: Option<u32>,
    /// Kapan diceritakan/direkam (ISO date).
    #[serde(default)]
    pub observed_at: String,
    #[serde(default)]
    pub created_at: String,
}

/// Jenis observasi yang dikenal (urutan = prioritas tampil di indeks konteks).
pub const OBS_TYPES: &[&str] = &["decision", "bugfix", "discovery", "feature", "refactor", "change", "session_summary"];

/// Label pendek ASCII untuk indeks konteks (tanpa emoji agar aman di semua terminal/font).
pub fn obs_type_label(t: Option<&str>) -> &'static str {
    match t.unwrap_or("") {
        "decision" => "keputusan",
        "bugfix" => "bugfix",
        "discovery" => "temuan",
        "feature" => "fitur",
        "refactor" => "refactor",
        "change" => "perubahan",
        "session_summary" => "sesi",
        "lesson" => "pelajaran",
        _ => "cerita",
    }
}

impl Episode {
    /// Perkiraan token bila teks penuh episode diambil (karakter / 4).
    pub fn approx_tokens(&self) -> usize {
        (self.text.chars().count() + self.summary.chars().count()) / 4
    }

    /// Label waktu manusiawi dengan fallback ke `time_start`, `observed_at`, atau `created_at`
    /// bila `time` bernilai `TimeRef::Unknown`.
    pub fn time_label(&self) -> String {
        if self.time != TimeRef::Unknown {
            return self.time.label();
        }
        let fmt_iso = |s: &str| -> Option<String> {
            let d = chrono::NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()?;
            Some(d.format("%d-%m-%Y").to_string())
        };
        if let Some(s) = self.time_start.as_deref() {
            if let Some(e) = self.time_end.as_deref()
                && e.get(..4) != s.get(..4)
                && let (Some(sy), Some(ey)) = (s.get(..4), e.get(..4))
            {
                return format!("{sy}–{ey}");
            }
            if let Some(f) = fmt_iso(s) {
                return f;
            }
        }
        if let Some(f) = fmt_iso(&self.observed_at) {
            return f;
        }
        if let Some(f) = fmt_iso(&self.created_at) {
            return f;
        }
        self.time.label()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    #[default]
    Person,
    Place,
    Org,
    /// Repositori, microservice, aplikasi, produk, atau proyek kerja.
    Project,
    /// Berkas kode, modul, package, endpoint API, tabel DB, atau fungsi.
    Component,
    /// Bahasa pemrograman, framework, library, database, CLI, atau AI tool.
    Tool,
    Event,
    Object,
    Concept,
}

impl EntityKind {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "place" | "location" | "tempat" | "lokasi" | "kota" | "negara" | "city" | "country" => EntityKind::Place,
            "org" | "organization" | "organisasi" | "company" | "perusahaan" | "instansi" | "party" | "partai" | "team" | "tim" => EntityKind::Org,
            "project" | "proyek" | "repo" | "repository" | "repositori" | "service" | "layanan" | "microservice" | "app" | "application" | "aplikasi" | "system" | "sistem" | "product" | "produk" | "workspace" => EntityKind::Project,
            "component" | "komponen" | "file" | "berkas" | "module" | "modul" | "crate" | "package" | "paket" | "endpoint" | "api" | "route" | "table" | "tabel" | "function" | "fungsi" | "struct" | "class" | "folder" | "direktori" => EntityKind::Component,
            "tool" | "alat" | "tech" | "technology" | "teknologi" | "language" | "bahasa" | "framework" | "library" | "pustaka" | "database" | "db" | "cli" | "ide" | "platform" | "infra" | "infrastruktur" => EntityKind::Tool,
            "event" | "peristiwa" | "kejadian" | "insiden" | "incident" | "rilis" | "release" => EntityKind::Event,
            "object" | "thing" | "benda" | "artefak" | "artifact" => EntityKind::Object,
            "concept" | "idea" | "konsep" | "pola" | "pattern" | "arsitektur" | "architecture" | "konvensi" | "convention" | "domain" => EntityKind::Concept,
            _ => EntityKind::Person,
        }
    }

    /// Inferensi jenis entitas dari nama sebutan (mencegah path repo, microservice,
    /// atau nama berkas kode jatuh ke default `EntityKind::Person`).
    pub fn infer_from_name(name: &str) -> Option<Self> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return None;
        }
        let low = trimmed.to_lowercase();

        // Pola komponen eksplisit: "berkas X pada Y", "file X", "tabel X", "endpoint X", dll.
        const COMP_PREFIXES: &[&str] = &[
            "berkas ", "file ", "modul ", "module ", "endpoint ", "route ", "tabel ", "table ",
            "fungsi ", "function ", "method ", "struct ", "class ", "trait ", "enum ", "komponen ",
            "crate ", "package ", "folder ", "direktori ", "script ",
        ];
        if COMP_PREFIXES.iter().any(|p| low.starts_with(p)) {
            return Some(EntityKind::Component);
        }

        const PROJ_PREFIXES: &[&str] = &[
            "proyek ", "project ", "repo ", "repositori ", "repository ", "microservice ",
            "service ", "layanan ", "aplikasi ", "sistem ",
        ];
        if PROJ_PREFIXES.iter().any(|p| low.starts_with(p)) {
            return Some(EntityKind::Project);
        }

        // Pola "<sesuatu> pada <path/repo>" (mis. "total_value_formulation_test.go pada BACKEND/api-data-warehouse")
        if let Some((_, right)) = low.split_once(" pada ")
            && (right.contains('/') || right.contains('-') || right.contains('_'))
        {
            return Some(EntityKind::Component);
        }

        // Token pertama atau nama tanpa spasi yang memiliki ekstensi kode/konfigurasi
        let first_tok = low.split_whitespace().next().unwrap_or("");
        const CODE_EXTS: &[&str] = &[
            ".rs", ".go", ".py", ".ts", ".tsx", ".js", ".jsx", ".json", ".toml", ".yaml", ".yml",
            ".sql", ".sh", ".bash", ".zsh", ".md", ".html", ".css", ".scss", ".vue", ".svelte",
            ".java", ".kt", ".swift", ".c", ".cpp", ".h", ".hpp", ".proto", ".lock", ".env",
            ".dockerfile", ".tf", ".hcl", ".graphql", ".prisma",
        ];
        if CODE_EXTS.iter().any(|ext| low.ends_with(ext) || first_tok.ends_with(ext)) {
            return Some(EntityKind::Component);
        }

        // Identifier tanpa spasi (atau path "A/B")
        if !trimmed.contains(' ') {
            if low.starts_with("/api/") || low.starts_with("src/") || low.starts_with("crates/") || low.starts_with("apps/") || low.starts_with("scripts/") || low.starts_with("internal/") || low.starts_with("pkg/") || low.starts_with("cmd/") {
                return Some(EntityKind::Component);
            }
            if trimmed.contains('/') {
                // mis. "BACKEND/api-master-sensor", "FRONTEND/web-dashboard", "cmjcode/engram"
                return Some(EntityKind::Project);
            }
            if trimmed.contains('_') {
                // snake_case identifier (fungsi, tabel, berkas, variabel)
                return Some(EntityKind::Component);
            }
            if trimmed.contains('-') {
                // kebab-case repo/service (mis. "api-master-sensor", "llm-memory", "memory-extract")
                if low.starts_with("api-")
                    || low.starts_with("svc-")
                    || low.starts_with("service-")
                    || low.starts_with("app-")
                    || low.starts_with("web-")
                    || low.starts_with("fe-")
                    || low.starts_with("be-")
                    || low.starts_with("backend-")
                    || low.starts_with("frontend-")
                    || low.starts_with("llm-")
                {
                    return Some(EntityKind::Project);
                }
                return Some(EntityKind::Component);
            }
        }

        None
    }

    /// Parse `kind` dari keluaran LLM dengan fallback cerdas berdasarkan `name`.
    pub fn parse_with_name(kind_str: &str, name: &str) -> Self {
        let parsed = Self::parse(kind_str);
        if parsed == EntityKind::Person
            && let Some(inferred) = Self::infer_from_name(name)
        {
            return inferred;
        }
        parsed
    }

    pub fn is_technical(self) -> bool {
        matches!(self, EntityKind::Project | EntityKind::Component | EntityKind::Tool)
    }

    pub fn label_id(self) -> &'static str {
        match self {
            EntityKind::Person => "Orang",
            EntityKind::Place => "Tempat",
            EntityKind::Org => "Organisasi",
            EntityKind::Project => "Proyek & Layanan",
            EntityKind::Component => "Komponen & Berkas",
            EntityKind::Tool => "Tool & Teknologi",
            EntityKind::Event => "Peristiwa",
            EntityKind::Object => "Benda",
            EntityKind::Concept => "Konsep",
        }
    }

    pub fn merge_prompt_noun(self) -> &'static str {
        match self {
            EntityKind::Person => "orang",
            EntityKind::Place => "tempat",
            EntityKind::Org => "organisasi",
            EntityKind::Project => "proyek/layanan",
            EntityKind::Component => "komponen/berkas",
            EntityKind::Tool => "tool/teknologi",
            EntityKind::Event => "peristiwa",
            EntityKind::Object => "objek",
            EntityKind::Concept => "konsep",
        }
    }
}

/// Ekstrak `(client, project)` dari tag `source` berformat `<client>:<project>`
/// (mis. `"antigravity:LLM-MEMORY"` -> `(Some("antigravity"), Some("LLM-MEMORY"))`).
pub fn parse_source_tag(raw: &str) -> (Option<String>, Option<String>) {
    let s = raw.trim();
    if s.is_empty() {
        return (None, None);
    }
    if let Some((client, proj)) = s.split_once(':') {
        let c = client.trim();
        let p = proj.trim();
        (
            (!c.is_empty()).then(|| c.to_string()),
            (!p.is_empty()).then(|| p.to_string()),
        )
    } else {
        (Some(s.to_string()), None)
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
    /// Kemungkinan dua entitas merujuk hal yang sama.
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

/// Sesi kerja satu agent AI (Claude Code, Antigravity, ...) pada satu proyek.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    /// ID sesi dari client (dibersihkan agar valid sebagai kunci record).
    pub id: String,
    pub subject: String,
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub worktree: Option<String>,
    /// Prompt pengguna pertama (setelah penyaring privasi), untuk judul di UI.
    #[serde(default)]
    pub first_prompt: String,
    #[serde(default)]
    pub prompt_counter: u32,
    /// Jumlah pemanggilan tool yang tercatat.
    #[serde(default)]
    pub tool_uses: u32,
    /// Jumlah episode (observasi + ringkasan) yang lahir dari sesi ini.
    #[serde(default)]
    pub episodes: u32,
    /// `active` | `completed`
    #[serde(default = "default_session_status")]
    pub status: String,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub last_activity_at: String,
    #[serde(default)]
    pub completed_at: Option<String>,
}

fn default_session_status() -> String {
    "active".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum LessonKind {
    /// Koreksi dari pengguna terhadap cara kerja/keluaran agent.
    Correction,
    /// Konvensi/aturan proyek (penamaan, struktur, alur kerja).
    #[default]
    Convention,
    /// Jebakan: hal yang mahal ditemukan ulang (gotcha).
    Pitfall,
    /// Temuan tentang cara kerja sistem.
    Discovery,
}

impl LessonKind {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "correction" | "koreksi" => LessonKind::Correction,
            "pitfall" | "gotcha" | "jebakan" => LessonKind::Pitfall,
            "discovery" | "temuan" => LessonKind::Discovery,
            _ => LessonKind::Convention,
        }
    }
    pub fn label_id(self) -> &'static str {
        match self {
            LessonKind::Correction => "koreksi",
            LessonKind::Convention => "konvensi",
            LessonKind::Pitfall => "jebakan",
            LessonKind::Discovery => "temuan",
        }
    }
}

/// Pelajaran proyek: konvensi, koreksi, atau jebakan yang berulang. Judul yang
/// sama (ternormalisasi) dalam satu proyek menambah `occurrences`, bukan duplikat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Lesson {
    pub id: String,
    pub subject: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub kind: LessonKind,
    pub title: String,
    /// Kunci dedup: judul ternormalisasi.
    #[serde(default)]
    pub title_key: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default = "default_occurrences")]
    pub occurrences: u32,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    /// `active` | `archived` | `promoted` (sudah menjadi prinsip)
    #[serde(default = "default_session_status")]
    pub status: String,
    #[serde(default)]
    pub promoted_to: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// Metadata proyek yang diisi pengguna: repositori git, catatan bergaya Obsidian
/// (tautan `[[Proyek Lain]]`), dan tag untuk mengelompokkan proyek.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProjectMeta {
    /// Deterministik dari (subjek, kunci proyek): lihat [`crate::project::project_meta_id`].
    pub id: String,
    pub subject: String,
    /// Nama proyek sebagaimana ditampilkan.
    pub project: String,
    /// [`crate::project::project_key`] dari `project`.
    #[serde(default)]
    pub project_key: String,
    /// URL remote atau nama repositori git (mis. `github.com/org/repo`).
    #[serde(default)]
    pub repo: Option<String>,
    /// Catatan bebas; `[[Nama]]`, `[[Nama|label]]`, dan `[[Nama#bagian]]` menjadi tautan proyek.
    #[serde(default)]
    pub notes: String,
    /// Target tautan `[[...]]` hasil parse `notes` (untuk backlink).
    #[serde(default)]
    pub links: Vec<String>,
    /// Tag grup proyek (tanpa `#`), mis. `OPERON`, `backend/api`.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl ProjectMeta {
    /// Bentuk metadata baru/terbarui dengan normalisasi tag dan parse tautan dari catatan.
    pub fn build(subject: &str, project: &str, repo: Option<&str>, notes: &str, tags: &[String], prev: Option<&ProjectMeta>) -> Self {
        let now = crate::ids::now_rfc3339();
        let project = project.trim().to_string();
        let key = crate::project::project_key(&project);
        let links = crate::project::parse_wikilinks(notes).into_iter().filter(|l| !crate::project::same_project(l, &project)).collect();
        Self {
            id: crate::project::project_meta_id(subject, &project),
            subject: subject.to_string(),
            project,
            project_key: key,
            repo: repo.map(str::trim).filter(|r| !r.is_empty()).map(str::to_string),
            notes: notes.trim_end().to_string(),
            links,
            tags: crate::project::normalize_tags(tags),
            created_at: prev.map(|p| p.created_at.clone()).filter(|c| !c.is_empty()).unwrap_or_else(|| now.clone()),
            updated_at: now,
        }
    }
}

fn default_occurrences() -> u32 {
    1
}

impl Lesson {
    /// Kunci dedup judul: huruf kecil, tanpa tanda baca, spasi tunggal.
    pub fn title_key_of(title: &str) -> String {
        let mut out = String::new();
        let mut last_space = true;
        for c in title.trim().to_lowercase().chars() {
            if c.is_alphanumeric() {
                out.push(c);
                last_space = false;
            } else if !last_space {
                out.push(' ');
                last_space = true;
            }
        }
        out.trim().to_string()
    }
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

    #[test]
    fn entity_kind_infers_technical_names() {
        assert_eq!(
            EntityKind::parse_with_name("person", "BACKEND/api-master-sensor"),
            EntityKind::Project
        );
        assert_eq!(
            EntityKind::parse_with_name("", "total_value_formulation_test.go pada BACKEND/api-data-warehouse"),
            EntityKind::Component
        );
        assert_eq!(
            EntityKind::parse_with_name("person", "berkas autentikasi pada BACKEND/api-trans-stock-company"),
            EntityKind::Component
        );
        assert_eq!(
            EntityKind::parse_with_name("project", "LLM-MEMORY"),
            EntityKind::Project
        );
        assert_eq!(
            EntityKind::parse_with_name("tool", "SurrealDB"),
            EntityKind::Tool
        );
        assert_eq!(
            EntityKind::parse_with_name("person", "Pakde Karno"),
            EntityKind::Person
        );
    }

    #[test]
    fn lesson_title_key_normalizes() {
        assert_eq!(Lesson::title_key_of("  Jangan commit tanpa izin!! (VOLTUNES) "), "jangan commit tanpa izin voltunes");
        assert_eq!(obs_type_label(Some("bugfix")), "bugfix");
        assert_eq!(obs_type_label(None), "cerita");
    }

    #[test]
    fn parse_source_tag_extracts_client_and_project() {
        assert_eq!(
            parse_source_tag("antigravity:LLM-MEMORY"),
            (Some("antigravity".into()), Some("LLM-MEMORY".into()))
        );
        assert_eq!(
            parse_source_tag("claude-code"),
            (Some("claude-code".into()), None)
        );
    }
}
