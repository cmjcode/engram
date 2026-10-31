//! Runtime platform: merakit store + LLM + pipeline + retriever, menjalankan
//! antrean job (worker paralel, retry dengan backoff, progres batch) dan
//! scheduler pekerjaan berkala (konsolidasi, distilasi, profil suara, ekspor).

pub mod archive;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::watch;

use memory_core::model::{Job, JobStatus};
use memory_core::{MemoryConfig, ids::now_rfc3339};
use memory_extract::ingest::PROCESS_SOURCE;
use memory_extract::review::Decision;
use memory_extract::{Pipeline, Vault};
use memory_llm::{Embedder, LlmProvider};
use memory_retrieve::Retriever;
use memory_store::Store;

pub const PROMOTE_STREAM: &str = "promote_stream";
pub const CONSOLIDATE: &str = "consolidate";
pub const DISTILL: &str = "distill";
pub const VOICE: &str = "voice";
pub const EXPORT: &str = "export";
pub const REVIEW: &str = "review";

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error(transparent)]
    Store(#[from] memory_store::StoreError),
    #[error(transparent)]
    Extract(#[from] memory_extract::ExtractError),
    #[error(transparent)]
    Retrieve(#[from] memory_retrieve::RetrieveError),
    #[error(transparent)]
    Llm(#[from] memory_llm::LlmError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, JobError>;

#[derive(Clone)]
pub struct Runtime {
    pub cfg: Arc<MemoryConfig>,
    pub store: Store,
    pub pipeline: Pipeline,
    pub retriever: Retriever,
    pub vault: Vault,
}

impl Runtime {
    /// Buka vault sesuai konfigurasi (membuat folder bila belum ada).
    pub async fn open(cfg: MemoryConfig) -> Result<Self> {
        let llm = memory_llm::llm_from_config(&cfg);
        let embedder = memory_llm::embedder_from_config(&cfg);
        Self::open_with(cfg, llm, embedder).await
    }

    pub async fn open_with(cfg: MemoryConfig, llm: Arc<dyn LlmProvider>, embedder: Arc<dyn Embedder>) -> Result<Self> {
        let vault = Vault::new(&cfg.vault.path);
        vault.init()?;
        let store = match cfg.vault.engine.as_str() {
            "mem" => Store::memory(cfg.embedding.dimension, &cfg.index).await?,
            _ => Store::open(&vault.db_dir(), cfg.embedding.dimension, &cfg.index).await?,
        };
        Ok(Self::from_parts(cfg, store, llm, embedder, vault))
    }

    pub fn from_parts(cfg: MemoryConfig, store: Store, llm: Arc<dyn LlmProvider>, embedder: Arc<dyn Embedder>, vault: Vault) -> Self {
        let cfg = Arc::new(cfg);
        let pipeline = Pipeline::new(store.clone(), llm.clone(), embedder.clone(), cfg.clone(), vault.clone());
        let retriever = Retriever::new(store.clone(), embedder, llm, cfg.clone());
        Self { cfg, store, pipeline, retriever, vault }
    }

    pub async fn enqueue(&self, kind: &str, payload: Value, key: String) -> Result<Job> {
        let (job, _) = self.store.enqueue(&Job::new(kind, payload, key, None)).await?;
        Ok(job)
    }

    /// Jalankan satu job sesuai jenisnya.
    pub async fn run_job(&self, job: &Job) -> Result<Value> {
        let s = |k: &str| job.payload[k].as_str().map(str::to_string).ok_or_else(|| JobError::Invalid(format!("payload job {} tanpa '{k}'", job.kind)));
        Ok(match job.kind.as_str() {
            PROCESS_SOURCE => json!(self.pipeline.process_source(&s("source_id")?, Some(&job.id)).await?),
            PROMOTE_STREAM => json!(self.pipeline.promote_stream_item(&s("item_id")?).await?),
            CONSOLIDATE => json!(self.pipeline.consolidate(&s("subject")?).await?),
            DISTILL => json!(self.pipeline.distill(&s("subject")?).await?),
            VOICE => json!(self.pipeline.build_voice_profile(&s("subject")?).await?),
            EXPORT => json!(archive::export(self, None).await?.display().to_string()),
            REVIEW => {
                let d = if job.payload["decision"].as_str() == Some("approve") { Decision::Approve } else { Decision::Reject };
                json!(self.pipeline.resolve_review(&s("review_id")?, d, job.payload["note"].as_str().map(str::to_string)).await?)
            }
            other => return Err(JobError::Invalid(format!("jenis job tidak dikenal: {other}"))),
        })
    }

    /// Klaim & jalankan satu job. `Ok(false)` bila antrean kosong.
    pub async fn step(&self) -> Result<bool> {
        let job = match self.store.claim_job(&now_rfc3339(), None).await {
            Ok(Some(job)) => job,
            Ok(None) => return Ok(false),
            // Worker lain mengklaim job yang sama pada saat bersamaan: coba lagi nanti.
            Err(e) if e.is_retryable() => {
                tracing::debug!("klaim job bentrok, diulang");
                return Ok(false);
            }
            Err(e) => return Err(e.into()),
        };
        let started = std::time::Instant::now();
        let res = self.run_job(&job).await;
        let outcome = res.as_ref().map(|_| ()).map_err(|e| e.to_string());
        if let Err(e) = &outcome {
            tracing::warn!(job = %job.id, kind = %job.kind, attempt = job.attempts, error = %e, "job gagal");
        } else {
            tracing::info!(job = %job.id, kind = %job.kind, ms = started.elapsed().as_millis() as u64, "job selesai");
        }
        self.store.finish_job(&job, outcome.clone()).await?;
        if let Some(b) = &job.batch_id {
            match &outcome {
                Ok(()) => self.store.batch_incr(b, "done", None).await?,
                Err(e) if job.attempts >= job.max_attempts => self.store.batch_incr(b, "failed", Some(format!("{}: {e}", job.payload["source_id"].as_str().unwrap_or("?")))).await?,
                Err(_) => {}
            }
        }
        Ok(true)
    }

    /// Jalankan semua job yang siap sampai antrean kosong (untuk CLI & uji).
    pub async fn drain(&self) -> Result<u32> {
        let mut n = 0;
        while self.step().await? {
            n += 1;
        }
        Ok(n)
    }

    /// Worker paralel sampai `shutdown` bernilai true.
    pub async fn run_workers(&self, mut shutdown: watch::Receiver<bool>) {
        let requeued = self.store.requeue_stale_jobs().await.unwrap_or(0);
        if requeued > 0 {
            tracing::info!(requeued, "job tertinggal diantre ulang");
        }
        let mut handles = Vec::new();
        for w in 0..self.cfg.pipeline.workers.max(1) {
            let rt = self.clone();
            let mut sd = shutdown.clone();
            handles.push(tokio::spawn(async move {
                loop {
                    if *sd.borrow() {
                        break;
                    }
                    match rt.step().await {
                        Ok(true) => continue,
                        Ok(false) => {}
                        Err(e) => tracing::error!(worker = w, error = %e, "worker error"),
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                        _ = sd.changed() => {}
                    }
                }
            }));
        }
        let _ = shutdown.changed().await;
        for h in handles {
            let _ = h.await;
        }
    }

    /// Antrekan pekerjaan berkala yang jatuh tempo (idempoten per periode).
    pub async fn schedule_due(&self) -> Result<u32> {
        let sc = &self.cfg.schedule;
        let now = chrono::Utc::now().timestamp() as u64;
        let mut n = 0;
        let bucket = |hours: u64| now / (hours.max(1) * 3600);
        for subject in self.store.list_subjects().await? {
            for (kind, hours) in [(CONSOLIDATE, sc.consolidate_hours), (DISTILL, sc.distill_hours), (VOICE, sc.voice_hours)] {
                let key = format!("{kind}:{}:{}", subject.id, bucket(hours));
                let (_, created) = self.store.enqueue(&Job::new(kind, json!({"subject": subject.id}), key, None)).await?;
                n += u32::from(created);
            }
        }
        let (_, created) = self.store.enqueue(&Job::new(EXPORT, json!({}), format!("export:{}", bucket(sc.export_hours)), None)).await?;
        n += u32::from(created);
        Ok(n)
    }

    pub async fn run_scheduler(&self, mut shutdown: watch::Receiver<bool>) {
        loop {
            if let Err(e) = self.schedule_due().await {
                tracing::error!(error = %e, "scheduler error");
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(300)) => {}
                _ = shutdown.changed() => break,
            }
        }
    }

    pub async fn job_summary(&self) -> Result<Value> {
        let mut out = serde_json::Map::new();
        for st in [JobStatus::Queued, JobStatus::Running, JobStatus::Failed] {
            let jobs = self.store.list_jobs(Some(st), 200).await?;
            out.insert(format!("{st:?}").to_lowercase(), json!(jobs.len()));
        }
        Ok(Value::Object(out))
    }
}

#[cfg(test)]
mod tests;
