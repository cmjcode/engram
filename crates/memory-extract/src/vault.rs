//! Tata letak vault di disk. Satu folder = seluruh memori; backup cukup salin.
//!
//! ```text
//! vault/
//!   db/        SurrealDB (RocksDB)
//!   sources/   teks & file asli, dialamatkan dengan hash
//!   inbox/     taruh file di sini untuk diimport otomatis
//!   exports/   dump JSONL + markdown berversi
//! ```

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
}

impl Vault {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn init(&self) -> std::io::Result<()> {
        for d in [self.db_dir(), self.sources_dir(), self.inbox_dir(), self.inbox_dir().join("processed"), self.inbox_dir().join("failed"), self.exports_dir()] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn db_dir(&self) -> PathBuf {
        self.root.join("db")
    }
    pub fn sources_dir(&self) -> PathBuf {
        self.root.join("sources")
    }
    pub fn inbox_dir(&self) -> PathBuf {
        self.root.join("inbox")
    }
    pub fn exports_dir(&self) -> PathBuf {
        self.root.join("exports")
    }

    /// Path relatif (terhadap root) untuk teks sumber dengan hash tertentu.
    pub fn source_rel_path(hash: &str, ext: &str) -> String {
        format!("sources/{}/{}.{}", &hash[..2.min(hash.len())], hash, ext.trim_start_matches('.'))
    }

    pub fn write_rel(&self, rel: &str, bytes: &[u8]) -> std::io::Result<()> {
        let p = self.root.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if !p.exists() {
            let tmp = p.with_extension("tmp");
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(tmp, &p)?;
        }
        Ok(())
    }

    pub fn read_rel(&self, rel: &str) -> std::io::Result<String> {
        if rel.contains("..") {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "path relatif tidak valid"));
        }
        std::fs::read_to_string(self.root.join(rel))
    }

    pub fn abs(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}
