//! Bahasa teks bawaan aplikasi desktop (menu bar, dialog, update).
//! Default bahasa Inggris; pilihan pengguna dari UI disimpan di `desktop.json` (`lang`).

use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

use anyhow::{Context, Result};

/// Preferensi aplikasi desktop (bukan bagian memory.toml).
pub const PREFS: &str = "desktop.json";

pub const SUPPORTED: [&str; 2] = ["en", "id"];

/// 0 = en, 1 = id.
static LANG: AtomicU8 = AtomicU8::new(0);

pub fn is_id() -> bool {
    LANG.load(Ordering::Relaxed) == 1
}

pub fn code() -> &'static str {
    if is_id() { "id" } else { "en" }
}

fn apply(code: &str) {
    LANG.store(u8::from(code == "id"), Ordering::Relaxed);
}

pub fn read_prefs(data_dir: &Path) -> serde_json::Value {
    std::fs::read_to_string(data_dir.join(PREFS))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}))
}

pub fn write_pref(data_dir: &Path, key: &str, value: serde_json::Value) -> Result<()> {
    let path = data_dir.join(PREFS);
    let mut v = read_prefs(data_dir);
    v[key] = value;
    std::fs::write(&path, serde_json::to_string_pretty(&v)?).with_context(|| format!("writing {}", path.display()))
}

/// Muat bahasa tersimpan (dipanggil sekali saat aplikasi dibuka).
pub fn load(data_dir: &Path) {
    apply(read_prefs(data_dir)["lang"].as_str().unwrap_or("en"));
}

/// Ganti & simpan bahasa.
pub fn set(data_dir: &Path, code: &str) -> Result<()> {
    anyhow::ensure!(SUPPORTED.contains(&code), "unsupported language: {code}");
    write_pref(data_dir, "lang", code.into())?;
    apply(code);
    Ok(())
}

/// `tr!("Quit", "Keluar")` memilih teks sesuai bahasa aktif;
/// dengan argumen tambahan keduanya dipakai sebagai format string.
macro_rules! tr {
    ($en:literal, $id:literal) => {
        if $crate::i18n::is_id() { $id } else { $en }
    };
    ($en:literal, $id:literal, $($arg:tt)*) => {
        if $crate::i18n::is_id() { format!($id, $($arg)*) } else { format!($en, $($arg)*) }
    };
}
pub(crate) use tr;

/// Seperti `tr!`, tetapi selalu memformat (untuk teks dengan `{var}` langsung di string).
macro_rules! trf {
    ($en:literal, $id:literal) => {
        if $crate::i18n::is_id() { format!($id) } else { format!($en) }
    };
}
pub(crate) use trf;
