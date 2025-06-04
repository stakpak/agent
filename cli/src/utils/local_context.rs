use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalContext {
    pub operating_system: String,
    pub shell_type: String,
    pub is_container: bool,
    pub working_directory: String,
    pub file_structure: HashMap<String, FileInfo>,
    pub git_info: Option<GitInfo>,
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

impl fmt::Display for LocalContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "# System Details")?;
        writeln!(f, "Operating System: {}", self.operating_system)?;
        writeln!(f, "Shell Type: {}", self.shell_type)?;
        writeln!(
            f,
            "Running in Container Environment: {}",
            if self.is_container { "yes" } else { "no" }
        )?;

        // Display git information
        if let Some(git_info) = &self.git_info {
            if git_info.is_git_repo {
                writeln!(f, "Git Repository: yes")?;
                if let Some(branch) = &git_info.current_branch {
                    writeln!(f, "Current Branch: {}", branch)?;
                }
                if let Some(has_changes) = git_info.has_uncommitted_changes {
                    writeln!(
                        f,
                        "Uncommitted Changes: {}",
                        if has_changes { "yes" } else { "no" }
                    )?;
                } else {
                    writeln!(f, "Uncommitted Changes: no")?;
                }
                if let Some(remote) = &git_info.remote_url {
                    writeln!(f, "Remote URL: {}", remote)?;
                }
            } else {
                writeln!(f, "Git Repository: no")?;
            }
        }

        writeln!(
            f,
            "# Current Working Directory ({})",
            self.working_directory
        )?;
        if self.file_structure.is_empty() {
            writeln!(f, "(No files or directories found)")?;
        } else {
            // Sort entries for consistent output
            let mut entries: Vec<_> = self.file_structure.iter().collect();
            entries.sort_by_key(|(name, info)| (info.is_directory, name.to_lowercase()));

            // Display as tree structure like ls output
            for (i, (name, info)) in entries.iter().enumerate() {
                let is_last = i == entries.len() - 1;
                let prefix = if is_last { "└── " } else { "├── " };

                write!(f, "{}", prefix)?;

                if info.is_directory {
                    write!(f, "{}/", name)?;
                    if let Some(children) = &info.children {
                        if !children.is_empty() {
                            write!(f, " ({} items)", children.len())?;
                        } else {
                            write!(f, " (empty)")?;
                        }
                    }
                } else {
                    write!(f, "{}", name)?;
                    if let Some(size) = info.size {
                        write!(f, " ({})", format_file_size(size))?;
                    }
                }
                writeln!(f)?;
            }
        }

        Ok(())
    }
}

fn format_file_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size_f = size as f64;
    let mut unit_index = 0;

    while size_f >= 1024.0 && unit_index < UNITS.len() - 1 {
        size_f /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", size, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size_f, UNITS[unit_index])
    }
}

pub async fn analyze_local_context() -> Result<LocalContext, Box<dyn std::error::Error>> {
    let operating_system = get_operating_system();
    let shell_type = get_shell_type();
    let is_container = detect_container_environment();
    let working_directory = get_working_directory()?;
    let file_structure = get_file_structure(&working_directory)?;
    let git_info = Some(get_git_info(&working_directory));

    Ok(LocalContext {
        operating_system,
        shell_type,
        is_container,
        working_directory,
        file_structure,
        git_info,
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
            if let Ok(output) = Command::new("uname").arg("-s").output() {
                if output.status.success() {
                    let os_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !os_name.is_empty() {
                        return os_name;
                    }
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
            if let Ok(output) = Command::new("uname").arg("-s").output() {
                if output.status.success() {
                    let os_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !os_name.is_empty() {
                        return os_name;
                    }
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
    if let Ok(shell_path) = env::var("SHELL") {
        if let Some(shell_name) = Path::new(&shell_path).file_name() {
            if let Some(shell_str) = shell_name.to_str() {
                return shell_str.to_string();
            }
        }
    }

    // Detect OS at runtime to determine shell detection strategy
    let os = std::env::consts::OS;

    if os == "windows" {
        // On Windows, check for common shells
        if env::var("PSModulePath").is_ok() {
            "PowerShell".to_string()
        } else if env::var("COMSPEC").is_ok() {
            // Get the command processor name
            if let Ok(comspec) = env::var("COMSPEC") {
                if let Some(shell_name) = Path::new(&comspec).file_name() {
                    if let Some(shell_str) = shell_name.to_str() {
                        return shell_str.to_string();
                    }
                }
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
        {
            if output.status.success() {
                let ppid = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if let Ok(parent_output) = Command::new("ps")
                    .args(["-p", &ppid, "-o", "comm="])
                    .output()
                {
                    if parent_output.status.success() {
                        let parent_comm = String::from_utf8_lossy(&parent_output.stdout)
                            .trim()
                            .to_string();
                        if !parent_comm.is_empty() && parent_comm != "ps" {
                            return parent_comm;
                        }
                    }
                }
            }
        }

        // Fallback: try common shells using which command
        let common_shells = ["bash", "zsh", "fish", "sh", "tcsh", "csh"];
        for shell in &common_shells {
            if let Ok(output) = Command::new("which").arg(shell).output() {
                if output.status.success() {
                    return shell.to_string();
                }
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
        if let Ok(cgroup_content) = fs::read_to_string("/proc/1/cgroup") {
            if cgroup_content.contains("docker")
                || cgroup_content.contains("containerd")
                || cgroup_content.contains("podman")
            {
                return true;
            }
        }

        // Check for systemd container detection
        if let Ok(systemd_container) = env::var("container") {
            if !systemd_container.is_empty() {
                return true;
            }
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

fn get_git_info(dir_path: &str) -> GitInfo {
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
    {
        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !branch.is_empty() && branch != "HEAD" {
                git_info.current_branch = Some(branch);
            }
        }
    }

    // Check for uncommitted changes
    if let Ok(output) = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
    {
        if output.status.success() {
            let status_output = String::from_utf8_lossy(&output.stdout);
            git_info.has_uncommitted_changes = Some(!status_output.trim().is_empty());
        }
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
        {
            if output.status.success() {
                let remotes = String::from_utf8_lossy(&output.stdout);
                if let Some(first_remote) = remotes.lines().next() {
                    if let Ok(url_output) = Command::new("git")
                        .args(["remote", "get-url", first_remote])
                        .current_dir(path)
                        .output()
                    {
                        if url_output.status.success() {
                            let remote_url = String::from_utf8_lossy(&url_output.stdout)
                                .trim()
                                .to_string();
                            if !remote_url.is_empty() {
                                git_info.remote_url = Some(remote_url);
                            }
                        }
                    }
                }
            }
        }
    }

    git_info
}
