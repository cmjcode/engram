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
    // Sinyal keputusan teknis, arsitektur, dan kerja profesional:
    "arsitektur", "architecture", "refactor", "migrasi", "migration", "konvensi", "convention",
    "standar", "standard", "solusi", "solution", "workaround", "root", "cause", "bug", "fix",
    "optimasi", "skema", "schema", "endpoint", "implementasi", "aturan", "rule", "validasi",
    "formulasi", "menggunakan", "memakai", "pakai", "diganti", "mengganti", "deprecated", "pattern",
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

/// Jumlah kata berhuruf kapital yang bukan awal kalimat (perkiraan nama diri)
/// ditambah identifier teknis (path `a/b`, `snake_case`, `kebab-case`, ekstensi kode).
pub fn proper_noun_count(text: &str) -> usize {
    let mut count = 0;
    let mut sentence_start = true;
    for w in text.split_whitespace() {
        let clean = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '_' && c != '-' && c != '.');
        let first = clean.chars().next();
        if (!sentence_start && first.is_some_and(char::is_uppercase) && clean.chars().count() > 1) || is_technical_token(clean) {
            count += 1;
        }
        sentence_start = w.ends_with(['.', '!', '?', ':']);
    }
    count
}

/// Apakah satu token tampak seperti identifier teknis (path, berkas kode, snake_case, kebab-case).
pub fn is_technical_token(tok: &str) -> bool {
    let t = tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '_' && c != '-' && c != '.');
    if t.chars().count() < 3 || t.contains(' ') {
        return false;
    }
    let has_alpha = t.chars().any(char::is_alphabetic);
    if !has_alpha {
        return false;
    }
    if t.contains('/') && !t.starts_with("http") && t.split('/').all(|s| !s.is_empty()) {
        return true;
    }
    if t.contains('_') {
        return true;
    }
    if t.contains('-') && t.split('-').count() >= 2 && t.chars().any(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
        return true;
    }
    const EXTS: &[&str] = &[
        ".rs", ".go", ".py", ".ts", ".tsx", ".js", ".jsx", ".json", ".toml", ".yaml", ".yml",
        ".sql", ".sh", ".md", ".html", ".css", ".vue", ".java", ".kt", ".swift", ".c", ".cpp", ".proto",
    ];
    let low = t.to_lowercase();
    EXTS.iter().any(|ext| low.ends_with(ext))
}

fn strip_entity_prefix(s: &str) -> &str {
    const PREFIXES: &[&str] = &[
        "berkas ", "file ", "modul ", "module ", "proyek ", "project ", "repo ", "repositori ",
        "repository ", "service ", "microservice ", "layanan ", "aplikasi ", "komponen ",
        "tabel ", "table ", "endpoint ", "fungsi ", "function ",
    ];
    let mut cur = s.trim();
    for p in PREFIXES {
        if let Some(rest) = cur.strip_prefix(p) {
            cur = rest.trim();
            break;
        }
    }
    cur
}

fn split_scoped_mention(s: &str) -> Option<(&str, &str)> {
    for sep in [" pada ", " di dalam ", " di repo ", " di proyek ", " di service ", " di folder ", " in ", " of "] {
        if let Some((left, right)) = s.split_once(sep) {
            let l = left.trim();
            let r = right.trim();
            if !l.is_empty() && !r.is_empty() {
                return Some((l, r));
            }
        }
    }
    None
}

/// Deteksi apakah dua nama entitas secara struktural adalah identifier teknis,
/// microservice, berkas, atau komponen yang BERBEDA (sehingga tidak boleh digabung
/// atau diajukan ke antrean review `EntityMerge`).
pub fn are_distinct_technical_names(a: &str, b: &str) -> bool {
    let al = a.trim().to_lowercase();
    let bl = b.trim().to_lowercase();
    if al.is_empty() || bl.is_empty() || normalize(&al) == normalize(&bl) {
        return false;
    }

    // 1. Relasi bagian-dari ("X pada Y" vs "Y", atau "X pada Y" vs "Z pada Y" dengan X != Z).
    let sa = split_scoped_mention(&al);
    let sb = split_scoped_mention(&bl);
    match (sa, sb) {
        (Some((la, ra)), None) => {
            let clean_b = strip_entity_prefix(&bl);
            if normalize(ra) == normalize(clean_b) || is_technical_token(ra) || is_technical_token(clean_b) {
                return normalize(la) != normalize(clean_b);
            }
        }
        (None, Some((lb, rb))) => {
            let clean_a = strip_entity_prefix(&al);
            if normalize(rb) == normalize(clean_a) || is_technical_token(rb) || is_technical_token(clean_a) {
                return normalize(lb) != normalize(clean_a);
            }
        }
        (Some((la, ra)), Some((lb, rb))) => {
            if normalize(la) != normalize(lb) || normalize(ra) != normalize(rb) {
                return true;
            }
        }
        (None, None) => {}
    }

    let ca = strip_entity_prefix(&al);
    let cb = strip_entity_prefix(&bl);
    let tech_a = is_technical_token(ca) || crate::model::EntityKind::infer_from_name(a).is_some_and(|k| k.is_technical());
    let tech_b = is_technical_token(cb) || crate::model::EntityKind::infer_from_name(b).is_some_and(|k| k.is_technical());

    if tech_a && tech_b {
        // Bandingkan segmen terakhir (basename) bila ada path `/`.
        let base_a = ca.rsplit('/').next().unwrap_or(ca).trim();
        let base_b = cb.rsplit('/').next().unwrap_or(cb).trim();
        if base_a != base_b {
            return true;
        }
        // Bila basename sama tapi prefix induknya eksplisit dan berbeda (mis. "FRONTEND/app" vs "BACKEND/app")
        if ca.contains('/') && cb.contains('/') && ca != cb {
            return true;
        }
    }

    false
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

    #[test]
    fn distinct_technical_names_blocks_false_merges() {
        assert!(are_distinct_technical_names(
            "BACKEND/api-master-sensor",
            "BACKEND/api-trans-project"
        ));
        assert!(are_distinct_technical_names(
            "BACKEND/api-data-warehouse",
            "BACKEND/api-master-sensor"
        ));
        assert!(are_distinct_technical_names(
            "BACKEND/api-master-product",
            "BACKEND/api-master-sensor"
        ));
        assert!(are_distinct_technical_names(
            "berkas autentikasi pada BACKEND/api-trans-stock-company",
            "BACKEND/api-trans-stock-company"
        ));
        assert!(are_distinct_technical_names(
            "total_value_formulation_test.go pada BACKEND/api-data-warehouse",
            "BACKEND/api-data-warehouse"
        ));
        // Short alias of the same service is NOT blocked:
        assert!(!are_distinct_technical_names(
            "BACKEND/api-master-sensor",
            "api-master-sensor"
        ));
        // Human names are NOT blocked by technical filter:
        assert!(!are_distinct_technical_names("Pakde Karno", "Soekarno"));
    }
}
