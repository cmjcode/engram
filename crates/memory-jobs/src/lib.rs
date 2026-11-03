//! Runtime platform: merakit store + LLM + pipeline + retriever, menjalankan
//! antrean job (worker paralel, retry dengan backoff, progres batch) dan
//! scheduler pekerjaan berkala (konsolidasi, distilasi, profil suara, ekspor).

pub mod archive;
pub mod storage;
pub mod sync;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::watch;

use memory_core::model::{Job, JobStatus, Source, SourceStatus};
use memory_core::{MemoryConfig, ids::now_rfc3339};
use memory_extract::ingest::PROCESS_SOURCE;
use memory_extract::review::Decision;
use memory_extract::{Pipeline, Vault};
use memory_llm::{ChatRequest, Embedder, LlmProvider, Message, ModelRole};
use memory_retrieve::Retriever;
use memory_store::Store;

pub const PROMOTE_STREAM: &str = "promote_stream";
pub const CONSOLIDATE: &str = "consolidate";
pub const DISTILL: &str = "distill";
pub const VOICE: &str = "voice";
pub const COMPACT: &str = "compact";
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

/// Hasil proses ulang sumber yang gagal.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RetryReport {
    /// Jumlah sumber berstatus gagal yang ditemukan.
    pub failed: u32,
    /// Diantre ulang sekarang.
    pub requeued: u32,
    /// Dilewati karena batas proses ulang otomatis sudah habis.
    pub exhausted: u32,
    /// Server LLM/embedding belum bisa dihubungi; tidak ada yang diantre.
    pub backend_error: Option<String>,
}

const AUTO_RETRIES: &str = "auto_retries";

#[derive(Clone)]
pub struct Runtime {
    pub cfg: Arc<MemoryConfig>,
    pub store: Store,
    pub pipeline: Pipeline,
    pub retriever: Retriever,
    pub vault: Vault,
    /// Satu sinkronisasi pada satu waktu (penjadwal & tombol UI).
    pub sync_lock: Arc<tokio::sync::Mutex<()>>,
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
        Self { cfg, store, pipeline, retriever, vault, sync_lock: Arc::new(tokio::sync::Mutex::new(())) }
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
            COMPACT => json!(self.pipeline.compact(&s("subject")?).await?),
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

    /// Cek singkat bahwa server embedding dan LLM (kedua peran) bisa dihubungi.
    pub async fn check_backends(&self) -> std::result::Result<(), String> {
        let p = &self.pipeline;
        p.embedder.embed_one("ping").await.map_err(|e| format!("embedding: {e}"))?;
        for role in [ModelRole::Light, ModelRole::Extract] {
            let req = ChatRequest {
                role,
                messages: vec![Message::user("ping")],
                json_schema: None,
                temperature: Some(0.0),
                max_tokens: Some(4),
                task: "ping".into(),
            };
            p.llm.chat(req).await.map_err(|e| format!("llm ({role:?}): {e}"))?;
        }
        Ok(())
    }

    /// Kembalikan satu sumber gagal ke antrean. `None` bila sumber tidak gagal.
    async fn requeue_source(&self, source: &Source, auto_retries: u64) -> Result<Option<String>> {
        if source.status != SourceStatus::Failed {
            return Ok(None);
        }
        self.store.patch("source", &source.id, json!({"status": "pending", "error": null, "meta": {AUTO_RETRIES: auto_retries}})).await?;
        Ok(Some(self.pipeline.enqueue_process(source).await?))
    }

    /// Proses ulang satu sumber gagal (manual; menyetel ulang hitungan otomatis).
    pub async fn retry_source(&self, source_id: &str) -> Result<Option<String>> {
        let source = self.store.get_source(source_id).await?.ok_or_else(|| JobError::Invalid(format!("sumber {source_id} tidak ada")))?;
        if source.status != SourceStatus::Failed {
            return Ok(None);
        }
        self.check_backends().await.map_err(|e| JobError::Invalid(format!("server belum bisa dihubungi, coba lagi nanti: {e}")))?;
        self.requeue_source(&source, 0).await
    }

    /// Antre ulang semua sumber gagal, hanya bila server LLM & embedding hidup.
    /// `auto`: dipanggil penjadwal — menghormati `retry_failed_max` per sumber.
    pub async fn retry_failed(&self, auto: bool) -> Result<RetryReport> {
        let sources = self.store.failed_sources(500).await?;
        let mut rep = RetryReport { failed: sources.len() as u32, ..Default::default() };
        if sources.is_empty() {
            return Ok(rep);
        }
        if let Err(e) = self.check_backends().await {
            rep.backend_error = Some(e);
            return Ok(rep);
        }
        let max = u64::from(self.cfg.schedule.retry_failed_max);
        for s in &sources {
            let done = s.meta.get(AUTO_RETRIES).and_then(Value::as_u64).unwrap_or(0);
            if auto && done >= max {
                rep.exhausted += 1;
                continue;
            }
            let next = if auto { done + 1 } else { 0 };
            if self.requeue_source(s, next).await?.is_some() {
                rep.requeued += 1;
            }
        }
        Ok(rep)
    }

    /// Antrekan pekerjaan berkala yang jatuh tempo (idempoten per periode).
    pub async fn schedule_due(&self) -> Result<u32> {
        let sc = &self.cfg.schedule;
        let now = chrono::Utc::now().timestamp() as u64;
        let mut n = 0;
        let bucket = |hours: u64| now / (hours.max(1) * 3600);
        for subject in self.store.list_subjects().await? {
            for (kind, hours) in [
                (CONSOLIDATE, sc.consolidate_hours),
                (DISTILL, sc.distill_hours),
                (VOICE, sc.voice_hours),
                (COMPACT, sc.compact_hours),
            ] {
                if hours == 0 {
                    continue;
                }
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
        let retry_every = Duration::from_secs(self.cfg.schedule.retry_failed_minutes * 60);
        let mut last_retry: Option<std::time::Instant> = None;
        let mut last_sync: Option<std::time::Instant> = None;
        loop {
            if let Err(e) = self.schedule_due().await {
                tracing::error!(error = %e, "scheduler error");
            }
            if !retry_every.is_zero() && last_retry.is_none_or(|t| t.elapsed() >= retry_every) {
                last_retry = Some(std::time::Instant::now());
                match self.retry_failed(true).await {
                    Ok(r) if r.backend_error.is_some() => {
                        tracing::info!(failed = r.failed, error = r.backend_error.as_deref().unwrap_or(""), "sumber gagal menunggu server LLM/embedding")
                    }
                    Ok(r) if r.requeued + r.exhausted > 0 => tracing::info!(requeued = r.requeued, exhausted = r.exhausted, "sumber gagal diproses ulang"),
                    Ok(_) => {}
                    Err(e) => tracing::error!(error = %e, "proses ulang otomatis gagal"),
                }
            }
            self.sync_if_due(&mut last_sync).await;
            // Tiap menit: pengaturan sync bisa diubah dari UI kapan saja.
            let tick = if retry_every.is_zero() { Duration::from_secs(60) } else { retry_every.min(Duration::from_secs(60)) };
            tokio::select! {
                _ = tokio::time::sleep(tick) => {}
                _ = shutdown.changed() => break,
            }
        }
    }

    /// Sinkronisasi otomatis bila folder sync diatur dan intervalnya sudah lewat.
    async fn sync_if_due(&self, last: &mut Option<std::time::Instant>) {
        let s = sync::load_settings(&self.vault);
        if s.dir.is_none() || s.interval_minutes == 0 || last.is_some_and(|t| t.elapsed() < Duration::from_secs(s.interval_minutes * 60)) {
            return;
        }
        *last = Some(std::time::Instant::now());
        match sync::sync_now(self).await {
            Ok(r) => {
                if r.pushed || !r.pulled_from.is_empty() {
                    tracing::info!(from = ?r.pulled_from, added = r.added, updated = r.updated, pushed = r.pushed, "sinkronisasi selesai");
                }
                for w in &r.warnings {
                    tracing::warn!("sinkronisasi: {w}");
                }
            }
            Err(e) => tracing::warn!(error = %e, "sinkronisasi gagal"),
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
