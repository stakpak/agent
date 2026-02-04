use crate::utils::plugins::{
    PluginConfig, download_and_install_plugin, execute_plugin_command, get_existing_plugin_path,
    get_latest_github_release_version, is_same_version,
};
use std::process::Command;

fn get_browser_config() -> PluginConfig {
    PluginConfig {
        name: "agent-tab".to_string(),
        base_url: "https://github.com/stakpak/tab".to_string(),
        targets: vec![
            "linux-x86_64".to_string(),
            "darwin-x86_64".to_string(),
            "darwin-aarch64".to_string(),
            "windows-x86_64".to_string(),
        ],
        version: None,
        repo: Some("tab".to_string()),
        owner: Some("stakpak".to_string()),
    }
}

fn get_daemon_config() -> PluginConfig {
    PluginConfig {
        name: "agent-tab-daemon".to_string(),
        base_url: "https://github.com/stakpak/tab".to_string(),
        targets: vec![
            "linux-x86_64".to_string(),
            "darwin-x86_64".to_string(),
            "darwin-aarch64".to_string(),
            "windows-x86_64".to_string(),
        ],
        version: None,
        repo: Some("tab".to_string()),
        owner: Some("stakpak".to_string()),
    }
}

pub async fn run_browser(args: Vec<String>) -> Result<(), String> {
    let config = get_browser_config();

    let browser_path = get_browser_plugin_path().await;
    let mut cmd = Command::new(&browser_path);
    cmd.args(&args);
    execute_plugin_command(cmd, config.name)
}

fn get_browser_version(path: &str) -> Result<String, String> {
    let config = get_browser_config();

    let output = std::process::Command::new(path)
        .arg("version")
        .output()
        .map_err(|e| format!("Failed to run {} version: {}", config.name, e))?;

    if !output.status.success() {
        return Err(format!("{} version command failed", config.name));
    }

    let version_output = String::from_utf8_lossy(&output.stdout);
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

fn get_daemon_version(path: &str) -> Result<String, String> {
    let config = get_daemon_config();

    let output = std::process::Command::new(path)
        .arg("--version")
        .output()
        .map_err(|e| format!("Failed to run {}: {}", config.name, e))?;

    if !output.status.success() {
        return Err(format!("{} version command failed", config.name));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn ensure_daemon_downloaded(version: Option<&str>) {
    let config = get_daemon_config();

    if let Ok(existing_path) = get_existing_plugin_path(&config.name) {
        if let Some(target) = version {
            if let Ok(current) = get_daemon_version(&existing_path) {
                if is_same_version(&current, target) {
                    return;
                }
                println!(
                    "{} {} is outdated (target: {}), updating...",
                    config.name, current, target
                );
            }
        } else {
            return; // No version to check, binary exists
        }
    }

    match download_and_install_plugin(&config).await {
        Ok(path) => {
            let version_str = version.map(|v| format!("{} ", v)).unwrap_or_default();
            println!(
                "Successfully installed {} {}-> {}",
                config.name, version_str, path
            );
        }
        Err(e) => eprintln!("Failed to download {}: {}", config.name, e),
    }
}

async fn get_browser_plugin_path() -> String {
    let config = get_browser_config();
    let existing = get_existing_plugin_path(&config.name).ok();
    let current_version = existing.as_ref().and_then(|p| get_browser_version(p).ok());

    let latest_version = get_latest_github_release_version(
        config.owner.as_deref().unwrap_or_default(),
        config.repo.as_deref().unwrap_or_default(),
    )
    .await;

    if let Some(ref path) = existing {
        match latest_version {
            Ok(target_version) => {
                if let Some(ref current) = current_version {
                    if is_same_version(current, &target_version) {
                        ensure_daemon_downloaded(Some(&target_version)).await;
                        return path.clone();
                    }
                    println!(
                        "{} {} is outdated (target: {}), updating...",
                        config.name, current, target_version
                    );
                }

                match download_and_install_plugin(&config).await {
                    Ok(new_path) => {
                        println!(
                            "Successfully installed {} {} -> {}",
                            config.name, target_version, new_path
                        );
                        ensure_daemon_downloaded(Some(&target_version)).await;
                        return new_path;
                    }
                    Err(e) => {
                        eprintln!("Failed to update {}: {}", config.name, e);
                        eprintln!("Using existing version");
                        ensure_daemon_downloaded(Some(&target_version)).await;
                        return path.clone();
                    }
                }
            }
            Err(_) => {
                ensure_daemon_downloaded(None).await;
                return path.clone();
            }
        }
    }

    // No existing installation - must download
    match latest_version {
        Ok(target_version) => match download_and_install_plugin(&config).await {
            Ok(path) => {
                println!(
                    "Successfully installed {} {} -> {}",
                    config.name, target_version, path
                );
                ensure_daemon_downloaded(Some(&target_version)).await;
                path
            }
            Err(e) => {
                eprintln!("Failed to download {}: {}", config.name, e);
                config.name.to_string()
            }
        },
        Err(e) => {
            eprintln!("Warning: Failed to check version: {}", e);
            match download_and_install_plugin(&config).await {
                Ok(path) => {
                    println!("Successfully installed {} -> {}", config.name, path);
                    ensure_daemon_downloaded(None).await;
                    path
                }
                Err(e) => {
                    eprintln!("Failed to download {}: {}", config.name, e);
                    config.name.to_string()
                }
            }
        }
    }
}
