use std::process::{Command, Stdio};

const TAB_BINARY: &str = if cfg!(windows) {
    "agent-tab.exe"
} else {
    "agent-tab"
};
const DAEMON_BINARY: &str = if cfg!(windows) {
    "agent-tab-daemon.exe"
} else {
    "agent-tab-daemon"
};

/// Pass-through to agent-tab plugin. All args after 'tab' are forwarded directly.
/// Run `stakpak tab --help` for available commands.
pub async fn run_tab(args: Vec<String>) -> Result<(), String> {
    let tab_path = get_tab_plugin_path().await;
    let mut cmd = Command::new(&tab_path);
    cmd.args(&args);

    cmd.stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::inherit());

    let status = cmd
        .status()
        .map_err(|e| format!("Failed to run agent-tab: {}", e))?;

    std::process::exit(status.code().unwrap_or(1));
}

// ============================================================================
// Path and platform utilities
// ============================================================================

fn get_home_dir() -> Result<String, String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "HOME/USERPROFILE environment variable not set".to_string())
}

fn get_plugins_dir() -> Result<std::path::PathBuf, String> {
    let home_dir = get_home_dir()?;
    Ok(std::path::PathBuf::from(&home_dir)
        .join(".stakpak")
        .join("plugins"))
}

fn get_platform_suffix() -> Result<(&'static str, &'static str), String> {
    let platform = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        os => return Err(format!("Unsupported OS: {}", os)),
    };

    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        arch => return Err(format!("Unsupported architecture: {}", arch)),
    };

    Ok((platform, arch))
}

fn get_existing_path(binary_name: &str) -> Result<String, String> {
    let binary_path = get_plugins_dir()?.join(binary_name);
    if binary_path.exists() {
        Ok(binary_path.to_string_lossy().to_string())
    } else {
        Err(format!("{} not found", binary_name))
    }
}

fn is_version_match(current: &str, target: &str) -> bool {
    let current_clean = current.strip_prefix('v').unwrap_or(current);
    let target_clean = target.strip_prefix('v').unwrap_or(target);
    current_clean == target_clean
}

// ============================================================================
// Version checking
// ============================================================================

fn get_tab_version(path: &str) -> Result<String, String> {
    let output = std::process::Command::new(path)
        .arg("version")
        .output()
        .map_err(|e| format!("Failed to run agent-tab version: {}", e))?;

    if !output.status.success() {
        return Err("agent-tab version command failed".to_string());
    }

    let version_output = String::from_utf8_lossy(&output.stdout);
    let trimmed = version_output.trim();

    // Parse version from output like "agent-tab v0.1.0" or just "v0.1.0"
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

fn get_daemon_version(path: &str) -> Result<String, String> {
    let output = std::process::Command::new(path)
        .arg("--version")
        .output()
        .map_err(|e| format!("Failed to run agent-tab-daemon: {}", e))?;

    if !output.status.success() {
        return Err("agent-tab-daemon version command failed".to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn get_latest_github_release_version() -> Result<String, String> {
    use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};

    let client = create_tls_client(TlsClientConfig::default())?;

    let response = client
        .get("https://api.github.com/repos/stakpak/tab/releases")
        .header("User-Agent", "stakpak-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch releases: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API returned: {}", response.status()));
    }

    let releases: Vec<serde_json::Value> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse GitHub response: {}", e))?;

    for release in releases {
        if let Some(tag_name) = release["tag_name"].as_str() {
            if let Some(version) = tag_name.strip_prefix('v') {
                return Ok(version.to_string());
            }
        }
    }

    Err("No release found with prefix 'v'".to_string())
}

// ============================================================================
// Download and extraction
// ============================================================================

async fn download_and_extract(
    artifact_prefix: &str,
    binary_name: &str,
    version: Option<&str>,
) -> Result<String, String> {
    use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};

    let plugins_dir = get_plugins_dir()?;
    std::fs::create_dir_all(&plugins_dir)
        .map_err(|e| format!("Failed to create plugins directory: {}", e))?;

    let (platform, arch) = get_platform_suffix()?;
    let artifact_name = format!("{}-{}-{}", artifact_prefix, platform, arch);
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };

    let download_url = match version {
        Some(v) => format!(
            "https://github.com/stakpak/tab/releases/download/v{}/{}.{}",
            v, artifact_name, extension
        ),
        None => format!(
            "https://github.com/stakpak/tab/releases/latest/download/{}.{}",
            artifact_name, extension
        ),
    };

    eprintln!("{}", download_url);
    println!("Downloading {} binary...", artifact_prefix);

    let client = create_tls_client(TlsClientConfig::default())?;
    let response = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download {}: {}", artifact_prefix, e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed: HTTP {}", response.status()));
    }

    let archive_bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download: {}", e))?;

    let binary_path = plugins_dir.join(binary_name);

    if cfg!(windows) {
        extract_zip(&archive_bytes, &plugins_dir, binary_name)?;
    } else {
        extract_tar_gz(&archive_bytes, &plugins_dir, binary_name)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&binary_path)
            .map_err(|e| format!("Failed to get binary metadata: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&binary_path, perms)
            .map_err(|e| format!("Failed to set binary permissions: {}", e))?;
    }

    Ok(binary_path.to_string_lossy().to_string())
}

fn extract_tar_gz(
    data: &[u8],
    dest_dir: &std::path::Path,
    binary_name: &str,
) -> Result<(), String> {
    use flate2::read::GzDecoder;
    use std::io::Cursor;
    use tar::Archive;

    let cursor = Cursor::new(data);
    let tar = GzDecoder::new(cursor);
    let mut archive = Archive::new(tar);

    for entry in archive
        .entries()
        .map_err(|e| format!("Failed to read archive: {}", e))?
    {
        let mut entry = entry.map_err(|e| format!("Failed to read archive entry: {}", e))?;
        let path = entry
            .path()
            .map_err(|e| format!("Failed to get entry path: {}", e))?;

        if let Some(file_name) = path.file_name() {
            if file_name == binary_name {
                let dest_path = dest_dir.join(file_name);
                entry
                    .unpack(&dest_path)
                    .map_err(|e| format!("Failed to extract binary: {}", e))?;
                return Ok(());
            }
        }
    }

    Err("Binary not found in archive".to_string())
}

#[cfg(windows)]
fn extract_zip(data: &[u8], dest_dir: &std::path::Path, binary_name: &str) -> Result<(), String> {
    use std::io::Cursor;
    use zip::ZipArchive;

    let cursor = Cursor::new(data);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| format!("Failed to read zip archive: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry: {}", e))?;

        if file.name().ends_with(binary_name) {
            let dest_path = dest_dir.join(binary_name);
            let mut outfile = std::fs::File::create(&dest_path)
                .map_err(|e| format!("Failed to create output file: {}", e))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| format!("Failed to write binary: {}", e))?;
            return Ok(());
        }
    }

    Err("Binary not found in archive".to_string())
}

#[cfg(not(windows))]
fn extract_zip(
    _data: &[u8],
    _dest_dir: &std::path::Path,
    _binary_name: &str,
) -> Result<(), String> {
    Err("ZIP extraction not supported on this platform".to_string())
}

// ============================================================================
// Daemon management
// ============================================================================

async fn ensure_daemon_downloaded(version: Option<&str>) {
    if let Ok(existing_path) = get_existing_path(DAEMON_BINARY) {
        if let Some(target) = version {
            if let Ok(current) = get_daemon_version(&existing_path) {
                if is_version_match(&current, target) {
                    return;
                }
                println!(
                    "agent-tab-daemon {} is outdated (target: {}), updating...",
                    current, target
                );
            }
        } else {
            return; // No version to check, binary exists
        }
    }

    match download_and_extract("agent-tab-daemon", DAEMON_BINARY, version).await {
        Ok(path) => {
            let version_str = version.map(|v| format!("{} ", v)).unwrap_or_default();
            println!(
                "Successfully installed agent-tab-daemon {}-> {}",
                version_str, path
            );
        }
        Err(e) => eprintln!("Failed to download agent-tab-daemon: {}", e),
    }
}

// ============================================================================
// Main plugin path resolution
// ============================================================================

async fn get_tab_plugin_path() -> String {
    let existing = get_existing_path(TAB_BINARY).ok();
    let current_version = existing.as_ref().and_then(|p| get_tab_version(p).ok());

    if let Some(ref path) = existing {
        match get_latest_github_release_version().await {
            Ok(target_version) => {
                if let Some(ref current) = current_version {
                    if is_version_match(current, &target_version) {
                        ensure_daemon_downloaded(Some(&target_version)).await;
                        return path.clone();
                    }
                    println!(
                        "agent-tab {} is outdated (target: {}), updating...",
                        current, target_version
                    );
                }

                match download_and_extract("agent-tab", TAB_BINARY, Some(&target_version)).await {
                    Ok(new_path) => {
                        println!(
                            "Successfully installed agent-tab {} -> {}",
                            target_version, new_path
                        );
                        ensure_daemon_downloaded(Some(&target_version)).await;
                        return new_path;
                    }
                    Err(e) => {
                        eprintln!("Failed to update agent-tab: {}", e);
                        eprintln!("Using existing version");
                        ensure_daemon_downloaded(Some(&target_version)).await;
                        return path.clone();
                    }
                }
            }
            Err(_) => {
                ensure_daemon_downloaded(None).await;
                return path.clone();
            }
        }
    }

    // No existing installation - must download
    match get_latest_github_release_version().await {
        Ok(target_version) => {
            match download_and_extract("agent-tab", TAB_BINARY, Some(&target_version)).await {
                Ok(path) => {
                    println!(
                        "Successfully installed agent-tab {} -> {}",
                        target_version, path
                    );
                    ensure_daemon_downloaded(Some(&target_version)).await;
                    path
                }
                Err(e) => {
                    eprintln!("Failed to download agent-tab: {}", e);
                    "agent-tab".to_string()
                }
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to check version: {}", e);
            match download_and_extract("agent-tab", TAB_BINARY, None).await {
                Ok(path) => {
                    println!("Successfully installed agent-tab -> {}", path);
                    ensure_daemon_downloaded(None).await;
                    path
                }
                Err(e) => {
                    eprintln!("Failed to download agent-tab: {}", e);
                    "agent-tab".to_string()
                }
            }
        }
    }
}
