//! Email: `.mbox` (banyak pesan) dan `.eml` (satu pesan). Hanya bagian
//! text/plain yang diambil; lampiran diabaikan.

use memory_core::model::SourceKind;

use crate::Imported;

fn header<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{}:", name.to_lowercase());
    headers.lines().find(|l| l.to_lowercase().starts_with(&prefix)).map(|l| l[prefix.len()..].trim())
}

fn plain_body(headers: &str, body: &str) -> String {
    let ct = header(headers, "Content-Type").unwrap_or("text/plain").to_lowercase();
    if !ct.starts_with("multipart/") {
        return body.to_string();
    }
    let Some(b) = ct.split("boundary=").nth(1) else { return String::new() };
    // Ambil boundary dari header asli (case-sensitive).
    let raw_ct = header(headers, "Content-Type").unwrap_or("");
    let idx = raw_ct.to_lowercase().find("boundary=").unwrap_or(0) + "boundary=".len();
    let boundary = raw_ct.get(idx..idx + b.len()).unwrap_or(b).trim_matches('"').split(';').next().unwrap_or("").to_string();
    for part in body.split(&format!("--{boundary}")) {
        let part = part.trim_start_matches(['\r', '\n']);
        let Some((ph, pb)) = part.split_once("\n\n") else { continue };
        let pct = header(ph, "Content-Type").unwrap_or("text/plain").to_lowercase();
        if pct.starts_with("text/plain") {
            return pb.trim().to_string();
        }
    }
    String::new()
}

pub fn parse_eml(raw: &str) -> Option<Imported> {
    let (headers, body) = raw.split_once("\n\n")?;
    // Gabungkan header yang dilipat (baris lanjutan diawali spasi).
    let headers = headers.replace("\n ", " ").replace("\n\t", " ");
    let text = plain_body(&headers, body);
    if text.trim().chars().count() < 20 {
        return None;
    }
    let subject = header(&headers, "Subject").unwrap_or("(tanpa subjek)").to_string();
    let from = header(&headers, "From").unwrap_or("?").to_string();
    let date = header(&headers, "Date").and_then(|d| chrono::DateTime::parse_from_rfc2822(d).ok()).map(|d| d.format("%Y-%m-%d").to_string());
    Some(Imported {
        kind: SourceKind::Email,
        title: format!("Email: {subject}"),
        text: format!("Dari: {from}\nSubjek: {subject}\n\n{}", text.trim()),
        occurred_from: date.clone(),
        occurred_to: date,
        meta: serde_json::json!({"format": "email", "from": from}),
    })
}

pub fn parse_mbox(raw: &str) -> Vec<Imported> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in raw.lines() {
        if line.starts_with("From ") && !cur.is_empty() {
            out.extend(parse_eml(&cur));
            cur.clear();
            continue;
        }
        if line.starts_with("From ") {
            continue;
        }
        cur.push_str(line.strip_prefix('>').filter(|l| l.starts_with("From ")).unwrap_or(line));
        cur.push('\n');
    }
    if !cur.is_empty() {
        out.extend(parse_eml(&cur));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mbox_with_multipart() {
        let mbox = "From a@x Mon Jan  1 00:00:00 2018\n\
Subject: Kabar dari Jogja\nFrom: Budi <b@x>\nDate: Mon, 1 Jan 2018 10:00:00 +0700\n\nNak, Bapak sudah sampai di Jogja dengan selamat, besok mulai kerja.\n\
From a@x Tue Jan  2 00:00:00 2018\n\
Subject: Foto\nFrom: Ani <a@x>\nDate: Tue, 2 Jan 2018 10:00:00 +0700\nContent-Type: multipart/alternative; boundary=\"XYZ\"\n\n--XYZ\nContent-Type: text/plain\n\nIni foto rumah lama kita di Klaten waktu lebaran.\n--XYZ\nContent-Type: text/html\n\n<p>x</p>\n--XYZ--\n";
        let docs = parse_mbox(mbox);
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].occurred_from.as_deref(), Some("2018-01-01"));
        assert!(docs[1].text.contains("rumah lama kita di Klaten"));
        assert!(!docs[1].text.contains("<p>"));
    }
}
