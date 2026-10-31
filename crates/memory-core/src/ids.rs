//! Pembuatan ID. ULID dipakai agar ID terurut menurut waktu pembuatan dan
//! aman dipakai sebagai kunci record SurrealDB maupun nama file.

/// ID baru (ULID, huruf kecil).
pub fn new_id() -> String {
    ulid::Ulid::new().to_string().to_lowercase()
}

/// Waktu sekarang dalam RFC3339 UTC dengan presisi detik, format yang dipakai
/// di seluruh penyimpanan sehingga perbandingan string sama dengan perbandingan waktu.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// ID valid: hanya alfanumerik, `-` dan `_`, panjang 1..=64.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_sortable_and_valid() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b);
        assert!(is_valid_id(&a));
        assert_eq!(a.len(), 26);
    }

    #[test]
    fn rejects_injection_like_ids() {
        assert!(!is_valid_id("a'; DELETE episode"));
        assert!(!is_valid_id(""));
    }
}
