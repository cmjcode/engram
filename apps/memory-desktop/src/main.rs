//! Aplikasi desktop mnemonic-memory.
//!
//! Satu proses berisi seluruh mesin: vault SurrealDB, worker job, scheduler,
//! pemantau inbox, dan server HTTP lokal (UI + API + MCP). Jendela aplikasi
//! menampilkan UI web yang sama dengan `memory serve`.
//!
//! - Data ada di folder data aplikasi (macOS: `~/Library/Application Support/id.memorihidup.desktop/`).
//! - Menutup jendela hanya menyembunyikannya; worker tetap jalan dari ikon menu bar.
//! - Alamat & token MCP ditulis ke `connection.json` di folder data, untuk agent AI.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use tauri::async_runtime::JoinHandle;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use memory_core::MemoryConfig;
use memory_import::Importer;
use memory_jobs::Runtime;
use memory_server::AppState;

const MAIN: &str = "main";

/// Status proses yang dibagikan ke handler menu & event.
struct Desktop {
    shutdown: tokio::sync::watch::Sender<bool>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    data_dir: PathBuf,
}

/// Hasil menjalankan mesin memori.
struct Started {
    url: String,
    token: String,
}

fn load_or_create_config(data_dir: &Path) -> Result<MemoryConfig> {
    let path = data_dir.join("memory.toml");
    if !path.exists() {
        let mut cfg = MemoryConfig::default();
        cfg.vault.path = data_dir.join("vault");
        std::fs::write(&path, cfg.to_toml()).with_context(|| format!("menulis {}", path.display()))?;
    }
    let s = std::fs::read_to_string(&path).with_context(|| format!("membaca {}", path.display()))?;
    let mut cfg = MemoryConfig::from_toml(&s).with_context(|| format!("format {}", path.display()))?;
    if cfg.vault.path.is_relative() {
        cfg.vault.path = data_dir.join(&cfg.vault.path);
    }
    Ok(cfg)
}

fn random_token() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    (0..32).map(|_| format!("{:x}", rng.random_range(0..16u8))).collect()
}

/// Tulis berkas hanya-pemilik (token akses tidak boleh terbaca pengguna lain).
fn write_private(path: &Path, body: &str) -> Result<()> {
    std::fs::write(path, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Buka vault, jalankan server + worker + scheduler + inbox.
fn start_engine(desktop: &Desktop) -> Result<Started> {
    let cfg = load_or_create_config(&desktop.data_dir)?;
    tauri::async_runtime::block_on(async {
        let rt = Runtime::open(cfg).await.context("membuka vault")?;
        let transcriber = memory_llm::WhisperTranscriber::new(&rt.cfg.transcription).map(|w| Arc::new(w) as Arc<dyn memory_llm::Transcriber>);
        let importer = Importer::new(rt.pipeline.clone(), transcriber);
        let mut state = AppState::new(rt.clone(), importer.clone());
        // Token wajib: server lokal tidak boleh bisa dipakai situs web lain di browser.
        let token = match &state.token {
            Some(t) => t.to_string(),
            None => {
                let t = random_token();
                state.token = Some(Arc::new(t.clone()));
                t
            }
        };
        // Pakai port konfigurasi bila bebas (agar alamat MCP tetap), selain itu port acak.
        let listener = match tokio::net::TcpListener::bind(&rt.cfg.server.bind).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(bind = %rt.cfg.server.bind, error = %e, "port terpakai, memakai port acak");
                tokio::net::TcpListener::bind("127.0.0.1:0").await?
            }
        };
        let url = format!("http://{}/", listener.local_addr()?);
        write_private(
            &desktop.data_dir.join("connection.json"),
            &serde_json::to_string_pretty(&serde_json::json!({
                "url": url,
                "mcp_url": format!("{url}mcp"),
                "token": token,
                "vault": rt.vault.root(),
                "note": "Kirim header Authorization: Bearer <token>. Berkas ini diperbarui setiap aplikasi dibuka."
            }))?,
        )?;

        let stop = desktop.shutdown.subscribe();
        let mut tasks = desktop.tasks.lock().expect("mutex tasks");
        let (r, s) = (rt.clone(), stop.clone());
        tasks.push(tauri::async_runtime::spawn(async move { r.run_workers(s).await }));
        let (r, s) = (rt.clone(), stop.clone());
        tasks.push(tauri::async_runtime::spawn(async move { r.run_scheduler(s).await }));
        let (subj, poll, s) = (rt.cfg.vault.default_subject.clone(), rt.cfg.schedule.inbox_poll_secs, stop.clone());
        tasks.push(tauri::async_runtime::spawn(async move { importer.watch_inbox(subj, poll, s).await }));
        tasks.push(tauri::async_runtime::spawn(async move {
            if let Err(e) = memory_server::serve_listener(state, listener, stop).await {
                tracing::error!(error = %e, "server berhenti");
            }
        }));
        Ok(Started { url, token })
    })
}

fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn build_main_window(app: &AppHandle, started: &Result<Started>) -> tauri::Result<()> {
    let builder = match started {
        Ok(s) => {
            let url = s.url.parse().expect("url server lokal valid");
            // Token disimpan sebelum UI berjalan, jadi UI tidak pernah meminta token.
            let init = format!("try {{ localStorage.setItem('token', {}); }} catch (e) {{}}", js_string(&s.token));
            WebviewWindowBuilder::new(app, MAIN, WebviewUrl::External(url)).initialization_script(&init)
        }
        Err(e) => {
            let init = format!("window.__STARTUP_ERROR = {};", js_string(&format!("{e:#}")));
            WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into())).initialization_script(&init)
        }
    };
    builder.title("mnemonic-memory").inner_size(1200.0, 820.0).min_inner_size(720.0, 520.0).build()?;
    Ok(())
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn open_in_file_manager(path: &Path) {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = "xdg-open";
    if let Err(e) = std::process::Command::new(cmd).arg(path).spawn() {
        tracing::warn!(error = %e, "gagal membuka folder");
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Buka mnemonic-memory", true, None::<&str>)?;
    let inbox = MenuItem::with_id(app, "inbox", "Buka folder inbox", true, None::<&str>)?;
    let data = MenuItem::with_id(app, "data", "Buka folder data", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Keluar", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &inbox, &data, &sep, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main").tooltip("mnemonic-memory").menu(&menu).show_menu_on_left_click(true);
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.on_menu_event(|app, ev| {
        let desktop = app.state::<Desktop>();
        match ev.id().as_ref() {
            "open" => show_main(app),
            "inbox" => {
                let dir = load_or_create_config(&desktop.data_dir).map(|c| c.vault.path.join("inbox")).unwrap_or_else(|_| desktop.data_dir.clone());
                let _ = std::fs::create_dir_all(&dir);
                open_in_file_manager(&dir);
            }
            "data" => open_in_file_manager(&desktop.data_dir),
            "quit" => app.exit(0),
            _ => {}
        }
    })
    .build(app)?;
    Ok(())
}

/// Hentikan worker & server dengan rapi agar database tertutup bersih.
fn stop_engine(app: &AppHandle) {
    let desktop = app.state::<Desktop>();
    let _ = desktop.shutdown.send(true);
    let tasks: Vec<JoinHandle<()>> = desktop.tasks.lock().map(|mut t| t.drain(..).collect()).unwrap_or_default();
    tauri::async_runtime::block_on(async move {
        for t in tasks {
            let _ = tokio::time::timeout(Duration::from_secs(10), t).await;
        }
    });
}

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,surrealdb=warn,surrealdb_core=warn,surrealdb_core::kvs::tasks=off".into()),
        )
        .init();

    let app = tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let (shutdown, _) = tokio::sync::watch::channel(false);
            app.manage(Desktop { shutdown, tasks: Mutex::new(Vec::new()), data_dir });
            let started = start_engine(&app.state::<Desktop>());
            if let Err(e) = &started {
                tracing::error!("mesin memori gagal: {e:#}");
            }
            build_main_window(app.handle(), &started)?;
            build_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Tutup jendela = sembunyikan; aplikasi tetap memproses dari menu bar.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("gagal membangun aplikasi");

    app.run(|app, event| match event {
        RunEvent::Exit => stop_engine(app),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_main(app),
        _ => {}
    });
}
