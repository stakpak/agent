//! Autopilot reload command — validate configuration and restart the service.

use crate::commands::watch::config::{STAKPAK_AUTOPILOT_CONFIG_PATH, expand_tilde};

const AUTOPILOT_SYSTEMD_SERVICE: &str = "stakpak-autopilot";
const AUTOPILOT_LAUNCHD_LABEL: &str = "dev.stakpak.autopilot";

/// Reload the autopilot configuration.
///
/// Validates the config file, then restarts the running service.
/// If no service is installed, reloads in-process (for foreground mode).
pub async fn reload_autopilot() -> Result<(), String> {
    let config_path = expand_tilde(STAKPAK_AUTOPILOT_CONFIG_PATH);

    // 1. Validate configuration
    println!("Validating configuration...");
    match crate::commands::watch::ScheduleConfig::load_default() {
        Ok(config) => {
            println!(
                "  ✓ Configuration valid ({} schedules)",
                config.schedules.len()
            );
        }
        Err(e) => {
            return Err(format!(
                "Configuration error in {}: {}\nFix the configuration and try again.",
                config_path.display(),
                e
            ));
        }
    }

    // 2. Check if a system service is installed and restart it
    if let Some(service_path) = installed_service_path() {
        println!("  ✓ Service installed ({})", service_path.display());
        println!();
        println!("Restarting autopilot service...");
        stop_service()?;
        start_service()?;
        println!();
        println!("\x1b[32m✓ Autopilot reloaded successfully\x1b[0m");
    } else {
        // No service installed — might be running in foreground mode.
        // A foreground process would need to be stopped and restarted manually.
        println!();
        println!("No autopilot service is installed.");
        println!("If running in foreground mode, stop and restart the process to pick up changes.");
    }

    Ok(())
}

fn installed_service_path() -> Option<std::path::PathBuf> {
    let path = service_path();
    if !path.as_os_str().is_empty() && path.exists() {
        Some(path)
    } else {
        None
    }
}

fn service_path() -> std::path::PathBuf {
    if cfg!(target_os = "linux") {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".config/systemd/user")
            .join(format!("{}.service", AUTOPILOT_SYSTEMD_SERVICE))
    } else if cfg!(target_os = "macos") {
        dirs::home_dir()
            .unwrap_or_default()
            .join("Library/LaunchAgents")
            .join(format!("{}.plist", AUTOPILOT_LAUNCHD_LABEL))
    } else {
        std::path::PathBuf::new()
    }
}

fn stop_service() -> Result<(), String> {
    if cfg!(target_os = "linux") {
        run_cmd(
            "systemctl",
            &["--user", "stop", AUTOPILOT_SYSTEMD_SERVICE],
            "Failed to stop systemd service",
        )
    } else if cfg!(target_os = "macos") {
        let output = std::process::Command::new("launchctl")
            .args(["stop", AUTOPILOT_LAUNCHD_LABEL])
            .output()
            .map_err(|e| format!("Failed to stop launchd service: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // "could not find service" is fine — means it wasn't running
            if stderr
                .to_ascii_lowercase()
                .contains("could not find service")
            {
                Ok(())
            } else {
                Err(format!("Failed to stop launchd service: {}", stderr))
            }
        }
    } else {
        Err("Unsupported platform".to_string())
    }
}

fn start_service() -> Result<(), String> {
    if cfg!(target_os = "linux") {
        run_cmd(
            "systemctl",
            &["--user", "daemon-reload"],
            "Failed to reload systemd",
        )?;
        run_cmd(
            "systemctl",
            &["--user", "start", AUTOPILOT_SYSTEMD_SERVICE],
            "Failed to start systemd service",
        )
    } else if cfg!(target_os = "macos") {
        let plist = service_path();
        let load_output = std::process::Command::new("launchctl")
            .args(["load", plist.to_string_lossy().as_ref()])
            .output()
            .map_err(|e| format!("Failed to load launchd service: {}", e))?;

        if !load_output.status.success() {
            let stderr = String::from_utf8_lossy(&load_output.stderr);
            if !stderr.to_ascii_lowercase().contains("already loaded") {
                return Err(format!("Failed to load launchd service: {}", stderr));
            }
        }

        run_cmd(
            "launchctl",
            &["start", AUTOPILOT_LAUNCHD_LABEL],
            "Failed to start launchd service",
        )
    } else {
        Err("Unsupported platform".to_string())
    }
}

fn run_cmd(program: &str, args: &[&str], error_msg: &str) -> Result<(), String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{}: {}", error_msg, e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("{}: {}", error_msg, stderr))
    }
}
