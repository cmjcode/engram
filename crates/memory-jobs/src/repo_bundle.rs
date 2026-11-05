//! Bundel per-repositori `.engram/`: pelajaran, keputusan, dan fakta aktif satu proyek
//! dalam JSONL yang bisa di-commit ke repo dan diimpor rekan tim ke vault mereka.
//! Impor menggabungkan berdasarkan kunci stabil dan tidak pernah menghapus.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use memory_core::ids::now_rfc3339;
use memory_core::model::*;
use memory_extract::ingest::DistilledDoc;
use memory_store::EpisodeFilter;

use crate::{JobError, Result, Runtime};

pub const BUNDLE_DIR: &str = ".engram";
pub const FORMAT: &str = "engram-repo-bundle";

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BundleReport {
    pub dir: PathBuf,
    pub project: String,
    pub lessons: u32,
    pub decisions: u32,
    pub facts: u32,
    /// Hanya saat impor.
    pub lessons_new: u32,
    pub lessons_merged: u32,
    pub decisions_new: u32,
    pub decisions_duplicate: u32,
}

fn write_jsonl(path: &Path, rows: &[Value]) -> Result<()> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    for r in rows {
        serde_json::to_writer(&mut f, r).map_err(|e| JobError::Invalid(e.to_string()))?;
        f.write_all(b"\n")?;
    }
    f.flush()?;
    Ok(())
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path).map(|s| s.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()).unwrap_or_default()
}

/// Nama proyek dari path repo bila tidak diberikan.
pub fn project_of(repo: &Path, project: Option<&str>) -> Option<String> {
    project.map(str::to_string).or_else(|| memory_core::project::detect_project(repo).map(|p| p.name))
}

pub async fn export_repo(rt: &Runtime, subject: &str, repo: &Path, project: Option<&str>) -> Result<BundleReport> {
    let project = project_of(repo, project).ok_or_else(|| JobError::Invalid("nama proyek tidak bisa ditentukan; beri --project".into()))?;
    let names = rt.store.project_names(subject, &project).await?;
    let dir = repo.join(BUNDLE_DIR);
    std::fs::create_dir_all(&dir)?;
    let lessons = rt.store.list_lessons(subject, &names, None, 5000).await?;
    let lesson_rows: Vec<Value> = lessons.iter().map(|l| serde_json::to_value(l).unwrap_or_default()).collect();
    write_jsonl(&dir.join("lessons.jsonl"), &lesson_rows)?;

    let eps = rt
        .store
        .list_episodes(subject, &EpisodeFilter { status: Some(EpisodeStatus::Active), projects: names.clone(), newest_first: true, limit: 5000, ..Default::default() })
        .await?;
    let mut decisions: Vec<Value> = Vec::new();
    for e in &eps {
        for d in &e.decisions {
            decisions.push(json!({
                "episode_id": e.id, "title": e.title, "date": e.time_start.clone().unwrap_or_else(|| e.created_at.chars().take(10).collect()),
                "obs_type": e.obs_type, "situation": d.situation, "action": d.action, "reason": d.reason,
            }));
        }
    }
    write_jsonl(&dir.join("decisions.jsonl"), &decisions)?;

    let mut facts: Vec<Value> = Vec::new();
    if let Some(ent) = rt.retriever.find_entity(subject, &project).await?
        && let Some(w) = rt.retriever.who_is_id(subject, &ent.id).await?
    {
        for f in w.current_facts.iter().filter(|f| !memory_retrieve::is_event_fact(&f.category)) {
            facts.push(json!({"id": f.id, "statement": f.statement, "category": f.category, "valid_from": f.valid_from, "evidence": f.evidence}));
        }
    }
    write_jsonl(&dir.join("facts.jsonl"), &facts)?;

    let manifest = json!({
        "format": FORMAT, "version": 1, "project": project, "project_names": names, "subject": subject,
        "exported_at": now_rfc3339(), "schema_version": memory_core::SCHEMA_VERSION,
        "counts": {"lessons": lesson_rows.len(), "decisions": decisions.len(), "facts": facts.len()},
        "note": "Diimpor dengan `engram repo import <repo>`; penggabungan berdasarkan kunci stabil, tidak menghapus.",
    });
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec_pretty(&manifest).unwrap_or_default())?;
    let mut readme = format!("# Memori proyek {project} (engram)\n\nBundel ini dibuat `engram repo export`. Impor: `engram repo import <folder repo>`.\n\n");
    if !lessons.is_empty() {
        readme.push_str("## Konvensi & jebakan\n\n");
        for l in &lessons {
            readme.push_str(&format!("- [{}] {} (x{})\n", l.kind.label_id(), l.title, l.occurrences));
        }
        readme.push('\n');
    }
    if !decisions.is_empty() {
        readme.push_str("## Keputusan\n\n");
        for d in decisions.iter().take(100) {
            readme.push_str(&format!("- {}: {} -> {}\n", d["date"].as_str().unwrap_or("?"), d["situation"].as_str().unwrap_or(""), d["action"].as_str().unwrap_or("")));
        }
    }
    std::fs::write(dir.join("README.md"), readme)?;
    Ok(BundleReport { dir, project, lessons: lesson_rows.len() as u32, decisions: decisions.len() as u32, facts: facts.len() as u32, ..Default::default() })
}

pub async fn import_repo(rt: &Runtime, subject: &str, repo: &Path, project: Option<&str>) -> Result<BundleReport> {
    let dir = if repo.file_name().is_some_and(|n| n == BUNDLE_DIR) { repo.to_path_buf() } else { repo.join(BUNDLE_DIR) };
    let manifest: Value = serde_json::from_slice(&std::fs::read(dir.join("manifest.json"))?).map_err(|e| JobError::Invalid(format!("manifest tidak terbaca: {e}")))?;
    if manifest["format"] != FORMAT {
        return Err(JobError::Invalid("bukan bundel engram".into()));
    }
    let project = project.map(str::to_string).or_else(|| manifest["project"].as_str().map(str::to_string)).ok_or_else(|| JobError::Invalid("proyek tidak diketahui".into()))?;
    let mut rep = BundleReport { dir: dir.clone(), project: project.clone(), ..Default::default() };
    let now = now_rfc3339();
    for row in read_jsonl(&dir.join("lessons.jsonl")) {
        let Ok(mut l) = serde_json::from_value::<Lesson>(row) else { continue };
        rep.lessons += 1;
        l.subject = subject.to_string();
        l.project = Some(project.clone());
        if l.title_key.is_empty() {
            l.title_key = Lesson::title_key_of(&l.title);
        }
        match rt.store.lesson_by_key(subject, Some(&project), &l.title_key).await? {
            Some(mut local) => {
                let mut added = false;
                for ev in &l.evidence {
                    if !local.evidence.iter().any(|x| x.episode_id == ev.episode_id) {
                        local.evidence.push(ev.clone());
                        added = true;
                    }
                }
                if added {
                    local.occurrences = local.occurrences.max(l.occurrences) + 1;
                }
                if local.detail.is_empty() {
                    local.detail = l.detail.clone();
                }
                local.updated_at = now.clone();
                rt.store.put_lesson(&local).await?;
                rep.lessons_merged += 1;
            }
            None => {
                l.id = memory_extract::det_id(&["lesson", subject, &project, &l.title_key]);
                l.promoted_to = None;
                l.status = "active".into();
                l.updated_at = now.clone();
                rt.store.put_lesson(&l).await?;
                rep.lessons_new += 1;
            }
        }
    }
    for row in read_jsonl(&dir.join("decisions.jsonl")) {
        let (sit, act) = (row["situation"].as_str().unwrap_or("").trim(), row["action"].as_str().unwrap_or("").trim());
        if sit.is_empty() || act.is_empty() {
            continue;
        }
        rep.decisions += 1;
        let reason = row["reason"].as_str().map(str::trim).filter(|r| !r.is_empty()).map(|r| format!("\nAlasan: {r}")).unwrap_or_default();
        let text = format!("Keputusan proyek {project} ({}).\nSituasi: {sit}\nKeputusan: {act}{reason}", row["date"].as_str().unwrap_or("tanggal tidak diketahui"));
        let out = rt
            .pipeline
            .ingest_distilled(DistilledDoc {
                subject: subject.to_string(),
                title: memory_core::text::truncate_chars(sit, 100),
                summary: memory_core::text::truncate_chars(act, 300),
                text,
                project: Some(project.clone()),
                original_name: Some("decisions.jsonl".into()),
                meta: json!({"via": "repo-bundle", "from_episode": row["episode_id"], "obs_type": "decision"}),
            })
            .await?;
        if out.duplicate {
            rep.decisions_duplicate += 1;
        } else {
            rep.decisions_new += 1;
        }
    }
    rep.facts = read_jsonl(&dir.join("facts.jsonl")).len() as u32;
    Ok(rep)
}
