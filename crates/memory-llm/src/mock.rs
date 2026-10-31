//! Provider tiruan untuk uji dan mode offline.

use async_trait::async_trait;

use crate::{ChatRequest, LlmProvider, Result};

type Handler = dyn Fn(&ChatRequest) -> Result<String> + Send + Sync;

/// LLM yang jawabannya ditentukan closure, biasanya bercabang pada `req.task`.
pub struct ScriptedLlm {
    handler: Box<Handler>,
}

impl ScriptedLlm {
    pub fn new(f: impl Fn(&ChatRequest) -> Result<String> + Send + Sync + 'static) -> Self {
        Self { handler: Box::new(f) }
    }
}

#[async_trait]
impl LlmProvider for ScriptedLlm {
    async fn chat(&self, req: ChatRequest) -> Result<String> {
        (self.handler)(&req)
    }
    fn describe(&self) -> String {
        "scripted".into()
    }
}

/// Isi pesan user terakhir (utilitas untuk handler tiruan).
pub fn last_user(req: &ChatRequest) -> &str {
    req.messages.iter().rev().find(|m| m.role == "user").map(|m| m.content.as_str()).unwrap_or("")
}
