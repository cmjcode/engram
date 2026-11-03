//! `bench tune`: cari ambang skor terbaik secara offline dari cache segmentasi.
//! Tidak ada panggilan LLM selain segmentasi yang belum ter-cache.
//!
//! Kriteria: di antara kombinasi dengan kebocoran noise <= `max_leak`, pilih
//! recall penting tertinggi; seri dipecah oleh beban review (GrayZone) lebih kecil.

use anyhow::{Result, anyhow, bail};
use serde::Serialize;

use memory_core::MemoryConfig;
use memory_core::config::ScoreConfig;

use super::eval::{CaseData, Metrics, load_all, measure, pct};
use super::gold::Case;

const DEFAULT_GRID: &str = "promote=0.40..0.70:0.05,gray=0.30..0.55:0.05,noise=0.40..0.80:0.10,min_chars=20..60:20";

/// `a..b:step`, `a|b|c`, atau satu nilai.
fn values(spec: &str) -> Result<Vec<f64>> {
    if let Some((range, step)) = spec.split_once(':') {
        let (a, b) = range.split_once("..").ok_or_else(|| anyhow!("rentang harus a..b:step, bukan '{spec}'"))?;
        let (a, b, step): (f64, f64, f64) = (a.trim().parse()?, b.trim().parse()?, step.trim().parse()?);
        if step <= 0.0 || b < a {
            bail!("rentang tidak valid: '{spec}'");
        }
        let n = ((b - a) / step + 1e-9).floor() as usize;
        return Ok((0..=n).map(|i| ((a + step * i as f64) * 1e4).round() / 1e4).collect());
    }
    spec.split('|').map(|v| Ok(v.trim().parse()?)).collect()
}

fn grid(spec: &str, base: &ScoreConfig) -> Result<Vec<ScoreConfig>> {
    let mut promote = vec![base.promote_threshold as f64];
    let mut gray = vec![base.gray_threshold as f64];
    let mut bio = vec![base.biographic_threshold as f64];
    let mut noise = vec![base.max_noise_ratio as f64];
    let mut min_chars = vec![base.min_chars as f64];
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = part.split_once('=').ok_or_else(|| anyhow!("format grid: nama=nilai, bukan '{part}'"))?;
        let vals = values(v)?;
        match k.trim() {
            "promote" => promote = vals,
            "gray" => gray = vals,
            "bio" => bio = vals,
            "noise" => noise = vals,
            "min_chars" => min_chars = vals,
            other => bail!("parameter grid tidak dikenal: {other} (pakai promote, gray, bio, noise, min_chars)"),
        }
    }
    let mut out = Vec::new();
    for &p in &promote {
        for &g in gray.iter().filter(|&&g| g <= p) {
            for &b in &bio {
                for &n in &noise {
                    for &m in &min_chars {
                        out.push(ScoreConfig {
                            promote_threshold: p as f32,
                            gray_threshold: g as f32,
                            biographic_threshold: b as f32,
                            max_noise_ratio: n as f32,
                            min_chars: m as usize,
                            ..base.clone()
                        });
                    }
                }
            }
        }
    }
    Ok(out)
}

#[derive(Serialize)]
struct Row {
    promote: f32,
    gray: f32,
    bio: f32,
    max_noise: f32,
    min_chars: usize,
    recall: f32,
    recall_with_review: f32,
    leak: f32,
    episodes: usize,
    gray_items: usize,
}

fn row(s: &ScoreConfig, m: &Metrics) -> Row {
    Row {
        promote: s.promote_threshold,
        gray: s.gray_threshold,
        bio: s.biographic_threshold,
        max_noise: s.max_noise_ratio,
        min_chars: s.min_chars,
        recall: m.recall(),
        recall_with_review: m.recall_with_review(),
        leak: m.leak(),
        episodes: m.episodes,
        gray_items: m.gray,
    }
}

fn distance(a: &ScoreConfig, b: &ScoreConfig) -> f32 {
    (a.promote_threshold - b.promote_threshold).abs()
        + (a.gray_threshold - b.gray_threshold).abs()
        + (a.biographic_threshold - b.biographic_threshold).abs()
        + (a.max_noise_ratio - b.max_noise_ratio).abs()
        + (a.min_chars as f32 - b.min_chars as f32).abs() / 100.0
}

/// Metrik setara (untuk memutuskan apakah konfigurasi sekarang sudah optimal).
fn same(a: &Metrics, b: &Metrics) -> bool {
    a.recall() == b.recall() && a.recall_with_review() == b.recall_with_review() && a.gray == b.gray && a.leak() == b.leak()
}

fn total(data: &[CaseData], s: &ScoreConfig) -> Metrics {
    let mut t = Metrics::default();
    for c in data {
        t.add(&measure(c, s, None));
    }
    t
}

pub async fn run(cfg: &MemoryConfig, cases: &[Case], spec: Option<&str>, max_leak: f32, top: usize, json: bool) -> Result<()> {
    let data = load_all(cfg, cases, false).await?;
    let base = total(&data, &cfg.score);
    if base.important + base.noise == 0 {
        bail!("gold set belum punya label; tambahkan [[important]]/[[noise]] atau jalankan `bench export-gold`");
    }
    let combos = grid(spec.unwrap_or(DEFAULT_GRID), &cfg.score)?;
    let n = combos.len();
    let mut scored: Vec<(ScoreConfig, Metrics)> = combos
        .into_iter()
        .map(|s| {
            let m = total(&data, &s);
            (s, m)
        })
        .filter(|(_, m)| m.leak() <= max_leak)
        .collect();
    // Seri dipecah oleh kedekatan ke konfigurasi sekarang: jangan usulkan perubahan tanpa manfaat.
    scored.sort_by(|(sa, a), (sb, b)| {
        b.recall()
            .total_cmp(&a.recall())
            .then(b.recall_with_review().total_cmp(&a.recall_with_review()))
            .then(a.gray.cmp(&b.gray))
            .then(a.leak().total_cmp(&b.leak()))
            .then(distance(sa, &cfg.score).total_cmp(&distance(sb, &cfg.score)))
    });

    if json {
        let rows: Vec<Row> = scored.iter().take(top).map(|(s, m)| row(s, m)).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({"current": row(&cfg.score, &base), "combos": n, "best": rows}))?);
        return Ok(());
    }

    println!("Tune lapis A: {n} kombinasi, {} penting + {} noise berlabel, batas bocor {}\n", base.important, base.noise, pct(max_leak));
    println!(
        "{:<9} {:>8} {:>6} {:>6} {:>10} {:>9} | {:>8} {:>9} {:>7} {:>8} {:>6}",
        "", "promote", "gray", "bio", "max_noise", "min_chars", "recall", "+review", "bocor", "episode", "gray#"
    );
    let print = |label: &str, s: &ScoreConfig, m: &Metrics| {
        println!(
            "{:<9} {:>8.2} {:>6.2} {:>6.2} {:>10.2} {:>9} | {:>8} {:>9} {:>7} {:>8} {:>6}",
            label,
            s.promote_threshold,
            s.gray_threshold,
            s.biographic_threshold,
            s.max_noise_ratio,
            s.min_chars,
            pct(m.recall()),
            pct(m.recall_with_review()),
            pct(m.leak()),
            m.episodes,
            m.gray
        )
    };
    print("sekarang", &cfg.score, &base);
    let Some((best, best_m)) = scored.first() else {
        println!("\nTidak ada kombinasi dengan bocor <= {}; longgarkan --max-leak.", pct(max_leak));
        return Ok(());
    };
    for (i, (s, m)) in scored.iter().take(top).enumerate() {
        print(&format!("#{}", i + 1), s, m);
    }
    if base.leak() <= max_leak && same(&base, best_m) {
        println!("\nKonfigurasi sekarang sudah setara dengan yang terbaik untuk gold set ini; tidak perlu diubah.");
        return Ok(());
    }
    println!(
        "\nUsulan untuk memory.toml (validasi dulu dengan `bench eval` + cek manual):\n[score]\npromote_threshold = {:.2}\ngray_threshold = {:.2}\nbiographic_threshold = {:.2}\nmax_noise_ratio = {:.2}\nmin_chars = {}",
        best.promote_threshold, best.gray_threshold, best.biographic_threshold, best.max_noise_ratio, best.min_chars
    );
    let labeled = base.important + base.noise;
    if labeled < 40 {
        println!("\nCatatan: label masih sedikit ({labeled}); hasil tuning mudah overfit.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ranges_and_lists() {
        assert_eq!(values("0.4..0.5:0.05").unwrap(), vec![0.4, 0.45, 0.5]);
        assert_eq!(values("20|40").unwrap(), vec![20.0, 40.0]);
        assert!(values("0.5..0.4:0.1").is_err());
    }

    #[test]
    fn grid_skips_gray_above_promote() {
        let g = grid("promote=0.4|0.5,gray=0.45", &ScoreConfig::default()).unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].promote_threshold, 0.5);
    }
}
