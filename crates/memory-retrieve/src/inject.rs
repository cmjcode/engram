//! Injeksi konteks saat sesi agent dimulai (hook `SessionStart`) dan konteks per berkas
//! (hook `PreToolUse Read|Edit`).
//!
//! Prinsip *progressive disclosure*: tampilkan APA yang ada dan BIAYA mengambilnya
//! (indeks ber-ID + perkiraan token), bukan isi penuh. Agent memutuskan sendiri apa yang
//! diambil lewat `get_episodes`. Ukuran keluaran DIUKUR, bukan dihitung dari jumlah item:
//! Claude Code memotong stdout hook di atas 10.000 karakter menjadi stub tanpa peringatan,
//! jadi blok dirender ulang sambil membuang bagian termahal sampai muat.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use memory_core::model::*;
use memory_core::text::truncate_chars;
use memory_store::EpisodeFilter;

use crate::{Result, Retriever, index_line, is_event_fact};

#[derive(Debug, Clone, Default)]
pub struct InjectOpts {
    pub project: Option<String>,
    pub client: Option<String>,
    /// Batas karakter; `None` = `inject.max_chars`.
    pub max_chars: Option<usize>,
    pub observations: Option<usize>,
    pub sessions: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct InjectResult {
    pub text: String,
    pub chars: usize,
    pub found: bool,
    pub project: Option<String>,
    /// Observasi yang masuk indeks setelah penyesuaian anggaran.
    pub observations: usize,
    pub sessions: usize,
    /// Berapa kali bagian dibuang agar muat.
    pub reductions: u32,
    /// Masih melebihi batas setelah semua yang bisa dibuang habis (hanya header + 1 item).
    pub over_budget: bool,
}

#[derive(Debug, Clone, Copy)]
struct Plan {
    sessions: usize,
    observations: usize,
    facts: usize,
    lessons: usize,
    principles: usize,
    session_excerpt: usize,
}

struct Parts<'a> {
    subject_name: &'a str,
    project: Option<&'a str>,
    client: Option<&'a str>,
    summaries: &'a [Episode],
    observations: &'a [Episode],
    facts: &'a [crate::FactView],
    lessons: &'a [Lesson],
    principles: &'a [Principle],
}

fn render(p: &Parts<'_>, plan: Plan) -> String {
    let mut out = String::new();
    match p.project {
        Some(pr) => out.push_str(&format!("# engram: memori proyek {pr} (subjek: {})\n", p.subject_name)),
        None => out.push_str(&format!("# engram: memori kerja terbaru (subjek: {})\n", p.subject_name)),
    }
    if let Some(c) = p.client {
        out.push_str(&format!("Client: {c}. "));
    }
    out.push_str("Ini indeks, bukan isi penuh: ambil detail hanya yang relevan dengan get_episodes(ids=[...]); recall(query, project) untuk mencari; remember untuk menyimpan keputusan baru.\n");

    if plan.sessions > 0 && !p.summaries.is_empty() {
        out.push_str("\n## Sesi terakhir\n");
        for e in p.summaries.iter().take(plan.sessions) {
            let date = e.created_at.chars().take(10).collect::<String>();
            out.push_str(&format!("- ep:{} | {date} | {}\n", e.id, if e.title.is_empty() { "(ringkasan sesi)" } else { &e.title }));
            if plan.session_excerpt > 0 {
                for line in truncate_chars(&e.text, plan.session_excerpt).lines().filter(|l| !l.trim().is_empty()) {
                    out.push_str(&format!("    {line}\n"));
                }
            }
        }
    }
    if plan.lessons > 0 && !p.lessons.is_empty() {
        out.push_str("\n## Konvensi & jebakan proyek\n");
        for l in p.lessons.iter().take(plan.lessons) {
            let n = if l.occurrences > 1 { format!(" (x{})", l.occurrences) } else { String::new() };
            out.push_str(&format!("- [{}] {}{n}\n", l.kind.label_id(), l.title));
        }
    }
    if plan.facts > 0 && !p.facts.is_empty() {
        out.push_str("\n## Fakta arsitektur yang berlaku\n");
        for f in p.facts.iter().take(plan.facts) {
            out.push_str(&format!("- {}\n", f.statement));
        }
    }
    if plan.principles > 0 && !p.principles.is_empty() {
        out.push_str("\n## Prinsip kerja\n");
        for pr in p.principles.iter().take(plan.principles) {
            out.push_str(&format!("- {}\n", pr.statement));
        }
    }
    if plan.observations > 0 && !p.observations.is_empty() {
        out.push_str("\n## Indeks observasi terbaru (ID | tanggal | jenis | judul | biaya)\n");
        let mut last_date = String::new();
        for e in p.observations.iter().take(plan.observations) {
            let date = e.created_at.chars().take(10).collect::<String>();
            if date != last_date {
                out.push_str(&format!("### {date}\n"));
                last_date = date.clone();
            }
            let proj = if p.project.is_none() { e.project.as_deref() } else { None };
            out.push_str(&index_line(
                &e.id,
                &e.created_at.chars().take(16).collect::<String>().replace('T', " "),
                e.obs_type.as_deref(),
                proj,
                &e.title,
                e.approx_tokens(),
            ));
            out.push('\n');
        }
    }
    out.push_str("\nJenis bertanda [keputusan], [bugfix], [pelajaran] biasanya paling layak diambil dulu. Fakta diberi status berlaku/dulu oleh engram; jangan mengarang di luar memori.\n");
    out
}

fn reduce(plan: Plan, obs_total: usize) -> Option<Plan> {
    let mut p = plan;
    if p.session_excerpt > 0 {
        p.session_excerpt = 0;
    } else if p.principles > 0 {
        p.principles = 0;
    } else if p.sessions > 1 {
        p.sessions /= 2;
    } else if p.facts > 4 {
        p.facts /= 2;
    } else if p.observations > 8 {
        p.observations /= 2;
    } else if p.lessons > 4 {
        p.lessons /= 2;
    } else if p.facts > 0 {
        p.facts = 0;
    } else if p.sessions > 0 {
        p.sessions = 0;
    } else if p.observations > 1 {
        p.observations = (p.observations / 2).max(1).min(obs_total.max(1));
    } else if p.lessons > 0 {
        p.lessons = 0;
    } else {
        return None;
    }
    Some(p)
}

impl Retriever {
    /// Blok konteks awal sesi, dipaskan ke `max_chars` dengan mengukur hasil render.
    pub async fn inject_context(&self, subject: &str, opts: &InjectOpts) -> Result<InjectResult> {
        let ic = &self.cfg.inject;
        let max_chars = opts.max_chars.unwrap_or(ic.max_chars).clamp(500, 10_000);
        let n_obs = opts.observations.unwrap_or(ic.observations);
        let n_sess = opts.sessions.unwrap_or(ic.sessions);
        let subj = self.store.get_subject(subject).await?;
        let subject_name = subj.as_ref().map(|s| s.name.clone()).unwrap_or_else(|| subject.to_string());
        let project = opts.project.as_deref().map(str::trim).filter(|p| !p.is_empty());
        let names = match project {
            Some(p) => self.store.project_names(subject, p).await?,
            None => vec![],
        };
        if project.is_some() && names.is_empty() {
            return Ok(InjectResult::default());
        }
        let recent_fut = self.store.list_episodes(
            subject,
            &EpisodeFilter {
                status: Some(EpisodeStatus::Active),
                projects: names.clone(),
                newest_first: true,
                limit: (n_obs + n_sess) * 2 + 10,
                ..Default::default()
            },
        );
        let ent_fut = async {
            match project {
                Some(p) => self.find_entity(subject, p).await,
                None => Ok(None),
            }
        };
        let lessons_fut = self.store.list_lessons(subject, &names, Some("active"), 10);
        let princ_fut = async {
            match project {
                Some(p) => Ok(self.principles(subject, Some(p)).await?.into_iter().take(3).map(|v| v.principle).collect::<Vec<_>>()),
                None => Ok(vec![]),
            }
        };
        let (recent, proj_ent, lessons, principles) = tokio::try_join!(recent_fut, ent_fut, lessons_fut, princ_fut)?;

        let summaries: Vec<Episode> =
            recent.iter().filter(|e| e.obs_type.as_deref() == Some("session_summary")).take(n_sess.max(1)).cloned().collect();
        let observations: Vec<Episode> =
            recent.iter().filter(|e| e.obs_type.as_deref() != Some("session_summary")).take(n_obs.max(1)).cloned().collect();
        let mut facts: Vec<crate::FactView> = Vec::new();
        if let Some(ent) = proj_ent
            && let Some(w) = self.who_is_id(subject, &ent.id).await?
        {
            facts = w.current_facts.into_iter().filter(|f| !is_event_fact(&f.category)).take(12).collect();
        }
        let found = !summaries.is_empty() || !observations.is_empty() || !facts.is_empty() || !lessons.is_empty();
        if !found {
            return Ok(InjectResult { project: project.map(str::to_string), ..Default::default() });
        }
        let parts = Parts {
            subject_name: &subject_name,
            project,
            client: opts.client.as_deref(),
            summaries: &summaries,
            observations: &observations,
            facts: &facts,
            lessons: &lessons,
            principles: &principles,
        };
        let mut plan = Plan { sessions: n_sess, observations: n_obs, facts: 8, lessons: 8, principles: 3, session_excerpt: 400 };
        let mut text = render(&parts, plan);
        let mut reductions = 0u32;
        let mut over = false;
        while text.chars().count() > max_chars {
            match reduce(plan, observations.len()) {
                Some(p) => {
                    plan = p;
                    reductions += 1;
                    text = render(&parts, plan);
                }
                None => {
                    over = true;
                    text = crate::fit_text(&text, max_chars);
                    break;
                }
            }
        }
        Ok(InjectResult {
            chars: text.chars().count(),
            text,
            found: true,
            project: project.map(str::to_string),
            observations: plan.observations.min(observations.len()),
            sessions: plan.sessions.min(summaries.len()),
            reductions,
            over_budget: over,
        })
    }

    /// Riwayat satu berkas dari memori (untuk disisipkan sebelum agent membaca/mengubahnya).
    pub async fn file_context(&self, subject: &str, path: &str, limit: usize, max_chars: usize) -> Result<Option<String>> {
        let eps = self.store.episodes_touching_file(subject, path, limit.clamp(1, 30)).await?;
        if eps.is_empty() {
            return Ok(None);
        }
        let mut out = format!("engram: riwayat berkas {path} ({} observasi; detail: get_episodes(ids=[...]))\n", eps.len());
        for e in &eps {
            let changed = if e.files_modified.iter().any(|f| path.ends_with(f.as_str()) || f.ends_with(path)) { " [diubah]" } else { "" };
            out.push_str(&format!(
                "{}{changed}\n",
                index_line(
                    &e.id,
                    &e.created_at.chars().take(10).collect::<String>(),
                    e.obs_type.as_deref(),
                    None,
                    &e.title,
                    e.approx_tokens()
                )
            ));
        }
        Ok(Some(crate::fit_text(&out, max_chars.clamp(300, 10_000))))
    }
}
