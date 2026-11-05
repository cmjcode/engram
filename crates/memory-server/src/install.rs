//! Pasang server MCP ini ke agent AI (Antigravity, VS Code, Claude Code, Cursor,
//! Codex CLI, Windsurf) dengan satu klik.
//!
//! Setiap client menyimpan daftar server MCP di berkas milik pengguna (JSON, atau
//! TOML untuk Codex). Modul ini menambah/mengganti satu entri bernama
//! [`SERVER_NAME`] tanpa menyentuh entri lain, membuat cadangan `.bak` lebih dulu,
//! dan menulis berkas secara atomik dengan izin hanya-pemilik (entri berisi token).
//!
//! Client koding murni (VS Code, Cursor, Codex, Windsurf) didaftarkan dengan
//! `?profile=coding` agar hanya tool sesi koding yang dimuat ke konteks; client
//! yang juga dipakai untuk memori hidup (Antigravity, Claude Code) memakai profil penuh.
//!
//! Fitur ini hanya aktif bila server tahu alamat MCP-nya sendiri ([`crate::AppState::mcp_url`]),
//! yaitu saat dijalankan aplikasi desktop atau `engram serve` di 127.0.0.1.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::mcp::Profile;

/// Nama entri server di config client.
pub const SERVER_NAME: &str = "engram";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    Antigravity,
    Vscode,
    ClaudeCode,
    Cursor,
    Codex,
    Windsurf,
}

/// Format berkas config client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Json,
    Toml,
}

impl Client {
    pub const ALL: [Client; 6] = [Client::Antigravity, Client::Vscode, Client::ClaudeCode, Client::Cursor, Client::Codex, Client::Windsurf];

    pub fn id(self) -> &'static str {
        match self {
            Client::Antigravity => "antigravity",
            Client::Vscode => "vscode",
            Client::ClaudeCode => "claude-code",
            Client::Cursor => "cursor",
            Client::Codex => "codex",
            Client::Windsurf => "windsurf",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Client::Antigravity => "Antigravity",
            Client::Vscode => "VS Code",
            Client::ClaudeCode => "Claude Code",
            Client::Cursor => "Cursor",
            Client::Codex => "Codex CLI",
            Client::Windsurf => "Windsurf",
        }
    }

    pub fn parse(id: &str) -> Option<Client> {
        Client::ALL.into_iter().find(|c| c.id() == id)
    }

    /// Profil tool yang dipasang untuk client ini.
    pub fn profile(self) -> Profile {
        match self {
            Client::Antigravity | Client::ClaudeCode => Profile::Full,
            Client::Vscode | Client::Cursor | Client::Codex | Client::Windsurf => Profile::Coding,
        }
    }

    fn format(self) -> Format {
        match self {
            Client::Codex => Format::Toml,
            _ => Format::Json,
        }
    }

    /// Kunci objek daftar server di berkas config.
    fn servers_key(self) -> &'static str {
        match self {
            Client::Vscode => "servers",
            Client::Codex => "mcp_servers",
            _ => "mcpServers",
        }
    }

    /// Petunjuk singkat setelah berhasil dipasang.
    fn after_install(self) -> &'static str {
        match self {
            Client::Antigravity => "Di Antigravity: panel Agent > ... > MCP Servers > Manage MCP Servers > Refresh.",
            Client::Vscode => "VS Code memuat ulang mcp.json otomatis. Aktifkan tool 'engram' di mode Agent pada Chat.",
            Client::ClaudeCode => "Mulai sesi Claude Code baru; cek dengan perintah /mcp.",
            Client::Cursor => "Di Cursor: Settings > MCP, pastikan 'engram' aktif (hijau).",
            Client::Codex => "Mulai sesi Codex baru; cek dengan perintah /mcp.",
            Client::Windsurf => "Di Windsurf: panel Cascade > MCP Servers > Refresh.",
        }
    }
}

/// Alamat yang didaftarkan ke client: URL server + `?profile=...` bila bukan profil penuh.
pub fn client_url(c: Client, url: &str) -> String {
    match c.profile() {
        Profile::Full => url.to_string(),
        p => format!("{url}{}profile={}", if url.contains('?') { '&' } else { '?' }, p.id()),
    }
}

/// Lokasi folder pengguna, dipisah agar bisa diuji dengan folder sementara.
#[derive(Debug, Clone)]
pub struct Dirs {
    pub home: PathBuf,
    /// `%APPDATA%` (Windows) atau `~/.config` (Linux); di macOS tidak dipakai.
    pub config: PathBuf,
}

impl Dirs {
    pub fn from_env() -> Option<Dirs> {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)?;
        let config = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| home.join("AppData").join("Roaming"))
        } else {
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"))
        };
        Some(Dirs { home, config })
    }

    /// Berkas config yang diedit untuk client.
    pub fn config_file(&self, c: Client) -> PathBuf {
        match c {
            Client::Antigravity => self.home.join(".gemini").join("antigravity").join("mcp_config.json"),
            Client::Vscode => self.vscode_user_dir().join("mcp.json"),
            Client::ClaudeCode => self.home.join(".claude.json"),
            Client::Cursor => self.home.join(".cursor").join("mcp.json"),
            Client::Codex => self.home.join(".codex").join("config.toml"),
            Client::Windsurf => self.home.join(".codeium").join("windsurf").join("mcp_config.json"),
        }
    }

    fn vscode_user_dir(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.home.join("Library").join("Application Support").join("Code").join("User")
        } else {
            self.config.join("Code").join("User")
        }
    }

    fn app_exists(&self, name: &str) -> bool {
        Path::new("/Applications").join(name).exists() || self.home.join("Applications").join(name).exists()
    }

    /// Apakah client tampaknya terpasang di komputer ini.
    pub fn detected(&self, c: Client) -> bool {
        match c {
            Client::Antigravity => self.home.join(".gemini").join("antigravity").exists() || self.app_exists("Antigravity.app"),
            Client::Vscode => self.vscode_user_dir().exists(),
            Client::ClaudeCode => self.home.join(".claude.json").exists() || self.home.join(".claude").exists() || find_cli(self, "claude").is_some(),
            Client::Cursor => self.home.join(".cursor").exists() || self.app_exists("Cursor.app"),
            Client::Codex => self.home.join(".codex").exists() || find_cli(self, "codex").is_some(),
            Client::Windsurf => self.home.join(".codeium").join("windsurf").exists() || self.app_exists("Windsurf.app"),
        }
    }
}

/// Entri server MCP dalam format masing-masing client (sebagai JSON; untuk Codex
/// dirender ke TOML oleh [`toml_section`]).
pub fn entry(c: Client, url: &str, token: Option<&str>) -> Value {
    let url = client_url(c, url);
    let mut e = Map::new();
    match c {
        Client::Antigravity | Client::Windsurf => {
            e.insert("serverUrl".into(), json!(url));
        }
        Client::Vscode | Client::ClaudeCode => {
            e.insert("type".into(), json!("http"));
            e.insert("url".into(), json!(url));
        }
        Client::Cursor | Client::Codex => {
            e.insert("url".into(), json!(url));
        }
    }
    if let Some(t) = token {
        let key = if c == Client::Codex { "http_headers" } else { "headers" };
        e.insert(key.into(), json!({"Authorization": format!("Bearer {t}")}));
    }
    if c == Client::Antigravity {
        e.insert("disabled".into(), json!(false));
    }
    Value::Object(e)
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallState {
    /// Belum ada entri.
    Missing,
    /// Ada entri dan sama dengan alamat & token saat ini.
    Installed,
    /// Ada entri tetapi alamat/token berbeda (perlu diperbarui).
    Outdated,
    /// Berkas config tidak bisa dibaca.
    Unreadable,
}

#[derive(Debug, Serialize)]
pub struct ClientStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub detected: bool,
    pub state: InstallState,
    pub config_path: String,
    /// Profil tool yang dipasang (`full` | `coding`).
    pub profile: &'static str,
}

fn read_text(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(None),
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("membaca {}: {e}", path.display())),
    }
}

fn read_json(path: &Path) -> Result<Option<Value>, String> {
    read_text(path)?
        .map(|s| {
            serde_json::from_str(&s).map_err(|e| format!("{} bukan JSON murni ({e}). Hapus komentar/koma berlebih di berkas itu, lalu coba lagi.", path.display()))
        })
        .transpose()
}

/// Entri `engram` yang sekarang ada di config client (sebagai JSON), bila ada.
fn current_entry(path: &Path, c: Client) -> Result<Option<Value>, String> {
    match c.format() {
        Format::Json => Ok(read_json(path)?.and_then(|d| d.get(c.servers_key()).and_then(|s| s.get(SERVER_NAME)).cloned())),
        Format::Toml => {
            let Some(text) = read_text(path)? else { return Ok(None) };
            let doc: toml::Table = toml::from_str(&text).map_err(|e| format!("{} bukan TOML valid ({e})", path.display()))?;
            let Some(e) = doc.get(c.servers_key()).and_then(|s| s.get(SERVER_NAME)) else { return Ok(None) };
            Ok(Some(serde_json::to_value(e).map_err(|e| e.to_string())?))
        }
    }
}

/// Status pemasangan satu client.
pub fn status(dirs: &Dirs, c: Client, url: &str, token: Option<&str>) -> ClientStatus {
    let path = dirs.config_file(c);
    let state = match current_entry(&path, c) {
        Err(_) => InstallState::Unreadable,
        Ok(None) => InstallState::Missing,
        Ok(Some(cur)) if same_entry(&cur, &entry(c, url, token)) => InstallState::Installed,
        Ok(Some(_)) => InstallState::Outdated,
    };
    ClientStatus { id: c.id(), label: c.label(), detected: dirs.detected(c), state, config_path: path.display().to_string(), profile: c.profile().id() }
}

/// Bandingkan hanya alamat & header (client boleh menambah kunci lain, mis. `disabled`).
fn same_entry(cur: &Value, want: &Value) -> bool {
    let url = |v: &Value| v.get("serverUrl").or_else(|| v.get("url")).cloned();
    let headers = |v: &Value| v.get("headers").or_else(|| v.get("http_headers")).cloned();
    url(cur) == url(want) && headers(cur) == headers(want)
}

/// Tambah/ganti (`Some`) atau hapus (`None`) entri server di berkas config JSON.
pub fn edit_config(path: &Path, servers_key: &str, value: Option<Value>) -> Result<(), String> {
    let real_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path = real_path.as_path();
    let mut doc = read_json(path)?.unwrap_or_else(|| json!({}));
    let root = doc.as_object_mut().ok_or_else(|| format!("{} bukan objek JSON", path.display()))?;
    let servers = root.entry(servers_key).or_insert_with(|| json!({}));
    let servers = servers.as_object_mut().ok_or_else(|| format!("'{servers_key}' di {} bukan objek", path.display()))?;
    match value {
        Some(v) => {
            servers.remove("memori");
            servers.insert(SERVER_NAME.into(), v);
        }
        None => {
            if servers.remove(SERVER_NAME).is_none() {
                return Ok(());
            }
        }
    }
    let body = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n";
    write_config(path, &body, "json")
}

/// Render entri (hasil [`entry`] untuk Codex) sebagai bagian TOML `[mcp_servers.engram]`.
fn toml_section(servers_key: &str, value: &Value) -> String {
    let quote = |s: &str| toml::Value::String(s.to_string()).to_string();
    let mut out = format!("[{servers_key}.{SERVER_NAME}]\n");
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            match v {
                Value::String(s) => out.push_str(&format!("{k} = {}\n", quote(s))),
                Value::Bool(b) => out.push_str(&format!("{k} = {b}\n")),
                Value::Object(m) => {
                    let inner: Vec<String> = m.iter().filter_map(|(ik, iv)| iv.as_str().map(|s| format!("{} = {}", quote(ik), quote(s)))).collect();
                    out.push_str(&format!("{k} = {{ {} }}\n", inner.join(", ")));
                }
                Value::Array(a) => {
                    let inner: Vec<String> = a.iter().filter_map(|x| x.as_str().map(quote)).collect();
                    out.push_str(&format!("{k} = [{}]\n", inner.join(", ")));
                }
                _ => {}
            }
        }
    }
    out
}

/// Apakah baris adalah header tabel TOML (`[x]` atau `[[x]]`).
fn is_toml_header(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('[') && t.trim_end().ends_with(']')
}

/// Apakah header menunjuk tabel `servers_key.name` atau subtabelnya (dengan/tanpa kutip).
fn header_is_server(line: &str, servers_key: &str, name: &str) -> bool {
    let t = line.trim().trim_start_matches('[').trim_end_matches(']').replace(['"', '\'', ' '], "");
    let prefix = format!("{servers_key}.{name}");
    t == prefix || t.starts_with(&format!("{prefix}."))
}

/// Hapus bagian `[servers_key.name]` (dan subtabelnya) dari teks TOML; isi lain utuh.
fn strip_toml_section(text: &str, servers_key: &str, name: &str) -> (String, bool) {
    let mut out = String::new();
    let mut skipping = false;
    let mut removed = false;
    for line in text.lines() {
        if is_toml_header(line) {
            skipping = header_is_server(line, servers_key, name);
            removed |= skipping;
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    (out, removed)
}

/// Tambah/ganti (`Some`) atau hapus (`None`) entri server di berkas config TOML (Codex).
/// Bagian lain berkas tidak disentuh (komentar dan urutan tetap).
pub fn edit_toml_config(path: &Path, servers_key: &str, value: Option<Value>) -> Result<(), String> {
    let real_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path = real_path.as_path();
    let text = read_text(path)?.unwrap_or_default();
    // Pastikan berkas valid sebelum diedit secara tekstual.
    toml::from_str::<toml::Table>(&text).map_err(|e| format!("{} bukan TOML valid ({e}); perbaiki dulu lalu coba lagi.", path.display()))?;
    let (text, _) = strip_toml_section(&text, servers_key, "memori");
    let (mut body, removed) = strip_toml_section(&text, servers_key, SERVER_NAME);
    match value {
        Some(v) => {
            let body_trim = body.trim_end();
            body = if body_trim.is_empty() { String::new() } else { format!("{body_trim}\n\n") };
            body.push_str(&toml_section(servers_key, &v));
        }
        None => {
            if !removed {
                return Ok(());
            }
            body = format!("{}\n", body.trim_end());
        }
    }
    toml::from_str::<toml::Table>(&body).map_err(|e| format!("hasil edit {} tidak valid ({e})", path.display()))?;
    write_config(path, &body, "toml")
}

/// Cadangkan (`.bak`) lalu tulis atomik dengan izin hanya-pemilik.
fn write_config(path: &Path, body: &str, ext: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("membuat {}: {e}", dir.display()))?;
    }
    if path.exists() {
        let bak = path.with_extension(format!("{ext}.bak"));
        std::fs::copy(path, &bak).map_err(|e| format!("mencadangkan ke {}: {e}", bak.display()))?;
    }
    let tmp = path.with_extension(format!("{ext}.tmp"));
    write_private(&tmp, body).map_err(|e| format!("menulis {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("mengganti {}: {e}", path.display()))?;
    Ok(())
}

fn write_private(path: &Path, body: &str) -> std::io::Result<()> {
    std::fs::write(path, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Folder yang biasa berisi CLI `claude`/`codex`. Aplikasi yang dibuka dari Finder hanya
/// mewarisi PATH minimal, jadi lokasi umum ditambahkan manual.
fn search_path(dirs: &Dirs) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    let h = &dirs.home;
    v.extend([
        h.join(".local/bin"),
        h.join(".claude/local"),
        h.join(".npm-global/bin"),
        h.join(".bun/bin"),
        h.join(".volta/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    if let Some(appdata) = std::env::var_os("APPDATA") {
        v.push(PathBuf::from(appdata).join("npm"));
    }
    v
}

fn find_cli(dirs: &Dirs, name: &str) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) { vec![format!("{name}.exe"), format!("{name}.cmd")] } else { vec![name.to_string()] };
    search_path(dirs).into_iter().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

/// Daftarkan lewat `claude mcp add` (cara resmi). Mengembalikan `Err` bila CLI tidak ada atau gagal.
async fn claude_cli(dirs: &Dirs, url: Option<(&str, Option<&str>)>) -> Result<(), String> {
    let bin = find_cli(dirs, "claude").ok_or("CLI claude tidak ditemukan")?;
    let path = std::env::join_paths(search_path(dirs)).map_err(|e| e.to_string())?;
    let run = |args: Vec<String>| {
        let mut cmd = tokio::process::Command::new(&bin);
        cmd.args(args).env("PATH", &path).stdin(std::process::Stdio::null()).kill_on_drop(true);
        async move { tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output()).await }
    };
    // Hapus dulu agar `add` tidak gagal karena nama sudah dipakai.
    let _ = run(vec!["mcp".into(), "remove".into(), "-s".into(), "user".into(), SERVER_NAME.into()]).await;
    let Some((url, token)) = url else { return Ok(()) };
    let url = client_url(Client::ClaudeCode, url);
    let mut args: Vec<String> = ["mcp", "add", "-s", "user", "--transport", "http", SERVER_NAME, url.as_str()].map(String::from).into();
    if let Some(t) = token {
        args.extend(["--header".into(), format!("Authorization: Bearer {t}")]);
    }
    let out = run(args).await.map_err(|_| "claude mcp add tidak selesai dalam 30 detik".to_string())?.map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

const RULE_START: &str = "<!-- ENGRAM_START -->";
const RULE_END: &str = "<!-- ENGRAM_END -->";
const LEGACY_RULE_START: &str = "<!-- MNEMONIC_MEMORY_START -->";
const LEGACY_RULE_END: &str = "<!-- MNEMONIC_MEMORY_END -->";

const RULE_BLOCK: &str = "<!-- ENGRAM_START -->
## Engram (`engram` MCP)

- **Simpan Otomatis (`remember`)**: Saat pengguna menceritakan kisah hidup, fakta pribadi/keluarga, pengalaman masa lalu, preferensi, atau keputusan penting (termasuk keputusan arsitektur/teknis, konvensi kode, akar masalah bug, dan konfigurasi di project), panggil tool MCP `remember` (dari server `engram`) untuk menyimpannya tanpa perlu diminta. Isi parameter `source` dengan format `<client>:<nama-project>` (misal `antigravity:LLM-MEMORY`) dan `project` sesuai nama project.
- **Ambil Konteks (`project_context`, `recall`, `who_is`, `timeline`, `principles`, `family_tree`)**: Saat mulai mengerjakan tugas di suatu project atau saat pengguna bertanya tentang arsitektur/keputusan teknis/masa lalu/keluarga/prinsip kerja, panggil tool MCP `engram` yang relevan terlebih dahulu (`project_context` untuk ringkasan lengkap project, atau `recall` dengan parameter `project`). Selalu bedakan fakta yang masih berlaku sekarang dengan riwayat masa lalu.
- **Rangkum & Padatkan Memori (`compact`)**: Saat memori sudah cukup banyak atau pengguna meminta merangkum/memadatkan memori, panggil tool MCP `compact` dengan `mode=\"candidates\"` untuk meninjau klaster episode serupa, fakta redundan/sesaat per entitas, dan review terbuka; lalu kirim rangkuman intinya lewat `compact` dengan `mode=\"apply\"` dan `plan` (non-destruktif: episode mikro ditandai sebagai `retelling` dari episode kanonik baru sehingga bukti asli tetap utuh).
<!-- ENGRAM_END -->";

/// Rule ringkas untuk client koding (profil `coding`: tanpa tool memori hidup).
const CODING_RULE_BLOCK: &str = "<!-- ENGRAM_START -->
## Engram (`engram` MCP)

- **Awal tugas**: panggil `project_context` (parameter `project` = nama repositori/folder kerja, `task` = tugas saat ini) sebelum membaca kode, agar keputusan arsitektur, konvensi, dan akar masalah bug dari sesi sebelumnya ikut dipertimbangkan. Untuk pertanyaan spesifik pakai `recall` dengan parameter `project`.
- **Fakta berlabel waktu**: hasil tool membedakan fakta yang masih berlaku dan riwayat. Jangan menyajikan fakta lama sebagai kondisi sekarang; bila tool menjawab tidak ada catatan, katakan apa adanya.
- **Simpan keputusan (`remember`)**: setelah keputusan teknis, perubahan arsitektur, konvensi baru, atau akar masalah bug ditemukan, simpan ringkasannya lewat `remember` dengan `source` berformat `<client>:<nama-project>` (misal `codex:LLM-MEMORY`).
<!-- ENGRAM_END -->";

fn strip_marker_block(text: &str, start: &str, end: &str) -> String {
    let Some(s) = text.find(start) else { return text.to_string() };
    let Some(e_rel) = text[s..].find(end) else { return text.to_string() };
    let mut e = s + e_rel + end.len();
    if text[e..].starts_with('\n') {
        e += 1;
    }
    format!("{}{}", &text[..s], &text[e..])
}

fn upsert_rule_file(path: &Path, block: &str, uninstall: bool) -> Result<(), String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("membaca {}: {e}", path.display())),
    };
    let cleaned = strip_marker_block(&existing, LEGACY_RULE_START, LEGACY_RULE_END);
    let without_engram = strip_marker_block(&cleaned, RULE_START, RULE_END);
    let new_text = if uninstall {
        let trimmed = without_engram.trim_end();
        if trimmed.is_empty() { String::new() } else { format!("{trimmed}\n") }
    } else {
        let trimmed = without_engram.trim_end();
        if trimmed.is_empty() {
            format!("{block}\n")
        } else {
            format!("{trimmed}\n\n{block}\n")
        }
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("membuat {}: {e}", dir.display()))?;
    }
    std::fs::write(path, new_text).map_err(|e| format!("menulis {}: {e}", path.display()))
}

/// Berkas rule global tiap client (kosong = client tidak punya rule global berbasis berkas).
fn rule_files(dirs: &Dirs, c: Client) -> Vec<PathBuf> {
    match c {
        Client::Antigravity => vec![dirs.home.join(".gemini").join("GEMINI.md"), dirs.home.join(".gemini").join("config").join("GEMINI.md")],
        Client::ClaudeCode => vec![dirs.home.join(".claude").join("CLAUDE.md")],
        Client::Codex => vec![dirs.home.join(".codex").join("AGENTS.md")],
        Client::Windsurf => vec![dirs.home.join(".codeium").join("windsurf").join("memories").join("global_rules.md")],
        Client::Vscode | Client::Cursor => vec![],
    }
}

fn sync_client_rules(dirs: &Dirs, c: Client, uninstall: bool) {
    let block = match c.profile() {
        Profile::Full => RULE_BLOCK,
        Profile::Coding => CODING_RULE_BLOCK,
    };
    for f in rule_files(dirs, c) {
        if let Err(e) = upsert_rule_file(&f, block, uninstall) {
            tracing::warn!(error = %e, path = %f.display(), "gagal memperbarui rule client");
        }
    }
}

fn edit_client_config(path: &Path, c: Client, value: Option<Value>) -> Result<(), String> {
    match c.format() {
        Format::Json => edit_config(path, c.servers_key(), value),
        Format::Toml => edit_toml_config(path, c.servers_key(), value),
    }
}

/// Pasang server ke client. Mengembalikan pesan untuk pengguna.
pub async fn install(dirs: &Dirs, c: Client, url: &str, token: Option<&str>) -> Result<String, String> {
    sync_client_rules(dirs, c, false);
    let path = dirs.config_file(c);
    if c == Client::ClaudeCode {
        match claude_cli(dirs, Some((url, token))).await {
            Ok(()) => return Ok(format!("Terdaftar di Claude Code (scope user). {}", c.after_install())),
            // CLI tidak ada / gagal: tulis langsung ke ~/.claude.json (lokasi yang sama dipakai CLI).
            Err(e) => tracing::warn!(error = %e, "claude mcp add gagal, menulis ~/.claude.json langsung"),
        }
    }
    edit_client_config(&path, c, Some(entry(c, url, token)))?;
    let profile = match c.profile() {
        Profile::Full => String::new(),
        p => format!(" Profil tool: {}.", p.id()),
    };
    Ok(format!("Terdaftar di {} ({}).{profile} {}", c.label(), path.display(), c.after_install()))
}

/// Hapus server dari client.
pub async fn uninstall(dirs: &Dirs, c: Client) -> Result<String, String> {
    sync_client_rules(dirs, c, true);
    if c == Client::ClaudeCode {
        let _ = claude_cli(dirs, None).await;
    }
    edit_client_config(&dirs.config_file(c), c, None)?;
    Ok(format!("Dihapus dari {}.", c.label()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(t: &tempfile::TempDir) -> Dirs {
        Dirs { home: t.path().join("home"), config: t.path().join("config") }
    }

    #[test]
    fn edit_keeps_other_servers_and_keys() {
        let t = tempfile::tempdir().unwrap();
        let d = dirs(&t);
        let p = d.config_file(Client::Vscode);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, r#"{"servers":{"lain":{"type":"stdio","command":"x"}},"inputs":[]}"#).unwrap();

        assert_eq!(status(&d, Client::Vscode, "http://127.0.0.1:7878/mcp", Some("tk")).state, InstallState::Missing);
        edit_config(&p, "servers", Some(entry(Client::Vscode, "http://127.0.0.1:7878/mcp", Some("tk")))).unwrap();

        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(doc["servers"]["lain"]["command"], "x");
        assert_eq!(doc["inputs"], json!([]));
        // Client koding murni memakai profil `coding`.
        assert_eq!(doc["servers"]["engram"]["url"], "http://127.0.0.1:7878/mcp?profile=coding");
        assert_eq!(doc["servers"]["engram"]["headers"]["Authorization"], "Bearer tk");
        assert!(p.with_extension("json.bak").exists());
        // Kunci asli tetap di urutan semula.
        let keys: Vec<_> = doc.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["servers", "inputs"]);

        assert_eq!(status(&d, Client::Vscode, "http://127.0.0.1:7878/mcp", Some("tk")).state, InstallState::Installed);
        assert_eq!(status(&d, Client::Vscode, "http://127.0.0.1:7878/mcp", Some("baru")).state, InstallState::Outdated);

        edit_config(&p, "servers", None).unwrap();
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(doc["servers"].get("engram").is_none());
        assert_eq!(doc["servers"]["lain"]["command"], "x");
    }

    #[test]
    fn creates_missing_file_privately() {
        let t = tempfile::tempdir().unwrap();
        let d = dirs(&t);
        let p = d.config_file(Client::Antigravity);
        edit_config(&p, "mcpServers", Some(entry(Client::Antigravity, "http://h/mcp", None))).unwrap();
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        // Antigravity dipakai juga untuk memori hidup: profil penuh, tanpa query.
        assert_eq!(doc, json!({"mcpServers": {"engram": {"serverUrl": "http://h/mcp", "disabled": false}}}));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn refuses_to_overwrite_non_json() {
        let t = tempfile::tempdir().unwrap();
        let d = dirs(&t);
        let p = d.config_file(Client::Vscode);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let jsonc = "{\n  // komentar\n  \"servers\": {}\n}";
        std::fs::write(&p, jsonc).unwrap();
        assert!(edit_config(&p, "servers", Some(json!({}))).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), jsonc);
        assert_eq!(status(&d, Client::Vscode, "u", None).state, InstallState::Unreadable);
    }

    #[test]
    fn cursor_and_windsurf_entries() {
        let cur = entry(Client::Cursor, "http://h/mcp", Some("tk"));
        assert_eq!(cur, json!({"url": "http://h/mcp?profile=coding", "headers": {"Authorization": "Bearer tk"}}));
        let ws = entry(Client::Windsurf, "http://h/mcp", Some("tk"));
        assert_eq!(ws, json!({"serverUrl": "http://h/mcp?profile=coding", "headers": {"Authorization": "Bearer tk"}}));
        assert_eq!(client_url(Client::Codex, "http://h/mcp?x=1"), "http://h/mcp?x=1&profile=coding");
    }

    #[test]
    fn codex_toml_edit_keeps_rest_of_file() {
        let t = tempfile::tempdir().unwrap();
        let d = dirs(&t);
        let p = d.config_file(Client::Codex);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let before = "# config codex\nmodel = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"npx\"\nargs = [\"-y\", \"x\"]\n\n[mcp_servers.\"engram\"]\nurl = \"http://lama/mcp\"\n\n[mcp_servers.engram.http_headers]\nAuthorization = \"Bearer lama\"\n\n[profiles.fast]\nmodel = \"gpt-5-mini\"\n";
        std::fs::write(&p, before).unwrap();
        assert_eq!(status(&d, Client::Codex, "http://127.0.0.1:7878/mcp", Some("tk")).state, InstallState::Outdated);

        edit_toml_config(&p, "mcp_servers", Some(entry(Client::Codex, "http://127.0.0.1:7878/mcp", Some("tk")))).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        let doc: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(doc["model"].as_str(), Some("gpt-5"));
        assert_eq!(doc["mcp_servers"]["other"]["command"].as_str(), Some("npx"));
        assert_eq!(doc["profiles"]["fast"]["model"].as_str(), Some("gpt-5-mini"));
        assert_eq!(doc["mcp_servers"]["engram"]["url"].as_str(), Some("http://127.0.0.1:7878/mcp?profile=coding"));
        assert_eq!(doc["mcp_servers"]["engram"]["http_headers"]["Authorization"].as_str(), Some("Bearer tk"));
        assert!(text.starts_with("# config codex\n"), "komentar harus utuh: {text}");
        assert_eq!(text.matches("[mcp_servers.engram").count(), 1, "{text}");
        assert!(p.with_extension("toml.bak").exists());
        assert_eq!(status(&d, Client::Codex, "http://127.0.0.1:7878/mcp", Some("tk")).state, InstallState::Installed);

        edit_toml_config(&p, "mcp_servers", None).unwrap();
        let doc: toml::Table = toml::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(doc["mcp_servers"].get("engram").is_none());
        assert_eq!(doc["mcp_servers"]["other"]["command"].as_str(), Some("npx"));
        assert_eq!(status(&d, Client::Codex, "u", None).state, InstallState::Missing);
    }

    #[test]
    fn codex_toml_created_when_missing_and_invalid_refused() {
        let t = tempfile::tempdir().unwrap();
        let d = dirs(&t);
        let p = d.config_file(Client::Codex);
        edit_toml_config(&p, "mcp_servers", Some(entry(Client::Codex, "http://h/mcp", None))).unwrap();
        let doc: toml::Table = toml::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(doc["mcp_servers"]["engram"]["url"].as_str(), Some("http://h/mcp?profile=coding"));
        assert!(doc["mcp_servers"]["engram"].get("http_headers").is_none());

        let broken = "model = \n";
        std::fs::write(&p, broken).unwrap();
        assert!(edit_toml_config(&p, "mcp_servers", Some(json!({}))).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), broken);
        assert_eq!(status(&d, Client::Codex, "u", None).state, InstallState::Unreadable);
    }

    #[test]
    fn rule_block_per_profile() {
        let t = tempfile::tempdir().unwrap();
        let d = dirs(&t);
        sync_client_rules(&d, Client::Codex, false);
        let agents = std::fs::read_to_string(d.home.join(".codex").join("AGENTS.md")).unwrap();
        assert!(agents.contains("project_context") && !agents.contains("family_tree"), "{agents}");
        sync_client_rules(&d, Client::Codex, true);
        assert_eq!(std::fs::read_to_string(d.home.join(".codex").join("AGENTS.md")).unwrap(), "");
    }
}
