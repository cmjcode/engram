//! Uji retrieval ujung-ke-ujung: data diisi oleh pipeline asli dengan LLM tiruan.

use std::sync::Arc;

use chrono::NaiveDate;
use memory_core::MemoryConfig;
use memory_extract::ingest::IngestDoc;
use memory_extract::{Pipeline, Vault};
use memory_llm::mock::{ScriptedLlm, last_user};
use memory_llm::{ChatRequest, HashEmbedder};
use memory_core::model::{ProjectMeta, SourceKind};
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
            if text.contains("SurrealDB") {
                out["title"] = json!("Arsitektur penyimpanan LLM-MEMORY");
                out["entities"].as_array_mut().unwrap().push(json!({"name": "LLM-MEMORY", "kind": "project"}));
                out["facts"].as_array_mut().unwrap().push(json!({
                    "subject": "Budi", "predicate": "menerapkan", "object": "SurrealDB embedded",
                    "object_is_entity": false, "category": "arsitektur", "kind": "fact", "exclusive": false,
                    "time": {"kind": "unknown"}, "quote": "memakai SurrealDB", "confidence": 0.9
                }));
                out["decisions"] = json!([{"situation": "Butuh database lokal", "action": "Memakai SurrealDB embedded", "reason": "Satu binary"}]);
            }
            if text.contains("Kandang__unit_id") {
                out["title"] = json!("Filter kandang di tracker");
                out["entities"].as_array_mut().unwrap().push(json!({"name": "FRONTEND/TRACKER", "kind": "project"}));
                out["facts"].as_array_mut().unwrap().push(json!({
                    "subject": "Budi", "predicate": "mendukung", "object": "filter Kandang__unit_id.in",
                    "object_is_entity": false, "category": "arsitektur", "kind": "fact", "exclusive": false,
                    "time": {"kind": "unknown"}, "quote": "filter Kandang__unit_id.in", "confidence": 0.9
                }));
                out["decisions"] = json!([{"situation": "Filter kandang lambat", "action": "Memakai Kandang__unit_id.in", "reason": "Query cepat"}]);
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

#[tokio::test]
async fn project_context_and_project_scoped_recall() {
    let (r, p, _d) = setup().await;
    let out = p
        .ingest(IngestDoc {
            subject: "budi".into(),
            kind: SourceKind::AgentConversation,
            title: "Diingat agent: antigravity:LLM-MEMORY".into(),
            text: "Pada project LLM-MEMORY kita memutuskan memakai SurrealDB dan BAAI/bge-m3 untuk penyimpanan vektor.".into(),
            meta: json!({"source": "antigravity:LLM-MEMORY"}),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(out.source.project.as_deref(), Some("LLM-MEMORY"));
    p.process_source(&out.source.id, None).await.unwrap();

    // Proyek lain dengan kategori fakta yang sama ("arsitektur") pada entitas diri ("Budi")
    // tidak boleh bocor ke project_context LLM-MEMORY.
    let out_other = p
        .ingest(IngestDoc {
            subject: "budi".into(),
            kind: SourceKind::AgentConversation,
            title: "Diingat agent: antigravity:FRONTEND/TRACKER".into(),
            text: "Pada project FRONTEND/TRACKER kita menerapkan arsitektur filter Kandang__unit_id.in untuk performa.".into(),
            meta: json!({"source": "antigravity:FRONTEND/TRACKER"}),
            ..Default::default()
        })
        .await
        .unwrap();
    p.process_source(&out_other.source.id, None).await.unwrap();

    // Metadata proyek: repo + tag di LLM-MEMORY, tautan [[LLM-MEMORY]] dari FRONTEND/TRACKER (backlink).
    let store = &r.store;
    store
        .put_project_meta(&ProjectMeta::build("budi", "LLM-MEMORY", Some("git@github.com:budi/engram.git"), "Dipakai oleh [[api-lain]].", &["#memori".into()], None))
        .await
        .unwrap();
    store.put_project_meta(&ProjectMeta::build("budi", "FRONTEND/TRACKER", None, "Butuh [[LLM-MEMORY]]", &[], None)).await.unwrap();

    let pc = r.project_context("budi", "LLM-MEMORY", Some("arsitektur penyimpanan vektor")).await.unwrap();
    assert!(pc.found);
    assert_eq!(pc.repo.as_deref(), Some("git@github.com:budi/engram.git"));
    assert_eq!(pc.related, vec!["api-lain".to_string(), "FRONTEND/TRACKER".to_string()]);
    assert!(pc.context.contains("Repositori: budi/engram") && pc.context.contains("#memori") && pc.context.contains("[[FRONTEND/TRACKER]]"), "{}", pc.context);
    assert_eq!(pc.project, "LLM-MEMORY");
    assert!(!pc.episodes.is_empty());
    assert!(pc.context.contains("LLM-MEMORY"));
    assert!(pc.context.contains("SurrealDB"), "{}", pc.context);
    assert!(!pc.context.contains("Kandang__unit_id"), "Fakta proyek lain bocor ke LLM-MEMORY:\n{}", pc.context);
    assert!(!pc.decisions.iter().any(|d| d.action.contains("Kandang__unit_id")));

    let pc_other = r.project_context("budi", "FRONTEND/TRACKER", None).await.unwrap();
    assert!(pc_other.found);
    assert!(pc_other.context.contains("Kandang__unit_id"), "{}", pc_other.context);
    assert!(!pc_other.context.contains("SurrealDB embedded"), "{}", pc_other.context);

    let q = RecallQuery {
        project: Some("LLM-MEMORY".into()),
        ..RecallQuery::new("budi", "arsitektur menyimpan vektor")
    };
    let res = r.recall(&q).await.unwrap();
    assert!(res.found);
    assert!(res.episodes.iter().any(|e| e.project.as_deref() == Some("LLM-MEMORY")));
    assert!(!res.facts.iter().any(|f| f.statement.contains("Kandang__unit_id")));
}

#[tokio::test]
async fn index_mode_get_episodes_and_anchor_timeline() {
    let (r, p, _d) = setup().await;
    // Memori proyek dengan alias: "engram" adalah alias LLM-MEMORY.
    let out = p
        .ingest(IngestDoc {
            subject: "budi".into(),
            kind: SourceKind::AgentConversation,
            title: "Diingat agent: claude-code:LLM-MEMORY".into(),
            text: "Pada project LLM-MEMORY kita memutuskan memakai SurrealDB embedded untuk penyimpanan vektor karena ekspor JSONL tetap sederhana.".into(),
            meta: json!({"source": "claude-code:LLM-MEMORY"}),
            ..Default::default()
        })
        .await
        .unwrap();
    p.process_source(&out.source.id, None).await.unwrap();

    // Mode indeks: satu baris per episode dengan ID & perkiraan token, tanpa kutipan panjang.
    let q = RecallQuery { detail: Some("index".into()), project: Some("LLM-MEMORY".into()), ..RecallQuery::new("budi", "penyimpanan vektor SurrealDB") };
    let res = r.recall(&q).await.unwrap();
    assert!(res.found);
    assert!(res.context.contains("## Indeks memori"), "{}", res.context);
    let line = res.context.lines().find(|l| l.starts_with("- ep:")).expect("baris indeks");
    assert!(line.contains(" tok"), "{line}");
    assert!(res.context.chars().count() < 1500, "{}", res.context);
    // Fakta kategori peristiwa disembunyikan di indeks kecuali diminta.
    assert!(!res.context.contains("## Fakta") || !res.context.contains("peristiwa"));

    // Detail per ID.
    let id = res.episodes[0].id.clone();
    let d = r.get_episodes("budi", &[format!("ep:{id}"), "tidakada".into()], 2000).await.unwrap();
    assert_eq!(d.episodes.len(), 1);
    assert_eq!(d.missing, vec!["tidakada".to_string()]);
    assert!(d.context.contains("SurrealDB"));
    assert!(d.context.contains(&format!("[ep:{id}]")));

    // Timeline jangkar: episode proyek yang sama di sekitar jangkar.
    let tl = r.timeline_around("budi", &id, 3, 3, None).await.unwrap();
    assert!(tl.items.iter().any(|i| i.anchor && i.id == id));
    assert!(tl.items.iter().all(|i| i.project.as_deref() == Some("LLM-MEMORY")));
    assert!(r.timeline_around("budi", "ep:nope", 1, 1, None).await.is_err());

    // Anggaran: project_context dengan budget kecil tetap valid dan pendek.
    let pc = r
        .project_context_with("budi", "LLM-MEMORY", None, &crate::ToolOpts { budget_tokens: Some(200), ..Default::default() })
        .await
        .unwrap();
    assert!(pc.found);
    assert!(pc.context.chars().count() <= 200 * 4 + 200, "{}", pc.context.chars().count());
    assert!(pc.context.starts_with("# Konteks Proyek"));
    assert_eq!(crate::fit_text("a\nb\nc", 100), "a\nb\nc");
    assert!(crate::fit_text(&"x".repeat(1000), 200).chars().count() <= 200);
}

#[allow(dead_code)]
fn _unused(_: Value) {}

#[tokio::test]
async fn project_context_and_recall_cover_project_group() {
    use memory_core::model::{Entity, EntityKind, EntityStatus, Evidence, Fact, FactKind, FactStatus};
    use memory_core::TimeRef;
    let (r, p, _d) = setup().await;
    let mut ep_of = std::collections::HashMap::new();
    for (proj, text) in [
        ("api-a", "Pada project api-a kita memutuskan memakai JWT untuk otentikasi pengguna."),
        ("api-b", "Pada project api-b kita memutuskan memakai antrean Redis Streams untuk job laporan."),
        ("VOLTUNES", "Seluruh layanan VOLTUNES memakai paket go-helper sqlsafe untuk kueri aman."),
        ("OTHER", "Pada project OTHER kita memakai Kafka untuk telemetri armada."),
    ] {
        let out = p
            .ingest(IngestDoc {
                subject: "budi".into(),
                kind: SourceKind::AgentConversation,
                title: format!("Diingat agent: claude-code:{proj}"),
                text: text.into(),
                meta: json!({"source": format!("claude-code:{proj}")}),
                ..Default::default()
            })
            .await
            .unwrap();
        p.process_source(&out.source.id, None).await.unwrap();
        let eps = r.store.list_episodes("budi", &memory_store::EpisodeFilter { project: Some(proj.into()), limit: 5, ..Default::default() }).await.unwrap();
        ep_of.insert(proj, eps[0].id.clone());
    }
    let store = &r.store;
    for (p, tags) in [("api-a", vec!["VOLTUNES"]), ("api-b", vec!["#voltunes"]), ("OTHER", vec!["FOX"])] {
        store.put_project_meta(&ProjectMeta::build("budi", p, None, "", &tags.iter().map(|t| t.to_string()).collect::<Vec<_>>(), None)).await.unwrap();
    }
    let ent = |id: &str, name: &str, kind: EntityKind| Entity {
        id: id.into(),
        subject: "budi".into(),
        kind,
        name: name.into(),
        aliases: vec![],
        description: String::new(),
        status: EntityStatus::Confirmed,
        merged_into: None,
        is_self: false,
        gender: None,
        mention_count: 1,
        episode_ids: vec![],
        created_at: String::new(),
    };
    store.put_entity(&ent("ent-b", "api-b", EntityKind::Project), None).await.unwrap();
    store.put_entity(&ent("ent-v", "VOLTUNES", EntityKind::Org), None).await.unwrap();
    let fact = |id: &str, eid: &str, pred: &str, obj: &str, cat: &str, ep: &str| Fact {
        id: id.into(),
        subject: "budi".into(),
        entity_id: eid.into(),
        predicate: pred.into(),
        category: cat.into(),
        object_entity: None,
        object_text: obj.into(),
        kind: FactKind::default(),
        exclusive: false,
        time: TimeRef::Unknown,
        valid_from: Some("2026-01-01".into()),
        valid_to: None,
        ongoing: true,
        time_confidence: 0.8,
        observed_at: "2026-01-01".into(),
        confidence: 0.9,
        evidence: vec![Evidence { episode_id: ep.into(), quote: String::new() }],
        status: FactStatus::Active,
        superseded_by: None,
        supersedes: None,
        created_at: String::new(),
    };
    store.put_fact(&fact("f-b1", "ent-b", "adalah", "layanan antrean laporan berbasis Redis Streams", "arsitektur", &ep_of["api-b"])).await.unwrap();
    store.put_fact(&fact("f-b2", "ent-b", "menggunakan", "Go", "tech_stack", &ep_of["api-b"])).await.unwrap();
    store.put_fact(&fact("f-v1", "ent-v", "berjalan di", "jaringan docker eksternal chick", "infrastruktur", &ep_of["VOLTUNES"])).await.unwrap();

    // Satu repo ditanya: konteks memuat peta repo saudara + arsitektur payung, tanpa grup lain.
    let pc = r.project_context("budi", "api-a", None).await.unwrap();
    let g = pc.group.as_ref().expect("api-a tergabung dalam grup VOLTUNES");
    assert_eq!(g.group.members, vec!["api-b".to_string()]);
    assert_eq!(g.group.umbrella.as_deref(), Some("VOLTUNES"));
    assert!(pc.context.contains("## Ekosistem grup #VOLTUNES"), "{}", pc.context);
    assert!(pc.context.contains("- api-b: adalah layanan antrean laporan berbasis Redis Streams | Go"), "{}", pc.context);
    assert!(pc.context.contains("jaringan docker eksternal chick"), "{}", pc.context);
    assert!(!pc.context.contains("Kafka"), "grup lain bocor:\n{}", pc.context);
    assert!(g.episodes.iter().any(|e| e.project.as_deref() == Some("api-b")));

    // Repo baru tanpa tag/memori: grup dikenali dari folder induk di path sesi tidak tersedia,
    // tetapi `VOLTUNES/<repo>` dikenali dari prefiksnya.
    let fresh = r.project_context("budi", "VOLTUNES/api-new", None).await.unwrap();
    assert!(fresh.found && fresh.context.contains("lihat konteks grupnya"), "{}", fresh.context);

    // Nama grup sendiri: seluruh anggota.
    let all = r.project_context("budi", "VOLTUNES", None).await.unwrap();
    let ga = all.group.as_ref().unwrap();
    assert!(ga.group.is_umbrella);
    assert_eq!(ga.group.members, vec!["api-a".to_string(), "api-b".to_string()]);

    // Bisa dimatikan.
    let solo = r.project_context_with("budi", "api-a", None, &crate::ToolOpts { skip_group: true, ..Default::default() }).await.unwrap();
    assert!(solo.group.is_none() && !solo.context.contains("Ekosistem grup"));

    // recall dengan project: fakta repo saudara ikut, kecuali skip_group.
    let q = RecallQuery { project: Some("api-a".into()), ..RecallQuery::new("budi", "api-b antrean Redis Streams") };
    let res = r.recall(&q).await.unwrap();
    assert!(res.facts.iter().any(|f| f.id == "f-b1"), "{:?}", res.facts.iter().map(|f| &f.statement).collect::<Vec<_>>());
    assert!(res.episodes.iter().any(|e| e.project.as_deref() == Some("api-b") && e.via.contains(&"group".to_string())));
    let res_solo = r.recall(&RecallQuery { skip_group: true, ..q.clone() }).await.unwrap();
    assert!(!res_solo.facts.iter().any(|f| f.id == "f-b1"));
}
