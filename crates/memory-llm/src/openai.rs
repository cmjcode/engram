use std::time::Duration;

use async_trait::async_trait;
use memory_core::config::LlmConfig;
use serde_json::json;

use crate::{ChatRequest, LlmError, LlmProvider, ModelRole, Result};

/// Klien chat completion OpenAI-compatible (vLLM, llama.cpp server, OpenAI, ...).
pub struct OpenAiCompatible {
    http: reqwest::Client,
    cfg: LlmConfig,
    api_key: Option<String>,
}

impl OpenAiCompatible {
    pub fn new(cfg: &LlmConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(cfg.timeout_secs))
            .build()
            .unwrap_or_default();
        let api_key = cfg.resolved_api_key();
        Self { http, cfg: cfg.clone(), api_key }
    }

    fn endpoint(&self, role: ModelRole) -> (String, String) {
        match role {
            ModelRole::Extract => (self.cfg.base_url.clone(), self.cfg.extract_model.clone()),
            ModelRole::Light => (
                self.cfg.light_base_url.clone().unwrap_or_else(|| self.cfg.base_url.clone()),
                self.cfg.light_model.clone(),
            ),
        }
    }

    pub fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        let (_, model) = self.endpoint(req.role);
        let mut body = json!({
            "model": model,
            "messages": req.messages,
            "temperature": req.temperature.unwrap_or(self.cfg.temperature),
            "max_tokens": req.max_tokens.unwrap_or(self.cfg.max_tokens),
        });
        if let Some((name, schema)) = &req.json_schema {
            match self.cfg.json_mode.as_str() {
                "json_schema" => {
                    body["response_format"] = json!({
                        "type": "json_schema",
                        "json_schema": { "name": name, "schema": schema, "strict": false }
                    });
                }
                "json_object" => body["response_format"] = json!({ "type": "json_object" }),
                _ => {}
            }
        }
        if self.cfg.disable_thinking {
            body["chat_template_kwargs"] = json!({ "enable_thinking": false });
        }
        body
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatible {
    async fn chat(&self, req: ChatRequest) -> Result<String> {
        let (base, _) = self.endpoint(req.role);
        let url = format!("{}/chat/completions", base.trim_end_matches('/'));
        let body = self.build_body(&req);
        let mut rb = self.http.post(&url).json(&body);
        if let Some(k) = &self.api_key {
            rb = rb.bearer_auth(k);
        }
        let started = std::time::Instant::now();
        let resp = rb.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(LlmError::Status { status: status.as_u16(), body: text.chars().take(500).collect() });
        }
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| LlmError::Parse(e.to_string()))?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| LlmError::Parse("choices[0].message.content kosong".into()))?;
        tracing::debug!(task = %req.task, ms = started.elapsed().as_millis() as u64, "llm selesai");
        Ok(content.to_string())
    }

    async fn health(&self, role: ModelRole) -> Result<()> {
        let (base, _) = self.endpoint(role);
        let mut rb = self.http.get(format!("{}/models", base.trim_end_matches('/'))).timeout(Duration::from_secs(3));
        if let Some(k) = &self.api_key {
            rb = rb.bearer_auth(k);
        }
        let resp = rb.send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(LlmError::Status { status: status.as_u16(), body: resp.text().await.unwrap_or_default().chars().take(200).collect() });
        }
        Ok(())
    }

    fn describe(&self) -> String {
        format!("{} [extract={}, light={}]", self.cfg.base_url, self.cfg.extract_model, self.cfg.light_model)
    }
}
