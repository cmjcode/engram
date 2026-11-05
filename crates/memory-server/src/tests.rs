use std::sync::Arc;

use memory_core::MemoryConfig;
use memory_import::Importer;
use memory_jobs::Runtime;
use memory_llm::mock::{ScriptedLlm, last_user};
use memory_llm::{ChatRequest, HashEmbedder};
use serde_json::{Value, json};

use crate::{AppState, ServiceControl, ServiceInfo, router};

const DIM: usize = 64;

fn llm(req: &ChatRequest) -> memory_llm::Result<String> {
    let user = last_user(req);
    Ok(match req.task.as_str() {
        "segment" => {
            let n = user.lines().filter(|l| l.starts_with('[')).count();
            json!({"segments": (0..n).map(|i| json!({"start": i, "end": i, "significance": 0.9})).collect::<Vec<_>>()}).to_string()
        }
        "extract" => json!({"title": "Toko batik", "summary": "Pakde membuka toko batik",
            "entities": [{"name": "Budi", "kind": "person", "is_self": true}, {"name": "Pakde Karno", "kind": "person", "aliases": ["Karno"]}],
            "facts": [{"subject": "Pakde Karno", "predicate": "membuka", "object": "toko batik di Solo", "category": "pekerjaan", "quote": "Pakde Karno membuka toko batik di Solo"}],
            "kin": [{"person": "Pakde Karno", "term": "pakde", "quote": "Pakde Karno"}]}).to_string(),
        "interview" => json!({"text": "Ceritakan masa kecil Bapak?"}).to_string(),
        "session" => {
            if user.contains("Pengguna: halo") {
                json!({"skip": true, "skip_reason": "basa-basi"}).to_string()
            } else {
                json!({
                    "skip": false,
                    "summary": {"request": "Perbaiki bug token login", "investigated": "src/auth.rs", "learned": "expiry memakai detik, bukan milidetik", "completed": "Perbaikan di src/auth.rs", "next_steps": "tambah tes regresi"},
                    "observations": [{
                        "obs_type": "bugfix", "title": "Bug expiry token login diperbaiki",
                        "narrative": "Token login kedaluwarsa terlalu cepat karena expiry dihitung dalam milidetik di src/auth.rs; diperbaiki menjadi detik.",
                        "concepts": ["token expiry"], "files_read": ["src/auth.rs"], "files_modified": ["src/auth.rs"],
                        "entities": [{"name": "src/auth.rs", "kind": "component"}, {"name": "LLM-MEMORY", "kind": "project"}],
                        "facts": [{"subject": "LLM-MEMORY", "predicate": "menyimpan expiry token dalam", "object": "detik", "category": "konvensi_kode", "quote": "expiry dihitung dalam detik"}],
                        "decisions": [{"situation": "token kedaluwarsa terlalu cepat", "action": "ubah satuan expiry ke detik", "reason": "server membandingkan dengan epoch detik"}],
                        "lessons": [{"kind": "pitfall", "title": "Expiry token harus dalam detik", "detail": "Jangan pakai milidetik", "quote": "expiry dihitung dalam detik"}],
                        "quotes": ["expiry dihitung dalam detik"]
                    }]
                }).to_string()
            }
        }
        _ => json!({"text": "ok"}).to_string(),
    })
}

async fn spawn(token: Option<&str>) -> (String, AppState, tempfile::TempDir) {
    spawn_with(token, None).await
}

async fn spawn_with(token: Option<&str>, services: Option<Arc<dyn ServiceControl>>) -> (String, AppState, tempfile::TempDir) {
    spawn_full(token, services, None).await
}

async fn spawn_full(token: Option<&str>, services: Option<Arc<dyn ServiceControl>>, config_path: Option<std::path::PathBuf>) -> (String, AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = MemoryConfig::for_tests(DIM);
    cfg.vault.path = dir.path().to_path_buf();
    cfg.vault.default_subject = "budi".into();
    cfg.server.token_env = None;
    let rt = Runtime::open_with(cfg, Arc::new(ScriptedLlm::new(llm)), Arc::new(HashEmbedder::new(DIM))).await.unwrap();
    let importer = Importer::new(rt.pipeline.clone(), None);
    let mut st = AppState::new(rt, importer);
    st.token = token.map(|t| Arc::new(t.to_string()));
    st.services = services;
    st.config_path = config_path;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(st.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), st, dir)
}

async fn post(c: &reqwest::Client, url: String, body: Value) -> Value {
    let r = c.post(&url).json(&body).send().await.unwrap();
    let status = r.status();
    if !status.is_success() {
        panic!("{url} -> {status}: {}", r.text().await.unwrap_or_default());
    }
    r.json().await.unwrap()
}

#[tokio::test]
async fn http_api_and_mcp_end_to_end() {
    let (base, st, _d) = spawn(None).await;
    let c = reqwest::Client::new();
    let html = c.get(format!("{base}/")).send().await.unwrap().text().await.unwrap();
    assert!(html.contains("engram"));
    post(&c, format!("{base}/api/subjects"), json!({"id": "budi", "name": "Budi", "birth": "1979-03-10"})).await;
    let r = post(&c, format!("{base}/api/subjects/budi/remember"), json!({"text": "Pakde Karno membuka toko batik di Solo waktu saya kecil.", "source": "uji"})).await;
    assert!(r["job_id"].is_string());
    st.rt.drain().await.unwrap();

    let stats: Value = c.get(format!("{base}/api/subjects/budi/stats")).send().await.unwrap().json().await.unwrap();
    assert_eq!(stats["stats"]["episodes"], 1);
    let eps: Value = c.get(format!("{base}/api/subjects/budi/episodes")).send().await.unwrap().json().await.unwrap();
    assert_eq!(eps.as_array().unwrap().len(), 1);
    let stream: Value = c.get(format!("{base}/api/subjects/budi/stream")).send().await.unwrap().json().await.unwrap();
    assert!(stream.is_array());
    let facts: Value = c.get(format!("{base}/api/subjects/budi/facts")).send().await.unwrap().json().await.unwrap();
    assert_eq!(facts.as_array().unwrap().len(), 1);
    assert_eq!(facts[0]["entity_name"], "Pakde Karno");
    let kin: Value = c.get(format!("{base}/api/subjects/budi/kin")).send().await.unwrap().json().await.unwrap();
    assert_eq!(kin.as_array().unwrap().len(), 1);
    assert_eq!(kin[0]["from_name"], "Pakde Karno");
    assert_eq!(kin[0]["relation_label"], "pakde");
    let rec = post(&c, format!("{base}/api/subjects/budi/recall"), json!({"query": "toko batik Pakde Karno"})).await;
    assert_eq!(rec["found"], true);
    let fam: Value = c.get(format!("{base}/api/subjects/budi/family")).send().await.unwrap().json().await.unwrap();
    assert!(fam["edges"].as_array().unwrap().iter().any(|e| e["label"] == "pakde"));
    let q: Value = c.get(format!("{base}/api/subjects/budi/interview/next")).send().await.unwrap().json().await.unwrap();
    assert!(q["question"].as_str().unwrap().contains("masa kecil"));
    let bad = c.get(format!("{base}/api/subjects/bud%27i/stats")).send().await.unwrap();
    assert_eq!(bad.status(), 400);

    // Import multipart (foto + caption berpasangan).
    let form = reqwest::multipart::Form::new()
        .part("file", reqwest::multipart::Part::bytes(b"Lebaran 1995 di rumah Mbah bersama sepupu.".to_vec()).file_name("lebaran.txt"))
        .part("file", reqwest::multipart::Part::bytes(vec![0xFF, 0xD8]).file_name("lebaran.jpg"))
        .part("file", reqwest::multipart::Part::bytes(b"Catatan harian tentang sawah Bapak di Klaten.".to_vec()).file_name("../../catatan.md"));
    let imp: Value = c.post(format!("{base}/api/subjects/budi/import")).multipart(form).send().await.unwrap().json().await.unwrap();
    assert_eq!(imp["queued"], 2, "{imp}");
    st.rt.drain().await.unwrap();
    let b: Value = c.get(format!("{base}/api/batches/{}", imp["batch_id"].as_str().unwrap())).send().await.unwrap().json().await.unwrap();
    assert_eq!(b["batch"]["done"], 2);

    // MCP lewat HTTP.
    let init = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}})).await;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "engram");
    // Versi tidak dikenal -> versi terbaru yang didukung server.
    let init2 = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2030-01-01"}})).await;
    assert_eq!(init2["result"]["protocolVersion"], crate::mcp::PROTOCOL_VERSION);
    // Streamable HTTP: GET membuka stream SSE, tanpa Accept SSE -> 405.
    let no_sse = c.get(format!("{base}/mcp")).send().await.unwrap();
    assert_eq!(no_sse.status(), 405);
    let sse = c.get(format!("{base}/mcp")).header("Accept", "text/event-stream").send().await.unwrap();
    assert_eq!(sse.status(), 200);
    assert!(sse.headers()["content-type"].to_str().unwrap().starts_with("text/event-stream"));
    drop(sse);
    // Profil coding hanya memuat tool sesi koding; profil tidak dikenal -> 400.
    let coding = post(&c, format!("{base}/mcp?profile=coding"), json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    let coding_names: Vec<&str> = coding["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(coding_names, crate::mcp::CODING_TOOLS);
    assert_eq!(c.post(format!("{base}/mcp?profile=nope")).json(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).send().await.unwrap().status(), 400);
    let notif = c.post(format!("{base}/mcp")).json(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).send().await.unwrap();
    assert_eq!(notif.status(), 202);
    let tools = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["recall", "who_is", "timeline", "principles", "persona_context", "family_tree", "remember", "memory_gaps", "compact"] {
        assert!(names.contains(&n), "{n}");
    }
    let recall_tool = tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "recall").unwrap();
    assert_eq!(recall_tool["annotations"]["readOnlyHint"], true);
    assert_eq!(recall_tool["outputSchema"]["type"], "object");
    let comp = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 20, "method": "tools/call", "params": {"name": "compact", "arguments": {"mode": "candidates"}}})).await;
    assert_eq!(comp["result"]["isError"], false, "{comp}");
    assert_eq!(comp["result"]["structuredContent"]["mode"], "candidates");
    assert!(comp["result"]["structuredContent"]["candidates"].is_object());
    let call = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "who_is", "arguments": {"name": "Karno"}}})).await;
    let text = call["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("pakde dari subjek"), "{text}");
    let none = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "recall", "arguments": {"query": "resep rendang padang"}}})).await;
    assert!(none["result"]["content"][0]["text"].as_str().unwrap().contains("Tidak ada catatan"));
    let unknown = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 5, "method": "nope"})).await;
    assert_eq!(unknown["error"]["code"], -32601);
    let persona = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "persona_context", "arguments": {"question": "toko batik", "as_of": "1990-01-01"}}})).await;
    assert!(persona["result"]["content"][0]["text"].as_str().unwrap().contains("TIDAK tahu"));
}

#[tokio::test]
async fn mcp_progressive_disclosure_budget_and_redaction() {
    let (base, st, _d) = spawn(None).await;
    let c = reqwest::Client::new();
    post(&c, format!("{base}/api/subjects"), json!({"id": "budi", "name": "Budi"})).await;
    // Rahasia di teks yang diingat tidak pernah tersimpan mentah; blok <private> dibuang.
    let secret_text = "Pakde Karno membuka toko batik di Solo. <private>alamat rumah rahasia</private> Token deploy: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij0123 dipakai CI.";
    let r = post(&c, format!("{base}/api/subjects/budi/remember"), json!({"text": secret_text, "source": "claude-code:LLM-MEMORY"})).await;
    let src: Value = c.get(format!("{base}/api/sources/{}", r["source_id"].as_str().unwrap())).send().await.unwrap().json().await.unwrap();
    let stored = serde_json::to_string(&src).unwrap();
    assert!(!stored.contains("ghp_ABCDEFGHIJ"), "{stored}");
    assert!(!stored.contains("alamat rumah rahasia"), "{stored}");
    assert!(stored.contains("<redacted type='github_pat'/>"), "{stored}");
    assert_eq!(src["source"]["project"], "LLM-MEMORY");
    // Teks yang seluruhnya privat ditolak.
    let bad = c.post(format!("{base}/api/subjects/budi/remember")).json(&json!({"text": "<private>semua</private>"})).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    st.rt.drain().await.unwrap();

    // recall lewat MCP: indeks ringkas + structuredContent; get_episodes mengambil detail.
    let rec = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "recall", "arguments": {"query": "toko batik Pakde Karno", "project": "LLM-MEMORY"}}})).await;
    let text = rec["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Indeks memori"), "{text}");
    let id = rec["result"]["structuredContent"]["episodes"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(rec["result"]["structuredContent"]["episodes"][0]["project"], "LLM-MEMORY");
    let det = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "get_episodes", "arguments": {"ids": [format!("ep:{id}")]}}})).await;
    assert_eq!(det["result"]["isError"], false, "{det}");
    assert!(det["result"]["content"][0]["text"].as_str().unwrap().contains("toko batik"));
    // Anggaran kecil memotong teks pada pagar terakhir.
    let small = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "recall", "arguments": {"query": "toko batik", "detail": "full", "budget": 200}}})).await;
    assert!(small["result"]["content"][0]["text"].as_str().unwrap().chars().count() <= 200 * 4 + 400);
    // timeline dengan jangkar & project_context slim.
    let tl = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "timeline", "arguments": {"anchor": id}}})).await;
    assert!(tl["result"]["content"][0]["text"].as_str().unwrap().contains("<- jangkar"));
    let pc = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "project_context", "arguments": {"project": "LLM-MEMORY", "budget": 800}}})).await;
    assert_eq!(pc["result"]["structuredContent"]["found"], true, "{pc}");
    assert!(pc["result"]["content"][0]["text"].as_str().unwrap().chars().count() <= 800 * 4 + 400);
    // Episode tersimpan dengan project (bukan null) dan API by-ids bekerja.
    let by: Value = post(&c, format!("{base}/api/subjects/budi/episodes/by-ids"), json!({"ids": [id]})).await;
    assert_eq!(by["episodes"][0]["project"], "LLM-MEMORY");
    let lessons: Value = c.get(format!("{base}/api/subjects/budi/lessons")).send().await.unwrap().json().await.unwrap();
    assert!(lessons.is_array());
}

#[tokio::test]
async fn hook_session_lifecycle_inject_and_file_context() {
    let (base, st, _d) = spawn(None).await;
    let c = reqwest::Client::new();
    post(&c, format!("{base}/api/subjects"), json!({"id": "budi", "name": "Budi"})).await;
    let hook = |ev: &str, body: Value| post(&c, format!("{base}/api/hooks/claude-code/{ev}"), body);
    // Giliran basa-basi: LLM melewatkannya, tidak jadi episode.
    hook("session-init", json!({"session_id": "s1", "project": "LLM-MEMORY", "cwd": "/tmp/x", "prompt": "halo"})).await;
    let r = hook("summarize", json!({"session_id": "s1", "project": "LLM-MEMORY", "assistant_text": "Halo juga, ada yang bisa dibantu hari ini di proyek ini?"})).await;
    assert_eq!(r["ingested"], true, "{r}");
    st.rt.drain().await.unwrap();
    assert_eq!(st.rt.store.count_episodes("budi").await.unwrap(), 0);

    // Giliran kerja: prompt, dua tool (satu dilewati karena duplikat id), stop.
    let init = hook("session-init", json!({"session_id": "s1", "project": "LLM-MEMORY", "cwd": "/tmp/x", "prompt": "perbaiki bug token login <private>password: rahasia123</private>"})).await;
    assert_eq!(init["prompt_number"], 2);
    hook("observation", json!({"session_id": "s1", "project": "LLM-MEMORY", "cwd": "/tmp/x", "tool_use_id": "t1", "tool_name": "Read", "tool_input": {"file_path": "/tmp/x/src/auth.rs"}, "tool_response": "fn expiry() { ms }"})).await;
    hook("observation", json!({"session_id": "s1", "project": "LLM-MEMORY", "cwd": "/tmp/x", "tool_use_id": "t1", "tool_name": "Read", "tool_input": {"file_path": "/tmp/x/src/auth.rs"}, "tool_response": "dup"})).await;
    let obs = hook("observation", json!({"session_id": "s1", "project": "LLM-MEMORY", "cwd": "/tmp/x", "tool_use_id": "t2", "tool_name": "Edit", "tool_input": {"file_path": "/tmp/x/src/auth.rs", "old_string": "ms", "new_string": "s"}, "tool_response": {"stdout": "ok AKIAABCDEFGHIJKLMNOP"}})).await;
    assert_eq!(obs["tool_uses"], 2);
    let sum = hook("summarize", json!({"session_id": "s1", "project": "LLM-MEMORY", "assistant_text": "Selesai. Akar masalah: expiry dihitung dalam detik, bukan milidetik; src/auth.rs diperbaiki."})).await;
    assert_eq!(sum["ingested"], true, "{sum}");
    // Stop kedua pada giliran yang sama tidak menyimpan ulang.
    let again = hook("summarize", json!({"session_id": "s1", "project": "LLM-MEMORY", "assistant_text": "Selesai. Akar masalah: expiry dihitung dalam detik, bukan milidetik; src/auth.rs diperbaiki."})).await;
    assert!(again["ingested"] == false || again["duplicate"] == true, "{again}");
    st.rt.drain().await.unwrap();

    let src: Value = c.get(format!("{base}/api/sources/{}", sum["source_id"].as_str().unwrap())).send().await.unwrap().json().await.unwrap();
    let dump = src.to_string();
    assert!(!dump.contains("rahasia123") && !dump.contains("AKIAABCDEFGHIJKLMNOP"), "{dump}");
    assert_eq!(src["source"]["kind"], "agent_session");
    let eps: Value = c.get(format!("{base}/api/subjects/budi/episodes?limit=50")).send().await.unwrap().json().await.unwrap();
    let eps = eps.as_array().unwrap();
    assert_eq!(eps.len(), 2, "{eps:?}");
    let bug = eps.iter().find(|e| e["obs_type"] == "bugfix").expect("observasi bugfix");
    assert_eq!(bug["project"], "LLM-MEMORY");
    assert_eq!(bug["files_modified"][0], "src/auth.rs");
    assert_eq!(bug["session_id"], "s1");
    assert!(eps.iter().any(|e| e["obs_type"] == "session_summary"));
    let lessons: Value = c.get(format!("{base}/api/subjects/budi/lessons?project=LLM-MEMORY")).send().await.unwrap().json().await.unwrap();
    assert_eq!(lessons[0]["title"], "Expiry token harus dalam detik");
    assert_eq!(lessons[0]["kind"], "pitfall");
    let sessions: Value = c.get(format!("{base}/api/subjects/budi/sessions")).send().await.unwrap().json().await.unwrap();
    assert_eq!(sessions[0]["id"], "s1");
    assert_eq!(sessions[0]["episodes"], 2);

    // Injeksi konteks awal sesi: indeks ber-ID, muat anggaran, menyebut pelajaran & sesi terakhir.
    let ctx = hook("context", json!({"session_id": "s2", "project": "LLM-MEMORY", "cwd": "/tmp/x"})).await;
    assert_eq!(ctx["found"], true, "{ctx}");
    let text = ctx["text"].as_str().unwrap();
    assert!(text.contains("## Sesi terakhir") && text.contains("Expiry token harus dalam detik") && text.contains("[bugfix]"), "{text}");
    assert!(text.chars().count() <= 9500);
    let tight = hook("context", json!({"session_id": "s2", "project": "LLM-MEMORY", "max_chars": 600})).await;
    assert!(tight["text"].as_str().unwrap().chars().count() <= 600, "{tight}");
    assert!(tight["reductions"].as_u64().unwrap() > 0);
    let none = hook("context", json!({"session_id": "s3", "project": "PROYEK-ASING"})).await;
    assert_eq!(none["found"], false);
    // Konteks per berkas.
    let fc = hook("file-context", json!({"session_id": "s2", "project": "LLM-MEMORY", "file_path": "/tmp/x/src/auth.rs"})).await;
    assert_eq!(fc["found"], true, "{fc}");
    assert!(fc["text"].as_str().unwrap().contains("[diubah]"));
    let fc2 = hook("file-context", json!({"session_id": "s2", "file_path": "/tmp/x/src/lain.rs"})).await;
    assert_eq!(fc2["found"], false);
    // Giliran Gemini/Antigravity tanpa tool.
    let turn = post(&c, format!("{base}/api/hooks/antigravity/turn"), json!({"session_id": "c9", "project": "LLM-MEMORY", "user_text": "catat: kita pakai SurrealDB embedded", "assistant_text": "Baik, dicatat bahwa LLM-MEMORY memakai SurrealDB embedded sebagai penyimpanan utama."})).await;
    assert_eq!(turn["ingested"], true, "{turn}");
    let end = hook("session-end", json!({"session_id": "s1", "project": "LLM-MEMORY"})).await;
    assert_eq!(end["ok"], true);
    let s1 = st.rt.store.get_session("s1").await.unwrap().unwrap();
    assert_eq!(s1.status, "completed");
    let bad = c.post(format!("{base}/api/hooks/claude-code/nope")).json(&json!({"session_id": "x"})).send().await.unwrap();
    assert_eq!(bad.status(), 404);
}

#[tokio::test]
async fn token_is_enforced_for_api_and_mcp_but_not_ui() {
    let (base, _st, _d) = spawn(Some("rahasia")).await;
    let c = reqwest::Client::new();
    assert_eq!(c.get(format!("{base}/")).send().await.unwrap().status(), 200);
    assert_eq!(c.get(format!("{base}/api/subjects")).send().await.unwrap().status(), 401);
    assert_eq!(c.post(format!("{base}/mcp")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})).send().await.unwrap().status(), 401);
    let ok = c.get(format!("{base}/api/subjects")).bearer_auth("rahasia").send().await.unwrap();
    assert_eq!(ok.status(), 200);
}

#[tokio::test]
async fn mcp_install_disabled_without_local_url() {
    let (base, _st, _d) = spawn(None).await;
    let c = reqwest::Client::new();
    let r: Value = c.get(format!("{base}/api/mcp/clients")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["enabled"], false);
    let res = c.post(format!("{base}/api/mcp/clients/vscode/install")).send().await.unwrap();
    assert_eq!(res.status(), 400);
    let res = c.post(format!("{base}/api/mcp/clients/nope/install")).send().await.unwrap();
    assert_eq!(res.status(), 404);
}

/// Penjaga: UI tidak boleh memakai glyph/emoji sebagai ikon (font sering tidak
/// punya glyph-nya dan tampil sebagai kotak). Hanya tanda baca tipografis yang
/// tersedia di semua font yang diizinkan; ikon wajib SVG inline.
#[test]
fn ui_has_no_icon_glyphs() {
    const ALLOWED: &[char] = &['·', '–', '—', '…'];
    for (name, src) in [("index.html", crate::INDEX_HTML), ("app.js", crate::APP_JS), ("app.css", crate::APP_CSS)] {
        let bad: Vec<char> = src.chars().filter(|c| !c.is_ascii() && !ALLOWED.contains(c)).collect();
        assert!(bad.is_empty(), "{name} memuat karakter non-ASCII yang dilarang: {bad:?}");
    }
}

/// Penjaga: WebView aplikasi desktop (Tauri) tidak menampilkan dialog browser,
/// jadi `prompt()`/`confirm()`/`alert()` akan diam-diam gagal. Pakai formulir di halaman.
#[test]
fn ui_has_no_browser_dialogs() {
    for call in ["prompt(", "confirm(", "alert("] {
        let used = crate::APP_JS.lines().filter(|l| !l.trim_start().starts_with("//")).any(|l| l.contains(call));
        assert!(!used, "app.js memakai {call}");
    }
}

/// Pengendali palsu: layanan "workers" berhenti sampai dijalankan.
struct FakeServices(std::sync::Mutex<bool>);

impl ServiceControl for FakeServices {
    fn list(&self) -> Vec<ServiceInfo> {
        let workers = *self.0.lock().unwrap();
        vec![
            ServiceInfo { id: "server".into(), name: "Server".into(), description: String::new(), running: true },
            ServiceInfo { id: "workers".into(), name: "Worker".into(), description: String::new(), running: workers },
        ]
    }

    fn start(&self, id: &str) -> Result<(), String> {
        match id {
            "workers" => {
                *self.0.lock().unwrap() = true;
                Ok(())
            }
            "server" => Ok(()),
            _ => Err(format!("layanan tidak dikenal: {id}")),
        }
    }

    fn update_progress(&self) -> Option<crate::settings::UpdateProgress> {
        Some(crate::settings::UpdateProgress {
            running: true,
            stage: "downloading".into(),
            version: Some("0.0.17".into()),
            downloaded_bytes: 5_242_880,
            total_bytes: Some(10_485_760),
            percent: Some(50),
            message: Some("Mengunduh update v0.0.17 — 5.0 / 10.0 MB (50%)".into()),
            error: None,
        })
    }
}

#[tokio::test]
async fn services_status_and_start() {
    let (base, _st, _d) = spawn_with(None, Some(Arc::new(FakeServices(std::sync::Mutex::new(false))))).await;
    let c = reqwest::Client::new();
    let r: Value = c.get(format!("{base}/api/services")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["enabled"], true);
    assert_eq!(r["services"][1]["running"], false);
    assert_eq!(r["update_progress"]["running"], true);
    assert_eq!(r["update_progress"]["percent"], 50);

    let prog: Value = c.get(format!("{base}/api/update/progress")).send().await.unwrap().json().await.unwrap();
    assert_eq!(prog["running"], true);
    assert_eq!(prog["stage"], "downloading");
    assert_eq!(prog["version"], "0.0.17");
    assert_eq!(prog["percent"], 50);
    // Server embedding (di luar engram) ikut dilaporkan; embedder uji selalu hidup.
    let emb = &r["services"][2];
    assert_eq!((emb["id"].as_str(), emb["external"].as_bool(), emb["running"].as_bool()), (Some("embedding"), Some(true), Some(true)));
    let llm = &r["services"][3];
    assert_eq!((llm["id"].as_str(), llm["external"].as_bool(), llm["running"].as_bool()), (Some("llm"), Some(true), Some(true)));

    let r = post(&c, format!("{base}/api/services/workers/start"), json!({})).await;
    assert_eq!(r["services"][1]["running"], true);
    assert_eq!(r["services"][2]["id"], "embedding");

    let bad = c.post(format!("{base}/api/services/nope/start")).send().await.unwrap();
    assert_eq!(bad.status(), 400);
}

#[tokio::test]
async fn services_disabled_without_controller() {
    let (base, _st, _d) = spawn(None).await;
    let r: Value = reqwest::get(format!("{base}/api/services")).await.unwrap().json().await.unwrap();
    assert_eq!(r["enabled"], false);
    let bad = reqwest::Client::new().post(format!("{base}/api/services/workers/start")).send().await.unwrap();
    assert_eq!(bad.status(), 400);
}

#[tokio::test]
async fn storage_and_sync_settings_api() {
    let (base, st, _dir) = spawn(None).await;
    let c = reqwest::Client::new();
    let r: Value = c.get(format!("{base}/api/storage")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["vault"], json!(st.rt.vault.root()));
    assert_eq!(r["can_change"], false, "tanpa aplikasi desktop lokasi tidak bisa diubah dari UI");
    let bad = c.post(format!("{base}/api/storage/move")).json(&json!({"path": "/tmp/x"})).send().await.unwrap();
    assert_eq!(bad.status(), 400);

    // Atur folder sync + kata sandi, lalu jalankan.
    let drive = tempfile::tempdir().unwrap();
    let r: Value = c
        .put(format!("{base}/api/sync"))
        .json(&json!({"dir": drive.path(), "interval_minutes": 5, "device_name": "Laptop", "passphrase": "rahasia-keluarga"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["configured"], true);
    assert_eq!(r["encrypted"], true);
    assert_eq!(r["settings"]["interval_minutes"], 5);
    let run = post(&c, format!("{base}/api/sync/run"), json!({})).await;
    assert_eq!(run["pushed"], true, "{run}");
    let r: Value = c.get(format!("{base}/api/sync")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["devices"][0]["name"], "Laptop");
    assert!(r["last_sync_at"].is_string());

    // Kata sandi terlalu pendek ditolak; folder relatif ditolak.
    let bad = c.put(format!("{base}/api/sync")).json(&json!({"dir": drive.path(), "passphrase": "pendek"})).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    let bad = c.put(format!("{base}/api/sync")).json(&json!({"dir": "relatif/folder"})).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    // Mematikan sync.
    let r: Value = c.put(format!("{base}/api/sync")).json(&json!({"dir": null})).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["configured"], false);
}

#[tokio::test]
async fn about_and_settings_api() {
    let cfg_dir = tempfile::tempdir().unwrap();
    let path = cfg_dir.path().join("memory.toml");
    std::fs::write(&path, "[llm]\nbase_url = \"http://localhost:5211/v1\"\n\n[index]\nhnsw_m = 24\n").unwrap();
    let (base, _st, _d) = spawn_full(None, None, Some(path.clone())).await;
    let c = reqwest::Client::new();

    let a: Value = c.get(format!("{base}/api/about")).send().await.unwrap().json().await.unwrap();
    assert_eq!(a["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(a["host"], "server");
    assert_eq!(a["can_restart"], false);

    let s: Value = c.get(format!("{base}/api/settings")).send().await.unwrap().json().await.unwrap();
    assert_eq!(s["editable"], true);
    assert_eq!(s["values"]["llm"]["base_url"], "http://localhost:5211/v1");
    assert_eq!(s["values"]["llm"]["api_key_set"], false);

    // Simpan: kunci lain di berkas tetap, API key tidak pernah dikirim balik.
    let r = c
        .put(format!("{base}/api/settings"))
        .json(&json!({"values": {"llm": {"extract_model": "qwen3", "api_key": "sk-rahasia"}, "pipeline": {"workers": 4}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let s: Value = r.json().await.unwrap();
    assert_eq!(s["values"]["llm"]["extract_model"], "qwen3");
    assert_eq!(s["values"]["llm"]["api_key_set"], true);
    assert_eq!(s["values"]["pipeline"]["workers"], 4);
    assert_eq!(s["restart_required"], true);
    assert!(!s.to_string().contains("sk-rahasia"));
    let file = std::fs::read_to_string(&path).unwrap();
    assert!(file.contains("hnsw_m = 24"), "{file}");
    assert!(file.contains("sk-rahasia"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    // Kunci di luar daftar dan nilai tidak valid ditolak; berkas tidak berubah.
    for bad in [json!({"index": {"hnsw_m": 8}}), json!({"pipeline": {"workers": 0}}), json!({"embedding": {"provider": "x"}})] {
        let r = c.put(format!("{base}/api/settings")).json(&json!({"values": bad})).send().await.unwrap();
        assert_eq!(r.status(), 400, "{bad}");
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), file);

    // Tes koneksi memakai nilai yang belum disimpan.
    let t = post(&c, format!("{base}/api/settings/test"), json!({"kind": "embedding", "values": {"embedding": {"provider": "hash"}}})).await;
    assert_eq!(t["ok"], true);
    let t = post(&c, format!("{base}/api/settings/test"), json!({"kind": "llm", "values": {"llm": {"base_url": "http://127.0.0.1:9/v1"}}})).await;
    assert_eq!(t["ok"], false);

    // Tanpa aplikasi desktop: buka ulang, pasang update, dan buka folder tidak tersedia.
    for ep in ["restart", "update/install", "open"] {
        let r = c.post(format!("{base}/api/{ep}")).json(&json!({"target": "vault"})).send().await.unwrap();
        assert_eq!(r.status(), 400, "{ep}");
    }
}

#[tokio::test]
async fn settings_read_only_without_config_path() {
    let (base, _st, _d) = spawn(None).await;
    let c = reqwest::Client::new();
    let s: Value = c.get(format!("{base}/api/settings")).send().await.unwrap().json().await.unwrap();
    assert_eq!(s["editable"], false);
    assert_eq!(s["restart_required"], false);
    assert_eq!(s["values"]["embedding"]["provider"], "hash");
    let r = c.put(format!("{base}/api/settings")).json(&json!({"values": {"pipeline": {"workers": 3}}})).send().await.unwrap();
    assert_eq!(r.status(), 400);
}

#[tokio::test]
async fn watched_folders_api() {
    let (base, _st, _d) = spawn(None).await;
    let c = reqwest::Client::new();
    let important = tempfile::tempdir().unwrap();
    std::fs::write(important.path().join("catatan.md"), "Budi mencatat arsitektur sistem memori.").unwrap();

    let added = post(
        &c,
        format!("{base}/api/subjects/budi/watched-folders"),
        json!({"path": important.path(), "label": "Catatan Arsitektur", "scan": false}),
    )
    .await;
    let fid = added["folder"]["id"].as_str().unwrap().to_string();
    assert_eq!(added["folder"]["label"], "Catatan Arsitektur");

    let list: Value = c
        .get(format!("{base}/api/subjects/budi/watched-folders"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list["folders"].as_array().unwrap().len(), 1);

    let removed = post(&c, format!("{base}/api/watched-folders/{fid}/remove"), json!({})).await;
    assert_eq!(removed["ok"], true);

    let list_after: Value = c
        .get(format!("{base}/api/subjects/budi/watched-folders"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(list_after["folders"].as_array().unwrap().is_empty());
}
