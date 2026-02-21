use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stakpak_shared::utils::{LocalFileSystemProvider, generate_directory_tree};
use std::env;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitContext {
    pub branch: Option<String>,
    pub has_uncommitted_changes: Option<bool>,
    pub remote_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentContext {
    pub machine_name: String,
    pub operating_system: String,
    pub shell_type: String,
    pub is_container: bool,
    pub working_directory: String,
    pub current_datetime_utc: DateTime<Utc>,
    pub directory_tree: String,
    pub git: Option<GitContext>,
}

impl EnvironmentContext {
    pub async fn snapshot(working_directory: &str) -> Self {
        let provider = LocalFileSystemProvider;
        let directory_tree = generate_directory_tree(&provider, working_directory, "", 1, 0)
            .await
            .ok()
            .filter(|tree| !tree.trim().is_empty())
            .unwrap_or_else(|| "(No files or directories found)".to_string());

        let wd = working_directory.to_string();
        let git = tokio::task::spawn_blocking(move || detect_git_context(&wd))
            .await
            .ok()
            .flatten();

        // Hostname detection can touch filesystem/process APIs; keep it off the
        // async runtime worker threads.
        let machine_name = tokio::task::spawn_blocking(detect_machine_name)
            .await
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "unknown-machine".to_string());

        Self {
            machine_name,
            operating_system: detect_operating_system(),
            shell_type: detect_shell_type(),
            is_container: detect_container_environment(),
            working_directory: working_directory.to_string(),
            current_datetime_utc: Utc::now(),
            directory_tree,
            git,
        }
    }

    pub fn to_local_context_block(&self) -> String {
        let mut block = String::new();

        block.push_str("# System Details\n\n");
        block.push_str(&format!("Machine Name: {}\n", self.machine_name));
        block.push_str(&format!(
            "Current Date/Time: {}\n",
            self.current_datetime_utc.format("%Y-%m-%d %H:%M:%S UTC")
        ));
        block.push_str(&format!("Operating System: {}\n", self.operating_system));
        block.push_str(&format!("Shell Type: {}\n", self.shell_type));
        block.push_str(&format!(
            "Running in Container Environment: {}\n",
            if self.is_container { "yes" } else { "no" }
        ));

        if let Some(git) = &self.git {
            block.push_str("Git Repository: yes\n");
            if let Some(branch) = &git.branch {
                block.push_str(&format!("Current Branch: {}\n", branch));
            }
            if let Some(has_changes) = git.has_uncommitted_changes {
                block.push_str(&format!(
                    "Uncommitted Changes: {}\n",
                    if has_changes { "yes" } else { "no" }
                ));
            }
            if let Some(remote_url) = &git.remote_url {
                block.push_str(&format!("Remote URL: {}\n", remote_url));
            }
        } else {
            block.push_str("Git Repository: no\n");
        }

        block.push_str(&format!(
            "\n# Current Working Directory ({})\n\n{}",
            self.working_directory, self.directory_tree
        ));

        block
    }
}

fn detect_machine_name() -> String {
    env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("COMPUTERNAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(platform_hostname)
        .unwrap_or_else(|| "unknown-machine".to_string())
}

/// Platform-native hostname fallback when env vars are not set.
#[cfg(unix)]
fn platform_hostname() -> Option<String> {
    // Try /etc/hostname first (common on Linux), then fall back to POSIX uname.
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::process::Command::new("uname")
                .arg("-n")
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

#[cfg(not(unix))]
fn platform_hostname() -> Option<String> {
    None
}

fn detect_operating_system() -> String {
    match std::env::consts::OS {
        "windows" => "Windows".to_string(),
        "macos" => "macOS".to_string(),
        "linux" => "Linux".to_string(),
        "freebsd" => "FreeBSD".to_string(),
        "openbsd" => "OpenBSD".to_string(),
        "netbsd" => "NetBSD".to_string(),
        value => value.to_string(),
    }
}

fn detect_shell_type() -> String {
    env::var("SHELL")
        .ok()
        .and_then(|path| {
            Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .or_else(|| env::var("COMSPEC").ok())
        .unwrap_or_else(|| "Unknown".to_string())
}

fn detect_container_environment() -> bool {
    if Path::new("/.dockerenv").exists() {
        return true;
    }

    [
        "DOCKER_CONTAINER",
        "KUBERNETES_SERVICE_HOST",
        "container",
        "PODMAN_VERSION",
    ]
    .iter()
    .any(|key| env::var(key).is_ok())
}

fn detect_git_context(working_directory: &str) -> Option<GitContext> {
    let path = Path::new(working_directory);

    let is_git_repo = run_git(path, ["rev-parse", "--is-inside-work-tree"])
        .map(|output| output.trim() == "true")
        .unwrap_or(false);
    if !is_git_repo {
        return None;
    }

    let branch = run_git(path, ["rev-parse", "--abbrev-ref", "HEAD"]);
    let has_uncommitted_changes = run_git(path, ["status", "--porcelain"]).map(|output| {
        let trimmed = output.trim();
        !trimmed.is_empty()
    });

    let remote_url = run_git(path, ["remote", "get-url", "origin"]).or_else(|| {
        let remotes = run_git(path, ["remote"])?;
        let first_remote = remotes.lines().next()?.trim();
        if first_remote.is_empty() {
            return None;
        }
        run_git(path, ["remote", "get-url", first_remote])
    });

    Some(GitContext {
        branch,
        has_uncommitted_changes,
        remote_url,
    })
}

fn run_git<const N: usize>(working_directory: &Path, args: [&str; N]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(working_directory)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return None;
    }

    Some(stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn builds_local_context_block() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let context = EnvironmentContext::snapshot(temp.path().to_string_lossy().as_ref()).await;
        let block = context.to_local_context_block();

        assert!(block.contains("# System Details"));
        assert!(block.contains("# Current Working Directory"));
    }

    #[tokio::test]
    async fn snapshot_populates_all_fields() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let context = EnvironmentContext::snapshot(temp.path().to_string_lossy().as_ref()).await;

        assert!(!context.machine_name.is_empty());
        assert!(!context.operating_system.is_empty());
        assert!(!context.shell_type.is_empty());
        assert!(!context.working_directory.is_empty());
    }

    #[test]
    fn no_git_context_for_non_repo_directory() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let git = detect_git_context(temp.path().to_string_lossy().as_ref());
        assert!(git.is_none(), "non-repo dir should have no git context");
    }

    #[test]
    fn detects_git_context_for_repo() {
        let temp = tempfile::TempDir::new().expect("temp dir");

        // Initialize a git repo with an initial commit so HEAD exists
        let init = Command::new("git")
            .args(["init"])
            .current_dir(temp.path())
            .output();

        if init.is_err() || !init.as_ref().map(|o| o.status.success()).unwrap_or(false) {
            // git not available in test env — skip
            return;
        }

        // Configure git user for the commit
        let _ = Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(temp.path())
            .output();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(temp.path())
            .output();

        // Create an initial commit so rev-parse HEAD works
        std::fs::write(temp.path().join("README.md"), "init").expect("write readme");
        let _ = Command::new("git")
            .args(["add", "."])
            .current_dir(temp.path())
            .output();
        let commit = Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(temp.path())
            .output();

        if commit.is_err() || !commit.as_ref().map(|o| o.status.success()).unwrap_or(false) {
            // commit failed — skip
            return;
        }

        let git = detect_git_context(temp.path().to_string_lossy().as_ref());
        assert!(git.is_some(), "initialized repo should have git context");

        let git = git.expect("git context");
        assert!(git.branch.is_some(), "should detect branch after commit");
    }

    #[test]
    fn detects_git_context_from_nested_directory() {
        let temp = tempfile::TempDir::new().expect("temp dir");

        let init = Command::new("git")
            .args(["init"])
            .current_dir(temp.path())
            .output();

        if init.is_err() || !init.as_ref().map(|o| o.status.success()).unwrap_or(false) {
            return;
        }

        let _ = Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(temp.path())
            .output();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(temp.path())
            .output();

        std::fs::write(temp.path().join("README.md"), "init").expect("write readme");
        let _ = Command::new("git")
            .args(["add", "."])
            .current_dir(temp.path())
            .output();
        let commit = Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(temp.path())
            .output();

        if commit.is_err() || !commit.as_ref().map(|o| o.status.success()).unwrap_or(false) {
            return;
        }

        let nested = temp.path().join("src").join("module");
        std::fs::create_dir_all(&nested).expect("create nested");

        let git = detect_git_context(nested.to_string_lossy().as_ref());
        assert!(
            git.is_some(),
            "nested path inside repo should still detect git context"
        );
    }

    #[test]
    fn local_context_block_includes_git_info() {
        let context = EnvironmentContext {
            machine_name: "test".to_string(),
            operating_system: "Linux".to_string(),
            shell_type: "bash".to_string(),
            is_container: false,
            working_directory: "/tmp".to_string(),
            current_datetime_utc: Utc::now(),
            directory_tree: "├── src".to_string(),
            git: Some(GitContext {
                branch: Some("main".to_string()),
                has_uncommitted_changes: Some(true),
                remote_url: Some("https://github.com/org/repo".to_string()),
            }),
        };

        let block = context.to_local_context_block();
        assert!(block.contains("Git Repository: yes"));
        assert!(block.contains("Current Branch: main"));
        assert!(block.contains("Uncommitted Changes: yes"));
        assert!(block.contains("Remote URL: https://github.com/org/repo"));
    }

    #[test]
    fn local_context_block_no_git() {
        let context = EnvironmentContext {
            machine_name: "test".to_string(),
            operating_system: "macOS".to_string(),
            shell_type: "zsh".to_string(),
            is_container: true,
            working_directory: "/app".to_string(),
            current_datetime_utc: Utc::now(),
            directory_tree: "├── Dockerfile".to_string(),
            git: None,
        };

        let block = context.to_local_context_block();
        assert!(block.contains("Git Repository: no"));
        assert!(block.contains("Running in Container Environment: yes"));
    }
}
