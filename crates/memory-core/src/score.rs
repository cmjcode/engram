//! Skor kepentingan: menentukan apakah segmen menjadi episode, masuk antrean
//! review (zona abu-abu), atau hanya disimpan di working memory (luruh).

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

pub fn score(input: &ScoreInput<'_>, cfg: &ScoreConfig) -> ScoreResult {
    let toks = text::tokens(input.text);
    let n = toks.len().max(1) as f32;
    let noise = text::noise_ratio(&toks);
    let proper = text::proper_noun_count(input.text) as f32;
    let de = (text::decision_signal(&toks) + text::emotion_signal(&toks)) as f32;
    let f = ScoreFeatures {
        significance: input.significance.unwrap_or(0.5).clamp(0.0, 1.0),
        novelty: input.novelty.clamp(0.0, 1.0),
        entity_signal: (proper / 3.0).min(1.0),
        decision_emotion: (de / 2.0).min(1.0),
        back_refs: (input.back_refs as f32 / 3.0).min(1.0),
        length: (n / 60.0).min(1.0),
        noise_ratio: noise,
    };
    let raw = 0.35 * f.significance
        + 0.20 * f.novelty
        + 0.15 * f.entity_signal
        + 0.15 * f.decision_emotion
        + 0.10 * f.back_refs
        + 0.05 * f.length;
    let importance = (raw * (1.0 - noise)).clamp(0.0, 1.0);

    let too_short = input.text.chars().count() < cfg.min_chars;
    let route = if noise >= cfg.max_noise_ratio || (too_short && f.decision_emotion == 0.0 && f.entity_signal == 0.0) {
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
    fn decay_halves_and_back_refs_boost() {
        assert!((decayed_importance(0.8, 30.0, 30.0, 0) - 0.4).abs() < 1e-3);
        assert!(decayed_importance(0.4, 60.0, 30.0, 2) > 0.3);
    }
}
