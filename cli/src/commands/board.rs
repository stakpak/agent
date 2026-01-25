use std::process::{Command, Stdio};

/// Pass-through to agent-board plugin. All args after 'board' are forwarded directly.
/// Run `stakpak board --help` for available commands.
pub async fn run_board(args: Vec<String>) -> Result<(), String> {
    let board_path = get_board_plugin_path().await;
    let mut cmd = Command::new(board_path);
    cmd.args(&args);
    execute_board_command(cmd)
}

async fn get_board_plugin_path() -> String {
    // Check if we have an existing installation first
    let existing = get_existing_board_path().ok();
    let current_version = existing
        .as_ref()
        .and_then(|path| get_board_version(path).ok());

    // If we have an existing installation, check if update needed
    if let Some(ref path) = existing {
        // Try to get latest version from GitHub API
        match get_latest_github_release_version().await {
            Ok(target_version) => {
                if let Some(ref current) = current_version {
                    if is_version_match(current, &target_version) {
                        // Already up to date, use existing
                        return path.clone();
                    }
                    println!(
                        "agent-board {} is outdated (target: {}), updating...",
                        current, target_version
                    );
                }
                // Need to update - download new version
                match download_board_plugin().await {
                    Ok(new_path) => {
                        println!(
                            "Successfully installed agent-board {} -> {}",
                            target_version, new_path
                        );
                        return new_path;
                    }
                    Err(e) => {
                        eprintln!("Failed to update agent-board: {}", e);
                        eprintln!("Using existing version");
                        return path.clone();
                    }
                }
            }
            Err(_) => {
                // Can't check version, use existing installation
                return path.clone();
            }
        }
    }

    // No existing installation - must download
    match get_latest_github_release_version().await {
        Ok(target_version) => match download_board_plugin().await {
            Ok(path) => {
                println!(
                    "Successfully installed agent-board {} -> {}",
                    target_version, path
                );
                path
            }
            Err(e) => {
                eprintln!("Failed to download agent-board: {}", e);
                "agent-board".to_string()
            }
        },
        Err(e) => {
            // Try download anyway (uses /latest/ URL)
            eprintln!("Warning: Failed to check version: {}", e);
            match download_board_plugin().await {
                Ok(path) => {
                    println!("Successfully installed agent-board -> {}", path);
                    path
                }
                Err(e) => {
                    eprintln!("Failed to download agent-board: {}", e);
                    "agent-board".to_string()
                }
            }
        }
    }
}

async fn get_latest_github_release_version() -> Result<String, String> {
    use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};

    let client = create_tls_client(TlsClientConfig::default())?;
    let response = client
        .get("https://api.github.com/repos/stakpak/agent-board/releases/latest")
        .header("User-Agent", "stakpak-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch latest release: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API returned: {}", response.status()));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse GitHub response: {}", e))?;

    json["tag_name"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No tag_name in release".to_string())
}

fn get_existing_board_path() -> Result<String, String> {
    let home_dir =
        std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;

    let plugin_path = std::path::PathBuf::from(&home_dir)
        .join(".stakpak")
        .join("plugins")
        .join("agent-board");

    if plugin_path.exists() {
        Ok(plugin_path.to_string_lossy().to_string())
    } else {
        Err("agent-board not found in plugins directory".to_string())
    }
}

fn get_board_version(path: &str) -> Result<String, String> {
    let output = std::process::Command::new(path)
        .arg("version")
        .output()
        .map_err(|e| format!("Failed to run agent-board version: {}", e))?;

    if !output.status.success() {
        return Err("agent-board version command failed".to_string());
    }

    let version_output = String::from_utf8_lossy(&output.stdout);
    // Parse version from output like "agent-board v0.1.6" or just "v0.1.6"
    let trimmed = version_output.trim();
    if let Some(v) = trimmed.split_whitespace().find(|s| {
        s.starts_with('v')
            || s.chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
    }) {
        Ok(v.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn is_version_match(current: &str, target: &str) -> bool {
    let current_clean = current.strip_prefix('v').unwrap_or(current);
    let target_clean = target.strip_prefix('v').unwrap_or(target);
    current_clean == target_clean
}

async fn download_board_plugin() -> Result<String, String> {
    use flate2::read::GzDecoder;
    use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};
    use std::io::Cursor;
    use tar::Archive;

    let home_dir =
        std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;

    let plugins_dir = std::path::PathBuf::from(&home_dir)
        .join(".stakpak")
        .join("plugins");

    std::fs::create_dir_all(&plugins_dir)
        .map_err(|e| format!("Failed to create plugins directory: {}", e))?;

    // Determine platform
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let target = match (os, arch) {
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("macos", "aarch64") => "darwin-aarch64",
        _ => return Err(format!("Unsupported platform: {} {}", os, arch)),
    };

    let download_url = format!(
        "https://github.com/stakpak/agent-board/releases/latest/download/agent-board-{}.tar.gz",
        target
    );

    println!("Downloading agent-board plugin...");

    let client = create_tls_client(TlsClientConfig::default())?;
    let response = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download agent-board: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed: HTTP {}", response.status()));
    }

    let archive_bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download: {}", e))?;

    // Extract tar.gz
    let cursor = Cursor::new(archive_bytes.as_ref());
    let tar = GzDecoder::new(cursor);
    let mut archive = Archive::new(tar);

    archive
        .unpack(&plugins_dir)
        .map_err(|e| format!("Failed to extract archive: {}", e))?;

    let plugin_path = plugins_dir.join("agent-board");

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&plugin_path)
            .map_err(|e| format!("Failed to get file metadata: {}", e))?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&plugin_path, permissions)
            .map_err(|e| format!("Failed to set executable permissions: {}", e))?;
    }

    Ok(plugin_path.to_string_lossy().to_string())
}

fn execute_board_command(mut cmd: Command) -> Result<(), String> {
    // Pass through stdio directly - no buffering needed
    cmd.stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::inherit());

    let status = cmd
        .status()
        .map_err(|e| format!("Failed to run agent-board: {}", e))?;

    // Exit with the same code as the plugin
    std::process::exit(status.code().unwrap_or(1));
}
