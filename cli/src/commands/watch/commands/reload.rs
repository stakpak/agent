//! Autopilot reload command - reload the installed autopilot configuration.

use super::service::{Platform, is_service_installed, reload_service};

/// Reload the autopilot configuration.
pub async fn reload_watch() -> Result<(), String> {
    let platform = Platform::detect();

    if !is_service_installed() {
        return Err(
            "Autopilot service is not installed. Install it first with 'stakpak autopilot up'."
                .to_string(),
        );
    }

    println!("Reloading stakpak autopilot configuration...");
    println!("Platform: {}", platform.name());
    println!();

    // Validate the config can be loaded
    let config_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".stakpak")
        .join("watch.toml");

    match crate::commands::watch::WatchConfig::load_default() {
        Ok(config) => {
            println!(
                "✓ Configuration validated ({} schedules)",
                config.schedules.len()
            );
        }
        Err(e) => {
            println!("\x1b[31m✗\x1b[0m Configuration error: {}", e);
            println!();
            println!(
                "Fix the configuration at {} and try again.",
                config_path.display()
            );
            return Err(format!("Invalid configuration: {}", e));
        }
    }

    // Reload the service
    let result = reload_service().await?;

    println!();
    println!("\x1b[32m✓ Autopilot reloaded successfully!\x1b[0m");
    println!();
    println!("{}", result.message);

    Ok(())
}
