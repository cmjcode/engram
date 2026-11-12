//! Skor kepentingan: menentukan apakah segmen menjadi episode, masuk antrean
//! review (zona abu-abu), atau hanya disimpan di working memory (luruh).
//!
//! Kategori `business_fact`: kalimat operasional bisnis yang pendek dan datar
//! ("Outlet Bandung tutup sejak September 2026", "Target omzet Q4 naik 15%")
//! tidak punya sinyal emosi/keputusan sehingga skornya rendah. Bila sinyal
//! bisnisnya kuat ([`is_business_fact`]), kepentingannya dinaikkan ke ambang
//! promosi tanpa menurunkan ambang global; basa-basi tetap dibuang.

use serde::{Deserialize, Serialize};

use crate::config::ScoreConfig;
use crate::model::SourceKind;
use crate::text;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    Episode,
    GrayZone,
    Stream,
    /// Noise murni: tidak disimpan sebagai item apa pun (sumber tetap ada).
    Drop,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct ScoreFeatures {
    pub significance: f32,
    pub novelty: f32,
    pub entity_signal: f32,
    pub decision_emotion: f32,
    pub back_refs: f32,
    pub length: f32,
    pub noise_ratio: f32,
    /// Sinyal fakta bisnis 0..1; 1.0 = kategori `business_fact` (lihat [`is_business_fact`]).
    #[serde(default)]
    pub business_fact: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScoreResult {
    pub importance: f32,
    pub route: Route,
    pub features: ScoreFeatures,
}

pub struct ScoreInput<'a> {
    pub text: &'a str,
    pub kind: SourceKind,
    /// 1 - kemiripan maksimum dengan memori yang sudah ada (0..1).
    pub novelty: f32,
    /// Berapa entitas yang sudah dikenal disebut di teks.
    pub back_refs: u32,
    /// Penilaian LLM ringan (0..1), bila ada.
    pub significance: Option<f32>,
}

/// Kategori skor `business_fact`.
pub const BUSINESS_FACT: &str = "business_fact";

/// Sinyal fakta bisnis 0..1. Bernilai 1.0 hanya bila ada istilah bisnis DAN jangkar
/// konkret: angka (jumlah/harga/persen/tahun) atau nama diri (outlet, orang, pemasok).
/// Tanpa jangkar (mis. "rapat mingguan membahas jadwal") sinyal maksimal 0.5, sehingga
/// rutinitas generik tidak ikut dipromosikan.
pub fn business_fact_signal(toks: &[String], proper_nouns: usize) -> f32 {
    let hits = text::business_signal(toks);
    if hits == 0 {
        return 0.0;
    }
    if text::has_number(toks) || proper_nouns > 0 { 1.0 } else { (hits as f32 / 4.0).min(0.5) }
}

/// Apakah teks termasuk kategori `business_fact`: sinyal bisnis penuh dan bukan
/// didominasi noise.
pub fn is_business_fact(text: &str, cfg: &ScoreConfig) -> bool {
    let toks = text::tokens(text);
    business_fact_signal(&toks, text::proper_noun_count(text)) >= 1.0 && text::noise_ratio(&toks) < cfg.max_noise_ratio
}

pub fn score(input: &ScoreInput<'_>, cfg: &ScoreConfig) -> ScoreResult {
    let toks = text::tokens(input.text);
    let n = toks.len().max(1) as f32;
    let noise = text::noise_ratio(&toks);
    let proper = text::proper_noun_count(input.text) as f32;
    let de = (text::decision_signal(&toks) + text::emotion_signal(&toks)) as f32;
    let business = business_fact_signal(&toks, proper as usize);
    let f = ScoreFeatures {
        significance: input.significance.unwrap_or(0.5).clamp(0.0, 1.0),
        novelty: input.novelty.clamp(0.0, 1.0),
        entity_signal: (proper / 3.0).min(1.0),
        decision_emotion: (de / 2.0).min(1.0),
        back_refs: (input.back_refs as f32 / 3.0).min(1.0),
        length: (n / 60.0).min(1.0),
        noise_ratio: noise,
        business_fact: business,
    };
    let raw = 0.35 * f.significance
        + 0.20 * f.novelty
        + 0.15 * f.entity_signal
        + 0.15 * f.decision_emotion
        + 0.10 * f.back_refs
        + 0.05 * f.length;
    let mut importance = (raw * (1.0 - noise)).clamp(0.0, 1.0);
    // Kategori business_fact: fakta operasional datar (tanpa emosi/keputusan) tetap
    // dipromosikan; ambang global tidak diturunkan, hanya skor kalimat ini yang dinaikkan.
    let business_fact = f.business_fact >= 1.0 && noise < cfg.max_noise_ratio;
    if business_fact {
        importance = importance.max(cfg.promote_threshold);
    }

    let too_short = input.text.chars().count() < cfg.min_chars;
    let route = if noise >= cfg.max_noise_ratio || (too_short && f.decision_emotion == 0.0 && f.entity_signal == 0.0 && !business_fact) {
        Route::Drop
    } else if input.kind.is_biographic() {
        if importance >= cfg.biographic_threshold { Route::Episode } else { Route::Stream }
    } else if importance >= cfg.promote_threshold {
        Route::Episode
    } else if importance >= cfg.gray_threshold {
        Route::GrayZone
    } else {
        Route::Stream
    };
    ScoreResult { importance, route, features: f }
}

/// Kepentingan setelah peluruhan waktu, ditambah dorongan dari referensi balik
/// (item yang disebut lagi kemudian hari menjadi lebih penting).
pub fn decayed_importance(base: f32, age_days: f64, half_life_days: f64, back_refs: u32) -> f32 {
    let decay = 0.5f64.powf(age_days.max(0.0) / half_life_days.max(1.0)) as f32;
    (base * decay + 0.15 * back_refs as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ScoreConfig {
        ScoreConfig::default()
    }

    #[test]
    fn chat_noise_is_dropped() {
        let r = score(&ScoreInput { text: "oke sip wkwk", kind: SourceKind::Chat, novelty: 0.9, back_refs: 0, significance: None }, &cfg());
        assert_eq!(r.route, Route::Drop);
    }

    #[test]
    fn meaningful_chat_becomes_episode() {
        let t = "Akhirnya aku memutuskan keluar dari kantor Pertamina dan pindah ke Jogja, berat sekali rasanya meninggalkan Bapak sendirian di Klaten.";
        let r = score(&ScoreInput { text: t, kind: SourceKind::Chat, novelty: 0.9, back_refs: 1, significance: Some(0.9) }, &cfg());
        assert_eq!(r.route, Route::Episode, "{r:?}");
    }

    #[test]
    fn routine_lifelog_goes_to_stream() {
        let t = "rapat mingguan seperti biasa, membahas jadwal minggu depan dan pembagian tugas tim yang sama seperti kemarin";
        let r = score(&ScoreInput { text: t, kind: SourceKind::Lifelog, novelty: 0.1, back_refs: 0, significance: Some(0.1) }, &cfg());
        assert_eq!(r.route, Route::Stream, "{r:?}");
    }

    #[test]
    fn interview_threshold_is_lower() {
        let t = "Rumah kami dulu di dekat sawah, setiap pagi saya membantu ibu menimba air sebelum berangkat sekolah.";
        let r = score(&ScoreInput { text: t, kind: SourceKind::Interview, novelty: 0.5, back_refs: 0, significance: Some(0.4) }, &cfg());
        assert_eq!(r.route, Route::Episode, "{r:?}");
    }

    #[test]
    fn business_facts_are_promoted_but_small_talk_is_still_dropped() {
        let cfg = cfg();
        let business = [
            "Outlet Bandung tutup sejak September 2026",
            "Pak Budi lebih suka laporan mingguan tiap Senin dalam Rupiah",
            "Target omzet Q4 naik 15%",
            "Harga jual kopi susu naik jadi Rp 18.000 mulai bulan depan",
            "Supplier tepung utama sekarang PT Sumber Pangan, pembayaran tempo 30 hari",
            "Stok gudang Surabaya dicek setiap Jumat sore oleh Mbak Rina",
        ];
        for t in business {
            assert!(is_business_fact(t, &cfg), "{t}: harus terdeteksi business_fact");
            let r = score(&ScoreInput { text: t, kind: SourceKind::Chat, novelty: 0.8, back_refs: 0, significance: None }, &cfg);
            assert!(r.importance >= cfg.promote_threshold, "{t}: importance {} < promote", r.importance);
            assert_eq!(r.route, Route::Episode, "{t}: {r:?}");
            assert!((r.features.business_fact - 1.0).abs() < f32::EPSILON, "{t}");
        }
        for t in ["ok makasih", "siap pak", "halo"] {
            assert!(!is_business_fact(t, &cfg), "{t}");
            let r = score(&ScoreInput { text: t, kind: SourceKind::Chat, novelty: 0.8, back_refs: 0, significance: None }, &cfg);
            assert_eq!(r.route, Route::Drop, "{t}: {r:?}");
        }
        // Rutinitas generik tanpa jangkar konkret (angka/nama diri) bukan business_fact.
        assert!(!is_business_fact("rapat mingguan seperti biasa, membahas jadwal minggu depan dan pembagian tugas tim", &cfg));
        // Ambang global tidak berubah.
        assert!((cfg.promote_threshold - 0.55).abs() < f32::EPSILON && (cfg.gray_threshold - 0.42).abs() < f32::EPSILON);
    }

    #[test]
    fn decay_halves_and_back_refs_boost() {
        assert!((decayed_importance(0.8, 30.0, 30.0, 0) - 0.4).abs() < 1e-3);
        assert!(decayed_importance(0.4, 60.0, 30.0, 2) > 0.3);
    }
}
