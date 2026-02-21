use std::fs;
use std::path::{Path, PathBuf};

/// Maximum number of parent directories to traverse when searching for project
/// files (AGENTS.md, APPS.md). 5 levels covers most monorepo nesting depths
/// without accidentally picking up unrelated files from distant ancestors.
const MAX_TRAVERSAL_DEPTH: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextPriority {
    Critical = 0,
    High = 1,
    Normal = 2,
    CallerSupplied = 3,
}

#[derive(Debug, Clone)]
pub struct ContextFile {
    pub name: String,
    pub path: String,
    pub content: String,
    /// Character count at construction time, before any budget truncation.
    /// Used for telemetry and logging to track how much content was trimmed.
    pub original_size: usize,
    pub truncated: bool,
    pub priority: ContextPriority,
}

impl ContextFile {
    pub fn new(
        name: impl Into<String>,
        path: impl Into<String>,
        content: impl Into<String>,
        priority: ContextPriority,
    ) -> Self {
        let content = content.into();
        Self {
            name: name.into(),
            path: path.into(),
            original_size: content.chars().count(),
            content,
            truncated: false,
            priority,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProjectContext {
    pub files: Vec<ContextFile>,
}

impl ProjectContext {
    pub fn discover(start_dir: &Path) -> Self {
        let mut files = Vec::new();

        if let Some(file) = discover_agents_md(start_dir) {
            files.push(file);
        }

        if let Some(file) = discover_apps_md(start_dir) {
            files.push(file);
        }

        Self { files }
    }

    pub fn with_caller_context(mut self, caller_files: Vec<ContextFile>) -> Self {
        self.files.extend(caller_files);
        self
    }
}

fn discover_agents_md(start_dir: &Path) -> Option<ContextFile> {
    let discovered = discover_nearest_file(start_dir, &["AGENTS.md", "agents.md"])?;

    Some(ContextFile::new(
        "AGENTS.md",
        discovered.path.display().to_string(),
        discovered.content,
        ContextPriority::Critical,
    ))
}

/// Discover APPS.md with a global fallback at `~/.stakpak/APPS.md`.
///
/// Unlike AGENTS.md (which is always project-specific), APPS.md can describe
/// globally-managed applications and infrastructure, so a user-level fallback
/// is supported when no project-local file is found.
fn discover_apps_md(start_dir: &Path) -> Option<ContextFile> {
    if let Some(discovered) = discover_nearest_file(start_dir, &["APPS.md", "apps.md"]) {
        return Some(ContextFile::new(
            "APPS.md",
            discovered.path.display().to_string(),
            discovered.content,
            ContextPriority::High,
        ));
    }

    // Global fallback: ~/.stakpak/APPS.md
    let home = dirs::home_dir()?;
    let global_apps = home.join(".stakpak").join("APPS.md");
    let content = fs::read_to_string(&global_apps).ok()?;

    let path = canonical_or_original(&global_apps);
    Some(ContextFile::new(
        "APPS.md",
        path.display().to_string(),
        content,
        ContextPriority::High,
    ))
}

struct DiscoveredFile {
    path: PathBuf,
    content: String,
}

fn discover_nearest_file(start_dir: &Path, file_names: &[&str]) -> Option<DiscoveredFile> {
    let mut current = start_dir.to_path_buf();

    for _ in 0..=MAX_TRAVERSAL_DEPTH {
        for file_name in file_names {
            let candidate = current.join(file_name);
            if !candidate.exists() {
                continue;
            }

            let content = match fs::read_to_string(&candidate) {
                Ok(content) => content,
                Err(_) => continue,
            };

            return Some(DiscoveredFile {
                path: canonical_or_original(&candidate),
                content,
            });
        }

        if !current.pop() {
            break;
        }
    }

    None
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_nearest_agents_file() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let root_agents = temp.path().join("AGENTS.md");
        std::fs::write(&root_agents, "root").expect("write root agents");

        let nested = temp.path().join("a").join("b");
        std::fs::create_dir_all(&nested).expect("create nested");

        let nested_agents = nested.join("AGENTS.md");
        std::fs::write(&nested_agents, "nested").expect("write nested agents");

        let context = ProjectContext::discover(&nested);
        let agents = context.files.iter().find(|file| file.name == "AGENTS.md");

        assert!(agents.is_some());
        assert!(
            agents
                .map(|file| file.content.contains("nested"))
                .unwrap_or(false)
        );
    }

    #[test]
    fn discovers_apps_file() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let apps = temp.path().join("APPS.md");
        std::fs::write(&apps, "apps data").expect("write apps");

        let context = ProjectContext::discover(temp.path());
        let apps_file = context.files.iter().find(|file| file.name == "APPS.md");

        assert!(apps_file.is_some());
        assert!(
            apps_file
                .map(|file| file.content.contains("apps data"))
                .unwrap_or(false)
        );
    }

    #[test]
    fn caller_context_is_appended() {
        let context = ProjectContext::default().with_caller_context(vec![ContextFile::new(
            "gateway_delivery",
            "/tmp/context.txt",
            "hello",
            ContextPriority::CallerSupplied,
        )]);

        assert_eq!(context.files.len(), 1);
        assert_eq!(context.files[0].name, "gateway_delivery");
    }

    #[test]
    fn discovers_agents_md_from_parent_directory() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let root_agents = temp.path().join("AGENTS.md");
        std::fs::write(&root_agents, "root config").expect("write root agents");

        let nested = temp.path().join("src").join("lib");
        std::fs::create_dir_all(&nested).expect("create nested");

        // No AGENTS.md in nested, should find root
        let context = ProjectContext::discover(&nested);
        let agents = context.files.iter().find(|file| file.name == "AGENTS.md");

        assert!(agents.is_some(), "should discover AGENTS.md from ancestor");
        assert!(
            agents
                .map(|file| file.content.contains("root config"))
                .unwrap_or(false)
        );
    }

    #[test]
    fn prefers_nearest_agents_md() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let root_agents = temp.path().join("AGENTS.md");
        std::fs::write(&root_agents, "root").expect("write root");

        let nested = temp.path().join("sub");
        std::fs::create_dir_all(&nested).expect("create nested");
        let nested_agents = nested.join("AGENTS.md");
        std::fs::write(&nested_agents, "nested").expect("write nested");

        let context = ProjectContext::discover(&nested);
        let agents = context.files.iter().find(|file| file.name == "AGENTS.md");

        assert!(
            agents.map(|file| file.content == "nested").unwrap_or(false),
            "should prefer the nearest AGENTS.md"
        );
    }

    #[test]
    fn empty_directory_discovers_nothing() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let context = ProjectContext::discover(temp.path());
        // May or may not find global APPS.md from home dir — that's OK
        let agents = context.files.iter().find(|file| file.name == "AGENTS.md");
        assert!(agents.is_none(), "empty dir should not have AGENTS.md");
    }

    #[test]
    fn context_file_tracks_original_size() {
        let content = "x".repeat(500);
        let file = ContextFile::new("test", "/test", content.clone(), ContextPriority::Normal);

        assert_eq!(file.original_size, 500);
        assert!(!file.truncated);
    }

    #[test]
    fn caller_context_appended_after_discovered_files() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let agents = temp.path().join("AGENTS.md");
        std::fs::write(&agents, "project config").expect("write agents");

        let context =
            ProjectContext::discover(temp.path()).with_caller_context(vec![ContextFile::new(
                "watch_result",
                "caller://watch_result",
                "health ok",
                ContextPriority::CallerSupplied,
            )]);

        assert!(context.files.len() >= 2, "should have agents + caller file");
        assert_eq!(
            context.files.last().map(|file| file.name.as_str()),
            Some("watch_result"),
            "caller context should come after discovered files"
        );
    }
}
