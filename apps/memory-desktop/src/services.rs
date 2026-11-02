//! Pengawas layanan latar aplikasi desktop: server (UI, API, MCP), worker,
//! scheduler, dan pemantau inbox. Menyimpan handle tiap layanan agar statusnya
//! bisa ditampilkan (UI web & menu bar) dan layanan yang berhenti bisa dijalankan lagi.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};

use tauri::async_runtime::JoinHandle;
use tokio::sync::watch;

use memory_import::Importer;
use memory_jobs::Runtime;
use memory_server::{AppState, ServiceControl, ServiceInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Server,
    Workers,
    Scheduler,
    Inbox,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Server, Kind::Workers, Kind::Scheduler, Kind::Inbox];

    pub fn id(self) -> &'static str {
        match self {
            Kind::Server => "server",
            Kind::Workers => "workers",
            Kind::Scheduler => "scheduler",
            Kind::Inbox => "inbox",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Server => "Server UI, API & MCP",
            Kind::Workers => "Worker pemrosesan",
            Kind::Scheduler => "Penjadwal",
            Kind::Inbox => "Pemantau inbox",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Kind::Server => "Melayani jendela aplikasi dan agent AI lewat MCP.",
            Kind::Workers => "Memproses antrean job: import, ekstraksi, dan embedding.",
            Kind::Scheduler => "Menjadwalkan pemeliharaan dan mencoba ulang job yang gagal.",
            Kind::Inbox => "Mengimpor berkas baru yang ditaruh di folder inbox.",
        }
    }

    pub fn from_id(id: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.id() == id)
    }
}

pub struct Services {
    rt: Runtime,
    importer: Importer,
    /// Diisi setelah `AppState` memegang pengendali ini (keduanya saling merujuk).
    state: OnceLock<AppState>,
    /// Alamat server; dipakai lagi saat server dijalankan ulang agar URL & config MCP tetap.
    addr: SocketAddr,
    shutdown: watch::Receiver<bool>,
    handles: Mutex<HashMap<Kind, JoinHandle<()>>>,
}

impl Services {
    pub fn new(rt: Runtime, importer: Importer, addr: SocketAddr, shutdown: watch::Receiver<bool>) -> Self {
        Self { rt, importer, state: OnceLock::new(), addr, shutdown, handles: Mutex::new(HashMap::new()) }
    }

    pub fn set_state(&self, state: AppState) {
        let _ = self.state.set(state);
    }

    pub fn url(&self) -> String {
        format!("http://{}/", self.addr)
    }

    pub fn is_running(&self, kind: Kind) -> bool {
        self.handles.lock().map(|h| h.get(&kind).is_some_and(|t| !t.inner().is_finished())).unwrap_or(false)
    }

    /// Jalankan server dengan listener yang sudah dibuka (saat aplikasi mulai).
    pub fn start_server_with(&self, listener: tokio::net::TcpListener) -> Result<(), String> {
        self.spawn(Kind::Server, Some(listener))
    }

    pub fn start_kind(&self, kind: Kind) -> Result<(), String> {
        self.spawn(kind, None)
    }

    /// Ambil semua handle untuk ditunggu saat aplikasi ditutup.
    pub fn take_all(&self) -> Vec<JoinHandle<()>> {
        self.handles.lock().map(|mut h| h.drain().map(|(_, t)| t).collect()).unwrap_or_default()
    }

    fn spawn(&self, kind: Kind, listener: Option<tokio::net::TcpListener>) -> Result<(), String> {
        if *self.shutdown.borrow() {
            return Err("aplikasi sedang ditutup".into());
        }
        let mut handles = self.handles.lock().map_err(|_| "status layanan rusak".to_string())?;
        if handles.get(&kind).is_some_and(|t| !t.inner().is_finished()) {
            return Ok(());
        }
        let stop = self.shutdown.clone();
        let task = match kind {
            Kind::Server => {
                let state = self.state.get().cloned().ok_or("server belum siap")?;
                let listener = match listener {
                    Some(l) => l,
                    None => self.bind()?,
                };
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = memory_server::serve_listener(state, listener, stop).await {
                        tracing::error!(error = %e, "server berhenti");
                    }
                })
            }
            Kind::Workers => {
                let rt = self.rt.clone();
                tauri::async_runtime::spawn(async move { rt.run_workers(stop).await })
            }
            Kind::Scheduler => {
                let rt = self.rt.clone();
                tauri::async_runtime::spawn(async move { rt.run_scheduler(stop).await })
            }
            Kind::Inbox => {
                let imp = self.importer.clone();
                let (subj, poll) = (self.rt.cfg.vault.default_subject.clone(), self.rt.cfg.schedule.inbox_poll_secs);
                tauri::async_runtime::spawn(async move { imp.watch_inbox(subj, poll, stop).await })
            }
        };
        handles.insert(kind, task);
        tracing::info!(service = kind.id(), "layanan dijalankan");
        Ok(())
    }

    /// Buka ulang port server yang sama (SO_REUSEADDR agar tidak terhalang TIME_WAIT).
    fn bind(&self) -> Result<tokio::net::TcpListener, String> {
        let rt = tauri::async_runtime::handle();
        let _guard = rt.inner().enter();
        let sock = if self.addr.is_ipv4() { tokio::net::TcpSocket::new_v4() } else { tokio::net::TcpSocket::new_v6() };
        let open = |s: tokio::net::TcpSocket| -> std::io::Result<tokio::net::TcpListener> {
            s.set_reuseaddr(true)?;
            s.bind(self.addr)?;
            s.listen(1024)
        };
        sock.and_then(open).map_err(|e| format!("tidak bisa membuka {}: {e}", self.addr))
    }
}

impl ServiceControl for Services {
    fn list(&self) -> Vec<ServiceInfo> {
        Kind::ALL
            .into_iter()
            .map(|k| ServiceInfo { id: k.id().into(), name: k.name().into(), description: k.description().into(), running: self.is_running(k) })
            .collect()
    }

    fn start(&self, id: &str) -> Result<(), String> {
        let kind = Kind::from_id(id).ok_or_else(|| format!("layanan tidak dikenal: {id}"))?;
        self.start_kind(kind)
    }
}
