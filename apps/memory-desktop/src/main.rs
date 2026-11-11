//! Aplikasi desktop engram.
//!
//! Satu proses berisi seluruh mesin: vault SurrealDB, worker job, scheduler,
//! pemantau inbox, dan server HTTP lokal (UI + API + MCP). Jendela aplikasi
//! menampilkan UI web yang sama dengan `engram serve`.
//!
//! - Data ada di folder data aplikasi (macOS: `~/Library/Application Support/id.engram.ai/`),
//!   atau di folder lain pilihan pengguna (Pengaturan > "Data & Sync" di UI; dipindah saat aplikasi dibuka ulang).
//! - Sinkronisasi antar komputer lewat folder Google Drive: lihat `memory_jobs::sync`.
//! - Menutup jendela hanya menyembunyikannya; worker tetap jalan dari ikon menu bar.
//! - Alamat & token MCP ditulis ke `connection.json` di folder data, untuk agent AI.
//!   Tab "Agent AI" di Pengaturan UI mendaftarkannya ke Antigravity, VS Code, atau Claude Code.
//! - Update otomatis dari GitHub Release (lihat `update.rs`).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod i18n;
mod services;
mod update;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use memory_core::MemoryConfig;
use memory_import::Importer;
use memory_jobs::Runtime;
use memory_server::AppState;

use i18n::{tr, trf};
use services::{Kind, Services};

const MAIN: &str = "main";

/// Status proses yang dibagikan ke handler menu & event.
struct Desktop {
    shutdown: tokio::sync::watch::Sender<bool>,
    /// Layanan latar; kosong bila mesin memori gagal dijalankan.
    engine: OnceLock<Arc<Services>>,
    data_dir: PathBuf,
    /// Baris status layanan di menu bar (diganti saat menu dibangun ulang).
    svc_items: Mutex<Vec<MenuItem<tauri::Wry>>>,
    /// Menu baru dibangun (mis. bahasa berganti): status layanan perlu ditulis ulang.
    tray_dirty: AtomicBool,
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

/// Penanda pemindahan vault yang menunggu aplikasi dibuka ulang.
const MOVE_MARKER: &str = "vault_move.json";

/// Catat permintaan pindah vault ke `target`; dikerjakan saat aplikasi dibuka lagi.
fn request_move(data_dir: &Path, target: &Path) -> Result<()> {
    let body = serde_json::to_string_pretty(&serde_json::json!({ "to": target }))?;
    std::fs::write(data_dir.join(MOVE_MARKER), body).context("menulis penanda pemindahan")?;
    Ok(())
}

/// Kerjakan pemindahan vault yang diminta sebelum database dibuka.
/// `Ok(None)` bila tidak ada permintaan. Vault lama tidak dihapus.
fn apply_pending_move(data_dir: &Path) -> Result<Option<String>> {
    let marker = data_dir.join(MOVE_MARKER);
    let Ok(raw) = std::fs::read_to_string(&marker) else { return Ok(None) };
    // Dihapus lebih dulu: bila gagal, aplikasi tetap terbuka dengan vault lama (tidak berulang).
    std::fs::remove_file(&marker)?;
    let to: PathBuf = serde_json::from_str::<serde_json::Value>(&raw)?["to"].as_str().map(PathBuf::from).context("penanda pemindahan rusak")?;
    let cfg = load_or_create_config(data_dir)?;
    let from = cfg.vault.path.clone();
    let plan = memory_jobs::storage::plan_move(&from, &to)?;
    let files = memory_jobs::storage::copy_vault(&from, &plan)?;
    let path = data_dir.join("memory.toml");
    let mut file_cfg = MemoryConfig::from_toml(&std::fs::read_to_string(&path)?)?;
    file_cfg.vault.path = plan.target.clone();
    std::fs::write(&path, file_cfg.to_toml()).with_context(|| format!("menulis {}", path.display()))?;
    tracing::info!(from = %from.display(), to = %plan.target.display(), files, "vault dipindahkan");
    Ok(Some(if plan.use_existing {
        tr!("Now using the data in:\n{}\n\nThe old data is still in:\n{}", "Sekarang memakai data di:\n{}\n\nData lama tetap ada di:\n{}", plan.target.display(), from.display())
    } else {
        tr!(
            "Data moved to:\n{}\n\nThe old copy is still in:\n{}\nDelete that old folder once everything works.",
            "Data dipindahkan ke:\n{}\n\nSalinan lama masih ada di:\n{}\nHapus folder lama itu bila semuanya sudah berjalan baik.",
            plan.target.display(),
            from.display()
        )
    }))
}

/// Token akses tetap di `mcp_token` (berkas yang sama dipakai `scripts/install-mcp.sh`).
fn load_or_create_token(data_dir: &Path) -> Result<String> {
    let path = data_dir.join("mcp_token");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let t = s.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    let t = random_token();
    write_private(&path, &format!("{t}\n")).with_context(|| format!("menulis {}", path.display()))?;
    Ok(t)
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
fn start_engine(app: &AppHandle, desktop: &Desktop) -> Result<Started> {
    let cfg = load_or_create_config(&desktop.data_dir)?;
    tauri::async_runtime::block_on(async {
        let rt = Runtime::open(cfg).await.context(tr!("opening the vault", "membuka vault"))?;
        let transcriber = memory_llm::WhisperTranscriber::new(&rt.cfg.transcription).map(|w| Arc::new(w) as Arc<dyn memory_llm::Transcriber>);
        let importer = Importer::new(rt.pipeline.clone(), transcriber);
        let mut state = AppState::new(rt.clone(), importer.clone());
        // Token wajib: server lokal tidak boleh bisa dipakai situs web lain di browser.
        // Token disimpan agar config MCP di agent AI tetap berlaku setelah aplikasi dibuka ulang.
        let token = match &state.token {
            Some(t) => t.to_string(),
            None => {
                let t = load_or_create_token(&desktop.data_dir)?;
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
        let services = Arc::new(Services::new(app.clone(), desktop.data_dir.clone(), rt.clone(), importer, listener.local_addr()?, desktop.shutdown.subscribe()));
        let url = services.url();
        state.mcp_url = Some(format!("{url}mcp"));
        state.config_path = Some(desktop.data_dir.join("memory.toml"));
        state.services = Some(services.clone());
        services.set_state(state);
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

        services.start_server_with(listener).map_err(anyhow::Error::msg)?;
        for k in [Kind::Workers, Kind::Scheduler, Kind::Inbox] {
            services.start_kind(k).map_err(anyhow::Error::msg)?;
        }
        let _ = desktop.engine.set(services);
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
            // Bahasa dari aplikasi desktop dipakai juga oleh UI web.
            let init = format!(
                "try {{ localStorage.setItem('token', {}); localStorage.setItem('lang', {}); }} catch (e) {{}}",
                js_string(&s.token),
                js_string(i18n::code())
            );
            WebviewWindowBuilder::new(app, MAIN, WebviewUrl::External(url)).initialization_script(&init)
        }
        Err(e) => {
            let init = format!("window.__STARTUP_ERROR = {}; window.__LANG = {};", js_string(&format!("{e:#}")), js_string(i18n::code()));
            WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into())).initialization_script(&init)
        }
    };
    builder.title("engram").inner_size(1200.0, 820.0).min_inner_size(720.0, 520.0).build()?;
    Ok(())
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Buka folder di pengelola berkas, atau URL di browser bawaan.
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

/// Menu bar dalam bahasa aktif, plus baris status tiap layanan.
fn tray_menu(app: &AppHandle) -> tauri::Result<(Menu<tauri::Wry>, Vec<MenuItem<tauri::Wry>>)> {
    let open = MenuItem::with_id(app, "open", tr!("Open engram", "Buka engram"), true, None::<&str>)?;
    let inbox = MenuItem::with_id(app, "inbox", tr!("Open inbox folder", "Buka folder inbox"), true, None::<&str>)?;
    let data = MenuItem::with_id(app, "data", tr!("Open data folder", "Buka folder data"), true, None::<&str>)?;
    let version = tr!("Version {}", "Versi {}", app.package_info().version);
    let version = MenuItem::with_id(app, "version", version, false, None::<&str>)?;
    let check_update = MenuItem::with_id(app, "check_update", tr!("Check for updates", "Periksa update"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tr!("Quit", "Keluar"), true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let header = MenuItem::with_id(app, "svc_header", tr!("Services", "Layanan"), false, None::<&str>)?;
    // Satu baris per layanan: teks & aktif/tidaknya diperbarui oleh `watch_services`.
    let svc_items = Kind::ALL
        .into_iter()
        .map(|k| MenuItem::with_id(app, format!("svc:{}", k.id()), k.name(), false, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let mut items: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = vec![&open, &inbox, &data, &sep, &header];
    items.extend(svc_items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>));
    items.extend([&sep3 as &dyn tauri::menu::IsMenuItem<tauri::Wry>, &version, &check_update, &sep2, &quit]);
    let menu = Menu::with_items(app, &items)?;
    Ok((menu, svc_items))
}

/// Bangun ulang menu bar (dipanggil saat bahasa diganti dari UI).
pub fn refresh_tray(app: &AppHandle) -> tauri::Result<()> {
    let (menu, items) = tray_menu(app)?;
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_menu(Some(menu))?;
    }
    let desktop = app.state::<Desktop>();
    if let Ok(mut g) = desktop.svc_items.lock() {
        *g = items;
    }
    desktop.tray_dirty.store(true, Ordering::SeqCst);
    Ok(())
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let (menu, svc_items) = tray_menu(app)?;
    if let Ok(mut g) = app.state::<Desktop>().svc_items.lock() {
        *g = svc_items;
    }
    let mut tray = TrayIconBuilder::with_id("main").tooltip("engram").menu(&menu).show_menu_on_left_click(true);
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
            "data" => {
                let dir = load_or_create_config(&desktop.data_dir).map(|c| c.vault.path).unwrap_or_else(|_| desktop.data_dir.clone());
                open_in_file_manager(&dir);
            }
            "check_update" => {
                tauri::async_runtime::spawn(update::check(app.clone(), true));
            }
            "quit" => app.exit(0),
            id => {
                if let Some(kind) = id.strip_prefix("svc:").and_then(Kind::from_id) {
                    start_service(app, kind);
                }
            }
        }
    })
    .build(app)?;
    let stop = app.state::<Desktop>().shutdown.subscribe();
    tauri::async_runtime::spawn(watch_services(app.clone(), stop));
    Ok(())
}

/// Label menu bar untuk satu layanan.
fn service_label(kind: Kind, running: bool) -> String {
    if running {
        tr!("{}: running", "{}: berjalan", kind.name())
    } else {
        tr!("{}: stopped - click to start", "{}: berhenti - klik untuk menjalankan", kind.name())
    }
}

/// Perbarui status layanan di menu bar secara berkala.
async fn watch_services(app: AppHandle, mut stop: tokio::sync::watch::Receiver<bool>) {
    let mut last: Option<Vec<bool>> = None;
    loop {
        let desktop = app.state::<Desktop>();
        if desktop.tray_dirty.swap(false, Ordering::SeqCst) {
            last = None;
        }
        let items = desktop.svc_items.lock().map(|g| g.clone()).unwrap_or_default();
        let engine = desktop.engine.get().cloned();
        let status: Vec<bool> = Kind::ALL.iter().map(|k| engine.as_ref().is_some_and(|e| e.is_running(*k))).collect();
        if last.as_ref() != Some(&status) {
            for ((kind, item), running) in Kind::ALL.iter().zip(&items).zip(&status) {
                let _ = item.set_text(service_label(*kind, *running));
                // Hanya layanan yang berhenti (dan mesin sudah siap) yang bisa diklik.
                let _ = item.set_enabled(engine.is_some() && !running);
            }
            last = Some(status);
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(3)) => {}
            _ = stop.changed() => return,
        }
    }
}

/// Jalankan layanan yang berhenti dari menu bar.
fn start_service(app: &AppHandle, kind: Kind) {
    let Some(engine) = app.state::<Desktop>().engine.get().cloned() else { return };
    match engine.start_kind(kind) {
        Ok(()) => {
            // Jendela utama dimuat dari server ini; muat ulang setelah server hidup lagi.
            if kind == Kind::Server
                && let (Some(w), Ok(url)) = (app.get_webview_window(MAIN), engine.url().parse())
            {
                let _ = w.navigate(url);
            }
        }
        Err(e) => {
            tracing::error!(service = kind.id(), error = %e, "gagal menjalankan layanan");
            use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
            app.dialog().message(tr!("{} failed to start:\n{e}", "{} gagal dijalankan:\n{e}", kind.name())).title("engram").kind(MessageDialogKind::Error).show(|_| {});
        }
    }
}

/// Hentikan worker & server dengan rapi agar database tertutup bersih.
/// Aman dipanggil lebih dari sekali (panggilan berikutnya tidak berbuat apa-apa).
fn stop_engine(app: &AppHandle) {
    let desktop = app.state::<Desktop>();
    let _ = desktop.shutdown.send(true);
    let tasks = desktop.engine.get().map(|e| e.take_all()).unwrap_or_default();
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let (shutdown, _) = tokio::sync::watch::channel(false);
            let moved = apply_pending_move(&data_dir);
            if let Err(e) = &moved {
                tracing::error!("pemindahan data gagal: {e:#}");
            }
            i18n::load(&data_dir);
            app.manage(Desktop { shutdown, engine: OnceLock::new(), data_dir, svc_items: Mutex::new(Vec::new()), tray_dirty: AtomicBool::new(false) });
            let started = start_engine(app.handle(), &app.state::<Desktop>());
            if let Err(e) = &started {
                tracing::error!("mesin memori gagal: {e:#}");
            }
            build_main_window(app.handle(), &started)?;
            build_tray(app.handle())?;
            {
                use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
                match moved {
                    Ok(Some(msg)) => app.dialog().message(msg).title(tr!("Data location changed", "Lokasi data diubah")).kind(MessageDialogKind::Info).show(|_| {}),
                    Ok(None) => {}
                    Err(e) => app
                        .dialog()
                        .message(trf!("Data was not moved; the app keeps using the old location.\n\n{e:#}", "Data tidak dipindahkan, aplikasi tetap memakai lokasi lama.\n\n{e:#}"))
                        .title(tr!("Failed to move data", "Gagal memindah data"))
                        .kind(MessageDialogKind::Error)
                        .show(|_| {}),
                }
            }
            // Tidak dimasukkan ke `tasks`: tidak memegang database, dan stop_engine
            // bisa dipanggil dari dalam proses update itu sendiri.
            let stop = app.state::<Desktop>().shutdown.subscribe();
            tauri::async_runtime::spawn(update::run_background(app.handle().clone(), stop));
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
