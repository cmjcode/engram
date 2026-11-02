//! Abstraksi LLM dan embedding.
//!
//! - [`LlmProvider`]: chat completion; [`complete_json`] membungkusnya dengan
//!   skema JSON (guided decoding vLLM) dan retry perbaikan.
//! - [`Embedder`]: vektor teks; [`OpenAiEmbedder`] untuk TEI/vLLM,
//!   [`HashEmbedder`] deterministik tanpa server (uji/offline).
//! - [`Transcriber`]: audio ke teks via endpoint Whisper OpenAI-compatible.

mod embed;
pub mod mock;
mod openai;
mod transcribe;

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub use embed::{HashEmbedder, OpenAiEmbedder, cosine, l2_normalize};
pub use openai::OpenAiCompatible;
pub use transcribe::{Transcriber, WhisperTranscriber};

use memory_core::MemoryConfig;

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("status {status}: {body}")]
    Status { status: u16, body: String },
    #[error("respons tidak bisa diparse: {0}")]
    Parse(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, LlmError>;

/// Peran model: model besar untuk ekstraksi/distilasi, ringan untuk klasifikasi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelRole {
    Extract,
    Light,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(c: impl Into<String>) -> Self {
        Self { role: "system".into(), content: c.into() }
    }
    pub fn user(c: impl Into<String>) -> Self {
        Self { role: "user".into(), content: c.into() }
    }
    pub fn assistant(c: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: c.into() }
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub role: ModelRole,
    pub messages: Vec<Message>,
    /// (nama, skema) untuk guided JSON.
    pub json_schema: Option<(String, serde_json::Value)>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// Nama tugas untuk logging & provider tiruan ("extract", "segment", ...).
    pub task: String,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<String>;
    fn describe(&self) -> String;

    /// Cek ringan bahwa server untuk peran ini bisa dihubungi (tanpa generate).
    async fn health(&self, _role: ModelRole) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
pub trait Embedder: Send + Sync {
    /// Vektor ter-normalisasi L2, satu per teks.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn dimension(&self) -> usize;

    async fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = self.embed(&[text.to_string()]).await?;
        v.pop().ok_or_else(|| LlmError::Other("embedding kosong".into()))
    }
}

/// Buang blok `<think>…</think>` dan pagar markdown, ambil objek JSON terluar.
pub fn extract_json_block(raw: &str) -> &str {
    let mut s = raw;
    if let Some(end) = s.rfind("</think>") {
        s = &s[end + "</think>".len()..];
    }
    let start = s.find(['{', '[']);
    let end = s.rfind(['}', ']']);
    match (start, end) {
        (Some(a), Some(b)) if b >= a => &s[a..=b],
        _ => s.trim(),
    }
}

/// Panggil LLM dan parse keluaran ke `T` sesuai skema JSON `T`. Bila gagal
/// parse, kirim ulang dengan pesan galat (maks `retries` kali).
pub async fn complete_json<T: DeserializeOwned + JsonSchema>(
    llm: &dyn LlmProvider,
    role: ModelRole,
    task: &str,
    system: &str,
    user: &str,
    retries: u32,
) -> Result<T> {
    let schema = serde_json::to_value(schemars::schema_for!(T)).map_err(|e| LlmError::Other(e.to_string()))?;
    let mut messages = vec![Message::system(system), Message::user(user)];
    let mut last_err = String::new();
    for attempt in 0..=retries {
        let req = ChatRequest {
            role,
            messages: messages.clone(),
            json_schema: Some((task.replace(|c: char| !c.is_ascii_alphanumeric(), "_"), schema.clone())),
            temperature: None,
            max_tokens: None,
            task: task.to_string(),
        };
        let raw = llm.chat(req).await?;
        match serde_json::from_str::<T>(extract_json_block(&raw)) {
            Ok(v) => return Ok(v),
            Err(e) => {
                tracing::warn!(task, attempt, error = %e, "keluaran LLM bukan JSON valid");
                last_err = e.to_string();
                messages.push(Message::assistant(raw));
                messages.push(Message::user(format!(
                    "Keluaran tadi tidak valid terhadap skema: {e}. Kirim ulang HANYA objek JSON yang valid sesuai skema, tanpa penjelasan."
                )));
            }
        }
    }
    Err(LlmError::Parse(last_err))
}

/// Bangun provider & embedder dari konfigurasi.
pub fn llm_from_config(cfg: &MemoryConfig) -> Arc<dyn LlmProvider> {
    Arc::new(OpenAiCompatible::new(&cfg.llm))
}

pub fn embedder_from_config(cfg: &MemoryConfig) -> Arc<dyn Embedder> {
    match cfg.embedding.provider.as_str() {
        "hash" => Arc::new(HashEmbedder::new(cfg.embedding.dimension)),
        _ => Arc::new(OpenAiEmbedder::new(&cfg.embedding)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::ScriptedLlm;

    #[derive(Debug, Deserialize, JsonSchema)]
    struct Out {
        answer: String,
    }

    #[test]
    fn strips_think_and_fences() {
        let raw = "<think>hmm {not this}</think>\n```json\n{\"answer\": \"ya\"}\n```";
        assert_eq!(extract_json_block(raw), "{\"answer\": \"ya\"}");
    }

    #[tokio::test]
    async fn retries_on_invalid_json() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let c = calls.clone();
        let llm = ScriptedLlm::new(move |req| {
            let n = c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert!(req.json_schema.is_some());
            Ok(if n == 0 { "bukan json".into() } else { "{\"answer\":\"ok\"}".into() })
        });
        let out: Out = complete_json(&llm, ModelRole::Light, "t", "s", "u", 2).await.unwrap();
        assert_eq!(out.answer, "ok");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn gives_up_after_retries() {
        let llm = ScriptedLlm::new(|_| Ok("nope".into()));
        let r: Result<Out> = complete_json(&llm, ModelRole::Light, "t", "s", "u", 1).await;
        assert!(matches!(r, Err(LlmError::Parse(_))));
    }
}
