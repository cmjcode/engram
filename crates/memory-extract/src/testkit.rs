//! Perlengkapan uji: pipeline dengan DB memori, embedding hash, LLM tiruan.

use std::sync::Arc;

use memory_core::MemoryConfig;
use memory_llm::mock::ScriptedLlm;
use memory_llm::{ChatRequest, HashEmbedder};
use memory_store::Store;

use crate::{Pipeline, Vault};

pub const DIM: usize = 256;

pub async fn pipeline_with(handler: impl Fn(&ChatRequest) -> memory_llm::Result<String> + Send + Sync + 'static) -> (Pipeline, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = MemoryConfig::for_tests(DIM);
    let store = Store::memory(DIM, &cfg.index).await.unwrap();
    let vault = Vault::new(dir.path());
    vault.init().unwrap();
    let p = Pipeline::new(store, Arc::new(ScriptedLlm::new(handler)), Arc::new(HashEmbedder::new(DIM)), Arc::new(cfg), vault);
    (p, dir)
}
