//! Kedaulatan data: ekspor format terbuka, backup, restore, re-embed.
//!
//! Ekspor berversi berisi:
//! ```text
//! exports/<waktu>/
//!   manifest.json           format, versi skema, dimensi, jumlah record
//!   tables/<tabel>.jsonl    satu record JSON per baris (tanpa embedding)
//!   markdown/<subjek>/...   kisah hidup yang bisa dibaca manusia tanpa software ini
//! ```
//! Backup = ekspor + `sources/` dalam satu `.tar.gz`, opsional dienkripsi
//! dengan `age` (`.tar.gz.age`). Embedding tidak ikut karena bisa dihitung ulang.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde_json::{Value, json};

use memory_core::model::*;
use memory_core::text::normalize;
use memory_store::{EpisodeFilter, TABLES};

use crate::{JobError, Result, Runtime};

const PAGE: usize = 1000;

fn slug(s: &str) -> String {
    let n = normalize(s).replace(' ', "-");
    let n: String = n.chars().take(60).collect();
    if n.is_empty() { "tanpa-judul".into() } else { n }
}

/// Ekspor seluruh vault ke folder baru. Mengembalikan path folder.
pub async fn export(rt: &Runtime, dir: Option<PathBuf>) -> Result<PathBuf> {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let dir = dir.unwrap_or_else(|| rt.vault.exports_dir().join(&stamp));
    let tables_dir = dir.join("tables");
    std::fs::create_dir_all(&tables_dir)?;
    let mut counts = serde_json::Map::new();
    for table in TABLES {
        let mut f = std::io::BufWriter::new(std::fs::File::create(tables_dir.join(format!("{table}.jsonl")))?);
        let mut offset = 0;
        let mut n = 0u64;
        loop {
            let rows = rt.store.dump_table(table, false, PAGE, offset).await?;
            for r in &rows {
                serde_json::to_writer(&mut f, r).map_err(|e| JobError::Invalid(e.to_string()))?;
                f.write_all(b"\n")?;
            }
            n += rows.len() as u64;
            if rows.len() < PAGE {
                break;
            }
            offset += PAGE;
        }
        f.flush()?;
        counts.insert((*table).to_string(), json!(n));
    }
    for subject in rt.store.list_subjects().await? {
        export_markdown(rt, &subject, &dir.join("markdown").join(slug(&subject.id))).await?;
    }
    let manifest = json!({
        "format": "llm-memory-export",
        "schema_version": memory_core::SCHEMA_VERSION,
        "created_at": memory_core::ids::now_rfc3339(),
        "embedding": {"model": rt.cfg.embedding.model, "dimension": rt.cfg.embedding.dimension},
        "counts": counts,
        "note": "Embedding tidak disertakan; hitung ulang dengan `memory reembed` setelah restore.",
    });
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec_pretty(&manifest).unwrap_or_default())?;
    Ok(dir)
}

async fn export_markdown(rt: &Runtime, subject: &Subject, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir.join("episodes"))?;
    let mut readme = format!("# Memori {}\n\n", subject.name);
    if let Some(b) = subject.anchors.birth {
        readme.push_str(&format!("Lahir: {b}\n\n"));
    }
    let phases = rt.store.list_life_phases(&subject.id).await?;
    if !phases.is_empty() {
        readme.push_str("## Fase hidup\n\n");
        for p in &phases {
            readme.push_str(&format!("### {}\n\n{}\n\n", p.name, p.summary));
        }
    }
    let principles = rt.store.list_principles(&subject.id, Some(ReflectiveStatus::Active)).await?;
    if !principles.is_empty() {
        readme.push_str("## Prinsip hidup\n\n");
        for p in &principles {
            let ev: Vec<String> = p.evidence.iter().map(|e| e.episode_id.clone()).collect();
            readme.push_str(&format!("- {} (bukti: {})\n", p.statement, ev.join(", ")));
        }
        readme.push('\n');
    }
    let disp = rt.store.list_dispositions(&subject.id).await?;
    if !disp.is_empty() {
        readme.push_str("## Sifat & nilai\n\n");
        for d in disp.iter().filter(|d| d.status == ReflectiveStatus::Active) {
            readme.push_str(&format!("- **{}** ({}–{}): {}\n", d.label, d.valid_from.as_deref().unwrap_or("?"), d.valid_to.as_deref().unwrap_or("sekarang"), d.description));
        }
        readme.push('\n');
    }
    std::fs::write(dir.join("README.md"), readme)?;
    let mut offset = 0;
    loop {
        let eps = rt
            .store
            .list_episodes(&subject.id, &EpisodeFilter { status: Some(EpisodeStatus::Active), limit: PAGE, offset, ..Default::default() })
            .await?;
        for e in &eps {
            let date = e.time_start.clone().unwrap_or_else(|| "0000-00-00".into());
            let name = format!("{date}-{}-{}.md", slug(&e.title), &e.id[..8.min(e.id.len())]);
            let body = format!(
                "---\nid: {}\ntitle: {:?}\nwaktu: {:?}\nfase: {:?}\nsumber: {}\ndiceritakan_ulang: {}\n---\n\n# {}\n\n{}\n\n## Teks asli\n\n{}\n",
                e.id,
                e.title,
                e.time.label(),
                e.life_phase.clone().unwrap_or_default(),
                e.source_id,
                e.retell_count,
                e.title,
                e.summary,
                e.text
            );
            std::fs::write(dir.join("episodes").join(name), body)?;
        }
        if eps.len() < PAGE {
            break;
        }
        offset += PAGE;
    }
    Ok(())
}

fn add_dir<W: Write>(tar: &mut tar::Builder<W>, src: &Path, name: &str) -> Result<()> {
    if src.exists() {
        tar.append_dir_all(name, src)?;
    }
    Ok(())
}

/// Backup ke satu berkas `.tar.gz` (atau `.tar.gz.age` bila ada penerima age).
pub async fn backup(rt: &Runtime, out: &Path, age_recipient: Option<&str>) -> Result<PathBuf> {
    let export_dir = export(rt, None).await?;
    let recipient = match age_recipient.filter(|r| !r.trim().is_empty()) {
        Some(r) => Some(age::x25519::Recipient::from_str(r.trim()).map_err(|e| JobError::Invalid(format!("penerima age tidak valid: {e}")))?),
        None => None,
    };
    let path = match &recipient {
        Some(_) if !out.to_string_lossy().ends_with(".age") => PathBuf::from(format!("{}.age", out.display())),
        _ => out.to_path_buf(),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let plain = match recipient {
        Some(_) => PathBuf::from(format!("{}.part", path.display())),
        None => path.clone(),
    };
    {
        let gz = flate2::write::GzEncoder::new(std::fs::File::create(&plain)?, flate2::Compression::default());
        let mut tar = tar::Builder::new(gz);
        add_dir(&mut tar, &export_dir, "export")?;
        add_dir(&mut tar, &rt.vault.sources_dir(), "sources")?;
        tar.into_inner()?.finish()?.sync_all()?;
    }
    if let Some(r) = &recipient {
        let enc = age::Encryptor::with_recipients(std::iter::once(r as &dyn age::Recipient)).map_err(|e| JobError::Invalid(e.to_string()))?;
        let mut w = enc.wrap_output(std::fs::File::create(&path)?)?;
        std::io::copy(&mut std::fs::File::open(&plain)?, &mut w)?;
        // finish() menulis potongan terakhir + tag autentikasi.
        w.finish()?.sync_all()?;
        std::fs::remove_file(&plain)?;
    }
    Ok(path)
}

/// Restore dari berkas backup ke vault runtime (biasanya vault baru/kosong).
pub async fn restore(rt: &Runtime, archive: &Path, identity_file: Option<&Path>) -> Result<RestoreReport> {
    let file = std::fs::File::open(archive)?;
    let reader: Box<dyn std::io::Read> = if archive.to_string_lossy().ends_with(".age") {
        let id_path = identity_file.ok_or_else(|| JobError::Invalid("backup terenkripsi: berikan --identity".into()))?;
        let ids = age::IdentityFile::from_file(id_path.to_string_lossy().to_string())?.into_identities().map_err(|e| JobError::Invalid(e.to_string()))?;
        let dec = age::Decryptor::new(BufReader::new(file)).map_err(|e| JobError::Invalid(e.to_string()))?;
        Box::new(dec.decrypt(ids.iter().map(|i| i.as_ref() as &dyn age::Identity)).map_err(|e| JobError::Invalid(e.to_string()))?)
    } else {
        Box::new(file)
    };
    let staging = rt.vault.exports_dir().join(format!("restore-{}", chrono::Utc::now().format("%Y%m%dT%H%M%SZ")));
    std::fs::create_dir_all(&staging)?;
    tar::Archive::new(flate2::read::GzDecoder::new(reader)).unpack(&staging)?;
    // Sumber asli.
    let src = staging.join("sources");
    let mut files = 0u64;
    if src.exists() {
        for entry in walk(&src)? {
            let rel = entry.strip_prefix(&src).map_err(|e| JobError::Invalid(e.to_string()))?;
            let target = rt.vault.sources_dir().join(rel);
            if let Some(p) = target.parent() {
                std::fs::create_dir_all(p)?;
            }
            if !target.exists() {
                std::fs::copy(&entry, &target)?;
                files += 1;
            }
        }
    }
    // Tabel.
    let mut rows = 0u64;
    for table in TABLES {
        let p = staging.join("export").join("tables").join(format!("{table}.jsonl"));
        if !p.exists() {
            continue;
        }
        for line in BufReader::new(std::fs::File::open(&p)?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(&line).map_err(|e| JobError::Invalid(format!("{table}: {e}")))?;
            rt.store.restore_row(table, &v).await?;
            rows += 1;
        }
    }
    let reembedded = reembed(rt).await?;
    Ok(RestoreReport { rows, source_files: files, reembedded })
}

fn walk(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            out.extend(walk(&p)?);
        } else {
            out.push(p);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RestoreReport {
    pub rows: u64,
    pub source_files: u64,
    pub reembedded: u64,
}

/// Hitung ulang semua embedding (setelah restore atau ganti model embedding).
pub async fn reembed(rt: &Runtime) -> Result<u64> {
    let emb = &rt.pipeline.embedder;
    let batch = rt.cfg.embedding.batch_size.max(1);
    let mut n = 0u64;
    for table in ["episode", "entity"] {
        let mut offset = 0;
        loop {
            let rows = rt.store.dump_table(table, false, PAGE, offset).await?;
            for chunk in rows.chunks(batch) {
                let texts: Vec<String> = chunk
                    .iter()
                    .map(|r| match table {
                        "episode" => r["text"].as_str().unwrap_or("").to_string(),
                        _ => serde_json::from_value::<Entity>(r.clone()).map(|e| memory_extract::resolve::entity_embed_text(&e)).unwrap_or_default(),
                    })
                    .collect();
                let vecs = emb.embed(&texts).await?;
                for (r, v) in chunk.iter().zip(vecs) {
                    if let Some(id) = r["id"].as_str() {
                        rt.store.patch(table, id, json!({"embedding": v})).await?;
                        n += 1;
                    }
                }
            }
            if rows.len() < PAGE {
                break;
            }
            offset += PAGE;
        }
    }
    rt.store.rebuild_vector_indexes().await?;
    Ok(n)
}
