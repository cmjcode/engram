//! `engram`: CLI platform engram.

mod bench;
mod hook;

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
    /// Impor auto-memory Claude Code (~/.claude/projects/*/memory/*.md) langsung sebagai
    /// episode tanpa LLM: isinya sudah terdistilasi. Dry-run bawaan; pakai --apply untuk menyimpan.
    #[command(name = "import-claude-memory")]
    ImportClaudeMemory {
        /// Semua proyek di ~/.claude/projects (bawaan: folder memory milik repo saat ini).
        #[arg(long)]
        all: bool,
        /// Folder memory tertentu.
        #[arg(long)]
        source: Option<PathBuf>,
        /// Nama proyek untuk episode yang diimpor (bawaan: ditebak dari nama folder/repo).
        #[arg(long)]
        project: Option<String>,
        /// Simpan sungguhan (tanpa ini hanya menampilkan apa yang akan diimpor).
        #[arg(long)]
        apply: bool,
    },
    /// Bundel memori per-repositori `.engram/` (pelajaran, keputusan, fakta) untuk di-commit & dibagikan.
    Repo {
        #[command(subcommand)]
        cmd: RepoCmd,
    },
    /// Hook agent AI (dipanggil Claude Code/Gemini CLI/Antigravity, payload JSON di stdin).
    /// Tidak membuka vault; mengirim event ke server engram dan selalu keluar 0.
    Hook {
        /// claude-code | gemini-cli | antigravity | cursor
        client: String,
        /// context | session-init | observation | summarize | pre-compact | session-end | file-context | turn
        event: String,
    },
    /// Server MCP lewat stdio (untuk Claude Code, Codex, Cursor, dll.).
    Mcp {
        /// Subset tool yang diekspos: full (semua) | coding (hanya tool sesi koding).
        #[arg(long, default_value = "full")]
        profile: String,
    },
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
    /// Rangkum & padatkan memori (episode roll-up & dedup fakta) secara non-destruktif.
    Compact {
        /// Hanya tampilkan kandidat klaster episode & fakta tanpa mengubah data.
        #[arg(long)]
        candidates: bool,
    },
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
    /// Kelola & pantau folder penting (konteks diekstrak menggunakan agy CLI).
    Watch {
        #[command(subcommand)]
        cmd: WatchCmd,
    },
    /// Hitung ulang semua embedding (setelah ganti model).
    Reembed,
    /// Statistik subjek.
    Stats,
    /// Cek koneksi LLM & embedding.
    Smoke,
    /// Benchmark penyimpanan & pencarian vektor; `eval`/`tune`/`export-gold` untuk kualitas.
    #[command(args_conflicts_with_subcommands = true)]
    Bench {
        #[command(subcommand)]
        cmd: Option<BenchCmd>,
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
enum BenchCmd {
    /// Lapis A: apakah potongan penting lolos & noise terbuang (gold set di folder kasus).
    Eval {
        /// Folder satu kasus (berisi gold.toml) atau folder berisi banyak kasus.
        path: PathBuf,
        /// Simpan laporan JSON ke berkas ini.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Abaikan cache segmentasi dan panggil LLM lagi.
        #[arg(long)]
        refresh: bool,
    },
    /// Cari ambang skor terbaik secara offline dari cache segmentasi.
    Tune {
        path: PathBuf,
        /// Mis. "promote=0.40..0.70:0.05,gray=0.30..0.55:0.05,noise=0.4|0.6,min_chars=40".
        #[arg(long)]
        grid: Option<String>,
        /// Batas maksimum noise yang boleh lolos jadi episode (0..1).
        #[arg(long, default_value_t = 0.10)]
        max_leak: f32,
        #[arg(long, default_value_t = 10)]
        top: usize,
    },
    /// Ekspor keputusan review GrayZone menjadi gold set `[[segment]]`.
    ExportGold {
        /// Folder tujuan (dibuat bila belum ada); berisi teks chat asli.
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 1000)]
        limit: usize,
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
enum RepoCmd {
    /// Tulis <repo>/.engram/ dari memori proyek ini.
    Export {
        /// Folder repositori (bawaan: folder saat ini).
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Nama proyek (bawaan: dari akar git / nama folder).
        #[arg(long)]
        project: Option<String>,
    },
    /// Gabungkan <repo>/.engram/ ke vault ini (tidak menghapus apa pun).
    Import {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        project: Option<String>,
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
enum WatchCmd {
    /// Tampilkan daftar folder penting yang dipantau.
    List,
    /// Tambahkan folder penting baru lalu baca konteksnya menggunakan agy CLI.
    Add {
        path: PathBuf,
        /// Nama/label untuk folder ini (default: nama folder).
        #[arg(long)]
        label: Option<String>,
        /// Hanya daftarkan folder tanpa langsung memindai sekarang.
        #[arg(long)]
        no_scan: bool,
        /// Hanya antrekan hasil bacaan ke job tanpa langsung menjalankan worker.
        #[arg(long)]
        no_process: bool,
    },
    /// Hapus folder penting dari daftar pantauan (berdasarkan ID atau path).
    Remove { target: String },
    /// Pindai perubahan di folder penting (satu atau semua) dan perbarui memori lewat agy CLI.
    Scan {
        /// ID atau path folder (kosongkan untuk memindai semua folder penting).
        target: Option<String>,
        /// Paksa baca ulang lewat agy CLI walaupun berkas belum berubah.
        #[arg(long)]
        force: bool,
        /// Hanya antrekan hasil bacaan tanpa langsung menjalankan worker.
        #[arg(long)]
        no_process: bool,
    },
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
    KeepBoth {
        id: String,
        #[arg(long)]
        note: Option<String>,
    },
    Reject {
        id: String,
        #[arg(long)]
        note: Option<String>,
    },
    /// Tinjau dan putuskan semua review terbuka secara otomatis menggunakan `agy` CLI.
    Auto,
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

/// Frontmatter YAML sederhana (`key: value`) + isi.
fn split_frontmatter(raw: &str) -> (std::collections::BTreeMap<String, String>, String) {
    let mut map = std::collections::BTreeMap::new();
    let t = raw.trim_start();
    if let Some(rest) = t.strip_prefix("---")
        && let Some(end) = rest.find("\n---")
    {
        for line in rest[..end].lines() {
            if let Some((k, v)) = line.split_once(':') {
                let k = k.trim();
                if !k.is_empty() && !line.starts_with(' ') {
                    map.insert(k.to_string(), v.trim().trim_matches('"').to_string());
                }
            }
        }
        return (map, rest[end + 4..].trim_start_matches('-').to_string());
    }
    (map, raw.to_string())
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
        let (r, s) = (rt.clone(), stop.clone());
        hs.push(tokio::spawn(async move { r.watch_folders(s).await }));
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
    if let Cmd::Hook { client, event } = &cli.cmd {
        if let Err(e) = hook::run(&cli.config, client, event).await {
            eprintln!("engram hook: {e}");
            println!("{{}}");
        }
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
    if let Cmd::Bench { cmd, episodes, queries, index, dir } = &cli.cmd {
        let cfg = load_config(&cli.config)?;
        return match cmd {
            None => bench::speed::run(&cfg, *episodes, *queries, index, dir.clone()).await,
            Some(BenchCmd::Eval { path, out, refresh }) => bench::eval::run(&cfg, &bench::gold::discover(path)?, *refresh, out.as_deref(), cli.json).await,
            Some(BenchCmd::Tune { path, grid, max_leak, top }) => {
                bench::tune::run(&cfg, &bench::gold::discover(path)?, grid.as_deref(), *max_leak, *top, cli.json).await
            }
            Some(BenchCmd::ExportGold { out, limit }) => {
                let subject = cli.subject.clone().unwrap_or_else(|| cfg.vault.default_subject.clone());
                bench::export::run(cfg, &subject, out, *limit).await
            }
        };
    }

    let cfg = load_config(&cli.config)?;
    let subject = cli.subject.clone().unwrap_or_else(|| cfg.vault.default_subject.clone());
    let rt = Runtime::open(cfg).await?;
    let j = cli.json;

    match cli.cmd {
        Cmd::Init { .. } | Cmd::Bench { .. } | Cmd::Vault { .. } | Cmd::Hook { .. } => unreachable!(),
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
            st.config_path = std::path::absolute(&cli.config).ok();
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
        Cmd::Mcp { profile } => {
            let profile = memory_server::mcp::Profile::parse(&profile).ok_or_else(|| anyhow::anyhow!("profile tidak dikenal: {profile} (pakai 'full' atau 'coding')"))?;
            let stop = shutdown_signal();
            let _hs = background(&rt, &stop, false).await;
            memory_server::mcp::serve_stdio(memory_server::AppState::new(rt.clone(), importer(&rt)), profile).await?;
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
            ReviewCmd::KeepBoth { id, note } => {
                rt.pipeline.resolve_review(&id, Decision::KeepBoth, note).await?;
                println!("Keduanya berlaku.");
            }
            ReviewCmd::Reject { id, note } => {
                rt.pipeline.resolve_review(&id, Decision::Reject, note).await?;
                println!("Ditolak.");
            }
            ReviewCmd::Auto => {
                let rep = memory_jobs::watch::auto_review(&rt, &subject).await?;
                print(j, &rep, || {
                    format!(
                        "{}/{} review diselesaikan via agy ({} setuju, {} keduanya berlaku, {} ditolak)",
                        rep.resolved, rep.total_open, rep.approved, rep.kept_both, rep.rejected
                    )
                });
            }
        },
        Cmd::Consolidate => {
            let r = rt.pipeline.consolidate(&subject).await?;
            print(j, &r, || format!("kedaluwarsa {}, dipromosikan {}, fase diperbarui {}", r.expired, r.promoted, r.phases_updated));
        }
        Cmd::Compact { candidates } => {
            if candidates {
                let c = rt.pipeline.compact_candidates(&subject).await?;
                print(j, &c, || {
                    format!(
                        "{} klaster episode, {} grup fakta entitas, {} review terbuka",
                        c.episode_clusters.len(),
                        c.entity_fact_groups.len(),
                        c.open_reviews.len()
                    )
                });
            } else {
                let r = rt.pipeline.compact(&subject).await?;
                print(j, &r, || {
                    format!(
                        "klaster {}, episode kanonik {}, episode dipadatkan {}, fakta digabung {}, fakta ditutup {}, fakta ditolak {}, review {}",
                        r.episode_clusters,
                        r.canonical_episodes_created,
                        r.episodes_compacted,
                        r.facts_merged,
                        r.facts_closed,
                        r.facts_rejected,
                        r.reviews_resolved
                    )
                });
            }
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
        Cmd::Repo { cmd } => match cmd {
            RepoCmd::Export { path, project } => {
                let r = memory_jobs::repo_bundle::export_repo(&rt, &subject, &std::path::absolute(&path)?, project.as_deref()).await?;
                print(j, &r, || format!("Bundel {} ditulis ke {}: {} pelajaran, {} keputusan, {} fakta.", r.project, r.dir.display(), r.lessons, r.decisions, r.facts));
            }
            RepoCmd::Import { path, project } => {
                let r = memory_jobs::repo_bundle::import_repo(&rt, &subject, &std::path::absolute(&path)?, project.as_deref()).await?;
                print(j, &r, || format!("Impor {}: pelajaran {} baru / {} digabung; keputusan {} baru / {} duplikat; {} fakta hanya dibaca.", r.project, r.lessons_new, r.lessons_merged, r.decisions_new, r.decisions_duplicate, r.facts));
            }
        },
        Cmd::ImportClaudeMemory { all, source, project, apply } => {
            let home = std::env::var_os("HOME").map(PathBuf::from).context("HOME tidak diketahui")?;
            let projects_dir = home.join(".claude").join("projects");
            let mut dirs: Vec<(PathBuf, Option<String>)> = Vec::new();
            if let Some(src) = source {
                dirs.push((src, project.clone()));
            } else if all {
                if let Ok(rd) = std::fs::read_dir(&projects_dir) {
                    for e in rd.flatten() {
                        let m = e.path().join("memory");
                        if m.is_dir() {
                            dirs.push((m, project.clone()));
                        }
                    }
                }
            } else {
                let cwd = std::env::current_dir()?;
                let root = memory_core::project::detect_project(&cwd).map(|p| p.root).unwrap_or(cwd);
                let encoded: String = root.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
                let proj = project.clone().or_else(|| memory_core::project::detect_project(&root).map(|p| p.name));
                dirs.push((projects_dir.join(encoded).join("memory"), proj));
            }
            let known: Vec<String> = rt.store.list_entities(&subject, Some(memory_core::model::EntityKind::Project), 2000, 0).await?.into_iter().map(|e| e.name).collect();
            let guess = |dir: &std::path::Path| -> Option<String> {
                let enc = dir.parent()?.file_name()?.to_string_lossy().to_lowercase();
                known.iter().filter(|n| { let k: String = n.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect(); enc.ends_with(&format!("-{k}")) || enc == k }).max_by_key(|n| n.len()).cloned()
            };
            let (mut found, mut saved, mut dup) = (0u32, 0u32, 0u32);
            for (dir, proj) in dirs {
                let proj = proj.or_else(|| guess(&dir));
                let Ok(rd) = std::fs::read_dir(&dir) else {
                    eprintln!("lewati (tidak ada): {}", dir.display());
                    continue;
                };
                let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "md") && p.file_name().is_some_and(|n| n != "MEMORY.md")).collect();
                files.sort();
                for f in files {
                    let raw = std::fs::read_to_string(&f)?;
                    let (fm, body) = split_frontmatter(&raw);
                    let title = fm.get("name").cloned().filter(|t| !t.is_empty()).unwrap_or_else(|| f.file_stem().unwrap_or_default().to_string_lossy().to_string());
                    if body.trim().is_empty() {
                        continue;
                    }
                    found += 1;
                    if !apply {
                        println!("{}	{}	{}", proj.as_deref().unwrap_or("-"), title, f.display());
                        continue;
                    }
                    let out = rt
                        .pipeline
                        .ingest_distilled(memory_extract::ingest::DistilledDoc {
                            subject: subject.clone(),
                            title,
                            summary: fm.get("description").cloned().unwrap_or_default(),
                            text: body.trim().to_string(),
                            project: proj.clone(),
                            original_name: Some(f.file_name().unwrap_or_default().to_string_lossy().to_string()),
                            meta: json!({"via": "claude-memory", "path": f.display().to_string(), "type": fm.get("type")}),
                        })
                        .await?;
                    if out.duplicate { dup += 1 } else { saved += 1 }
                }
            }
            if apply {
                println!("{found} berkas: {saved} disimpan sebagai episode, {dup} duplikat (tanpa LLM).");
            } else {
                println!("{found} berkas akan diimpor. Jalankan lagi dengan --apply untuk menyimpan.");
            }
        }
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
        Cmd::Watch { cmd } => {
            use memory_jobs::watch;
            match cmd {
                WatchCmd::List => {
                    let st = watch::status(&rt, Some(&subject));
                    print(j, &st, || {
                        let agy_info = st.agy_path.as_deref().unwrap_or("TIDAK DITEMUKAN");
                        let mut out = format!("agy CLI: {agy_info} | interval pantau: {} detik\n", st.poll_secs);
                        if st.folders.is_empty() {
                            out.push_str("Belum ada folder penting. Tambahkan dengan `engram watch add <folder>`.");
                        } else {
                            for f in &st.folders {
                                out.push_str(&format!(
                                    "{}\t{}\t({} berkas)\t{}\tperubahan terakhir: {}{}\n",
                                    f.id,
                                    f.label,
                                    f.file_count,
                                    f.path.display(),
                                    f.last_change_at.as_deref().unwrap_or("-"),
                                    f.last_error.as_ref().map(|e| format!(" [GALAT: {e}]")).unwrap_or_default()
                                ));
                            }
                        }
                        out.trim_end().to_string()
                    });
                }
                WatchCmd::Add { path, label, no_scan, no_process } => {
                    rt.pipeline.ensure_subject(&subject, None).await?;
                    let f = watch::add_folder(&rt.vault, &subject, &path, label.as_deref())?;
                    eprintln!("Folder penting terdaftar: {} ({})", f.label, f.path.display());
                    if !no_scan {
                        let rep = watch::scan_one(&rt, &f.id, false, 0).await?;
                        if let Some(err) = &rep.error {
                            bail!("gagal membaca konteks folder lewat agy CLI: {err}");
                        }
                        if rep.changed && !no_process {
                            let n = rt.drain().await?;
                            eprintln!("{n} job ekstraksi memori dijalankan.");
                        }
                        print(j, &rep, || {
                            format!(
                                "{}: {} berkas dibaca via agy CLI -> sumber {}",
                                rep.label,
                                rep.files,
                                rep.source_id.as_deref().unwrap_or("-")
                            )
                        });
                    } else {
                        print(j, &f, || format!("Terdaftar: {} ({})", f.label, f.id));
                    }
                }
                WatchCmd::Remove { target } => {
                    if watch::remove_folder(&rt.vault, &target)? {
                        println!("Folder penting dihapus dari daftar pantauan.");
                    } else {
                        bail!("folder penting tidak ditemukan: {target}");
                    }
                }
                WatchCmd::Scan { target, force, no_process } => {
                    let outcomes = match target {
                        Some(t) => vec![watch::scan_one(&rt, &t, force, 0).await?],
                        None => watch::scan_all(&rt, Some(&subject), force, 0).await?,
                    };
                    let any_changed = outcomes.iter().any(|o| o.changed && o.error.is_none());
                    if any_changed && !no_process {
                        let n = rt.drain().await?;
                        eprintln!("{n} job ekstraksi memori dijalankan.");
                    }
                    print(j, &outcomes, || {
                        if outcomes.is_empty() {
                            return "Tidak ada folder penting yang dipantau.".into();
                        }
                        outcomes
                            .iter()
                            .map(|o| {
                                if let Some(err) = &o.error {
                                    format!("{}: GALAT - {err}", o.label)
                                } else if o.changed {
                                    format!(
                                        "{}: diperbarui ({} berkas; +{} ~{} -{}) -> sumber {}",
                                        o.label,
                                        o.files,
                                        o.added,
                                        o.modified,
                                        o.removed,
                                        o.source_id.as_deref().unwrap_or("-")
                                    )
                                } else {
                                    format!("{}: tidak ada perubahan ({} berkas)", o.label, o.files)
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    });
                }
            }
        }
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

#[cfg(test)]
mod tests {
    #[test]
    fn frontmatter_split() {
        let (fm, body) = super::split_frontmatter("---\nname: disk-space\ndescription: \"disk nearly full\"\ntype: project\n---\n\nIsi catatan.");
        assert_eq!(fm.get("name").map(String::as_str), Some("disk-space"));
        assert_eq!(fm.get("description").map(String::as_str), Some("disk nearly full"));
        assert_eq!(body.trim(), "Isi catatan.");
        let (fm2, body2) = super::split_frontmatter("tanpa frontmatter");
        assert!(fm2.is_empty() && body2 == "tanpa frontmatter");
    }
}
