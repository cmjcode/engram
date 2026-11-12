//! ID, hash, dan nilai acak yang dipakai klien maupun server hub.

/// ID deterministik dari beberapa bagian: 26 heksa pertama blake3. Algoritmanya sama dengan
/// `memory_extract::det_id` (crate ini sengaja tidak bergantung pada memory-extract).
pub fn det_id(parts: &[&str]) -> String {
    blake3::hash(parts.join("\u{1f}").as_bytes()).to_hex()[..26].to_string()
}

/// Hash blake3 heksa penuh (refresh token, PAT, join key disimpan sebagai hash ini).
pub fn hash_hex(s: &str) -> String {
    blake3::hash(s.as_bytes()).to_hex().to_string()
}

/// `n` byte acak dari CSPRNG sebagai heksa huruf kecil.
pub fn random_hex(n: usize) -> String {
    use rand::RngCore;
    let mut b = vec![0u8; n];
    rand::rng().fill_bytes(&mut b);
    hex(&b)
}

pub fn hex(bytes: &[u8]) -> String {
    const T: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(T[(b >> 4) as usize] as char);
        s.push(T[(b & 15) as usize] as char);
    }
    s
}

/// Perbandingan waktu-konstan (join key, token) agar panjang kecocokan tidak bocor lewat waktu.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Jenis record memori bersama (urutan dipakai untuk tampilan).
pub const RECORD_KINDS: [&str; 4] = ["lessons", "decisions", "facts", "episodes"];

pub fn is_record_kind(k: &str) -> bool {
    RECORD_KINDS.contains(&k)
}

/// ID record (`rid`): alfanumerik 1..=64 (keluaran [`det_id`] atau ID legacy).
pub fn is_valid_rid(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// ID perangkat/pengguna/repo: alfanumerik, `-`, `_`, `:`, `.` (ULID atau `legacy:<folder>`).
pub fn is_valid_key(s: &str) -> bool {
    !s.is_empty() && s.len() <= 96 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
}

/// Join key baru: `ek1_` + 20 byte acak heksa.
pub fn new_join_key() -> String {
    format!("ek1_{}", random_hex(20))
}

/// Personal access token baru: `engram_pat_` + 32 byte acak heksa.
pub fn new_pat() -> String {
    format!("{PAT_PREFIX}{}", random_hex(32))
}

pub const PAT_PREFIX: &str = "engram_pat_";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_keys() {
        assert_eq!(det_id(&["a", "b"]).len(), 26);
        assert_eq!(det_id(&["a", "b"]), det_id(&["a", "b"]));
        assert_ne!(det_id(&["ab"]), det_id(&["a", "b"]));
        assert_eq!(random_hex(16).len(), 32);
        assert!(constant_time_eq("abc", "abc") && !constant_time_eq("abc", "abd") && !constant_time_eq("a", "ab"));
        assert!(new_join_key().starts_with("ek1_") && new_pat().starts_with(PAT_PREFIX));
        assert!(is_valid_rid(&det_id(&["x"])) && !is_valid_rid("../x") && is_valid_key("legacy:budi-1a2b") && !is_valid_key("a b"));
    }
}
