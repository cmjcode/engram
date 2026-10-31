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
