//! Linux systemd service installation.
//!
//! Installs the stakpak watch service as a user systemd service.
//! The service file is installed to ~/.config/systemd/user/stakpak-watch.service

use super::{InstallResult, ReloadResult, UninstallResult, get_stakpak_binary_path};
use std::path::PathBuf;
use std::process::Command;

/// Service name for systemd.
const SERVICE_NAME: &str = "stakpak-watch";

/// Get the path to the systemd user service file.
pub fn get_service_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("systemd")
        .join("user")
        .join(format!("{}.service", SERVICE_NAME))
}

/// Check if the service is installed.
pub fn is_installed() -> bool {
    get_service_path().exists()
}

/// Check if the service is currently active.
fn is_active() -> bool {
    let output = Command::new("systemctl")
        .args(["--user", "is-active", SERVICE_NAME])
        .output();

    match output {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

/// Generate the systemd service unit content.
fn generate_service_unit(binary_path: &std::path::Path) -> String {
    let home_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .display()
        .to_string();

    format!(
        r#"[Unit]
Description=Stakpak Autonomous Agent Watch Service
Documentation=https://stakpak.dev/docs/watch
After=network.target

[Service]
Type=simple
ExecStart={} watch run
Restart=on-failure
RestartSec=10
WorkingDirectory={}

# Environment
Environment=HOME={}
Environment=PATH=/usr/local/bin:/usr/bin:/bin

# Logging
StandardOutput=append:{}/.stakpak/watch/logs/stdout.log
StandardError=append:{}/.stakpak/watch/logs/stderr.log

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths={}/.stakpak

[Install]
WantedBy=default.target
"#,
        binary_path.display(),
        home_dir,
        home_dir,
        home_dir,
        home_dir,
        home_dir,
    )
}

/// Install the systemd user service.
pub async fn install() -> Result<InstallResult, String> {
    let binary_path = get_stakpak_binary_path()?;
    let service_path = get_service_path();

    // Ensure the binary exists
    if !binary_path.exists() {
        return Err(format!(
            "Stakpak binary not found at: {}",
            binary_path.display()
        ));
    }

    // Ensure systemd user directory exists
    if let Some(parent) = service_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create systemd user directory: {}", e))?;
    }

    // Ensure log directory exists
    let log_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".stakpak")
        .join("watch")
        .join("logs");
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| format!("Failed to create log directory: {}", e))?;

    // Stop existing service if running
    if is_active() {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", SERVICE_NAME])
            .output();
    }

    // Generate and write service unit
    let service_content = generate_service_unit(&binary_path);
    std::fs::write(&service_path, &service_content)
        .map_err(|e| format!("Failed to write service file: {}", e))?;

    // Reload systemd daemon
    let reload_output = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output()
        .map_err(|e| format!("Failed to reload systemd: {}", e))?;

    if !reload_output.status.success() {
        let stderr = String::from_utf8_lossy(&reload_output.stderr);
        return Err(format!("Failed to reload systemd: {}", stderr));
    }

    // Enable the service
    let enable_output = Command::new("systemctl")
        .args(["--user", "enable", SERVICE_NAME])
        .output()
        .map_err(|e| format!("Failed to enable service: {}", e))?;

    if !enable_output.status.success() {
        let stderr = String::from_utf8_lossy(&enable_output.stderr);
        return Err(format!("Failed to enable service: {}", stderr));
    }

    // Start the service
    let start_output = Command::new("systemctl")
        .args(["--user", "start", SERVICE_NAME])
        .output()
        .map_err(|e| format!("Failed to start service: {}", e))?;

    if !start_output.status.success() {
        let stderr = String::from_utf8_lossy(&start_output.stderr);
        return Err(format!("Failed to start service: {}", stderr));
    }

    Ok(InstallResult {
        config_path: service_path.clone(),
        post_install_commands: vec![],
        message: format!(
            "Stakpak watch installed and started as systemd user service.\n\
             Service name: {}\n\
             Service file: {}\n\
             Logs: ~/.stakpak/watch/logs/\n\n\
             The watch service will start automatically on login.\n\
             Use 'systemctl --user status {}' to check status.\n\
             Use 'journalctl --user -u {}' to view logs.",
            SERVICE_NAME,
            service_path.display(),
            SERVICE_NAME,
            SERVICE_NAME
        ),
    })
}

/// Uninstall the systemd user service.
pub async fn uninstall() -> Result<UninstallResult, String> {
    let service_path = get_service_path();

    if !service_path.exists() {
        return Err(format!(
            "Service not installed (service file not found at {})",
            service_path.display()
        ));
    }

    // Stop the service if running
    if is_active() {
        let stop_output = Command::new("systemctl")
            .args(["--user", "stop", SERVICE_NAME])
            .output()
            .map_err(|e| format!("Failed to stop service: {}", e))?;

        if !stop_output.status.success() {
            let stderr = String::from_utf8_lossy(&stop_output.stderr);
            eprintln!("Warning: Failed to stop service: {}", stderr);
        }
    }

    // Disable the service
    let disable_output = Command::new("systemctl")
        .args(["--user", "disable", SERVICE_NAME])
        .output()
        .map_err(|e| format!("Failed to disable service: {}", e))?;

    if !disable_output.status.success() {
        let stderr = String::from_utf8_lossy(&disable_output.stderr);
        eprintln!("Warning: Failed to disable service: {}", stderr);
    }

    // Remove the service file
    std::fs::remove_file(&service_path)
        .map_err(|e| format!("Failed to remove service file: {}", e))?;

    // Reload systemd daemon
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();

    Ok(UninstallResult {
        config_path: service_path.clone(),
        message: format!(
            "Stakpak watch service uninstalled.\n\
             Removed: {}\n\n\
             Note: Log files in ~/.stakpak/watch/logs/ were preserved.\n\
             Run history in ~/.stakpak/watch/watch.db was preserved.",
            service_path.display()
        ),
    })
}

/// Reload the systemd service to pick up configuration changes.
pub async fn reload() -> Result<ReloadResult, String> {
    let service_path = get_service_path();

    if !service_path.exists() {
        return Err(format!(
            "Service not installed (service file not found at {})",
            service_path.display()
        ));
    }

    if !is_active() {
        return Err(format!(
            "Service is installed but not running. Start it first with 'systemctl --user start {}'",
            SERVICE_NAME
        ));
    }

    // Restart the service to reload configuration
    // The watch service will re-read watch.toml on startup
    let restart_output = Command::new("systemctl")
        .args(["--user", "restart", SERVICE_NAME])
        .output()
        .map_err(|e| format!("Failed to restart service: {}", e))?;

    if !restart_output.status.success() {
        let stderr = String::from_utf8_lossy(&restart_output.stderr);
        return Err(format!("Failed to restart service: {}", stderr));
    }

    // Wait a moment and verify it's running
    std::thread::sleep(std::time::Duration::from_millis(500));

    if !is_active() {
        return Err(
            "Service restarted but is not active. Check logs with 'journalctl --user -u stakpak-watch'".to_string()
        );
    }

    Ok(ReloadResult {
        message: format!(
            "Watch service restarted and configuration reloaded.\n\
             Service name: {}\n\
             Use 'systemctl --user status {}' to verify status.",
            SERVICE_NAME, SERVICE_NAME
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_service_path() {
        let path = get_service_path();
        assert!(path.to_string_lossy().contains("systemd"));
        assert!(path.to_string_lossy().contains("stakpak-watch.service"));
    }

    #[test]
    fn test_generate_service_unit() {
        let binary_path = PathBuf::from("/usr/local/bin/stakpak");
        let unit = generate_service_unit(&binary_path);

        assert!(unit.contains("[Unit]"));
        assert!(unit.contains("[Service]"));
        assert!(unit.contains("[Install]"));
        assert!(unit.contains("/usr/local/bin/stakpak watch run"));
        assert!(unit.contains("Restart=on-failure"));
    }
}
