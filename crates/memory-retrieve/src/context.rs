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

/// Potong teks agar muat `max_chars` karakter pada batas baris, dengan catatan bahwa
/// hasil dipotong. Dipakai sebagai pagar terakhir semua keluaran tool MCP.
pub fn fit_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let note = "\n… (dipotong agar muat anggaran; naikkan `budget` atau ambil detail per ID dengan get_episodes)";
    let keep = max_chars.saturating_sub(note.chars().count()).max(1);
    let cut: String = text.chars().take(keep).collect();
    let cut = match cut.rfind('\n') {
        Some(i) if i > keep / 2 => cut[..i].to_string(),
        _ => cut,
    };
    format!("{cut}{note}")
}

/// Satu baris indeks per episode: ID, tanggal, jenis, judul, perkiraan biaya token
/// bila teks penuhnya diambil. Dipakai mode `detail=index` dan injeksi konteks.
pub fn index_line(id: &str, date: &str, obs_type: Option<&str>, project: Option<&str>, title: &str, approx_tokens: usize) -> String {
    let label = memory_core::model::obs_type_label(obs_type);
    let proj = project.map(|p| format!(" ({p})")).unwrap_or_default();
    format!("- ep:{id} | {date} | [{label}]{proj} | {} | ~{} tok", if title.trim().is_empty() { "(tanpa judul)" } else { title.trim() }, approx_tokens.max(1))
}

/// Fakta sekali-pakai ("Budi memberikan instruksi") yang tidak berguna di indeks ringkas.
pub fn is_event_fact(category: &str) -> bool {
    matches!(category, "peristiwa" | "event" | "kejadian")
}

/// Konteks ringkas (lapis indeks): agent melihat APA yang ada dan BIAYA mengambilnya,
/// lalu memanggil `get_episodes(ids)` hanya untuk yang relevan.
pub fn assemble_index(r: &RecallResult, reference: &str, as_of: bool, budget_tokens: usize, show_event_facts: bool) -> String {
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
    let facts: Vec<&crate::FactView> = r
        .facts
        .iter()
        .filter(|f| f.current && f.status != FactStatus::Conflict && (show_event_facts || !is_event_fact(&f.category)))
        .collect();
    if !facts.is_empty() {
        push(format!("## Fakta berlaku {acuan}"), &mut out);
        for f in facts.iter().take(10) {
            let cite = f.evidence.first().map(|e| format!(" [ep:{e}]")).unwrap_or_default();
            if !push(format!("- {}{cite}", f.statement), &mut out) {
                break;
            }
        }
    }
    let conflicts = r.facts.iter().filter(|f| f.status == FactStatus::Conflict).count();
    if conflicts > 0 {
        push(format!("- ({conflicts} fakta bertentangan menunggu konfirmasi; lihat detail=full)"), &mut out);
    }
    if !r.principles.is_empty() {
        push("## Prinsip relevan".into(), &mut out);
        for p in r.principles.iter().take(3) {
            push(format!("- {}", p.statement), &mut out);
        }
    }
    if !r.episodes.is_empty() {
        push("## Indeks memori (ambil detail dengan get_episodes(ids=[...]))".into(), &mut out);
        for e in &r.episodes {
            let date = e.time_start.as_deref().unwrap_or(&e.time);
            let tok = (e.excerpt.chars().count().max(e.summary.chars().count()) / 4).max(20);
            if !push(index_line(&e.id, date, e.obs_type.as_deref(), e.project.as_deref(), &e.title, tok), &mut out) {
                break;
            }
        }
    }
    out
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
