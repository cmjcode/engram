//! Lapis A: apakah penyaringan (segmentasi + skor + jalur) meloloskan potongan
//! penting dan membuang noise.
//!
//! Hanya segmentasi yang memanggil LLM; hasilnya di-cache di `<kasus>/.cache/`
//! sehingga skor bisa dihitung ulang gratis (dipakai `bench tune`).
//! Evaluasi memakai store in-memory: vault asli tidak disentuh, dan setiap
//! segmen dinilai seolah vault kosong (novelty 1, tanpa back-ref entitas).

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};

use memory_core::MemoryConfig;
use memory_core::config::ScoreConfig;
use memory_core::model::SourceKind;
use memory_core::score::{Route, ScoreInput, ScoreResult, score};
use memory_core::text::{quote_supported, truncate_chars};
use memory_extract::segment::strip_chat_prefix;
use memory_extract::{Pipeline, Vault, prompts};
use memory_store::Store;

use super::gold::Case;

/// Segmen hasil LLM yang cukup untuk menghitung ulang skor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSeg {
    pub text: String,
    pub topic: String,
    pub about: bool,
    pub significance: Option<f32>,
}

/// Label gold yang sudah dipetakan ke segmen.
#[derive(Debug, Clone)]
pub struct Label {
    pub quote: String,
    pub important: bool,
    pub seg: Option<usize>,
}

pub struct CaseData {
    pub name: String,
    pub kind: SourceKind,
    pub segs: Vec<CachedSeg>,
    pub labels: Vec<Label>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Episode,
    Gray,
    Stream,
    Drop,
    NotAbout,
}

impl Outcome {
    fn label(self) -> &'static str {
        match self {
            Outcome::Episode => "Episode",
            Outcome::Gray => "GrayZone",
            Outcome::Stream => "Stream",
            Outcome::Drop => "Drop",
            Outcome::NotAbout => "bukan-subjek",
        }
    }
}

pub fn judge(seg: &CachedSeg, kind: SourceKind, cfg: &ScoreConfig) -> (Outcome, ScoreResult) {
    let text = strip_chat_prefix(&seg.text);
    let r = score(&ScoreInput { text: &text, kind, novelty: 1.0, back_refs: 0, significance: seg.significance }, cfg);
    let o = if !seg.about {
        Outcome::NotAbout
    } else {
        match r.route {
            Route::Episode => Outcome::Episode,
            Route::GrayZone => Outcome::Gray,
            Route::Stream => Outcome::Stream,
            Route::Drop => Outcome::Drop,
        }
    };
    (o, r)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Metrics {
    pub segments: usize,
    pub episodes: usize,
    pub gray: usize,
    pub important: usize,
    pub important_episode: usize,
    pub important_gray: usize,
    pub noise: usize,
    pub noise_leak: usize,
    pub noise_gray: usize,
    pub unmatched: usize,
}

fn ratio(a: usize, b: usize) -> f32 {
    if b == 0 { 0.0 } else { a as f32 / b as f32 }
}

impl Metrics {
    /// Potongan penting yang langsung jadi episode.
    pub fn recall(&self) -> f32 {
        ratio(self.important_episode, self.important)
    }
    /// Penting yang jadi episode atau setidaknya masuk antrean review.
    pub fn recall_with_review(&self) -> f32 {
        ratio(self.important_episode + self.important_gray, self.important)
    }
    /// Noise yang lolos jadi episode.
    pub fn leak(&self) -> f32 {
        ratio(self.noise_leak, self.noise)
    }
    pub fn add(&mut self, o: &Metrics) {
        self.segments += o.segments;
        self.episodes += o.episodes;
        self.gray += o.gray;
        self.important += o.important;
        self.important_episode += o.important_episode;
        self.important_gray += o.important_gray;
        self.noise += o.noise;
        self.noise_leak += o.noise_leak;
        self.noise_gray += o.noise_gray;
        self.unmatched += o.unmatched;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Miss {
    pub case: String,
    pub expected: &'static str,
    pub got: Outcome,
    pub importance: f32,
    pub significance: Option<f32>,
    pub noise_ratio: f32,
    pub text: String,
}

pub fn measure(c: &CaseData, cfg: &ScoreConfig, misses: Option<&mut Vec<Miss>>) -> Metrics {
    let judged: Vec<(Outcome, ScoreResult)> = c.segs.iter().map(|s| judge(s, c.kind, cfg)).collect();
    let mut m = Metrics { segments: c.segs.len(), ..Default::default() };
    m.episodes = judged.iter().filter(|(o, _)| *o == Outcome::Episode).count();
    m.gray = judged.iter().filter(|(o, _)| *o == Outcome::Gray).count();
    let mut out = Vec::new();
    for l in &c.labels {
        let Some(i) = l.seg else {
            m.unmatched += 1;
            continue;
        };
        let (o, r) = judged[i];
        let wrong = if l.important {
            m.important += 1;
            match o {
                Outcome::Episode => m.important_episode += 1,
                Outcome::Gray => m.important_gray += 1,
                _ => {}
            }
            o != Outcome::Episode
        } else {
            m.noise += 1;
            match o {
                Outcome::Episode => m.noise_leak += 1,
                Outcome::Gray => m.noise_gray += 1,
                _ => {}
            }
            o == Outcome::Episode
        };
        if wrong {
            out.push(Miss {
                case: c.name.clone(),
                expected: if l.important { "penting" } else { "noise" },
                got: o,
                importance: r.importance,
                significance: c.segs[i].significance,
                noise_ratio: r.features.noise_ratio,
                text: truncate_chars(&l.quote.replace('\n', " "), 90),
            });
        }
    }
    if let Some(v) = misses {
        v.extend(out);
    }
    m
}

/// Pipeline minimal di atas store in-memory (hanya LLM yang dipakai).
pub async fn eval_pipeline(cfg: &MemoryConfig) -> Result<(Pipeline, tempfile::TempDir)> {
    let tmp = tempfile::tempdir()?;
    let store = Store::memory(cfg.embedding.dimension, &cfg.index).await?;
    let p = Pipeline::new(
        store,
        memory_llm::llm_from_config(cfg),
        memory_llm::embedder_from_config(cfg),
        Arc::new(cfg.clone()),
        Vault::new(tmp.path()),
    );
    Ok((p, tmp))
}

fn cache_key(cfg: &MemoryConfig, subject: &str, kind: SourceKind, body: &str) -> String {
    let mut h = blake3::Hasher::new();
    for part in [cfg.llm.light_model.as_str(), prompts::SEGMENT_SYSTEM, subject, &format!("{kind:?}"), &cfg.pipeline.max_window_chars.to_string(), body] {
        h.update(part.as_bytes());
        h.update(&[0]);
    }
    h.finalize().to_hex()[..16].to_string()
}

/// Muat kasus: segmentasi via LLM (atau dari cache), lalu petakan label.
pub async fn load_case(p: &Pipeline, case: &Case, refresh: bool) -> Result<CaseData> {
    let cfg = &p.cfg;
    let g = &case.gold;
    let subject = g.subject_name.clone().unwrap_or_else(|| cfg.vault.default_subject.clone());
    let hint = match g.kind.as_deref() {
        Some(k) => Some(SourceKind::parse(k).with_context(|| format!("{}: jenis tidak dikenal: {k}", case.name))?),
        None => None,
    };

    // Teks yang disegmentasi: dokumen hasil adapter (sama seperti `import`) atau potongan berlabel.
    let (kind, docs, seg_mode) = if !g.segment.is_empty() {
        (hint.unwrap_or(SourceKind::Chat), g.segment.iter().map(|s| s.text.clone()).collect::<Vec<_>>(), true)
    } else {
        let path = case.dir.join(g.source.as_deref().unwrap_or("source.txt"));
        let bytes = std::fs::read(&path).with_context(|| format!("membaca {}", path.display()))?;
        let imported = memory_import::parse_bytes(&path, &bytes, hint).with_context(|| format!("mem-parse {}", path.display()))?;
        let kind = hint.or_else(|| imported.first().map(|d| d.kind)).unwrap_or(SourceKind::Note);
        (kind, imported.into_iter().map(|d| d.text).collect(), false)
    };

    let key = cache_key(cfg, &subject, kind, &docs.join("\u{1}"));
    let cache = case.dir.join(".cache").join(format!("seg-{key}.json"));
    let cached = if refresh { None } else { std::fs::read(&cache).ok().and_then(|b| serde_json::from_slice::<Vec<CachedSeg>>(&b).ok()) };
    let segs = match cached {
        Some(v) => v,
        None => {
            eprintln!("  {}: segmentasi {} dokumen via LLM...", case.name, docs.len());
            let per_doc: Vec<Vec<CachedSeg>> = stream::iter(docs.iter().map(|d| {
                let subject = subject.clone();
                async move {
                    let mapped: Vec<CachedSeg> = p
                        .segment(&subject, d, "")
                        .await
                        .into_iter()
                        .map(|s| CachedSeg { text: s.text, topic: s.topic, about: s.about_subject, significance: s.significance })
                        .collect();
                    if seg_mode { vec![merge_one(d, mapped)] } else { mapped }
                }
            }))
            .buffered(4)
            .collect()
            .await;
            let v: Vec<CachedSeg> = per_doc.into_iter().flatten().collect();
            // Segmen tanpa significance berarti LLM gagal dan pipeline memakai heuristik:
            // jangan di-cache, karena hasilnya bukan perilaku sistem yang sebenarnya.
            let fallback = v.iter().filter(|s| s.significance.is_none()).count();
            if fallback == v.len() && !v.is_empty() {
                bail!(
                    "{}: segmentasi LLM gagal untuk semua segmen (lihat WARN di atas). Cek `engram smoke` dan --config (sekarang: light model '{}' di {})",
                    case.name,
                    cfg.llm.light_model,
                    cfg.llm.light_base_url.as_deref().unwrap_or(&cfg.llm.base_url)
                );
            }
            if fallback > 0 {
                eprintln!("  {}: {fallback}/{} segmen memakai heuristik (LLM gagal sebagian); cache tidak disimpan", case.name, v.len());
            } else {
                if let Some(dir) = cache.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&cache, serde_json::to_vec_pretty(&v)?)?;
            }
            v
        }
    };

    let labels = if seg_mode {
        g.segment.iter().enumerate().map(|(i, s)| Label { quote: s.text.clone(), important: s.important, seg: Some(i) }).collect()
    } else {
        let find = |q: &str| segs.iter().position(|s| quote_supported(q, &s.text));
        g.important
            .iter()
            .map(|l| (l, true))
            .chain(g.noise.iter().map(|l| (l, false)))
            .map(|(l, important)| Label { quote: l.quote.clone(), important, seg: find(&l.quote) })
            .collect()
    };
    Ok(CaseData { name: case.name.clone(), kind, segs, labels })
}

/// Potongan berlabel dinilai sebagai satu segmen utuh: pakai significance dari
/// segmen terpanjang yang dikembalikan LLM; tentang subjek bila salah satunya ya.
fn merge_one(text: &str, segs: Vec<CachedSeg>) -> CachedSeg {
    let about = segs.is_empty() || segs.iter().any(|s| s.about);
    let main = segs.into_iter().max_by_key(|s| s.text.chars().count());
    CachedSeg {
        text: text.to_string(),
        topic: main.as_ref().map(|s| s.topic.clone()).unwrap_or_default(),
        about,
        significance: main.and_then(|s| s.significance),
    }
}

pub async fn load_all(cfg: &MemoryConfig, cases: &[Case], refresh: bool) -> Result<Vec<CaseData>> {
    let (p, _tmp) = eval_pipeline(cfg).await?;
    let mut out = Vec::with_capacity(cases.len());
    for c in cases {
        out.push(load_case(&p, c, refresh).await?);
    }
    Ok(out)
}

#[derive(Serialize)]
struct Report<'a> {
    cases: Vec<(String, Metrics)>,
    total: &'a Metrics,
    misses: &'a [Miss],
    unmatched: Vec<(String, String)>,
}

pub fn pct(x: f32) -> String {
    format!("{:>5.1}%", x * 100.0)
}

pub async fn run(cfg: &MemoryConfig, cases: &[Case], refresh: bool, out: Option<&std::path::Path>, json: bool) -> Result<()> {
    let data = load_all(cfg, cases, refresh).await?;
    let mut total = Metrics::default();
    let mut misses = Vec::new();
    let mut rows = Vec::new();
    for c in &data {
        let m = measure(c, &cfg.score, Some(&mut misses));
        total.add(&m);
        rows.push((c.name.clone(), m));
    }
    let unmatched: Vec<(String, String)> = data
        .iter()
        .flat_map(|c| c.labels.iter().filter(|l| l.seg.is_none()).map(|l| (c.name.clone(), truncate_chars(&l.quote, 90))))
        .collect();
    let report = Report { cases: rows, total: &total, misses: &misses, unmatched };
    if let Some(path) = out {
        std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    let s = &cfg.score;
    println!(
        "Lapis A — penyaringan (promote {:.2}, gray {:.2}, bio {:.2}, max_noise {:.2}, min_chars {})\n",
        s.promote_threshold, s.gray_threshold, s.biographic_threshold, s.max_noise_ratio, s.min_chars
    );
    println!("{:<28} {:>6} {:>8} {:>6} | {:>8} {:>9} {:>11} | {:>6} {:>7}", "kasus", "segmen", "episode", "gray", "penting", "recall", "+review", "noise", "bocor");
    let line = |name: &str, m: &Metrics| {
        println!(
            "{:<28} {:>6} {:>8} {:>6} | {:>8} {:>9} {:>11} | {:>6} {:>7}",
            truncate_chars(name, 28),
            m.segments,
            m.episodes,
            m.gray,
            m.important,
            pct(m.recall()),
            pct(m.recall_with_review()),
            m.noise,
            pct(m.leak())
        )
    };
    for (n, m) in &report.cases {
        line(n, m);
    }
    if report.cases.len() > 1 {
        line("TOTAL", &total);
    }
    if total.important + total.noise == 0 {
        println!("\nBelum ada label [[important]]/[[noise]]/[[segment]]; hanya distribusi jalur yang ditampilkan.");
    }
    if !misses.is_empty() {
        println!("\nSalah jalur ({}):", misses.len());
        for x in &misses {
            println!(
                "  [{}] {} → {:<12} imp {:.2} sig {} noise {:.2}  \"{}\"",
                x.case,
                x.expected,
                x.got.label(),
                x.importance,
                x.significance.map(|v| format!("{v:.2}")).unwrap_or_else(|| "-".into()),
                x.noise_ratio,
                x.text
            );
        }
    }
    if !report.unmatched.is_empty() {
        println!("\nKutipan gold tidak ditemukan di segmen mana pun ({}) — cek ejaan:", report.unmatched.len());
        for (c, q) in &report.unmatched {
            println!("  [{c}] \"{q}\"");
        }
    }
    println!("\nrecall = penting yang jadi Episode; +review = termasuk GrayZone; bocor = noise yang jadi Episode.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, sig: f32) -> CachedSeg {
        CachedSeg { text: text.into(), topic: String::new(), about: true, significance: Some(sig) }
    }

    #[test]
    fn measures_recall_and_leak() {
        let important = "Akhirnya aku memutuskan keluar dari kantor Pertamina dan pindah ke Jogja, berat sekali meninggalkan Bapak sendirian di Klaten.";
        let c = CaseData {
            name: "t".into(),
            kind: SourceKind::Chat,
            segs: vec![seg(important, 0.9), seg("oke sip wkwk", 0.0)],
            labels: vec![
                Label { quote: "keluar dari kantor".into(), important: true, seg: Some(0) },
                Label { quote: "oke sip".into(), important: false, seg: Some(1) },
                Label { quote: "tidak ada".into(), important: true, seg: None },
            ],
        };
        let mut misses = Vec::new();
        let m = measure(&c, &ScoreConfig::default(), Some(&mut misses));
        assert_eq!((m.important, m.important_episode, m.noise, m.noise_leak, m.unmatched), (1, 1, 1, 0, 1));
        assert!(misses.is_empty(), "{misses:?}");
        // Ambang mustahil: yang penting jadi salah jalur.
        let strict = ScoreConfig { promote_threshold: 1.1, gray_threshold: 1.1, ..Default::default() };
        let mut misses = Vec::new();
        assert_eq!(measure(&c, &strict, Some(&mut misses)).recall(), 0.0);
        assert_eq!(misses.len(), 1);
    }

    #[test]
    fn not_about_subject_is_filtered() {
        let mut s = seg("Promo besar akhir tahun di Toko Sinar Jaya, diskon semua barang sampai lima puluh persen untuk pelanggan.", 0.9);
        s.about = false;
        assert_eq!(judge(&s, SourceKind::Chat, &ScoreConfig::default()).0, Outcome::NotAbout);
    }
}
