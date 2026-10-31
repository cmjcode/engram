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
