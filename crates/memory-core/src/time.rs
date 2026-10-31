//! Waktu kabur (`TimeRef`) dan resolusinya menjadi rentang tanggal.
//!
//! Orang bercerita dengan waktu yang tidak presisi: "waktu SD", "umur 20-an",
//! "setahun setelah nikah", "sekitar 2015", "sekarang". Semua bentuk itu
//! disimpan apa adanya (`TimeRef`) lalu diresolusikan ke rentang tanggal
//! (`ResolvedTime`) memakai jangkar hidup subjek (`LifeAnchors`). Bentuk asli
//! tidak dibuang, sehingga resolusi bisa diulang saat jangkar diperbaiki.

use std::collections::BTreeMap;

use chrono::{Datelike, Duration, NaiveDate};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::text::normalize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[derive(Default)]
pub enum TimeRef {
    /// Tanggal pasti.
    Exact { date: NaiveDate },
    /// "sekitar 2015": titik tengah plus toleransi.
    Approx { around: NaiveDate, tolerance_days: i64 },
    /// Rentang eksplisit. `ongoing` = masih berlangsung sampai sekarang.
    Range { start: Option<NaiveDate>, end: Option<NaiveDate>, ongoing: bool },
    /// "umur 20" atau "umur 20 sampai 25".
    Age { from: u32, to: Option<u32> },
    /// "setahun setelah nikah": jangkar peristiwa plus offset.
    Relative { anchor: String, offset_years: f64, span_years: Option<f64> },
    /// "waktu SD", "masa merantau".
    LifePhase { phase: String },
    /// "sekarang": benar pada saat diceritakan, awal tidak diketahui.
    Current,
    /// "dulu", "pernah", "mbiyen", "baheula": sebelum saat diceritakan.
    Past,
    #[default]
    Unknown,
}


/// Rentang tanggal hasil resolusi.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct ResolvedTime {
    pub start: Option<NaiveDate>,
    pub end: Option<NaiveDate>,
    /// Masih berlaku sampai sekarang (end terbuka).
    pub ongoing: bool,
    /// 0..1, seberapa yakin rentang ini benar.
    pub confidence: f32,
}

impl ResolvedTime {
    pub fn unknown() -> Self {
        Self::default()
    }

    pub fn is_known(&self) -> bool {
        self.start.is_some() || self.end.is_some()
    }

    /// Apakah rentang ini memotong jendela `[from, to]`. Batas yang tidak
    /// diketahui dianggap terbuka (tidak mengeluarkan hasil).
    pub fn intersects(&self, from: Option<NaiveDate>, to: Option<NaiveDate>) -> bool {
        if !self.is_known() {
            return false;
        }
        let starts_before_window_ends = match (self.start, to) {
            (Some(s), Some(t)) => s <= t,
            _ => true,
        };
        let ends_after_window_starts = match (self.end, from) {
            (Some(e), Some(f)) => e >= f || self.ongoing,
            (None, _) => true,
            (Some(_), None) => true,
        };
        starts_before_window_ends && ends_after_window_starts
    }

    /// Titik tengah, untuk pengurutan timeline.
    pub fn sort_key(&self) -> Option<NaiveDate> {
        match (self.start, self.end) {
            (Some(s), Some(e)) => Some(s + (e - s) / 2),
            (Some(s), None) => Some(s),
            (None, Some(e)) => Some(e),
            _ => None,
        }
    }
}

/// Jangkar hidup subjek untuk meresolusi waktu relatif.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LifeAnchors {
    pub birth: Option<NaiveDate>,
    /// Peristiwa bernama: "nikah" -> 2005-06-01, "pindah jogja" -> 2010-01-01.
    #[serde(default)]
    pub events: BTreeMap<String, NaiveDate>,
    /// Fase hidup khusus subjek dalam umur: "merantau" -> (19, 27).
    #[serde(default)]
    pub phases: BTreeMap<String, (u32, u32)>,
}

impl LifeAnchors {
    pub fn event(&self, name: &str) -> Option<NaiveDate> {
        let key = normalize(name);
        if matches!(key.as_str(), "lahir" | "kelahiran" | "birth" | "born") {
            return self.birth;
        }
        self.events
            .iter()
            .find(|(k, _)| normalize(k) == key)
            .map(|(_, d)| *d)
            .or_else(|| {
                // Cocokkan sebagian: "menikah" vs "nikah".
                self.events
                    .iter()
                    .find(|(k, _)| {
                        let k = normalize(k);
                        k.contains(&key) || key.contains(&k)
                    })
                    .map(|(_, d)| *d)
            })
    }

    pub fn phase_ages(&self, phase: &str) -> Option<(u32, u32)> {
        let key = normalize(phase);
        self.phases
            .iter()
            .find(|(k, _)| normalize(k) == key)
            .map(|(_, v)| *v)
            .or_else(|| default_phase_ages(&key))
    }

    /// Umur subjek pada tanggal tertentu.
    pub fn age_at(&self, date: NaiveDate) -> Option<u32> {
        let b = self.birth?;
        let mut age = date.year() - b.year();
        if (date.month(), date.day()) < (b.month(), b.day()) {
            age -= 1;
        }
        u32::try_from(age).ok()
    }
}

/// Fase hidup umum dalam konteks Indonesia, dalam umur (inklusif).
pub fn default_phase_ages(phase: &str) -> Option<(u32, u32)> {
    let p = normalize(phase);
    let has = |words: &[&str]| words.iter().any(|w| p.split_whitespace().any(|t| t == *w) || p == *w);
    let r = if has(&["balita", "bayi"]) {
        (0, 4)
    } else if has(&["tk", "paud"]) {
        (4, 6)
    } else if has(&["sd", "sekolah dasar", "sr"]) || p.contains("sekolah dasar") {
        (6, 12)
    } else if has(&["smp", "sltp", "mts", "tsanawiyah"]) {
        (12, 15)
    } else if has(&["sma", "smk", "stm", "slta", "ma", "aliyah"]) {
        (15, 18)
    } else if has(&["kuliah", "mahasiswa", "kampus", "s1", "sarjana"]) {
        (18, 23)
    } else if has(&["kecil", "anak", "bocah", "cilik"]) {
        (0, 12)
    } else if has(&["remaja", "abg"]) {
        (12, 18)
    } else if has(&["muda", "bujang", "lajang"]) {
        (18, 30)
    } else if has(&["pensiun", "tua", "sepuh"]) {
        (56, 90)
    } else {
        return None;
    };
    Some(r)
}

/// Fase hidup standar (nama, umur awal, umur akhir) untuk pengelompokan
/// timeline dan deteksi celah wawancara.
pub const STANDARD_PHASES: &[(&str, u32, u32)] = &[
    ("masa kecil", 0, 11),
    ("remaja", 12, 17),
    ("dewasa muda", 18, 24),
    ("dewasa", 25, 39),
    ("paruh baya", 40, 55),
    ("usia lanjut", 56, 120),
];

pub fn standard_phase_for_age(age: u32) -> &'static str {
    STANDARD_PHASES
        .iter()
        .find(|(_, a, b)| age >= *a && age <= *b)
        .map(|(n, _, _)| *n)
        .unwrap_or("usia lanjut")
}

fn add_years(date: NaiveDate, years: f64) -> NaiveDate {
    let days = (years * 365.25).round() as i64;
    date.checked_add_signed(Duration::days(days)).unwrap_or(date)
}

fn age_range(birth: NaiveDate, from: u32, to: u32) -> (NaiveDate, NaiveDate) {
    let start = add_years(birth, f64::from(from));
    let end = add_years(birth, f64::from(to) + 1.0) - Duration::days(1);
    (start, end)
}

impl TimeRef {
    /// Resolusikan ke rentang tanggal. `observed` = tanggal cerita direkam,
    /// dipakai untuk `Current`.
    pub fn resolve(&self, anchors: &LifeAnchors, observed: NaiveDate) -> ResolvedTime {
        match self {
            TimeRef::Exact { date } => ResolvedTime { start: Some(*date), end: Some(*date), ongoing: false, confidence: 1.0 },
            TimeRef::Approx { around, tolerance_days } => {
                let tol = Duration::days((*tolerance_days).max(0));
                ResolvedTime { start: Some(*around - tol), end: Some(*around + tol), ongoing: false, confidence: 0.7 }
            }
            TimeRef::Range { start, end, ongoing } => {
                let confidence = if start.is_some() && (end.is_some() || *ongoing) { 0.9 } else { 0.6 };
                ResolvedTime { start: *start, end: *end, ongoing: *ongoing, confidence }
            }
            TimeRef::Age { from, to } => match anchors.birth {
                Some(b) => {
                    let (s, e) = age_range(b, *from, to.unwrap_or(*from).max(*from));
                    ResolvedTime { start: Some(s), end: Some(e), ongoing: false, confidence: 0.8 }
                }
                None => ResolvedTime::unknown(),
            },
            TimeRef::Relative { anchor, offset_years, span_years } => match anchors.event(anchor) {
                Some(a) => {
                    let s = add_years(a, *offset_years);
                    let e = add_years(s, span_years.unwrap_or(1.0)) - Duration::days(1);
                    ResolvedTime { start: Some(s), end: Some(e), ongoing: false, confidence: 0.6 }
                }
                None => ResolvedTime::unknown(),
            },
            TimeRef::LifePhase { phase } => match (anchors.birth, anchors.phase_ages(phase)) {
                (Some(b), Some((from, to))) => {
                    let (s, e) = age_range(b, from, to);
                    ResolvedTime { start: Some(s), end: Some(e), ongoing: false, confidence: 0.5 }
                }
                _ => match anchors.event(phase) {
                    Some(d) => ResolvedTime { start: Some(d), end: Some(add_years(d, 1.0)), ongoing: false, confidence: 0.4 },
                    None => ResolvedTime::unknown(),
                },
            },
            TimeRef::Current => ResolvedTime { start: Some(observed), end: None, ongoing: true, confidence: 0.5 },
            TimeRef::Past => ResolvedTime { start: None, end: Some(observed - Duration::days(1)), ongoing: false, confidence: 0.3 },
            TimeRef::Unknown => ResolvedTime::unknown(),
        }
    }

    /// Label manusiawi dalam bahasa Indonesia.
    pub fn label(&self) -> String {
        match self {
            TimeRef::Exact { date } => date.format("%d-%m-%Y").to_string(),
            TimeRef::Approx { around, .. } => format!("sekitar {}", around.year()),
            TimeRef::Range { start, end, ongoing } => {
                let s = start.map(|d| d.year().to_string()).unwrap_or_else(|| "?".into());
                let e = match (end, ongoing) {
                    (_, true) => "sekarang".to_string(),
                    (Some(d), _) => d.year().to_string(),
                    (None, _) => "?".into(),
                };
                format!("{s}–{e}")
            }
            TimeRef::Age { from, to: Some(t) } if t != from => format!("umur {from}–{t}"),
            TimeRef::Age { from, .. } => format!("umur {from}"),
            TimeRef::Relative { anchor, offset_years, .. } => {
                if *offset_years >= 0.0 {
                    format!("{offset_years} tahun setelah {anchor}")
                } else {
                    format!("{} tahun sebelum {anchor}", -offset_years)
                }
            }
            TimeRef::LifePhase { phase } => format!("masa {phase}"),
            TimeRef::Current => "sekarang".into(),
            TimeRef::Past => "dulu".into(),
            TimeRef::Unknown => "waktu tidak diketahui".into(),
        }
    }
}

/// Waktu mentah yang dikeluarkan LLM saat ekstraksi. Dibuat longgar agar
/// model lokal mudah mengisinya; konversi ke `TimeRef` dilakukan di sini.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RawTime {
    /// exact | approx | range | age | relative | life_phase | current | unknown
    #[serde(default)]
    pub kind: String,
    /// Kutipan ungkapan waktu apa adanya, mis. "waktu kelas 3 SD".
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub month: Option<u32>,
    #[serde(default)]
    pub day: Option<u32>,
    #[serde(default)]
    pub year_end: Option<i32>,
    #[serde(default)]
    pub age: Option<u32>,
    #[serde(default)]
    pub age_end: Option<u32>,
    #[serde(default)]
    pub anchor: Option<String>,
    #[serde(default)]
    pub offset_years: Option<f64>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub ongoing: Option<bool>,
}

fn ymd(y: i32, m: u32, d: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(y, m, d)
}

fn year_bounds(y: i32) -> (Option<NaiveDate>, Option<NaiveDate>) {
    (ymd(y, 1, 1), ymd(y, 12, 31))
}

impl RawTime {
    pub fn to_time_ref(&self) -> TimeRef {
        let kind = self.kind.trim().to_lowercase();
        let ongoing = self.ongoing.unwrap_or(false);
        let from_year = |approx: bool| -> Option<TimeRef> {
            let y = self.year?;
            if approx {
                return ymd(y, 7, 1).map(|around| TimeRef::Approx { around, tolerance_days: 365 });
            }
            match (self.month, self.day) {
                (Some(m), Some(d)) => ymd(y, m, d).map(|date| TimeRef::Exact { date }),
                (Some(m), None) => {
                    let start = ymd(y, m, 1)?;
                    let next = if m == 12 { ymd(y + 1, 1, 1)? } else { ymd(y, m + 1, 1)? };
                    Some(TimeRef::Range { start: Some(start), end: Some(next - Duration::days(1)), ongoing: false })
                }
                _ => {
                    let (s, e) = year_bounds(y);
                    Some(TimeRef::Range { start: s, end: e, ongoing: false })
                }
            }
        };
        let parsed = match kind.as_str() {
            "exact" => from_year(false),
            "approx" | "approximate" => from_year(true),
            "range" => {
                let start = self.year.and_then(|y| ymd(y, 1, 1));
                let end = self.year_end.and_then(|y| ymd(y, 12, 31));
                if start.is_some() || end.is_some() || ongoing {
                    Some(TimeRef::Range { start, end, ongoing })
                } else {
                    None
                }
            }
            "age" => self.age.map(|from| TimeRef::Age { from, to: self.age_end }),
            "relative" => self.anchor.as_ref().map(|a| TimeRef::Relative {
                anchor: a.clone(),
                offset_years: self.offset_years.unwrap_or(0.0),
                span_years: None,
            }),
            "life_phase" | "phase" => self.phase.as_ref().map(|p| TimeRef::LifePhase { phase: p.clone() }),
            "current" | "now" | "sekarang" => Some(TimeRef::Current),
            "past" | "dulu" => Some(TimeRef::Past),
            _ => None,
        };
        parsed
            .or_else(|| self.age.map(|from| TimeRef::Age { from, to: self.age_end }))
            .or_else(|| from_year(false))
            .or_else(|| self.phase.as_ref().map(|p| TimeRef::LifePhase { phase: p.clone() }))
            .or_else(|| self.text.as_deref().and_then(parse_time_hint))
            .unwrap_or(TimeRef::Unknown)
    }
}

/// Tebakan sederhana dari teks bila LLM tidak mengisi struktur:
/// "umur 20", "usia 40-an", "tahun 1998", "sekarang", "waktu SD".
pub fn parse_time_hint(text: &str) -> Option<TimeRef> {
    let t = normalize(text);
    let tokens: Vec<&str> = t.split_whitespace().collect();
    for (i, tok) in tokens.iter().enumerate() {
        if matches!(*tok, "umur" | "usia" | "umurku" | "berumur")
            && let Some(n) = tokens.get(i + 1).and_then(|n| n.trim_end_matches("an").parse::<u32>().ok()) {
                let decade = tokens[i + 1].ends_with("an") || tokens.get(i + 2) == Some(&"an");
                return Some(TimeRef::Age { from: n, to: decade.then_some(n + 9) });
            }
    }
    for tok in &tokens {
        if tok.len() == 4
            && let Ok(y) = tok.parse::<i32>()
                && (1900..=2100).contains(&y) {
                    let (s, e) = year_bounds(y);
                    return Some(TimeRef::Range { start: s, end: e, ongoing: false });
                }
    }
    if tokens.iter().any(|w| matches!(*w, "sekarang" | "kini" | "skrg" | "saiki" | "ayeuna")) || t.contains("saat ini") {
        return Some(TimeRef::Current);
    }
    if tokens.iter().any(|w| matches!(*w, "dulu" | "dahulu" | "dulunya" | "pernah" | "mbiyen" | "biyen" | "baheula" | "bareto" | "jaman" | "zaman")) {
        return Some(TimeRef::Past);
    }
    if default_phase_ages(&t).is_some() {
        let phase = tokens
            .iter()
            .find(|w| default_phase_ages(w).is_some())
            .map(|w| (*w).to_string())
            .unwrap_or(t.clone());
        return Some(TimeRef::LifePhase { phase });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn anchors() -> LifeAnchors {
        let mut a = LifeAnchors { birth: Some(d(1979, 3, 10)), ..Default::default() };
        a.events.insert("nikah".into(), d(2005, 6, 1));
        a.phases.insert("merantau".into(), (19, 27));
        a
    }

    #[test]
    fn age_resolves_against_birth() {
        let r = TimeRef::Age { from: 20, to: None }.resolve(&anchors(), d(2024, 1, 1));
        assert_eq!(r.start.unwrap().year(), 1999);
        assert_eq!(r.end.unwrap().year(), 2000);
        assert!(r.confidence > 0.7);
    }

    #[test]
    fn life_phase_uses_indonesian_school_ages() {
        let r = TimeRef::LifePhase { phase: "waktu SD".into() }.resolve(&anchors(), d(2024, 1, 1));
        // "waktu sd" tidak persis "sd" -> default_phase_ages mencari token.
        assert_eq!(r.start.unwrap().year(), 1985);
        let r = TimeRef::LifePhase { phase: "merantau".into() }.resolve(&anchors(), d(2024, 1, 1));
        assert_eq!(r.start.unwrap().year(), 1998);
    }

    #[test]
    fn relative_to_named_event() {
        let r = TimeRef::Relative { anchor: "menikah".into(), offset_years: 1.0, span_years: None }
            .resolve(&anchors(), d(2024, 1, 1));
        assert_eq!(r.start.unwrap().year(), 2006);
    }

    #[test]
    fn current_is_open_ended_from_observation() {
        let r = TimeRef::Current.resolve(&anchors(), d(2024, 5, 1));
        assert_eq!(r.start, Some(d(2024, 5, 1)));
        assert!(r.ongoing);
        assert!(r.intersects(Some(d(2030, 1, 1)), Some(d(2030, 12, 31))));
    }

    #[test]
    fn unknown_without_birth_is_not_guessed() {
        let r = TimeRef::Age { from: 20, to: None }.resolve(&LifeAnchors::default(), d(2024, 1, 1));
        assert!(!r.is_known());
    }

    #[test]
    fn raw_time_conversions() {
        let raw = RawTime { kind: "approx".into(), year: Some(2015), ..Default::default() };
        assert!(matches!(raw.to_time_ref(), TimeRef::Approx { .. }));
        let raw = RawTime { kind: "garbage".into(), text: Some("waktu umur 40-an".into()), ..Default::default() };
        assert_eq!(raw.to_time_ref(), TimeRef::Age { from: 40, to: Some(49) });
        let raw = RawTime { kind: "".into(), text: Some("sekarang".into()), ..Default::default() };
        assert_eq!(raw.to_time_ref(), TimeRef::Current);
        let raw = RawTime { kind: "unknown".into(), text: Some("mbiyen".into()), ..Default::default() };
        assert_eq!(raw.to_time_ref(), TimeRef::Past);
        let r = TimeRef::Past.resolve(&LifeAnchors::default(), d(2024, 5, 1));
        assert_eq!((r.start, r.end), (None, Some(d(2024, 4, 30))));
    }

    #[test]
    fn intersects_window() {
        let r = ResolvedTime { start: Some(d(1999, 1, 1)), end: Some(d(2015, 1, 1)), ongoing: false, confidence: 1.0 };
        assert!(r.intersects(Some(d(2000, 1, 1)), Some(d(2001, 1, 1))));
        assert!(!r.intersects(Some(d(2019, 1, 1)), None));
    }
}
