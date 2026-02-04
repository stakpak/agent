use crate::commands::plugin_utils::{
    Plugin, download_plugin_from_github, execute_plugin_command, get_latest_github_release_version,
    get_plugin_existing_path, is_version_match,
};
use std::process::Command;

const BROWSER: Plugin = Plugin {
    plugin_name: "agent-tab",
    repo_owner: "stakpak",
    repo_name: "tab",
    artifact_prefix: "agent-tab",
    binary_name: if cfg!(windows) {
        "agent-tab.exe"
    } else {
        "agent-tab"
    },
};

const DAEMON: Plugin = Plugin {
    plugin_name: "agent-tab-daemon",
    repo_owner: "stakpak",
    repo_name: "tab",
    artifact_prefix: "agent-tab-daemon",
    binary_name: if cfg!(windows) {
        "agent-tab-daemon.exe"
    } else {
        "agent-tab-daemon"
    },
};

pub async fn run_browser(args: Vec<String>) -> Result<(), String> {
    let browser_path = get_browser_plugin_path().await;
    let mut cmd = Command::new(&browser_path);
    cmd.args(&args);
    execute_plugin_command(cmd, BROWSER.binary_name.to_string())
}

fn get_browser_version(path: &str) -> Result<String, String> {
    let output = std::process::Command::new(path)
        .arg("version")
        .output()
        .map_err(|e| format!("Failed to run {} version: {}", BROWSER.binary_name, e))?;

    if !output.status.success() {
        return Err(format!("{} version command failed", BROWSER.binary_name));
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
    let output = std::process::Command::new(path)
        .arg("--version")
        .output()
        .map_err(|e| format!("Failed to run {}: {}", DAEMON.plugin_name, e))?;

    if !output.status.success() {
        return Err(format!("{} version command failed", DAEMON.plugin_name));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn ensure_daemon_downloaded(version: Option<&str>) {
    if let Ok(existing_path) = get_plugin_existing_path(DAEMON.binary_name.to_string()).await {
        if let Some(target) = version {
            if let Ok(current) = get_daemon_version(&existing_path) {
                if is_version_match(&current, target) {
                    return;
                }
                println!(
                    "{} {} is outdated (target: {}), updating...",
                    DAEMON.binary_name, current, target
                );
            }
        } else {
            return; // No version to check, binary exists
        }
    }

    match download_plugin_from_github(
        DAEMON.repo_owner,
        DAEMON.repo_name,
        DAEMON.artifact_prefix,
        DAEMON.binary_name,
        version,
    )
    .await
    {
        Ok(path) => {
            let version_str = version.map(|v| format!("{} ", v)).unwrap_or_default();
            println!(
                "Successfully installed {} {}-> {}",
                DAEMON.binary_name, version_str, path
            );
        }
        Err(e) => eprintln!("Failed to download {}: {}", DAEMON.binary_name, e),
    }
}

async fn get_browser_plugin_path() -> String {
    let existing = get_plugin_existing_path(BROWSER.binary_name.to_string())
        .await
        .ok();
    let current_version = existing.as_ref().and_then(|p| get_browser_version(p).ok());

    let latest_version = get_latest_github_release_version(
        BROWSER.repo_owner.to_string(),
        BROWSER.repo_name.to_string(),
    )
    .await;

    if let Some(ref path) = existing {
        match latest_version {
            Ok(target_version) => {
                if let Some(ref current) = current_version {
                    if is_version_match(current, &target_version) {
                        ensure_daemon_downloaded(Some(&target_version)).await;
                        return path.clone();
                    }
                    println!(
                        "{} {} is outdated (target: {}), updating...",
                        BROWSER.binary_name, current, target_version
                    );
                }

                match download_plugin_from_github(
                    BROWSER.repo_owner,
                    BROWSER.repo_name,
                    BROWSER.artifact_prefix,
                    BROWSER.binary_name,
                    Some(&target_version),
                )
                .await
                {
                    Ok(new_path) => {
                        println!(
                            "Successfully installed {} {} -> {}",
                            BROWSER.binary_name, target_version, new_path
                        );
                        ensure_daemon_downloaded(Some(&target_version)).await;
                        return new_path;
                    }
                    Err(e) => {
                        eprintln!("Failed to update {}: {}", BROWSER.binary_name, e);
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
        Ok(target_version) => {
            match download_plugin_from_github(
                BROWSER.repo_owner,
                BROWSER.repo_name,
                BROWSER.artifact_prefix,
                BROWSER.binary_name,
                Some(&target_version),
            )
            .await
            {
                Ok(path) => {
                    println!(
                        "Successfully installed {} {} -> {}",
                        BROWSER.plugin_name, target_version, path
                    );
                    ensure_daemon_downloaded(Some(&target_version)).await;
                    path
                }
                Err(e) => {
                    eprintln!("Failed to download {}: {}", BROWSER.plugin_name, e);
                    BROWSER.binary_name.to_string()
                }
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to check version: {}", e);
            match download_plugin_from_github(
                BROWSER.repo_owner,
                BROWSER.repo_name,
                BROWSER.artifact_prefix,
                BROWSER.binary_name,
                None,
            )
            .await
            {
                Ok(path) => {
                    println!("Successfully installed {} -> {}", BROWSER.plugin_name, path);
                    ensure_daemon_downloaded(None).await;
                    path
                }
                Err(e) => {
                    eprintln!("Failed to download {}: {}", BROWSER.plugin_name, e);
                    BROWSER.binary_name.to_string()
                }
            }
        }
    }
}
