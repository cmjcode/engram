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

#[tokio::test]
async fn two_computers_sync_through_shared_folder() {
    use crate::sync::{self, SyncSettings};

    let drive = tempfile::tempdir().unwrap();
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (rt(da.path()).await, rt(db.path()).await);
    let setup = |r: &Runtime, name: &str| {
        let s = sync::save_settings(&r.vault, SyncSettings { dir: Some(drive.path().to_path_buf()), interval_minutes: 0, device_name: Some(name.into()) }).unwrap();
        sync::set_passphrase(&r.vault, Some("rahasia-keluarga")).unwrap();
        s
    };
    let ingest = |r: Runtime, text: &'static str| async move {
        r.pipeline.ingest(IngestDoc { subject: "budi".into(), kind: SourceKind::Memoir, text: text.into(), ..Default::default() }).await.unwrap();
        r.drain().await.unwrap();
    };

    // Folder Drive kosong dipakai langsung.
    assert_eq!(setup(&a, "Laptop").dir.unwrap(), drive.path());
    setup(&b, "PC kantor");
    ingest(a.clone(), "Waktu kecil saya tinggal di rumah kayu dekat sungai bersama simbah di desa.").await;

    // A mengunggah; snapshot terenkripsi, tidak ada teks polos di folder Drive.
    let ra = sync::sync_now(&a).await.unwrap();
    assert!(ra.pushed && ra.source_files_out >= 1, "{ra:?}");
    let devs = sync::status(&a).devices;
    assert_eq!(devs.len(), 1);
    assert!(devs[0].encrypted && devs[0].this_device);
    for e in walk(drive.path()) {
        let bytes = std::fs::read(&e).unwrap();
        if e.to_string_lossy().contains("snapshot") {
            assert!(bytes.starts_with(b"age-encryption.org/v1"), "{}", e.display());
        }
    }

    // B menarik data A dan bisa langsung mencarinya (embedding dihitung di B).
    let rb = sync::sync_now(&b).await.unwrap();
    assert_eq!(rb.pulled_from, vec!["Laptop".to_string()], "{rb:?}");
    assert!(rb.added > 0 && rb.embedded > 0 && rb.pending_embed == 0, "{rb:?}");
    assert_eq!(b.store.count_episodes("budi").await.unwrap(), 1);
    let hits = b.retriever.recall(&memory_retrieve::RecallQuery::new("budi", "rumah kayu dekat sungai")).await.unwrap();
    assert!(hits.found, "embedding harus ada setelah sync");

    // B menambah cerita; A menerimanya. Data A tidak hilang.
    ingest(b.clone(), "Tahun 1998 saya merantau ke Jakarta dan bekerja di pabrik tekstil di Tangerang.").await;
    sync::sync_now(&b).await.unwrap();
    let ra = sync::sync_now(&a).await.unwrap();
    assert_eq!(ra.pulled_from, vec!["PC kantor".to_string()], "{ra:?}");
    assert_eq!(a.store.count_episodes("budi").await.unwrap(), 2);
    assert_eq!(b.store.count_episodes("budi").await.unwrap(), 2);

    // B menarik snapshot A yang isinya sudah sama: tidak ada record yang ditimpa.
    let same = sync::sync_now(&b).await.unwrap();
    assert_eq!(same.pulled_from.len(), 1, "{same:?}");
    assert_eq!((same.added, same.updated, same.embedded), (0, 0, 0), "{same:?}");

    // Tanpa perubahan: tidak ada yang diunggah atau ditarik ulang.
    let again = sync::sync_now(&a).await.unwrap();
    let again_b = sync::sync_now(&b).await.unwrap();
    assert!(!again_b.pushed && again_b.pulled_from.is_empty() && again_b.added + again_b.updated == 0, "{again_b:?} {again:?}");

    // Komputer ketiga dengan kata sandi salah: ditolak dengan pesan jelas, data tidak tercampur.
    let dc = tempfile::tempdir().unwrap();
    let c = rt(dc.path()).await;
    sync::save_settings(&c.vault, SyncSettings { dir: Some(drive.path().to_path_buf()), interval_minutes: 0, device_name: None }).unwrap();
    sync::set_passphrase(&c.vault, Some("kata-sandi-lain")).unwrap();
    let rc = sync::sync_now(&c).await.unwrap();
    assert!(rc.pulled_from.is_empty() && rc.warnings.iter().any(|w| w.contains("tidak cocok")), "{rc:?}");
    assert_eq!(c.store.count_episodes("budi").await.unwrap(), 0);
    assert!(sync::set_passphrase(&c.vault, Some("pendek")).is_err());
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().is_dir() {
            out.extend(walk(&e.path()));
        } else {
            out.push(e.path());
        }
    }
    out
}

#[test]
fn merge_unions_id_lists_and_converges() {
    use crate::sync::{merge, same};
    let a = json!({"id": "e1", "name": "Budi", "episode_ids": ["x", "y"], "mention_count": 2, "error": null});
    let b = json!({"id": "e1", "name": "Budi", "episode_ids": ["x", "z"], "mention_count": 3});
    // Masing-masing komputer menggabung dari sisinya; hasilnya setara.
    let on_a = merge(&a, &b);
    let on_b = merge(&b, &a);
    assert!(same(&on_a, &on_b), "{on_a} vs {on_b}");
    assert_eq!(on_a["mention_count"], 3);
    assert_eq!(on_a["episode_ids"].as_array().unwrap().len(), 3);
    // null sama dengan field tidak ada.
    assert!(same(&json!({"id": "s", "error": null}), &json!({"id": "s"})));
}
