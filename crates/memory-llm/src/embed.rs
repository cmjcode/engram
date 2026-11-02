use std::time::Duration;

use async_trait::async_trait;
use memory_core::config::EmbeddingConfig;
use serde_json::json;

use crate::{Embedder, LlmError, Result};

pub fn l2_normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

/// Kemiripan kosinus. Vektor kosong atau beda dimensi -> 0.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na.sqrt() * nb.sqrt()) }
}

/// Embedding via `/v1/embeddings` (TEI, vLLM, OpenAI).
pub struct OpenAiEmbedder {
    http: reqwest::Client,
    cfg: EmbeddingConfig,
    api_key: Option<String>,
}

impl OpenAiEmbedder {
    pub fn new(cfg: &EmbeddingConfig) -> Self {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(120)).build().unwrap_or_default();
        let api_key = cfg.resolved_api_key();
        Self { http, cfg: cfg.clone(), api_key }
    }
}

#[async_trait]
impl Embedder for OpenAiEmbedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let url = format!("{}/embeddings", self.cfg.base_url.trim_end_matches('/'));
        let mut out = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(self.cfg.batch_size.max(1)) {
            let mut rb = self.http.post(&url).json(&json!({ "model": self.cfg.model, "input": chunk }));
            if let Some(k) = &self.api_key {
                rb = rb.bearer_auth(k);
            }
            let resp = rb.send().await?;
            let status = resp.status();
            let body = resp.text().await?;
            if !status.is_success() {
                return Err(LlmError::Status { status: status.as_u16(), body: body.chars().take(500).collect() });
            }
            let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| LlmError::Parse(e.to_string()))?;
            let mut data: Vec<(usize, Vec<f32>)> = v["data"]
                .as_array()
                .ok_or_else(|| LlmError::Parse("field data tidak ada".into()))?
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    let idx = d["index"].as_u64().map(|x| x as usize).unwrap_or(i);
                    let vec = d["embedding"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect())
                        .unwrap_or_default();
                    (idx, vec)
                })
                .collect();
            data.sort_by_key(|(i, _)| *i);
            for (_, mut vec) in data {
                if vec.len() != self.cfg.dimension {
                    return Err(LlmError::Other(format!(
                        "dimensi embedding {} != konfigurasi {}",
                        vec.len(),
                        self.cfg.dimension
                    )));
                }
                l2_normalize(&mut vec);
                out.push(vec);
            }
        }
        Ok(out)
    }

    fn dimension(&self) -> usize {
        self.cfg.dimension
    }
}

/// Embedding deterministik tanpa server: token kata dan trigram karakter di-hash
/// ke bucket bertanda. Teks yang berbagi kata menghasilkan kemiripan tinggi.
pub struct HashEmbedder {
    dim: usize,
}

impl HashEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim: dim.max(8) }
    }

    fn vector(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0f32; self.dim];
        let toks = memory_core::text::tokens(text);
        let mut add = |feature: &str, w: f32| {
            let h = blake3::hash(feature.as_bytes());
            let b = h.as_bytes();
            let idx = u64::from_le_bytes(b[0..8].try_into().unwrap_or([0; 8])) as usize % self.dim;
            let sign = if b[8] & 1 == 0 { 1.0 } else { -1.0 };
            v[idx] += sign * w;
        };
        for t in &toks {
            add(&format!("w:{t}"), 1.0);
            let chars: Vec<char> = format!(" {t} ").chars().collect();
            for w in chars.windows(3) {
                add(&format!("c:{}", w.iter().collect::<String>()), 0.3);
            }
        }
        l2_normalize(&mut v);
        v
    }
}

#[async_trait]
impl Embedder for HashEmbedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| self.vector(t)).collect())
    }
    fn dimension(&self) -> usize {
        self.dim
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_embedder_similarity_is_meaningful() {
        let e = HashEmbedder::new(256);
        let v = e
            .embed(&[
                "Bapak pindah ke Jogja tahun 2015".into(),
                "tahun 2015 bapak pindah ke Jogja".into(),
                "resep sambal terasi pedas".into(),
            ])
            .await
            .unwrap();
        assert!(cosine(&v[0], &v[1]) > 0.9);
        assert!(cosine(&v[0], &v[2]) < 0.3);
    }

    #[tokio::test]
    async fn openai_embedder_parses_and_checks_dimension() {
        use axum::{Json, Router, routing::post};
        let app = Router::new().route(
            "/v1/embeddings",
            post(|Json(body): Json<serde_json::Value>| async move {
                let n = body["input"].as_array().map(|a| a.len()).unwrap_or(0);
                let data: Vec<_> = (0..n).rev().map(|i| json!({"index": i, "embedding": [i as f32 + 1.0, 0.0, 0.0, 0.0]})).collect();
                Json(json!({ "data": data }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let cfg = EmbeddingConfig { base_url: format!("http://{addr}/v1"), dimension: 4, batch_size: 2, ..Default::default() };
        let e = OpenAiEmbedder::new(&cfg);
        let v = e.embed(&["a".into(), "b".into(), "c".into()]).await.unwrap();
        assert_eq!(v.len(), 3);
        assert!((v[0][0] - 1.0).abs() < 1e-6);
        let bad = OpenAiEmbedder::new(&EmbeddingConfig { dimension: 8, ..cfg });
        assert!(bad.embed(&["a".into()]).await.is_err());
    }
}
