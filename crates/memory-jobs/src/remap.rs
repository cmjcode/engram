//! Pemetaan ulang atribusi proyek untuk data lama: memindahkan episode, sesi, sumber, dan
//! pelajaran ke proyek (repo) yang benar, serta mencatat tag grup proyek. Rencana berupa
//! JSON yang bisa ditinjau; tanpa `apply` tidak ada yang ditulis.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use memory_core::ids::is_valid_id;
use memory_core::model::ProjectMeta;

use crate::{JobError, Result, Runtime};

/// Tabel yang punya kolom `project` dan boleh dipetakan ulang.
pub const REMAP_TABLES: &[&str] = &["episode", "session", "source", "lesson"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RemapPlan {
    pub subject: String,
    /// `id -> proyek baru` per tabel; `null` mengosongkan proyek (mis. sesi di folder temp).
    pub episodes: BTreeMap<String, Option<String>>,
    pub sessions: BTreeMap<String, Option<String>>,
    pub sources: BTreeMap<String, Option<String>>,
    pub lessons: BTreeMap<String, Option<String>>,
    /// Tag grup yang ditambahkan ke metadata proyek (`proyek -> [tag]`).
    pub tags: BTreeMap<String, Vec<String>>,
}

impl RemapPlan {
    fn tables(&self) -> [(&'static str, &BTreeMap<String, Option<String>>); 4] {
        [("episode", &self.episodes), ("session", &self.sessions), ("source", &self.sources), ("lesson", &self.lessons)]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemapChange {
    pub table: String,
    pub id: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RemapReport {
    pub applied: bool,
    pub changes: Vec<RemapChange>,
    pub unchanged: usize,
    /// ID yang tidak ada atau milik subjek lain (dilewati).
    pub missing: Vec<String>,
    pub tagged: Vec<String>,
}

fn valid_project(p: &str) -> bool {
    let p = p.trim();
    !p.is_empty() && p.chars().count() <= 200 && !p.contains('\n')
}

/// Jalankan rencana. `apply == false` hanya menghitung perubahan.
pub async fn remap_projects(rt: &Runtime, plan: &RemapPlan, apply: bool) -> Result<RemapReport> {
    if !is_valid_id(&plan.subject) {
        return Err(JobError::Invalid(format!("subject tidak valid: {}", plan.subject)));
    }
    for (table, map) in plan.tables() {
        for (id, to) in map {
            if !is_valid_id(id) {
                return Err(JobError::Invalid(format!("id {table} tidak valid: {id}")));
            }
            if to.as_deref().is_some_and(|t| !valid_project(t)) {
                return Err(JobError::Invalid(format!("nama proyek tidak valid untuk {table}:{id}: {to:?}")));
            }
        }
    }
    let mut rep = RemapReport { applied: apply, ..Default::default() };
    for (table, map) in plan.tables() {
        for (id, to) in map {
            let to = to.as_deref().map(str::trim);
            let row: Option<Value> = rt.store.get(table, id).await?;
            let Some(row) = row.filter(|r| r["subject"].as_str() == Some(plan.subject.as_str())) else {
                rep.missing.push(format!("{table}:{id}"));
                continue;
            };
            let from = row["project"].as_str().map(str::to_string);
            if from.as_deref() == to {
                rep.unchanged += 1;
                continue;
            }
            if apply {
                match to {
                    Some(t) => rt.store.patch(table, id, json!({"project": t})).await?,
                    None => rt.store.execute("UPDATE type::record($tb, $id) SET project = NONE RETURN NONE;", memory_store::binds! {"tb" => table, "id" => id.as_str()}).await.map(|_| ())?,
                }
            }
            rep.changes.push(RemapChange { table: table.to_string(), id: id.clone(), from, to: to.map(str::to_string) });
        }
    }
    for (project, tags) in &plan.tags {
        if !valid_project(project) {
            return Err(JobError::Invalid(format!("nama proyek tidak valid: {project:?}")));
        }
        let prev = rt.store.get_project_meta(&plan.subject, project).await?;
        let mut all = prev.as_ref().map(|m| m.tags.clone()).unwrap_or_default();
        all.extend(tags.iter().cloned());
        let all = memory_core::project::normalize_tags(&all);
        if prev.as_ref().is_some_and(|m| m.tags == all) || all.is_empty() {
            continue;
        }
        if apply {
            let name = prev.as_ref().map(|m| m.project.clone()).unwrap_or_else(|| project.trim().to_string());
            let m = ProjectMeta::build(
                &plan.subject,
                &name,
                prev.as_ref().and_then(|m| m.repo.as_deref()),
                prev.as_ref().map(|m| m.notes.as_str()).unwrap_or(""),
                &all,
                prev.as_ref(),
            );
            rt.store.put_project_meta(&m).await?;
        }
        rep.tagged.push(format!("{project}: {}", all.join(", ")));
    }
    Ok(rep)
}
