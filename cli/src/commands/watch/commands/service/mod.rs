//! Platform-specific service installation for the watch service.
//!
//! This module provides functionality to install/uninstall the stakpak watch service
//! as a system service on different platforms:
//! - macOS: launchd (LaunchAgents)
//! - Linux: systemd (user services)
//! - Windows: Windows Service (future)

mod launchd;
mod systemd;

use std::path::PathBuf;

/// Supported platforms for service installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOS,
    Linux,
    Windows,
    Unknown,
}

impl Platform {
    /// Detect the current platform.
    pub fn detect() -> Self {
        #[cfg(target_os = "macos")]
        {
            Platform::MacOS
        }
        #[cfg(target_os = "linux")]
        {
            Platform::Linux
        }
        #[cfg(target_os = "windows")]
        {
            Platform::Windows
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Platform::Unknown
        }
    }

    /// Get a human-readable name for the platform.
    pub fn name(&self) -> &'static str {
        match self {
            Platform::MacOS => "macOS (launchd)",
            Platform::Linux => "Linux (systemd)",
            Platform::Windows => "Windows",
            Platform::Unknown => "Unknown",
        }
    }
}

/// Result of a service installation operation.
#[derive(Debug)]
pub struct InstallResult {
    /// Path to the generated service configuration file.
    pub config_path: PathBuf,
    /// Commands to run to complete installation (if any manual steps needed).
    pub post_install_commands: Vec<String>,
    /// Human-readable message about the installation.
    pub message: String,
}

/// Result of a service uninstallation operation.
#[derive(Debug)]
pub struct UninstallResult {
    /// Path to the removed service configuration file.
    pub config_path: PathBuf,
    /// Human-readable message about the uninstallation.
    pub message: String,
}

/// Result of a service reload operation.
#[derive(Debug)]
pub struct ReloadResult {
    /// Human-readable message about the reload.
    pub message: String,
}

/// Install the watch service as a system service.
pub async fn install_service() -> Result<InstallResult, String> {
    let platform = Platform::detect();

    match platform {
        Platform::MacOS => launchd::install().await,
        Platform::Linux => systemd::install().await,
        Platform::Windows => Err("Windows service installation is not yet supported. Run 'stakpak watch run' manually or use Task Scheduler.".to_string()),
        Platform::Unknown => Err("Unknown platform. Cannot install service.".to_string()),
    }
}

/// Uninstall the watch system service.
pub async fn uninstall_service() -> Result<UninstallResult, String> {
    let platform = Platform::detect();

    match platform {
        Platform::MacOS => launchd::uninstall().await,
        Platform::Linux => systemd::uninstall().await,
        Platform::Windows => {
            Err("Windows service uninstallation is not yet supported.".to_string())
        }
        Platform::Unknown => Err("Unknown platform. Cannot uninstall service.".to_string()),
    }
}

/// Reload the watch service configuration.
pub async fn reload_service() -> Result<ReloadResult, String> {
    let platform = Platform::detect();

    match platform {
        Platform::MacOS => launchd::reload().await,
        Platform::Linux => systemd::reload().await,
        Platform::Windows => Err("Windows service reload is not yet supported.".to_string()),
        Platform::Unknown => Err("Unknown platform. Cannot reload service.".to_string()),
    }
}

/// Check if the service is currently installed.
pub fn is_service_installed() -> bool {
    let platform = Platform::detect();

    match platform {
        Platform::MacOS => launchd::is_installed(),
        Platform::Linux => systemd::is_installed(),
        Platform::Windows => false,
        Platform::Unknown => false,
    }
}

/// Get the path to the service configuration file.
pub fn get_service_config_path() -> Option<PathBuf> {
    let platform = Platform::detect();

    match platform {
        Platform::MacOS => Some(launchd::get_plist_path()),
        Platform::Linux => Some(systemd::get_service_path()),
        Platform::Windows => None,
        Platform::Unknown => None,
    }
}

/// Get the current stakpak binary path.
pub fn get_stakpak_binary_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("Failed to get current executable path: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_platform() {
        let platform = Platform::detect();
        // Should detect something on any supported platform
        #[cfg(target_os = "macos")]
        assert_eq!(platform, Platform::MacOS);
        #[cfg(target_os = "linux")]
        assert_eq!(platform, Platform::Linux);
        #[cfg(target_os = "windows")]
        assert_eq!(platform, Platform::Windows);
    }

    #[test]
    fn test_platform_name() {
        assert_eq!(Platform::MacOS.name(), "macOS (launchd)");
        assert_eq!(Platform::Linux.name(), "Linux (systemd)");
    }
}
