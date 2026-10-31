use std::sync::Arc;

use memory_core::MemoryConfig;
use memory_import::Importer;
use memory_jobs::Runtime;
use memory_llm::mock::{ScriptedLlm, last_user};
use memory_llm::{ChatRequest, HashEmbedder};
use serde_json::{Value, json};

use crate::{AppState, router};

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
            "kin": [{"person": "Pakde Karno", "term": "pakde", "quote": "Pakde Karno"}]}).to_string(),
        "interview" => json!({"text": "Ceritakan masa kecil Bapak?"}).to_string(),
        _ => json!({"text": "ok"}).to_string(),
    })
}

async fn spawn(token: Option<&str>) -> (String, AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = MemoryConfig::for_tests(DIM);
    cfg.vault.path = dir.path().to_path_buf();
    cfg.vault.default_subject = "budi".into();
    cfg.server.token_env = None;
    let rt = Runtime::open_with(cfg, Arc::new(ScriptedLlm::new(llm)), Arc::new(HashEmbedder::new(DIM))).await.unwrap();
    let importer = Importer::new(rt.pipeline.clone(), None);
    let mut st = AppState::new(rt, importer);
    st.token = token.map(|t| Arc::new(t.to_string()));
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
    assert!(html.contains("mnemonic-memory"));
    post(&c, format!("{base}/api/subjects"), json!({"id": "budi", "name": "Budi", "birth": "1979-03-10"})).await;
    let r = post(&c, format!("{base}/api/subjects/budi/remember"), json!({"text": "Pakde Karno membuka toko batik di Solo waktu saya kecil.", "source": "uji"})).await;
    assert!(r["job_id"].is_string());
    st.rt.drain().await.unwrap();

    let stats: Value = c.get(format!("{base}/api/subjects/budi/stats")).send().await.unwrap().json().await.unwrap();
    assert_eq!(stats["stats"]["episodes"], 1);
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
    let notif = c.post(format!("{base}/mcp")).json(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).send().await.unwrap();
    assert_eq!(notif.status(), 202);
    let tools = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["recall", "who_is", "timeline", "principles", "persona_context", "family_tree", "remember", "memory_gaps"] {
        assert!(names.contains(&n), "{n}");
    }
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
async fn token_is_enforced_for_api_and_mcp_but_not_ui() {
    let (base, _st, _d) = spawn(Some("rahasia")).await;
    let c = reqwest::Client::new();
    assert_eq!(c.get(format!("{base}/")).send().await.unwrap().status(), 200);
    assert_eq!(c.get(format!("{base}/api/subjects")).send().await.unwrap().status(), 401);
    assert_eq!(c.post(format!("{base}/mcp")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})).send().await.unwrap().status(), 401);
    let ok = c.get(format!("{base}/api/subjects")).bearer_auth("rahasia").send().await.unwrap();
    assert_eq!(ok.status(), 200);
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
