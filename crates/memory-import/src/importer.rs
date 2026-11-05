use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use memory_core::ids::{is_valid_id, now_rfc3339};
use memory_core::model::{ImportBatch, SourceKind};
use memory_core::new_id;
use memory_extract::Pipeline;
use memory_extract::ingest::IngestDoc;
use memory_llm::Transcriber;

use crate::adapters::{Format, IMAGE_EXT, detect_format, parse_bytes};
use crate::{ImportError, Imported, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportSummary {
    pub batch_id: String,
    pub files: u32,
    pub documents: u32,
    pub queued: u32,
    pub duplicates: u32,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InboxReport {
    pub processed: Vec<String>,
    pub failed: Vec<String>,
    pub waiting: u32,
}

#[derive(Clone)]
pub struct Importer {
    pub pipeline: Pipeline,
    pub transcriber: Option<Arc<dyn Transcriber>>,
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn is_hidden(p: &Path) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

/// Caption foto: berkas pendamping dengan nama sama (.txt / .md / .json{caption}).
fn sidecar_caption(img: &Path) -> Option<String> {
    for e in ["txt", "md", "json"] {
        let p = img.with_extension(e);
        if let Ok(s) = std::fs::read_to_string(&p) {
            if e == "json" {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s)
                    && let Some(c) = v["caption"].as_str().or(v["description"].as_str()) {
                        return Some(c.to_string());
                    }
                continue;
            }
            if !s.trim().is_empty() {
                return Some(s);
            }
        }
    }
    None
}

/// `.txt`/`.md`/`.json` yang merupakan caption foto (bukan dokumen sendiri).
fn is_sidecar(p: &Path) -> bool {
    matches!(ext(p).as_str(), "txt" | "md" | "json") && IMAGE_EXT.iter().any(|e| p.with_extension(e).exists())
}

fn collect_files(path: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for p in entries {
            if !is_hidden(&p) {
                collect_files(&p, out)?;
            }
        }
    } else if path.is_file() {
        out.push(path.to_path_buf());
    }
    Ok(())
}

impl Importer {
    pub fn new(pipeline: Pipeline, transcriber: Option<Arc<dyn Transcriber>>) -> Self {
        Self { pipeline, transcriber }
    }

    /// Ubah satu berkas menjadi dokumen (termasuk foto & audio).
    pub async fn documents_for(&self, path: &Path, hint: Option<SourceKind>) -> Result<(Vec<Imported>, Option<(String, Vec<u8>)>)> {
        let bytes = tokio::fs::read(path).await?;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let original = Some((ext(path), bytes.clone()));
        match detect_format(path, &bytes)? {
            Format::Image => {
                let caption = sidecar_caption(path).ok_or_else(|| ImportError::Unsupported(format!("{name}: foto butuh caption (berkas {name}.txt di sebelahnya)")))?;
                Ok((vec![Imported { kind: SourceKind::Photo, title: format!("Foto: {name}"), text: caption, occurred_from: None, occurred_to: None, meta: serde_json::json!({"format": "photo"}) }], original))
            }
            Format::Audio => {
                let t = self.transcriber.as_ref().ok_or_else(|| ImportError::Unsupported(format!("{name}: audio butuh [transcription] base_url di konfigurasi")))?;
                let text = t.transcribe(path).await?;
                Ok((vec![Imported { kind: hint.unwrap_or(SourceKind::Audio), title: format!("Rekaman: {name}"), text, occurred_from: None, occurred_to: None, meta: serde_json::json!({"format": "audio"}) }], original))
            }
            Format::Text => Ok((parse_bytes(path, &bytes, hint)?, None)),
            _ => Ok((parse_bytes(path, &bytes, hint)?, original)),
        }
    }

    /// Import berkas/folder ke satu batch. Progres pemrosesan dicatat di batch
    /// oleh worker job (done/failed); di sini dicatat total & duplikat.
    pub async fn import_paths(&self, subject: &str, paths: &[PathBuf], label: &str, hint: Option<SourceKind>) -> Result<ImportSummary> {
        if !is_valid_id(subject) {
            return Err(ImportError::Unsupported(format!("id subjek tidak valid: {subject}")));
        }
        self.pipeline.ensure_subject(subject, None).await?;
        let batch = ImportBatch {
            id: new_id(),
            subject: subject.to_string(),
            label: label.to_string(),
            total: 0,
            done: 0,
            failed: 0,
            duplicates: 0,
            failures: vec![],
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
        };
        self.pipeline.store.put_batch(&batch).await?;
        let mut files = Vec::new();
        for p in paths {
            collect_files(p, &mut files)?;
        }
        files.retain(|f| !is_sidecar(f) && !is_hidden(f));
        let mut sum = ImportSummary { batch_id: batch.id.clone(), ..Default::default() };
        for f in files {
            sum.files += 1;
            match self.import_file(subject, &f, hint, &batch.id, &mut sum).await {
                Ok(()) => {}
                Err(e) => {
                    let msg = format!("{}: {e}", f.display());
                    tracing::warn!("{msg}");
                    self.pipeline.store.batch_incr(&batch.id, "failed", Some(msg.clone())).await?;
                    sum.failed.push(msg);
                }
            }
        }
        Ok(sum)
    }

    async fn import_file(&self, subject: &str, f: &Path, hint: Option<SourceKind>, batch_id: &str, sum: &mut ImportSummary) -> Result<()> {
        let (docs, original) = self.documents_for(f, hint).await?;
        let name = f.file_name().map(|n| n.to_string_lossy().to_string());
        // Berkas asli disimpan sekali (pada dokumen pertama) agar tidak terduplikasi.
        let mut original = original;
        for d in docs {
            sum.documents += 1;
            let out = self
                .pipeline
                .ingest(IngestDoc {
                    subject: subject.to_string(),
                    kind: d.kind,
                    title: d.title,
                    text: d.text,
                    original_name: name.clone(),
                    original: original.take(),
                    occurred_from: d.occurred_from,
                    occurred_to: d.occurred_to,
                    meta: d.meta,
                    batch_id: Some(batch_id.to_string()),
                    ..Default::default()
                })
                .await?;
            if out.duplicate && out.job_id.is_none() {
                sum.duplicates += 1;
                self.pipeline.store.batch_incr(batch_id, "duplicates", None).await?;
            } else {
                sum.queued += 1;
                self.pipeline.store.batch_incr(batch_id, "total", None).await?;
            }
        }
        Ok(())
    }

    /// Satu putaran inbox: berkas yang ukurannya stabil diimport lalu
    /// dipindah ke `processed/` (atau `failed/` beserta alasannya).
    /// Subfolder `inbox/<subjek>/` mengarahkan ke subjek itu.
    pub async fn scan_inbox(&self, default_subject: &str, min_age_secs: u64) -> Result<InboxReport> {
        let inbox = self.pipeline.vault.inbox_dir();
        let mut report = InboxReport::default();
        let mut candidates: Vec<(String, PathBuf)> = Vec::new();
        for e in std::fs::read_dir(&inbox)? {
            let p = e?.path();
            let n = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if is_hidden(&p) || n == "processed" || n == "failed" {
                continue;
            }
            if p.is_dir() && is_valid_id(&n) {
                let mut files = Vec::new();
                collect_files(&p, &mut files)?;
                candidates.extend(files.into_iter().map(|f| (n.clone(), f)));
            } else if p.is_file() {
                let mut subj = default_subject.to_string();
                if subj == "default"
                    && let Ok(subs) = self.pipeline.store.list_subjects().await
                    && !subs.iter().any(|s| s.id == "default")
                    && let Some(first) = subs.first()
                {
                    subj = first.id.clone();
                }
                candidates.push((subj, p));
            }
        }
        let now = std::time::SystemTime::now();
        for (subject, f) in candidates {
            if is_sidecar(&f) || matches!(ext(&f).as_str(), "part" | "tmp" | "crdownload" | "download") {
                continue;
            }
            let fresh = std::fs::metadata(&f)?.modified().ok().and_then(|m| now.duration_since(m).ok()).is_none_or(|age| age.as_secs() < min_age_secs);
            if fresh {
                report.waiting += 1;
                continue;
            }
            let label = format!("inbox {}", chrono::Utc::now().format("%Y-%m-%d"));
            let res = self.import_paths(&subject, std::slice::from_ref(&f), &label, None).await;
            let ok = matches!(&res, Ok(s) if s.failed.is_empty());
            let dest_dir = inbox.join(if ok { "processed" } else { "failed" });
            std::fs::create_dir_all(&dest_dir)?;
            let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S");
            let fname = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let dest = dest_dir.join(format!("{stamp}-{fname}"));
            let display = f.display().to_string();
            std::fs::rename(&f, &dest)?;
            for side in ["txt", "md", "json"] {
                let s = f.with_extension(side);
                if IMAGE_EXT.contains(&ext(&f).as_str()) && s.exists() {
                    let _ = std::fs::rename(&s, dest_dir.join(format!("{stamp}-{}", s.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())));
                }
            }
            if ok {
                report.processed.push(display);
            } else {
                let why = match res {
                    Ok(s) => s.failed.join("; "),
                    Err(e) => e.to_string(),
                };
                std::fs::write(dest.with_extension("error.txt"), &why)?;
                report.failed.push(format!("{display}: {why}"));
            }
        }
        Ok(report)
    }

    pub async fn watch_inbox(&self, default_subject: String, poll_secs: u64, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        loop {
            match self.scan_inbox(&default_subject, 3).await {
                Ok(r) if !r.processed.is_empty() || !r.failed.is_empty() => {
                    tracing::info!(processed = r.processed.len(), failed = r.failed.len(), "inbox diproses");
                }
                Err(e) => tracing::error!(error = %e, "inbox error"),
                _ => {}
            }
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(poll_secs.max(1))) => {}
                _ = shutdown.changed() => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory_core::MemoryConfig;
    use memory_llm::mock::ScriptedLlm;
    use memory_llm::HashEmbedder;
    use memory_store::Store;

    async fn importer(dir: &Path) -> Importer {
        let cfg = MemoryConfig::for_tests(64);
        let store = Store::memory(64, &cfg.index).await.unwrap();
        let vault = memory_extract::Vault::new(dir);
        vault.init().unwrap();
        let p = Pipeline::new(store, Arc::new(ScriptedLlm::new(|_| Ok("{}".into()))), Arc::new(HashEmbedder::new(64)), Arc::new(cfg), vault);
        Importer::new(p, None)
    }

    #[tokio::test]
    async fn folder_import_dedups_pairs_captions_and_reports_failures() {
        let dir = tempfile::tempdir().unwrap();
        let imp = importer(dir.path()).await;
        let src = dir.path().join("bahan");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("catatan.md"), "Waktu kecil saya sering diajak Bapak ke sawah.").unwrap();
        std::fs::write(src.join("salinan.txt"), "Waktu kecil saya sering diajak Bapak ke sawah.").unwrap();
        std::fs::write(src.join("lebaran.jpg"), [0xFFu8, 0xD8, 0xFF]).unwrap();
        std::fs::write(src.join("lebaran.txt"), "Lebaran 1995 di rumah Mbah, semua sepupu berkumpul.").unwrap();
        std::fs::write(src.join("tanpa-caption.png"), [0x89u8, 0x50]).unwrap();
        std::fs::write(src.join("data.xlsx"), [0u8]).unwrap();
        let s = imp.import_paths("budi", &[src], "uji", None).await.unwrap();
        assert_eq!(s.files, 5, "{s:?}");
        assert_eq!(s.queued, 2);
        assert_eq!(s.duplicates, 1);
        assert_eq!(s.failed.len(), 2);
        let b = imp.pipeline.store.get_batch(&s.batch_id).await.unwrap().unwrap();
        assert_eq!((b.total, b.duplicates, b.failed), (2, 1, 2));
        let srcs = imp.pipeline.store.list_sources("budi", None, 10, 0).await.unwrap();
        let photo = srcs.iter().find(|x| x.kind == SourceKind::Photo).unwrap();
        assert!(photo.meta["original_path"].as_str().unwrap().ends_with(".orig.jpg"));
    }

    #[tokio::test]
    async fn inbox_moves_files_and_routes_subfolders() {
        let dir = tempfile::tempdir().unwrap();
        let imp = importer(dir.path()).await;
        let inbox = imp.pipeline.vault.inbox_dir();
        std::fs::write(inbox.join("cerita.txt"), "Ibu dulu berjualan nasi pecel di pasar Beringharjo.").unwrap();
        std::fs::create_dir_all(inbox.join("ani")).unwrap();
        std::fs::write(inbox.join("ani/kenangan.md"), "Ani lahir di Bandung, tumbuh bersama Ambu dan Abah.").unwrap();
        std::fs::write(inbox.join("rusak.xyz"), "?").unwrap();
        let r = imp.scan_inbox("budi", 0).await.unwrap();
        assert_eq!(r.processed.len(), 2, "{r:?}");
        assert_eq!(r.failed.len(), 1);
        assert!(!inbox.join("cerita.txt").exists());
        assert_eq!(std::fs::read_dir(inbox.join("processed")).unwrap().count(), 2);
        assert!(std::fs::read_dir(inbox.join("failed")).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".error.txt")));
        assert_eq!(imp.pipeline.store.list_sources("ani", None, 10, 0).await.unwrap().len(), 1);
    }
}
