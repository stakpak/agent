//! Watch install command - install as a system service.

use super::service::{Platform, get_service_config_path, install_service, is_service_installed};

/// Install the watch service as a system service.
pub async fn install_watch(force: bool) -> Result<(), String> {
    let platform = Platform::detect();

    println!("Installing stakpak watch as a system service...");
    println!("Platform: {}", platform.name());
    println!();

    // Check if already installed
    if is_service_installed()
        && !force
        && let Some(path) = get_service_config_path()
    {
        println!("Service is already installed at: {}", path.display());
        println!();
        println!("Use --force to reinstall.");
        return Ok(());
    }

    // Check for watch config
    let config_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".stakpak")
        .join("watch.toml");

    if !config_path.exists() {
        println!(
            "\x1b[33m⚠\x1b[0m  No watch configuration found at: {}",
            config_path.display()
        );
        println!();
        println!("Create a configuration first with:");
        println!("  stakpak watch init");
        println!();
        println!("Then run this command again.");
        return Err("Watch configuration not found".to_string());
    }

    // Validate the config can be loaded
    match crate::commands::watch::WatchConfig::load_default() {
        Ok(config) => {
            println!(
                "✓ Configuration validated ({} triggers)",
                config.triggers.len()
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

    // Install the service
    let result = install_service().await?;

    println!();
    println!("\x1b[32m✓ Service installed successfully!\x1b[0m");
    println!();
    println!("{}", result.message);

    if !result.post_install_commands.is_empty() {
        println!();
        println!("Run these commands to complete installation:");
        for cmd in &result.post_install_commands {
            println!("  {}", cmd);
        }
    }

    Ok(())
}
