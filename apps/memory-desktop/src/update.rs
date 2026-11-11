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
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tauri::window::{ProgressBarState, ProgressBarStatus};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};

use memory_server::settings::{UpdateInfo, UpdateProgress};

use crate::i18n::{tr, trf};

/// Cek update otomatis aktif? Default: ya.
pub fn auto_enabled(data_dir: &Path) -> bool {
    crate::i18n::read_prefs(data_dir)["auto_update"]
        .as_bool()
        .unwrap_or(true)
}

pub fn set_auto(data_dir: &Path, on: bool) -> Result<()> {
    crate::i18n::write_pref(data_dir, "auto_update", on.into())
}

const FIRST_CHECK: Duration = Duration::from_secs(30);
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const TITLE: &str = "Update engram";

/// Hanya satu pemeriksaan/pemasangan yang boleh berjalan.
static BUSY: AtomicBool = AtomicBool::new(false);
/// Versi yang ditolak pengguna; cek otomatis tidak menawarkannya lagi.
static DECLINED: Mutex<Option<String>> = Mutex::new(None);
/// Status kemajuan pengunduhan & pemasangan update untuk UI.
static PROGRESS: Mutex<Option<UpdateProgress>> = Mutex::new(None);

pub fn progress() -> UpdateProgress {
    PROGRESS
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default()
}

fn set_progress(app: &AppHandle, p: UpdateProgress) {
    set_tooltip(app, p.message.as_deref().filter(|_| p.running));
    if let Some(w) = app.get_webview_window(crate::MAIN) {
        let bar = match p.stage.as_str() {
            "checking" | "installing" | "restarting" => ProgressBarState {
                status: Some(ProgressBarStatus::Indeterminate),
                progress: None,
            },
            "downloading" => match p.percent {
                Some(pct) => ProgressBarState {
                    status: Some(ProgressBarStatus::Normal),
                    progress: Some(u64::from(pct)),
                },
                None => ProgressBarState {
                    status: Some(ProgressBarStatus::Indeterminate),
                    progress: None,
                },
            },
            _ => ProgressBarState {
                status: Some(ProgressBarStatus::None),
                progress: None,
            },
        };
        let _ = w.set_progress_bar(bar);
        if let Ok(json) = serde_json::to_string(&p) {
            let _ = w.eval(&format!(
                "if (window.__onUpdateProgress) window.__onUpdateProgress({json});"
            ));
        }
    }
    if let Ok(mut g) = PROGRESS.lock() {
        *g = Some(p);
    }
}

fn format_mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

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
            message(
                &app,
                MessageDialogKind::Info,
                tr!("An update check is already running.", "Pemeriksaan update sedang berjalan."),
            )
            .await;
        }
        return;
    }
    let res = check_inner(&app, manual).await;
    BUSY.store(false, Ordering::SeqCst);
    if let Err(e) = res {
        tracing::warn!("update gagal: {e:#}");
        let mut p = progress();
        if p.running {
            p.running = false;
            p.stage = "error".into();
            p.error = Some(format!("{e:#}"));
            p.message = Some(trf!("Failed to install update: {e:#}", "Gagal memasang update: {e:#}"));
            set_progress(&app, p);
        } else {
            set_tooltip(&app, None);
        }
        if manual {
            message(
                &app,
                MessageDialogKind::Error,
                &trf!("Failed to check for or install the update:\n{e:#}", "Gagal memeriksa atau memasang update:\n{e:#}"),
            )
            .await;
        }
    } else if !progress().running {
        set_tooltip(&app, None);
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
        .context(tr!("setting up the updater", "menyiapkan updater"))?;
    match updater.check().await {
        Ok(u) => Ok(u),
        // Rilis baru sedang dibangun di CI: latest.json atau paket platform ini belum diunggah.
        Err(
            tauri_plugin_updater::Error::ReleaseNotFound
            | tauri_plugin_updater::Error::TargetNotFound(_)
            | tauri_plugin_updater::Error::TargetsNotFound(_),
        ) => {
            tracing::debug!("manifest atau paket rilis untuk platform ini belum tersedia");
            Ok(None)
        }
        Err(e) => Err(e).context(tr!("reading release info", "membaca info rilis")),
    }
}

/// Cek update untuk halaman Pengaturan (tanpa dialog).
pub async fn info(app: &AppHandle) -> Result<UpdateInfo> {
    let current = app.package_info().version.to_string();
    let update = find(app).await?;
    Ok(UpdateInfo {
        available: update.is_some(),
        latest: Some(
            update
                .as_ref()
                .map(|u| u.version.clone())
                .unwrap_or_else(|| current.clone()),
        ),
        notes: update
            .as_ref()
            .and_then(|u| u.body.clone())
            .filter(|s| !s.trim().is_empty()),
        date: update
            .as_ref()
            .and_then(|u| u.date)
            .and_then(|d| chrono::DateTime::from_timestamp(d.unix_timestamp(), 0))
            .map(|d| d.to_rfc3339()),
        can_install: update.is_some(),
        url: format!("{}/releases/latest", memory_server::settings::REPO_URL),
        current,
    })
}

/// Pasang update yang diminta dari UI (pengguna sudah setuju di sana).
pub fn start_install(app: &AppHandle) -> Result<(), String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err(tr!("an update check or install is already running", "pemeriksaan atau pemasangan update sedang berjalan").into());
    }
    set_progress(
        app,
        UpdateProgress {
            running: true,
            stage: "checking".into(),
            version: None,
            downloaded_bytes: 0,
            total_bytes: None,
            percent: Some(0),
            message: Some(tr!("Preparing update download...", "Menyiapkan unduhan update...").into()),
            error: None,
        },
    );
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let res = async {
            let update = find(&app).await?.context(tr!("no new update", "tidak ada update baru"))?;
            install(&app, update).await
        }
        .await;
        BUSY.store(false, Ordering::SeqCst);
        if let Err(e) = res {
            tracing::warn!("update gagal: {e:#}");
            let mut p = progress();
            p.running = false;
            p.stage = "error".into();
            p.error = Some(format!("{e:#}"));
            p.message = Some(trf!("Failed to install update: {e:#}", "Gagal memasang update: {e:#}"));
            set_progress(&app, p);
            message(
                &app,
                MessageDialogKind::Error,
                &trf!("Failed to install update:\n{e:#}", "Gagal memasang update:\n{e:#}"),
            )
            .await;
        }
    });
    Ok(())
}

async fn check_inner(app: &AppHandle, manual: bool) -> Result<()> {
    let Some(update) = find(app).await? else {
        tracing::info!("aplikasi sudah versi terbaru");
        if manual {
            let v = app.package_info().version.to_string();
            message(
                app,
                MessageDialogKind::Info,
                &trf!("You have the latest version ({v}).", "Sudah versi terbaru ({v})."),
            )
            .await;
        }
        return Ok(());
    };
    tracing::info!(version = %update.version, "update tersedia");

    if !manual
        && DECLINED
            .lock()
            .map(|d| d.as_deref() == Some(update.version.as_str()))
            .unwrap_or(false)
    {
        return Ok(());
    }
    let mut text = tr!(
        "Version {} is available (current: {}).",
        "Versi {} tersedia (sekarang {}).",
        update.version, update.current_version
    );
    if let Some(notes) = update
        .body
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let notes: String = notes.chars().take(600).collect();
        text.push_str(&format!("\n\n{notes}"));
    }
    text.push_str(tr!("\n\nInstall now? The app will restart.", "\n\nPasang sekarang? Aplikasi akan dibuka ulang."));
    if !ask(app, &text, tr!("Install", "Pasang"), tr!("Later", "Nanti")).await {
        if let Ok(mut d) = DECLINED.lock() {
            *d = Some(update.version.clone());
        }
        return Ok(());
    }
    install(app, update).await
}

/// Unduh, pasang, lalu buka ulang aplikasi.
async fn install(app: &AppHandle, update: Update) -> Result<()> {
    crate::show_main(app);
    let version = update.version.clone();
    set_progress(
        app,
        UpdateProgress {
            running: true,
            stage: "downloading".into(),
            version: Some(version.clone()),
            downloaded_bytes: 0,
            total_bytes: None,
            percent: Some(0),
            message: Some(trf!("Downloading update v{version}...", "Mengunduh update v{version}...")),
            error: None,
        },
    );

    let mut done: u64 = 0;
    let mut known_total: Option<u64> = None;
    let mut last_pct: Option<u8> = None;
    let mut last_emit = Instant::now();
    let v_chunk = version.clone();
    let bytes = update
        .download(
            |chunk, total| {
                done = done.saturating_add(chunk as u64);
                known_total = total.filter(|&t| t > 0);
                let pct = known_total.map(|t| ((done * 100) / t).min(100) as u8);
                let should_emit = pct != last_pct
                    || last_emit.elapsed() >= Duration::from_millis(80)
                    || known_total.is_some_and(|t| done >= t);
                if !should_emit {
                    return;
                }
                last_pct = pct;
                last_emit = Instant::now();
                let label = match (known_total, pct) {
                    (Some(t), Some(p)) => tr!(
                        "Downloading update v{v_chunk} — {} / {} ({p}%)",
                        "Mengunduh update v{v_chunk} — {} / {} ({p}%)",
                        format_mb(done),
                        format_mb(t)
                    ),
                    _ => tr!("Downloading update v{v_chunk} — {}", "Mengunduh update v{v_chunk} — {}", format_mb(done)),
                };
                set_progress(
                    app,
                    UpdateProgress {
                        running: true,
                        stage: "downloading".into(),
                        version: Some(v_chunk.clone()),
                        downloaded_bytes: done,
                        total_bytes: known_total,
                        percent: pct,
                        message: Some(label),
                        error: None,
                    },
                );
            },
            || {},
        )
        .await
        .context(tr!("downloading the update", "mengunduh update"))?;

    set_progress(
        app,
        UpdateProgress {
            running: true,
            stage: "installing".into(),
            version: Some(version.clone()),
            downloaded_bytes: done,
            total_bytes: known_total.or(Some(done)),
            percent: Some(100),
            message: Some(trf!("Installing update v{version}...", "Memasang update v{version}...")),
            error: None,
        },
    );

    // install() bisa memblok (dan di Windows memanggil on_before_exit), jadi jalankan di luar runtime async.
    tauri::async_runtime::spawn_blocking(move || update.install(bytes))
        .await
        .context(tr!("install task", "tugas pemasangan"))?
        .context(tr!("installing the update", "memasang update"))?;

    set_progress(
        app,
        UpdateProgress {
            running: true,
            stage: "restarting".into(),
            version: Some(version),
            downloaded_bytes: done,
            total_bytes: known_total.or(Some(done)),
            percent: Some(100),
            message: Some(tr!("Update installed, restarting the app...", "Update terpasang, membuka ulang aplikasi...").into()),
            error: None,
        },
    );

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
        .buttons(MessageDialogButtons::OkCancelCustom(
            ok.into(),
            cancel.into(),
        ))
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
