use crate::utils::check_update::get_latest_cli_version;
use crate::utils::plugins::{PluginConfig, get_plugin_path};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[allow(clippy::needless_return)]
pub async fn run_auto_update() -> Result<(), String> {
    // 1. Check OS
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    // 2. If macOS, check for Homebrew
    if os == "macos" {
        if is_homebrew_installed() && is_stakpak_homebrew_install() {
            println!("Detected Homebrew installation. Updating via Homebrew...");
            return update_via_brew();
        } else {
            println!("Detected direct binary installation on macOS. Updating binary...");
            let version = get_latest_cli_version().await.unwrap_or_default();
            return update_via_plugin_system(os, arch, Some(version)).await;
        }
    } else {
        // 3. Other OS: use plugin system
        println!("Detected {}. Updating binary via plugin system...", os);
        let version = get_latest_cli_version().await.unwrap_or_default();
        return update_via_plugin_system(os, arch, Some(version)).await;
    }
}

fn is_homebrew_installed() -> bool {
    std::process::Command::new("brew")
        .arg("--version")
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

fn update_via_brew() -> Result<(), String> {
    let tap_status = Command::new("brew")
        .arg("tap")
        .arg("stakpak/stakpak")
        .status()
        .map_err(|e| format!("Failed to run brew tap: {}", e))?;
    if !tap_status.success() {
        println!("brew tap failed!");
    }

    let upgrade_status = Command::new("brew")
        .arg("upgrade")
        .arg("stakpak")
        .status()
        .map_err(|e| format!("Failed to run brew upgrade: {}", e))?;
    if upgrade_status.success() {
        println!("Update complete! Please restart the CLI to use the new version.");
        std::process::exit(0);
    } else {
        println!("brew upgrade failed!");
        std::process::exit(1);
    }
}

async fn update_via_plugin_system(
    os: &str,
    arch: &str,
    version: Option<String>,
) -> Result<(), String> {
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
        _ => return Err(format!("Unsupported platform: {} {}", os, arch)),
    };

    let config = PluginConfig {
        name: cli_name.to_string(),
        base_url: base_url.to_string(),
        targets: vec![target.to_string()],
        version: Some(version.clone()),
    };

    // 3. Download and install the latest CLI binary using the plugin system
    let new_bin_path = get_plugin_path(config).await;

    // 4. Replace the current executable with the new one
    let current_exe =
        env::current_exe().map_err(|e| format!("Failed to get current exe: {}", e))?;
    fs::copy(&new_bin_path, &current_exe)
        .map_err(|e| format!("Failed to replace binary: {}", e))?;

    // 5. Set executable permissions (Unix)
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&current_exe)
            .map_err(|e| format!("Failed to get file metadata: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&current_exe, perms)
            .map_err(|e| format!("Failed to set permissions: {}", e))?;
    }

    println!("CLI updated successfully! Please restart the CLI to use the new version.");
    Ok(())
}
