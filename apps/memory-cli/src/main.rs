//! `engram`: CLI platform engram.

mod bench;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde_json::json;

use memory_core::MemoryConfig;
use memory_core::model::{ReviewStatus, SourceKind};
use memory_extract::review::Decision;
use memory_import::Importer;
use memory_jobs::Runtime;
use memory_retrieve::RecallQuery;

#[derive(Parser)]
#[command(name = "engram", version, about = "Orkestrator memori engram (Rust + SurrealDB embedded)")]
struct Cli {
    /// Berkas konfigurasi (default: ./memory.toml).
    #[arg(long, global = true, env = "MEMORY_CONFIG", default_value = "memory.toml")]
    config: PathBuf,
    /// Subjek (default: vault.default_subject).
    #[arg(long, short, global = true)]
    subject: Option<String>,
    /// Keluaran JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Buat memory.toml, folder vault, dan subjek pertama.
    Init {
        #[arg(long, default_value = "vault")]
        vault: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        /// Tanggal lahir YYYY-MM-DD.
        #[arg(long)]
        birth: Option<String>,
    },
    /// Kelola subjek.
    Subject {
        #[command(subcommand)]
        cmd: SubjectCmd,
    },
    /// Import berkas/folder lalu proses.
    Import {
        paths: Vec<PathBuf>,
        /// interview | memoir | note | chat | lifelog | email | document
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, default_value = "import cli")]
        label: String,
        /// Hanya antrekan, jangan proses sekarang.
        #[arg(long)]
        no_process: bool,
    },
    /// Antrean job.
    Jobs {
        #[command(subcommand)]
        cmd: JobsCmd,
    },
    /// Worker + scheduler + pemantau inbox (tanpa HTTP).
    Worker,
    /// UI web + API + MCP HTTP + worker + scheduler + inbox.
    Serve {
        #[arg(long)]
        bind: Option<String>,
        /// Jangan jalankan worker di proses ini.
        #[arg(long)]
        no_worker: bool,
    },
    /// Server MCP lewat stdio (untuk Claude Code, Claude Desktop, dll.).
    Mcp,
    /// Cari memori.
    Recall {
        query: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        as_of: Option<String>,
    },
    /// Tanya (jawaban LLM dari memori, bersitasi).
    Ask {
        question: String,
        #[arg(long)]
        persona: bool,
        #[arg(long)]
        as_of: Option<String>,
    },
    /// Siapa orang ini?
    Who { name: String },
    /// Timeline kronologis.
    Timeline {
        #[arg(long)]
        entity: Option<String>,
        #[arg(long)]
        phase: Option<String>,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    /// Prinsip hidup.
    Principles { situation: Option<String> },
    /// Antrean review manusia.
    Review {
        #[command(subcommand)]
        cmd: ReviewCmd,
    },
    /// Konsolidasi: working memory & fase hidup.
    Consolidate,
    /// Distilasi prinsip & disposisi.
    Distill,
    /// Bangun profil gaya tutur.
    Voice,
    /// Wawancara terpandu di terminal.
    Interview,
    /// Ekspor JSONL + Markdown.
    Export {
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Backup ke .tar.gz (atau .tar.gz.age bila ada penerima).
    Backup {
        out: PathBuf,
        /// Penerima age (age1...). Default: backup.age_recipient di konfigurasi.
        #[arg(long)]
        recipient: Option<String>,
    },
    /// Restore dari backup.
    Restore {
        file: PathBuf,
        #[arg(long)]
        identity: Option<PathBuf>,
    },
    /// Lokasi vault (database & berkas sumber).
    Vault {
        #[command(subcommand)]
        cmd: VaultCmd,
    },
    /// Sinkronisasi antar komputer lewat folder Google Drive.
    Sync {
        #[command(subcommand)]
        cmd: SyncCmd,
    },
    /// Hitung ulang semua embedding (setelah ganti model).
    Reembed,
    /// Statistik subjek.
    Stats,
    /// Cek koneksi LLM & embedding.
    Smoke,
    /// Benchmark penyimpanan & pencarian vektor.
    Bench {
        #[arg(long, default_value_t = 20_000)]
        episodes: usize,
        #[arg(long, default_value_t = 200)]
        queries: usize,
        /// hnsw | diskann
        #[arg(long, default_value = "hnsw")]
        index: String,
        /// Folder vault sementara (default: folder temp sistem).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum SubjectCmd {
    List,
    Add {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        birth: Option<String>,
    },
    /// Tambah peristiwa jangkar: --event nikah=2005-06-01
    Anchor {
        #[arg(long)]
        event: Vec<String>,
        #[arg(long)]
        birth: Option<String>,
    },
}

#[derive(Subcommand)]
enum VaultCmd {
    /// Tampilkan lokasi vault.
    Where,
    /// Pindahkan vault ke folder lain (disalin; vault lama tidak dihapus) dan perbarui memory.toml.
    /// Hentikan dulu `engram serve`/`worker`/aplikasi desktop yang memakai vault ini.
    Move { to: PathBuf },
}

#[derive(Subcommand)]
enum SyncCmd {
    /// Atur folder sync (mis. folder di Google Drive) dan kata sandi enkripsi.
    Setup {
        /// Folder sync; kosongkan dengan --off.
        #[arg(long, required_unless_present = "off")]
        dir: Option<PathBuf>,
        /// Sync otomatis tiap N menit saat worker/serve berjalan (0 = manual).
        #[arg(long)]
        interval: Option<u64>,
        /// Nama komputer ini di daftar perangkat.
        #[arg(long)]
        name: Option<String>,
        /// Kata sandi enkripsi (sama di semua komputer). Atau env ENGRAM_SYNC_PASSPHRASE.
        #[arg(long)]
        passphrase: Option<String>,
        /// Matikan sinkronisasi.
        #[arg(long)]
        off: bool,
    },
    /// Sync sekarang: tarik data komputer lain lalu unggah data komputer ini.
    Run,
    /// Status & daftar perangkat.
    Status,
}

#[derive(Subcommand)]
enum JobsCmd {
    /// Jalankan semua job yang siap sampai antrean kosong.
    Run,
    List {
        #[arg(long)]
        status: Option<String>,
    },
    /// Proses ulang sumber yang gagal (semua, atau satu dengan --source).
    Retry {
        #[arg(long)]
        source: Option<String>,
        /// Langsung jalankan antreannya (tanpa worker terpisah).
        #[arg(long)]
        run: bool,
    },
}

#[derive(Subcommand)]
enum ReviewCmd {
    List,
    Approve {
        id: String,
        #[arg(long)]
        note: Option<String>,
    },
    Reject {
        id: String,
        #[arg(long)]
        note: Option<String>,
    },
}

fn load_config(path: &Path) -> Result<MemoryConfig> {
    if !path.exists() {
        return Ok(MemoryConfig::default());
    }
    let s = std::fs::read_to_string(path).with_context(|| format!("membaca {}", path.display()))?;
    let mut cfg = MemoryConfig::from_toml(&s).with_context(|| format!("format {}", path.display()))?;
    if cfg.vault.path.is_relative()
        && let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            cfg.vault.path = dir.join(&cfg.vault.path);
        }
    Ok(cfg)
}

fn date(s: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").with_context(|| format!("tanggal tidak valid (YYYY-MM-DD): {s}"))
}

fn print<T: serde::Serialize>(as_json: bool, v: &T, human: impl FnOnce() -> String) {
    if as_json {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
    } else {
        println!("{}", human());
    }
}

fn importer(rt: &Runtime) -> Importer {
    let t = memory_llm::WhisperTranscriber::new(&rt.cfg.transcription).map(|w| Arc::new(w) as Arc<dyn memory_llm::Transcriber>);
    Importer::new(rt.pipeline.clone(), t)
}

fn shutdown_signal() -> tokio::sync::watch::Receiver<bool> {
    let (tx, rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("berhenti...");
        let _ = tx.send(true);
    });
    rx
}

async fn background(rt: &Runtime, stop: &tokio::sync::watch::Receiver<bool>, inbox: bool) -> Vec<tokio::task::JoinHandle<()>> {
    let mut hs = Vec::new();
    let (r, s) = (rt.clone(), stop.clone());
    hs.push(tokio::spawn(async move { r.run_workers(s).await }));
    let (r, s) = (rt.clone(), stop.clone());
    hs.push(tokio::spawn(async move { r.run_scheduler(s).await }));
    if inbox {
        let imp = importer(rt);
        let (subj, poll, s) = (rt.cfg.vault.default_subject.clone(), rt.cfg.schedule.inbox_poll_secs, stop.clone());
        hs.push(tokio::spawn(async move { imp.watch_inbox(subj, poll, s).await }));
    }
    hs
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,surrealdb=warn,surrealdb_core=warn,surrealdb_core::kvs::tasks=off".into()))
        .init();
    let cli = Cli::parse();

    if let Cmd::Init { vault, id, name, birth } = &cli.cmd {
        if !cli.config.exists() {
            let mut cfg = MemoryConfig::default();
            cfg.vault.path = vault.clone();
            cfg.vault.default_subject = id.clone();
            std::fs::write(&cli.config, cfg.to_toml())?;
            eprintln!("Konfigurasi dibuat: {}", cli.config.display());
        }
        let rt = Runtime::open(load_config(&cli.config)?).await?;
        let mut s = rt.pipeline.ensure_subject(id, Some(name)).await?;
        if let Some(b) = birth {
            s.anchors.birth = Some(date(b)?);
            rt.store.put_subject(&s).await?;
        }
        println!("Vault siap di {} dengan subjek '{}' ({}).", rt.vault.root().display(), s.name, s.id);
        return Ok(());
    }
    if let Cmd::Vault { cmd } = &cli.cmd {
        let cfg = load_config(&cli.config)?;
        return match cmd {
            VaultCmd::Where => {
                println!("{}", cfg.vault.path.display());
                Ok(())
            }
            VaultCmd::Move { to } => move_vault(&cli.config, &cfg, to),
        };
    }
    if let Cmd::Bench { episodes, queries, index, dir } = &cli.cmd {
        let cfg = load_config(&cli.config)?;
        return bench::run(&cfg, *episodes, *queries, index, dir.clone()).await;
    }

    let cfg = load_config(&cli.config)?;
    let subject = cli.subject.clone().unwrap_or_else(|| cfg.vault.default_subject.clone());
    let rt = Runtime::open(cfg).await?;
    let j = cli.json;

    match cli.cmd {
        Cmd::Init { .. } | Cmd::Bench { .. } | Cmd::Vault { .. } => unreachable!(),
        Cmd::Subject { cmd } => match cmd {
            SubjectCmd::List => {
                let subs = rt.store.list_subjects().await?;
                print(j, &subs, || subs.iter().map(|s| format!("{}\t{}\t{}", s.id, s.name, s.anchors.birth.map(|b| b.to_string()).unwrap_or_default())).collect::<Vec<_>>().join("\n"));
            }
            SubjectCmd::Add { id, name, birth } => {
                let mut s = rt.pipeline.ensure_subject(&id, Some(&name)).await?;
                if let Some(b) = birth {
                    s.anchors.birth = Some(date(&b)?);
                }
                rt.store.put_subject(&s).await?;
                println!("Subjek {id} tersimpan.");
            }
            SubjectCmd::Anchor { event, birth } => {
                let mut s = rt.pipeline.ensure_subject(&subject, None).await?;
                if let Some(b) = birth {
                    s.anchors.birth = Some(date(&b)?);
                }
                for e in event {
                    let (k, v) = e.split_once('=').context("format --event nama=YYYY-MM-DD")?;
                    s.anchors.events.insert(k.trim().to_string(), date(v.trim())?);
                }
                rt.store.put_subject(&s).await?;
                print(j, &s.anchors, || format!("Jangkar {}: lahir {:?}, peristiwa {:?}", s.id, s.anchors.birth, s.anchors.events));
            }
        },
        Cmd::Import { paths, kind, label, no_process } => {
            if paths.is_empty() {
                bail!("berikan minimal satu berkas atau folder");
            }
            let hint = match kind.as_deref() {
                Some(k) => Some(SourceKind::parse(k).with_context(|| format!("jenis tidak dikenal: {k}"))?),
                None => None,
            };
            let sum = importer(&rt).import_paths(&subject, &paths, &label, hint).await?;
            eprintln!("{} berkas -> {} dokumen: {} diantrekan, {} duplikat, {} gagal", sum.files, sum.documents, sum.queued, sum.duplicates, sum.failed.len());
            for f in &sum.failed {
                eprintln!("  gagal: {f}");
            }
            if !no_process {
                let n = rt.drain().await?;
                let b = rt.store.get_batch(&sum.batch_id).await?;
                eprintln!("{n} job dijalankan.");
                print(j, &b, || b.as_ref().map(|b| format!("Batch {}: {}/{} selesai, {} gagal", b.id, b.done, b.total, b.failed)).unwrap_or_default());
            }
        }
        Cmd::Jobs { cmd } => match cmd {
            JobsCmd::Run => println!("{} job dijalankan.", rt.drain().await?),
            JobsCmd::Retry { source, run } => {
                match source {
                    Some(id) => match rt.retry_source(&id).await? {
                        Some(job) => println!("Sumber {id} diantre ulang (job {job})."),
                        None => println!("Sumber {id} tidak berstatus gagal; tidak ada yang diantre."),
                    },
                    None => {
                        let r = rt.retry_failed(false).await?;
                        print(j, &r, || match &r.backend_error {
                            Some(e) => format!("{} sumber gagal, belum diantre: server belum bisa dihubungi ({e}).", r.failed),
                            None => format!("{} sumber gagal, {} diantre ulang.", r.failed, r.requeued),
                        });
                    }
                }
                if run {
                    println!("{} job dijalankan.", rt.drain().await?);
                }
            }
            JobsCmd::List { status } => {
                let st = status.as_deref().and_then(|s| serde_json::from_value(json!(s)).ok());
                let jobs = rt.store.list_jobs(st, 100).await?;
                print(j, &jobs, || {
                    jobs.iter()
                        .map(|x| format!("{}\t{}\t{:?}\t{}/{} {}\t{}", x.id, x.kind, x.status, x.progress_done, x.progress_total, x.stage.clone().unwrap_or_default(), x.error.clone().unwrap_or_default()))
                        .collect::<Vec<_>>()
                        .join("\n")
                });
            }
        },
        Cmd::Worker => {
            let stop = shutdown_signal();
            for h in background(&rt, &stop, true).await {
                let _ = h.await;
            }
        }
        Cmd::Serve { bind, no_worker } => {
            let stop = shutdown_signal();
            let hs = if no_worker { vec![] } else { background(&rt, &stop, true).await };
            let mut st = memory_server::AppState::new(rt.clone(), importer(&rt));
            let addr = bind.clone().unwrap_or_else(|| rt.cfg.server.bind.clone());
            // Pemasangan MCP ke agent AI hanya untuk server di komputer ini sendiri.
            if addr.starts_with("127.") || addr.starts_with("localhost") {
                st.mcp_url = Some(format!("http://{addr}/mcp"));
            }
            if st.token.is_none() && bind.as_deref().is_some_and(|b| !b.starts_with("127.") && !b.starts_with("localhost")) {
                tracing::warn!("server terbuka ke jaringan tanpa token; set env MEMORY_TOKEN");
            }
            memory_server::serve(st, &addr, stop).await?;
            for h in hs {
                let _ = h.await;
            }
        }
        Cmd::Mcp => {
            let stop = shutdown_signal();
            let _hs = background(&rt, &stop, false).await;
            memory_server::mcp::serve_stdio(memory_server::AppState::new(rt.clone(), importer(&rt))).await?;
        }
        Cmd::Recall { query, from, to, as_of } => {
            let r = rt.retriever.recall(&RecallQuery { from, to, as_of, ..RecallQuery::new(&subject, &query) }).await?;
            print(j, &r, || r.context.clone());
        }
        Cmd::Ask { question, persona, as_of } => {
            let a = rt.retriever.ask(&subject, &question, persona, as_of.as_deref()).await?;
            print(j, &a, || a.answer.clone());
        }
        Cmd::Who { name } => {
            let w = rt.retriever.who_is(&subject, &name).await?;
            print(j, &w, || match &w {
                None => format!("Tidak ada catatan tentang '{name}'."),
                Some(w) => {
                    let mut s = format!("{} ({:?}) {}", w.entity.name, w.entity.kind, w.relation_to_subject.clone().unwrap_or_default());
                    for f in &w.current_facts {
                        s.push_str(&format!("\n  [sekarang] {}", f.statement));
                    }
                    for f in &w.history {
                        s.push_str(&format!("\n  [dulu {}-{}] {}", f.valid_from.as_deref().unwrap_or("?"), f.valid_to.as_deref().unwrap_or("?"), f.statement));
                    }
                    for k in &w.kin {
                        s.push_str(&format!("\n  {}: {}", k.relation, k.name));
                    }
                    s
                }
            });
        }
        Cmd::Timeline { entity, phase, limit } => {
            let t = rt.retriever.timeline(&subject, entity.as_deref(), phase.as_deref(), None, None, limit).await?;
            print(j, &t, || t.items.iter().map(|i| format!("{}\t{}\t{}", i.date.as_deref().unwrap_or("?"), i.time, i.label)).collect::<Vec<_>>().join("\n"));
        }
        Cmd::Principles { situation } => {
            let ps = rt.retriever.principles(&subject, situation.as_deref()).await?;
            print(j, &ps, || ps.iter().map(|p| format!("- {} ({:.0}%, {} bukti)", p.principle.statement, p.principle.confidence * 100.0, p.evidence.len())).collect::<Vec<_>>().join("\n"));
        }
        Cmd::Review { cmd } => match cmd {
            ReviewCmd::List => {
                let items = rt.store.list_reviews(&subject, ReviewStatus::Open, None, 200).await?;
                print(j, &items, || items.iter().map(|r| format!("{}\t{:?}\t{}", r.id, r.kind, r.summary)).collect::<Vec<_>>().join("\n"));
            }
            ReviewCmd::Approve { id, note } => {
                rt.pipeline.resolve_review(&id, Decision::Approve, note).await?;
                println!("Disetujui.");
            }
            ReviewCmd::Reject { id, note } => {
                rt.pipeline.resolve_review(&id, Decision::Reject, note).await?;
                println!("Ditolak.");
            }
        },
        Cmd::Consolidate => {
            let r = rt.pipeline.consolidate(&subject).await?;
            print(j, &r, || format!("kedaluwarsa {}, dipromosikan {}, fase diperbarui {}", r.expired, r.promoted, r.phases_updated));
        }
        Cmd::Distill => {
            let r = rt.pipeline.distill(&subject).await?;
            print(j, &r, || format!("{r:?}"));
        }
        Cmd::Voice => {
            let v = rt.pipeline.build_voice_profile(&subject).await?;
            print(j, &v, || format!("{}\nIdiom: {:?}", v.style, v.idioms));
        }
        Cmd::Interview => {
            use std::io::BufRead;
            eprintln!("Wawancara terpandu. Ketik jawaban, akhiri dengan baris kosong. Ketik 'selesai' untuk keluar.\n");
            loop {
                let q = rt.pipeline.next_question(&subject).await?;
                println!("\n[{}] {}\n", q.reason, q.question);
                let mut answer = String::new();
                for line in std::io::stdin().lock().lines() {
                    let line = line?;
                    if line.trim().is_empty() {
                        break;
                    }
                    answer.push_str(&line);
                    answer.push('\n');
                }
                if answer.trim().eq_ignore_ascii_case("selesai") || answer.trim().is_empty() {
                    break;
                }
                rt.pipeline.answer_question(&subject, &q.question, &answer).await?;
                let n = rt.drain().await?;
                eprintln!("(tersimpan, {n} job diproses)");
            }
        }
        Cmd::Export { dir } => println!("{}", memory_jobs::archive::export(&rt, dir).await?.display()),
        Cmd::Backup { out, recipient } => {
            let r = recipient.or_else(|| rt.cfg.backup.age_recipient.clone());
            let p = memory_jobs::archive::backup(&rt, &out, r.as_deref()).await?;
            println!("Backup: {}", p.display());
        }
        Cmd::Restore { file, identity } => {
            let r = memory_jobs::archive::restore(&rt, &file, identity.as_deref()).await?;
            print(j, &r, || format!("{} record, {} berkas sumber, {} embedding dihitung ulang", r.rows, r.source_files, r.reembedded));
        }
        Cmd::Sync { cmd } => {
            use memory_jobs::sync;
            match cmd {
                SyncCmd::Setup { dir, interval, name, passphrase, off } => {
                    let mut s = sync::load_settings(&rt.vault);
                    s.dir = if off { None } else { dir.map(|d| std::path::absolute(&d)).transpose()? };
                    if let Some(i) = interval {
                        s.interval_minutes = i;
                    }
                    if name.is_some() {
                        s.device_name = name;
                    }
                    if let Some(p) = passphrase {
                        sync::set_passphrase(&rt.vault, Some(&p))?;
                    }
                    let s = sync::save_settings(&rt.vault, s)?;
                    match &s.dir {
                        Some(d) => println!("Folder sync: {}\nSync otomatis: {}", d.display(), if s.interval_minutes == 0 { "mati (manual)".to_string() } else { format!("tiap {} menit saat worker/serve berjalan", s.interval_minutes) }),
                        None => println!("Sinkronisasi dimatikan."),
                    }
                    if s.dir.is_some() && !sync::status(&rt).encrypted {
                        eprintln!("Peringatan: data di Drive tidak dienkripsi. Tambahkan --passphrase (sama di semua komputer).");
                    }
                }
                SyncCmd::Run => {
                    let r = sync::sync_now(&rt).await?;
                    print(j, &r, || {
                        let mut s = format!(
                            "Ditarik dari: {}\n{} record baru, {} diperbarui, {} versi lokal dipertahankan\nBerkas sumber: {} masuk, {} keluar\nDiunggah: {}",
                            if r.pulled_from.is_empty() { "-".to_string() } else { r.pulled_from.join(", ") },
                            r.added,
                            r.updated,
                            r.kept_local,
                            r.source_files_in,
                            r.source_files_out,
                            if r.pushed { "ya" } else { "tidak ada perubahan" }
                        );
                        for w in &r.warnings {
                            s.push_str(&format!("\nPeringatan: {w}"));
                        }
                        s
                    });
                }
                SyncCmd::Status => {
                    let st = sync::status(&rt);
                    print(j, &st, || {
                        let mut s = match &st.settings.dir {
                            Some(d) => format!("Folder sync: {}{}", d.display(), if st.dir_ok { "" } else { " (TIDAK bisa diakses)" }),
                            None => "Sinkronisasi belum diatur. Jalankan `engram sync setup --dir <folder Google Drive>`.".into(),
                        };
                        s.push_str(&format!("\nKomputer ini: {} | enkripsi: {} | sync terakhir: {}", st.device_name, if st.encrypted { "ya" } else { "tidak" }, st.last_sync_at.as_deref().unwrap_or("-")));
                        if let Some(e) = &st.last_error {
                            s.push_str(&format!("\nGalat terakhir: {e}"));
                        }
                        for d in &st.devices {
                            s.push_str(&format!("\n  {}{}\t{} record\t{}", d.name, if d.this_device { " (ini)" } else { "" }, d.rows, d.updated_at));
                        }
                        for p in st.suggestions.iter().filter(|_| st.settings.dir.is_none()) {
                            s.push_str(&format!("\nSaran folder: {}", p.display()));
                        }
                        s
                    });
                }
            }
        }
        Cmd::Reembed => println!("{} embedding dihitung ulang.", memory_jobs::archive::reembed(&rt).await?),
        Cmd::Stats => {
            let s = rt.store.stats(&subject).await?;
            print(j, &s, || format!("{s:#?}"));
        }
        Cmd::Smoke => smoke(&rt).await?,
    }
    Ok(())
}

/// Salin vault ke lokasi baru lalu arahkan memory.toml ke sana.
fn move_vault(config: &Path, cfg: &MemoryConfig, to: &Path) -> Result<()> {
    let to = std::path::absolute(to)?;
    let plan = memory_jobs::storage::plan_move(&cfg.vault.path, &to)?;
    let files = memory_jobs::storage::copy_vault(&cfg.vault.path, &plan)?;
    // Edit sebagai tabel TOML agar kunci lain tetap (komentar tidak ikut terbawa).
    let mut table: toml::Table = match std::fs::read_to_string(config) {
        Ok(s) => s.parse().with_context(|| format!("format {}", config.display()))?,
        Err(_) => toml::Table::new(),
    };
    let vault = table.entry("vault").or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let toml::Value::Table(vault) = vault else { bail!("[vault] di {} bukan tabel", config.display()) };
    vault.insert("path".into(), toml::Value::String(plan.target.to_string_lossy().into_owned()));
    std::fs::write(config, toml::to_string_pretty(&table)?)?;
    if plan.use_existing {
        println!("memory.toml sekarang memakai vault yang sudah ada di {}", plan.target.display());
    } else {
        println!("{files} berkas disalin ke {}.\nVault lama di {} tidak dihapus; hapus bila semuanya sudah berjalan baik.", plan.target.display(), cfg.vault.path.display());
    }
    Ok(())
}

async fn smoke(rt: &Runtime) -> Result<()> {
    use memory_llm::{ChatRequest, Message, ModelRole};
    let llm = memory_llm::llm_from_config(&rt.cfg);
    println!("LLM: {}", llm.describe());
    for role in [ModelRole::Light, ModelRole::Extract] {
        let t = std::time::Instant::now();
        let r = llm
            .chat(ChatRequest { role, messages: vec![Message::user("Jawab satu kata: ibu kota Jawa Tengah?")], json_schema: None, temperature: Some(0.0), max_tokens: Some(20), task: "smoke".into() })
            .await;
        match r {
            Ok(s) => println!("  {role:?}: OK {} ms -> {}", t.elapsed().as_millis(), s.trim().chars().take(60).collect::<String>()),
            Err(e) => println!("  {role:?}: GAGAL -> {e}"),
        }
    }
    let t = std::time::Instant::now();
    match rt.pipeline.embedder.embed_one("Pakde Karno membuka toko batik di Solo").await {
        Ok(v) => println!("Embedding: OK {} ms, dimensi {}", t.elapsed().as_millis(), v.len()),
        Err(e) => println!("Embedding: GAGAL -> {e}"),
    }
    Ok(())
}
