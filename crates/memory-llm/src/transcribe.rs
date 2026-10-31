use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use memory_core::config::TranscriptionConfig;

use crate::{LlmError, Result};

#[async_trait]
pub trait Transcriber: Send + Sync {
    async fn transcribe(&self, path: &Path) -> Result<String>;
}

/// Transkripsi via endpoint OpenAI-compatible `/v1/audio/transcriptions`
/// (faster-whisper-server, vLLM Whisper, OpenAI).
pub struct WhisperTranscriber {
    http: reqwest::Client,
    base_url: String,
    cfg: TranscriptionConfig,
}

impl WhisperTranscriber {
    pub fn new(cfg: &TranscriptionConfig) -> Option<Self> {
        let base_url = cfg.base_url.clone()?;
        let http = reqwest::Client::builder().timeout(Duration::from_secs(1800)).build().ok()?;
        Some(Self { http, base_url, cfg: cfg.clone() })
    }
}

#[async_trait]
impl Transcriber for WhisperTranscriber {
    async fn transcribe(&self, path: &Path) -> Result<String> {
        let bytes = tokio::fs::read(path).await.map_err(|e| LlmError::Other(e.to_string()))?;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "audio".into());
        let mut form = reqwest::multipart::Form::new()
            .part("file", reqwest::multipart::Part::bytes(bytes).file_name(name))
            .text("model", self.cfg.model.clone())
            .text("response_format", "text");
        if let Some(lang) = &self.cfg.language {
            form = form.text("language", lang.clone());
        }
        let url = format!("{}/audio/transcriptions", self.base_url.trim_end_matches('/'));
        let resp = self.http.post(url).multipart(form).send().await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(LlmError::Status { status: status.as_u16(), body: body.chars().take(500).collect() });
        }
        // Beberapa server mengembalikan JSON {"text": ...} walau diminta text.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body)
            && let Some(t) = v["text"].as_str() {
                return Ok(t.to_string());
            }
        Ok(body)
    }
}
