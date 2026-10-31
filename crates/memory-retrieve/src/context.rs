//! Perakitan konteks untuk LLM: ringkas, bersitasi, dan jujur soal waktu.

use memory_core::model::FactStatus;
use memory_core::text::truncate_chars;

use crate::RecallResult;

/// Perkiraan token (karakter / 4).
pub fn approx_tokens(s: &str) -> usize {
    s.chars().count() / 4
}

fn year(d: &str) -> &str {
    &d[..4.min(d.len())]
}

fn period(from: &Option<String>, to: &Option<String>) -> String {
    let f = from.as_deref().map(year).unwrap_or("?");
    match to {
        Some(t) => format!("{f}–{}", year(t)),
        None => format!("sejak {f}"),
    }
}

pub fn assemble_context(r: &RecallResult, reference: &str, as_of: bool, budget_tokens: usize) -> String {
    if !r.found {
        return r.note.clone().unwrap_or_default();
    }
    let acuan = if as_of { format!("per {reference}") } else { "sekarang".to_string() };
    let mut out = String::new();
    let push = |line: String, out: &mut String| -> bool {
        if approx_tokens(out) + approx_tokens(&line) > budget_tokens {
            return false;
        }
        out.push_str(&line);
        out.push('\n');
        true
    };
    if !r.facts.is_empty() {
        push(format!("## Fakta (status {acuan})"), &mut out);
        for f in &r.facts {
            let state = match (f.status, f.current) {
                (FactStatus::Conflict, _) => "BERTENTANGAN, menunggu konfirmasi".to_string(),
                (_, true) => format!("berlaku {acuan}, {}", period(&f.valid_from, &None)),
                (_, false) if f.valid_to.is_some() => format!("dulu, {}", period(&f.valid_from, &f.valid_to)),
                _ => format!("belum berlaku {acuan}"),
            };
            let cite = f.evidence.iter().take(2).map(|e| format!("[ep:{e}]")).collect::<Vec<_>>().join("");
            if !push(format!("- {} ({state}; waktu: {}) {cite}", f.statement, f.time), &mut out) {
                break;
            }
        }
    }
    if !r.principles.is_empty() {
        push("## Prinsip hidup (didukung beberapa cerita)".into(), &mut out);
        for p in &r.principles {
            let cite = p.evidence.iter().take(3).map(|e| format!("[ep:{}]", e.episode_id)).collect::<Vec<_>>().join("");
            let origin = p.origin.as_deref().map(|o| format!(" Asal: {o}.")).unwrap_or_default();
            push(format!("- {}{origin} {cite}", p.statement), &mut out);
        }
    }
    if !r.dispositions.is_empty() {
        push("## Sifat & nilai".into(), &mut out);
        for d in &r.dispositions {
            push(format!("- {} ({}): {}", d.label, period(&d.valid_from, &d.valid_to), d.description), &mut out);
        }
    }
    if !r.episodes.is_empty() {
        push("## Cerita".into(), &mut out);
        for e in &r.episodes {
            let retold = if e.retell_count > 0 { format!(", diceritakan {}x", e.retell_count + 1) } else { String::new() };
            let body = if e.summary.is_empty() { truncate_chars(&e.excerpt, 400) } else { e.summary.clone() };
            if !push(format!("- [ep:{}] ({}{retold}) {}: {}", e.id, e.time, e.title, body), &mut out) {
                break;
            }
        }
    }
    out
}
