use crate::commands::autopilot::{
    autopilot_service_installed, is_autopilot_running, start_autopilot_service,
    stop_autopilot_service,
};
use crate::utils::check_update::{get_latest_cli_version, is_newer_version};
use crate::utils::plugins::{PluginConfig, extract_tar_gz, extract_zip, get_download_info};
use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
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
        update_via_brew(&latest_version, silent, autopilot_was_running)
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

fn update_success_message(version: &str) -> String {
    format!(
        "✓ Updated to {}. Restart any long-running stakpak processes to pick up the new binary.",
        version
    )
}

pub fn restart_current_process() -> Result<(), String> {
    let executable = env::current_exe().map_err(|e| format!("Failed to get current exe: {}", e))?;
    let args: Vec<String> = env::args().skip(1).collect();
    restart_process(&executable, &args)
}

#[cfg(unix)]
fn restart_process(executable: &Path, args: &[String]) -> Result<(), String> {
    use std::os::unix::process::CommandExt;

    let err = Command::new(executable).args(args).exec();
    Err(format!("Failed to exec updated binary: {}", err))
}

#[cfg(windows)]
fn restart_process(executable: &Path, args: &[String]) -> Result<(), String> {
    Command::new(executable)
        .args(args)
        .spawn()
        .map_err(|e| format!("Failed to spawn updated binary: {}", e))?;
    std::process::exit(0);
}

fn update_via_brew(
    latest_version: &str,
    silent: bool,
    autopilot_was_running: bool,
) -> Result<(), String> {
    update_via_brew_with_command("brew", latest_version, silent, autopilot_was_running)
}

#[cfg(test)]
fn update_via_brew_with_path(
    brew_path: &str,
    latest_version: &str,
    silent: bool,
    autopilot_was_running: bool,
) -> Result<(), String> {
    update_via_brew_with_command(brew_path, latest_version, silent, autopilot_was_running)
}

fn update_via_brew_with_command(
    brew_command: &str,
    latest_version: &str,
    silent: bool,
    autopilot_was_running: bool,
) -> Result<(), String> {
    let update_status = Command::new(brew_command)
        .arg("update")
        .status()
        .map_err(|e| format!("Failed to run brew update: {}", e))?;
    if !update_status.success() {
        update_info!(silent, "brew update failed!");
    }

    let upgrade_status = Command::new(brew_command)
        .arg("upgrade")
        .arg("stakpak")
        .status()
        .map_err(|e| format!("Failed to run brew upgrade: {}", e))?;
    if !upgrade_status.success() {
        return Err("brew upgrade stakpak failed".to_string());
    }

    if autopilot_was_running {
        update_info!(silent, "Restarting autopilot service with new binary...");
        if let Err(e) = start_autopilot_service() {
            update_info!(silent, "⚠ Failed to restart autopilot service: {}", e);
        } else {
            update_info!(silent, "✓ Autopilot service restarted");
        }
    }

    update_info!(silent, "{}", update_success_message(latest_version));
    Ok(())
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

fn cleanup_downloaded_binary_update(temp_exe: &Path, extracted_binary_path: &Path) {
    fs::remove_file(temp_exe).ok();
    fs::remove_file(extracted_binary_path).ok();
}

fn apply_downloaded_binary_update(
    current_exe: &Path,
    extracted_binary_path: &Path,
    version: &str,
    silent: bool,
    autopilot_was_running: bool,
) -> Result<(), String> {
    let temp_exe = current_exe.with_extension("new");
    let backup_exe = current_exe.with_extension("backup");

    update_info!(silent, "Preparing new binary...");
    if let Err(e) = fs::copy(extracted_binary_path, &temp_exe) {
        cleanup_downloaded_binary_update(&temp_exe, extracted_binary_path);
        return Err(format!(
            "Failed to copy extracted binary to temp location: {}",
            e
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = match fs::metadata(&temp_exe) {
            Ok(metadata) => metadata.permissions(),
            Err(e) => {
                cleanup_downloaded_binary_update(&temp_exe, extracted_binary_path);
                return Err(format!("Failed to get temp file metadata: {}", e));
            }
        };
        perms.set_mode(0o755);
        if let Err(e) = fs::set_permissions(&temp_exe, perms) {
            cleanup_downloaded_binary_update(&temp_exe, extracted_binary_path);
            return Err(format!(
                "Failed to set executable permissions on temp file: {}",
                e
            ));
        }
    }

    update_info!(silent, "Verifying new binary...");
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
                    cleanup_downloaded_binary_update(&temp_exe, extracted_binary_path);
                    return Err(format!(
                        "Failed to run verification test on new binary: {}",
                        e
                    ));
                }
            }
        }
    };

    if !verification_success {
        cleanup_downloaded_binary_update(&temp_exe, extracted_binary_path);
        return Err("New binary failed all verification tests".to_string());
    }

    update_info!(silent, "Creating backup of current executable...");
    if let Err(e) = fs::copy(current_exe, &backup_exe) {
        cleanup_downloaded_binary_update(&temp_exe, extracted_binary_path);
        return Err(format!("Failed to create backup: {}", e));
    }

    update_info!(silent, "Performing atomic replacement...");
    match fs::rename(&temp_exe, current_exe) {
        Ok(()) => {
            update_info!(silent, "✅ Binary replacement successful!");
            fs::remove_file(&backup_exe).ok();
            fs::remove_file(extracted_binary_path).ok();
            update_info!(silent, "{}", update_success_message(version));

            if autopilot_was_running {
                update_info!(silent, "Restarting autopilot service with new binary...");
                if let Err(e) = start_autopilot_service() {
                    update_info!(silent, "⚠ Failed to restart autopilot service: {}", e);
                } else {
                    update_info!(silent, "✓ Autopilot service restarted");
                }
            }

            Ok(())
        }
        Err(e) => {
            update_info!(silent, "❌ Atomic replacement failed: {}", e);

            if backup_exe.exists() {
                update_info!(silent, "Attempting to restore backup...");
                match fs::copy(&backup_exe, current_exe) {
                    Ok(_) => {
                        update_info!(silent, "✅ Backup restored successfully");
                        fs::remove_file(&backup_exe).ok();
                    }
                    Err(restore_err) => {
                        update_info!(silent, "❌ Failed to restore backup: {}", restore_err);
                        fs::remove_file(&temp_exe).ok();
                        fs::remove_file(extracted_binary_path).ok();
                        return Err(format!(
                            "Critical error: Failed to replace executable AND failed to restore backup. Original error: {}, Restore error: {}",
                            e, restore_err
                        ));
                    }
                }
            }

            fs::remove_file(&temp_exe).ok();
            fs::remove_file(extracted_binary_path).ok();

            Err(format!("Failed to replace executable: {}", e))
        }
    }
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
    let base_url = "https://github.com/stakpak/agent";
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

    apply_downloaded_binary_update(
        &current_exe,
        Path::new(&extracted_binary_path),
        &version,
        silent,
        autopilot_was_running,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[cfg(unix)]
    fn write_executable_script(path: &std::path::Path, content: &str) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(path, content).expect("write script");
        let mut permissions = std::fs::metadata(path)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod script");
    }

    #[cfg(unix)]
    #[test]
    fn failed_binary_update_removes_downloaded_binary() {
        let temp_dir = TempDir::new().expect("temp dir");
        let missing_current_exe = temp_dir.path().join("missing-stakpak");
        let extracted_binary = temp_dir.path().join("stakpak_downloaded");

        write_executable_script(&extracted_binary, "#!/bin/sh\necho new-binary\n");

        let result = apply_downloaded_binary_update(
            &missing_current_exe,
            &extracted_binary,
            "v9.9.9",
            true,
            false,
        );

        assert!(result.is_err());
        assert!(
            !extracted_binary.exists(),
            "downloaded binary should be removed on update failure"
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_binary_update_returns_ok_without_exiting() {
        let temp_dir = TempDir::new().expect("temp dir");
        let current_exe = temp_dir.path().join("stakpak");
        let extracted_binary = temp_dir.path().join("stakpak_downloaded");
        let sentinel = temp_dir.path().join("still-alive");

        write_executable_script(&current_exe, "#!/bin/sh\necho old-binary\n");
        write_executable_script(&extracted_binary, "#!/bin/sh\necho new-binary\n");

        apply_downloaded_binary_update(&current_exe, &extracted_binary, "v9.9.9", true, false)
            .expect("update succeeds");

        std::fs::write(&sentinel, "alive").expect("write sentinel");
        assert!(sentinel.exists(), "test process should still be alive");

        let output = Command::new(&current_exe)
            .arg("--help")
            .output()
            .expect("run swapped binary");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("new-binary"));
    }

    #[cfg(unix)]
    #[test]
    fn brew_update_returns_ok_without_exiting() {
        let temp_dir = TempDir::new().expect("temp dir");
        let fake_bin = temp_dir.path().join("fake-bin");
        let brew_log = temp_dir.path().join("brew.log");
        std::fs::create_dir_all(&fake_bin).expect("create fake bin");

        write_executable_script(
            &fake_bin.join("brew"),
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
                brew_log.display()
            ),
        );

        let fake_brew = fake_bin.join("brew");

        let result = update_via_brew_with_path(&fake_brew.to_string_lossy(), "v9.9.9", true, false);

        assert!(result.is_ok());
        let log = std::fs::read_to_string(&brew_log).expect("read brew log");
        assert!(log.contains("update"));
        assert!(log.contains("upgrade stakpak"));
    }
}
