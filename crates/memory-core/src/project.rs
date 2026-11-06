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

/// Deteksi proyek dari `cwd`. `None` bila `cwd` adalah home, root, atau folder konfigurasi agent.
pub fn detect_project(cwd: &Path) -> Option<ProjectId> {
    let cwd = std::path::absolute(cwd).unwrap_or_else(|_| cwd.to_path_buf());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_folder_and_git_root() {
        let t = tempfile::tempdir().unwrap();
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
        let t = tempfile::tempdir().unwrap();
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
}
