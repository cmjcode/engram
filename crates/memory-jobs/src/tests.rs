use std::sync::Arc;

use memory_core::MemoryConfig;
use memory_core::model::{ImportBatch, SourceKind};
use memory_extract::Vault;
use memory_extract::ingest::IngestDoc;
use memory_llm::mock::{ScriptedLlm, last_user};
use memory_llm::{ChatRequest, HashEmbedder};
use serde_json::json;

use crate::Runtime;

const DIM: usize = 128;

fn llm(req: &ChatRequest) -> memory_llm::Result<String> {
    let user = last_user(req);
    Ok(match req.task.as_str() {
        "segment" => {
            let n = user.lines().filter(|l| l.starts_with('[')).count();
            json!({"segments": (0..n).map(|i| json!({"start": i, "end": i, "significance": 0.9})).collect::<Vec<_>>()}).to_string()
        }
        "extract" => json!({"title": "cerita", "summary": "ringkas", "entities": [{"name": "Budi", "kind": "person", "is_self": true}]}).to_string(),
        _ => json!({"text": "ok"}).to_string(),
    })
}

async fn rt(dir: &std::path::Path) -> Runtime {
    let mut cfg = MemoryConfig::for_tests(DIM);
    cfg.vault.path = dir.to_path_buf();
    Runtime::open_with(cfg, Arc::new(ScriptedLlm::new(llm)), Arc::new(HashEmbedder::new(DIM))).await.unwrap()
}

#[tokio::test]
async fn batch_progress_scheduler_and_encrypted_backup_roundtrip() {
    let d1 = tempfile::tempdir().unwrap();
    let r = rt(d1.path()).await;
    let batch = ImportBatch { id: "b1".into(), subject: "budi".into(), label: "uji".into(), total: 0, done: 0, failed: 0, duplicates: 0, failures: vec![], created_at: memory_core::ids::now_rfc3339(), updated_at: String::new() };
    r.store.put_batch(&batch).await.unwrap();
    for i in 0..3 {
        r.pipeline
            .ingest(IngestDoc {
                subject: "budi".into(),
                kind: SourceKind::Memoir,
                text: format!("Cerita nomor {i}: waktu kecil saya tinggal di rumah kayu dekat sungai bersama simbah di desa {i}."),
                batch_id: Some("b1".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        r.store.batch_incr("b1", "total", None).await.unwrap();
    }
    assert_eq!(r.drain().await.unwrap(), 3);
    let b = r.store.get_batch("b1").await.unwrap().unwrap();
    assert_eq!((b.total, b.done, b.failed), (3, 3, 0));
    let eps = r.store.count_episodes("budi").await.unwrap();
    assert_eq!(eps, 3);

    // Scheduler idempoten dalam satu periode.
    assert!(r.schedule_due().await.unwrap() > 0);
    assert_eq!(r.schedule_due().await.unwrap(), 0);
    r.drain().await.unwrap();

    // Ekspor + backup terenkripsi.
    use age::secrecy::ExposeSecret;
    let id = age::x25519::Identity::generate();
    let id_path = d1.path().join("key.txt");
    std::fs::write(&id_path, id.to_string().expose_secret()).unwrap();
    let out = crate::archive::backup(&r, &d1.path().join("backup.tar.gz"), Some(&id.to_public().to_string())).await.unwrap();
    assert!(out.to_string_lossy().ends_with(".age"));
    let head = std::fs::read(&out).unwrap();
    assert!(head.starts_with(b"age-encryption.org/v1"));

    // Restore ke vault baru.
    let d2 = tempfile::tempdir().unwrap();
    let r2 = rt(d2.path()).await;
    let rep = crate::archive::restore(&r2, &out, Some(&id_path)).await.unwrap();
    assert!(rep.rows > 0 && rep.source_files >= 3, "{rep:?}");
    assert_eq!(r2.store.count_episodes("budi").await.unwrap(), 3);
    let hits = r2.retriever.recall(&memory_retrieve::RecallQuery::new("budi", "rumah kayu dekat sungai")).await.unwrap();
    assert!(hits.found && !hits.episodes.is_empty(), "embedding harus dihitung ulang setelah restore");
    // Ekspor berisi markdown yang bisa dibaca manusia.
    let exp = crate::archive::export(&r2, None).await.unwrap();
    assert!(exp.join("manifest.json").exists());
    assert!(exp.join("markdown/budi/README.md").exists());
    let _ = Vault::new(d2.path());
}

#[tokio::test]
async fn failed_sources_are_retried_only_when_backends_are_up() {
    use memory_core::model::SourceStatus;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Embedder yang bisa "dimatikan" seperti server TEI yang mati.
    struct FlakyEmbedder(Arc<AtomicBool>, HashEmbedder);
    #[async_trait::async_trait]
    impl memory_llm::Embedder for FlakyEmbedder {
        async fn embed(&self, texts: &[String]) -> memory_llm::Result<Vec<Vec<f32>>> {
            if self.0.load(Ordering::SeqCst) {
                return Err(memory_llm::LlmError::Other("server embedding mati".into()));
            }
            self.1.embed(texts).await
        }
        fn dimension(&self) -> usize {
            DIM
        }
    }

    let down = Arc::new(AtomicBool::new(true));
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = MemoryConfig::for_tests(DIM);
    cfg.vault.path = dir.path().to_path_buf();
    cfg.schedule.retry_failed_max = 1;
    let embedder = FlakyEmbedder(down.clone(), HashEmbedder::new(DIM));
    let r = Runtime::open_with(cfg, Arc::new(ScriptedLlm::new(llm)), Arc::new(embedder)).await.unwrap();

    let out = r
        .pipeline
        .ingest(IngestDoc {
            subject: "budi".into(),
            kind: SourceKind::Memoir,
            text: "Waktu kecil saya tinggal di rumah kayu dekat sungai bersama simbah di desa.".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let id = out.source.id.clone();
    // Sumber yang gagal di versi lama / karena server mati.
    let fail = |r: Runtime, id: String| async move { r.store.set_source_status(&id, SourceStatus::Failed, Some("http: server mati".into())).await.unwrap() };
    fail(r.clone(), id.clone()).await;
    let status = |r: Runtime, id: String| async move { r.store.get_source(&id).await.unwrap().unwrap() };
    assert_eq!(status(r.clone(), id.clone()).await.status, SourceStatus::Failed);

    // Server masih mati: tidak ada yang diantre, jatah otomatis tidak terpakai.
    let rep = r.retry_failed(true).await.unwrap();
    assert_eq!((rep.failed, rep.requeued), (1, 0));
    assert!(rep.backend_error.is_some());
    assert!(r.retry_source(&id).await.is_err(), "manual juga menolak bila server mati");
    assert_eq!(status(r.clone(), id.clone()).await.status, SourceStatus::Failed);

    // Server mati lagi saat diproses: jatah otomatis (1) habis.
    down.store(false, Ordering::SeqCst);
    assert_eq!(r.retry_failed(true).await.unwrap().requeued, 1);
    let s = status(r.clone(), id.clone()).await;
    assert_eq!((s.status, s.meta["auto_retries"].as_u64()), (SourceStatus::Pending, Some(1)));
    fail(r.clone(), id.clone()).await;
    let rep = r.retry_failed(true).await.unwrap();
    assert_eq!((rep.requeued, rep.exhausted), (0, 1));

    // Manual tetap bisa, lalu sumber selesai diproses.
    assert!(r.retry_source(&id).await.unwrap().is_some());
    r.drain().await.unwrap();
    let s = status(r.clone(), id.clone()).await;
    assert!(matches!(s.status, SourceStatus::Done | SourceStatus::Skipped), "{:?} {:?}", s.status, s.error);
    assert_eq!(s.meta["auto_retries"].as_u64(), Some(0));
    assert!(r.retry_source(&id).await.unwrap().is_none(), "sumber yang tidak gagal tidak diantre");
}
