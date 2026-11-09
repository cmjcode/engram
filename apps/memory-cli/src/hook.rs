//! `engram hook <client> <event>`: hook agent AI yang tipis dan cepat.
//!
//! Dipanggil Claude Code (SessionStart, UserPromptSubmit, PreToolUse, PostToolUse,
//! Stop, PreCompact, SessionEnd), Gemini CLI (AfterAgent/SessionStart) dan Antigravity
//! (Stop) dengan payload JSON di stdin. Hook TIDAK membuka vault: ia menormalkan
//! payload (proyek dari akar git, teks asisten dari transkrip, I/O tool yang dipangkas
//! dan dibersihkan dari gambar), mengirimnya ke server engram lewat HTTP dengan batas
//! waktu ketat, dan selalu keluar 0 agar tidak pernah menghambat agent. Bila server
//! mati, event disimpan ke spool lokal dan dikirim ulang pada panggilan berikutnya.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use memory_core::project::detect_project;

/// Tool yang tidak pernah layak jadi observasi (termasuk tool engram sendiri: cegah umpan balik).
const SKIP_TOOLS: &[&str] = &["TodoWrite", "AskUserQuestion", "ExitPlanMode", "EnterPlanMode", "ToolSearch", "ListMcpResourcesTool", "ReadMcpResourceTool", "Monitor", "ScheduleWakeup"];
const PAYLOAD_MAX: usize = 64 * 1024;

struct Conn {
    base: String,
    token: Option<String>,
    data_dir: PathBuf,
}

fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("ENGRAM_DATA_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support").join("id.engram.ai")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join("id.engram.ai")
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local").join("share")).join("id.engram.ai")
    }
}

/// Alamat & token server: env `ENGRAM_URL`/`MEMORY_TOKEN`, lalu `connection.json`
/// aplikasi desktop, lalu `memory.toml` (`server.bind`), lalu bawaan 127.0.0.1:7878.
fn connection(config: &Path) -> Conn {
    let dir = data_dir();
    let mut base = std::env::var("ENGRAM_URL").ok().filter(|u| !u.trim().is_empty());
    let mut token = std::env::var("MEMORY_TOKEN").ok().filter(|t| !t.trim().is_empty());
    if let Ok(s) = std::fs::read_to_string(dir.join("connection.json"))
        && let Ok(v) = serde_json::from_str::<Value>(&s)
    {
        if base.is_none() {
            base = v["url"].as_str().map(|u| u.trim_end_matches('/').to_string()).or_else(|| v["mcp_url"].as_str().map(|u| u.trim_end_matches("/mcp").to_string()));
        }
        if token.is_none() {
            token = v["token"].as_str().map(str::to_string).filter(|t| !t.is_empty());
        }
    }
    if token.is_none()
        && let Ok(t) = std::fs::read_to_string(dir.join("mcp_token"))
        && !t.trim().is_empty()
    {
        token = Some(t.trim().to_string());
    }
    if base.is_none()
        && config.exists()
        && let Ok(s) = std::fs::read_to_string(config)
        && let Ok(cfg) = memory_core::MemoryConfig::from_toml(&s)
    {
        base = Some(format!("http://{}", cfg.server.bind));
    }
    Conn { base: base.unwrap_or_else(|| "http://127.0.0.1:7878".into()), token, data_dir: dir }
}

fn read_stdin() -> Value {
    use std::io::Read;
    let mut s = String::new();
    let _ = std::io::stdin().read_to_string(&mut s);
    serde_json::from_str(&s).unwrap_or(json!({}))
}

fn str_of(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| v[k].as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn looks_base64(s: &str) -> bool {
    s.len() > 2000 && s.bytes().take(4000).all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=' || b == b'\n' || b == b'\r')
}

/// Buang gambar (base64 / blok image) dari payload tool dan pangkas string raksasa.
fn strip_images(v: Value, depth: usize) -> Value {
    if depth > 12 {
        return Value::String("[terlalu dalam]".into());
    }
    match v {
        Value::String(s) => {
            if looks_base64(&s) || s.starts_with("data:image/") {
                Value::String(format!("[gambar dibuang: {} byte]", s.len()))
            } else if s.len() > PAYLOAD_MAX {
                let cut: String = s.chars().take(PAYLOAD_MAX).collect();
                Value::String(format!("{cut}…[dipotong: {} byte]", s.len()))
            } else {
                Value::String(s)
            }
        }
        Value::Array(a) => Value::Array(a.into_iter().map(|x| strip_images(x, depth + 1)).collect()),
        Value::Object(m) => {
            let is_image = m.get("type").and_then(Value::as_str).is_some_and(|t| t == "image" || t == "image_url");
            if is_image {
                return json!({"type": "image", "elided": true});
            }
            Value::Object(m.into_iter().map(|(k, x)| (k, strip_images(x, depth + 1))).collect())
        }
        other => other,
    }
}

/// Hasil ekstraksi satu giliran dari berkas transkrip agent.
#[derive(Debug, Default)]
struct TranscriptTurn {
    user: Option<String>,
    assistant: Option<String>,
    files_read: Vec<String>,
    files_modified: Vec<String>,
}

fn collect_tool_files(tool_name: &str, input: &Value, files_read: &mut Vec<String>, files_modified: &mut Vec<String>) {
    let (fr, fm) = memory_extract::session::files_of_tool(tool_name, input, None);
    for f in fr {
        if !files_read.contains(&f) {
            files_read.push(f);
        }
    }
    for f in fm {
        if !files_modified.contains(&f) {
            files_modified.push(f);
        }
    }
}

/// Teks terakhir asisten, pengguna, dan berkas yang disentuh dari transkrip JSONL Claude Code.
fn claude_transcript(path: &str) -> TranscriptTurn {
    let Ok(s) = std::fs::read_to_string(path) else { return TranscriptTurn::default() };
    let mut out = TranscriptTurn::default();
    for line in s.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let content = &v["message"]["content"];
        if let Value::Array(parts) = content {
            for p in parts {
                if p["type"] == "tool_use"
                    && let Some(name) = p["name"].as_str()
                {
                    collect_tool_files(name, &p["input"], &mut out.files_read, &mut out.files_modified);
                }
            }
        }
        let text = match content {
            Value::String(t) => t.trim().to_string(),
            Value::Array(parts) => {
                if parts.iter().any(|p| p["type"] == "tool_result") {
                    continue;
                }
                parts.iter().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).map(str::trim).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n")
            }
            _ => String::new(),
        };
        if text.is_empty() {
            continue;
        }
        match v["type"].as_str() {
            Some("assistant") if out.assistant.is_none() => out.assistant = Some(text),
            Some("user") if out.user.is_none() => out.user = Some(text),
            _ => {}
        }
        if out.user.is_some() && out.assistant.is_some() {
            break;
        }
    }
    out
}

/// Transkrip Antigravity: baris `USER_INPUT` / `PLANNER_RESPONSE` beserta `tool_calls`.
fn antigravity_transcript(path: &str) -> TranscriptTurn {
    let p = Path::new(path);
    let full = p.with_file_name("transcript_full.jsonl");
    let target = if full.exists() { full } else { p.to_path_buf() };
    let Ok(s) = std::fs::read_to_string(target) else { return TranscriptTurn::default() };
    let mut out = TranscriptTurn::default();
    for line in s.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let content = v["content"].as_str().map(str::trim).filter(|c| !c.is_empty()).map(str::to_string);
        if v["type"].as_str() == Some("PLANNER_RESPONSE")
            && let Some(calls) = v["tool_calls"].as_array()
        {
            for tc in calls {
                if let Some(name) = tc["name"].as_str().or_else(|| tc["tool_name"].as_str()) {
                    let args = tc.get("arguments").or_else(|| tc.get("args")).or_else(|| tc.get("input")).unwrap_or(&Value::Null);
                    let parsed_args = match args {
                        Value::String(raw) => serde_json::from_str::<Value>(raw).unwrap_or(Value::Null),
                        other => other.clone(),
                    };
                    collect_tool_files(name, &parsed_args, &mut out.files_read, &mut out.files_modified);
                }
            }
        }
        match v["type"].as_str() {
            Some("PLANNER_RESPONSE") if out.assistant.is_none() && content.is_some() => out.assistant = content,
            Some("USER_INPUT") if out.user.is_none() => out.user = content.map(|c| clean_user_text(&c)),
            _ => {}
        }
        if out.user.is_some() && out.assistant.is_some() {
            break;
        }
    }
    out
}

fn clean_user_text(raw: &str) -> String {
    if let Some(a) = raw.find("<USER_REQUEST>")
        && let Some(b) = raw.find("</USER_REQUEST>")
        && b > a
    {
        return raw[a + "<USER_REQUEST>".len()..b].trim().to_string();
    }
    let mut out = raw.to_string();
    for tag in ["ADDITIONAL_METADATA", "USER_SETTINGS_CHANGE", "system-reminder"] {
        while let (Some(a), Some(b)) = (out.find(&format!("<{tag}")), out.find(&format!("</{tag}>"))) {
            if b < a {
                break;
            }
            out.replace_range(a..b + tag.len() + 3, "");
        }
    }
    out.trim().to_string()
}

fn spool_dir(c: &Conn) -> PathBuf {
    c.data_dir.join("hook-spool")
}

fn spool_write(c: &Conn, client: &str, event: &str, body: &Value) {
    let dir = spool_dir(c);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let name = format!("{}-{}-{}.json", chrono::Utc::now().format("%Y%m%dT%H%M%S%3f"), client, memory_core::new_id());
    let _ = std::fs::write(dir.join(name), json!({"client": client, "event": event, "body": body}).to_string());
}

async fn post(http: &reqwest::Client, c: &Conn, client: &str, event: &str, body: &Value, timeout: Duration) -> Result<Value, bool> {
    let mut req = http.post(format!("{}/api/hooks/{client}/{event}", c.base)).timeout(timeout).json(body);
    if let Some(t) = &c.token {
        req = req.bearer_auth(t);
    }
    match req.send().await {
        Ok(r) if r.status().is_success() => Ok(r.json().await.unwrap_or(json!({}))),
        // 4xx = payload ditolak (jangan diulang); selain itu server bermasalah -> spool.
        Ok(r) => Err(r.status().is_server_error()),
        Err(_) => Err(true),
    }
}

/// Kirim ulang event yang tertunda (maks 25 per panggilan, best effort).
async fn drain_spool(http: &reqwest::Client, c: &Conn) {
    let Ok(rd) = std::fs::read_dir(spool_dir(c)) else { return };
    let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    for f in files.into_iter().take(25) {
        let Ok(s) = std::fs::read_to_string(&f) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&s) else {
            let _ = std::fs::remove_file(&f);
            continue;
        };
        let (client, event) = (v["client"].as_str().unwrap_or("agent").to_string(), v["event"].as_str().unwrap_or("").to_string());
        if event == "context" || event == "file-context" {
            let _ = std::fs::remove_file(&f);
            continue;
        }
        match post(http, c, &client, &event, &v["body"], Duration::from_millis(1500)).await {
            Ok(_) | Err(false) => {
                let _ = std::fs::remove_file(&f);
            }
            Err(true) => break,
        }
    }
}

fn project_fields(cwd: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(c) = cwd else { return (None, None) };
    match detect_project(Path::new(c)) {
        Some(p) => (Some(p.name), p.worktree),
        None => (None, None),
    }
}

fn emit(v: Value) {
    println!("{v}");
}

/// Jalankan aksi `engram repo ...` lewat server yang sedang berjalan (server memegang kunci
/// database). `None` bila server tidak bisa dihubungi atau belum mengenal aksi ini; CLI lalu
/// membuka vault langsung.
pub async fn repo_via_server(config: &Path, subject: Option<&str>, cwd: &Path, project: Option<&str>, options: Value) -> Option<Result<Value, String>> {
    let c = connection(config);
    let http = reqwest::Client::builder().connect_timeout(Duration::from_millis(400)).build().ok()?;
    let body = json!({"subject": subject, "cwd": cwd, "project": project, "options": options});
    let mut req = http.post(format!("{}/api/hooks/cli/repo-cli", c.base)).timeout(Duration::from_secs(180)).json(&body);
    if let Some(t) = &c.token {
        req = req.bearer_auth(t);
    }
    let r = req.send().await.ok()?;
    let status = r.status();
    if status.is_success() {
        return Some(r.json::<Value>().await.map(|v| v["result"].clone()).map_err(|e| e.to_string()));
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return None;
    }
    let text = r.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["error"].as_str().map(str::to_string)).unwrap_or(text);
    Some(Err(format!("server engram: {msg}")))
}

/// Jalankan hook. Selalu `Ok(())`: galat apa pun hanya dicatat ke stderr.
pub async fn run(config: &Path, client: &str, event: &str) -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    let payload = read_stdin();
    let client = client.trim().to_ascii_lowercase();
    let event = event.trim().to_ascii_lowercase();
    let c = connection(config);
    let http = reqwest::Client::builder().connect_timeout(Duration::from_millis(400)).build()?;
    let antigravity = client == "antigravity";
    let fallback_out = || if antigravity { json!({"decision": "allow"}) } else { json!({}) };

    let cwd = str_of(&payload, &["cwd", "workspaceRoot", "projectDir"]).or_else(|| payload["workspacePaths"].as_array().and_then(|a| a.first()).and_then(Value::as_str).map(str::to_string)).or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string()));
    let (project, worktree) = project_fields(cwd.as_deref());
    let session_id = str_of(&payload, &["session_id", "sessionId", "conversationId", "conversation_id"]).unwrap_or_else(|| format!("{client}-{}", cwd.as_deref().map(|c| blake3::hash(c.as_bytes()).to_hex()[..12].to_string()).unwrap_or_else(|| "tanpa-cwd".into())));
    let spool_pending = std::fs::read_dir(spool_dir(&c)).map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "json")).count()).unwrap_or(0);
    let mut body = json!({"session_id": session_id, "project": project, "cwd": cwd, "worktree": worktree, "spool_pending": spool_pending});

    let (server_event, timeout) = match event.as_str() {
        "context" | "session-start" => {
            body["max_chars"] = json!(std::env::var("ENGRAM_INJECT_MAX_CHARS").ok().and_then(|v| v.parse::<usize>().ok()));
            ("context", Duration::from_millis(2500))
        }
        "session-init" | "user-prompt" => {
            let prompt = str_of(&payload, &["prompt", "user_prompt", "lastUserInput"]).map(|p| clean_user_text(&p)).unwrap_or_default();
            if prompt.is_empty() {
                emit(fallback_out());
                return Ok(());
            }
            body["prompt"] = json!(prompt);
            ("session-init", Duration::from_millis(1500))
        }
        "observation" | "post-tool" => {
            let Some(tool_name) = str_of(&payload, &["tool_name", "toolName"]) else {
                emit(fallback_out());
                return Ok(());
            };
            let short = tool_name.rsplit("__").next().unwrap_or(&tool_name).to_string();
            if SKIP_TOOLS.contains(&short.as_str()) || tool_name.starts_with("mcp__engram__") || tool_name.contains("__engram__") {
                emit(fallback_out());
                return Ok(());
            }
            body["tool_name"] = json!(tool_name);
            body["tool_use_id"] = json!(str_of(&payload, &["tool_use_id", "toolUseId"]));
            body["tool_input"] = strip_images(payload.get("tool_input").cloned().unwrap_or(Value::Null), 0);
            body["tool_response"] = strip_images(payload.get("tool_response").cloned().or_else(|| payload.get("tool_result").cloned()).or_else(|| payload.get("error").cloned()).unwrap_or(Value::Null), 0);
            body["failed"] = json!(payload["hook_event_name"].as_str() == Some("PostToolUseFailure") || payload.get("error").is_some_and(|e| !e.is_null()));
            ("observation", Duration::from_millis(2000))
        }
        "summarize" | "stop" | "pre-compact" | "session-end" | "turn" => {
            let mut turn = match client.as_str() {
                "antigravity" => str_of(&payload, &["transcriptPath", "transcript_path"]).map(|p| antigravity_transcript(&p)).unwrap_or_default(),
                "gemini-cli" | "gemini" => TranscriptTurn {
                    user: str_of(&payload, &["prompt"]).map(|x| clean_user_text(&x)),
                    assistant: str_of(&payload, &["prompt_response", "response"]),
                    ..Default::default()
                },
                _ => str_of(&payload, &["transcript_path", "transcriptPath"]).map(|p| claude_transcript(&p)).unwrap_or_default(),
            };
            if let Some(u) = str_of(&payload, &["lastUserInput", "prompt"]).map(|x| clean_user_text(&x)) {
                turn.user = Some(u);
            }
            if let Some(a) = str_of(&payload, &["last_assistant_message", "response", "prompt_response"]) {
                turn.assistant = Some(a);
            }
            body["assistant_text"] = json!(turn.assistant.map(|a| a.chars().take(12_000).collect::<String>()));
            body["user_text"] = json!(turn.user.map(|u| u.chars().take(6_000).collect::<String>()));
            body["files_read"] = json!(turn.files_read);
            body["files_modified"] = json!(turn.files_modified);
            body["reason"] = json!(match event.as_str() {
                "pre-compact" => "pre_compact",
                "session-end" => "session_end",
                "turn" => "turn",
                _ => "stop",
            });
            let ev = match event.as_str() {
                "session-end" => "session-end",
                "turn" => "turn",
                // Client tanpa hook per tool mengirim giliran utuh.
                _ if client == "antigravity" || client.starts_with("gemini") || client == "cursor" || client == "windsurf" => "turn",
                _ => "summarize",
            };
            (ev, Duration::from_millis(3000))
        }
        "file-context" | "pre-tool" => {
            let tool = str_of(&payload, &["tool_name", "toolName"]).unwrap_or_default();
            let short = tool.rsplit("__").next().unwrap_or(&tool);
            let fp = str_of(&payload["tool_input"], &["file_path", "notebook_path", "path", "Path", "TargetFile", "AbsolutePath"]);
            let Some(fp) = fp.filter(|_| {
                matches!(
                    short,
                    "Read"
                        | "Edit"
                        | "MultiEdit"
                        | "Write"
                        | "NotebookEdit"
                        | "read_file"
                        | "view_file"
                        | "edit_file"
                        | "write_file"
                        | "replace_file_content"
                        | "multi_replace_file_content"
                        | "create_file"
                        | "apply_patch"
                )
            }) else {
                emit(fallback_out());
                return Ok(());
            };
            // Sekali per sesi per berkas: cukup diingatkan sekali.
            let seen = c.data_dir.join("hook-seen").join(&session_id);
            let key = blake3::hash(fp.as_bytes()).to_hex()[..16].to_string();
            if seen.join(&key).exists() {
                emit(fallback_out());
                return Ok(());
            }
            let _ = std::fs::create_dir_all(&seen).and_then(|_| std::fs::write(seen.join(&key), b""));
            body["file_path"] = json!(fp);
            body["max_chars"] = json!(2500);
            ("file-context", Duration::from_millis(1500))
        }
        // Git hook memori repo bersama: post-merge/post-checkout (antre impor) dan pre-commit
        // (tulis bundel sebelum commit). Tanpa server dilewati; diulang saat sesi/berkala.
        "repo-sync" => ("repo-sync", Duration::from_millis(1500)),
        "repo-export" => ("repo-export", Duration::from_secs(15)),
        other => {
            eprintln!("engram hook: event tidak dikenal: {other}");
            emit(fallback_out());
            return Ok(());
        }
    };

    let result = post(&http, &c, &client, server_event, &body, timeout).await;
    match (&result, server_event) {
        (Ok(v), "context") => {
            let text = v["text"].as_str().unwrap_or("");
            if text.trim().is_empty() {
                emit(fallback_out());
            } else {
                emit(json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": text}}));
            }
        }
        (Ok(v), "file-context") => {
            let text = v["text"].as_str().unwrap_or("");
            if text.trim().is_empty() {
                emit(fallback_out());
            } else {
                emit(json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "additionalContext": text}}));
            }
        }
        (Ok(_), _) => {
            emit(fallback_out());
            drain_spool(&http, &c).await;
        }
        (Err(retry), _) => {
            if *retry && !matches!(server_event, "context" | "file-context" | "repo-sync" | "repo-export") {
                spool_write(&c, &client, server_event, &body);
            }
            emit(fallback_out());
        }
    }
    if std::env::var_os("ENGRAM_HOOK_DEBUG").is_some() {
        eprintln!("engram hook {client} {event}: {} dalam {} ms", if result.is_ok() { "ok" } else { "gagal" }, started.elapsed().as_millis());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_images_and_cleans_user_text() {
        let big = "A".repeat(5000);
        let v = strip_images(json!({"a": big, "b": {"type": "image", "source": {"data": "xx"}}, "c": "halo"}), 0);
        assert!(v["a"].as_str().unwrap().starts_with("[gambar dibuang"));
        assert_eq!(v["b"]["elided"], true);
        assert_eq!(v["c"], "halo");
        assert_eq!(clean_user_text("<ADDITIONAL_METADATA>x</ADDITIONAL_METADATA>halo <USER_REQUEST>tolong</USER_REQUEST>"), "tolong");
        assert_eq!(clean_user_text("<system-reminder>abc</system-reminder> kerjakan"), "kerjakan");
    }

    #[test]
    fn parses_claude_transcript() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.jsonl");
        std::fs::write(
            &p,
            [
                json!({"type": "user", "message": {"content": "perbaiki bug login"}}).to_string(),
                json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Read", "input": {"file_path": "src/auth.rs"}}]}}).to_string(),
                json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "isi"}]}}).to_string(),
                json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Edit", "input": {"file_path": "src/auth.rs"}}]}}).to_string(),
                json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "Selesai: bug ada di token expiry."}]}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();
        let t = claude_transcript(p.to_str().unwrap());
        assert_eq!(t.user.as_deref(), Some("perbaiki bug login"));
        assert!(t.assistant.unwrap().contains("token expiry"));
        assert_eq!(t.files_read, vec!["src/auth.rs".to_string()]);
        assert_eq!(t.files_modified, vec!["src/auth.rs".to_string()]);
    }

    #[test]
    fn parses_antigravity_transcript_with_tool_calls() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("transcript.jsonl");
        std::fs::write(
            &p,
            [
                json!({"type": "USER_INPUT", "content": "<USER_REQUEST>optimasi recall</USER_REQUEST>"}).to_string(),
                json!({"type": "PLANNER_RESPONSE", "content": "", "tool_calls": [
                    {"name": "read_file", "arguments": {"Path": "/repo/crates/memory-retrieve/src/recall.rs"}},
                    {"name": "edit_file", "arguments": {"Path": "/repo/crates/memory-retrieve/src/recall.rs"}}
                ]}).to_string(),
                json!({"type": "PLANNER_RESPONSE", "content": "Recall sudah dioptimasi dengan batch query."}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();
        let t = antigravity_transcript(p.to_str().unwrap());
        assert_eq!(t.user.as_deref(), Some("optimasi recall"));
        assert!(t.assistant.unwrap().contains("batch query"));
        assert_eq!(t.files_read, vec!["/repo/crates/memory-retrieve/src/recall.rs".to_string()]);
        assert_eq!(t.files_modified, vec!["/repo/crates/memory-retrieve/src/recall.rs".to_string()]);
    }
}
