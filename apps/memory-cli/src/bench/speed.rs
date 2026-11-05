//! Benchmark: kecepatan tulis episode dan latensi pencarian vektor / BM25
//! pada SurrealDB embedded (RocksDB), untuk keputusan go/no-go skala besar.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use rand::Rng;

use memory_core::MemoryConfig;
use memory_core::model::{Episode, EpisodeStatus};
use memory_core::time::TimeRef;
use memory_store::{Store, VectorIndex};

const WORDS: &[&str] = &[
    "bapak", "ibu", "sawah", "pasar", "toko", "batik", "sekolah", "kerja", "pabrik", "jogja", "solo", "klaten", "merantau", "lebaran",
    "nikah", "anak", "cucu", "masjid", "sepeda", "hujan", "kampung", "kebun", "usaha", "rugi", "untung", "karyawan", "pakde", "mbah",
];

fn rand_vec(rng: &mut impl Rng, dim: usize) -> Vec<f32> {
    let mut v: Vec<f32> = (0..dim).map(|_| rng.random_range(-1.0f32..1.0)).collect();
    memory_llm::l2_normalize(&mut v);
    v
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
}

pub async fn run(cfg: &MemoryConfig, n: usize, queries: usize, index: &str, dir: Option<PathBuf>) -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let path = dir.unwrap_or_else(|| tmp.path().join("bench-db"));
    let mut idx = cfg.index.clone();
    idx.vector = index.to_string();
    let dim = cfg.embedding.dimension;
    let store = Store::open(&path, dim, &idx).await?;
    let mut rng = rand::rng();
    println!("Benchmark: {n} episode, dimensi {dim}, index {index}, path {}", path.display());

    let started = Instant::now();
    let batch = 200;
    let mut done = 0;
    while done < n {
        let mut tx = store.tx();
        for i in done..(done + batch).min(n) {
            let text: Vec<&str> = (0..40).map(|_| WORDS[rng.random_range(0..WORDS.len())]).collect();
            let year = 1950 + (i % 70);
            let e = Episode {
                id: format!("b{i:09}"),
                subject: "bench".into(),
                source_id: "bench".into(),
                text: text.join(" "),
                summary: String::new(),
                title: format!("cerita {i}"),
                time: TimeRef::Unknown,
                time_start: Some(format!("{year}-01-01")),
                time_end: Some(format!("{year}-12-31")),
                time_ongoing: false,
                time_confidence: 0.5,
                importance: rng.random_range(0.0..1.0),
                emotions: vec![],
                languages: vec!["id".into()],
                life_phase: None,
                project: None,
                speaker: None,
                status: EpisodeStatus::Active,
                retelling_of: None,
                retell_count: 0,
                decisions: vec![],
                quotes: vec![],
                trait_signals: vec![],
                entity_ids: vec![],
                obs_type: None,
                concepts: vec![],
                files_read: vec![],
                files_modified: vec![],
                session_id: None,
                prompt_number: None,
                observed_at: "2024-01-01".into(),
                created_at: memory_core::ids::now_rfc3339(),
            };
            let v = rand_vec(&mut rng, dim);
            tx.upsert("episode", &e.id, &e, Some(&v))?;
        }
        tx.commit().await?;
        done = (done + batch).min(n);
        if done % 5000 == 0 || done == n {
            println!("  ditulis {done} ({:.0}/detik)", done as f64 / started.elapsed().as_secs_f64());
        }
    }
    let write_secs = started.elapsed().as_secs_f64();

    let vi = store.vector_index();
    let mut lat = Vec::with_capacity(queries);
    let mut found = 0usize;
    for _ in 0..queries {
        let q = rand_vec(&mut rng, dim);
        let t = Instant::now();
        let hits = vi.search("episode", "bench", &q, 10, "status = 'active'").await?;
        lat.push(t.elapsed().as_secs_f64() * 1000.0);
        found += hits.len();
    }
    lat.sort_by(f64::total_cmp);
    let mut ft = Vec::new();
    for _ in 0..queries.min(100) {
        let q = format!("{} {}", WORDS[rng.random_range(0..WORDS.len())], WORDS[rng.random_range(0..WORDS.len())]);
        let t = Instant::now();
        store.fulltext_episodes("bench", &q, 10).await?;
        ft.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    ft.sort_by(f64::total_cmp);
    let size = dir_size(&path);
    println!("\nHasil:");
    println!("  tulis          : {:.1} dtk total, {:.0} episode/detik", write_secs, n as f64 / write_secs);
    println!("  vektor top-10  : p50 {:.1} ms, p95 {:.1} ms, p99 {:.1} ms (rata-rata {:.1} hasil)", pct(&lat, 0.5), pct(&lat, 0.95), pct(&lat, 0.99), found as f64 / queries.max(1) as f64);
    println!("  BM25 top-10    : p50 {:.1} ms, p95 {:.1} ms", pct(&ft, 0.5), pct(&ft, 0.95));
    println!("  ukuran di disk : {:.1} MB", size as f64 / 1e6);
    Ok(())
}

fn dir_size(p: &std::path::Path) -> u64 {
    std::fs::read_dir(p)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| {
                    let p = e.path();
                    if p.is_dir() { dir_size(&p) } else { e.metadata().map(|m| m.len()).unwrap_or(0) }
                })
                .sum()
        })
        .unwrap_or(0)
}
