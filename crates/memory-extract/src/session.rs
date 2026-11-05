//! Sesi kerja agent AI (hook `engram hook <client> <event>`):
//!
//! - `session_init`: prompt pengguna masuk (UserPromptSubmit) → record sesi + spool prompt.
//! - `record_tool_use`: tiap pemanggilan tool (PostToolUse) → baris di spool giliran
//!   (I/O mentah dipotong 64 KB, rahasia diredaksi, gambar dibuang di sisi hook).
//! - `summarize_turn`: giliran selesai (Stop/PreCompact) → spool disusun menjadi satu
//!   dokumen giliran lalu di-ingest sebagai [`SourceKind::AgentSession`]; pipeline
//!   mengekstraknya menjadi observasi bertipe + ringkasan sesi (lihat `process.rs`).
//! - `end_session`: sesi ditutup.
//!
//! Spool ada di `vault/sessions/<sesi>/p<N>.jsonl`, sehingga I/O tool mentah tetap bisa
//! ditelusuri dari episode (prinsip: setiap memori punya sumber asli).

use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use memory_core::ids::now_rfc3339;
use memory_core::model::{Session, SourceKind};
use memory_core::privacy;
use memory_core::text::truncate_chars;

use crate::ingest::{IngestDoc, IngestOutcome};
use crate::{ExtractError, Pipeline, Result};

/// Batas ukuran I/O tool yang disimpan per pemanggilan.
pub const TOOL_PAYLOAD_MAX: usize = 64 * 1024;
/// Batas dokumen giliran yang dikirim ke LLM.
const TURN_DOC_MAX_CHARS: usize = 24_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionInit {
    pub session_id: String,
    pub client: String,
    pub project: Option<String>,
    pub cwd: Option<String>,
    pub worktree: Option<String>,
    pub prompt: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolUseIn {
    pub session_id: String,
    pub client: String,
    pub project: Option<String>,
    pub cwd: Option<String>,
    pub tool_use_id: Option<String>,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: Value,
    #[serde(default)]
    pub tool_response: Value,
    #[serde(default)]
    pub failed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TurnEnd {
    pub session_id: String,
    pub client: String,
    pub project: Option<String>,
    pub cwd: Option<String>,
    /// Jawaban akhir agent (dibaca hook dari transkrip).
    pub assistant_text: Option<String>,
    /// Prompt pengguna bila hook `session-init` tidak sempat terpasang.
    pub user_text: Option<String>,
    /// Alasan: stop | pre_compact | session_end | turn
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SpoolLine {
    Prompt { text: String, at: String },
    Tool { id: String, name: String, input: Value, response: String, failed: bool, at: String, files_read: Vec<String>, files_modified: Vec<String> },
    Assistant { text: String, at: String },
}

/// ID sesi yang aman sebagai kunci record & nama folder.
pub fn clean_session_id(raw: &str) -> String {
    let cleaned: String = raw.trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(64).collect();
    if cleaned.is_empty() { format!("s-{}", &blake3::hash(raw.as_bytes()).to_hex()[..16]) } else { cleaned }
}

/// Berkas yang disentuh suatu pemanggilan tool, dari nama tool & inputnya.
pub fn files_of_tool(tool_name: &str, input: &Value, root: Option<&std::path::Path>) -> (Vec<String>, Vec<String>) {
    let path_of = |keys: &[&str]| -> Vec<String> {
        keys.iter()
            .filter_map(|k| input[k].as_str())
            .filter(|p| !p.trim().is_empty())
            .map(|p| memory_core::project::relative_to_root(p.trim(), root))
            .collect()
    };
    let name = tool_name.rsplit("__").next().unwrap_or(tool_name);
    match name {
        "Read" | "NotebookRead" => (path_of(&["file_path", "notebook_path", "path"]), vec![]),
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => (vec![], path_of(&["file_path", "notebook_path", "path"])),
        "Glob" | "Grep" | "LS" => (vec![], vec![]),
        _ => (path_of(&["file_path"]), vec![]),
    }
}

/// Ringkasan input tool satu baris untuk dokumen giliran.
fn describe_input(tool_name: &str, input: &Value) -> String {
    let name = tool_name.rsplit("__").next().unwrap_or(tool_name);
    let s = |k: &str| input[k].as_str().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let text = match name {
        "Bash" => s("command").unwrap_or_default(),
        "Read" | "NotebookRead" => s("file_path").or_else(|| s("notebook_path")).unwrap_or_default(),
        "Edit" | "MultiEdit" => {
            let f = s("file_path").unwrap_or_default();
            match (s("old_string"), s("new_string")) {
                (Some(o), Some(n)) => format!("{f}: '{}' -> '{}'", truncate_chars(&o, 160), truncate_chars(&n, 240)),
                _ => f,
            }
        }
        "Write" => format!("{} ({} karakter)", s("file_path").unwrap_or_default(), input["content"].as_str().map(|c| c.chars().count()).unwrap_or(0)),
        "Grep" => format!("{} di {}", s("pattern").unwrap_or_default(), s("path").unwrap_or_else(|| ".".into())),
        "Glob" => s("pattern").unwrap_or_default(),
        "WebFetch" | "WebSearch" => s("url").or_else(|| s("query")).unwrap_or_default(),
        "Agent" | "Task" => s("description").or_else(|| s("prompt")).unwrap_or_default(),
        _ => match input {
            Value::Object(m) if !m.is_empty() => {
                let parts: Vec<String> = m.iter().take(4).map(|(k, v)| format!("{k}={}", truncate_chars(&v.to_string().replace('\n', " "), 120))).collect();
                parts.join(" ")
            }
            Value::Null => String::new(),
            other => other.to_string(),
        },
    };
    truncate_chars(&text.replace('\n', " "), 400)
}

fn response_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Array(a) => a.iter().map(response_text).collect::<Vec<_>>().join("\n"),
        Value::Object(m) => {
            for k in ["stdout", "text", "content", "output", "result", "error", "message"] {
                if let Some(x) = m.get(k)
                    && !x.is_null()
                {
                    let t = response_text(x);
                    if !t.trim().is_empty() {
                        let err = m.get("stderr").map(response_text).filter(|e| !e.trim().is_empty()).map(|e| format!("\n[stderr] {e}")).unwrap_or_default();
                        return format!("{t}{err}");
                    }
                }
            }
            v.to_string()
        }
        other => other.to_string(),
    }
}

/// Potong string JSON yang terlalu besar sambil menyisakan penanda.
fn cap_json(v: Value, max: usize) -> Value {
    let s = v.to_string();
    if s.len() <= max {
        return v;
    }
    let cut: String = s.chars().take(max).collect();
    json!({"_truncated": format!("{cut}…[dipotong: {} byte]", s.len())})
}

impl Pipeline {
    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.vault.root().join("sessions").join(session_id)
    }

    fn spool_path(&self, session_id: &str, prompt: u32) -> PathBuf {
        self.session_dir(session_id).join(format!("p{prompt}.jsonl"))
    }

    fn append_spool(&self, session_id: &str, prompt: u32, line: &SpoolLine) -> Result<()> {
        let p = self.spool_path(session_id, prompt);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p)?;
        serde_json::to_writer(&mut f, line).map_err(|e| ExtractError::Invalid(e.to_string()))?;
        f.write_all(b"\n")?;
        Ok(())
    }

    fn read_spool(&self, session_id: &str, prompt: u32) -> Vec<SpoolLine> {
        let p = self.spool_path(session_id, prompt);
        let Ok(s) = std::fs::read_to_string(&p) else { return vec![] };
        s.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    /// Ambil sesi, atau buat bila hook `session-init` terlewat.
    async fn session_or_create(&self, subject: &str, session_id: &str, client: &str, project: Option<&str>, cwd: Option<&str>) -> Result<Session> {
        let now = now_rfc3339();
        Ok(match self.store.get_session(session_id).await? {
            Some(mut s) => {
                if s.project.is_none() && project.is_some() {
                    s.project = project.map(str::to_string);
                }
                s
            }
            None => {
                self.ensure_subject(subject, None).await?;
                Session {
                    id: session_id.to_string(),
                    subject: subject.to_string(),
                    client: client.to_string(),
                    project: project.map(str::to_string),
                    cwd: cwd.map(str::to_string),
                    worktree: None,
                    first_prompt: String::new(),
                    prompt_counter: 1,
                    tool_uses: 0,
                    episodes: 0,
                    status: "active".into(),
                    started_at: now.clone(),
                    last_activity_at: now,
                    completed_at: None,
                }
            }
        })
    }

    /// Prompt baru dari pengguna: naikkan nomor prompt & simpan teksnya ke spool.
    pub async fn session_init(&self, subject: &str, init: SessionInit) -> Result<Session> {
        let sid = clean_session_id(&init.session_id);
        let prompt = privacy::sanitize(&init.prompt, &self.cfg.privacy).map(|s| s.text).unwrap_or_default();
        let mut s = match self.store.get_session(&sid).await? {
            Some(mut s) => {
                s.prompt_counter += 1;
                s
            }
            None => {
                let mut s = self.session_or_create(subject, &sid, &init.client, init.project.as_deref(), init.cwd.as_deref()).await?;
                s.prompt_counter = 1;
                s
            }
        };
        if init.project.is_some() {
            s.project = init.project.clone();
        }
        if init.cwd.is_some() {
            s.cwd = init.cwd.clone();
        }
        if init.worktree.is_some() {
            s.worktree = init.worktree.clone();
        }
        if s.first_prompt.is_empty() && !prompt.is_empty() {
            s.first_prompt = truncate_chars(&prompt, 200);
        }
        s.last_activity_at = now_rfc3339();
        s.status = "active".into();
        if !prompt.is_empty() {
            self.append_spool(&sid, s.prompt_counter, &SpoolLine::Prompt { text: prompt, at: s.last_activity_at.clone() })?;
        }
        self.store.put_session(&s).await?;
        if let (Some(p), Some(w)) = (s.project.as_deref(), s.worktree.as_deref()) {
            self.ensure_project_alias(subject, p, w).await?;
        }
        Ok(s)
    }

    /// Worktree git menjadi alias entitas proyek induknya (memori semua worktree menyatu).
    pub async fn ensure_project_alias(&self, subject: &str, project: &str, alias: &str) -> Result<()> {
        if alias.trim().is_empty() || memory_core::project::same_project(project, alias) {
            return Ok(());
        }
        let found = self.store.entities_by_alias(subject, project).await?.into_iter().find(|e| e.kind == memory_core::model::EntityKind::Project);
        if let Some(mut e) = found
            && !e.aliases.iter().any(|a| a.eq_ignore_ascii_case(alias))
        {
            e.aliases.push(alias.to_string());
            self.store.put_entity(&e, None).await?;
        }
        Ok(())
    }

    /// Catat satu pemanggilan tool ke spool giliran yang sedang berjalan. Idempoten per `tool_use_id`.
    pub async fn record_tool_use(&self, subject: &str, t: ToolUseIn) -> Result<Session> {
        let sid = clean_session_id(&t.session_id);
        let mut s = self.session_or_create(subject, &sid, &t.client, t.project.as_deref(), t.cwd.as_deref()).await?;
        let prompt_n = s.prompt_counter.max(1);
        let root = s.cwd.as_deref().map(std::path::Path::new).and_then(memory_core::project::detect_project).map(|p| p.root);
        let (files_read, files_modified) = files_of_tool(&t.tool_name, &t.tool_input, root.as_deref());
        let id = t.tool_use_id.clone().filter(|i| !i.trim().is_empty()).unwrap_or_else(|| {
            let h = blake3::hash(format!("{}|{}|{}", t.tool_name, t.tool_input, now_rfc3339()).as_bytes());
            format!("tu-{}", &h.to_hex()[..16])
        });
        let existing = self.read_spool(&sid, prompt_n);
        if existing.iter().any(|l| matches!(l, SpoolLine::Tool { id: x, .. } if *x == id)) {
            return Ok(s);
        }
        let input_s = t.tool_input.to_string();
        let (input_s, _) = privacy::redact(&input_s, &self.cfg.privacy);
        let input: Value = serde_json::from_str(&input_s).unwrap_or(Value::String(input_s));
        let input = cap_json(input, TOOL_PAYLOAD_MAX);
        let resp = response_text(&t.tool_response);
        let resp = privacy::sanitize(&resp, &self.cfg.privacy).map(|s| s.text).unwrap_or_default();
        let resp = if resp.len() > TOOL_PAYLOAD_MAX {
            let cut: String = resp.chars().take(TOOL_PAYLOAD_MAX).collect();
            format!("{cut}…[dipotong: {} byte]", resp.len())
        } else {
            resp
        };
        self.append_spool(
            &sid,
            prompt_n,
            &SpoolLine::Tool { id, name: t.tool_name.clone(), input, response: resp, failed: t.failed, at: now_rfc3339(), files_read, files_modified },
        )?;
        s.tool_uses += 1;
        s.last_activity_at = now_rfc3339();
        self.store.put_session(&s).await?;
        Ok(s)
    }

    /// Susun dokumen giliran dari spool (untuk LLM & sebagai teks sumber).
    fn compose_turn(&self, s: &Session, prompt_n: u32, lines: &[SpoolLine], end: &TurnEnd) -> (String, Value) {
        let mut user = end.user_text.clone().unwrap_or_default();
        let mut assistant = end.assistant_text.clone().unwrap_or_default();
        let mut tools: Vec<&SpoolLine> = Vec::new();
        for l in lines {
            match l {
                SpoolLine::Prompt { text, .. } if user.trim().is_empty() => user = text.clone(),
                SpoolLine::Prompt { .. } => {}
                SpoolLine::Tool { .. } => tools.push(l),
                SpoolLine::Assistant { text, .. } if assistant.trim().is_empty() => assistant = text.clone(),
                SpoolLine::Assistant { .. } => {}
            }
        }
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        let mut doc = format!("[{} | proyek: {} | sesi: {} | prompt {} | {}]\n", s.client, s.project.as_deref().unwrap_or("-"), s.id, prompt_n, now);
        if !user.trim().is_empty() {
            doc.push_str(&format!("Pengguna: {}\n", truncate_chars(user.trim(), 4000)));
        }
        let mut files_read: Vec<String> = Vec::new();
        let mut files_modified: Vec<String> = Vec::new();
        if !tools.is_empty() {
            doc.push_str(&format!("\nTool ({} pemanggilan):\n", tools.len()));
            // Bagi anggaran karakter rata per tool, minimal cukup untuk satu baris bermakna.
            let per = (TURN_DOC_MAX_CHARS.saturating_sub(doc.len() + assistant.len().min(6000)) / tools.len().max(1)).clamp(160, 1200);
            for t in &tools {
                let SpoolLine::Tool { name, input, response, failed, files_read: fr, files_modified: fm, .. } = t else { continue };
                for f in fr {
                    if !files_read.contains(f) {
                        files_read.push(f.clone());
                    }
                }
                for f in fm {
                    if !files_modified.contains(f) {
                        files_modified.push(f.clone());
                    }
                }
                let short = name.rsplit("__").next().unwrap_or(name);
                let inp = describe_input(name, input);
                let resp = truncate_chars(response.trim(), per.saturating_sub(inp.chars().count().min(per / 2)).max(80));
                let status = if *failed { " [GAGAL]" } else { "" };
                doc.push_str(&format!("- {short}{status} {inp}"));
                if !resp.is_empty() {
                    doc.push_str(&format!("\n    -> {}", resp.replace('\n', "\n       ")));
                }
                doc.push('\n');
            }
        }
        if !assistant.trim().is_empty() {
            doc.push_str(&format!("\nAsisten: {}\n", truncate_chars(assistant.trim(), 6000)));
        }
        let meta = json!({
            "via": "hook",
            "session_id": s.id,
            "prompt_number": prompt_n,
            "client": s.client,
            "cwd": s.cwd,
            "project": s.project,
            "tool_uses": tools.len(),
            "files_read": files_read,
            "files_modified": files_modified,
            "reason": end.reason,
            "spool": format!("sessions/{}/p{}.jsonl", s.id, prompt_n),
        });
        (doc, meta)
    }

    /// Giliran selesai: ingest dokumen giliran. `Ok(None)` bila tidak ada apa pun untuk disimpan.
    pub async fn summarize_turn(&self, subject: &str, end: TurnEnd) -> Result<Option<IngestOutcome>> {
        let sid = clean_session_id(&end.session_id);
        let mut s = self.session_or_create(subject, &sid, &end.client, end.project.as_deref(), end.cwd.as_deref()).await?;
        let prompt_n = s.prompt_counter.max(1);
        let spool = self.spool_path(&sid, prompt_n);
        // Stop & PreCompact (atau Stop ganda) pada giliran yang sudah dirangkum: tidak ada
        // spool baru -> tidak ada yang disimpan lagi.
        if !spool.exists() && spool.with_extension("done.jsonl").exists() {
            return Ok(None);
        }
        let lines = self.read_spool(&sid, prompt_n);
        let has_tools = lines.iter().any(|l| matches!(l, SpoolLine::Tool { .. }));
        let has_prompt = lines.iter().any(|l| matches!(l, SpoolLine::Prompt { .. })) || end.user_text.as_deref().is_some_and(|t| !t.trim().is_empty());
        let assistant = end.assistant_text.as_deref().map(str::trim).unwrap_or("");
        if !has_tools && (!has_prompt || assistant.chars().count() < 40) && assistant.chars().count() < 40 {
            return Ok(None);
        }
        if let Some(a) = end.assistant_text.as_deref().filter(|a| !a.trim().is_empty()) {
            let clean = privacy::sanitize(a, &self.cfg.privacy).map(|x| x.text).unwrap_or_default();
            if !clean.is_empty() {
                self.append_spool(&sid, prompt_n, &SpoolLine::Assistant { text: clean, at: now_rfc3339() })?;
            }
        }
        let lines = self.read_spool(&sid, prompt_n);
        let (doc, meta) = self.compose_turn(&s, prompt_n, &lines, &end);
        let title = format!("Sesi {}: {} #{}", s.client, s.project.as_deref().unwrap_or("tanpa proyek"), prompt_n);
        let out = self
            .ingest(IngestDoc {
                subject: subject.to_string(),
                kind: SourceKind::AgentSession,
                title,
                text: doc,
                project: s.project.clone(),
                occurred_from: Some(crate::today_iso()),
                meta,
                ..Default::default()
            })
            .await?;
        s.last_activity_at = now_rfc3339();
        if end.reason == "session_end" {
            s.status = "completed".into();
            s.completed_at = Some(s.last_activity_at.clone());
        }
        self.store.put_session(&s).await?;
        // Spool giliran ditutup agar PreCompact + Stop tidak menyimpan dua kali.
        let p = self.spool_path(&sid, prompt_n);
        if p.exists() {
            let _ = std::fs::rename(&p, p.with_extension("done.jsonl"));
        }
        Ok(Some(out))
    }

    /// Sesi ditutup; sisa giliran yang belum dirangkum ikut di-ingest.
    pub async fn end_session(&self, subject: &str, end: TurnEnd) -> Result<Option<IngestOutcome>> {
        let sid = clean_session_id(&end.session_id);
        let out = self.summarize_turn(subject, TurnEnd { reason: "session_end".into(), ..end }).await?;
        if let Some(mut s) = self.store.get_session(&sid).await? {
            s.status = "completed".into();
            s.completed_at = Some(now_rfc3339());
            s.last_activity_at = now_rfc3339();
            self.store.put_session(&s).await?;
        }
        Ok(out)
    }

    /// Semua giliran sesi (spool aktif maupun yang sudah dirangkum) untuk tampilan detail
    /// sesi: prompt, pemanggilan tool (input diringkas, respons dipangkas), jawaban asisten.
    pub fn session_turns(&self, session_id: &str) -> Vec<Value> {
        let sid = clean_session_id(session_id);
        let dir = self.session_dir(&sid);
        let Ok(rd) = std::fs::read_dir(&dir) else { return vec![] };
        let mut files: Vec<(u32, bool, PathBuf)> = rd
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let name = p.file_name()?.to_string_lossy().to_string();
                let num: u32 = name.strip_prefix('p')?.split('.').next()?.parse().ok()?;
                Some((num, name.ends_with(".done.jsonl"), p))
            })
            .collect();
        files.sort();
        files
            .into_iter()
            .map(|(n, done, p)| {
                let lines: Vec<SpoolLine> = std::fs::read_to_string(&p).map(|s| s.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()).unwrap_or_default();
                let items: Vec<Value> = lines
                    .iter()
                    .map(|l| match l {
                        SpoolLine::Prompt { text, at } => json!({"kind": "prompt", "text": text, "at": at}),
                        SpoolLine::Assistant { text, at } => json!({"kind": "assistant", "text": text, "at": at}),
                        SpoolLine::Tool { id, name, input, response, failed, at, files_read, files_modified } => json!({
                            "kind": "tool", "id": id, "name": name, "summary": describe_input(name, input),
                            "response": truncate_chars(response, 600), "failed": failed, "at": at,
                            "files_read": files_read, "files_modified": files_modified,
                        }),
                    })
                    .collect();
                json!({"prompt_number": n, "summarized": done, "items": items})
            })
            .collect()
    }

    /// Hitung episode per sesi (dipanggil setelah sumber sesi diproses).
    pub async fn bump_session_episodes(&self, session_id: &str, n: u32) -> Result<()> {
        if let Some(mut s) = self.store.get_session(session_id).await? {
            s.episodes += n;
            self.store.put_session(&s).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_and_descriptions() {
        let root = std::path::Path::new("/repo/app");
        let (r, m) = files_of_tool("Edit", &json!({"file_path": "/repo/app/src/main.rs", "old_string": "a", "new_string": "b"}), Some(root));
        assert!(r.is_empty());
        assert_eq!(m, vec!["src/main.rs".to_string()]);
        let (r, _) = files_of_tool("mcp__foo__Read", &json!({"file_path": "/other/x.py"}), Some(root));
        assert_eq!(r, vec!["/other/x.py".to_string()]);
        assert!(describe_input("Bash", &json!({"command": "cargo test -p memory-core"})).contains("cargo test"));
        assert!(describe_input("Edit", &json!({"file_path": "a.rs", "old_string": "x", "new_string": "y"})).contains("'x' -> 'y'"));
        assert_eq!(clean_session_id("abc-123"), "abc-123");
        assert!(clean_session_id("!!!").starts_with("s-"));
        assert_eq!(response_text(&json!({"stdout": "ok", "stderr": ""})), "ok");
        assert!(response_text(&json!({"stdout": "ok", "stderr": "warn"})).contains("[stderr] warn"));
    }
}
