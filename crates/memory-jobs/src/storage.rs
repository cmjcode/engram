//! Lokasi penyimpanan vault: pemeriksaan folder tujuan dan pemindahan vault.
//!
//! Vault hanya boleh dipindah saat database tertutup (aplikasi desktop
//! melakukannya saat dibuka ulang, CLI sebelum membuka vault).

use std::path::{Path, PathBuf};

use crate::{JobError, Result};

/// Nama subfolder yang dibuat bila folder pilihan pengguna sudah berisi berkas lain.
pub const VAULT_DIR_NAME: &str = "engram-vault";

/// Folder ini berisi vault engram (ada database-nya).
pub fn is_vault(dir: &Path) -> bool {
    // `db-skv/` = vault v0.4 (SurrealKV); `db/` = vault v0.3 (RocksDB) yang belum dimigrasi.
    dir.join("db-skv").is_dir() || dir.join("db").is_dir()
}

/// Folder yang disinkron layanan cloud. Database embedded rusak bila disinkron
/// berkas-per-berkas (salinan setengah jadi, dua komputer menulis bersamaan).
pub fn looks_like_cloud_folder(path: &Path) -> bool {
    // Pisah manual agar path gaya Windows juga dikenali di macOS/Linux (dan sebaliknya).
    path.to_string_lossy().split(['/', '\\']).any(|c| {
        let s = c.to_lowercase();
        s == "cloudstorage"
            || s.starts_with("googledrive")
            || s == "google drive"
            || s == "my drive"
            || s == "drive saya"
            || s.starts_with("dropbox")
            || s.starts_with("onedrive")
            || s == "icloud drive"
            || s == "mobile documents"
    })
}

/// Rencana pemindahan vault.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MovePlan {
    /// Lokasi vault yang akan dipakai.
    pub target: PathBuf,
    /// Tujuan sudah berisi vault: cukup pakai vault itu, tanpa menyalin.
    pub use_existing: bool,
}

/// Periksa folder tujuan pemindahan vault dari `from`.
///
/// Folder kosong/baru dipakai langsung; folder yang sudah berisi berkas lain
/// mendapat subfolder [`VAULT_DIR_NAME`]; folder yang berisi vault engram dipakai apa adanya.
pub fn plan_move(from: &Path, to: &Path) -> Result<MovePlan> {
    if !to.is_absolute() {
        return Err(JobError::Invalid("lokasi harus berupa path lengkap (absolut)".into()));
    }
    if looks_like_cloud_folder(to) {
        return Err(JobError::Invalid(
            "jangan taruh database di folder Google Drive/Dropbox/OneDrive/iCloud: database bisa rusak. Pakai fitur Sinkronisasi untuk berbagi data antar komputer".into(),
        ));
    }
    let mut target = to.to_path_buf();
    if target.is_dir() && !is_vault(&target) && !dir_is_empty(&target)? {
        target = target.join(VAULT_DIR_NAME);
    }
    if target.exists() && !target.is_dir() {
        return Err(JobError::Invalid(format!("{} bukan folder", target.display())));
    }
    let (from_abs, to_abs) = (canonical(from), canonical(&target));
    if from_abs == to_abs {
        return Err(JobError::Invalid("lokasi baru sama dengan lokasi sekarang".into()));
    }
    if to_abs.starts_with(&from_abs) || from_abs.starts_with(&to_abs) {
        return Err(JobError::Invalid("lokasi baru tidak boleh berada di dalam vault lama (atau sebaliknya)".into()));
    }
    let use_existing = is_vault(&target);
    if !use_existing && target.is_dir() && !dir_is_empty(&target)? {
        return Err(JobError::Invalid(format!("folder {} tidak kosong", target.display())));
    }
    Ok(MovePlan { target, use_existing })
}

fn dir_is_empty(dir: &Path) -> Result<bool> {
    Ok(std::fs::read_dir(dir)?.next().is_none())
}

/// Path absolut sejauh yang ada di disk (bagian yang belum ada ditempel apa adanya).
fn canonical(p: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut cur = p.to_path_buf();
    loop {
        if let Ok(c) = cur.canonicalize() {
            return missing.into_iter().rev().fold(c, |acc: PathBuf, part| acc.join(part));
        }
        match (cur.file_name().map(|n| n.to_os_string()), cur.parent()) {
            (Some(name), Some(parent)) => {
                missing.push(name);
                cur = parent.to_path_buf();
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// Salin vault `from` ke `plan.target` (kecuali tujuan sudah berisi vault).
/// Vault lama tidak dihapus. Mengembalikan jumlah berkas yang disalin.
pub fn copy_vault(from: &Path, plan: &MovePlan) -> Result<u64> {
    // Tujuan sudah berisi vault, atau vault lama belum pernah dibuat: cukup pindah penunjuknya.
    if plan.use_existing || !is_vault(from) {
        return Ok(0);
    }
    let created = !plan.target.exists();
    let res = copy_dir(from, &plan.target);
    if res.is_err() && created {
        let _ = std::fs::remove_dir_all(&plan.target);
    }
    res
}

fn copy_dir(from: &Path, to: &Path) -> Result<u64> {
    std::fs::create_dir_all(to)?;
    let mut n = 0;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let ty = e.file_type()?;
        let dst = to.join(e.file_name());
        if ty.is_dir() {
            n += copy_dir(&e.path(), &dst)?;
        } else if ty.is_file() {
            std::fs::copy(e.path(), &dst)?;
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_folders_are_rejected() {
        assert!(looks_like_cloud_folder(Path::new("/Users/a/Library/CloudStorage/GoogleDrive-a@b.c/My Drive/x")));
        assert!(looks_like_cloud_folder(Path::new("G:\\My Drive\\engram")));
        assert!(!looks_like_cloud_folder(Path::new("/Users/a/Documents/engram")));
        let d = tempfile::tempdir().unwrap();
        let err = plan_move(d.path(), Path::new("/Users/a/Library/CloudStorage/GoogleDrive-x/My Drive/v")).unwrap_err();
        assert!(err.to_string().contains("Sinkronisasi"));
    }

    #[test]
    fn move_copies_into_empty_or_subfolder_and_reuses_existing_vault() {
        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("old");
        std::fs::create_dir_all(from.join("db")).unwrap();
        std::fs::create_dir_all(from.join("sources/ab")).unwrap();
        std::fs::write(from.join("db/CURRENT"), "x").unwrap();
        std::fs::write(from.join("sources/ab/f.txt"), "cerita").unwrap();

        // Folder baru.
        let plan = plan_move(&from, &root.path().join("new")).unwrap();
        assert!(!plan.use_existing);
        assert_eq!(copy_vault(&from, &plan).unwrap(), 2);
        assert_eq!(std::fs::read_to_string(plan.target.join("sources/ab/f.txt")).unwrap(), "cerita");

        // Folder berisi berkas lain -> subfolder.
        let docs = root.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("lain.txt"), "x").unwrap();
        assert_eq!(plan_move(&from, &docs).unwrap().target, docs.join(VAULT_DIR_NAME));

        // Folder yang sudah berisi vault dipakai apa adanya.
        let p = plan_move(&from, &root.path().join("new")).unwrap();
        assert!(p.use_existing);

        // Tidak boleh ke dalam vault sendiri.
        assert!(plan_move(&from, &from.join("x")).is_err());
        assert!(plan_move(&from, &from).is_err());
    }
}
