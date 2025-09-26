use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stakpak_shared::utils::{LocalFileSystemProvider, generate_directory_tree};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::config::AppConfig;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalContext {
    pub machine_name: String,
    pub operating_system: String,
    pub shell_type: String,
    pub is_container: bool,
    pub working_directory: String,
    pub file_structure: HashMap<String, FileInfo>,
    pub git_info: Option<GitInfo>,
    pub current_datetime_utc: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileInfo {
    pub is_directory: bool,
    pub size: Option<u64>,
    pub children: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GitInfo {
    pub is_git_repo: bool,
    pub current_branch: Option<String>,
    pub has_uncommitted_changes: Option<bool>,
    pub remote_url: Option<String>,
}

impl LocalContext {
    pub async fn format_display(&self) -> Result<String, Box<dyn std::error::Error>> {
        let mut result = String::new();

        result.push_str("# System Details\n\n");
        result.push_str(&format!("Machine Name: {}\n", self.machine_name));
        result.push_str(&format!(
            "Current Date/Time: {}\n",
            self.current_datetime_utc.format("%Y-%m-%d %H:%M:%S UTC")
        ));
        result.push_str(&format!("Operating System: {}\n", self.operating_system));
        result.push_str(&format!("Shell Type: {}\n", self.shell_type));
        result.push_str(&format!(
            "Running in Container Environment: {}\n",
            if self.is_container { "yes" } else { "no" }
        ));

        // Display git information
        if let Some(git_info) = &self.git_info {
            if git_info.is_git_repo {
                result.push_str("Git Repository: yes\n");
                if let Some(branch) = &git_info.current_branch {
                    result.push_str(&format!("Current Branch: {}\n", branch));
                }
                if let Some(has_changes) = git_info.has_uncommitted_changes {
                    result.push_str(&format!(
                        "Uncommitted Changes: {}\n",
                        if has_changes { "yes" } else { "no" }
                    ));
                } else {
                    result.push_str("Uncommitted Changes: no\n");
                }
                if let Some(remote) = &git_info.remote_url {
                    result.push_str(&format!("Remote URL: {}\n", remote));
                }
            } else {
                result.push_str("Git Repository: no\n");
            }
        }

        result.push_str(&format!(
            "\n# Current Working Directory ({})\n\n",
            self.working_directory
        ));

        // Use the shared directory tree function for better structure visualization
        let provider = LocalFileSystemProvider;
        match generate_directory_tree(&provider, &self.working_directory, "", 1, 0).await {
            Ok(tree_content) => {
                if tree_content.trim().is_empty() {
                    result.push_str("(No files or directories found)\n");
                } else {
                    result.push_str(&tree_content);
                }
            }
            Err(_) => {
                result.push_str("(No files or directories found)\n");
            }
        }

        Ok(result)
    }
}

pub async fn analyze_local_context(
    config: &AppConfig,
) -> Result<LocalContext, Box<dyn std::error::Error>> {
    let current_datetime_utc = Utc::now();
    let operating_system = get_operating_system();
    let shell_type = get_shell_type();
    let is_container = detect_container_environment();
    let working_directory = get_working_directory()?;
    let file_structure = get_file_structure(&working_directory)?;
    let git_info = Some(get_git_info(&working_directory));

    Ok(LocalContext {
        machine_name: config
            .machine_name
            .clone()
            .unwrap_or("unknown-machine".to_string()),
        operating_system,
        shell_type,
        is_container,
        working_directory,
        file_structure,
        git_info,
        current_datetime_utc,
    })
}

fn get_operating_system() -> String {
    // Try to detect OS using runtime methods

    // First, try using std::env::consts::OS
    let os = std::env::consts::OS;
    match os {
        "windows" => "Windows".to_string(),
        "macos" => "macOS".to_string(),
        "linux" => {
            // For Linux, try to get more specific distribution info
            if let Ok(os_release) = fs::read_to_string("/etc/os-release") {
                // Parse the PRETTY_NAME or NAME field
                for line in os_release.lines() {
                    if line.starts_with("PRETTY_NAME=") {
                        let name = line.trim_start_matches("PRETTY_NAME=").trim_matches('"');
                        return name.to_string();
                    }
                }
                // Fallback to NAME field
                for line in os_release.lines() {
                    if line.starts_with("NAME=") {
                        let name = line.trim_start_matches("NAME=").trim_matches('"');
                        return name.to_string();
                    }
                }
            }
            // If we can't read os-release, try other methods
            if let Ok(output) = Command::new("uname").arg("-s").output()
                && output.status.success()
            {
                let os_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !os_name.is_empty() {
                    return os_name;
                }
            }
            "Linux".to_string()
        }
        "freebsd" => "FreeBSD".to_string(),
        "openbsd" => "OpenBSD".to_string(),
        "netbsd" => "NetBSD".to_string(),
        #[allow(clippy::unwrap_used)]
        _ => {
            // Fallback: try using uname command for Unix-like systems
            if let Ok(output) = Command::new("uname").arg("-s").output()
                && output.status.success()
            {
                let os_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !os_name.is_empty() {
                    return os_name;
                }
            }

            // Last resort: return the const value capitalized

            os.chars()
                .next()
                .unwrap()
                .to_uppercase()
                .collect::<String>()
                + &os[1..]
        }
    }
}

fn get_shell_type() -> String {
    // First try to get from SHELL environment variable
    if let Ok(shell_path) = env::var("SHELL")
        && let Some(shell_name) = Path::new(&shell_path).file_name()
        && let Some(shell_str) = shell_name.to_str()
    {
        return shell_str.to_string();
    }

    // Detect OS at runtime to determine shell detection strategy
    let os = std::env::consts::OS;

    if os == "windows" {
        // On Windows, check for common shells
        if env::var("PSModulePath").is_ok() {
            "PowerShell".to_string()
        } else if env::var("COMSPEC").is_ok() {
            // Get the command processor name
            if let Ok(comspec) = env::var("COMSPEC")
                && let Some(shell_name) = Path::new(&comspec).file_name()
                && let Some(shell_str) = shell_name.to_str()
            {
                return shell_str.to_string();
            }
            "cmd".to_string()
        } else {
            "cmd".to_string()
        }
    } else {
        // On Unix-like systems, try to detect current shell

        // Try to get parent process shell
        let current_pid = std::process::id().to_string();
        if let Ok(output) = Command::new("ps")
            .args(["-p", &current_pid, "-o", "ppid="])
            .output()
            && output.status.success()
        {
            let ppid = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Ok(parent_output) = Command::new("ps")
                .args(["-p", &ppid, "-o", "comm="])
                .output()
                && parent_output.status.success()
            {
                let parent_comm = String::from_utf8_lossy(&parent_output.stdout)
                    .trim()
                    .to_string();
                if !parent_comm.is_empty() && parent_comm != "ps" {
                    return parent_comm;
                }
            }
        }

        // Fallback: try common shells using which command
        let common_shells = ["bash", "zsh", "fish", "sh", "tcsh", "csh"];
        for shell in &common_shells {
            if let Ok(output) = Command::new("which").arg(shell).output()
                && output.status.success()
            {
                return shell.to_string();
            }
        }

        "Unknown".to_string()
    }
}

pub fn detect_container_environment() -> bool {
    // Check for common container indicators

    // Check for /.dockerenv file (Docker)
    if Path::new("/.dockerenv").exists() {
        return true;
    }

    // Check for container environment variables
    let container_env_vars = [
        "DOCKER_CONTAINER",
        "KUBERNETES_SERVICE_HOST",
        "container",
        "PODMAN_VERSION",
    ];

    for var in &container_env_vars {
        if env::var(var).is_ok() {
            return true;
        }
    }

    // Check cgroup for container indicators (Linux and other Unix-like systems)
    let os = std::env::consts::OS;
    if os == "linux" || os == "freebsd" || os == "openbsd" || os == "netbsd" {
        if let Ok(cgroup_content) = fs::read_to_string("/proc/1/cgroup")
            && (cgroup_content.contains("docker")
                || cgroup_content.contains("containerd")
                || cgroup_content.contains("podman"))
        {
            return true;
        }

        // Check for systemd container detection
        if let Ok(systemd_container) = env::var("container")
            && !systemd_container.is_empty()
        {
            return true;
        }
    }

    false
}

fn get_working_directory() -> Result<String, Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    Ok(cwd.to_string_lossy().to_string())
}

fn get_file_structure(
    dir_path: &str,
) -> Result<HashMap<String, FileInfo>, Box<dyn std::error::Error>> {
    let mut file_structure = HashMap::new();
    let path = Path::new(dir_path);

    if !path.exists() {
        return Ok(file_structure);
    }

    // Read the current directory
    let entries = fs::read_dir(path)?;

    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        let file_path = entry.path();
        let metadata = entry.metadata()?;

        let is_directory = metadata.is_dir();
        let size = if is_directory {
            None
        } else {
            Some(metadata.len())
        };

        // For directories, get immediate children (non-recursive to avoid deep trees)
        let children = if is_directory {
            match fs::read_dir(&file_path) {
                Ok(dir_entries) => {
                    let child_names: Result<Vec<String>, _> = dir_entries
                        .map(|entry| entry.map(|e| e.file_name().to_string_lossy().to_string()))
                        .collect();
                    child_names.ok()
                }
                Err(_) => None,
            }
        } else {
            None
        };

        file_structure.insert(
            file_name,
            FileInfo {
                is_directory,
                size,
                children,
            },
        );
    }

    Ok(file_structure)
}

pub fn get_git_info(dir_path: &str) -> GitInfo {
    let path = Path::new(dir_path);

    // Check if .git directory exists
    let git_dir = path.join(".git");
    if !git_dir.exists() {
        return GitInfo {
            is_git_repo: false,
            current_branch: None,
            has_uncommitted_changes: None,
            remote_url: None,
        };
    }

    let mut git_info = GitInfo {
        is_git_repo: true,
        current_branch: None,
        has_uncommitted_changes: None,
        remote_url: None,
    };

    // Get current branch
    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(path)
        .output()
        && output.status.success()
    {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !branch.is_empty() && branch != "HEAD" {
            git_info.current_branch = Some(branch);
        }
    }

    // Check for uncommitted changes
    if let Ok(output) = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        && output.status.success()
    {
        let status_output = String::from_utf8_lossy(&output.stdout);
        git_info.has_uncommitted_changes = Some(!status_output.trim().is_empty());
    }

    // Get remote URL (try origin first, then any remote)
    if let Ok(output) = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(path)
        .output()
    {
        if output.status.success() {
            let remote_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !remote_url.is_empty() {
                git_info.remote_url = Some(remote_url);
            }
        }
    } else {
        // If origin doesn't exist, try to get any remote
        if let Ok(output) = Command::new("git")
            .args(["remote"])
            .current_dir(path)
            .output()
            && output.status.success()
        {
            let remotes = String::from_utf8_lossy(&output.stdout);
            if let Some(first_remote) = remotes.lines().next()
                && let Ok(url_output) = Command::new("git")
                    .args(["remote", "get-url", first_remote])
                    .current_dir(path)
                    .output()
                && url_output.status.success()
            {
                let remote_url = String::from_utf8_lossy(&url_output.stdout)
                    .trim()
                    .to_string();
                if !remote_url.is_empty() {
                    git_info.remote_url = Some(remote_url);
                }
            }
        }
    }

    git_info
}
