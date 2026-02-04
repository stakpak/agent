use crate::utils::plugins::{
    PluginConfig, download_and_install_plugin, execute_plugin_command, get_existing_plugin_path,
    get_latest_github_release_version, is_same_version,
};
use std::process::Command;

fn get_board_plugin_config() -> PluginConfig {
    PluginConfig {
        name: "agent-board".to_string(),
        base_url: "https://github.com/stakpak/agent-board".to_string(),
        targets: vec![
            "linux-x86_64".to_string(),
            "windows-x86_64".to_string(),
            "darwin-x86_64".to_string(),
            "darwin-aarch64".to_string(),
        ],
        version: None,
        repo: Some("agent-board".to_string()),
        owner: Some("stakpak".to_string()),
    }
}

/// Pass-through to agent-board plugin. All args after 'board' are forwarded directly.
/// Run `stakpak board --help` for available commands.
pub async fn run_board(args: Vec<String>) -> Result<(), String> {
    let plugin_config = get_board_plugin_config();

    let board_path = get_board_plugin_path().await;
    let mut cmd = Command::new(board_path);
    cmd.args(&args);
    execute_plugin_command(cmd, plugin_config.name)
}

async fn get_board_plugin_path() -> String {
    let config = get_board_plugin_config();
    let existing = get_existing_plugin_path(&config.name).ok();

    let current_version = existing
        .as_ref()
        .and_then(|path| get_board_version(path).ok());

    let latest_version = get_latest_github_release_version(
        config.owner.as_deref().unwrap_or_default(),
        config.repo.as_deref().unwrap_or_default(),
    )
    .await;

    // If we have an existing installation, check if update needed
    if let Some(ref path) = existing {
        // Try to get latest version from GitHub API
        match latest_version {
            Ok(target_version) => {
                if let Some(ref current) = current_version {
                    if is_same_version(current, &target_version) {
                        // Already up to date, use existing
                        return path.clone();
                    }
                    println!(
                        "{} {} is outdated (target: {}), updating...",
                        &config.name, current, target_version
                    );
                }

                // Need to update - download new version
                match download_and_install_plugin(&config).await {
                    Ok(new_path) => {
                        println!(
                            "Successfully installed {} {} -> {}",
                            config.name, target_version, new_path
                        );
                        return new_path;
                    }
                    Err(e) => {
                        eprintln!("Failed to update agent-board: {}", e);
                        eprintln!("Using existing version");
                        return path.clone();
                    }
                }
            }
            Err(_) => {
                // Can't check version, use existing installation
                return path.clone();
            }
        }
    }

    // No existing installation - must download
    match latest_version {
        Ok(target_version) => match download_and_install_plugin(&config).await {
            Ok(path) => {
                println!(
                    "Successfully installed agent-board {} -> {}",
                    target_version, path
                );
                path
            }
            Err(e) => {
                eprintln!("Failed to download agent-board: {}", e);
                "agent-board".to_string()
            }
        },
        Err(e) => {
            // Try download anyway (uses /latest/ URL)
            eprintln!("Warning: Failed to check version: {}", e);
            match download_and_install_plugin(&config).await {
                Ok(path) => {
                    println!("Successfully installed agent-board -> {}", path);
                    path
                }
                Err(e) => {
                    eprintln!("Failed to download agent-board: {}", e);
                    config.name.clone()
                }
            }
        }
    }
}

fn get_board_version(path: &str) -> Result<String, String> {
    let config = get_board_plugin_config();

    let output = std::process::Command::new(path)
        .arg("version")
        .output()
        .map_err(|e| format!("Failed to run {} version: {}", config.name, e))?;

    if !output.status.success() {
        return Err(format!("{} version command failed", config.name));
    }

    let version_output = String::from_utf8_lossy(&output.stdout);
    // Parse version from output like "agent-board v0.1.6" or just "v0.1.6"
    let trimmed = version_output.trim();
    if let Some(v) = trimmed.split_whitespace().find(|s| {
        s.starts_with('v')
            || s.chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
    }) {
        Ok(v.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}
