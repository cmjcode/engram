//! `bench export-gold`: ubah keputusan review GrayZone (Approved/Rejected)
//! menjadi gold set `[[segment]]`, sehingga tidak perlu melabeli dari nol.

use std::path::Path;

use anyhow::{Context, Result, bail};

use memory_core::MemoryConfig;
use memory_core::model::{ReviewKind, ReviewStatus};
use memory_jobs::Runtime;

use super::gold::{Gold, SegLabel};

pub async fn run(cfg: MemoryConfig, subject: &str, out: &Path, limit: usize) -> Result<()> {
    let path = out.join("gold.toml");
    if path.exists() {
        bail!("{} sudah ada; pilih folder lain agar label manual tidak tertimpa", path.display());
    }
    let rt = Runtime::open(cfg).await.context("membuka vault (hentikan `engram serve`/aplikasi desktop dulu bila database terkunci)")?;
    let name = rt.store.get_subject(subject).await?.map(|s| s.name).unwrap_or_else(|| subject.to_string());
    let mut segment = Vec::new();
    for (status, important) in [(ReviewStatus::Approved, true), (ReviewStatus::Rejected, false)] {
        for r in rt.store.list_reviews(subject, status, Some(ReviewKind::GrayZone), limit).await? {
            let text = r.payload["text"].as_str().unwrap_or(&r.summary).trim().to_string();
            if !text.is_empty() {
                segment.push(SegLabel { text, important, note: r.note.clone() });
            }
        }
    }
    if segment.is_empty() {
        bail!("subjek '{subject}' belum punya review GrayZone yang diputuskan (approve/reject di `engram review`)");
    }
    let (pos, neg) = (segment.iter().filter(|s| s.important).count(), segment.iter().filter(|s| !s.important).count());
    let gold = Gold { kind: Some("chat".into()), subject_name: Some(name), segment, ..Default::default() };
    std::fs::create_dir_all(out)?;
    let header = "# Diekspor dari antrean review GrayZone. Berisi teks chat asli — jangan di-commit.\n# Semua item ini dulunya berskor di zona abu-abu, jadi ini kasus perbatasan.\n\n";
    std::fs::write(&path, format!("{header}{}", toml::to_string_pretty(&gold)?))?;
    println!("{} ditulis: {pos} penting, {neg} noise.", path.display());
    if pos == 0 || neg == 0 {
        println!("Peringatan: hanya satu jenis label; tambahkan kasus lain agar tuning seimbang.");
    }
    Ok(())
}
