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
    let notif = c.post(format!("{base}/mcp")).json(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).send().await.unwrap();
    assert_eq!(notif.status(), 202);
    let tools = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["recall", "who_is", "timeline", "principles", "persona_context", "family_tree", "remember", "memory_gaps", "compact"] {
        assert!(names.contains(&n), "{n}");
    }
    let comp = post(&c, format!("{base}/mcp"), json!({"jsonrpc": "2.0", "id": 20, "method": "tools/call", "params": {"name": "compact", "arguments": {"mode": "candidates"}}})).await;
    assert_eq!(comp["result"]["isError"], false, "{comp}");
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
}

#[tokio::test]
async fn services_status_and_start() {
    let (base, _st, _d) = spawn_with(None, Some(Arc::new(FakeServices(std::sync::Mutex::new(false))))).await;
    let c = reqwest::Client::new();
    let r: Value = c.get(format!("{base}/api/services")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["enabled"], true);
    assert_eq!(r["services"][1]["running"], false);
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
