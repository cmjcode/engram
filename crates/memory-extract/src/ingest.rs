//! Tahap Ingest: simpan teks & file asli ke vault (dialamatkan hash),
//! deduplikasi, deteksi bahasa, lalu antrekan job pemrosesan.

use serde_json::{Value, json};

use memory_core::model::{Job, Source, SourceKind, SourceStats, SourceStatus};
use memory_core::text::detect_languages;
use memory_core::{ids::now_rfc3339, new_id};

use crate::{ExtractError, Pipeline, Result, Vault};

pub const PROCESS_SOURCE: &str = "process_source";

#[derive(Debug, Clone, Default)]
pub struct IngestDoc {
    pub subject: String,
    pub kind: SourceKind,
    pub title: String,
    pub text: String,
    pub original_name: Option<String>,
    /// (ekstensi, bytes) file asli bila berbeda dari teks (audio, json ekspor, ...).
    pub original: Option<(String, Vec<u8>)>,
    pub occurred_from: Option<String>,
    pub occurred_to: Option<String>,
    pub meta: Value,
    pub batch_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IngestOutcome {
    pub source: Source,
    pub duplicate: bool,
    pub job_id: Option<String>,
}

pub fn text_hash(text: &str) -> String {
    blake3::hash(text.trim().as_bytes()).to_hex().to_string()
}

impl Pipeline {
    pub async fn ingest(&self, doc: IngestDoc) -> Result<IngestOutcome> {
        let text = doc.text.trim();
        if text.is_empty() {
            return Err(ExtractError::Invalid("teks kosong".into()));
        }
        self.ensure_subject(&doc.subject, None).await?;
        let hash = text_hash(text);
        if let Some(existing) = self.store.source_by_hash(&doc.subject, &hash).await? {
            // Sumber yang dulu gagal diproses: antre ulang.
            let job_id = if existing.status == SourceStatus::Failed { Some(self.enqueue_process(&existing).await?) } else { None };
            return Ok(IngestOutcome { source: existing, duplicate: true, job_id });
        }
        let rel = Vault::source_rel_path(&hash, "txt");
        self.vault.write_rel(&rel, text.as_bytes())?;
        let mut meta = if doc.meta.is_object() { doc.meta.clone() } else { json!({}) };
        if let Some((ext, bytes)) = &doc.original {
            let orig = Vault::source_rel_path(&hash, &format!("orig.{}", ext.trim_start_matches('.')));
            self.vault.write_rel(&orig, bytes)?;
            meta["original_path"] = json!(orig);
        }
        let source = Source {
            id: new_id(),
            subject: doc.subject.clone(),
            kind: doc.kind,
            title: if doc.title.is_empty() { doc.original_name.clone().unwrap_or_else(|| "tanpa judul".into()) } else { doc.title.clone() },
            path: rel,
            hash,
            original_name: doc.original_name.clone(),
            languages: detect_languages(&text.chars().take(20_000).collect::<String>()),
            chars: text.chars().count() as u64,
            status: SourceStatus::Pending,
            error: None,
            batch_id: doc.batch_id.clone(),
            occurred_from: doc.occurred_from.clone(),
            occurred_to: doc.occurred_to.clone(),
            stats: SourceStats::default(),
            meta,
            created_at: now_rfc3339(),
        };
        self.store.put_source(&source).await?;
        let job_id = self.enqueue_process(&source).await?;
        Ok(IngestOutcome { source, duplicate: false, job_id: Some(job_id) })
    }

    pub async fn enqueue_process(&self, source: &Source) -> Result<String> {
        let job = Job::new(
            PROCESS_SOURCE,
            json!({"source_id": source.id}),
            format!("process:{}:{}", source.subject, source.hash),
            source.batch_id.clone(),
        );
        let (job, _) = self.store.enqueue(&job).await?;
        Ok(job.id)
    }
}
