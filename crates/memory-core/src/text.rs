//! Utilitas teks: normalisasi, deteksi bahasa campur (Indonesia/Jawa/Sunda/
//! Inggris), verifikasi kutipan terhadap sumber, dan leksikon sinyal.

use unicode_normalization::UnicodeNormalization;

/// Huruf kecil, NFKC, tanda baca jadi spasi, spasi dirapatkan.
pub fn normalize(s: &str) -> String {
    let lowered: String = s.nfkc().collect::<String>().to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut last_space = true;
    for c in lowered.chars() {
        if c.is_alphanumeric() {
            out.push(c);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim_end().to_string()
}

pub fn tokens(s: &str) -> Vec<String> {
    normalize(s).split_whitespace().map(str::to_string).collect()
}

/// Kutipan dianggap didukung sumber bila (setelah normalisasi) merupakan
/// substring sumber, atau ada jendela di sumber dengan overlap token >= 80%
/// (toleransi kesalahan transkripsi kecil).
pub fn quote_supported(quote: &str, source: &str) -> bool {
    let q = normalize(quote);
    if q.is_empty() {
        return false;
    }
    let s = normalize(source);
    if s.contains(&q) {
        return true;
    }
    let qt: Vec<&str> = q.split_whitespace().collect();
    let st: Vec<&str> = s.split_whitespace().collect();
    if qt.len() < 3 || st.len() < qt.len() {
        return false;
    }
    let need = (qt.len() as f32 * 0.8).ceil() as usize;
    let window = qt.len() + 2;
    let qset: std::collections::HashSet<&str> = qt.iter().copied().collect();
    let mut i = 0;
    while i + qt.len() <= st.len() {
        let end = (i + window).min(st.len());
        let hit = st[i..end].iter().filter(|t| qset.contains(*t)).collect::<std::collections::HashSet<_>>().len();
        if hit >= need {
            return true;
        }
        i += 1;
    }
    false
}

const JV: &[&str] = &[
    "ora", "kowe", "piye", "ngene", "ngono", "wong", "mangan", "turu", "ning", "nek", "sopo", "sampeyan",
    "njenengan", "mboten", "inggih", "matur", "nuwun", "wis", "durung", "isih", "tenan", "wae", "karo",
    "kuwi", "iki", "kae", "opo", "ngopo", "aku", "awakdewe", "bapake", "simbok", "mbah", "lan", "ojo",
    "arep", "teko", "lunga", "neng", "sing", "dadi", "ngerti", "akeh", "sithik", "gedhe", "cilik", "omah",
];
const SU: &[&str] = &[
    "abdi", "anjeun", "teu", "naon", "kumaha", "mah", "atuh", "euy", "sok", "urang", "aya", "teh",
    "sareng", "hatur", "nuhun", "punten", "mangga", "kitu", "kieu", "pisan", "henteu", "acan", "geus",
    "bade", "badé", "ambu", "abah", "imah", "dahar", "sakola", "lain", "ieu", "eta", "kudu",
];
const ID: &[&str] = &[
    "yang", "dan", "di", "tidak", "saya", "itu", "ini", "dengan", "untuk", "ke", "dari", "ada", "sudah",
    "belum", "karena", "waktu", "kami", "kita", "mereka", "dia", "akan", "bisa", "juga", "tapi", "kalau",
    "nggak", "gak", "enggak", "banget", "sekali", "rumah", "bapak", "ibu", "sekolah", "kerja",
];
const EN: &[&str] = &[
    "the", "and", "is", "are", "was", "were", "i", "you", "he", "she", "they", "with", "for", "of",
    "to", "in", "that", "this", "it", "my", "have", "has", "not", "but",
];

/// Bahasa yang terdeteksi dengan pangsa >= 10%, diurutkan dari terbesar.
/// Kode: id, jv, su, en, atau kode ISO 639-3 dari whatlang untuk lainnya.
pub fn detect_languages(text: &str) -> Vec<String> {
    let toks = tokens(text);
    if toks.is_empty() {
        return vec![];
    }
    let mut counts = [("id", 0usize), ("jv", 0), ("su", 0), ("en", 0)];
    for t in &toks {
        let t = t.as_str();
        if JV.contains(&t) {
            counts[1].1 += 1;
        }
        if SU.contains(&t) {
            counts[2].1 += 1;
        }
        if ID.contains(&t) {
            counts[0].1 += 1;
        }
        if EN.contains(&t) {
            counts[3].1 += 1;
        }
    }
    let total: usize = counts.iter().map(|c| c.1).sum();
    if total == 0 {
        return whatlang::detect(text)
            .filter(|i| i.is_reliable())
            .map(|i| vec![i.lang().code().to_string()])
            .unwrap_or_default();
    }
    let mut v: Vec<(&str, usize)> = counts.into_iter().filter(|(_, c)| *c * 10 >= total).collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    v.into_iter().map(|(l, _)| l.to_string()).collect()
}

const DECISION: &[&str] = &[
    "memutuskan", "keputusan", "memilih", "pilih", "akhirnya", "nekat", "berani", "mengalah", "prinsip",
    "selalu", "harus", "jangan", "pantang", "milih", "mutuske", "nekad", "rela", "mengorbankan", "berjanji",
    "janji", "bertekad", "menolak", "mundur", "bertahan", "memaafkan", "decided", "chose",
];
const EMOTION: &[&str] = &[
    "sedih", "senang", "bahagia", "marah", "kecewa", "takut", "bangga", "malu", "menangis", "nangis",
    "rindu", "kangen", "terharu", "susah", "berat", "syukur", "bersyukur", "trauma", "hancur", "gembira",
    "seneng", "isin", "wedi", "nesu", "sedhih", "bungah", "lega", "menyesal", "nyesel", "cinta", "sayang",
];
const NOISE: &[&str] = &[
    "oke", "ok", "okey", "sip", "wkwk", "wkwkwk", "haha", "hahaha", "hehe", "test", "tes", "halo", "hai",
    "siap", "noted", "otw", "ya", "iya", "yoi", "oh", "ohh", "hmm", "mantap", "thanks", "makasih", "lol",
    "pagi", "siang", "malam", "selamat",
];

pub fn count_lexicon(toks: &[String], lex: &[&str]) -> usize {
    toks.iter().filter(|t| lex.contains(&t.as_str())).count()
}

pub fn decision_signal(toks: &[String]) -> usize {
    count_lexicon(toks, DECISION)
}

pub fn emotion_signal(toks: &[String]) -> usize {
    count_lexicon(toks, EMOTION)
}

/// Pangsa token noise di antara token non-angka.
pub fn noise_ratio(toks: &[String]) -> f32 {
    let words: Vec<String> = toks.iter().filter(|t| !t.chars().all(|c| c.is_ascii_digit())).cloned().collect();
    if words.is_empty() {
        return 1.0;
    }
    count_lexicon(&words, NOISE) as f32 / words.len() as f32
}

/// Jumlah kata berhuruf kapital yang bukan awal kalimat (perkiraan nama diri).
pub fn proper_noun_count(text: &str) -> usize {
    let mut count = 0;
    let mut sentence_start = true;
    for w in text.split_whitespace() {
        let first = w.chars().next();
        if !sentence_start && first.is_some_and(char::is_uppercase) && w.chars().count() > 1 {
            count += 1;
        }
        sentence_start = w.ends_with(['.', '!', '?', ':']);
    }
    count
}

/// Potong teks pada batas karakter agar aman untuk UTF-8.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_punctuation_and_case() {
        assert_eq!(normalize("  Pak-Dhe  KARNO, bilang: \"Ojo!\" "), "pak dhe karno bilang ojo");
    }

    #[test]
    fn quote_exact_and_fuzzy() {
        let src = "Waktu itu bapak bilang, kalau usaha rugi, karyawan tetap harus digaji dulu.";
        assert!(quote_supported("karyawan tetap harus digaji dulu", src));
        // Satu kata salah transkripsi tetap diterima.
        assert!(quote_supported("kalau usaha rugi karyawan tetep harus digaji dulu", src));
        assert!(!quote_supported("saya tidak pernah menggaji karyawan", src));
        assert!(!quote_supported("", src));
    }

    #[test]
    fn detects_mixed_javanese_indonesian() {
        let langs = detect_languages("Saya dulu tinggal di rumah mbah, wong ora duwe duit, tapi aku ora sedih.");
        assert!(langs.contains(&"jv".to_string()), "{langs:?}");
        assert!(langs.contains(&"id".to_string()), "{langs:?}");
    }

    #[test]
    fn detects_sundanese() {
        let langs = detect_languages("Abdi teh teu acan dahar, kumaha atuh");
        assert_eq!(langs.first().map(String::as_str), Some("su"));
    }

    #[test]
    fn noise_detection() {
        assert!(noise_ratio(&tokens("oke sip wkwk")) > 0.9);
        assert!(noise_ratio(&tokens("Bapak memutuskan pindah ke Jogja tahun 2015")) < 0.1);
    }

    #[test]
    fn truncate_is_utf8_safe() {
        assert_eq!(truncate_chars("héllo", 2), "hé…");
    }
}
