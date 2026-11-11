//! Konteks grup proyek: saat satu repo ditanyakan, sertakan peta seluruh proyek yang
//! berbagi tag grup (mis. `#VOLTUNES`), arsitektur bersama dari proyek payung,
//! konvensi lintas repo, dan sesi terbaru di grup.

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use memory_core::model::*;
use memory_core::project::{ProjectGroup, project_variants, same_project};
use memory_core::text::{semantic_text_similarity, truncate_chars};
use memory_store::EpisodeFilter;

use crate::recall::{FactView, fact_view};
use crate::tools::EpisodeRef;
use crate::{Result, Retriever};

/// Ringkasan satu repo anggota grup.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct GroupMember {
    pub project: String,
    /// Peran/arsitektur singkat repo (fakta "adalah/berfungsi" terkini, atau judul sesi terbaru).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Tech stack terkini.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stack: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// Episode sumber ringkasan (ambil detail arsitektur repo lewat `get_episodes`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode: Option<String>,
}

/// Konteks seluruh grup proyek yang ditempelkan ke `project_context`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct GroupContext {
    #[serde(flatten)]
    pub group: ProjectGroup,
    /// Anggota grup beserta ringkasannya (tanpa proyek yang ditanyakan).
    pub member_info: Vec<GroupMember>,
    /// Fakta terkini proyek payung (arsitektur bersama ekosistem).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared_facts: Vec<FactView>,
    /// Keputusan teknis dari proyek payung.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared_decisions: Vec<Decision>,
    /// Konvensi/jebakan dari repo lain di grup.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lessons: Vec<Lesson>,
    /// Sesi terbaru di repo lain dalam grup.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub episodes: Vec<EpisodeRef>,
}

impl GroupContext {
    pub fn has_data(&self) -> bool {
        !self.member_info.is_empty() || !self.shared_facts.is_empty() || !self.shared_decisions.is_empty()
    }
}

/// Nama anggota + payung, beserta variannya, untuk filter `project IN [...]`.
pub fn group_names(g: &ProjectGroup) -> Vec<String> {
    project_variants(g.members.iter().map(String::as_str).chain(g.umbrella.as_deref()))
}

fn short_stack(object: &str) -> String {
    let o = object.trim();
    let o = o.strip_prefix("tool ").unwrap_or(o);
    o.trim_matches(|c| c == '`' || c == '.').to_string()
}

fn is_role_fact(f: &Fact) -> bool {
    let p = format!(" {} ", f.predicate.to_lowercase());
    let o = f.object_text.to_lowercase();
    // "merupakan bagian dari VOLTUNES" tidak menjelaskan peran repo.
    if o.starts_with("bagian dari") || p.contains(" bagian dari ") || o.split_whitespace().count() < 3 {
        return false;
    }
    [" adalah ", " berfungsi ", " merupakan ", " bertugas ", " melayani "].iter().any(|k| p.contains(k))
}

/// Skor episode sebagai sumber ringkasan peran repo: judul bernuansa arsitektur/ikhtisar dulu.
fn overview_score(title: &str, obs_type: Option<&str>) -> u8 {
    let t = title.to_lowercase();
    let kw = ["arsitektur", "stack", "struktur", "ikhtisar", "overview", "ekosistem", "teknologi", "gambaran"];
    let mut s = if kw.iter().any(|k| t.contains(k)) { 2 } else { 0 };
    if obs_type.is_none() || obs_type == Some("session_summary") {
        s += 1;
    }
    s
}

/// Ringkasan episode yang diawali potongan kode/kalimat (hasil pemotongan teks) kurang layak.
fn starts_cleanly(text: &str) -> bool {
    text.trim_start_matches(['*', '_', ' ']).chars().next().is_some_and(|c| c.is_uppercase())
}

/// Rapikan ringkasan untuk satu baris peta repo: buang markdown, path lokal & tautan berkas,
/// dan nama subjek di awal kalimat ("Budi mengembangkan ..." -> "mengembangkan ...").
fn clean_summary(text: &str, self_name: Option<&str>) -> String {
    let mut out = String::new();
    let mut rest = text;
    // Segmen berbacktick yang berupa path/URL berkas dibuang, sisanya dipertahankan tanpa backtick.
    while let Some(start) = rest.find('`') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('`') {
            Some(end) => {
                let inner = &after[..end];
                if !(inner.starts_with('/') || inner.starts_with("file:") || inner.starts_with("~/")) {
                    out.push_str(inner);
                }
                rest = &after[end + 1..];
            }
            None => {
                rest = after;
            }
        }
    }
    out.push_str(rest);
    let mut t = out.replace("**", "").replace("()", "").replace("( )", "");
    for marker in ["(berlokasi di )", "(berlokasi di)", " yang berlokasi di ,", " yang berlokasi di "] {
        t = t.replace(marker, " ");
    }
    let mut t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(n) = self_name.map(str::trim).filter(|n| !n.is_empty())
        && let Some(r) = t.strip_prefix(n)
    {
        t = r.trim_start().to_string();
    }
    t
}

fn first_sentence(text: &str, max: usize) -> String {
    let t = text.trim();
    let cut = t.char_indices().find(|(i, c)| *c == '.' && t[i + 1..].starts_with(' ') && *i > 20).map(|(i, _)| &t[..i]).unwrap_or(t);
    truncate_chars(cut.trim(), max)
}

impl Retriever {
    /// Kumpulkan konteks grup untuk proyek `proj` (`None` bila proyek tidak tergabung dalam grup).
    /// `own_lesson_ids` dipakai untuk membuang pelajaran yang sudah tampil di konteks proyek sendiri.
    pub async fn group_context(
        &self,
        subject: &str,
        proj: &str,
        metas: &[ProjectMeta],
        cwds: &[String],
        own_lesson_ids: &HashSet<String>,
    ) -> Result<Option<GroupContext>> {
        let Some(group) = memory_core::project::resolve_group(metas, proj, cwds.iter().map(String::as_str)) else {
            return Ok(None);
        };
        let names = group_names(&group);
        if names.is_empty() {
            return Ok(Some(GroupContext { group, ..Default::default() }));
        }
        let lookup: Vec<String> = group.members.iter().cloned().chain(group.umbrella.clone()).collect();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

        let umbrella_names: Vec<String> = project_variants(group.umbrella.as_deref());
        let (ents, lessons, briefs, umbrella_eps) = tokio::try_join!(
            async { Ok::<_, crate::RetrieveError>(self.store.entities_by_aliases(subject, &lookup).await?) },
            async { Ok::<_, crate::RetrieveError>(self.store.list_lessons(subject, &names, Some("active"), 60).await?) },
            async { Ok::<_, crate::RetrieveError>(self.store.episode_briefs(subject, &names, 3000).await?) },
            async {
                if umbrella_names.is_empty() {
                    return Ok::<_, crate::RetrieveError>(vec![]);
                }
                Ok(self
                    .store
                    .list_episodes(
                        subject,
                        &EpisodeFilter { status: Some(EpisodeStatus::Active), projects: umbrella_names.clone(), limit: 20, newest_first: true, ..Default::default() },
                    )
                    .await?)
            },
        )?;
        let ents: Vec<Entity> =
            ents.into_iter().filter(|e| matches!(e.kind, EntityKind::Project | EntityKind::Org) && !same_project(&e.name, proj)).collect();
        // Entitas proyek diutamakan atas organisasi bernama mirip (mis. proyek VOLTUNES vs PT Voltunes).
        let ent_of = |name: &str| -> Option<&Entity> {
            let by_name = |k: Option<EntityKind>| ents.iter().find(|e| k.is_none_or(|k| e.kind == k) && same_project(&e.name, name));
            let by_alias = |k: Option<EntityKind>| ents.iter().find(|e| k.is_none_or(|k| e.kind == k) && e.aliases.iter().any(|a| same_project(a, name)));
            by_name(Some(EntityKind::Project)).or_else(|| by_alias(Some(EntityKind::Project))).or_else(|| by_name(None)).or_else(|| by_alias(None))
        };
        let self_name = self.store.self_entity(subject).await?.map(|e| e.name);
        let ids: Vec<String> = ents.iter().map(|e| e.id.clone()).collect();
        let facts: Vec<Fact> = self
            .store
            .facts_for_entities(&ids)
            .await?
            .into_iter()
            .filter(|f| f.status == FactStatus::Active && f.valid_at(&today) && ids.contains(&f.entity_id))
            .collect();

        let mut member_info: Vec<GroupMember> = Vec::new();
        for m in &group.members {
            let ent = ent_of(m);
            let mine: Vec<&Fact> = ent.map(|e| facts.iter().filter(|f| f.entity_id == e.id).collect()).unwrap_or_default();
            let role = mine
                .iter()
                .filter(|f| is_role_fact(f))
                .max_by(|a, b| a.valid_from.cmp(&b.valid_from).then(a.object_text.len().cmp(&b.object_text.len())))
                .map(|f| format!("{} {}", f.predicate.trim(), f.object_text.trim()));
            let mut stack: Vec<String> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            for f in mine.iter().filter(|f| f.category == "tech_stack") {
                let s = short_stack(&f.object_text);
                if !s.is_empty() && s.chars().count() <= 40 && !s.contains("github.com/") && seen.insert(memory_core::text::normalize(&s)) {
                    stack.push(s);
                }
            }
            stack.truncate(6);
            // Ringkasan: fakta peran bila ada, selain itu kalimat pertama episode ikhtisar repo itu.
            let best_ep = briefs
                .iter()
                .enumerate()
                .filter(|(_, b)| b.project.as_deref().is_some_and(|p| same_project(p, m)))
                .max_by_key(|(i, b)| {
                    let clean = !b.summary.trim().is_empty() && starts_cleanly(&b.summary);
                    (u8::from(clean) * 4 + overview_score(&b.title, b.obs_type.as_deref()), std::cmp::Reverse(*i))
                })
                .map(|(_, b)| b);
            let (summary, episode) = match role {
                Some(r) => (Some(truncate_chars(&r, 140)), None),
                None => match best_ep {
                    Some(b) => {
                        let text = if b.summary.trim().is_empty() || !starts_cleanly(&b.summary) {
                            b.title.trim().to_string()
                        } else {
                            first_sentence(&clean_summary(&b.summary, self_name.as_deref()), 140)
                        };
                        (Some(text), Some(b.id.clone()))
                    }
                    None => (None, None),
                },
            };
            let repo = metas.iter().find(|x| same_project(&x.project, m)).and_then(|x| x.repo.clone());
            member_info.push(GroupMember { project: m.clone(), summary: summary.filter(|s| !s.is_empty()), stack, repo, episode });
        }

        let mut shared_facts: Vec<FactView> = Vec::new();
        let mut shared_decisions: Vec<Decision> = Vec::new();
        if let Some(u) = group.umbrella.as_deref() {
            if let Some(ue) = ent_of(u) {
                let mut uf: Vec<&Fact> =
                    facts.iter().filter(|f| f.entity_id == ue.id && f.category != "komponen" && !crate::is_event_fact(&f.category)).collect();
                uf.sort_by(|a, b| b.valid_from.cmp(&a.valid_from));
                shared_facts = uf.into_iter().take(12).map(|f| fact_view(f, &ue.name, &today)).collect();
            }
            for e in &umbrella_eps {
                for d in &e.decisions {
                    if shared_decisions.len() < 5 && !shared_decisions.iter().any(|x| x.situation == d.situation && x.action == d.action) {
                        shared_decisions.push(d.clone());
                    }
                }
            }
        }

        // Ragamkan: paling banyak 3 pelajaran dan 2 sesi per repo, agar satu repo aktif tidak mendominasi.
        let mut lessons: Vec<Lesson> = lessons.into_iter().filter(|l| !own_lesson_ids.contains(&l.id)).collect();
        lessons.sort_by_key(|l| std::cmp::Reverse(l.occurrences));
        let mut per: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        lessons.retain(|l| {
            let c = per.entry(l.project.clone().unwrap_or_default().to_lowercase()).or_default();
            *c += 1;
            *c <= 3
        });
        lessons.truncate(10);

        per.clear();
        let recent_ids: Vec<String> = briefs
            .iter()
            .filter(|b| {
                let c = per.entry(b.project.clone().unwrap_or_default().to_lowercase()).or_default();
                *c += 1;
                *c <= 2
            })
            .take(10)
            .map(|b| b.id.clone())
            .collect();
        let mut recent = self.store.episodes_by_ids(&recent_ids).await?;
        recent.sort_by_key(|e| recent_ids.iter().position(|i| *i == e.id));
        let episodes: Vec<EpisodeRef> = recent.iter().map(EpisodeRef::from).collect();
        Ok(Some(GroupContext { group, member_info, shared_facts, shared_decisions, lessons, episodes }))
    }
}

/// Tulis bagian "Ekosistem grup" ke konteks memakai `push` (berhenti bila anggaran habis).
pub fn render_group(g: &GroupContext, own: &str, task: Option<&str>, push: &mut dyn FnMut(&str) -> bool) {
    let tags = g.group.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ");
    let total = g.member_info.len() + usize::from(g.group.umbrella.is_some());
    push(&format!("\n## Ekosistem grup {tags} ({total} proyek lain selain {own}; detail repo lain: project_context(project=\"<nama>\"))"));
    if let Some(u) = g.group.umbrella.as_deref()
        && (!g.shared_facts.is_empty() || !g.shared_decisions.is_empty())
    {
        push(&format!("### Arsitektur bersama (proyek payung {u})"));
        for f in &g.shared_facts {
            let cite = f.evidence.first().map(|e| format!(" [ep:{e}]")).unwrap_or_default();
            if !push(&format!("- {}{cite}", f.statement)) {
                return;
            }
        }
        for d in &g.shared_decisions {
            let reason = d.reason.as_deref().map(|r| format!(" (alasan: {r})")).unwrap_or_default();
            if !push(&format!("- Keputusan: {} -> {}{reason}", d.situation, d.action)) {
                return;
            }
        }
    }
    if !g.member_info.is_empty() {
        let line = |m: &GroupMember| -> String {
            let mut l = format!("- {}", m.project);
            if let Some(s) = &m.summary {
                l.push_str(&format!(": {s}"));
            }
            if !m.stack.is_empty() {
                l.push_str(&format!(" | {}", m.stack.join(", ")));
            }
            if let Some(e) = &m.episode {
                l.push_str(&format!(" [ep:{e}]"));
            }
            l
        };
        let mut members: Vec<&GroupMember> = g.member_info.iter().collect();
        match task.map(str::trim).filter(|t| !t.is_empty()) {
            // Dengan tugas: repo paling relevan dulu agar tidak terpotong anggaran.
            Some(t) => members.sort_by(|a, b| semantic_text_similarity(t, &line(b)).total_cmp(&semantic_text_similarity(t, &line(a)))),
            // Tanpa tugas: repo yang punya ringkasan dulu, lalu urut nama.
            None => members.sort_by_key(|m| (m.summary.is_none() && m.stack.is_empty(), m.project.to_lowercase())),
        }
        push("### Peta repo dalam grup");
        for (i, m) in members.iter().enumerate() {
            if !push(&line(m)) {
                let rest: Vec<&str> = members[i..].iter().map(|m| m.project.as_str()).collect();
                push(&format!("- (+{} lainnya: {})", rest.len(), rest.join(", ")));
                return;
            }
        }
    }
    if !g.lessons.is_empty() {
        push("### Konvensi & jebakan lintas repo");
        for l in &g.lessons {
            let p = l.project.as_deref().map(|p| format!(" ({p})")).unwrap_or_default();
            if !push(&format!("- [{}] {}{p}", l.kind.label_id(), l.title)) {
                return;
            }
        }
    }
    if !g.episodes.is_empty() {
        push("### Sesi terbaru di grup (get_episodes(ids=[...]))");
        for ep in &g.episodes {
            let date = ep.time_start.as_deref().unwrap_or(&ep.time);
            if !push(&crate::index_line(&ep.id, date, ep.obs_type.as_deref(), ep.project.as_deref(), &ep.title, ep.approx_tokens)) {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_summary_strips_markdown_paths_and_self_name() {
        let raw = "**Budi Santoso** mengembangkan layanan `api-x` (berlokasi di `/Users/budi/PROJECT/api-x`) memakai Go.";
        assert_eq!(clean_summary(raw, Some("Budi Santoso")), "mengembangkan layanan api-x memakai Go.");
        assert!(starts_cleanly("**Budi** membangun"));
        assert!(!starts_cleanly("yml` (dengan zona waktu"));
        assert_eq!(first_sentence("Layanan Go 1.23 untuk stok. Kalimat kedua.", 100), "Layanan Go 1.23 untuk stok");
    }
}
