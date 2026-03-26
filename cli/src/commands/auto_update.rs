use crate::commands::autopilot::{
    autopilot_service_installed, is_autopilot_running, start_autopilot_service,
    stop_autopilot_service,
};
use crate::utils::check_update::{get_latest_cli_version, is_newer_version};
use crate::utils::plugins::{PluginConfig, extract_tar_gz, extract_zip, get_download_info};
use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Print an informational message to stdout, or stderr when `silent` is true.
/// When running in ACP mode, stdout is reserved for the JSON-RPC protocol
/// stream, so all informational output must go to stderr to avoid corruption.
macro_rules! update_info {
    ($silent:expr, $($arg:tt)*) => {
        if $silent {
            eprintln!($($arg)*);
        } else {
            println!($($arg)*);
        }
    };
}

/// Run the auto-update process.
///
/// When `silent` is true, all informational output is sent to stderr instead
/// of stdout. This is required when called from ACP mode where stdout carries
/// the JSON-RPC protocol stream.
#[allow(clippy::needless_return)]
pub async fn run_auto_update(silent: bool) -> Result<(), String> {
    // 0. Check if an update is actually needed
    let current_version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let latest_version = get_latest_cli_version()
        .await
        .map_err(|e| format!("Failed to fetch latest version: {}", e))?;

    if !is_newer_version(&current_version, &latest_version) {
        update_info!(silent, "✓ Already up to date ({})", current_version);
        return Ok(());
    }

    update_info!(silent, "Updating {} → {}", current_version, latest_version);

    // 1. Detect if autopilot is running before we replace the binary
    let autopilot_was_running = autopilot_service_installed() && (is_autopilot_running().is_some());

    if autopilot_was_running {
        update_info!(silent, "Stopping autopilot service before update...");
        if let Err(e) = stop_autopilot_service() {
            update_info!(silent, "⚠ Failed to stop autopilot service: {}", e);
            // Continue with update anyway — the service will pick up the new
            // binary on its next restart.
        } else {
            // Give the service a moment to fully stop
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            update_info!(silent, "✓ Autopilot service stopped");
        }
    }

    // 2. Check OS
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    // 3. Perform the update
    let update_result = if is_homebrew_installed()
        && is_stakpak_homebrew_install()
        && is_current_binary_homebrew_managed()?
    {
        update_info!(
            silent,
            "Detected current binary is managed by Homebrew. Updating via Homebrew..."
        );
        update_via_brew(silent, autopilot_was_running)
    } else {
        update_info!(
            silent,
            "Detected direct binary installation. Updating binary..."
        );
        update_binary_atomic(
            os,
            arch,
            Some(latest_version),
            silent,
            autopilot_was_running,
        )
        .await
    };

    // If the update failed and autopilot was running, restart it with the old binary
    if update_result.is_err() && autopilot_was_running {
        update_info!(
            silent,
            "Restarting autopilot service with previous binary..."
        );
        if let Err(e) = start_autopilot_service() {
            update_info!(silent, "⚠ Failed to restart autopilot service: {}", e);
        } else {
            update_info!(silent, "✓ Autopilot service restarted");
        }
    }

    update_result
}

fn is_homebrew_installed() -> bool {
    Command::new("which")
        .arg("brew")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn is_stakpak_homebrew_install() -> bool {
    match std::process::Command::new("brew")
        .arg("list")
        .arg("stakpak")
        .output()
    {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

fn update_via_brew(silent: bool, autopilot_was_running: bool) -> Result<(), String> {
    // update brew
    let update_status = Command::new("brew")
        .arg("update")
        .status()
        .map_err(|e| format!("Failed to run brew update: {}", e))?;
    if !update_status.success() {
        update_info!(silent, "brew update failed!");
    }

    let upgrade_status = Command::new("brew")
        .arg("upgrade")
        .arg("stakpak")
        .status()
        .map_err(|e| format!("Failed to run brew upgrade: {}", e))?;
    if upgrade_status.success() {
        // Restart autopilot with the new binary before exiting
        if autopilot_was_running {
            update_info!(silent, "Restarting autopilot service with new binary...");
            if let Err(e) = start_autopilot_service() {
                update_info!(silent, "⚠ Failed to restart autopilot service: {}", e);
            } else {
                update_info!(silent, "✓ Autopilot service restarted");
            }
        }
        update_info!(
            silent,
            "Update complete! Please restart the CLI to use the new version."
        );
        std::process::exit(0);
    } else {
        Err("brew upgrade stakpak failed".to_string())
    }
}

fn is_current_binary_homebrew_managed() -> Result<bool, String> {
    // Get current executable path
    let current_exe =
        env::current_exe().map_err(|e| format!("Failed to get current exe: {}", e))?;

    // Get Homebrew's stakpak path
    match Command::new("brew").arg("--prefix").arg("stakpak").output() {
        Ok(output) if output.status.success() => {
            let homebrew_path_lossy = String::from_utf8_lossy(&output.stdout);
            let homebrew_path = homebrew_path_lossy.trim();
            let homebrew_binary = std::path::Path::new(homebrew_path)
                .join("bin")
                .join("stakpak");

            // Compare canonical paths (resolves symlinks)
            match (current_exe.canonicalize(), homebrew_binary.canonicalize()) {
                (Ok(current_canonical), Ok(homebrew_canonical)) => {
                    Ok(current_canonical == homebrew_canonical)
                }
                _ => {
                    // If canonicalize fails, fall back to string comparison
                    Ok(current_exe == homebrew_binary)
                }
            }
        }
        Ok(_) => {
            // brew --prefix stakpak failed (stakpak not installed via brew)
            Ok(false)
        }
        Err(_) => {
            // brew command not found or failed
            Ok(false)
        }
    }
}

fn get_binary_dir() -> Result<(PathBuf, PathBuf), String> {
    let binary_path =
        env::current_exe().map_err(|e| format!("Failed to get current exe: {}", e))?;
    let binary_dir = match binary_path.parent() {
        Some(dir) => dir.to_path_buf(),
        None => Err("Failed to determine the directory of the current executable".to_string())?,
    };
    Ok((binary_path, binary_dir))
}

async fn download_and_extract_binary(
    config: &PluginConfig,
    silent: bool,
) -> Result<String, String> {
    // Determine the appropriate download URL based on OS and architecture
    let (download_url, _binary_name, is_zip) = get_download_info(config)?;

    let (_binary_path, binary_dir) = get_binary_dir()?;

    update_info!(silent, "Downloading {}...", config.name);

    // Download the archive
    let client = create_tls_client(TlsClientConfig::default())?;
    let response = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download {}: {}", config.name, e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to download {}: HTTP {}",
            config.name,
            response.status()
        ));
    }

    let archive_bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download response: {}", e))?;

    // Create a temporary directory for extraction
    let temp_dir = binary_dir.join("temp_update");
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir)
            .map_err(|e| format!("Failed to clean temp directory: {}", e))?;
    }
    fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp directory: {}", e))?;

    // Extract the archive to temp directory
    if is_zip {
        extract_zip(&archive_bytes, &temp_dir)?;
    } else {
        extract_tar_gz(&archive_bytes, &temp_dir)?;
    }

    // Find the extracted binary
    let extracted_binary = find_extracted_binary(&temp_dir, &config.name)?;

    // Copy to a permanent location before cleaning up temp_dir
    let permanent_extracted = binary_dir.join(format!("{}_downloaded", config.name));
    fs::copy(&extracted_binary, &permanent_extracted)
        .map_err(|e| format!("Failed to copy extracted binary: {}", e))?;

    // Clean up temp directory immediately
    fs::remove_dir_all(&temp_dir).map_err(|e| format!("Failed to clean temp directory: {}", e))?;

    Ok(permanent_extracted.to_string_lossy().to_string())
}

fn find_extracted_binary(temp_dir: &PathBuf, binary_name: &str) -> Result<PathBuf, String> {
    // Look for the binary in the temp directory and subdirectories
    let mut binary_path = None;

    // Check direct path
    let direct_path = temp_dir.join(binary_name);
    if direct_path.exists() {
        binary_path = Some(direct_path);
    }

    // Check with .exe extension on Windows
    #[cfg(windows)]
    {
        let exe_path = temp_dir.join(format!("{}.exe", binary_name));
        if exe_path.exists() {
            binary_path = Some(exe_path);
        }
    }

    // If not found, search recursively
    if binary_path.is_none() {
        binary_path = search_for_binary(temp_dir, binary_name)?;
    }

    binary_path.ok_or_else(|| {
        format!(
            "Could not find extracted binary '{}' in temp directory",
            binary_name
        )
    })
}

fn search_for_binary(dir: &PathBuf, binary_name: &str) -> Result<Option<PathBuf>, String> {
    let entries = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let path = entry.path();

        if path.is_file() {
            let file_name = path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("");

            if file_name == binary_name {
                return Ok(Some(path));
            }
        } else if path.is_dir()
            && let Ok(Some(found)) = search_for_binary(&path, binary_name)
        {
            return Ok(Some(found));
        }
    }

    Ok(None)
}

async fn update_binary_atomic(
    os: &str,
    arch: &str,
    version: Option<String>,
    silent: bool,
    autopilot_was_running: bool,
) -> Result<(), String> {
    update_info!(silent, "Starting atomic binary update for {} {}", os, arch);

    // 1. Set up PluginConfig for the CLI itself
    let cli_name = "stakpak";
    let base_url = "https://github.com/stakpak/agent/releases/download";
    let version = version.unwrap_or_default();

    // 2. Map OS/arch to plugin target
    let target = match (os, arch) {
        ("linux", "x86_64") => "linux-x86_64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("macos", "aarch64") => "darwin-aarch64",
        ("windows", "x86_64") => "windows-x86_64",
        _ => {
            return Err(format!("Unsupported platform: {} {}", os, arch));
        }
    };

    let config = PluginConfig {
        name: cli_name.to_string(),
        base_url: base_url.to_string(),
        targets: vec![target.to_string()],
        version: Some(version.clone()),
        repo: Some("agent".to_string()),
        owner: Some("stakpak".to_string()),
        version_arg: None,
        prefer_server_version: false,
    };

    // 3. Get current executable path
    let current_exe =
        env::current_exe().map_err(|e| format!("Failed to get current exe: {}", e))?;

    // 4. Create file paths for atomic update
    let temp_exe = current_exe.with_extension("new");
    let backup_exe = current_exe.with_extension("backup");

    // Clean up any existing temp files
    if temp_exe.exists() {
        fs::remove_file(&temp_exe)
            .map_err(|e| format!("Failed to clean existing temp file: {}", e))?;
    }
    if backup_exe.exists() {
        fs::remove_file(&backup_exe)
            .map_err(|e| format!("Failed to clean existing backup file: {}", e))?;
    }

    // 5. Download and extract new binary
    update_info!(silent, "Downloading new version {}...", version);
    let extracted_binary_path = download_and_extract_binary(&config, silent).await?;

    // 6. Copy extracted binary to temp location
    update_info!(silent, "Preparing new binary...");
    fs::copy(&extracted_binary_path, &temp_exe)
        .map_err(|e| format!("Failed to copy extracted binary to temp location: {}", e))?;

    // 7. Set executable permissions on temp file (Unix systems)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&temp_exe)
            .map_err(|e| format!("Failed to get temp file metadata: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&temp_exe, perms)
            .map_err(|e| format!("Failed to set executable permissions on temp file: {}", e))?;
    }

    // 8. Verify the new binary works - try multiple verification methods
    update_info!(silent, "Verifying new binary...");

    // First try --help (most binaries support this)
    let verification_result = Command::new(&temp_exe).arg("--help").output();

    let verification_success = match verification_result {
        Ok(output) if output.status.success() => {
            let help_output = String::from_utf8_lossy(&output.stdout);
            update_info!(silent, "✅ New binary verified successfully with --help!");
            update_info!(
                silent,
                "   Help output preview: {}",
                help_output.lines().take(2).collect::<Vec<_>>().join(" ")
            );
            true
        }
        Ok(_) | Err(_) => {
            // If --help fails, try running without arguments
            update_info!(silent, "--help failed, trying without arguments...");
            match Command::new(&temp_exe).output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    update_info!(silent, "✅ New binary verified successfully (no args)!");
                    update_info!(
                        silent,
                        "   Output preview: {}",
                        stdout
                            .lines()
                            .chain(stderr.lines())
                            .take(2)
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                    true
                }
                Err(e) => {
                    // Clean up and fail
                    fs::remove_file(&temp_exe).ok();
                    fs::remove_file(&extracted_binary_path).ok();
                    return Err(format!(
                        "Failed to run verification test on new binary: {}",
                        e
                    ));
                }
            }
        }
    };

    if !verification_success {
        // Clean up and fail
        fs::remove_file(&temp_exe).ok();
        fs::remove_file(&extracted_binary_path).ok();
        return Err("New binary failed all verification tests".to_string());
    }

    // 9. Create backup of current executable
    update_info!(silent, "Creating backup of current executable...");
    fs::copy(&current_exe, &backup_exe).map_err(|e| format!("Failed to create backup: {}", e))?;

    // 10. Atomic replacement using rename
    update_info!(silent, "Performing atomic replacement...");
    match fs::rename(&temp_exe, &current_exe) {
        Ok(()) => {
            update_info!(silent, "✅ Binary replacement successful!");

            // Clean up backup file
            fs::remove_file(&backup_exe).ok();

            // Clean up downloaded binary
            fs::remove_file(&extracted_binary_path).ok();

            update_info!(
                silent,
                "🎉 Update complete! Restarting with version {}...",
                version
            );

            // Restart autopilot service with the new binary
            if autopilot_was_running {
                update_info!(silent, "Restarting autopilot service with new binary...");
                if let Err(e) = start_autopilot_service() {
                    update_info!(silent, "⚠ Failed to restart autopilot service: {}", e);
                } else {
                    update_info!(silent, "✓ Autopilot service restarted");
                }
            }

            // Re-exec the new binary with the same arguments — but only when
            // the update was triggered implicitly (e.g. interactive auto-update
            // prompt at startup).  When the user ran `stakpak update` explicitly
            // there is nothing left to do, so we just exit cleanly.
            let args: Vec<String> = std::env::args().collect();
            let is_explicit_update = args.iter().any(|a| a == "update");

            if is_explicit_update {
                // Nothing more to do — the binary has been replaced.
                std::process::exit(0);
            }

            // This replaces the current process with the updated binary
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                // exec() replaces the current process - never returns on success
                let err = Command::new(&current_exe)
                    .args(&args[1..]) // Skip the program name, pass remaining args
                    .exec();
                // If we get here, exec failed
                eprintln!("Failed to exec new binary: {}", err);
                std::process::exit(1);
            }

            #[cfg(windows)]
            {
                // Windows doesn't have exec(), so spawn and exit
                match Command::new(&current_exe).args(&args[1..]).spawn() {
                    Ok(_) => std::process::exit(0),
                    Err(e) => {
                        eprintln!("Failed to spawn new binary: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Err(e) => {
            // Atomic rename failed, try to restore backup
            update_info!(silent, "❌ Atomic replacement failed: {}", e);

            if backup_exe.exists() {
                update_info!(silent, "Attempting to restore backup...");
                match fs::copy(&backup_exe, &current_exe) {
                    Ok(_) => {
                        update_info!(silent, "✅ Backup restored successfully");
                        fs::remove_file(&backup_exe).ok();
                    }
                    Err(restore_err) => {
                        update_info!(silent, "❌ Failed to restore backup: {}", restore_err);
                        // Clean up temp files
                        fs::remove_file(&temp_exe).ok();
                        fs::remove_file(&extracted_binary_path).ok();
                        return Err(format!(
                            "Critical error: Failed to replace executable AND failed to restore backup. Original error: {}, Restore error: {}",
                            e, restore_err
                        ));
                    }
                }
            }

            // Clean up temp file
            fs::remove_file(&temp_exe).ok();
            fs::remove_file(&extracted_binary_path).ok();

            Err(format!("Failed to replace executable: {}", e))
        }
    }
}
