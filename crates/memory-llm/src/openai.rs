use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use memory_core::config::{LlmConfig, is_auto_model};
use serde_json::json;

use crate::{ChatRequest, LlmError, LlmProvider, ModelRole, Result};

/// Klien chat completion OpenAI-compatible (vLLM, llama.cpp server, OpenAI, ...).
///
/// Nama model `"auto"` dipetakan ke model pertama di `GET {base}/models`. Hasilnya
/// disimpan per server dan dipilih ulang bila server menjawab 404 (model diganti)
/// atau saat cek kesehatan, sehingga pergantian model di server tidak butuh restart.
pub struct OpenAiCompatible {
    http: reqwest::Client,
    cfg: LlmConfig,
    api_key: Option<String>,
    /// base_url -> model hasil resolusi `"auto"`.
    auto_models: Mutex<HashMap<String, String>>,
}

impl OpenAiCompatible {
    pub fn new(cfg: &LlmConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(cfg.timeout_secs))
            .build()
            .unwrap_or_default();
        let api_key = cfg.resolved_api_key();
        Self { http, cfg: cfg.clone(), api_key, auto_models: Mutex::new(HashMap::new()) }
    }

    /// (base_url, nama model seperti di konfigurasi) untuk peran ini.
    fn endpoint(&self, role: ModelRole) -> (String, String) {
        match role {
            ModelRole::Extract => (self.cfg.base_url.clone(), self.cfg.extract_model.clone()),
            ModelRole::Light => (
                self.cfg
                    .light_base_url
                    .clone()
                    .filter(|u| !u.trim().is_empty())
                    .unwrap_or_else(|| self.cfg.base_url.clone()),
                self.cfg.light_model.clone(),
            ),
        }
    }

    fn cached_auto(&self, base: &str) -> Option<String> {
        self.auto_models.lock().ok()?.get(base).cloned()
    }

    fn forget_auto(&self, base: &str) {
        if let Ok(mut m) = self.auto_models.lock() {
            m.remove(base);
        }
    }

    /// Daftar id model di `{base}/models`.
    async fn list_models(&self, base: &str, timeout: Option<Duration>) -> Result<Vec<String>> {
        let mut rb = self.http.get(format!("{}/models", base.trim_end_matches('/')));
        if let Some(t) = timeout {
            rb = rb.timeout(t);
        }
        if let Some(k) = &self.api_key {
            rb = rb.bearer_auth(k);
        }
        let resp = rb.send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(LlmError::Status {
                status: status.as_u16(),
                body: resp.text().await.unwrap_or_default().chars().take(200).collect(),
            });
        }
        let v: serde_json::Value = resp.json().await.map_err(|e| LlmError::Parse(e.to_string()))?;
        Ok(v["data"]
            .as_array()
            .map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect())
            .unwrap_or_default())
    }

    /// Ambil ulang model `"auto"` dari server dan simpan.
    async fn refresh_auto(&self, base: &str, timeout: Option<Duration>) -> Result<String> {
        let model = self
            .list_models(base, timeout)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::Other(format!("server {base} tidak melayani model apa pun")))?;
        if let Ok(mut m) = self.auto_models.lock() {
            if m.get(base) != Some(&model) {
                tracing::info!(base, model = %model, "model otomatis dipilih dari server");
            }
            m.insert(base.to_string(), model.clone());
        }
        Ok(model)
    }

    /// (base_url, nama model yang dikirim ke server) untuk peran ini.
    async fn resolve(&self, role: ModelRole) -> Result<(String, String)> {
        let (base, model) = self.endpoint(role);
        if !is_auto_model(&model) {
            return Ok((base, model));
        }
        if let Some(m) = self.cached_auto(&base) {
            return Ok((base, m));
        }
        let m = self.refresh_auto(&base, None).await?;
        Ok((base, m))
    }

    /// Body permintaan dengan nama model dari konfigurasi (tanpa resolusi `"auto"`).
    pub fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        let (_, model) = self.endpoint(req.role);
        self.build_body_for(req, &model)
    }

    fn build_body_for(&self, req: &ChatRequest, model: &str) -> serde_json::Value {
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

    async fn send_chat(&self, base: &str, model: &str, req: &ChatRequest) -> Result<String> {
        let url = format!("{}/chat/completions", base.trim_end_matches('/'));
        let mut rb = self.http.post(&url).json(&self.build_body_for(req, model));
        if let Some(k) = &self.api_key {
            rb = rb.bearer_auth(k);
        }
        let resp = rb.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(LlmError::Status { status: status.as_u16(), body: text.chars().take(500).collect() });
        }
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| LlmError::Parse(e.to_string()))?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| LlmError::Parse("choices[0].message.content kosong".into()))
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatible {
    async fn chat(&self, req: ChatRequest) -> Result<String> {
        let auto = is_auto_model(&self.endpoint(req.role).1);
        let (base, model) = self.resolve(req.role).await?;
        let started = std::time::Instant::now();
        let out = match self.send_chat(&base, &model, &req).await {
            // Model "auto" hilang dari server (diganti): pilih ulang lalu coba sekali lagi.
            Err(LlmError::Status { status: 404, .. }) if auto => {
                self.forget_auto(&base);
                let fresh = self.refresh_auto(&base, None).await?;
                tracing::warn!(old = %model, new = %fresh, "model otomatis tidak ada lagi di server; diganti");
                self.send_chat(&base, &fresh, &req).await?
            }
            r => r?,
        };
        tracing::debug!(task = %req.task, ms = started.elapsed().as_millis() as u64, "llm selesai");
        Ok(out)
    }

    /// Server bisa dihubungi dan model untuk peran ini tersedia. Untuk `"auto"`,
    /// sekaligus memperbarui model pilihan bila server sudah ganti model.
    async fn health(&self, role: ModelRole) -> Result<()> {
        let (base, model) = self.endpoint(role);
        let timeout = Some(Duration::from_secs(3));
        if is_auto_model(&model) {
            self.refresh_auto(&base, timeout).await?;
            return Ok(());
        }
        let models = self.list_models(&base, timeout).await?;
        if !models.is_empty() && !models.contains(&model) {
            return Err(LlmError::Other(format!(
                "model \"{model}\" tidak ada di server (tersedia: {}); ganti nama model atau pakai \"auto\"",
                models.join(", ")
            )));
        }
        Ok(())
    }

    fn describe(&self) -> String {
        let show = |role| {
            let (base, model) = self.endpoint(role);
            if is_auto_model(&model) {
                match self.cached_auto(&base) {
                    Some(m) => format!("auto:{m}"),
                    None => "auto".into(),
                }
            } else {
                model
            }
        };
        format!("{} [extract={}, light={}]", self.cfg.base_url, show(ModelRole::Extract), show(ModelRole::Light))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing::{get, post};
    use axum::{Json, Router};

    use super::*;
    use crate::Message;

    /// Server tiruan yang melayani satu model (bisa diganti saat uji berjalan).
    async fn fake_server(model: Arc<Mutex<String>>) -> String {
        async fn models(State(m): State<Arc<Mutex<String>>>) -> Json<serde_json::Value> {
            let id = m.lock().unwrap().clone();
            Json(json!({"object": "list", "data": [{"id": id, "object": "model"}]}))
        }
        async fn chat(
            State(m): State<Arc<Mutex<String>>>,
            Json(body): Json<serde_json::Value>,
        ) -> (StatusCode, Json<serde_json::Value>) {
            let served = m.lock().unwrap().clone();
            if body["model"].as_str() != Some(served.as_str()) {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({"error": {"message": format!("The model `{}` does not exist.", body["model"]), "code": 404}})),
                );
            }
            (StatusCode::OK, Json(json!({"choices": [{"message": {"content": format!("dari {served}")}}]})))
        }
        let app = Router::new()
            .route("/v1/models", get(models))
            .route("/v1/chat/completions", post(chat))
            .with_state(model);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}/v1")
    }

    fn req(role: ModelRole) -> ChatRequest {
        ChatRequest {
            role,
            messages: vec![Message::user("halo")],
            json_schema: None,
            temperature: None,
            max_tokens: None,
            task: "uji".into(),
        }
    }

    fn client(base: String, extract: &str, light: &str) -> OpenAiCompatible {
        OpenAiCompatible::new(&LlmConfig {
            base_url: base,
            extract_model: extract.into(),
            light_model: light.into(),
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn auto_follows_server_model_change() {
        let served = Arc::new(Mutex::new("model-lama".to_string()));
        let c = client(fake_server(served.clone()).await, "auto", "");
        assert_eq!(c.chat(req(ModelRole::Extract)).await.unwrap(), "dari model-lama");
        // Light kosong = auto juga.
        assert_eq!(c.chat(req(ModelRole::Light)).await.unwrap(), "dari model-lama");

        *served.lock().unwrap() = "model-baru".into();
        assert_eq!(c.chat(req(ModelRole::Extract)).await.unwrap(), "dari model-baru");
        assert!(c.describe().contains("extract=auto:model-baru"));
    }

    #[tokio::test]
    async fn health_refreshes_auto_and_rejects_missing_explicit_model() {
        let served = Arc::new(Mutex::new("qwen-a".to_string()));
        let base = fake_server(served.clone()).await;

        let auto = client(base.clone(), "AUTO", "auto");
        auto.health(ModelRole::Extract).await.unwrap();
        *served.lock().unwrap() = "qwen-b".into();
        auto.health(ModelRole::Extract).await.unwrap();
        assert_eq!(auto.cached_auto(&base).as_deref(), Some("qwen-b"));

        let explicit = client(base, "qwen-a", "qwen-b");
        let err = explicit.health(ModelRole::Extract).await.unwrap_err().to_string();
        assert!(err.contains("qwen-a") && err.contains("qwen-b"), "{err}");
        explicit.health(ModelRole::Light).await.unwrap();
    }

    #[tokio::test]
    async fn explicit_model_is_not_replaced_on_404() {
        let served = Arc::new(Mutex::new("ada".to_string()));
        let c = client(fake_server(served).await, "hilang", "hilang");
        let err = c.chat(req(ModelRole::Extract)).await.unwrap_err();
        assert!(matches!(err, LlmError::Status { status: 404, .. }));
    }
}
