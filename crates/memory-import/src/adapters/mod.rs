//! Deteksi format dan adapter per format.

mod chat_export;
mod email;
mod telegram;
mod whatsapp;

use std::path::Path;

use memory_core::model::SourceKind;

use crate::{ImportError, Imported, Result};

pub use whatsapp::parse_whatsapp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    WhatsApp,
    Telegram,
    ChatGpt,
    Claude,
    Mbox,
    Eml,
    Image,
    Audio,
}

pub const IMAGE_EXT: &[&str] = &["jpg", "jpeg", "png", "heic", "webp", "gif"];
pub const AUDIO_EXT: &[&str] = &["mp3", "m4a", "wav", "ogg", "opus", "flac", "aac", "webm"];

fn ext(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Deteksi format dari ekstensi dan isi.
pub fn detect_format(path: &Path, bytes: &[u8]) -> Result<Format> {
    let e = ext(path);
    if IMAGE_EXT.contains(&e.as_str()) {
        return Ok(Format::Image);
    }
    if AUDIO_EXT.contains(&e.as_str()) {
        return Ok(Format::Audio);
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    match e.as_str() {
        "mbox" => return Ok(Format::Mbox),
        "eml" => return Ok(Format::Eml),
        "json" => {
            let h = head.trim_start();
            if h.contains("\"chat_messages\"") {
                return Ok(Format::Claude);
            }
            if h.contains("\"mapping\"") {
                return Ok(Format::ChatGpt);
            }
            if h.contains("\"messages\"") || h.contains("\"chats\"") {
                return Ok(Format::Telegram);
            }
            // Isi lengkap untuk berkas besar yang kunci pentingnya jauh di dalam.
            let full = String::from_utf8_lossy(bytes);
            if full.contains("\"chat_messages\"") {
                return Ok(Format::Claude);
            }
            if full.contains("\"mapping\"") {
                return Ok(Format::ChatGpt);
            }
            return Err(ImportError::Unsupported(format!("{}: JSON tidak dikenali", path.display())));
        }
        "txt" | "md" | "markdown" | "text" | "" => {
            if head.starts_with("From ") && head.contains("\nSubject:") {
                return Ok(Format::Mbox);
            }
            if whatsapp::looks_like_whatsapp(&head) {
                return Ok(Format::WhatsApp);
            }
            return Ok(Format::Text);
        }
        _ => {}
    }
    Err(ImportError::Unsupported(format!("{} (.{e})", path.display())))
}

/// Ubah isi berkas menjadi dokumen. Gambar & audio ditangani `Importer`
/// karena butuh caption/transkripsi.
pub fn parse_bytes(path: &Path, bytes: &[u8], hint: Option<SourceKind>) -> Result<Vec<Imported>> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let text = || String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    match detect_format(path, bytes)? {
        Format::Text => {
            let t = text();
            if t.trim().is_empty() {
                return Ok(vec![]);
            }
            Ok(vec![Imported { kind: hint.unwrap_or(SourceKind::Note), title: name, text: t, occurred_from: None, occurred_to: None, meta: serde_json::json!({"format": "text"}) }])
        }
        Format::WhatsApp => Ok(parse_whatsapp(&text(), &name)),
        Format::Telegram => telegram::parse(bytes, &name),
        Format::ChatGpt => chat_export::parse_chatgpt(bytes),
        Format::Claude => chat_export::parse_claude(bytes),
        Format::Mbox => Ok(email::parse_mbox(&text())),
        Format::Eml => Ok(email::parse_eml(&text()).into_iter().collect()),
        Format::Image | Format::Audio => Err(ImportError::Unsupported(format!("{name}: butuh Importer (caption/transkripsi)"))),
    }
}

/// Kelompokkan baris chat "[YYYY-MM-DD HH:MM] Nama: pesan" per bulan.
pub(crate) fn group_chat_by_month(lines: Vec<(String, String)>, title: &str, kind: SourceKind, format: &str) -> Vec<Imported> {
    let mut out: Vec<Imported> = Vec::new();
    let mut cur_month = String::new();
    let mut buf: Vec<(String, String)> = Vec::new();
    let flush = |buf: &mut Vec<(String, String)>, out: &mut Vec<Imported>, month: &str| {
        if buf.is_empty() {
            return;
        }
        let text = buf.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n");
        out.push(Imported {
            kind,
            title: format!("{title} ({month})"),
            text,
            occurred_from: buf.first().map(|(d, _)| d[..10].to_string()),
            occurred_to: buf.last().map(|(d, _)| d[..10].to_string()),
            meta: serde_json::json!({"format": format, "messages": buf.len()}),
        });
        buf.clear();
    };
    for (date, line) in lines {
        let month = date.chars().take(7).collect::<String>();
        if month != cur_month {
            flush(&mut buf, &mut out, &cur_month);
            cur_month = month;
        }
        buf.push((date, line));
    }
    flush(&mut buf, &mut out, &cur_month);
    out
}
