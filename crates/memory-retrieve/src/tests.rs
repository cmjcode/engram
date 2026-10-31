//! Uji retrieval ujung-ke-ujung: data diisi oleh pipeline asli dengan LLM tiruan.

use std::sync::Arc;

use chrono::NaiveDate;
use memory_core::MemoryConfig;
use memory_extract::ingest::IngestDoc;
use memory_extract::{Pipeline, Vault};
use memory_llm::mock::{ScriptedLlm, last_user};
use memory_llm::{ChatRequest, HashEmbedder};
use memory_core::model::SourceKind;
use memory_store::Store;
use serde_json::{Value, json};

use crate::{RecallQuery, Retriever};

const DIM: usize = 256;

fn units(user: &str) -> Vec<usize> {
    user.lines().filter_map(|l| l.strip_prefix('[')?.split_once("] ")?.0.parse().ok()).collect()
}

fn llm(req: &ChatRequest) -> memory_llm::Result<String> {
    let user = last_user(req);
    let fact = |pred: &str, obj: &str, age: u32, quote: &str| {
        json!({"subject": "Budi", "predicate": pred, "object": obj, "object_is_entity": true, "category": "afiliasi_politik",
               "kind": "preference", "exclusive": true, "time": {"kind": "age", "age": age}, "quote": quote, "confidence": 0.9})
    };
    match req.task.as_str() {
        "segment" => Ok(json!({"segments": units(user).into_iter().map(|i| json!({"start": i, "end": i, "significance": 0.9})).collect::<Vec<_>>()}).to_string()),
        "extract" => {
            let text = user.split("TEKS:\n").nth(1).unwrap_or("");
            let mut out = json!({"title": "cerita", "summary": text, "time": {"kind": "unknown"},
                                  "entities": [{"name": "Budi", "kind": "person", "is_self": true}], "facts": [], "kin": []});
            if text.contains("Partai A") {
                out["time"] = json!({"kind": "age", "age": 20});
                out["title"] = json!("Kampanye Partai A");
                out["facts"].as_array_mut().unwrap().push(fact("mendukung", "Partai A", 20, "saya pendukung Partai A"));
            }
            if text.contains("Partai B") {
                out["time"] = json!({"kind": "age", "age": 40});
                out["title"] = json!("Pindah ke Partai B");
                out["facts"].as_array_mut().unwrap().push(fact("mendukung", "Partai B", 40, "saya pindah mendukung Partai B"));
            }
            if text.contains("Pakde Karno") {
                out["title"] = json!("Toko batik Pakde");
                out["entities"].as_array_mut().unwrap().push(json!({"name": "Pakde Karno", "kind": "person", "aliases": ["Karno"]}));
                out["kin"] = json!([{"person": "Pakde Karno", "term": "pakde", "quote": "Pakde Karno"}]);
            }
            Ok(out.to_string())
        }
        "ask" => Ok("Dulu Partai A, sekarang Partai B [ep:ngawur] dan [ep:REAL].".into()),
        _ => Ok(json!({"text": "x", "same": false}).to_string()),
    }
}

async fn setup() -> (Retriever, Pipeline, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Arc::new(MemoryConfig::for_tests(DIM));
    let store = Store::memory(DIM, &cfg.index).await.unwrap();
    let vault = Vault::new(dir.path());
    vault.init().unwrap();
    let llm: Arc<dyn memory_llm::LlmProvider> = Arc::new(ScriptedLlm::new(llm));
    let emb: Arc<dyn memory_llm::Embedder> = Arc::new(HashEmbedder::new(DIM));
    let p = Pipeline::new(store.clone(), llm.clone(), emb.clone(), cfg.clone(), vault);
    let mut s = p.ensure_subject("budi", Some("Budi")).await.unwrap();
    s.anchors.birth = NaiveDate::from_ymd_opt(1979, 3, 10);
    store.put_subject(&s).await.unwrap();
    let text = "Waktu umur 20 saya pendukung Partai A, ikut kampanye keliling kampung.\n\
                Setelah umur 40 saya pindah mendukung Partai B karena kecewa dengan pimpinannya.\n\
                Pakde Karno membuka toko batik di Solo dan sering mengajak saya ke pasar Klewer.";
    let out = p.ingest(IngestDoc { subject: "budi".into(), kind: SourceKind::Interview, title: "w".into(), text: text.into(), ..Default::default() }).await.unwrap();
    p.process_source(&out.source.id, None).await.unwrap();
    (Retriever::new(store, emb, llm, cfg), p, dir)
}

fn fact<'a>(r: &'a crate::RecallResult, obj: &str) -> Option<&'a crate::FactView> {
    r.facts.iter().find(|f| f.statement.ends_with(obj))
}

#[tokio::test]
async fn now_query_separates_current_and_history() {
    let (r, _p, _d) = setup().await;
    let res = r.recall(&RecallQuery::new("budi", "Budi mendukung partai politik apa?")).await.unwrap();
    assert!(res.found);
    let a = fact(&res, "Partai A").expect("riwayat Partai A ikut");
    let b = fact(&res, "Partai B").expect("Partai B ada");
    assert!(b.current && !a.current);
    assert_eq!(a.valid_to.as_deref(), Some("2019-03-09"));
    assert!(res.context.contains("Partai B (berlaku sekarang"), "{}", res.context);
    assert!(res.context.contains("Partai A (dulu, 1999–2019"), "{}", res.context);
}

#[tokio::test]
async fn as_of_hides_the_future() {
    let (r, _p, _d) = setup().await;
    let q = RecallQuery { as_of: Some("2005-01-01".into()), ..RecallQuery::new("budi", "partai yang didukung") };
    let res = r.recall(&q).await.unwrap();
    assert!(fact(&res, "Partai A").unwrap().current);
    assert!(fact(&res, "Partai B").is_none(), "Partai B belum terjadi pada 2005");
    assert!(res.episodes.iter().all(|e| e.time_start.as_deref().is_none_or(|t| t <= "2005-01-01")));
}

#[tokio::test]
async fn window_filters_facts_and_episodes() {
    let (r, _p, _d) = setup().await;
    let q = RecallQuery { from: Some("2020-01-01".into()), to: Some("2023-12-31".into()), ..RecallQuery::new("budi", "partai") };
    let res = r.recall(&q).await.unwrap();
    assert!(fact(&res, "Partai A").is_none());
    assert!(fact(&res, "Partai B").is_some());
}

#[tokio::test]
async fn unrelated_question_returns_no_record() {
    let (r, _p, _d) = setup().await;
    let res = r.recall(&RecallQuery::new("budi", "resep rendang padang yang enak")).await.unwrap();
    assert!(!res.found, "{:?}", res.episodes.iter().map(|e| &e.title).collect::<Vec<_>>());
    assert!(res.context.contains("Tidak ada catatan"));
}

#[tokio::test]
async fn who_is_pakde_and_family_tree() {
    let (r, _p, _d) = setup().await;
    let w = r.who_is("budi", "Karno").await.unwrap().unwrap();
    assert_eq!(w.relation_to_subject.as_deref(), Some("pakde"));
    assert_eq!(w.episodes.len(), 1);
    let tree = r.family_tree("budi").await.unwrap();
    assert!(tree.edges.iter().any(|e| e.label == "keponakan"));
    let graph_hit = r.recall(&RecallQuery::new("budi", "cerita tentang Pakde Karno")).await.unwrap();
    assert!(graph_hit.episodes.iter().any(|e| e.via.contains(&"graph".to_string())));
}

#[tokio::test]
async fn persona_guardrails_and_citation_filtering() {
    let (r, _p, _d) = setup().await;
    let pc = r.persona_context("budi", "Partai apa yang Bapak dukung?", Some("2005-01-01")).await.unwrap();
    assert_eq!(pc.age_at_reference, Some(25));
    assert!(pc.system_prompt.contains("TIDAK tahu apa pun yang terjadi setelah 2005-01-01"));
    assert!(!pc.system_prompt.contains("Partai B"));
    let ans = r.ask("budi", "Partai apa yang Budi dukung?", false, None).await.unwrap();
    assert!(ans.found);
    assert!(!ans.citations.contains(&"ngawur".to_string()), "sitasi yang tidak ada di konteks dibuang");
    let none = r.ask("budi", "resep rendang padang yang enak", true, None).await.unwrap();
    assert!(!none.found);
}

#[test]
fn citation_parser() {
    assert_eq!(crate::citations("a [ep:x1] b [ep:y2][ep:x1]"), vec!["x1", "y2"]);
}

#[allow(dead_code)]
fn _unused(_: Value) {}
