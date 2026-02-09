//! macOS launchd service installation.
//!
//! Installs the stakpak watch service as a LaunchAgent that runs on user login.
//! The plist is installed to ~/Library/LaunchAgents/dev.stakpak.watch.plist

use super::{InstallResult, ReloadResult, UninstallResult, get_stakpak_binary_path};
use std::path::PathBuf;
use std::process::Command;

/// Service identifier for launchd.
const SERVICE_LABEL: &str = "dev.stakpak.watch";

/// Get the path to the LaunchAgent plist file.
pub fn get_plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", SERVICE_LABEL))
}

/// Check if the service is installed.
pub fn is_installed() -> bool {
    get_plist_path().exists()
}

/// Check if the service is currently loaded.
fn is_loaded() -> bool {
    let output = Command::new("launchctl")
        .args(["list", SERVICE_LABEL])
        .output();

    match output {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

/// Generate the launchd plist content.
fn generate_plist(binary_path: &std::path::Path) -> String {
    let home_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .display()
        .to_string();

    let log_dir = format!("{}/.stakpak/watch/logs", home_dir);

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>

    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>watch</string>
        <string>run</string>
    </array>

    <key>RunAtLoad</key>
    <true/>

    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>

    <key>StandardOutPath</key>
    <string>{}/stdout.log</string>

    <key>StandardErrorPath</key>
    <string>{}/stderr.log</string>

    <key>WorkingDirectory</key>
    <string>{}</string>

    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>{}</string>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
    </dict>

    <key>ProcessType</key>
    <string>Background</string>

    <key>ThrottleInterval</key>
    <integer>10</integer>
</dict>
</plist>
"#,
        SERVICE_LABEL,
        binary_path.display(),
        log_dir,
        log_dir,
        home_dir,
        home_dir,
    )
}

/// Install the launchd service.
pub async fn install() -> Result<InstallResult, String> {
    let binary_path = get_stakpak_binary_path()?;
    let plist_path = get_plist_path();

    // Ensure the binary exists and is executable
    if !binary_path.exists() {
        return Err(format!(
            "Stakpak binary not found at: {}",
            binary_path.display()
        ));
    }

    // Ensure LaunchAgents directory exists
    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create LaunchAgents directory: {}", e))?;
    }

    // Ensure log directory exists
    let log_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".stakpak")
        .join("watch")
        .join("logs");
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| format!("Failed to create log directory: {}", e))?;

    // Unload existing service if loaded
    if is_loaded() {
        let _ = Command::new("launchctl")
            .args(["unload", plist_path.to_str().unwrap_or("")])
            .output();
    }

    // Generate and write plist
    let plist_content = generate_plist(&binary_path);
    std::fs::write(&plist_path, &plist_content)
        .map_err(|e| format!("Failed to write plist file: {}", e))?;

    // Load the service
    let load_output = Command::new("launchctl")
        .args(["load", plist_path.to_str().unwrap_or("")])
        .output()
        .map_err(|e| format!("Failed to run launchctl load: {}", e))?;

    if !load_output.status.success() {
        let stderr = String::from_utf8_lossy(&load_output.stderr);
        return Err(format!("Failed to load service: {}", stderr));
    }

    Ok(InstallResult {
        config_path: plist_path.clone(),
        post_install_commands: vec![],
        message: format!(
            "Stakpak watch installed and started as launchd service.\n\
             Service label: {}\n\
             Plist path: {}\n\
             Logs: ~/.stakpak/watch/logs/\n\n\
             The watch service will start automatically on login.\n\
             Use 'launchctl list {}' to check status.",
            SERVICE_LABEL,
            plist_path.display(),
            SERVICE_LABEL
        ),
    })
}

/// Uninstall the launchd service.
pub async fn uninstall() -> Result<UninstallResult, String> {
    let plist_path = get_plist_path();

    if !plist_path.exists() {
        return Err(format!(
            "Service not installed (plist not found at {})",
            plist_path.display()
        ));
    }

    // Unload the service first
    if is_loaded() {
        let unload_output = Command::new("launchctl")
            .args(["unload", plist_path.to_str().unwrap_or("")])
            .output()
            .map_err(|e| format!("Failed to run launchctl unload: {}", e))?;

        if !unload_output.status.success() {
            let stderr = String::from_utf8_lossy(&unload_output.stderr);
            // Don't fail on unload errors, continue to remove the file
            eprintln!("Warning: Failed to unload service: {}", stderr);
        }
    }

    // Remove the plist file
    std::fs::remove_file(&plist_path).map_err(|e| format!("Failed to remove plist file: {}", e))?;

    Ok(UninstallResult {
        config_path: plist_path.clone(),
        message: format!(
            "Stakpak watch service uninstalled.\n\
             Removed: {}\n\n\
             Note: Log files in ~/.stakpak/watch/logs/ were preserved.\n\
             Run history in ~/.stakpak/watch/watch.db was preserved.",
            plist_path.display()
        ),
    })
}

/// Reload the launchd service to pick up configuration changes.
pub async fn reload() -> Result<ReloadResult, String> {
    let plist_path = get_plist_path();

    if !plist_path.exists() {
        return Err(format!(
            "Service not installed (plist not found at {})",
            plist_path.display()
        ));
    }

    if !is_loaded() {
        return Err("Service is installed but not running. Start it first with 'launchctl load ~/Library/LaunchAgents/dev.stakpak.watch.plist'".to_string());
    }

    // Use kickstart -k to kill and restart the service
    // This is more reliable than stop + wait for KeepAlive
    // gui/$(id -u) is the domain for user LaunchAgents
    let uid_output = Command::new("id")
        .args(["-u"])
        .output()
        .map_err(|e| format!("Failed to get user ID: {}", e))?;
    let uid = String::from_utf8_lossy(&uid_output.stdout)
        .trim()
        .to_string();
    let service_target = format!("gui/{}/{}", uid, SERVICE_LABEL);

    let kickstart_output = Command::new("launchctl")
        .args(["kickstart", "-k", &service_target])
        .output()
        .map_err(|e| format!("Failed to run launchctl kickstart: {}", e))?;

    if !kickstart_output.status.success() {
        let stderr = String::from_utf8_lossy(&kickstart_output.stderr);
        // Fallback to stop + start if kickstart fails (older macOS)
        let _ = Command::new("launchctl")
            .args(["stop", SERVICE_LABEL])
            .output();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let start_output = Command::new("launchctl")
            .args(["start", SERVICE_LABEL])
            .output()
            .map_err(|e| format!("Failed to start service: {}", e))?;

        if !start_output.status.success() {
            return Err(format!(
                "Failed to restart service. kickstart error: {}",
                stderr
            ));
        }
    }

    // Wait a moment for it to restart
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Verify it restarted
    if !is_loaded() {
        return Err(
            "Service stopped but did not restart. Check logs at ~/.stakpak/watch/logs/".to_string(),
        );
    }

    Ok(ReloadResult {
        message: format!(
            "Watch service restarted and configuration reloaded.\n\
             Service label: {}\n\
             Use 'launchctl list {}' to verify status.",
            SERVICE_LABEL, SERVICE_LABEL
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_plist_path() {
        let path = get_plist_path();
        assert!(path.to_string_lossy().contains("LaunchAgents"));
        assert!(path.to_string_lossy().contains("dev.stakpak.watch.plist"));
    }

    #[test]
    fn test_generate_plist() {
        let binary_path = PathBuf::from("/usr/local/bin/stakpak");
        let plist = generate_plist(&binary_path);

        assert!(plist.contains("dev.stakpak.watch"));
        assert!(plist.contains("/usr/local/bin/stakpak"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<key>KeepAlive</key>"));
    }
}
