//! Pembaruan otomatis lewat `tauri-plugin-updater`.
//!
//! Aplikasi membaca `latest.json` dari GitHub Release terbaru (lihat
//! `plugins.updater` di `tauri.conf.json`). Paket update ditandatangani dengan
//! kunci updater; tanda tangan dicek sebelum dipasang.
//!
//! - Cek otomatis tidak lama setelah aplikasi dibuka, lalu berkala.
//! - Cek manual dari menu tray "Periksa update".
//! - Update baru dipasang setelah pengguna setuju. Mesin memori dihentikan dulu
//!   agar database tertutup bersih, lalu aplikasi dibuka ulang.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK: Duration = Duration::from_secs(30);
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const TITLE: &str = "Update mnemonic-memory";

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
        check(app.clone(), false).await;
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

async fn check_inner(app: &AppHandle, manual: bool) -> Result<()> {
    let app_for_exit = app.clone();
    let updater = app
        .updater_builder()
        // Windows: installer menutup aplikasi lewat process::exit, jadi hentikan mesin dulu.
        .on_before_exit(move || crate::stop_engine(&app_for_exit))
        .build()
        .context("menyiapkan updater")?;
    let Some(update) = updater.check().await.context("membaca info rilis")? else {
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
            Some(s) => format!("mnemonic-memory - {s}"),
            None => "mnemonic-memory".to_string(),
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
