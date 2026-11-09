//! Atribusi proyek dari folder kerja: nama proyek diambil dari akar repositori
//! git (bukan sekadar basename `cwd`), dan worktree git dipetakan ke repo
//! induknya sebagai alias sehingga memori dari semua worktree menyatu.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectId {
    /// Nama proyek kanonik (basename akar repo, atau repo induk bila worktree).
    pub name: String,
    /// Akar proyek (folder yang memuat `.git`, atau `cwd` bila bukan repo).
    pub root: PathBuf,
    /// Nama worktree bila `cwd` ada di worktree terpisah; dipakai sebagai alias.
    pub worktree: Option<String>,
    /// Ditemukan lewat `.git` (bukan tebakan dari nama folder).
    pub from_git: bool,
}

fn is_ignored_dir(p: &Path) -> bool {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if p.parent().is_none() {
        return true;
    }
    if let Some(h) = &home
        && (p == h || p == h.join(".gemini") || p == h.join(".gemini").join("config") || p == h.join(".claude") || p == h.join(".codex") || p == h.join(".cursor"))
    {
        return true;
    }
    false
}

fn basename(p: &Path) -> Option<String> {
    let n = p.file_name()?.to_string_lossy().trim().to_string();
    if n.is_empty() || n.starts_with('.') { None } else { Some(n) }
}

/// Baca `gitdir: <path>` dari berkas `.git` (worktree/submodule).
fn gitdir_of(file: &Path) -> Option<PathBuf> {
    let s = std::fs::read_to_string(file).ok()?;
    let line = s.lines().find_map(|l| l.trim().strip_prefix("gitdir:"))?.trim();
    let p = PathBuf::from(line);
    Some(if p.is_relative() { file.parent()?.join(p) } else { p })
}

/// Folder sementara OS (`/tmp`, `/private/tmp`, `/var/folders`, `$TMPDIR`): bukan proyek.
fn temp_root_of(p: &Path) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = ["/tmp", "/private/tmp", "/var/folders", "/private/var/folders", "/var/tmp", "/private/var/tmp"].iter().map(PathBuf::from).collect();
    if let Some(t) = std::env::var_os("TMPDIR").map(PathBuf::from).filter(|t| t.is_absolute() && t.components().count() > 2) {
        roots.push(t);
    }
    roots.into_iter().find(|r| p.starts_with(r))
}

/// Nama folder versi Claude Code: setiap karakter non-alfanumerik menjadi `-`.
fn claude_encode(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Uraikan nama folder proyek Claude Code (`-Users-budi-Documents-PROJECT-LLM-MEMORY`) kembali
/// menjadi path yang ada di disk. Tanda `-` ambigu (pemisah path atau bagian nama), jadi
/// setiap langkah mencocokkan isi folder yang benar-benar ada, utamakan nama terpanjang.
pub fn decode_claude_project_dir(encoded: &str) -> Option<PathBuf> {
    fn walk(dir: &Path, rest: &str, depth: usize) -> Option<PathBuf> {
        if rest.is_empty() {
            return Some(dir.to_path_buf());
        }
        if depth > 24 {
            return None;
        }
        let mut names: Vec<String> = std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort_by_key(|n| std::cmp::Reverse(n.len()));
        for n in names {
            let enc = claude_encode(&n);
            if enc.is_empty() {
                continue;
            }
            let next = if rest == enc {
                Some("")
            } else {
                rest.strip_prefix(&enc).and_then(|r| r.strip_prefix('-'))
            };
            if let Some(r) = next
                && let Some(found) = walk(&dir.join(&n), r, depth + 1)
            {
                return Some(found);
            }
        }
        None
    }
    let rest = encoded.strip_prefix('-')?;
    if rest.is_empty() {
        return None;
    }
    walk(Path::new("/"), rest, 0)
}

/// Path di dalam scratchpad Claude Code (`<tmp>/claude-<uid>/<proyek-terenkode>/<sesi>/...`):
/// kembalikan folder proyek asli yang dienkode di path itu.
fn claude_scratch_origin(p: &Path, temp_root: &Path) -> Option<PathBuf> {
    let rel = p.strip_prefix(temp_root).ok()?;
    let mut comps = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string());
    let mut first = comps.next()?;
    // `/var/folders/xx/yyyy/T/claude-501/...`: lewati segmen acak sampai `claude-*`.
    for _ in 0..4 {
        if first.starts_with("claude") {
            break;
        }
        first = comps.next()?;
    }
    if !first.starts_with("claude") {
        return None;
    }
    let encoded = comps.next()?;
    decode_claude_project_dir(&encoded).filter(|d| temp_root_of(d).is_none())
}

/// Deteksi proyek dari `cwd`. `None` bila `cwd` adalah home, root, folder konfigurasi agent,
/// atau folder sementara OS. Scratchpad Claude Code dipetakan ke proyek asal sesinya.
pub fn detect_project(cwd: &Path) -> Option<ProjectId> {
    let cwd = std::path::absolute(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    if let Some(tr) = temp_root_of(&cwd) {
        let origin = claude_scratch_origin(&cwd, &tr)?;
        return detect_project(&origin);
    }
    let mut dir: Option<&Path> = Some(cwd.as_path());
    while let Some(d) = dir {
        if is_ignored_dir(d) {
            break;
        }
        let git = d.join(".git");
        if git.is_dir() {
            return basename(d).map(|name| ProjectId { name, root: d.to_path_buf(), worktree: None, from_git: true });
        }
        if git.is_file() {
            // Worktree: gitdir menunjuk ke <repo>/.git/worktrees/<nama>.
            if let Some(gd) = gitdir_of(&git) {
                let s = gd.to_string_lossy().replace('\\', "/");
                if let Some(i) = s.find("/.git/worktrees/") {
                    let parent = PathBuf::from(&s[..i]);
                    if let Some(name) = basename(&parent) {
                        return Some(ProjectId { name, root: d.to_path_buf(), worktree: basename(d), from_git: true });
                    }
                }
                // Submodule (`.git/modules/<nama>`): perlakukan folder ini sebagai proyek sendiri.
            }
            return basename(d).map(|name| ProjectId { name, root: d.to_path_buf(), worktree: None, from_git: true });
        }
        dir = d.parent();
    }
    if is_ignored_dir(&cwd) {
        return None;
    }
    basename(&cwd).map(|name| ProjectId { name, root: cwd.clone(), worktree: None, from_git: false })
}

/// Kunci pembanding nama proyek: huruf kecil, pemisah path disamakan, spasi dibuang.
pub fn project_key(name: &str) -> String {
    name.trim().to_lowercase().replace('\\', "/").replace(' ', "")
}

/// Dua nama proyek merujuk hal yang sama? `BACKEND/api-x` cocok dengan `api-x`.
pub fn same_project(a: &str, b: &str) -> bool {
    let (ka, kb) = (project_key(a), project_key(b));
    if ka.is_empty() || kb.is_empty() {
        return false;
    }
    ka == kb || ka.ends_with(&format!("/{kb}")) || kb.ends_with(&format!("/{ka}"))
}

/// Varian nama yang layak dipakai sebagai filter `project IN [...]` di database:
/// nama asli, huruf kecil, dan segmen terakhir setelah `/`.
pub fn project_variants<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        let n = n.trim();
        if n.is_empty() {
            continue;
        }
        for v in [n.to_string(), n.to_lowercase(), n.rsplit('/').next().unwrap_or(n).to_string()] {
            if !v.is_empty() && !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out
}

/// Path relatif terhadap akar proyek bila berada di dalamnya; selain itu apa adanya.
pub fn relative_to_root(path: &str, root: Option<&Path>) -> String {
    if let Some(r) = root
        && let Ok(rel) = Path::new(path).strip_prefix(r)
    {
        return rel.to_string_lossy().replace('\\', "/");
    }
    path.replace('\\', "/")
}

/// Metadata Git ringan (branch & commit SHA pendek) yang dibaca langsung dari `.git`
/// tanpa membuat proses eksternal (`git`), sehingga aman dan instan (< 0.1 ms) di dalam hook.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GitInfo {
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
}

fn resolve_git_ref(git_dir: &Path, common_dir: &Path, ref_name: &str) -> Option<String> {
    for base in [git_dir, common_dir] {
        let ref_file = base.join(ref_name);
        if let Ok(s) = std::fs::read_to_string(&ref_file) {
            let sha = s.trim();
            if sha.len() >= 7 && sha.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(sha[..sha.len().min(12)].to_ascii_lowercase());
            }
        }
        let packed = base.join("packed-refs");
        if let Ok(s) = std::fs::read_to_string(&packed) {
            for line in s.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                    continue;
                }
                if let Some((sha, r)) = line.split_once(' ')
                    && r.trim() == ref_name
                    && sha.len() >= 7
                    && sha.chars().all(|c| c.is_ascii_hexdigit())
                {
                    return Some(sha[..sha.len().min(12)].to_ascii_lowercase());
                }
            }
        }
    }
    None
}

/// Baca branch aktif dan commit SHA pendek dari `.git` (mendukung repo biasa & git worktree).
pub fn detect_git_info(cwd: &Path) -> Option<GitInfo> {
    let cwd = std::path::absolute(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut dir: Option<&Path> = Some(cwd.as_path());
    while let Some(d) = dir {
        if is_ignored_dir(d) {
            break;
        }
        let git = d.join(".git");
        let (git_dir, common_dir) = if git.is_dir() {
            (git.clone(), git)
        } else if git.is_file() {
            let Some(gd) = gitdir_of(&git) else {
                dir = d.parent();
                continue;
            };
            let common = std::fs::read_to_string(gd.join("commondir"))
                .ok()
                .map(|s| {
                    let p = PathBuf::from(s.trim());
                    if p.is_relative() { gd.join(p) } else { p }
                })
                .or_else(|| gd.parent().and_then(|p| p.parent()).map(Path::to_path_buf))
                .unwrap_or_else(|| gd.clone());
            (gd, common)
        } else {
            dir = d.parent();
            continue;
        };

        let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
        let head = head.trim();
        if let Some(ref_name) = head.strip_prefix("ref:").map(str::trim) {
            let branch = ref_name
                .strip_prefix("refs/heads/")
                .unwrap_or(ref_name)
                .trim()
                .to_string();
            let commit = resolve_git_ref(&git_dir, &common_dir, ref_name);
            return Some(GitInfo {
                branch: (!branch.is_empty()).then_some(branch),
                commit,
            });
        } else if head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(GitInfo {
                branch: None,
                commit: Some(head[..head.len().min(12)].to_ascii_lowercase()),
            });
        }
        return None;
    }
    None
}

/// Periksa daftar path berkas relatif terhadap akar proyek `root`; kembalikan daftar
/// berkas yang sudah tidak ada di disk (dihapus/dipindah sejak memori direkam).
pub fn check_stale_files(root: &Path, files: &[String]) -> Vec<String> {
    if !root.is_dir() {
        return vec![];
    }
    let mut stale = Vec::new();
    for f in files {
        let f = f.trim().replace('\\', "/");
        if f.is_empty() || f.starts_with('/') || f.contains("..") {
            continue;
        }
        if !root.join(&f).exists() && !stale.contains(&f) {
            stale.push(f);
        }
    }
    stale
}

/// ID record metadata proyek yang deterministik dari (subjek, kunci proyek), sehingga
/// perangkat yang tersinkron menulis ke record yang sama, bukan duplikat.
pub fn project_meta_id(subject: &str, project: &str) -> String {
    // FNV-1a 64-bit dua kali dengan basis berbeda: cukup untuk kunci, bukan kriptografi.
    fn fnv(bytes: &[u8], basis: u64) -> u64 {
        bytes.iter().fold(basis, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
    }
    let input = format!("{}\u{0}{}", subject.trim(), project_key(project));
    format!("pm{:016x}{:016x}", fnv(input.as_bytes(), 0xcbf2_9ce4_8422_2325), fnv(input.as_bytes(), 0x6c62_272e_07bb_0142))
}

/// Target tautan bergaya Obsidian di `text`: `[[Nama]]`, `[[Nama|label]]`, `[[Nama#bagian]]`.
/// Urutan kemunculan dipertahankan, duplikat (kunci proyek sama) dibuang.
pub fn parse_wikilinks(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else { break };
        let inner = &after[..end];
        rest = &after[end + 2..];
        if inner.contains('\n') || inner.contains("[[") {
            continue;
        }
        let target = inner.split(['|', '#']).next().unwrap_or("").trim();
        if target.is_empty() || target.chars().count() > 120 {
            continue;
        }
        if !out.iter().any(|o| project_key(o) == project_key(target)) {
            out.push(target.to_string());
        }
    }
    out
}

/// Normalisasi satu tag: tanpa `#` di depan, spasi menjadi `-`, hanya huruf/angka dan `-_/.`.
pub fn normalize_tag(tag: &str) -> Option<String> {
    let t: String = tag
        .trim()
        .trim_start_matches('#')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '/' | '.'))
        .collect();
    let t = t.trim_matches(|c| c == '/' || c == '.').to_string();
    (!t.is_empty() && t.chars().count() <= 60).then_some(t)
}

/// Normalisasi daftar tag (juga menerima satu string berisi koma/spasi); duplikat
/// dibandingkan tanpa membedakan huruf besar-kecil.
pub fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in tags {
        for part in raw.split([',', ' ', '\n', '\t']) {
            if let Some(t) = normalize_tag(part)
                && !out.iter().any(|o| o.eq_ignore_ascii_case(&t))
            {
                out.push(t);
            }
        }
    }
    out
}

/// Folder `.git` (atau common dir untuk worktree) dari repo yang memuat `cwd`.
fn git_common_dir(cwd: &Path) -> Option<PathBuf> {
    let cwd = std::path::absolute(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut dir: Option<&Path> = Some(cwd.as_path());
    while let Some(d) = dir {
        if is_ignored_dir(d) {
            return None;
        }
        let git = d.join(".git");
        if git.is_dir() {
            return Some(git);
        }
        if git.is_file() {
            let gd = gitdir_of(&git)?;
            let common = std::fs::read_to_string(gd.join("commondir"))
                .ok()
                .map(|s| {
                    let p = PathBuf::from(s.trim());
                    if p.is_relative() { gd.join(p) } else { p }
                })
                .unwrap_or(gd);
            return Some(common);
        }
        dir = d.parent();
    }
    None
}

/// URL remote git (`origin` bila ada, selain itu remote pertama) dibaca langsung dari
/// `.git/config` tanpa proses eksternal. Kredensial di URL (`user:token@`) dibuang.
pub fn detect_git_remote(cwd: &Path) -> Option<String> {
    let cfg = std::fs::read_to_string(git_common_dir(cwd)?.join("config")).ok()?;
    let mut current: Option<String> = None;
    let mut first: Option<String> = None;
    for line in cfg.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            current = l
                .strip_prefix("[remote")
                .and_then(|r| r.trim().strip_prefix('"'))
                .and_then(|r| r.split('"').next())
                .map(str::to_string);
            continue;
        }
        let Some(name) = &current else { continue };
        if let Some((k, v)) = l.split_once('=')
            && k.trim() == "url"
        {
            let url = strip_url_credentials(v.trim());
            if url.is_empty() {
                continue;
            }
            if name == "origin" {
                return Some(url);
            }
            first.get_or_insert(url);
        }
    }
    first
}

fn strip_url_credentials(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://")
        && let Some((auth, host)) = rest.split_once('@')
        && !auth.contains('/')
    {
        return format!("{scheme}://{host}");
    }
    url.to_string()
}

/// Nama repositori ringkas dari URL remote: `git@github.com:org/repo.git` dan
/// `https://github.com/org/repo` menjadi `org/repo`.
pub fn repo_short_name(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    let u = u.strip_suffix(".git").unwrap_or(u);
    let path = if let Some((_, rest)) = u.split_once("://") {
        rest.split_once('/').map(|(_, p)| p).unwrap_or(rest)
    } else if let Some((_, p)) = u.split_once(':').filter(|(h, _)| h.contains('@') || h.contains('.')) {
        p
    } else {
        u
    };
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match segs.len() {
        0 => u.to_string(),
        1 => segs[0].to_string(),
        n => format!("{}/{}", segs[n - 2], segs[n - 1]),
    }
}

/// URL halaman web repositori (https) dari URL remote, bila bisa diturunkan.
pub fn repo_web_url(url: &str) -> Option<String> {
    let u = url.trim().trim_end_matches('/');
    let u = u.strip_suffix(".git").unwrap_or(u);
    if let Some(rest) = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://")) {
        return Some(format!("https://{rest}"));
    }
    if let Some(rest) = u.strip_prefix("ssh://") {
        let rest = rest.split_once('@').map(|(_, h)| h).unwrap_or(rest);
        let (host, path) = rest.split_once('/')?;
        let host = host.split(':').next().unwrap_or(host);
        return Some(format!("https://{host}/{path}"));
    }
    if let Some((left, path)) = u.split_once(':')
        && !left.contains('/')
    {
        let host = left.split_once('@').map(|(_, h)| h).unwrap_or(left);
        if host.contains('.') && !path.starts_with('/') {
            return Some(format!("https://{host}/{path}"));
        }
    }
    // Bentuk `github.com/org/repo` tanpa skema.
    let first = u.split('/').next().unwrap_or("");
    (first.contains('.') && u.matches('/').count() >= 2 && !u.contains(' ')).then(|| format!("https://{u}"))
}

/// Remote git proyek `name` dari daftar folder kerja sesi (`cwd`): folder pertama yang
/// terdeteksi sebagai repo bernama sama dan punya remote.
pub fn remote_for_project<'a>(name: &str, cwds: impl IntoIterator<Item = &'a str>) -> Option<String> {
    cwds.into_iter()
        .filter_map(|c| detect_project(Path::new(c)).filter(|p| p.from_git && same_project(&p.name, name)).map(|p| p.root))
        .find_map(|root| detect_git_remote(&root))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Direktori uji di luar folder temp OS (yang sengaja tidak dianggap proyek) dan di luar repo git.
    fn proj_tempdir() -> tempfile::TempDir {
        let base = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        tempfile::Builder::new().prefix(".engram-test-").tempdir_in(base).unwrap()
    }

    #[test]
    fn plain_folder_and_git_root() {
        let t = proj_tempdir();
        let repo = t.path().join("api-master-sensor");
        std::fs::create_dir_all(repo.join("src").join("deep")).unwrap();
        // Tanpa .git: pakai basename cwd.
        let p = detect_project(&repo.join("src").join("deep")).unwrap();
        assert_eq!(p.name, "deep");
        assert!(!p.from_git);
        std::fs::create_dir_all(repo.join(".git").join("refs").join("heads")).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(repo.join(".git/refs/heads/main"), "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4\n").unwrap();
        let p = detect_project(&repo.join("src").join("deep")).unwrap();
        assert_eq!(p.name, "api-master-sensor");
        assert!(p.from_git);
        assert_eq!(p.root, std::path::absolute(&repo).unwrap());
        assert_eq!(relative_to_root(&repo.join("src/main.rs").to_string_lossy(), Some(&p.root)), "src/main.rs");

        let g = detect_git_info(&repo.join("src").join("deep")).unwrap();
        assert_eq!(g.branch.as_deref(), Some("main"));
        assert_eq!(g.commit.as_deref(), Some("a1b2c3d4e5f6"));

        std::fs::write(repo.join("src/main.rs"), "fn main() {}").unwrap();
        let stale = check_stale_files(&repo, &["src/main.rs".into(), "src/deleted.rs".into()]);
        assert_eq!(stale, vec!["src/deleted.rs".to_string()]);
    }

    #[test]
    fn worktree_maps_to_parent_repo() {
        let t = proj_tempdir();
        let repo = t.path().join("engram");
        std::fs::create_dir_all(repo.join(".git").join("worktrees").join("fitur-x")).unwrap();
        let wt = t.path().join("engram-fitur-x");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", repo.join(".git/worktrees/fitur-x").display())).unwrap();
        let p = detect_project(&wt).unwrap();
        assert_eq!(p.name, "engram");
        assert_eq!(p.worktree.as_deref(), Some("engram-fitur-x"));
    }

    #[test]
    fn temp_dirs_and_claude_scratchpads() {
        // Folder temp OS bukan proyek (dulu tercatat sebagai proyek "T").
        assert!(detect_project(Path::new("/private/var/folders/lz/b92rvr6129s/T")).is_none());
        assert!(detect_project(Path::new("/tmp/acak")).is_none());

        // Scratchpad Claude: nama proyek terenkode diuraikan kembali ke folder asli di disk.
        let t = proj_tempdir();
        let repo = t.path().join("LLM-MEMORY").join("inference_docker");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(t.path().join("LLM")).unwrap(); // tetangga yang menyesatkan
        let abs = std::path::absolute(&repo).unwrap();
        let enc = claude_encode(&abs.to_string_lossy());
        assert_eq!(decode_claude_project_dir(&enc).as_deref(), Some(abs.as_path()));
        let scratch = PathBuf::from(format!("/private/tmp/claude-503/{enc}/55964e9e-5f11/scratchpad/glands/v4"));
        let p = detect_project(&scratch).expect("scratchpad dipetakan ke repo asal");
        assert_eq!(p.name, "inference_docker");
        assert!(p.from_git);
        assert!(detect_project(Path::new("/private/tmp/claude-503/-tidak-ada-di-disk/s/scratchpad")).is_none());
    }

    #[test]
    fn ignores_home_and_root() {
        assert!(detect_project(Path::new("/")).is_none());
        if let Some(h) = std::env::var_os("HOME") {
            assert!(detect_project(Path::new(&h)).is_none());
        }
    }

    #[test]
    fn keys_and_variants() {
        assert!(same_project("BACKEND/api-master-sensor", "api-master-sensor"));
        assert!(same_project("LLM-MEMORY", "llm-memory"));
        assert!(!same_project("api-a", "api-b"));
        let v = project_variants(["BACKEND/Api-X", "engram"]);
        assert!(v.contains(&"BACKEND/Api-X".to_string()) && v.contains(&"backend/api-x".to_string()) && v.contains(&"Api-X".to_string()) && v.contains(&"engram".to_string()));
    }

    #[test]
    fn wikilinks_tags_and_meta_id() {
        let l = parse_wikilinks("Pakai [[api-master-sensor]] dan [[TABULAR|tabel]], lihat [[api-master-sensor#auth]] serta [[ ]] [[rusak");
        assert_eq!(l, vec!["api-master-sensor".to_string(), "TABULAR".to_string()]);
        let t = normalize_tags(&["#OPERON, backend/api".into(), "operon".into(), "data warehouse".into(), "##".into()]);
        assert_eq!(t, vec!["OPERON".to_string(), "backend/api".to_string(), "data".to_string(), "warehouse".to_string()]);
        assert_eq!(normalize_tag("  #Data Warehouse "), Some("Data-Warehouse".into()));
        let a = project_meta_id("budi", "LLM-MEMORY");
        assert_eq!(a, project_meta_id("budi", "llm-memory"));
        assert_ne!(a, project_meta_id("budi", "LLM-MEMORY-2"));
        assert!(crate::ids::is_valid_id(&a));
    }

    #[test]
    fn repo_names_and_remote_detection() {
        assert_eq!(repo_short_name("git@github.com:org/repo.git"), "org/repo");
        assert_eq!(repo_short_name("https://github.com/org/sub/repo/"), "sub/repo");
        assert_eq!(repo_short_name("engram"), "engram");
        assert_eq!(repo_web_url("git@github.com:org/repo.git").as_deref(), Some("https://github.com/org/repo"));
        assert_eq!(repo_web_url("ssh://git@gitlab.com:2222/org/repo.git").as_deref(), Some("https://gitlab.com/org/repo"));
        assert_eq!(repo_web_url("github.com/org/repo").as_deref(), Some("https://github.com/org/repo"));
        assert_eq!(repo_web_url("engram"), None);

        let t = proj_tempdir();
        let repo = t.path().join("svc");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(
            repo.join(".git/config"),
            "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = https://github.com/up/svc.git\n[remote \"origin\"]\n\turl = https://user:tok@github.com/me/svc.git\n",
        )
        .unwrap();
        assert_eq!(detect_git_remote(&repo.join("src")).as_deref(), Some("https://github.com/me/svc.git"));
    }
}
