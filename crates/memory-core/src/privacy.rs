//! Privasi sebelum simpan: blok `<private>…</private>` dibuang, dan rahasia
//! (API key, token, kunci privat, kredensial di URL) diganti penanda
//! `<redacted type='…'/>` agar konteks di sekitarnya tetap bisa dicari.
//!
//! Dijalankan di satu titik (tahap Ingest) sehingga semua jalur masuk, dari
//! hook agent, tool `remember`, sampai import berkas, melewati penyaring yang sama.

use std::sync::OnceLock;

use regex::{Regex, RegexBuilder};

use crate::config::PrivacyConfig;

/// Satu pola rahasia bawaan.
struct Pattern {
    name: &'static str,
    /// Pola berkeyakinan rendah: hanya aktif bila `redact_generic = true`.
    generic: bool,
    re: Regex,
    /// Grup capture yang diganti (0 = seluruh cocokan); grup lain dipertahankan.
    group: usize,
}

fn build(name: &'static str, generic: bool, pat: &str, group: usize) -> Pattern {
    let re = RegexBuilder::new(pat).case_insensitive(false).build().unwrap_or_else(|e| panic!("pola redaksi {name} tidak valid: {e}"));
    Pattern { name, generic, re, group }
}

fn patterns() -> &'static [Pattern] {
    static P: OnceLock<Vec<Pattern>> = OnceLock::new();
    P.get_or_init(|| {
        vec![
            build("aws_access_key", false, r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b", 0),
            build("aws_secret_key", false, r#"(?i)(aws_secret_access_key\s*[=:]\s*["']?)([A-Za-z0-9/+=]{40})"#, 2),
            build("github_pat", false, r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36,}\b|\bgithub_pat_[A-Za-z0-9_]{22,}\b", 0),
            build("anthropic_key", false, r"\bsk-ant-[A-Za-z0-9_\-]{20,}\b", 0),
            build("openai_key", false, r"\bsk-(?:proj-)?[A-Za-z0-9_\-]{20,}\b", 0),
            build("slack_token", false, r"\bxox[abprse]-[A-Za-z0-9\-]{10,}\b|\bxapp-[A-Za-z0-9\-]{10,}\b", 0),
            build("jwt", false, r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\b", 0),
            build("private_key_pem", false, r"-----BEGIN (?:[A-Z ]+ )?PRIVATE KEY(?: BLOCK)?-----[\s\S]*?-----END (?:[A-Z ]+ )?PRIVATE KEY(?: BLOCK)?-----", 0),
            build("stripe_key", false, r"\b(?:sk|pk|rk)_(?:live|test)_[A-Za-z0-9]{16,}\b|\bwhsec_[A-Za-z0-9]{16,}\b", 0),
            build("google_api_key", false, r"\bAIza[A-Za-z0-9_\-]{35}\b", 0),
            build("google_oauth_token", false, r"\bya29\.[A-Za-z0-9_\-\.]{20,}\b", 0),
            build("gitlab_token", false, r"\bgl(?:pat|dt|rt)-[A-Za-z0-9_\-\.]{20,}\b", 0),
            build("huggingface_token", false, r"\bhf_[A-Za-z0-9]{30,}\b", 0),
            build("npm_token", false, r"\bnpm_[A-Za-z0-9]{36}\b", 0),
            build("docker_pat", false, r"\bdckr_pat_[A-Za-z0-9_\-]{27,}\b", 0),
            build("linear_key", false, r"\blin_api_[A-Za-z0-9]{40}\b", 0),
            build("notion_token", false, r"\bntn_[A-Za-z0-9]{46}\b|\bsecret_[A-Za-z0-9]{43}\b", 0),
            build("sendgrid_key", false, r"\bSG\.[A-Za-z0-9_\-]{22}\.[A-Za-z0-9_\-]{43}\b", 0),
            build("digitalocean_token", false, r"\bdo[opr]_v1_[a-f0-9]{64}\b", 0),
            build("shopify_token", false, r"\bshp(?:at|ca|pa|ss)_[a-f0-9]{32}\b", 0),
            build("pypi_token", false, r"\bpypi-AgE[A-Za-z0-9_\-]{50,}\b", 0),
            build("nvidia_nim_key", false, r"\bnvapi-[A-Za-z0-9_\-]{32,}\b", 0),
            build("groq_key", false, r"\bgsk_[A-Za-z0-9]{52}\b", 0),
            build("xai_key", false, r"\bxai-[A-Za-z0-9]{80}\b", 0),
            build("slack_webhook", false, r"(hooks\.slack\.com/(?:services|workflows|triggers)/)([A-Za-z0-9/_\-]{20,})", 2),
            build("discord_webhook", false, r"(discord(?:app)?\.com/api/webhooks/\d+/)([A-Za-z0-9_\-\.]{20,})", 2),
            build("telegram_bot_token", false, r"\b\d{8,10}:[A-Za-z0-9_\-]{35}\b", 0),
            build("engram_token", false, r"\bcmem_[A-Za-z0-9]{20,}\b|\bcm_pro_[A-Za-z0-9]{20,}\b", 0),
            build("url_credentials", false, r"([a-z][a-z0-9+\-.]*://[^/\s:@]+:)([^@\s/]+)(@)", 2),
            build("bearer_token", true, r"(?i)((?:proxy-)?authorization:\s*bearer\s+)([A-Za-z0-9_\-\.=+/]{8,})", 2),
            build("basic_auth", true, r"(?i)((?:proxy-)?authorization:\s*basic\s+)([A-Za-z0-9+/=]{8,})", 2),
            build(
                "generic_password",
                true,
                r#"(?i)\b((?:[A-Z0-9_]*(?:password|passwd|pwd))\s*[=:]\s*["']?)([^\s"'&;,]{8,})"#,
                2,
            ),
            build(
                "generic_token",
                true,
                r#"(?i)((?:\b[A-Z0-9_]*(?:_token|_secret|_api_key|api_key|apikey|client_secret|auth_token)\b|\b(?:token|secret)\b|--(?:api-key|token|auth-token|client-secret|password))\s*[=:]\s*["']?)([A-Za-z0-9_\-\.=+/]{8,})"#,
                2,
            ),
        ]
    })
}

/// Nama semua pola bawaan (untuk dokumentasi & pengaturan `disabled_patterns`).
pub fn pattern_names() -> Vec<&'static str> {
    patterns().iter().map(|p| p.name).collect()
}

/// Hasil penyaringan.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sanitized {
    pub text: String,
    /// Jumlah rahasia yang diganti penanda.
    pub redacted: usize,
    /// Ada blok `<private>` yang dibuang.
    pub stripped_private: bool,
}

fn private_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?is)<private>.*?</private>|<private>.*$").expect("regex private"))
}

/// Buang semua blok `<private>…</private>` (tag tanpa penutup membuang sampai akhir teks).
pub fn strip_private(text: &str) -> (String, bool) {
    if !text.to_ascii_lowercase().contains("<private>") {
        return (text.to_string(), false);
    }
    let out = private_re().replace_all(text, "");
    (out.into_owned(), true)
}

fn generic_value_ok(v: &str) -> bool {
    // Hindari positif palsu: wajib ada huruf, bukan path kerja, bukan angka murni.
    v.chars().any(|c| c.is_ascii_alphabetic()) && !v.starts_with('/') && !v.starts_with("./") && !v.starts_with("~/")
}

/// Ganti rahasia dengan `<redacted type='nama'/>`.
pub fn redact(text: &str, cfg: &PrivacyConfig) -> (String, usize) {
    if !cfg.redact {
        return (text.to_string(), 0);
    }
    let mut out = text.to_string();
    let mut n = 0usize;
    for p in patterns() {
        if (p.generic && !cfg.redact_generic) || cfg.disabled_patterns.iter().any(|d| d == p.name) {
            continue;
        }
        if !p.re.is_match(&out) {
            continue;
        }
        let marker = format!("<redacted type='{}'/>", p.name);
        let mut count = 0usize;
        let replaced = p.re.replace_all(&out, |caps: &regex::Captures| {
            let whole = caps.get(0).map(|m| m.as_str()).unwrap_or_default();
            if p.group == 0 {
                count += 1;
                return marker.clone();
            }
            let Some(target) = caps.get(p.group) else { return whole.to_string() };
            if target.as_str().starts_with("<redacted") || (p.generic && !generic_value_ok(target.as_str())) {
                return whole.to_string();
            }
            count += 1;
            let start = caps.get(0).map(|m| m.start()).unwrap_or(0);
            let (a, b) = (target.start() - start, target.end() - start);
            format!("{}{}{}", &whole[..a], marker, &whole[b..])
        });
        if count > 0 {
            out = replaced.into_owned();
            n += count;
        }
    }
    (out, n)
}

/// Buang blok privat lalu redaksi rahasia. `None` bila teks kosong setelah disaring
/// (mis. seluruh pesan ditandai privat): tidak boleh disimpan sama sekali.
pub fn sanitize(text: &str, cfg: &PrivacyConfig) -> Option<Sanitized> {
    let (stripped, stripped_private) = strip_private(text);
    let (redacted_text, redacted) = redact(&stripped, cfg);
    if redacted_text.trim().is_empty() {
        return None;
    }
    Some(Sanitized { text: redacted_text, redacted, stripped_private })
}

/// Teks mengandung penanda redaksi? (untuk memberi tahu LLM bahwa itu placeholder.)
pub fn has_redaction_marker(text: &str) -> bool {
    text.contains("<redacted type=")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(generic: bool) -> PrivacyConfig {
        PrivacyConfig { redact: true, redact_generic: generic, disabled_patterns: vec![] }
    }

    #[test]
    fn strips_private_blocks_including_unclosed() {
        let (t, s) = strip_private("Tanya: <private>host db-prod user admin</private> kenapa gagal? <PRIVATE>sisa");
        assert!(s);
        assert_eq!(t.trim(), "Tanya:  kenapa gagal?".trim());
        assert_eq!(sanitize("<private>semua rahasia</private>", &cfg(false)), None);
    }

    #[test]
    fn redacts_vendor_keys_and_keeps_context() {
        let text = "curl -H 'Authorization: Bearer sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789' https://api.anthropic.com\nAKIAABCDEFGHIJKLMNOP token ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij0123";
        let (out, n) = redact(text, &cfg(false));
        assert_eq!(n, 3, "{out}");
        assert!(out.contains("<redacted type='anthropic_key'/>"));
        assert!(out.contains("<redacted type='aws_access_key'/>"));
        assert!(out.contains("<redacted type='github_pat'/>"));
        assert!(out.contains("https://api.anthropic.com"));
        assert!(!out.contains("sk-ant-"));
    }

    #[test]
    fn redacts_url_credentials_but_keeps_host() {
        let (out, n) = redact("DATABASE_URL=postgres://app:s3cretPass@db.internal:5432/app", &cfg(false));
        assert_eq!(n, 1);
        assert_eq!(out, "DATABASE_URL=postgres://app:<redacted type='url_credentials'/>@db.internal:5432/app");
    }

    #[test]
    fn pem_block_and_jwt() {
        let text = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nabc\n-----END RSA PRIVATE KEY-----\nlogin eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let (out, n) = redact(text, &cfg(false));
        assert_eq!(n, 2, "{out}");
        assert!(!out.contains("MIIEow"));
    }

    #[test]
    fn generic_patterns_opt_in_and_avoid_false_positives() {
        let text = "DB_PASSWORD=supersecret123 max_tokens=4096 PWD=/home/user token=abcdef0123456789";
        let (out, n) = redact(text, &cfg(false));
        assert_eq!(n, 0, "{out}");
        let (out, n) = redact(text, &cfg(true));
        assert!(out.contains("DB_PASSWORD=<redacted type='generic_password'/>"), "{out}");
        assert!(out.contains("max_tokens=4096"));
        assert!(out.contains("PWD=/home/user"), "{out}");
        assert!(out.contains("token=<redacted type='generic_token'/>"), "{out}");
        assert_eq!(n, 2);
    }

    #[test]
    fn disabled_pattern_and_off_switch() {
        let text = "key sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789";
        let c = PrivacyConfig { redact: true, redact_generic: false, disabled_patterns: vec!["anthropic_key".into(), "openai_key".into()] };
        assert_eq!(redact(text, &c).1, 0);
        let off = PrivacyConfig { redact: false, ..cfg(false) };
        assert_eq!(redact(text, &off).0, text);
        assert!(pattern_names().contains(&"url_credentials"));
    }

    #[test]
    fn plain_prose_untouched() {
        let text = "Pakde Karno membuka toko batik di Solo tahun 1995 dan pindah ke Klaten.";
        let s = sanitize(text, &cfg(true)).unwrap();
        assert_eq!(s.text, text);
        assert_eq!(s.redacted, 0);
        assert!(!s.stripped_private);
    }
}
