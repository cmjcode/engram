//! Repository per tabel. Semua SurrealQL ada di sini; crate lain hanya
//! memakai method bertipe.

use serde_json::{Value, json};

use memory_core::model::*;
use memory_core::text::normalize;

use crate::{Result, Store, StoreError, binds};

const EP: &str = "SELECT * OMIT embedding FROM episode";
const EN: &str = "SELECT * OMIT embedding, alias_keys FROM entity";

/// Baris ringan episode (tanpa teks/embedding) untuk meringkas banyak proyek sekaligus.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EpisodeBrief {
    pub id: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub title: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub summary: String,
    #[serde(default)]
    pub obs_type: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub created_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct EpisodeFilter {
    pub status: Option<EpisodeStatus>,
    pub source_id: Option<String>,
    pub project: Option<String>,
    /// Cocok salah satu nama (alias/varian proyek); mengalahkan `project` bila terisi.
    pub projects: Vec<String>,
    /// Hanya episode dengan `obs_type` ini (mis. "session_summary").
    pub obs_type: Option<String>,
    pub session_id: Option<String>,
    /// Urutkan dari yang terbaru dibuat (bukan kronologi cerita).
    pub newest_first: bool,
    pub life_phase: Option<String>,
    /// Jendela waktu (ISO date) yang dipotong rentang episode.
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

/// `null` (kolom yang dipilih eksplisit tetapi tidak ada di record lama) dibaca sebagai nilai default.
fn null_as_default<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + serde::Deserialize<'de>,
{
    Ok(<Option<T> as serde::Deserialize>::deserialize(d)?.unwrap_or_default())
}

/// Baris ringan episode untuk graf/backlink proyek: tanpa teks dan embedding. Episode dari
/// sebelum field `links` ada mengembalikan `links: null` pada SELECT berkolom, bukan field kosong.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EpisodeLinkRow {
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub entity_ids: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub links: Vec<String>,
    /// Waktu episode dibuat (ISO 8601), untuk "terakhir aktif" di graf. Kosong bila tidak ada.
    #[serde(default, deserialize_with = "null_as_default")]
    pub created_at: String,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SubjectStats {
    pub sources: u64,
    pub episodes: u64,
    pub retellings: u64,
    pub stream_items: u64,
    pub entities: u64,
    pub facts: u64,
    pub kin: u64,
    pub principles: u64,
    pub dispositions: u64,
    pub open_reviews: u64,
}

fn enum_str<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

pub fn alias_keys(name: &str, aliases: &[String]) -> Vec<String> {
    let mut keys: Vec<String> = std::iter::once(name).chain(aliases.iter().map(String::as_str)).map(normalize).filter(|k| !k.is_empty()).collect();
    keys.sort();
    keys.dedup();
    keys
}

/// Dokumen entitas siap tulis (dengan `alias_keys` untuk pencarian alias).
pub fn entity_doc(e: &Entity) -> Value {
    let mut v = serde_json::to_value(e).unwrap_or_default();
    v["alias_keys"] = json!(alias_keys(&e.name, &e.aliases));
    v
}

impl Store {
    // ---------------------------------------------------------------- subject
    pub async fn put_subject(&self, s: &Subject) -> Result<()> {
        self.upsert("subject", &s.id, s, None).await
    }

    pub async fn get_subject(&self, id: &str) -> Result<Option<Subject>> {
        self.get("subject", id).await
    }

    pub async fn list_subjects(&self) -> Result<Vec<Subject>> {
        self.rows_as("SELECT * FROM subject ORDER BY name;", binds! {}).await
    }

    // ----------------------------------------------------------------- source
    pub async fn put_source(&self, s: &Source) -> Result<()> {
        self.upsert("source", &s.id, s, None).await
    }

    pub async fn get_source(&self, id: &str) -> Result<Option<Source>> {
        self.get("source", id).await
    }

    pub async fn source_by_hash(&self, subject: &str, hash: &str) -> Result<Option<Source>> {
        Ok(self
            .rows_as("SELECT * FROM source WHERE subject = $s AND hash = $h LIMIT 1;", binds! {"s" => subject, "h" => hash})
            .await?
            .pop())
    }

    pub async fn list_sources(&self, subject: &str, status: Option<SourceStatus>, limit: usize, offset: usize) -> Result<Vec<Source>> {
        let (cond, st) = match status {
            Some(s) => (" AND status = $st", enum_str(&s)),
            None => ("", String::new()),
        };
        self.rows_as(
            &format!("SELECT * FROM source WHERE subject = $s{cond} ORDER BY created_at DESC LIMIT {limit} START {offset};"),
            binds! {"s" => subject, "st" => st},
        )
        .await
    }

    /// Sumber berstatus gagal di semua subjek, terlama dulu.
    pub async fn failed_sources(&self, limit: usize) -> Result<Vec<Source>> {
        self.rows_as(&format!("SELECT * FROM source WHERE status = 'failed' ORDER BY created_at ASC LIMIT {limit};"), binds! {}).await
    }

    pub async fn sources_for_batch(&self, batch_id: &str) -> Result<Vec<Source>> {
        self.rows_as("SELECT * FROM source WHERE batch_id = $b ORDER BY created_at;", binds! {"b" => batch_id}).await
    }

    pub async fn set_source_status(&self, id: &str, status: SourceStatus, error: Option<String>) -> Result<()> {
        self.patch("source", id, json!({"status": status, "error": error})).await
    }

    pub async fn set_source_stats(&self, id: &str, stats: &SourceStats) -> Result<()> {
        self.patch("source", id, json!({"stats": stats})).await
    }

    // ------------------------------------------------------------ stream_item
    pub async fn put_stream_item(&self, s: &StreamItem) -> Result<()> {
        self.upsert("stream_item", &s.id, s, None).await
    }

    pub async fn stream_items(&self, subject: &str, only_open: bool, limit: usize) -> Result<Vec<StreamItem>> {
        let cond = if only_open { " AND (promoted_to = NONE OR promoted_to = NULL)" } else { "" };
        self.rows_as(
            &format!("SELECT * FROM stream_item WHERE subject = $s{cond} ORDER BY created_at DESC LIMIT {limit};"),
            binds! {"s" => subject},
        )
        .await
    }

    /// Hapus item working memory yang kedaluwarsa dan tidak dipromosikan.
    pub async fn delete_expired_stream(&self, now: &str) -> Result<u64> {
        let rows = self
            .rows(
                "DELETE stream_item WHERE expires_at < $now AND (promoted_to = NONE OR promoted_to = NULL) RETURN BEFORE;",
                binds! {"now" => now},
            )
            .await?;
        Ok(rows.len() as u64)
    }

    // ---------------------------------------------------------------- episode
    pub async fn put_episode(&self, e: &Episode, embedding: Option<&[f32]>) -> Result<()> {
        self.upsert("episode", &e.id, e, embedding).await
    }

    pub async fn get_episode(&self, id: &str) -> Result<Option<Episode>> {
        self.get("episode", id).await
    }

    pub async fn episodes_by_ids(&self, ids: &[String]) -> Result<Vec<Episode>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let mut out: Vec<Episode> = self
            .rows_as("SELECT * OMIT embedding FROM array::map($ids, |$i| type::record('episode', $i));", binds! {"ids" => ids})
            .await?;
        // Pertahankan urutan masukan.
        out.sort_by_key(|e| ids.iter().position(|i| i == &e.id).unwrap_or(usize::MAX));
        Ok(out)
    }

    pub async fn list_episodes(&self, subject: &str, f: &EpisodeFilter) -> Result<Vec<Episode>> {
        let mut cond = String::from("subject = $s");
        let mut b = binds! {"s" => subject};
        if let Some(st) = f.status {
            cond.push_str(" AND status = $st");
            b.insert("st".into(), json!(st));
        }
        if let Some(src) = &f.source_id {
            cond.push_str(" AND source_id = $src");
            b.insert("src".into(), json!(src));
        }
        if !f.projects.is_empty() {
            cond.push_str(" AND project IN $projs");
            b.insert("projs".into(), json!(f.projects));
        } else if let Some(proj) = &f.project {
            cond.push_str(" AND project = $proj");
            b.insert("proj".into(), json!(proj));
        }
        if let Some(ot) = &f.obs_type {
            cond.push_str(" AND obs_type = $ot");
            b.insert("ot".into(), json!(ot));
        }
        if let Some(sid) = &f.session_id {
            cond.push_str(" AND session_id = $sid");
            b.insert("sid".into(), json!(sid));
        }
        if let Some(lp) = &f.life_phase {
            cond.push_str(" AND life_phase = $lp");
            b.insert("lp".into(), json!(lp));
        }
        if let Some(to) = &f.to {
            cond.push_str(" AND time_start != NONE AND time_start <= $to");
            b.insert("to".into(), json!(to));
        }
        if let Some(from) = &f.from {
            cond.push_str(" AND (time_ongoing = true OR (time_end != NONE AND time_end >= $from) OR (time_end = NONE AND time_start != NONE AND time_start >= $from))");
            b.insert("from".into(), json!(from));
        }
        let limit = if f.limit == 0 { 100 } else { f.limit };
        let order = if f.newest_first { "created_at DESC" } else { "time_start ASC, created_at ASC" };
        self.rows_as(
            &format!("{EP} WHERE {cond} ORDER BY {order} LIMIT {limit} START {};", f.offset),
            b,
        )
        .await
    }

    /// Episode di sekitar satu episode jangkar menurut urutan dibuat (untuk tool
    /// `timeline(anchor)`): `before` sebelum dan `after` sesudahnya, dalam proyek yang
    /// sama bila `projects` terisi. Hasil urut kronologis, jangkar disertakan.
    pub async fn episodes_around(&self, subject: &str, anchor: &Episode, projects: &[String], before: usize, after: usize) -> Result<Vec<Episode>> {
        let mut cond = String::from("subject = $s AND status = 'active'");
        let mut b = binds! {"s" => subject, "at" => anchor.created_at.clone(), "aid" => anchor.id.clone()};
        if !projects.is_empty() {
            cond.push_str(" AND project IN $projs");
            b.insert("projs".into(), json!(projects));
        }
        let mut prev: Vec<Episode> = self
            .rows_as(&format!("{EP} WHERE {cond} AND created_at <= $at AND id != type::record('episode', $aid) ORDER BY created_at DESC LIMIT {};", before.max(1)), b.clone())
            .await?;
        let next: Vec<Episode> = self
            .rows_as(&format!("{EP} WHERE {cond} AND created_at >= $at AND id != type::record('episode', $aid) ORDER BY created_at ASC LIMIT {};", after.max(1)), b)
            .await?;
        prev.reverse();
        prev.truncate(before);
        let mut out = prev;
        out.push(anchor.clone());
        out.extend(next.into_iter().take(after));
        Ok(out)
    }

    /// Episode yang menyentuh berkas (dibaca/diubah). `path` dicocokkan persis dan
    /// sebagai akhiran path (relatif vs absolut).
    pub async fn episodes_touching_file(&self, subject: &str, path: &str, limit: usize) -> Result<Vec<Episode>> {
        let p = path.replace('\\', "/");
        let tail = p.rsplit('/').next().unwrap_or(&p).to_string();
        let rows: Vec<Episode> = self
            .rows_as(
                &format!(
                    "{EP} WHERE subject = $s AND status = 'active' AND (array::any(files_modified, |$f| string::ends_with($f, $tail)) OR array::any(files_read, |$f| string::ends_with($f, $tail))) ORDER BY created_at DESC LIMIT {};",
                    (limit * 3).max(10)
                ),
                binds! {"s" => subject, "tail" => tail},
            )
            .await?;
        let matches = |f: &str| {
            let f = f.replace('\\', "/");
            f == p || f.ends_with(&format!("/{p}")) || p.ends_with(&format!("/{f}")) || (f.len() > 3 && p.ends_with(&f))
        };
        Ok(rows.into_iter().filter(|e| e.files_modified.iter().chain(e.files_read.iter()).any(|f| matches(f))).take(limit).collect())
    }

    // ---------------------------------------------------------------- session
    pub async fn put_session(&self, s: &Session) -> Result<()> {
        self.upsert("session", &s.id, s, None).await
    }

    pub async fn get_session(&self, id: &str) -> Result<Option<Session>> {
        self.get("session", id).await
    }

    pub async fn list_sessions(&self, subject: &str, projects: &[String], limit: usize) -> Result<Vec<Session>> {
        let mut cond = String::from("subject = $s");
        let mut b = binds! {"s" => subject};
        if !projects.is_empty() {
            cond.push_str(" AND project IN $projs");
            b.insert("projs".into(), json!(projects));
        }
        self.rows_as(&format!("SELECT * FROM session WHERE {cond} ORDER BY last_activity_at DESC LIMIT {limit};"), b).await
    }

    // ----------------------------------------------------------- project_meta
    pub async fn put_project_meta(&self, m: &ProjectMeta) -> Result<()> {
        self.upsert("project_meta", &m.id, m, None).await
    }

    pub async fn list_project_meta(&self, subject: &str) -> Result<Vec<ProjectMeta>> {
        self.rows_as("SELECT * FROM project_meta WHERE subject = $s ORDER BY project ASC;", binds! {"s" => subject}).await
    }

    /// Metadata proyek `name`: kunci persis lebih dulu, lalu varian (`BACKEND/api-x` ~ `api-x`).
    pub async fn get_project_meta(&self, subject: &str, name: &str) -> Result<Option<ProjectMeta>> {
        if let Some(m) = self.get::<ProjectMeta>("project_meta", &memory_core::project::project_meta_id(subject, name)).await? {
            return Ok(Some(m));
        }
        Ok(self.list_project_meta(subject).await?.into_iter().find(|m| memory_core::project::same_project(&m.project, name)))
    }

    /// Proyek yang catatannya menautkan `[[name]]` (backlink), dari daftar metadata `all`.
    pub fn project_backlinks(all: &[ProjectMeta], name: &str) -> Vec<String> {
        all.iter()
            .filter(|m| !memory_core::project::same_project(&m.project, name))
            .filter(|m| m.links.iter().any(|l| memory_core::project::same_project(l, name)))
            .map(|m| m.project.clone())
            .collect()
    }

    /// Proyek, entitas, dan tautan `[[...]]` dari semua episode aktif berproyek milik subjek.
    pub async fn episode_link_rows(&self, subject: &str) -> Result<Vec<EpisodeLinkRow>> {
        let rows: Vec<EpisodeLinkRow> = self
            .rows_as(
                "SELECT project, entity_ids, links, created_at FROM episode WHERE subject = $s AND status = 'active' AND project != NONE AND project != NULL;",
                binds! {"s" => subject},
            )
            .await?;
        Ok(rows.into_iter().filter(|r| r.project.as_deref().is_some_and(|p| !p.trim().is_empty())).collect())
    }

    /// Proyek yang episodenya menautkan `[[name]]` (backlink dari memori agent/sesi), unik per kunci proyek.
    pub fn episode_backlinks(rows: &[EpisodeLinkRow], name: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in rows {
            let Some(p) = r.project.as_deref() else { continue };
            if memory_core::project::same_project(p, name) || !r.links.iter().any(|l| memory_core::project::same_project(l, name)) {
                continue;
            }
            if !out.iter().any(|o| memory_core::project::same_project(o, p)) {
                out.push(p.to_string());
            }
        }
        out.sort_by_key(|p| p.to_lowercase());
        out
    }

    pub async fn delete_project_meta(&self, id: &str) -> Result<()> {
        self.delete("project_meta", id).await
    }

    // ----------------------------------------------------------------- lesson
    pub async fn put_lesson(&self, l: &Lesson) -> Result<()> {
        self.upsert("lesson", &l.id, l, None).await
    }

    pub async fn get_lesson(&self, id: &str) -> Result<Option<Lesson>> {
        self.get("lesson", id).await
    }

    pub async fn lesson_by_key(&self, subject: &str, project: Option<&str>, title_key: &str) -> Result<Option<Lesson>> {
        let mut rows: Vec<Lesson> = match project {
            Some(p) => self.rows_as("SELECT * FROM lesson WHERE subject = $s AND project = $p AND title_key = $k LIMIT 1;", binds! {"s" => subject, "p" => p, "k" => title_key}).await?,
            None => self.rows_as("SELECT * FROM lesson WHERE subject = $s AND (project = NONE OR project = NULL) AND title_key = $k LIMIT 1;", binds! {"s" => subject, "k" => title_key}).await?,
        };
        Ok(rows.pop())
    }

    pub async fn list_lessons(&self, subject: &str, projects: &[String], status: Option<&str>, limit: usize) -> Result<Vec<Lesson>> {
        let mut cond = String::from("subject = $s");
        let mut b = binds! {"s" => subject};
        if !projects.is_empty() {
            cond.push_str(" AND project IN $projs");
            b.insert("projs".into(), json!(projects));
        }
        if let Some(st) = status {
            cond.push_str(" AND status = $st");
            b.insert("st".into(), json!(st));
        }
        self.rows_as(&format!("SELECT * FROM lesson WHERE {cond} ORDER BY occurrences DESC, updated_at DESC LIMIT {limit};"), b).await
    }

    /// Judul & ringkasan episode aktif dari proyek-proyek `projects` (terbaru dulu), tanpa teks.
    pub async fn episode_briefs(&self, subject: &str, projects: &[String], limit: usize) -> Result<Vec<EpisodeBrief>> {
        if projects.is_empty() {
            return Ok(vec![]);
        }
        self.rows_as(
            &format!(
                "SELECT id, project, title, summary, obs_type, created_at FROM episode WHERE subject = $s AND status = 'active' AND project IN $projs ORDER BY created_at DESC LIMIT {limit};"
            ),
            binds! {"s" => subject, "projs" => projects},
        )
        .await
    }

    pub async fn episodes_mentioning(&self, entity_id: &str, limit: usize) -> Result<Vec<Episode>> {
        self.rows_as(
            &format!("{EP} WHERE entity_ids CONTAINS $e AND status != 'retelling' ORDER BY time_start ASC LIMIT {limit};"),
            binds! {"e" => entity_id},
        )
        .await
    }

    /// Ambil episode yang menyebut salah satu dari `entity_ids` dalam satu query (batch).
    pub async fn episodes_mentioning_any(&self, entity_ids: &[String], limit: usize) -> Result<Vec<Episode>> {
        if entity_ids.is_empty() {
            return Ok(vec![]);
        }
        self.rows_as(
            &format!("{EP} WHERE entity_ids CONTAINSANY $eids AND status != 'retelling' ORDER BY time_start ASC LIMIT {limit};"),
            binds! {"eids" => entity_ids},
        )
        .await
    }

    /// Full-text BM25: (id, skor). Token query disaring dari kata penghubung umum dan
    /// dideduplikasi agar BM25 tidak memindai seluruh tabel akibat stopword frekuensi tinggi.
    pub async fn fulltext_episodes(&self, subject: &str, query: &str, limit: usize) -> Result<Vec<(String, f32)>> {
        const STOP: &[&str] = &[
            "yang", "dan", "di", "ke", "dari", "untuk", "dengan", "pada", "adalah", "ini", "itu",
            "atau", "juga", "dalam", "akan", "bisa", "ada", "tidak", "sudah", "saya", "kami", "kita",
            "the", "and", "for", "with", "from", "that", "this", "are", "was", "were", "have", "has",
        ];
        let all_toks = memory_core::text::tokens(query);
        if all_toks.is_empty() {
            return Ok(vec![]);
        }
        let mut seen = std::collections::HashSet::new();
        let filtered: Vec<&str> = all_toks
            .iter()
            .map(String::as_str)
            .filter(|w| w.chars().count() >= 2 && !STOP.contains(w) && seen.insert(*w))
            .take(8)
            .collect();
        let effective_q = if filtered.is_empty() {
            all_toks.iter().take(6).cloned().collect::<Vec<_>>().join(" ")
        } else {
            filtered.join(" ")
        };
        let rows = self
            .rows(
                &format!(
                    "SELECT id, search::score(1) AS score FROM episode WHERE text @1@ $q AND subject = $s AND status = 'active' ORDER BY score DESC LIMIT {limit};"
                ),
                binds! {"q" => effective_q, "s" => subject},
            )
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| Some((r["id"].as_str()?.to_string(), r["score"].as_f64().unwrap_or(0.0) as f32)))
            .collect())
    }

    pub async fn count_episodes(&self, subject: &str) -> Result<u64> {
        self.count("episode", "subject = $s AND status = 'active'", binds! {"s" => subject}).await
    }

    // ----------------------------------------------------------------- entity
    pub async fn put_entity(&self, e: &Entity, embedding: Option<&[f32]>) -> Result<()> {
        self.upsert("entity", &e.id, &entity_doc(e), embedding).await
    }

    pub async fn get_entity(&self, id: &str) -> Result<Option<Entity>> {
        Ok(self
            .rows_as("SELECT * OMIT embedding, alias_keys FROM type::record('entity', $id);", binds! {"id" => id})
            .await?
            .pop())
    }

    /// Entitas untuk daftar ID (yang tidak ada dilewati; urutan tidak dijamin).
    pub async fn entities_by_ids(&self, ids: &[String]) -> Result<Vec<Entity>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        self.rows_as("SELECT * OMIT embedding, alias_keys FROM array::map($ids, |$i| type::record('entity', $i));", binds! {"ids" => ids}).await
    }

    /// Ikuti rantai `merged_into` sampai entitas aktif.
    pub async fn resolve_entity(&self, id: &str) -> Result<Option<Entity>> {
        let mut cur = self.get_entity(id).await?;
        for _ in 0..8 {
            match &cur {
                Some(e) if e.status == EntityStatus::Merged => match &e.merged_into {
                    Some(next) => cur = self.get_entity(next).await?,
                    None => break,
                },
                _ => break,
            }
        }
        Ok(cur)
    }

    pub async fn entities_by_alias(&self, subject: &str, name: &str) -> Result<Vec<Entity>> {
        let key = normalize(name);
        self.rows_as(
            &format!("{EN} WHERE subject = $s AND alias_keys CONTAINS $k AND status != 'merged';"),
            binds! {"s" => subject, "k" => key},
        )
        .await
    }

    /// Cari entitas yang cocok dengan salah satu dari banyak kandidat alias sekaligus (batch).
    pub async fn entities_by_aliases(&self, subject: &str, names: &[String]) -> Result<Vec<Entity>> {
        let mut keys: Vec<String> = names.iter().map(|n| normalize(n)).filter(|k| !k.is_empty()).collect();
        keys.sort();
        keys.dedup();
        if keys.is_empty() {
            return Ok(vec![]);
        }
        self.rows_as(
            &format!("{EN} WHERE subject = $s AND alias_keys CONTAINSANY $keys AND status != 'merged';"),
            binds! {"s" => subject, "keys" => keys},
        )
        .await
    }

    /// Semua nama yang merujuk proyek `name`: nama entitas proyek beserta alias-aliasnya
    /// (dan varian huruf kecil / segmen terakhir), untuk filter `project IN [...]`.
    pub async fn project_names(&self, subject: &str, name: &str) -> Result<Vec<String>> {
        let mut names: Vec<String> = vec![name.trim().to_string()];
        for e in self.entities_by_alias(subject, name).await? {
            if e.kind == EntityKind::Project || e.kind == EntityKind::Org {
                names.push(e.name.clone());
                names.extend(e.aliases.iter().cloned());
            }
        }
        Ok(memory_core::project::project_variants(names.iter().map(String::as_str)))
    }

    pub async fn list_entities(&self, subject: &str, kind: Option<EntityKind>, limit: usize, offset: usize) -> Result<Vec<Entity>> {
        let (cond, k) = match kind {
            Some(k) => (" AND kind = $k", enum_str(&k)),
            None => ("", String::new()),
        };
        self.rows_as(
            &format!("{EN} WHERE subject = $s AND status != 'merged'{cond} ORDER BY mention_count DESC, name ASC LIMIT {limit} START {offset};"),
            binds! {"s" => subject, "k" => k},
        )
        .await
    }

    pub async fn self_entity(&self, subject: &str) -> Result<Option<Entity>> {
        Ok(self
            .rows_as(&format!("{EN} WHERE subject = $s AND is_self = true AND status != 'merged' LIMIT 1;"), binds! {"s" => subject})
            .await?
            .pop())
    }

    // ------------------------------------------------------------------- fact
    pub async fn put_fact(&self, f: &Fact) -> Result<()> {
        self.upsert("fact", &f.id, f, None).await
    }

    pub async fn get_fact(&self, id: &str) -> Result<Option<Fact>> {
        self.get("fact", id).await
    }

    /// Semua fakta di mana entitas menjadi pelaku atau objek, urut waktu.
    pub async fn facts_for_entity(&self, entity_id: &str) -> Result<Vec<Fact>> {
        self.rows_as(
            "SELECT * FROM fact WHERE (entity_id = $e OR object_entity = $e) AND status != 'rejected' ORDER BY valid_from ASC, observed_at ASC;",
            binds! {"e" => entity_id},
        )
        .await
    }

    /// Semua fakta untuk banyak entitas sekaligus (batch) agar `recall` tidak melakukan N query berurutan.
    pub async fn facts_for_entities(&self, entity_ids: &[String]) -> Result<Vec<Fact>> {
        if entity_ids.is_empty() {
            return Ok(vec![]);
        }
        self.rows_as(
            "SELECT * FROM fact WHERE (entity_id IN $eids OR object_entity IN $eids) AND status != 'rejected' ORDER BY valid_from ASC, observed_at ASC;",
            binds! {"eids" => entity_ids},
        )
        .await
    }

    /// Fakta aktif dengan (entity, category) yang sama: kandidat rekonsiliasi.
    pub async fn facts_in_category(&self, entity_id: &str, category: &str) -> Result<Vec<Fact>> {
        self.rows_as(
            "SELECT * FROM fact WHERE entity_id = $e AND category = $c AND status IN ['active', 'conflict'] ORDER BY valid_from ASC;",
            binds! {"e" => entity_id, "c" => category},
        )
        .await
    }

    pub async fn list_facts(&self, subject: &str, status: Option<FactStatus>, limit: usize) -> Result<Vec<Fact>> {
        let (cond, st) = match status {
            Some(s) => (" AND status = $st", enum_str(&s)),
            None => ("", String::new()),
        };
        self.rows_as(
            &format!("SELECT * FROM fact WHERE subject = $s{cond} ORDER BY created_at DESC LIMIT {limit};"),
            binds! {"s" => subject, "st" => st},
        )
        .await
    }

    pub async fn facts_by_episode(&self, episode_id: &str) -> Result<Vec<Fact>> {
        self.rows_as("SELECT * FROM fact WHERE evidence.episode_id CONTAINS $e;", binds! {"e" => episode_id}).await
    }

    /// Alihkan semua rujukan entitas `from` ke `into` (setelah merge disetujui).
    pub async fn repoint_entity(&self, from: &str, into: &str) -> Result<()> {
        self.execute(
            "BEGIN TRANSACTION;
             UPDATE fact SET entity_id = $into WHERE entity_id = $from RETURN NONE;
             UPDATE fact SET object_entity = $into WHERE object_entity = $from RETURN NONE;
             UPDATE kin SET from = $into WHERE from = $from RETURN NONE;
             UPDATE kin SET to = $into WHERE to = $from RETURN NONE;
             UPDATE episode SET entity_ids = array::union(array::complement(entity_ids, [$from]), [$into]) WHERE entity_ids CONTAINS $from RETURN NONE;
             COMMIT TRANSACTION;",
            binds! {"from" => from, "into" => into},
        )
        .await?;
        Ok(())
    }

    // -------------------------------------------------------------------- kin
    pub async fn put_kin(&self, k: &Kin) -> Result<()> {
        self.upsert("kin", &k.id, k, None).await
    }

    pub async fn kin_for_subject(&self, subject: &str) -> Result<Vec<Kin>> {
        self.rows_as("SELECT * FROM kin WHERE subject = $s;", binds! {"s" => subject}).await
    }

    pub async fn kin_touching(&self, entity_id: &str) -> Result<Vec<Kin>> {
        self.rows_as("SELECT * FROM kin WHERE from = $e OR to = $e;", binds! {"e" => entity_id}).await
    }

    pub async fn kin_touching_any(&self, entity_ids: &[String]) -> Result<Vec<Kin>> {
        if entity_ids.is_empty() {
            return Ok(vec![]);
        }
        self.rows_as("SELECT * FROM kin WHERE from IN $eids OR to IN $eids;", binds! {"eids" => entity_ids}).await
    }

    // ------------------------------------------------------------- reflective
    pub async fn put_principle(&self, p: &Principle) -> Result<()> {
        self.upsert("principle", &p.id, p, None).await
    }

    pub async fn list_principles(&self, subject: &str, status: Option<ReflectiveStatus>) -> Result<Vec<Principle>> {
        let (cond, st) = match status {
            Some(s) => (" AND status = $st", enum_str(&s)),
            None => (" AND status != 'rejected'", String::new()),
        };
        self.rows_as(
            &format!("SELECT * FROM principle WHERE subject = $s{cond} ORDER BY confidence DESC;"),
            binds! {"s" => subject, "st" => st},
        )
        .await
    }

    pub async fn put_disposition(&self, d: &Disposition) -> Result<()> {
        self.upsert("disposition", &d.id, d, None).await
    }

    pub async fn list_dispositions(&self, subject: &str) -> Result<Vec<Disposition>> {
        self.rows_as(
            "SELECT * FROM disposition WHERE subject = $s AND status != 'rejected' ORDER BY label ASC, valid_from ASC;",
            binds! {"s" => subject},
        )
        .await
    }

    pub async fn put_life_phase(&self, p: &LifePhase) -> Result<()> {
        self.upsert("life_phase", &p.id, p, None).await
    }

    pub async fn list_life_phases(&self, subject: &str) -> Result<Vec<LifePhase>> {
        self.rows_as("SELECT * FROM life_phase WHERE subject = $s ORDER BY start ASC;", binds! {"s" => subject}).await
    }

    pub async fn put_voice_profile(&self, v: &VoiceProfile) -> Result<()> {
        self.upsert("voice_profile", &v.subject, v, None).await
    }

    pub async fn get_voice_profile(&self, subject: &str) -> Result<Option<VoiceProfile>> {
        self.get("voice_profile", subject).await
    }

    // ----------------------------------------------------------------- review
    pub async fn put_review(&self, r: &ReviewItem) -> Result<()> {
        self.upsert("review_item", &r.id, r, None).await
    }

    pub async fn get_review(&self, id: &str) -> Result<Option<ReviewItem>> {
        self.get("review_item", id).await
    }

    pub async fn list_reviews(&self, subject: &str, status: ReviewStatus, kind: Option<ReviewKind>, limit: usize) -> Result<Vec<ReviewItem>> {
        let (cond, k) = match kind {
            Some(k) => (" AND kind = $k", enum_str(&k)),
            None => ("", String::new()),
        };
        self.rows_as(
            &format!("SELECT * FROM review_item WHERE subject = $s AND status = $st{cond} ORDER BY created_at ASC LIMIT {limit};"),
            binds! {"s" => subject, "st" => status, "k" => k},
        )
        .await
    }

    /// Keputusan review terbaru untuk dipakai sebagai contoh few-shot.
    pub async fn review_examples(&self, subject: &str, kind: ReviewKind, limit: usize) -> Result<Vec<ReviewItem>> {
        self.rows_as(
            &format!("SELECT * FROM review_item WHERE subject = $s AND kind = $k AND status != 'open' ORDER BY resolved_at DESC LIMIT {limit};"),
            binds! {"s" => subject, "k" => kind},
        )
        .await
    }

    // ------------------------------------------------------------------ batch
    pub async fn put_batch(&self, b: &ImportBatch) -> Result<()> {
        self.upsert("import_batch", &b.id, b, None).await
    }

    pub async fn get_batch(&self, id: &str) -> Result<Option<ImportBatch>> {
        self.get("import_batch", id).await
    }

    pub async fn list_batches(&self, subject: &str, limit: usize) -> Result<Vec<ImportBatch>> {
        self.rows_as(
            &format!("SELECT * FROM import_batch WHERE subject = $s ORDER BY created_at DESC LIMIT {limit};"),
            binds! {"s" => subject},
        )
        .await
    }

    /// Naikkan penghitung batch secara atomik. `field`: done | failed | duplicates | total.
    pub async fn batch_incr(&self, id: &str, field: &str, failure: Option<String>) -> Result<()> {
        if !matches!(field, "done" | "failed" | "duplicates" | "total") {
            return Err(StoreError::Invalid(format!("field batch tidak dikenal: {field}")));
        }
        let extra = if failure.is_some() { ", failures += $msg" } else { "" };
        self.execute(
            &format!("UPDATE type::record('import_batch', $id) SET {field} += 1, updated_at = $now{extra} RETURN NONE;"),
            binds! {"id" => id, "now" => memory_core::ids::now_rfc3339(), "msg" => failure},
        )
        .await?;
        Ok(())
    }

    // -------------------------------------------------------------------- job
    /// Masukkan job secara idempoten. Bila kunci sudah ada: job gagal diantre
    /// ulang, selain itu job lama dikembalikan. `(job, baru?)`.
    pub async fn enqueue(&self, job: &Job) -> Result<(Job, bool)> {
        if let Some(existing) = self
            .rows_as::<Job>("SELECT * FROM job WHERE idempotency_key = $k LIMIT 1;", binds! {"k" => job.idempotency_key})
            .await?
            .pop()
        {
            if existing.status == JobStatus::Failed {
                self.patch("job", &existing.id, json!({"status": "queued", "attempts": 0, "error": null, "run_after": memory_core::ids::now_rfc3339()}))
                    .await?;
                let again = self.require::<Job>("job", &existing.id).await?;
                return Ok((again, true));
            }
            return Ok((existing, false));
        }
        self.upsert("job", &job.id, job, None).await?;
        Ok((job.clone(), true))
    }

    /// Ambil satu job siap jalan secara atomik (compare-and-set status).
    pub async fn claim_job(&self, now: &str, kinds: Option<&[&str]>) -> Result<Option<Job>> {
        let kind_cond = if kinds.is_some() { " AND kind IN $kinds" } else { "" };
        let candidates = self
            .rows(
                &format!("SELECT id FROM job WHERE status = 'queued' AND run_after <= $now{kind_cond} ORDER BY created_at ASC LIMIT 8;"),
                binds! {"now" => now, "kinds" => kinds},
            )
            .await?;
        for c in candidates {
            let Some(id) = c["id"].as_str() else { continue };
            let claimed: Vec<Job> = self
                .rows_as(
                    "UPDATE type::record('job', $id) SET status = 'running', attempts += 1, updated_at = $now WHERE status = 'queued' RETURN AFTER;",
                    binds! {"id" => id, "now" => now},
                )
                .await?;
            if let Some(j) = claimed.into_iter().next() {
                return Ok(Some(j));
            }
        }
        Ok(None)
    }

    pub async fn finish_job(&self, job: &Job, result: std::result::Result<(), String>) -> Result<()> {
        let now = memory_core::ids::now_rfc3339();
        let patch = match result {
            Ok(()) => json!({"status": "done", "error": null, "updated_at": now}),
            Err(e) if job.attempts < job.max_attempts => {
                // Backoff eksponensial: 30 dtk, 60 dtk, 120 dtk, ...
                let delay = 30i64 * 2i64.pow(job.attempts.saturating_sub(1).min(6));
                let run_after = (chrono::Utc::now() + chrono::Duration::seconds(delay)).format("%Y-%m-%dT%H:%M:%SZ").to_string();
                json!({"status": "queued", "error": e, "updated_at": now, "run_after": run_after})
            }
            Err(e) => json!({"status": "failed", "error": e, "updated_at": now}),
        };
        self.patch("job", &job.id, patch).await
    }

    pub async fn job_progress(&self, id: &str, stage: &str, done: u32, total: u32) -> Result<()> {
        self.patch("job", id, json!({"stage": stage, "progress_done": done, "progress_total": total, "updated_at": memory_core::ids::now_rfc3339()}))
            .await
    }

    pub async fn list_jobs(&self, status: Option<JobStatus>, limit: usize) -> Result<Vec<Job>> {
        let (cond, st) = match status {
            Some(s) => ("WHERE status = $st ", enum_str(&s)),
            None => ("", String::new()),
        };
        self.rows_as(&format!("SELECT * FROM job {cond}ORDER BY created_at DESC LIMIT {limit};"), binds! {"st" => st}).await
    }

    pub async fn jobs_for_batch(&self, batch_id: &str) -> Result<Vec<Job>> {
        self.rows_as("SELECT * FROM job WHERE batch_id = $b ORDER BY created_at;", binds! {"b" => batch_id}).await
    }

    /// Saat start: job `running` yang tertinggal (proses mati) diantre ulang.
    pub async fn requeue_stale_jobs(&self) -> Result<u64> {
        let rows = self
            .rows("UPDATE job SET status = 'queued' WHERE status = 'running' RETURN AFTER;", binds! {})
            .await?;
        Ok(rows.len() as u64)
    }

    pub async fn pending_job_count(&self) -> Result<u64> {
        self.count("job", "status IN ['queued', 'running']", binds! {}).await
    }

    // ------------------------------------------------------------------ stats
    pub async fn stats(&self, subject: &str) -> Result<SubjectStats> {
        let b = || binds! {"s" => subject};
        Ok(SubjectStats {
            sources: self.count("source", "subject = $s", b()).await?,
            episodes: self.count("episode", "subject = $s AND status = 'active'", b()).await?,
            retellings: self.count("episode", "subject = $s AND status = 'retelling'", b()).await?,
            stream_items: self.count("stream_item", "subject = $s", b()).await?,
            entities: self.count("entity", "subject = $s AND status != 'merged'", b()).await?,
            facts: self.count("fact", "subject = $s AND status = 'active'", b()).await?,
            kin: self.count("kin", "subject = $s", b()).await?,
            principles: self.count("principle", "subject = $s AND status = 'active'", b()).await?,
            dispositions: self.count("disposition", "subject = $s AND status = 'active'", b()).await?,
            open_reviews: self.count("review_item", "subject = $s AND status = 'open'", b()).await?,
        })
    }

    // ---------------------------------------------------------- export/import
    /// Semua baris sebuah tabel (tanpa embedding bila `with_embeddings` false).
    pub async fn dump_table(&self, table: &str, with_embeddings: bool, limit: usize, offset: usize) -> Result<Vec<Value>> {
        if !crate::TABLES.contains(&table) {
            return Err(StoreError::Invalid(format!("tabel tidak dikenal: {table}")));
        }
        let omit = if with_embeddings { "" } else { " OMIT embedding" };
        self.rows(&format!("SELECT *{omit} FROM {table} ORDER BY id LIMIT {limit} START {offset};"), binds! {}).await
    }

    pub async fn restore_row(&self, table: &str, row: &Value) -> Result<()> {
        if !crate::TABLES.contains(&table) {
            return Err(StoreError::Invalid(format!("tabel tidak dikenal: {table}")));
        }
        let id = row["id"].as_str().ok_or_else(|| StoreError::Invalid("baris tanpa id".into()))?.to_string();
        self.upsert(table, &id, row, None).await
    }
}
