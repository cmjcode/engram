//! Tahap Ingest: simpan teks & file asli ke vault (dialamatkan hash),
//! deduplikasi, deteksi bahasa, lalu antrekan job pemrosesan.

use serde_json::{Value, json};

use memory_core::model::{Job, Source, SourceKind, SourceStats, SourceStatus, parse_source_tag};
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
    pub project: Option<String>,
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
        // Penyaring privasi: satu titik untuk semua jalur masuk (hook agent, `remember`, import).
        let Some(clean) = memory_core::privacy::sanitize(&doc.text, &self.cfg.privacy) else {
            return Err(ExtractError::Invalid("teks kosong (atau seluruhnya ditandai <private>)".into()));
        };
        if clean.redacted > 0 {
            tracing::info!(redacted = clean.redacted, "rahasia diganti penanda sebelum disimpan");
        }
        let mut doc = doc;
        doc.text = clean.text;
        if clean.redacted > 0 || clean.stripped_private {
            if !doc.meta.is_object() {
                doc.meta = json!({});
            }
            doc.meta["privacy"] = json!({"redacted": clean.redacted, "stripped_private": clean.stripped_private});
        }
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
        let project = doc
            .project
            .clone()
            .or_else(|| meta["project"].as_str().map(str::to_string))
            .or_else(|| meta["label"].as_str().map(str::to_string))
            .or_else(|| meta["source"].as_str().and_then(|s| parse_source_tag(s).1))
            .or_else(|| {
                doc.title
                    .strip_prefix("Diingat agent: ")
                    .and_then(|s| parse_source_tag(s).1)
            })
            .filter(|p| !p.trim().is_empty());
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
            project,
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

/// Dokumen yang sudah terdistilasi (mis. auto-memory Claude Code: satu fakta per berkas).
#[derive(Debug, Clone, Default)]
pub struct DistilledDoc {
    pub subject: String,
    pub title: String,
    pub summary: String,
    pub text: String,
    pub project: Option<String>,
    pub original_name: Option<String>,
    pub meta: Value,
}

#[derive(Debug, Clone)]
pub struct DistilledOutcome {
    pub source_id: String,
    pub episode_id: Option<String>,
    pub duplicate: bool,
}

impl Pipeline {
    /// Simpan teks yang SUDAH terdistilasi langsung sebagai episode, tanpa segmentasi
    /// dan tanpa LLM (hanya embedding + deteksi penceritaan ulang). Dipakai
    /// `engram import claude-memory`: menjalankan ulang ekstraksi atas prosa yang sudah
    /// disuling hanya menambah biaya dan kehilangan informasi.
    pub async fn ingest_distilled(&self, doc: DistilledDoc) -> Result<DistilledOutcome> {
        use crate::llm_types::ExtractionOut;
        use crate::process::SourceCtx;
        use crate::segment::Segment;
        let Some(clean) = memory_core::privacy::sanitize(&doc.text, &self.cfg.privacy) else {
            return Err(ExtractError::Invalid("teks kosong".into()));
        };
        let text = clean.text.trim().to_string();
        let subject = self.ensure_subject(&doc.subject, None).await?;
        let hash = text_hash(&text);
        if let Some(existing) = self.store.source_by_hash(&doc.subject, &hash).await? {
            return Ok(DistilledOutcome { source_id: existing.id, episode_id: None, duplicate: true });
        }
        let rel = Vault::source_rel_path(&hash, "md");
        self.vault.write_rel(&rel, text.as_bytes())?;
        let mut meta = if doc.meta.is_object() { doc.meta.clone() } else { json!({}) };
        // Struktur yang sudah diketahui pengirim (keputusan, fakta, jenis observasi; mis. dari
        // bundel repo rekan tim) diteruskan apa adanya ke commit episode, tidak disimpan di meta.
        let known: ExtractionOut = meta.as_object_mut().and_then(|m| m.remove("extraction")).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        meta["distilled"] = json!(true);
        let now = now_rfc3339();
        let source = Source {
            id: new_id(),
            subject: doc.subject.clone(),
            kind: SourceKind::Note,
            title: if doc.title.is_empty() { "catatan terdistilasi".into() } else { doc.title.clone() },
            path: rel,
            hash,
            original_name: doc.original_name.clone(),
            languages: detect_languages(&text),
            chars: text.chars().count() as u64,
            status: SourceStatus::Processing,
            error: None,
            batch_id: None,
            occurred_from: None,
            occurred_to: None,
            project: doc.project.clone(),
            stats: SourceStats::default(),
            meta,
            created_at: now.clone(),
        };
        self.store.put_source(&source).await?;
        let ctx = SourceCtx::from(&source);
        let seg = Segment { start: 0, end: 0, text: text.clone(), topic: doc.title.clone(), about_subject: true, speaker: None, significance: Some(0.8), date: None };
        let ext = ExtractionOut { title: doc.title.clone(), summary: if doc.summary.is_empty() { memory_core::text::truncate_chars(&text, 300) } else { doc.summary.clone() }, ..known };
        let ep_id = crate::det_id(&[&source.id, "ep", "0", "0"]);
        let lock = self.subject_lock(&subject.id).await;
        let _g = lock.lock().await;
        let subject = self.store.get_subject(&subject.id).await?.unwrap_or(subject);
        let c = self.commit_episode(&ctx, &subject, &seg, 0.7, &ext, &ep_id).await?;
        let stats = SourceStats { segments: 1, episodes: u32::from(c.created && !c.retelling), retellings: u32::from(c.retelling), ..Default::default() };
        self.store.set_source_stats(&source.id, &stats).await?;
        self.store.set_source_status(&source.id, SourceStatus::Done, None).await?;
        Ok(DistilledOutcome { source_id: source.id, episode_id: Some(ep_id), duplicate: false })
    }
}
