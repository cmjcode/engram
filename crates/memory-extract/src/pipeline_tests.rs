//! Uji ujung-ke-ujung pipeline dengan LLM tiruan yang deterministik.

use chrono::NaiveDate;
use memory_core::model::*;
use memory_llm::ChatRequest;
use memory_llm::mock::last_user;
use serde_json::{Value, json};

use crate::ingest::IngestDoc;
use crate::testkit::pipeline_with;
use crate::Pipeline;

fn units(user: &str) -> Vec<(usize, String)> {
    user.lines()
        .filter_map(|l| {
            let rest = l.strip_prefix('[')?;
            let (n, t) = rest.split_once("] ")?;
            Some((n.parse().ok()?, t.to_string()))
        })
        .collect()
}

fn fact(subject: &str, pred: &str, obj: &str, cat: &str, kind: &str, time: Value, quote: &str) -> Value {
    json!({"subject": subject, "predicate": pred, "object": obj, "object_is_entity": true, "category": cat,
           "kind": kind, "exclusive": true, "time": time, "quote": quote, "confidence": 0.9})
}

/// LLM tiruan: segment = satu unit per segmen; extract = pola kata kunci.
fn llm(req: &ChatRequest) -> memory_llm::Result<String> {
    let user = last_user(req);
    match req.task.as_str() {
        "segment" => {
            let segs: Vec<Value> = units(user)
                .into_iter()
                .map(|(i, t)| {
                    let noise = t.contains("wkwk") || t.contains("oke");
                    json!({"start": i, "end": i, "topic": "t", "about_subject": !t.contains("IKLAN"),
                           "significance": if noise { 0.0 } else { 0.9 }})
                })
                .collect();
            Ok(json!({"segments": segs}).to_string())
        }
        "extract" => {
            let text = user.split("TEKS:\n").nth(1).unwrap_or("");
            let mut out = json!({"title": "cerita", "summary": text, "time": {"kind": "unknown"},
                                  "entities": [{"name": "Budi", "kind": "person", "is_self": true}],
                                  "facts": [], "kin": [], "decisions": [], "quotes": [], "trait_signals": [], "emotions": []});
            let facts = out["facts"].as_array_mut().unwrap();
            if text.contains("Partai A") {
                facts.push(fact("Budi", "mendukung", "Partai A", "afiliasi_politik", "preference", json!({"kind": "age", "age": 20}), "saya pendukung Partai A"));
            }
            if text.contains("Partai B") {
                facts.push(fact("Budi", "mendukung", "Partai B", "afiliasi_politik", "preference", json!({"kind": "age", "age": 40}), "saya pindah mendukung Partai B"));
            }
            if text.contains("karangan") {
                facts.push(fact("Budi", "tinggal di", "Paris", "tempat_tinggal", "fact", json!({"kind": "unknown"}), "saya tinggal di Paris sejak lahir"));
            }
            if text.contains("Pakde Karno") {
                out["entities"].as_array_mut().unwrap().push(json!({"name": "Pakde Karno", "kind": "person", "aliases": ["Karno"]}));
                out["kin"] = json!([{"person": "Pakde Karno", "term": "pakde", "quote": "Pakde Karno"}]);
            } else if text.contains("Pakde") {
                out["entities"].as_array_mut().unwrap().push(json!({"name": "Pakde", "kind": "person"}));
            }
            if text.contains("karyawan") {
                out["decisions"] = json!([{"situation": "usaha rugi", "action": "gaji karyawan dulu", "quote": "karyawan tetap digaji dulu"}]);
                out["trait_signals"] = json!([{"label": "peduli tim", "kind": "value", "polarity": 1, "quote": "karyawan tetap digaji dulu"}]);
            }
            Ok(out.to_string())
        }
        "same_entity" => Ok(json!({"same": false, "confidence": 0.2}).to_string()),
        "compact_episodes" => {
            let ids: Vec<String> = user
                .lines()
                .filter_map(|l| l.strip_prefix('[')?.split_once(']').map(|(id, _)| id.to_string()))
                .collect();
            Ok(json!({
                "rollups": [{
                    "episode_ids": ids,
                    "title": "Rangkuman Kerja Proyek",
                    "summary": "Ringkasan keputusan teknis proyek.",
                    "text": "Dalam proyek ini diputuskan pemakaian RocksDB dan perbaikan validasi tanggal."
                }]
            })
            .to_string())
        }
        "compact_facts" => Ok(json!({"merges": [], "closures": [], "rejections": []}).to_string()),
        other => Ok(json!({"text": format!("tiruan {other}")}).to_string()),
    }
}

async fn setup() -> (Pipeline, tempfile::TempDir) {
    let (p, dir) = pipeline_with(llm).await;
    let mut s = p.ensure_subject("budi", Some("Budi")).await.unwrap();
    s.anchors.birth = NaiveDate::from_ymd_opt(1979, 3, 10);
    p.store.put_subject(&s).await.unwrap();
    (p, dir)
}

async fn ingest(p: &Pipeline, kind: SourceKind, text: &str) -> Source {
    let out = p
        .ingest(IngestDoc { subject: "budi".into(), kind, title: "t".into(), text: text.into(), ..Default::default() })
        .await
        .unwrap();
    p.process_source(&out.source.id, out.job_id.as_deref()).await.unwrap();
    p.store.get_source(&out.source.id).await.unwrap().unwrap()
}

async fn self_id(p: &Pipeline) -> String {
    p.store.get_subject("budi").await.unwrap().unwrap().self_entity.unwrap()
}

#[tokio::test]
async fn party_change_is_stored_bitemporally_end_to_end() {
    let (p, _d) = setup().await;
    let src = ingest(
        &p,
        SourceKind::Interview,
        "Waktu umur 20 saya pendukung Partai A, ikut kampanye keliling kampung.\nSetelah umur 40 saya pindah mendukung Partai B karena kecewa dengan pimpinannya.",
    )
    .await;
    assert_eq!(src.status, SourceStatus::Done);
    assert_eq!(src.stats.episodes, 2, "{:?}", src.stats);
    let facts = p.store.facts_in_category(&self_id(&p).await, "afiliasi_politik").await.unwrap();
    let a = facts.iter().find(|f| f.object_text == "Partai A").unwrap();
    let b = facts.iter().find(|f| f.object_text == "Partai B").unwrap();
    assert_eq!(a.valid_from.as_deref(), Some("1999-03-10"));
    assert_eq!(a.valid_to.as_deref(), Some("2019-03-09"));
    assert_eq!(a.superseded_by.as_deref(), Some(b.id.as_str()));
    assert!(b.is_current());
    assert!(a.valid_at("2005-01-01") && !b.valid_at("2005-01-01"));
    // Partai menjadi entitas organisasi.
    let obj = p.store.get_entity(b.object_entity.as_deref().unwrap()).await.unwrap().unwrap();
    assert_eq!(obj.kind, EntityKind::Org);
}

#[tokio::test]
async fn rerun_is_idempotent() {
    let (p, _d) = setup().await;
    let src = ingest(&p, SourceKind::Interview, "Waktu umur 20 saya pendukung Partai A dan aktif di organisasi pemuda kampung.").await;
    let before = p.store.stats("budi").await.unwrap();
    // Paksa jalankan ulang seolah job diulang.
    p.store.set_source_status(&src.id, SourceStatus::Pending, None).await.unwrap();
    p.process_source(&src.id, None).await.unwrap();
    let after = p.store.stats("budi").await.unwrap();
    assert_eq!(before.episodes, after.episodes);
    assert_eq!(before.facts, after.facts);
    // Ingest teks yang sama = duplikat, tidak ada job baru.
    let dup = p.ingest(IngestDoc { subject: "budi".into(), kind: SourceKind::Interview, text: "Waktu umur 20 saya pendukung Partai A dan aktif di organisasi pemuda kampung.".into(), ..Default::default() }).await.unwrap();
    assert!(dup.duplicate && dup.job_id.is_none());
}

#[tokio::test]
async fn retold_story_links_to_canonical_and_strengthens_fact() {
    let (p, _d) = setup().await;
    let story = "Waktu umur 20 saya pendukung Partai A, ikut kampanye keliling kampung naik sepeda.";
    ingest(&p, SourceKind::Interview, story).await;
    let src2 = ingest(&p, SourceKind::Interview, &format!("{story} ")).await; // hash sama -> duplikat
    assert_eq!(src2.stats.episodes, 1, "teks identik adalah duplikat sumber yang sama");
    let src3 = ingest(&p, SourceKind::Memoir, "Waktu umur 20 saya pendukung Partai A, ikut kampanye keliling kampung naik sepeda!").await;
    assert_eq!(src3.stats.retellings, 1, "{:?}", src3.stats);
    let eps = p.store.list_episodes("budi", &Default::default()).await.unwrap();
    let canonical = eps.iter().find(|e| e.status == EpisodeStatus::Active).unwrap();
    let retelling = eps.iter().find(|e| e.status == EpisodeStatus::Retelling).unwrap();
    assert_eq!(retelling.retelling_of.as_deref(), Some(canonical.id.as_str()));
    assert_eq!(canonical.retell_count, 1);
    let facts = p.store.facts_in_category(&self_id(&p).await, "afiliasi_politik").await.unwrap();
    assert_eq!(facts.len(), 1, "fakta yang diceritakan ulang tidak digandakan");
    assert_eq!(facts[0].evidence.len(), 2);
}

#[tokio::test]
async fn chat_noise_never_becomes_episode() {
    let (p, _d) = setup().await;
    let src = ingest(&p, SourceKind::Chat, "[2024-01-01 08:00] Budi: oke sip\n[2024-01-01 08:01] Ani: wkwk oke\n[2024-01-01 08:02] IKLAN promo pulsa murah").await;
    assert_eq!(src.status, SourceStatus::Skipped);
    assert_eq!(src.stats.episodes, 0);
    assert_eq!(src.stats.not_about_subject, 1);
    assert!(p.store.list_episodes("budi", &Default::default()).await.unwrap().is_empty());
}

#[tokio::test]
async fn kin_term_resolves_to_same_person_and_confirms_entity() {
    let (p, _d) = setup().await;
    ingest(&p, SourceKind::Interview, "Pakde Karno membuka toko batik di Solo waktu saya masih kecil sekali.").await;
    ingest(&p, SourceKind::Interview, "Suatu sore Pakde datang ke rumah membawa oleh-oleh serabi dari Solo untuk kami sekeluarga.").await;
    let ents = p.store.entities_by_alias("budi", "Karno").await.unwrap();
    assert_eq!(ents.len(), 1);
    let karno = &ents[0];
    assert_eq!(karno.status, EntityStatus::Confirmed, "{karno:?}");
    assert_eq!(karno.episode_ids.len(), 2);
    assert_eq!(karno.gender, Some(memory_core::Gender::Male));
    let kin = p.store.kin_for_subject("budi").await.unwrap();
    assert_eq!(kin.len(), 1);
    assert_eq!(kin[0].relation, memory_core::KinRelation::ParentSibling);
    assert_eq!(kin[0].older, Some(true));
}

#[tokio::test]
async fn fabricated_quotes_are_rejected() {
    let (p, _d) = setup().await;
    let src = ingest(&p, SourceKind::Interview, "Ini cerita karangan tentang perjalanan saya ke luar negeri yang panjang sekali.").await;
    assert_eq!(src.stats.facts_rejected, 1);
    assert_eq!(src.stats.facts, 0);
}

#[tokio::test]
async fn decisions_and_trait_signals_keep_only_supported_quotes() {
    let (p, _d) = setup().await;
    ingest(&p, SourceKind::Interview, "Waktu usaha rugi besar, saya bilang karyawan tetap digaji dulu, baru saya hitung kerugian saya sendiri.").await;
    let eps = p.store.list_episodes("budi", &Default::default()).await.unwrap();
    assert_eq!(eps[0].decisions.len(), 1);
    assert_eq!(eps[0].trait_signals.len(), 1);
    assert!(eps[0].emotions.is_empty());
}

#[tokio::test]
async fn offline_llm_falls_back_and_commits_episode() {
    let (p, _d) = pipeline_with(|_req| Err(memory_llm::LlmError::Other("llm offline".into()))).await;
    p.ensure_subject("budi", Some("Budi")).await.unwrap();
    let src = ingest(
        &p,
        SourceKind::AgentConversation,
        "Untuk project LLM-MEMORY, kita memutuskan memakai RocksDB sebagai storage engine utama dan BAAI/bge-m3 untuk embedding.",
    )
    .await;
    assert_eq!(src.status, SourceStatus::Done);
    assert_eq!(src.stats.episodes, 1);
    let eps = p.store.list_episodes("budi", &Default::default()).await.unwrap();
    assert_eq!(eps.len(), 1);
    assert!(eps[0].text.contains("RocksDB"));
    assert!(eps[0].time_start.is_some(), "AgentConversation tanpa waktu di teks memakai tanggal pencatatan");
    assert_ne!(eps[0].time_label(), "waktu tidak diketahui");
}

#[tokio::test]
async fn compact_rolls_up_episodes_and_compacts_facts_non_destructively() {
    let (p, _d) = setup().await;
    // Dua percakapan agent dari proyek yang sama, ditambah fakta afiliasi politik.
    let s1 = p
        .ingest(IngestDoc {
            subject: "budi".into(),
            kind: SourceKind::AgentConversation,
            title: "Diingat agent: antigravity:VOLTUNES".into(),
            text: "Waktu umur 20 saya pendukung Partai A dan memperbaiki modul tanggal di proyek VOLTUNES.".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    p.process_source(&s1.source.id, None).await.unwrap();

    let s2 = p
        .ingest(IngestDoc {
            subject: "budi".into(),
            kind: SourceKind::AgentConversation,
            title: "Diingat agent: antigravity:VOLTUNES".into(),
            text: "Waktu usaha rugi, karyawan tetap digaji dulu sambil menyelesaikan unit test di proyek VOLTUNES.".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    p.process_source(&s2.source.id, None).await.unwrap();

    let cands = p.compact_candidates("budi").await.unwrap();
    assert_eq!(cands.episode_clusters.len(), 1, "dua episode AgentConversation seproyek masuk kandidat klaster");
    assert_eq!(cands.episode_clusters[0].episodes.len(), 2);

    // Jalankan compact otomatis: 2 episode mikro menjadi 1 episode kanonik aktif + 2 retelling.
    let rep = p.compact("budi").await.unwrap();
    assert_eq!(rep.canonical_episodes_created, 1);
    assert_eq!(rep.episodes_compacted, 2);

    let active_eps = p
        .store
        .list_episodes("budi", &memory_store::EpisodeFilter { status: Some(EpisodeStatus::Active), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(active_eps.len(), 1);
    let canon = &active_eps[0];
    assert_eq!(canon.title, "Rangkuman Kerja Proyek");
    assert!(canon.time_start.is_some());
    assert_ne!(canon.time_label(), "waktu tidak diketahui");
    assert_eq!(canon.decisions.len(), 1, "keputusan dari episode anggota ikut terwarisi ke episode kanonik");
    assert_eq!(canon.trait_signals.len(), 1, "sinyal sifat ikut terwarisi ke episode kanonik");

    let all_eps = p.store.list_episodes("budi", &Default::default()).await.unwrap();
    assert_eq!(all_eps.len(), 3, "episode asli tidak dihapus dari penyimpanan");
    for m in all_eps.iter().filter(|e| e.id != canon.id) {
        assert_eq!(m.status, EpisodeStatus::Retelling);
        assert_eq!(m.retelling_of.as_deref(), Some(canon.id.as_str()));
    }

    // Uji pemadatan fakta (merge duplikat, close fakta lama, reject fakta sesaat).
    let sid = self_id(&p).await;
    let f_keep = Fact {
        id: "f_keep".into(),
        subject: "budi".into(),
        entity_id: sid.clone(),
        predicate: "mengembangkan".into(),
        category: "proyek".into(),
        object_entity: None,
        object_text: "Tabular".into(),
        kind: FactKind::Fact,
        exclusive: false,
        time: memory_core::TimeRef::Unknown,
        valid_from: Some("2026-02-01".into()),
        valid_to: None,
        ongoing: true,
        time_confidence: 1.0,
        observed_at: "2026-02-01".into(),
        confidence: 0.8,
        evidence: vec![Evidence { episode_id: all_eps[0].id.clone(), quote: "Tabular".into() }],
        status: FactStatus::Active,
        superseded_by: None,
        supersedes: None,
        created_at: "2026-02-01T00:00:00Z".into(),
    };
    let f_dup = Fact {
        id: "f_dup".into(),
        valid_from: Some("2026-01-15".into()),
        confidence: 0.95,
        predicate: "memiliki proyek".into(),
        evidence: vec![Evidence { episode_id: all_eps[1].id.clone(), quote: "proyek Tabular".into() }],
        ..f_keep.clone()
    };
    let f_old = Fact {
        id: "f_old".into(),
        predicate: "mengerjakan".into(),
        object_text: "migrasi awal".into(),
        ..f_keep.clone()
    };
    let f_noise = Fact {
        id: "f_noise".into(),
        predicate: "sedang menjalankan".into(),
        object_text: "kompilasi Gradle".into(),
        ..f_keep.clone()
    };
    for f in [&f_keep, &f_dup, &f_old, &f_noise] {
        p.store.put_fact(f).await.unwrap();
    }

    let apply_rep = p
        .compact_apply(
            "budi",
            crate::compact::CompactPlan {
                episode_rollups: vec![],
                fact_merges: vec![crate::llm_types::FactMergeOut {
                    keep_id: "f_keep".into(),
                    absorb_ids: vec!["f_dup".into()],
                    predicate: Some("mengembangkan proyek".into()),
                    object_text: Some("Tabular".into()),
                }],
                fact_closures: vec![crate::llm_types::FactClosureOut {
                    fact_id: "f_old".into(),
                    valid_to: Some("2026-03-01".into()),
                    superseded_by: None,
                }],
                fact_rejections: vec!["f_noise".into()],
                review_decisions: vec![],
            },
        )
        .await
        .unwrap();
    assert_eq!(apply_rep.facts_merged, 1);
    assert_eq!(apply_rep.facts_closed, 1);
    assert_eq!(apply_rep.facts_rejected, 1);

    let kept = p.store.get_fact("f_keep").await.unwrap().unwrap();
    assert_eq!(kept.evidence.len(), 2, "bukti fakta duplikat digabung ke fakta utama");
    assert_eq!(kept.valid_from.as_deref(), Some("2026-01-15"));
    assert_eq!(kept.confidence, 0.95);
    assert_eq!(kept.predicate, "mengembangkan proyek");

    let dup = p.store.get_fact("f_dup").await.unwrap().unwrap();
    assert_eq!(dup.status, FactStatus::Rejected);
    assert_eq!(dup.superseded_by.as_deref(), Some("f_keep"));

    let closed = p.store.get_fact("f_old").await.unwrap().unwrap();
    assert_eq!(closed.status, FactStatus::Active);
    assert_eq!(closed.valid_to.as_deref(), Some("2026-03-01"));
    assert!(!closed.is_current(), "fakta yang ditutup berpindah menjadi riwayat masa lalu");

    let rejected = p.store.get_fact("f_noise").await.unwrap().unwrap();
    assert_eq!(rejected.status, FactStatus::Rejected);
}

#[tokio::test]
async fn technical_entities_never_become_person_merge_reviews_and_auto_sanitize() {
    let (p, _d) = setup().await;
    // Simulasikan entitas dan review salah klasifikasi dari versi lama ("Orang yang sama?").
    let e1 = Entity {
        id: "ent_sensor".into(),
        subject: "budi".into(),
        name: "BACKEND/api-master-sensor".into(),
        aliases: vec![],
        kind: EntityKind::Person, // salah diklasifikasikan sebagai Person di versi lama
        gender: None,
        is_self: false,
        description: String::new(),
        status: EntityStatus::Confirmed,
        merged_into: None,
        mention_count: 2,
        episode_ids: vec![],
        created_at: "2026-01-01T00:00:00Z".into(),
    };
    let e2 = Entity {
        id: "ent_project".into(),
        name: "BACKEND/api-trans-project".into(),
        ..e1.clone()
    };
    let e3 = Entity {
        id: "ent_file".into(),
        name: "total_value_formulation_test.go pada BACKEND/api-data-warehouse".into(),
        ..e1.clone()
    };
    let e4 = Entity {
        id: "ent_dw".into(),
        name: "BACKEND/api-data-warehouse".into(),
        ..e1.clone()
    };
    for e in [&e1, &e2, &e3, &e4] {
        p.store.put_entity(e, None).await.unwrap();
    }
    p.store
        .put_review(&ReviewItem {
            id: "rev_false_1".into(),
            subject: "budi".into(),
            kind: ReviewKind::EntityMerge,
            status: ReviewStatus::Open,
            summary: "Apakah \"BACKEND/api-master-sensor\" sama dengan \"BACKEND/api-trans-project\"? (kemiripan 0.82)".into(),
            targets: vec!["ent_sensor".into(), "ent_project".into()],
            payload: json!({"entity_a": "ent_sensor", "entity_b": "ent_project", "similarity": 0.82}),
            note: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            resolved_at: None,
        })
        .await
        .unwrap();
    p.store
        .put_review(&ReviewItem {
            id: "rev_false_2".into(),
            subject: "budi".into(),
            kind: ReviewKind::EntityMerge,
            status: ReviewStatus::Open,
            summary: "Apakah \"total_value_formulation_test.go pada BACKEND/api-data-warehouse\" sama dengan \"BACKEND/api-data-warehouse\"? (kemiripan 0.79)".into(),
            targets: vec!["ent_file".into(), "ent_dw".into()],
            payload: json!({"entity_a": "ent_file", "entity_b": "ent_dw", "similarity": 0.79}),
            note: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            resolved_at: None,
        })
        .await
        .unwrap();

    let (reclassified, dismissed) = p.sanitize_technical_entities("budi").await.unwrap();
    assert_eq!(reclassified, 4, "keempat entitas teknis dikoreksi dari Person ke Project/Component");
    assert_eq!(dismissed, 2, "kedua review EntityMerge false-positive otomatis ditolak");

    let sensor = p.store.get_entity("ent_sensor").await.unwrap().unwrap();
    assert_eq!(sensor.kind, EntityKind::Project);
    let file_ent = p.store.get_entity("ent_file").await.unwrap().unwrap();
    assert_eq!(file_ent.kind, EntityKind::Component);

    let open_revs = p.store.list_reviews("budi", ReviewStatus::Open, None, 50).await.unwrap();
    assert!(open_revs.is_empty(), "tidak ada review EntityMerge palsu yang tersisa");
}
