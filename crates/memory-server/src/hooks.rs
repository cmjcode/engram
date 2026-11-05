//! Endpoint untuk hook agent (`engram hook <client> <event>`): semua logika ada di
//! server sehingga binary hook tetap tipis dan cepat. Satu route
//! `POST /api/hooks/{client}/{event}` menerima payload yang SUDAH dinormalisasi hook
//! (nama proyek dari akar git, teks asisten dari transkrip, I/O tool yang dipangkas).
//!
//! Event:
//! - `context`      (SessionStart)         -> blok konteks awal sesi (indeks ber-ID, <= inject.max_chars)
//! - `session-init` (UserPromptSubmit)     -> sesi + prompt ke spool
//! - `observation`  (PostToolUse/Failure)  -> satu pemanggilan tool ke spool
//! - `summarize`    (Stop / PreCompact)    -> dokumen giliran di-ingest (observasi + ringkasan)
//! - `session-end`  (SessionEnd)           -> sesi ditutup (+ sisa giliran di-ingest)
//! - `file-context` (PreToolUse Read|Edit) -> riwayat berkas dari memori
//! - `turn`         (Gemini/Antigravity)   -> prompt + jawaban sekaligus (tanpa tool)

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::post;
use serde::Deserialize;
use serde_json::{Value, json};

use memory_core::ids::is_valid_id;
use memory_extract::session::{SessionInit, ToolUseIn, TurnEnd};
use memory_retrieve::InjectOpts;

use crate::{ApiError, ApiResult, AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/hooks/{client}/{event}", post(hook_event))
}

#[derive(Deserialize, Default)]
pub struct HookIn {
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub tool_input: Value,
    #[serde(default)]
    pub tool_response: Value,
    #[serde(default)]
    pub failed: bool,
    #[serde(default)]
    pub assistant_text: Option<String>,
    #[serde(default)]
    pub user_text: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub max_chars: Option<usize>,
}

fn clean_client(c: &str) -> String {
    let c: String = c.trim().to_ascii_lowercase().chars().filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_').take(32).collect();
    if c.is_empty() { "agent".into() } else { c }
}

async fn hook_event(State(st): State<AppState>, Path((client, event)): Path<(String, String)>, Json(b): Json<HookIn>) -> ApiResult<Value> {
    let subject = st.resolve_subject(b.subject.as_deref()).await;
    if !is_valid_id(&subject) {
        return Err(ApiError::BadRequest(format!("subjek tidak valid: {subject}")));
    }
    let client = clean_client(&client);
    let project = b.project.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(str::to_string);
    let sid = || b.session_id.clone().filter(|s| !s.trim().is_empty()).ok_or_else(|| ApiError::BadRequest("session_id wajib diisi".into()));
    let p = &st.rt.pipeline;
    let out = match event.as_str() {
        "context" => {
            if !st.rt.cfg.inject.enabled {
                return Ok(Json(json!({"ok": true, "text": "", "found": false, "disabled": true})));
            }
            let r = st
                .rt
                .retriever
                .inject_context(&subject, &InjectOpts { project: project.clone(), client: Some(client.clone()), max_chars: b.max_chars, ..Default::default() })
                .await?;
            json!({"ok": true, "text": r.text, "found": r.found, "chars": r.chars, "observations": r.observations, "sessions": r.sessions, "reductions": r.reductions, "over_budget": r.over_budget})
        }
        "session-init" => {
            let s = p
                .session_init(&subject, SessionInit { session_id: sid()?, client, project, cwd: b.cwd.clone(), worktree: b.worktree.clone(), prompt: b.prompt.clone().unwrap_or_default() })
                .await?;
            json!({"ok": true, "session_id": s.id, "prompt_number": s.prompt_counter})
        }
        "observation" => {
            let Some(tool_name) = b.tool_name.clone().filter(|t| !t.trim().is_empty()) else {
                return Err(ApiError::BadRequest("tool_name wajib diisi".into()));
            };
            let s = p
                .record_tool_use(
                    &subject,
                    ToolUseIn { session_id: sid()?, client, project, cwd: b.cwd.clone(), tool_use_id: b.tool_use_id.clone(), tool_name, tool_input: b.tool_input.clone(), tool_response: b.tool_response.clone(), failed: b.failed },
                )
                .await?;
            json!({"ok": true, "session_id": s.id, "prompt_number": s.prompt_counter, "tool_uses": s.tool_uses})
        }
        "summarize" | "session-end" | "turn" => {
            let reason = match event.as_str() {
                "session-end" => "session_end".to_string(),
                "turn" => "turn".to_string(),
                _ => b.reason.clone().unwrap_or_else(|| "stop".into()),
            };
            let end = TurnEnd { session_id: sid()?, client: client.clone(), project: project.clone(), cwd: b.cwd.clone(), assistant_text: b.assistant_text.clone(), user_text: b.user_text.clone(), reason };
            if event == "turn" {
                // Client tanpa hook per tool: prompt + jawaban dicatat sebagai satu giliran.
                p.session_init(&subject, SessionInit { session_id: end.session_id.clone(), client, project, cwd: b.cwd.clone(), worktree: b.worktree.clone(), prompt: b.user_text.clone().unwrap_or_default() }).await?;
            }
            let r = if event == "session-end" { p.end_session(&subject, end).await? } else { p.summarize_turn(&subject, end).await? };
            match r {
                Some(o) => json!({"ok": true, "ingested": true, "source_id": o.source.id, "duplicate": o.duplicate, "job_id": o.job_id}),
                None => json!({"ok": true, "ingested": false}),
            }
        }
        "file-context" => {
            let Some(path) = b.file_path.clone().filter(|f| !f.trim().is_empty()) else {
                return Err(ApiError::BadRequest("file_path wajib diisi".into()));
            };
            let text = st.rt.retriever.file_context(&subject, &path, 8, b.max_chars.unwrap_or(2500)).await?;
            json!({"ok": true, "found": text.is_some(), "text": text.unwrap_or_default()})
        }
        other => return Err(ApiError::NotFound(format!("event hook tidak dikenal: {other}"))),
    };
    Ok(Json(out))
}
