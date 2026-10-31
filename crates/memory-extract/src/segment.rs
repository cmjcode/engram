//! Segmentasi: pecah teks sumber menjadi unit (baris/paragraf), kelompokkan
//! secara topikal oleh LLM ringan, dengan fallback heuristik.

use memory_core::text::truncate_chars;
use memory_llm::{ModelRole, complete_json};

use crate::llm_types::{SegmentOut, SegmentPlan};
use crate::{Pipeline, prompts};

/// Satu segmen hasil (indeks unit global, inklusif).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub topic: String,
    pub about_subject: bool,
    pub speaker: Option<String>,
    pub significance: Option<f32>,
    /// Tanggal dari stempel waktu unit pertama (chat/lifelog), ISO.
    pub date: Option<String>,
}

const MAX_UNIT_CHARS: usize = 1500;

/// Pecah teks menjadi unit: baris non-kosong; paragraf panjang dipotong per kalimat.
pub fn split_units(text: &str) -> Vec<String> {
    let mut units = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.chars().count() <= MAX_UNIT_CHARS {
            units.push(line.to_string());
            continue;
        }
        let mut cur = String::new();
        for sentence in line.split_inclusive(['.', '!', '?']) {
            if cur.chars().count() + sentence.chars().count() > MAX_UNIT_CHARS && !cur.is_empty() {
                units.push(cur.trim().to_string());
                cur.clear();
            }
            cur.push_str(sentence);
        }
        if !cur.trim().is_empty() {
            units.push(cur.trim().to_string());
        }
    }
    units
}

/// Jendela unit dengan total karakter <= max.
pub fn windows(units: &[String], max_chars: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut size = 0;
    for (i, u) in units.iter().enumerate() {
        let len = u.chars().count() + 8;
        if size + len > max_chars && i > start {
            out.push((start, i - 1));
            start = i;
            size = 0;
        }
        size += len;
    }
    if start < units.len() {
        out.push((start, units.len() - 1));
    }
    out
}

/// Tanggal ISO dari awalan unit `[YYYY-MM-DD ...]` (format adapter chat).
pub fn unit_date(unit: &str) -> Option<String> {
    let s = unit.strip_prefix('[')?;
    let d = s.get(..10)?;
    let ok = d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-' && d.chars().filter(char::is_ascii_digit).count() == 8;
    ok.then(|| d.to_string())
}

/// Buang awalan `[YYYY-MM-DD HH:MM] Nama:` per baris (format adapter chat)
/// agar stempel waktu dan nama pengirim tidak mengencerkan sinyal skor.
pub fn strip_chat_prefix(text: &str) -> String {
    text.lines()
        .map(|l| {
            let l = l.trim();
            let rest = match l.strip_prefix('[').and_then(|r| r.split_once(']')) {
                Some((_, rest)) => rest.trim_start(),
                None => return l,
            };
            match rest.split_once(':') {
                Some((name, msg)) if name.chars().count() <= 40 && name.split_whitespace().count() <= 3 => msg.trim_start(),
                _ => rest,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Validasi rencana LLM terhadap jendela [lo, hi]: urut, tidak tumpang tindih,
/// dalam batas; celah diisi segmen netral.
pub fn normalize_plan(mut segs: Vec<SegmentOut>, lo: usize, hi: usize) -> Vec<SegmentOut> {
    segs.retain(|s| s.start <= s.end);
    // Model kadang memakai indeks lokal (mulai 0) untuk jendela non-pertama.
    if lo > 0 && segs.iter().all(|s| s.end <= hi - lo) && segs.iter().any(|s| s.start < lo) {
        for s in &mut segs {
            s.start += lo;
            s.end += lo;
        }
    }
    segs.retain(|s| s.start >= lo && s.end <= hi);
    segs.sort_by_key(|s| s.start);
    let mut out: Vec<SegmentOut> = Vec::new();
    let mut next = lo;
    for mut s in segs {
        if s.end < next {
            continue;
        }
        if s.start < next {
            s.start = next;
        }
        if s.start > next {
            out.push(SegmentOut { start: next, end: s.start - 1, topic: String::new(), about_subject: true, speaker: None, significance: None });
        }
        next = s.end + 1;
        out.push(s);
    }
    if next <= hi {
        out.push(SegmentOut { start: next, end: hi, topic: String::new(), about_subject: true, speaker: None, significance: None });
    }
    out
}

/// Fallback heuristik: unit berurutan dikelompokkan sampai ~1200 karakter.
pub fn heuristic_plan(units: &[String], lo: usize, hi: usize) -> Vec<SegmentOut> {
    let mut out = Vec::new();
    let mut start = lo;
    let mut size = 0;
    for (i, unit) in units.iter().enumerate().take(hi + 1).skip(lo) {
        size += unit.chars().count();
        if size >= 1200 || i == hi {
            out.push(SegmentOut { start, end: i, topic: String::new(), about_subject: true, speaker: None, significance: None });
            start = i + 1;
            size = 0;
        }
    }
    out
}

impl Pipeline {
    /// Segmentasi seluruh teks. `examples`: contoh few-shot dari review.
    pub async fn segment(&self, subject_name: &str, text: &str, examples: &str) -> Vec<Segment> {
        let units = split_units(text);
        if units.is_empty() {
            return vec![];
        }
        let mut plan: Vec<SegmentOut> = Vec::new();
        for (lo, hi) in windows(&units, self.cfg.pipeline.max_window_chars) {
            let numbered: Vec<(usize, String)> = (lo..=hi).map(|i| (i, truncate_chars(&units[i], 600))).collect();
            let user = prompts::segment_user(subject_name, &numbered, examples);
            let res: memory_llm::Result<SegmentPlan> =
                complete_json(self.llm.as_ref(), ModelRole::Light, "segment", prompts::SEGMENT_SYSTEM, &user, self.cfg.llm.max_retries).await;
            match res {
                Ok(p) if !p.segments.is_empty() => plan.extend(normalize_plan(p.segments, lo, hi)),
                Ok(_) => plan.extend(heuristic_plan(&units, lo, hi)),
                Err(e) => {
                    tracing::warn!(error = %e, "segmentasi LLM gagal, memakai heuristik");
                    plan.extend(heuristic_plan(&units, lo, hi));
                }
            }
        }
        plan.into_iter()
            .map(|s| Segment {
                text: units[s.start..=s.end].join("\n"),
                date: unit_date(&units[s.start]),
                start: s.start,
                end: s.end,
                topic: s.topic,
                about_subject: s.about_subject,
                speaker: s.speaker,
                significance: s.significance,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn so(start: usize, end: usize) -> SegmentOut {
        SegmentOut { start, end, topic: String::new(), about_subject: true, speaker: None, significance: None }
    }

    #[test]
    fn units_and_windows() {
        let text = "baris satu\n\nbaris dua\n".to_string() + &"Kalimat panjang. ".repeat(200);
        let u = split_units(&text);
        assert!(u.len() >= 4);
        assert!(u.iter().all(|x| x.chars().count() <= MAX_UNIT_CHARS));
        let w = windows(&u, 2000);
        assert_eq!(w.first().unwrap().0, 0);
        assert_eq!(w.last().unwrap().1, u.len() - 1);
    }

    #[test]
    fn plan_gaps_overlaps_and_local_indices_are_fixed() {
        let p = normalize_plan(vec![so(2, 4), so(3, 6), so(9, 20)], 0, 9);
        let ranges: Vec<_> = p.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(ranges, vec![(0, 1), (2, 4), (5, 6), (7, 9)]);
        // Indeks lokal untuk jendela 10..=15
        let p = normalize_plan(vec![so(0, 2), so(3, 5)], 10, 15);
        assert_eq!(p.iter().map(|s| (s.start, s.end)).collect::<Vec<_>>(), vec![(10, 12), (13, 15)]);
    }

    #[test]
    fn strips_chat_prefixes() {
        assert_eq!(strip_chat_prefix("[2024-01-01 08:00] Budi: oke sip\nbiasa"), "oke sip\nbiasa");
    }

    #[test]
    fn chat_unit_date() {
        assert_eq!(unit_date("[2019-12-31 21:41] Budi: halo"), Some("2019-12-31".into()));
        assert_eq!(unit_date("Budi: halo"), None);
    }
}
