//! Pasang server MCP ini ke agent AI (Antigravity, VS Code, Claude Code) dengan satu klik.
//!
//! Setiap client menyimpan daftar server MCP di berkas JSON milik pengguna. Modul ini
//! menambah/mengganti satu entri bernama [`SERVER_NAME`] tanpa menyentuh entri lain,
//! membuat cadangan `.bak` lebih dulu, dan menulis berkas secara atomik dengan izin
//! hanya-pemilik (entri berisi token akses).
//!
//! Fitur ini hanya aktif bila server tahu alamat MCP-nya sendiri ([`crate::AppState::mcp_url`]),
//! yaitu saat dijalankan aplikasi desktop atau `engram serve` di 127.0.0.1.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value, json};

/// Nama entri server di config client.
pub const SERVER_NAME: &str = "engram";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    Antigravity,
    Vscode,
    ClaudeCode,
}

impl Client {
    pub const ALL: [Client; 3] = [Client::Antigravity, Client::Vscode, Client::ClaudeCode];

    pub fn id(self) -> &'static str {
        match self {
            Client::Antigravity => "antigravity",
            Client::Vscode => "vscode",
            Client::ClaudeCode => "claude-code",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Client::Antigravity => "Antigravity",
            Client::Vscode => "VS Code",
            Client::ClaudeCode => "Claude Code",
        }
    }

    pub fn parse(id: &str) -> Option<Client> {
        Client::ALL.into_iter().find(|c| c.id() == id)
    }

    /// Kunci objek daftar server di berkas config.
    fn servers_key(self) -> &'static str {
        match self {
            Client::Vscode => "servers",
            _ => "mcpServers",
        }
    }

    /// Petunjuk singkat setelah berhasil dipasang.
    fn after_install(self) -> &'static str {
        match self {
            Client::Antigravity => "Di Antigravity: panel Agent > ... > MCP Servers > Manage MCP Servers > Refresh.",
            Client::Vscode => "VS Code memuat ulang mcp.json otomatis. Aktifkan tool 'engram' di mode Agent pada Chat.",
            Client::ClaudeCode => "Mulai sesi Claude Code baru; cek dengan perintah /mcp.",
        }
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
        }
    }

    fn vscode_user_dir(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.home.join("Library").join("Application Support").join("Code").join("User")
        } else {
            self.config.join("Code").join("User")
        }
    }

    /// Apakah client tampaknya terpasang di komputer ini.
    pub fn detected(&self, c: Client) -> bool {
        match c {
            Client::Antigravity => {
                self.home.join(".gemini").join("antigravity").exists()
                    || Path::new("/Applications/Antigravity.app").exists()
                    || self.home.join("Applications").join("Antigravity.app").exists()
            }
            Client::Vscode => self.vscode_user_dir().exists(),
            Client::ClaudeCode => self.home.join(".claude.json").exists() || self.home.join(".claude").exists() || find_claude_cli(self).is_some(),
        }
    }
}

/// Entri server MCP dalam format masing-masing client.
pub fn entry(c: Client, url: &str, token: Option<&str>) -> Value {
    let mut e = Map::new();
    match c {
        Client::Antigravity => {
            e.insert("serverUrl".into(), json!(url));
        }
        Client::Vscode | Client::ClaudeCode => {
            e.insert("type".into(), json!("http"));
            e.insert("url".into(), json!(url));
        }
    }
    if let Some(t) = token {
        e.insert("headers".into(), json!({"Authorization": format!("Bearer {t}")}));
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
    /// Berkas config tidak bisa dibaca sebagai JSON.
    Unreadable,
}

#[derive(Debug, Serialize)]
pub struct ClientStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub detected: bool,
    pub state: InstallState,
    pub config_path: String,
}

fn read_json(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(None),
        Ok(s) => serde_json::from_str(&s).map(Some).map_err(|e| {
            format!("{} bukan JSON murni ({e}). Hapus komentar/koma berlebih di berkas itu, lalu coba lagi.", path.display())
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("membaca {}: {e}", path.display())),
    }
}

/// Status pemasangan satu client.
pub fn status(dirs: &Dirs, c: Client, url: &str, token: Option<&str>) -> ClientStatus {
    let path = dirs.config_file(c);
    let state = match read_json(&path) {
        Err(_) => InstallState::Unreadable,
        Ok(doc) => match doc.as_ref().and_then(|d| d.get(c.servers_key())).and_then(|s| s.get(SERVER_NAME)) {
            None => InstallState::Missing,
            Some(cur) if same_entry(cur, &entry(c, url, token)) => InstallState::Installed,
            Some(_) => InstallState::Outdated,
        },
    };
    ClientStatus { id: c.id(), label: c.label(), detected: dirs.detected(c), state, config_path: path.display().to_string() }
}

/// Bandingkan hanya alamat & header (client boleh menambah kunci lain, mis. `disabled`).
fn same_entry(cur: &Value, want: &Value) -> bool {
    let url = |v: &Value| v.get("serverUrl").or_else(|| v.get("url")).cloned();
    url(cur) == url(want) && cur.get("headers") == want.get("headers")
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
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("membuat {}: {e}", dir.display()))?;
    }
    if path.exists() {
        let bak = path.with_extension("json.bak");
        std::fs::copy(path, &bak).map_err(|e| format!("mencadangkan ke {}: {e}", bak.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    write_private(&tmp, &body).map_err(|e| format!("menulis {}: {e}", tmp.display()))?;
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

/// Folder yang biasa berisi CLI `claude`. Aplikasi yang dibuka dari Finder hanya
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

fn find_claude_cli(dirs: &Dirs) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["claude.exe", "claude.cmd"] } else { &["claude"] };
    search_path(dirs).into_iter().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

/// Daftarkan lewat `claude mcp add` (cara resmi). Mengembalikan `Err` bila CLI tidak ada atau gagal.
async fn claude_cli(dirs: &Dirs, url: Option<(&str, Option<&str>)>) -> Result<(), String> {
    let bin = find_claude_cli(dirs).ok_or("CLI claude tidak ditemukan")?;
    let path = std::env::join_paths(search_path(dirs)).map_err(|e| e.to_string())?;
    let run = |args: Vec<String>| {
        let mut cmd = tokio::process::Command::new(&bin);
        cmd.args(args).env("PATH", &path).stdin(std::process::Stdio::null()).kill_on_drop(true);
        async move { tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output()).await }
    };
    // Hapus dulu agar `add` tidak gagal karena nama sudah dipakai.
    let _ = run(vec!["mcp".into(), "remove".into(), "-s".into(), "user".into(), SERVER_NAME.into()]).await;
    let Some((url, token)) = url else { return Ok(()) };
    let mut args: Vec<String> = ["mcp", "add", "-s", "user", "--transport", "http", SERVER_NAME, url].map(String::from).into();
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

- **Simpan Otomatis (`remember`)**: Saat pengguna menceritakan kisah hidup, fakta pribadi/keluarga, pengalaman masa lalu, preferensi, atau keputusan penting (termasuk keputusan arsitektur/teknis di project), panggil tool MCP `remember` (dari server `engram`) untuk menyimpannya tanpa perlu diminta. Isi parameter `source` dengan format `<client>:<nama-project>` (misal `antigravity:LLM-MEMORY`) dan sebutkan nama project di dalam `text` bila konteksnya spesifik pada project tersebut.
- **Ambil Konteks (`recall`, `who_is`, `timeline`, `principles`, `family_tree`)**: Saat pengguna bertanya tentang masa lalu, anggota keluarga, relasi seseorang, kronologi peristiwa, prinsip hidup, atau konteks project sebelumnya, panggil tool MCP `engram` yang relevan terlebih dahulu sebelum menjawab. Untuk mencari memori spesifik project saat ini, sertakan nama project di `query` pada `recall` atau `entity` pada `timeline`. Selalu bedakan fakta yang masih berlaku sekarang dengan riwayat masa lalu.
- **Rangkum & Padatkan Memori (`compact`)**: Saat memori sudah cukup banyak atau pengguna meminta merangkum/memadatkan memori, panggil tool MCP `compact` dengan `mode=\"candidates\"` untuk meninjau klaster episode serupa, fakta redundan/sesaat per entitas, dan review terbuka; lalu kirim rangkuman intinya lewat `compact` dengan `mode=\"apply\"` dan `plan` (non-destruktif: episode mikro ditandai sebagai `retelling` dari episode kanonik baru sehingga bukti asli tetap utuh).
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

fn upsert_rule_file(path: &Path, uninstall: bool) -> Result<(), String> {
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
            format!("{RULE_BLOCK}\n")
        } else {
            format!("{trimmed}\n\n{RULE_BLOCK}\n")
        }
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("membuat {}: {e}", dir.display()))?;
    }
    std::fs::write(path, new_text).map_err(|e| format!("menulis {}: {e}", path.display()))
}

fn sync_client_rules(dirs: &Dirs, c: Client, uninstall: bool) {
    let files: Vec<PathBuf> = match c {
        Client::Antigravity => vec![
            dirs.home.join(".gemini").join("GEMINI.md"),
            dirs.home.join(".gemini").join("config").join("GEMINI.md"),
        ],
        Client::ClaudeCode => vec![dirs.home.join(".claude").join("CLAUDE.md")],
        Client::Vscode => vec![],
    };
    for f in files {
        if let Err(e) = upsert_rule_file(&f, uninstall) {
            tracing::warn!(error = %e, path = %f.display(), "gagal memperbarui rule client");
        }
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
    edit_config(&path, c.servers_key(), Some(entry(c, url, token)))?;
    Ok(format!("Terdaftar di {} ({}). {}", c.label(), path.display(), c.after_install()))
}

/// Hapus server dari client.
pub async fn uninstall(dirs: &Dirs, c: Client) -> Result<String, String> {
    sync_client_rules(dirs, c, true);
    if c == Client::ClaudeCode {
        let _ = claude_cli(dirs, None).await;
    }
    edit_config(&dirs.config_file(c), c.servers_key(), None)?;
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
        assert_eq!(doc["servers"]["engram"]["url"], "http://127.0.0.1:7878/mcp");
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
}
