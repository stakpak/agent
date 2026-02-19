use ignore::WalkBuilder;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Discover all git repositories under $HOME (or common dev paths).
/// Returns a formatted string listing each repo with its language and remote.
pub fn discover(home: Option<&Path>) -> String {
    let search_roots = build_search_roots(home);
    if search_roots.is_empty() {
        return String::new();
    }

    let mut repos: Vec<RepoInfo> = Vec::new();

    for root in &search_roots {
        if !root.exists() {
            continue;
        }
        // Use ignore crate for fast traversal that respects .gitignore
        let walker = WalkBuilder::new(root)
            .hidden(false) // don't skip hidden dirs (we need .git)
            .git_ignore(false) // don't use gitignore for the walk itself
            .max_depth(Some(6))
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                // Skip known heavy dirs that never contain user repos
                !matches!(
                    name.as_ref(),
                    "node_modules"
                        | "vendor"
                        | "target"
                        | ".terraform"
                        | "venv"
                        | ".venv"
                        | "__pycache__"
                        | ".cache"
                        | ".Trash"
                        | "Library"
                        | ".local"
                        | ".cargo"
                        | ".rustup"
                        | ".npm"
                        | ".nvm"
                        | ".pyenv"
                        | ".gradle"
                        | ".m2"
                        | ".docker"
                        | ".kube"
                        | ".aws"
                )
            })
            .build();

        for entry in walker.flatten() {
            let path = entry.path();
            if path.file_name().map(|n| n == ".git").unwrap_or(false) && path.is_dir() {
                let repo_root = match path.parent() {
                    Some(p) => p,
                    None => continue,
                };
                let remote = get_remote(repo_root);
                let lang = detect_language(repo_root);
                let branch = get_branch(repo_root);
                repos.push(RepoInfo {
                    path: repo_root.to_path_buf(),
                    remote,
                    language: lang,
                    branch,
                });
            }
        }
    }

    if repos.is_empty() {
        return "(no git repositories found)\n".to_string();
    }

    repos.sort_by(|a, b| a.path.cmp(&b.path));
    repos.dedup_by(|a, b| a.path == b.path);

    let mut out = String::with_capacity(repos.len() * 120);
    for repo in &repos {
        let _ = writeln!(
            out,
            "- {}  [{}]  branch:{}  remote:{}",
            repo.path.display(),
            repo.language,
            repo.branch.as_deref().unwrap_or("?"),
            repo.remote.as_deref().unwrap_or("(none)"),
        );
    }
    out
}

struct RepoInfo {
    path: PathBuf,
    remote: Option<String>,
    language: String,
    branch: Option<String>,
}

fn build_search_roots(home: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(h) = home {
        roots.push(h.to_path_buf());
    }
    // Also check common non-home dev paths
    for extra in &["/opt", "/srv", "/var/www"] {
        let p = PathBuf::from(extra);
        if p.exists() {
            roots.push(p);
        }
    }
    roots
}

fn get_remote(repo_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if output.status.success() {
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !url.is_empty() {
            return Some(url);
        }
    }
    None
}

fn get_branch(repo_root: &Path) -> Option<String> {
    // Fast path: read HEAD file directly instead of spawning git
    let head_path = repo_root.join(".git/HEAD");
    if let Ok(content) = std::fs::read_to_string(&head_path) {
        let content = content.trim();
        if let Some(branch) = content.strip_prefix("ref: refs/heads/") {
            return Some(branch.to_string());
        }
        // Detached HEAD — return short hash
        return Some(content.chars().take(8).collect());
    }
    None
}

fn detect_language(repo_root: &Path) -> String {
    static MARKERS: &[(&str, &str)] = &[
        ("package.json", "Node.js"),
        ("go.mod", "Go"),
        ("Cargo.toml", "Rust"),
        ("pyproject.toml", "Python"),
        ("setup.py", "Python"),
        ("requirements.txt", "Python"),
        ("pom.xml", "Java"),
        ("build.gradle", "Java/Gradle"),
        ("build.gradle.kts", "Kotlin"),
        ("Gemfile", "Ruby"),
        ("composer.json", "PHP"),
        ("mix.exs", "Elixir"),
        ("pubspec.yaml", "Dart"),
        ("*.csproj", "C#"),
        ("*.sln", "C#"),
        ("CMakeLists.txt", "C/C++"),
        ("Makefile", "Make"),
    ];

    for (marker, lang) in MARKERS {
        if let Some(ext) = marker.strip_prefix('*') {
            // Glob pattern — check if any file matches
            // e.g. ".csproj"
            if let Ok(entries) = std::fs::read_dir(repo_root) {
                for entry in entries.flatten() {
                    if entry.file_name().to_string_lossy().ends_with(ext) {
                        return lang.to_string();
                    }
                }
            }
        } else if repo_root.join(marker).exists() {
            return lang.to_string();
        }
    }
    "unknown".to_string()
}
