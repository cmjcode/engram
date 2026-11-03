//! Konfigurasi platform (dibaca dari `memory.toml`). Semua field punya default
//! yang mengarah ke infrastruktur inferensi lokal (vLLM + TEI).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MemoryConfig {
    pub vault: VaultConfig,
    pub llm: LlmConfig,
    pub embedding: EmbeddingConfig,
    pub index: IndexConfig,
    pub pipeline: PipelineConfig,
    pub score: ScoreConfig,
    pub distill: DistillConfig,
    pub retrieval: RetrievalConfig,
    pub schedule: ScheduleConfig,
    pub server: ServerConfig,
    pub transcription: TranscriptionConfig,
    pub backup: BackupConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultConfig {
    pub path: PathBuf,
    /// Subjek default bila perintah tidak menyebut `--subject`.
    pub default_subject: String,
    /// Backend KV: "rocksdb" (default) atau "mem" (uji).
    pub engine: String,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self { path: PathBuf::from("vault"), default_subject: "default".into(), engine: "rocksdb".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Endpoint OpenAI-compatible (vLLM), tanpa `/chat/completions`.
    pub base_url: String,
    /// Nama env var berisi API key (opsional untuk vLLM lokal).
    pub api_key_env: Option<String>,
    /// API key langsung (dipakai bila env var kosong). Untuk aplikasi desktop yang tidak
    /// mewarisi env shell; disimpan apa adanya di memory.toml.
    pub api_key: Option<String>,
    /// Model besar: ekstraksi, distilasi, verifikasi.
    pub extract_model: String,
    /// Model ringan: segmentasi, klasifikasi, adjudikasi.
    pub light_model: String,
    /// Base URL khusus model ringan bila beda server.
    pub light_base_url: Option<String>,
    pub temperature: f32,
    pub max_tokens: u32,
    pub timeout_secs: u64,
    pub max_retries: u32,
    /// "json_schema" (guided decoding vLLM), "json_object", atau "none".
    pub json_mode: String,
    /// Kirim `chat_template_kwargs.enable_thinking=false` (Qwen3).
    pub disable_thinking: bool,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:5211/v1".into(),
            api_key_env: None,
            api_key: None,
            extract_model: "/models/Qwen3.6-35B-A3B-FP8".into(),
            light_model: "/models/gemma-4-12B-it".into(),
            light_base_url: None,
            temperature: 0.1,
            max_tokens: 4096,
            timeout_secs: 180,
            max_retries: 2,
            json_mode: "json_schema".into(),
            disable_thinking: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingConfig {
    /// Endpoint OpenAI-compatible `/v1/embeddings` (TEI mendukungnya).
    pub base_url: String,
    pub model: String,
    pub dimension: usize,
    pub batch_size: usize,
    pub api_key_env: Option<String>,
    /// API key langsung (dipakai bila env var kosong).
    pub api_key: Option<String>,
    /// "openai" (default) atau "hash" (deterministik, tanpa server; untuk uji/offline).
    pub provider: String,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8081/v1".into(),
            model: "BAAI/bge-m3".into(),
            dimension: 1024,
            batch_size: 32,
            api_key_env: None,
            api_key: None,
            provider: "openai".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexConfig {
    /// "hnsw" (memori, cepat) atau "diskann" (disk, untuk data sangat besar).
    pub vector: String,
    pub hnsw_m: u32,
    pub hnsw_efc: u32,
    /// Parameter ef saat query.
    pub ef_search: u32,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self { vector: "hnsw".into(), hnsw_m: 16, hnsw_efc: 150, ef_search: 80 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PipelineConfig {
    /// Jendela teks maksimum per panggilan segmentasi.
    pub max_window_chars: usize,
    /// Kemiripan >= ini dengan episode lama -> dianggap penceritaan ulang.
    pub merge_similarity: f32,
    /// Kemiripan >= ini untuk kandidat rangkuman episode saat compact.
    pub compact_similarity: f32,
    /// Minimal jumlah episode dalam satu cluster untuk dirangkum saat compact.
    pub compact_min_episodes: usize,
    /// Kemiripan nama entitas >= ini -> gabung otomatis (entitas confirmed).
    pub entity_auto_merge: f32,
    /// Kemiripan di [review, auto_merge) -> adjudikasi LLM / review manusia.
    pub entity_review: f32,
    /// Fakta dengan confidence < ini dikarantina.
    pub min_fact_confidence: f32,
    /// Faktor confidence untuk detail baru dari penceritaan ulang.
    pub retelling_confidence_factor: f32,
    /// Jumlah contoh review terselesaikan yang dipakai sebagai few-shot.
    pub few_shot_examples: usize,
    /// Pekerja paralel pemroses job.
    pub workers: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_window_chars: 6000,
            merge_similarity: 0.92,
            compact_similarity: 0.78,
            compact_min_episodes: 2,
            entity_auto_merge: 0.9,
            entity_review: 0.75,
            min_fact_confidence: 0.5,
            retelling_confidence_factor: 0.8,
            few_shot_examples: 6,
            workers: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoreConfig {
    /// Ambang promosi untuk sumber volume tinggi (chat, lifelog).
    pub promote_threshold: f32,
    /// Di atas ini tapi di bawah promote -> zona abu-abu (review).
    pub gray_threshold: f32,
    /// Ambang untuk sumber biografis (wawancara, memoar).
    pub biographic_threshold: f32,
    pub max_noise_ratio: f32,
    pub min_chars: usize,
    pub stream_ttl_days: i64,
    pub half_life_days: f64,
}

impl Default for ScoreConfig {
    fn default() -> Self {
        Self {
            promote_threshold: 0.55,
            gray_threshold: 0.42,
            biographic_threshold: 0.2,
            max_noise_ratio: 0.6,
            min_chars: 40,
            stream_ttl_days: 90,
            half_life_days: 30.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DistillConfig {
    /// Minimal episode bukti untuk prinsip/disposisi.
    pub min_evidence: usize,
    /// Bukti harus tersebar di minimal sekian tahun berbeda (atau fase hidup).
    pub min_distinct_periods: usize,
    pub cluster_similarity: f32,
    /// Prinsip dengan confidence di bawah ini masuk review, bukan aktif.
    pub auto_accept_confidence: f32,
}

impl Default for DistillConfig {
    fn default() -> Self {
        Self { min_evidence: 3, min_distinct_periods: 2, cluster_similarity: 0.72, auto_accept_confidence: 0.75 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetrievalConfig {
    pub rrf_k: f32,
    pub top_k: usize,
    pub candidate_k: usize,
    /// Anggaran konteks (perkiraan token = karakter/4).
    pub context_budget_tokens: usize,
    /// Kemiripan vektor minimum agar hasil dianggap relevan.
    pub min_similarity: f32,
    pub graph_hops: usize,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self { rrf_k: 60.0, top_k: 8, candidate_k: 40, context_budget_tokens: 3000, min_similarity: 0.35, graph_hops: 2 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduleConfig {
    pub consolidate_hours: u64,
    pub distill_hours: u64,
    pub voice_hours: u64,
    pub compact_hours: u64,
    pub export_hours: u64,
    pub inbox_poll_secs: u64,
    /// Interval cek perubahan folder penting (detik, 0 = hanya manual).
    pub watch_poll_secs: u64,
    /// Interval proses ulang otomatis sumber yang gagal (menit, 0 = mati).
    pub retry_failed_minutes: u64,
    /// Batas proses ulang otomatis per sumber; setelahnya hanya manual.
    pub retry_failed_max: u32,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            consolidate_hours: 24,
            distill_hours: 24,
            voice_hours: 168,
            compact_hours: 168,
            export_hours: 24,
            inbox_poll_secs: 10,
            watch_poll_secs: 30,
            retry_failed_minutes: 10,
            retry_failed_max: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub bind: String,
    /// Token bearer opsional untuk API & MCP HTTP (env var).
    pub token_env: Option<String>,
    pub max_upload_mb: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { bind: "127.0.0.1:7878".into(), token_env: Some("MEMORY_TOKEN".into()), max_upload_mb: 512 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscriptionConfig {
    /// Endpoint OpenAI-compatible `/v1/audio/transcriptions` (Whisper).
    pub base_url: Option<String>,
    pub model: String,
    pub language: Option<String>,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self { base_url: None, model: "whisper-large-v3".into(), language: Some("id".into()) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BackupConfig {
    /// Penerima `age` (age1...) untuk enkripsi backup. Kosong = tanpa enkripsi.
    pub age_recipient: Option<String>,
}

/// API key dari env var `env` (bila terisi), selain itu dari nilai langsung.
pub fn resolve_api_key(env: Option<&str>, direct: Option<&str>) -> Option<String> {
    env.and_then(|k| std::env::var(k).ok())
        .filter(|k| !k.trim().is_empty())
        .or_else(|| direct.map(str::trim).filter(|k| !k.is_empty()).map(String::from))
}

impl LlmConfig {
    pub fn resolved_api_key(&self) -> Option<String> {
        resolve_api_key(self.api_key_env.as_deref(), self.api_key.as_deref())
    }
}

impl EmbeddingConfig {
    pub fn resolved_api_key(&self) -> Option<String> {
        resolve_api_key(self.api_key_env.as_deref(), self.api_key.as_deref())
    }
}

impl MemoryConfig {
    pub fn from_toml(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    /// Konfigurasi untuk uji: embedding hash, vault in-memory.
    pub fn for_tests(dimension: usize) -> Self {
        let mut c = Self::default();
        c.vault.engine = "mem".into();
        c.embedding.provider = "hash".into();
        c.embedding.dimension = dimension;
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrips_through_toml() {
        let c = MemoryConfig::default();
        let back = MemoryConfig::from_toml(&c.to_toml()).unwrap();
        assert_eq!(back.embedding.dimension, 1024);
        assert_eq!(back.llm.base_url, "http://localhost:5211/v1");
    }

    #[test]
    fn partial_toml_uses_defaults() {
        let c = MemoryConfig::from_toml("[vault]\ndefault_subject = \"bapak\"\n").unwrap();
        assert_eq!(c.vault.default_subject, "bapak");
        assert_eq!(c.pipeline.merge_similarity, 0.92);
    }
}
