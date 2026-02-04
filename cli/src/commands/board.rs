use crate::commands::plugin_utils::{
    Plugin, download_plugin_from_github, execute_plugin_command, get_latest_github_release_version,
    get_plugin_existing_path, is_version_match,
};
use std::process::Command;

const BOARD: Plugin = Plugin {
    plugin_name: "agent-board",
    repo_owner: "stakpak",
    repo_name: "agent-board",
    artifact_prefix: "agent-board",
    binary_name: if cfg!(windows) {
        "agent-board.exe"
    } else {
        "agent-board"
    },
};

/// Pass-through to agent-board plugin. All args after 'board' are forwarded directly.
/// Run `stakpak board --help` for available commands.
pub async fn run_board(args: Vec<String>) -> Result<(), String> {
    let board_path = get_board_plugin_path().await;
    let mut cmd = Command::new(board_path);
    cmd.args(&args);
    execute_plugin_command(cmd, BOARD.plugin_name.to_string())
}

async fn get_board_plugin_path() -> String {
    // Check if we have an existing installation first
    let existing = get_plugin_existing_path(BOARD.binary_name.to_string())
        .await
        .ok();

    let current_version = existing
        .as_ref()
        .and_then(|path| get_board_version(path).ok());

    let latest_version = get_latest_github_release_version(
        BOARD.repo_owner.to_string(),
        BOARD.repo_name.to_string(),
    )
    .await;

    // If we have an existing installation, check if update needed
    if let Some(ref path) = existing {
        // Try to get latest version from GitHub API
        match latest_version {
            Ok(target_version) => {
                if let Some(ref current) = current_version {
                    if is_version_match(current, &target_version) {
                        // Already up to date, use existing
                        return path.clone();
                    }
                    println!(
                        "{} {} is outdated (target: {}), updating...",
                        BOARD.plugin_name, current, target_version
                    );
                }

                // Need to update - download new version
                match download_plugin_from_github(
                    BOARD.repo_owner,
                    BOARD.repo_name,
                    BOARD.artifact_prefix,
                    BOARD.binary_name,
                    None,
                )
                .await
                {
                    Ok(new_path) => {
                        println!(
                            "Successfully installed {} {} -> {}",
                            BOARD.plugin_name, target_version, new_path
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
        Ok(target_version) => {
            match download_plugin_from_github(
                BOARD.repo_owner,
                BOARD.repo_name,
                BOARD.artifact_prefix,
                BOARD.binary_name,
                None,
            )
            .await
            {
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
            }
        }
        Err(e) => {
            // Try download anyway (uses /latest/ URL)
            eprintln!("Warning: Failed to check version: {}", e);
            match download_plugin_from_github(
                BOARD.repo_owner,
                BOARD.repo_name,
                BOARD.artifact_prefix,
                BOARD.binary_name,
                None,
            )
            .await
            {
                Ok(path) => {
                    println!("Successfully installed agent-board -> {}", path);
                    path
                }
                Err(e) => {
                    eprintln!("Failed to download agent-board: {}", e);
                    "agent-board".to_string()
                }
            }
        }
    }
}

fn get_board_version(path: &str) -> Result<String, String> {
    let output = std::process::Command::new(path)
        .arg("version")
        .output()
        .map_err(|e| format!("Failed to run agent-board version: {}", e))?;

    if !output.status.success() {
        return Err("agent-board version command failed".to_string());
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
