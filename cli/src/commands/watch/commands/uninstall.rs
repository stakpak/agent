//! Watch uninstall command - remove the system service.

use super::service::{Platform, get_service_config_path, is_service_installed, uninstall_service};

/// Uninstall the watch system service.
pub async fn uninstall_watch() -> Result<(), String> {
    let platform = Platform::detect();

    println!("Uninstalling stakpak watch service...");
    println!("Platform: {}", platform.name());
    println!();

    // Check if installed
    if !is_service_installed() {
        if let Some(path) = get_service_config_path() {
            println!("Service is not installed (no config at {})", path.display());
        } else {
            println!("Service is not installed.");
        }
        return Ok(());
    }

    // Uninstall the service
    let result = uninstall_service().await?;

    println!("\x1b[32m✓ Service uninstalled successfully!\x1b[0m");
    println!();
    println!("{}", result.message);

    Ok(())
}
