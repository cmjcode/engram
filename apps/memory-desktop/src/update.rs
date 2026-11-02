//! Pembaruan otomatis lewat `tauri-plugin-updater`.
//!
//! Aplikasi membaca `latest.json` dari GitHub Release terbaru (lihat
//! `plugins.updater` di `tauri.conf.json`). Paket update ditandatangani dengan
//! kunci updater; tanda tangan dicek sebelum dipasang.
//!
//! - Cek otomatis tidak lama setelah aplikasi dibuka, lalu berkala.
//! - Cek manual dari menu tray "Periksa update" atau dari Pengaturan di UI.
//! - Cek otomatis bisa dimatikan di Pengaturan (disimpan di `desktop.json`).
//! - Update baru dipasang setelah pengguna setuju. Mesin memori dihentikan dulu
//!   agar database tertutup bersih, lalu aplikasi dibuka ulang.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};

use memory_server::settings::UpdateInfo;

/// Preferensi aplikasi desktop (bukan bagian memory.toml).
const PREFS: &str = "desktop.json";

/// Cek update otomatis aktif? Default: ya.
pub fn auto_enabled(data_dir: &Path) -> bool {
    std::fs::read_to_string(data_dir.join(PREFS))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["auto_update"].as_bool())
        .unwrap_or(true)
}

pub fn set_auto(data_dir: &Path, on: bool) -> Result<()> {
    let path = data_dir.join(PREFS);
    let mut v = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).filter(|v| v.is_object()).unwrap_or_else(|| serde_json::json!({}));
    v["auto_update"] = serde_json::Value::Bool(on);
    std::fs::write(&path, serde_json::to_string_pretty(&v)?).with_context(|| format!("menulis {}", path.display()))
}

const FIRST_CHECK: Duration = Duration::from_secs(30);
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const TITLE: &str = "Update engram";

/// Hanya satu pemeriksaan/pemasangan yang boleh berjalan.
static BUSY: AtomicBool = AtomicBool::new(false);
/// Versi yang ditolak pengguna; cek otomatis tidak menawarkannya lagi.
static DECLINED: Mutex<Option<String>> = Mutex::new(None);

/// Cek otomatis berkala sampai aplikasi ditutup.
pub async fn run_background(app: AppHandle, mut stop: tokio::sync::watch::Receiver<bool>) {
    let mut wait = FIRST_CHECK;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = stop.changed() => return,
        }
        if auto_enabled(&app.state::<crate::Desktop>().data_dir) {
            check(app.clone(), false).await;
        }
        wait = INTERVAL;
    }
}

/// Cek update. `manual` = dipicu pengguna: hasil "sudah terbaru" dan error ikut ditampilkan.
pub async fn check(app: AppHandle, manual: bool) {
    if BUSY.swap(true, Ordering::SeqCst) {
        if manual {
            message(&app, MessageDialogKind::Info, "Pemeriksaan update sedang berjalan.").await;
        }
        return;
    }
    let res = check_inner(&app, manual).await;
    BUSY.store(false, Ordering::SeqCst);
    set_tooltip(&app, None);
    if let Err(e) = res {
        tracing::warn!("update gagal: {e:#}");
        if manual {
            message(&app, MessageDialogKind::Error, &format!("Gagal memeriksa atau memasang update:\n{e:#}")).await;
        }
    }
}

/// Rilis yang lebih baru dari aplikasi ini, bila ada.
async fn find(app: &AppHandle) -> Result<Option<Update>> {
    let app_for_exit = app.clone();
    let updater = app
        .updater_builder()
        // Windows: installer menutup aplikasi lewat process::exit, jadi hentikan mesin dulu.
        .on_before_exit(move || crate::stop_engine(&app_for_exit))
        .build()
        .context("menyiapkan updater")?;
    updater.check().await.context("membaca info rilis")
}

/// Cek update untuk halaman Pengaturan (tanpa dialog).
pub async fn info(app: &AppHandle) -> Result<UpdateInfo> {
    let current = app.package_info().version.to_string();
    let update = find(app).await?;
    Ok(UpdateInfo {
        available: update.is_some(),
        latest: Some(update.as_ref().map(|u| u.version.clone()).unwrap_or_else(|| current.clone())),
        notes: update.as_ref().and_then(|u| u.body.clone()).filter(|s| !s.trim().is_empty()),
        date: update.as_ref().and_then(|u| u.date).and_then(|d| chrono::DateTime::from_timestamp(d.unix_timestamp(), 0)).map(|d| d.to_rfc3339()),
        can_install: update.is_some(),
        url: format!("{}/releases/latest", memory_server::settings::REPO_URL),
        current,
    })
}

/// Pasang update yang diminta dari UI (pengguna sudah setuju di sana).
pub fn start_install(app: &AppHandle) -> Result<(), String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("pemeriksaan atau pemasangan update sedang berjalan".into());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let res = async {
            let update = find(&app).await?.context("tidak ada update baru")?;
            install(&app, update).await
        }
        .await;
        BUSY.store(false, Ordering::SeqCst);
        set_tooltip(&app, None);
        if let Err(e) = res {
            tracing::warn!("update gagal: {e:#}");
            message(&app, MessageDialogKind::Error, &format!("Gagal memasang update:\n{e:#}")).await;
        }
    });
    Ok(())
}

async fn check_inner(app: &AppHandle, manual: bool) -> Result<()> {
    let Some(update) = find(app).await? else {
        tracing::info!("aplikasi sudah versi terbaru");
        if manual {
            let v = app.package_info().version.to_string();
            message(app, MessageDialogKind::Info, &format!("Sudah versi terbaru ({v}).")).await;
        }
        return Ok(());
    };
    tracing::info!(version = %update.version, "update tersedia");

    if !manual && DECLINED.lock().map(|d| d.as_deref() == Some(update.version.as_str())).unwrap_or(false) {
        return Ok(());
    }
    let mut text = format!("Versi {} tersedia (sekarang {}).", update.version, update.current_version);
    if let Some(notes) = update.body.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let notes: String = notes.chars().take(600).collect();
        text.push_str(&format!("\n\n{notes}"));
    }
    text.push_str("\n\nPasang sekarang? Aplikasi akan dibuka ulang.");
    if !ask(app, &text, "Pasang", "Nanti").await {
        if let Ok(mut d) = DECLINED.lock() {
            *d = Some(update.version.clone());
        }
        return Ok(());
    }
    install(app, update).await
}

/// Unduh, pasang, lalu buka ulang aplikasi.
async fn install(app: &AppHandle, update: Update) -> Result<()> {
    let mut done: usize = 0;
    let bytes = update
        .download(
            |chunk, total| {
                done += chunk;
                let label = match total {
                    Some(t) if t > 0 => format!("Mengunduh update {}%", done as u64 * 100 / t),
                    _ => format!("Mengunduh update {} MB", done / 1_000_000),
                };
                set_tooltip(app, Some(&label));
            },
            || {},
        )
        .await
        .context("mengunduh update")?;
    set_tooltip(app, Some("Memasang update"));

    // install() bisa memblok (dan di Windows memanggil on_before_exit), jadi jalankan di luar runtime async.
    tauri::async_runtime::spawn_blocking(move || update.install(bytes))
        .await
        .context("tugas pemasangan")?
        .context("memasang update")?;

    // macOS / Linux: berkas aplikasi sudah diganti. Buka ulang lewat RunEvent::Exit
    // agar mesin memori dihentikan dengan rapi (lihat stop_engine).
    tracing::info!("update terpasang, membuka ulang aplikasi");
    app.request_restart();
    Ok(())
}

fn set_tooltip(app: &AppHandle, status: Option<&str>) {
    if let Some(tray) = app.tray_by_id("main") {
        let text = match status {
            Some(s) => format!("engram - {s}"),
            None => "engram".to_string(),
        };
        let _ = tray.set_tooltip(Some(text));
    }
}

async fn ask(app: &AppHandle, text: &str, ok: &str, cancel: &str) -> bool {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(text)
        .title(TITLE)
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(ok.into(), cancel.into()))
        .show(move |yes| {
            let _ = tx.send(yes);
        });
    rx.await.unwrap_or(false)
}

async fn message(app: &AppHandle, kind: MessageDialogKind, text: &str) {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(text)
        .title(TITLE)
        .kind(kind)
        .buttons(MessageDialogButtons::Ok)
        .show(move |_| {
            let _ = tx.send(());
        });
    let _ = rx.await;
}
