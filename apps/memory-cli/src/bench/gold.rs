//! Format gold set evaluasi (`gold.toml`) dan pencarian kasus.
//!
//! Satu kasus = satu folder berisi `gold.toml`, ditambah `source.txt` bila
//! labelnya berupa kutipan. Dua cara melabeli:
//! - `[[important]]`/`[[noise]]` + `source`: kutipan dicari di segmen hasil
//!   segmentasi chat utuh (menguji segmentasi + skor).
//! - `[[segment]]`: potongan teks siap pakai berlabel (hasil `export-gold`
//!   dari antrean review; menguji skor saja).
//!
//! Field lain (`[[fact]]`, `[[qa]]`) disiapkan untuk lapis B/C dan diabaikan di sini.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Gold {
    /// interview | memoir | note | chat | lifelog | email | document
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Nama subjek untuk prompt segmentasi (default: subjek bawaan konfigurasi).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_name: Option<String>,
    /// Berkas sumber relatif terhadap folder kasus (default `source.txt`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub important: Vec<QuoteLabel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub noise: Vec<QuoteLabel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segment: Vec<SegLabel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteLabel {
    pub quote: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegLabel {
    pub text: String,
    /// true = harus jadi episode, false = noise.
    pub important: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

pub struct Case {
    pub name: String,
    pub dir: PathBuf,
    pub gold: Gold,
}

fn load(dir: &Path) -> Result<Case> {
    let p = dir.join("gold.toml");
    let s = std::fs::read_to_string(&p).with_context(|| format!("membaca {}", p.display()))?;
    let gold: Gold = toml::from_str(&s).with_context(|| format!("format {}", p.display()))?;
    let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| dir.display().to_string());
    Ok(Case { name, dir: dir.to_path_buf(), gold })
}

/// `path` boleh folder satu kasus atau folder berisi banyak kasus.
pub fn discover(path: &Path) -> Result<Vec<Case>> {
    if path.join("gold.toml").is_file() {
        return Ok(vec![load(path)?]);
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(path)
        .with_context(|| format!("membaca folder {}", path.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("gold.toml").is_file())
        .collect();
    dirs.sort();
    if dirs.is_empty() {
        bail!("tidak ada gold.toml di {} atau subfoldernya", path.display());
    }
    dirs.iter().map(|d| load(d)).collect()
}
