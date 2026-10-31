//! Ekspor chat WhatsApp (Android & iOS, locale Indonesia/Inggris).
//!
//! Contoh baris yang dikenali:
//! - `31/12/19 21.41 - Budi: pesan`         (Android, Indonesia)
//! - `12/31/19, 9:41 PM - Budi: pesan`      (Android, AS)
//! - `31/12/2019, 21:41 - Budi: pesan`
//! - `[31/12/19 21.41.12] Budi: pesan`       (iOS)
//!
//! Baris lanjutan (tanpa stempel) digabung ke pesan sebelumnya.

use memory_core::model::SourceKind;

use crate::Imported;

struct Msg {
    date: String,
    time: String,
    sender: String,
    text: String,
}

const SYSTEM_MARKERS: &[&str] = &[
    "<media omitted>",
    "<media tidak disertakan>",
    "pesan dan panggilan terenkripsi",
    "messages and calls are end-to-end encrypted",
    "pesan ini telah dihapus",
    "this message was deleted",
    "anda menghapus pesan ini",
    "you deleted this message",
];

fn parse_header(line: &str) -> Option<(String, String, String)> {
    // Kembalikan (tanggal ISO, jam HH:MM, sisa setelah pemisah).
    let (stamp, rest) = if let Some(r) = line.strip_prefix('[') {
        let (s, rest) = r.split_once(']')?;
        (s.to_string(), rest.trim_start().to_string())
    } else {
        let (s, rest) = line.split_once(" - ")?;
        (s.to_string(), rest.to_string())
    };
    let stamp = stamp.replace(['\u{202f}', '\u{a0}'], " ");
    let (date_part, time_part) = stamp.split_once([',', ' '])?;
    let time_part = time_part.trim_start_matches([',', ' ']).trim();
    let nums: Vec<u32> = date_part.split(['/', '.', '-']).filter_map(|x| x.trim().parse().ok()).collect();
    if nums.len() != 3 {
        return None;
    }
    let pm = time_part.to_lowercase().contains("pm");
    let am = time_part.to_lowercase().contains("am");
    let us = am || pm;
    let (d, m, y) = if nums[0] > 12 || !us { (nums[0], nums[1], nums[2]) } else { (nums[1], nums[0], nums[2]) };
    let y = if y < 100 { 2000 + y } else { y };
    if !(1..=31).contains(&d) || !(1..=12).contains(&m) {
        return None;
    }
    let tnums: Vec<u32> = time_part
        .split(|c: char| !c.is_ascii_digit())
        .filter(|x| !x.is_empty())
        .filter_map(|x| x.parse().ok())
        .collect();
    let (mut hh, mm) = (*tnums.first()?, *tnums.get(1)?);
    if pm && hh < 12 {
        hh += 12;
    }
    if am && hh == 12 {
        hh = 0;
    }
    if hh > 23 || mm > 59 {
        return None;
    }
    Some((format!("{y:04}-{m:02}-{d:02}"), format!("{hh:02}:{mm:02}"), rest))
}

pub fn looks_like_whatsapp(head: &str) -> bool {
    let lines: Vec<&str> = head.lines().filter(|l| !l.trim().is_empty()).take(10).collect();
    !lines.is_empty() && lines.iter().filter(|l| parse_header(l).is_some()).count() * 2 >= lines.len()
}

pub fn parse_whatsapp(text: &str, file_name: &str) -> Vec<Imported> {
    let mut msgs: Vec<Msg> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_start_matches('\u{200e}').trim_end();
        match parse_header(line) {
            Some((date, time, rest)) => {
                let (sender, body) = match rest.split_once(": ") {
                    Some((s, b)) => (s.trim().to_string(), b.to_string()),
                    None => continue, // pesan sistem (grup dibuat, anggota ditambahkan)
                };
                msgs.push(Msg { date, time, sender, text: body });
            }
            None => {
                if let Some(last) = msgs.last_mut()
                    && !line.trim().is_empty() {
                        last.text.push(' ');
                        last.text.push_str(line.trim());
                    }
            }
        }
    }
    let lines: Vec<(String, String)> = msgs
        .into_iter()
        .filter(|m| {
            let t = m.text.trim().to_lowercase();
            !t.is_empty() && t != "null" && !SYSTEM_MARKERS.iter().any(|s| t.contains(s))
        })
        .map(|m| (format!("{} {}", m.date, m.time), format!("[{} {}] {}: {}", m.date, m.time, m.sender, m.text.trim())))
        .collect();
    let title = file_name.trim_end_matches(".txt").replace("WhatsApp Chat with ", "WhatsApp: ").replace("Chat WhatsApp dengan ", "WhatsApp: ");
    super::group_chat_by_month(lines, &title, SourceKind::Chat, "whatsapp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_android_indonesian_and_ios_and_us() {
        assert_eq!(parse_header("31/12/19 21.41 - Budi: halo").unwrap().0, "2019-12-31");
        let (d, t, _) = parse_header("12/31/19, 9:41 PM - Budi: hi").unwrap();
        assert_eq!((d.as_str(), t.as_str()), ("2019-12-31", "21:41"));
        let (d, t, rest) = parse_header("[05/01/20 07.05.33] Ibu: Nak, sudah makan?").unwrap();
        assert_eq!((d.as_str(), t.as_str()), ("2020-01-05", "07:05"));
        assert_eq!(rest, "Ibu: Nak, sudah makan?");
        assert!(parse_header("hanya teks biasa").is_none());
    }

    #[test]
    fn groups_by_month_merges_continuations_and_skips_system() {
        let chat = "31/12/19 21.41 - Pesan dan panggilan terenkripsi secara end-to-end.\n\
                    31/12/19 21.41 - Budi: Bapak dulu kerja di pabrik gula\n\
                    sampai tahun 1998.\n\
                    31/12/19 21.42 - Ani: <Media tidak disertakan>\n\
                    02/01/20 08.00 - Ani: Selamat tahun baru!";
        assert!(looks_like_whatsapp(chat));
        let docs = parse_whatsapp(chat, "Chat WhatsApp dengan Ani.txt");
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].text, "[2019-12-31 21:41] Budi: Bapak dulu kerja di pabrik gula sampai tahun 1998.");
        assert_eq!(docs[0].occurred_from.as_deref(), Some("2019-12-31"));
        assert_eq!(docs[1].title, "WhatsApp: Ani (2020-01)");
        assert_eq!(docs[0].kind, SourceKind::Chat);
    }
}
