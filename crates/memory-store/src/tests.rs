use serde::{Deserialize, Serialize};
use serde_json::json;

use super::*;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Doc {
    id: String,
    subject: String,
    text: String,
    #[serde(default)]
    valid_to: Option<String>,
    #[serde(default)]
    status: String,
}

async fn mem() -> Store {
    Store::memory(4, &IndexConfig::default()).await.unwrap()
}

#[tokio::test]
async fn upsert_get_roundtrip_with_ulid_key() {
    let s = mem().await;
    let id = memory_core::new_id();
    let d = Doc { id: id.clone(), subject: "s".into(), text: "halo".into(), valid_to: None, status: "active".into() };
    s.upsert("episode", &id, &d, Some(&[1.0, 0.0, 0.0, 0.0])).await.unwrap();
    let back: Doc = s.require("episode", &id).await.unwrap();
    assert_eq!(back, d);
    assert_eq!(s.embedding_of("episode", &id).await.unwrap().unwrap().len(), 4);
}

#[tokio::test]
async fn absent_optional_matches_none_condition() {
    let s = mem().await;
    for (i, vt) in [None, Some("2015-01-01")].into_iter().enumerate() {
        let id = format!("f{i}");
        let d = Doc { id: id.clone(), subject: "s".into(), text: "x".into(), valid_to: vt.map(Into::into), status: "active".into() };
        s.upsert("fact", &id, &d, None).await.unwrap();
    }
    let open: Vec<Doc> = s
        .rows_as("SELECT * FROM fact WHERE (valid_to = NONE OR valid_to = NULL);", binds! {})
        .await
        .unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, "f0");
}

#[tokio::test]
async fn hnsw_search_with_subject_filter_and_missing_embeddings() {
    let s = mem().await;
    let vecs = [("a", "s1", Some([1.0, 0.0, 0.0, 0.0])), ("b", "s1", Some([0.9, 0.1, 0.0, 0.0])), ("c", "s2", Some([1.0, 0.0, 0.0, 0.0])), ("d", "s1", None)];
    for (id, subj, v) in vecs {
        let d = Doc { id: id.into(), subject: subj.into(), text: "t".into(), valid_to: None, status: "active".into() };
        s.upsert("episode", id, &d, v.as_ref().map(|x| x.as_slice())).await.unwrap();
    }
    let hits = s.vector_index().search("episode", "s1", &[1.0, 0.0, 0.0, 0.0], 5, "status = 'active'").await.unwrap();
    let ids: Vec<_> = hits.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b"]);
    assert!(hits[0].similarity > 0.99);
}

#[tokio::test]
async fn diskann_index_works_too() {
    let idx = IndexConfig { vector: "diskann".into(), ..Default::default() };
    let s = Store::memory(4, &idx).await.unwrap();
    let d = Doc { id: "a".into(), subject: "s".into(), text: "t".into(), valid_to: None, status: "active".into() };
    s.upsert("episode", "a", &d, Some(&[0.0, 1.0, 0.0, 0.0])).await.unwrap();
    let hits = s.vector_index().search("episode", "s", &[0.0, 1.0, 0.0, 0.0], 3, "").await.unwrap();
    assert_eq!(hits.len(), 1);
}

#[tokio::test]
async fn fulltext_bm25() {
    let s = mem().await;
    for (id, t) in [("a", "Pakde Karno membuka toko batik di Solo"), ("b", "Kami makan bakso bersama")] {
        let d = Doc { id: id.into(), subject: "s".into(), text: t.into(), valid_to: None, status: "active".into() };
        s.upsert("episode", id, &d, None).await.unwrap();
    }
    let rows = s
        .rows(
            "SELECT id, search::score(1) AS score FROM episode WHERE text @1@ $q AND subject = $s ORDER BY score DESC;",
            binds! {"q" => "toko batik", "s" => "s"},
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], json!("a"));
}

#[tokio::test]
async fn transaction_is_atomic() {
    let s = mem().await;
    let mut tx = s.tx();
    tx.upsert("episode", "ok", &json!({"subject": "s", "text": "a"}), None).unwrap();
    tx.raw("THROW 'gagal';", binds! {});
    assert!(tx.commit().await.is_err());
    assert!(s.get::<serde_json::Value>("episode", "ok").await.unwrap().is_none());
}

#[tokio::test]
async fn dimension_mismatch_is_detected_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let _s = Store::open(dir.path(), 4, &IndexConfig::default()).await.unwrap();
    }
    // RocksDB melepas LOCK secara asinkron setelah drop.
    let mut last = String::new();
    for _ in 0..50 {
        match Store::open(dir.path(), 8, &IndexConfig::default()).await {
            Err(e) if e.to_string().contains("lock") => {
                last = e.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Err(e) => {
                assert!(e.to_string().contains("dimensi"), "{e}");
                return;
            }
            Ok(_) => panic!("dimensi berbeda seharusnya ditolak"),
        }
    }
    panic!("LOCK RocksDB tidak terlepas: {last}");
}

mod repo_tests {
    use super::*;
    use memory_core::model::*;
    use memory_core::time::TimeRef;

    fn ep(id: &str, start: Option<&str>, end: Option<&str>) -> Episode {
        Episode {
            id: id.into(),
            subject: "s".into(),
            source_id: "src".into(),
            text: format!("cerita {id}"),
            summary: String::new(),
            title: String::new(),
            time: TimeRef::Unknown,
            time_start: start.map(Into::into),
            time_end: end.map(Into::into),
            time_ongoing: false,
            time_confidence: 0.5,
            importance: 0.5,
            emotions: vec![],
            languages: vec![],
            life_phase: None,
            project: None,
            speaker: None,
            status: EpisodeStatus::Active,
            retelling_of: None,
            retell_count: 0,
            decisions: vec![],
            quotes: vec![],
            trait_signals: vec![],
            entity_ids: vec!["e1".into()],
            links: vec![],
            obs_type: None,
            concepts: vec![],
            files_read: vec![],
            files_modified: vec![],
            session_id: None,
            prompt_number: None,
            observed_at: "2024-01-01".into(),
            created_at: memory_core::ids::now_rfc3339(),
        }
    }

    fn job(key: &str) -> Job {
        Job {
            id: memory_core::new_id(),
            kind: "process_source".into(),
            payload: json!({}),
            status: JobStatus::Queued,
            attempts: 0,
            max_attempts: 2,
            idempotency_key: key.into(),
            progress_done: 0,
            progress_total: 0,
            stage: None,
            error: None,
            batch_id: None,
            run_after: "2000-01-01T00:00:00Z".into(),
            created_at: memory_core::ids::now_rfc3339(),
            updated_at: String::new(),
        }
    }

    #[tokio::test]
    async fn episodes_by_ids_window_and_mentions() {
        let s = mem().await;
        s.put_episode(&ep("a", Some("1999-01-01"), Some("2000-01-01")), None).await.unwrap();
        s.put_episode(&ep("b", Some("2019-01-01"), None), None).await.unwrap();
        s.put_episode(&ep("c", None, None), None).await.unwrap();
        let got = s.episodes_by_ids(&["b".into(), "a".into()]).await.unwrap();
        assert_eq!(got.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["b", "a"]);
        let f = EpisodeFilter { from: Some("1998-01-01".into()), to: Some("2005-01-01".into()), ..Default::default() };
        let w = s.list_episodes("s", &f).await.unwrap();
        assert_eq!(w.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["a"]);
        assert_eq!(s.episodes_mentioning("e1", 10).await.unwrap().len(), 3);
        assert_eq!(s.count_episodes("s").await.unwrap(), 3);
    }

    #[tokio::test]
    async fn entity_alias_lookup_and_merge_chain() {
        let s = mem().await;
        let mut e = Entity {
            id: "e1".into(),
            subject: "s".into(),
            kind: EntityKind::Person,
            name: "Sukarno Hadi".into(),
            aliases: vec!["Pakde Karno".into()],
            description: String::new(),
            status: EntityStatus::Confirmed,
            merged_into: None,
            is_self: false,
            gender: None,
            mention_count: 2,
            episode_ids: vec![],
            created_at: String::new(),
        };
        s.put_entity(&e, Some(&[1.0, 0.0, 0.0, 0.0])).await.unwrap();
        assert_eq!(s.entities_by_alias("s", "pakde  KARNO").await.unwrap().len(), 1);
        e.id = "e0".into();
        e.status = EntityStatus::Merged;
        e.merged_into = Some("e1".into());
        s.put_entity(&e, None).await.unwrap();
        assert_eq!(s.resolve_entity("e0").await.unwrap().unwrap().id, "e1");
        assert_eq!(s.list_entities("s", Some(EntityKind::Person), 10, 0).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn job_queue_idempotent_claim_and_retry() {
        let s = mem().await;
        let (j, new) = s.enqueue(&job("k1")).await.unwrap();
        assert!(new);
        let (_, again) = s.enqueue(&job("k1")).await.unwrap();
        assert!(!again);
        let now = memory_core::ids::now_rfc3339();
        let claimed = s.claim_job(&now, None).await.unwrap().unwrap();
        assert_eq!(claimed.id, j.id);
        assert_eq!(claimed.attempts, 1);
        assert!(s.claim_job(&now, None).await.unwrap().is_none(), "tidak boleh diklaim dua kali");
        s.finish_job(&claimed, Err("boom".into())).await.unwrap();
        let back: Job = s.require("job", &j.id).await.unwrap();
        assert_eq!(back.status, JobStatus::Queued);
        assert!(back.run_after > now, "harus ada backoff");
        let mut c2 = back.clone();
        c2.attempts = 2;
        s.finish_job(&c2, Err("boom".into())).await.unwrap();
        let failed: Job = s.require("job", &j.id).await.unwrap();
        assert_eq!(failed.status, JobStatus::Failed);
        let (_, requeued) = s.enqueue(&job("k1")).await.unwrap();
        assert!(requeued, "job gagal diantre ulang saat diimport lagi");
        assert_eq!(s.pending_job_count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn batch_counters_and_stats() {
        let s = mem().await;
        let b = ImportBatch { id: "b1".into(), subject: "s".into(), label: "x".into(), total: 0, done: 0, failed: 0, duplicates: 0, failures: vec![], created_at: memory_core::ids::now_rfc3339(), updated_at: String::new() };
        s.put_batch(&b).await.unwrap();
        s.batch_incr("b1", "total", None).await.unwrap();
        s.batch_incr("b1", "failed", Some("a.txt: rusak".into())).await.unwrap();
        let got = s.get_batch("b1").await.unwrap().unwrap();
        assert_eq!((got.total, got.failed, got.failures.len()), (1, 1, 1));
        assert!(s.batch_incr("b1", "id", None).await.is_err());
        let st = s.stats("s").await.unwrap();
        assert_eq!(st.episodes, 0);
    }

    #[tokio::test]
    async fn facts_by_category_and_evidence() {
        let s = mem().await;
        let f = Fact {
            id: "f1".into(),
            subject: "s".into(),
            entity_id: "e1".into(),
            predicate: "mendukung".into(),
            category: "afiliasi_politik".into(),
            object_entity: None,
            object_text: "Partai A".into(),
            kind: FactKind::Preference,
            exclusive: true,
            time: TimeRef::Unknown,
            valid_from: Some("1999-01-01".into()),
            valid_to: None,
            ongoing: false,
            time_confidence: 0.8,
            observed_at: "2024-01-01".into(),
            confidence: 0.9,
            evidence: vec![Evidence { episode_id: "ep1".into(), quote: "q".into() }],
            status: FactStatus::Active,
            superseded_by: None,
            supersedes: None,
            created_at: String::new(),
        };
        s.put_fact(&f).await.unwrap();
        assert_eq!(s.facts_in_category("e1", "afiliasi_politik").await.unwrap().len(), 1);
        assert_eq!(s.facts_by_episode("ep1").await.unwrap().len(), 1);
        assert_eq!(s.facts_for_entity("e1").await.unwrap().len(), 1);
        let dump = s.dump_table("fact", false, 10, 0).await.unwrap();
        assert_eq!(dump[0]["id"], json!("f1"));
    }
}

#[tokio::test]
async fn project_meta_roundtrip_and_backlinks() {
    use memory_core::model::ProjectMeta;
    let store = mem().await;
    let a = ProjectMeta::build("budi", "BACKEND/api-x", Some("git@github.com:org/api-x.git"), "Memakai [[LLM-MEMORY]] dan [[tabular|tabel]]", &["#backend, OPERON".into()], None);
    let b = ProjectMeta::build("budi", "LLM-MEMORY", None, "Tanpa tautan, hanya [[LLM-MEMORY]] dirinya sendiri", &[], None);
    store.put_project_meta(&a).await.unwrap();
    store.put_project_meta(&b).await.unwrap();
    assert!(b.links.is_empty(), "tautan ke diri sendiri dibuang");
    let got = store.get_project_meta("budi", "api-x").await.unwrap().expect("varian nama ditemukan");
    assert_eq!(got.tags, vec!["backend".to_string(), "OPERON".to_string()]);
    assert_eq!(got.links, vec!["LLM-MEMORY".to_string(), "tabular".to_string()]);
    assert!(store.get_project_meta("lain", "api-x").await.unwrap().is_none(), "terisolasi per subjek");
    let all = store.list_project_meta("budi").await.unwrap();
    assert_eq!(Store::project_backlinks(&all, "llm-memory"), vec!["BACKEND/api-x".to_string()]);
    assert_eq!(Store::project_backlinks(&all, "TABULAR"), vec!["BACKEND/api-x".to_string()]);
    // Simpan ulang memakai ID yang sama (deterministik), bukan record baru.
    let a2 = ProjectMeta::build("budi", "BACKEND/api-x", None, "", &[], Some(&got));
    assert_eq!(a2.id, got.id);
    assert_eq!(a2.created_at, got.created_at);
    store.put_project_meta(&a2).await.unwrap();
    assert_eq!(store.list_project_meta("budi").await.unwrap().len(), 2);
}

