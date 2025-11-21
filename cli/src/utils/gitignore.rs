use std::{fs::OpenOptions, io::Write, path::Path};

use crate::config::AppConfig;

/// Appends `.stakpak/session*` to the `.gitignore` file in the current directory if:
/// 1. The feature is enabled in config (auto_append_gitignore is true)
/// 2. We're in a git repository
/// 3. `.stakpak/session*` is not already in the `.gitignore` file
pub fn ensure_stakpak_in_gitignore(config: &AppConfig) -> Result<bool, String> {
    // Check if the feature is enabled
    if !config.auto_append_gitignore.unwrap_or(true) {
        return Ok(false);
    }

    // Check if we're in a git repository
    let current_dir =
        std::env::current_dir().map_err(|e| format!("Failed to get current directory: {}", e))?;

    let git_info = crate::utils::local_context::get_git_info(&current_dir.to_string_lossy());

    if !git_info.is_git_repo {
        return Ok(false);
    }

    let gitignore_path = current_dir.join(".gitignore");

    // Check if we need to update or add the entry
    if gitignore_path.exists() {
        let status = check_and_update_gitignore(&gitignore_path)?;
        Ok(status)
    } else {
        // Create new .gitignore with .stakpak/session* entry
        append_stakpak_to_gitignore(&gitignore_path)?;
        Ok(true)
    }
}

/// Checks and updates the `.gitignore` file:
/// - Returns Ok(false) if `.stakpak/session*` already exists
/// - Updates `.stakpak` or `.stakpak/` to `.stakpak/session*` and returns Ok(true)
/// - Appends `.stakpak/session*` if no stakpak entry exists and returns Ok(true)
fn check_and_update_gitignore(gitignore_path: &Path) -> Result<bool, String> {
    let content = std::fs::read_to_string(gitignore_path)
        .map_err(|e| format!("Failed to read .gitignore: {}", e))?;

    let lines: Vec<&str> = content.lines().collect();
    let mut has_session_pattern = false;
    let mut has_old_pattern = false;
    let mut old_pattern_index = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == ".stakpak/session*" {
            has_session_pattern = true;
            break;
        } else if trimmed == ".stakpak" || trimmed == ".stakpak/" {
            has_old_pattern = true;
            old_pattern_index = Some(i);
        }
    }

    // Already has the correct pattern
    if has_session_pattern {
        return Ok(false);
    }

    // Update old pattern to new pattern
    if has_old_pattern && let Some(index) = old_pattern_index {
        let mut new_lines = lines.clone();
        new_lines[index] = ".stakpak/session*";
        let new_content = new_lines.join("\n");

        // Preserve trailing newline if it existed
        let new_content = if content.ends_with('\n') {
            format!("{}\n", new_content)
        } else {
            new_content
        };

        std::fs::write(gitignore_path, new_content)
            .map_err(|e| format!("Failed to update .gitignore: {}", e))?;
        return Ok(true);
    }

    // No stakpak entry exists, append new one
    append_stakpak_to_gitignore(gitignore_path)?;
    Ok(true)
}

/// Appends `.stakpak/session*` to the `.gitignore` file, creating it if it doesn't exist
fn append_stakpak_to_gitignore(gitignore_path: &Path) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(gitignore_path)
        .map_err(|e| format!("Failed to open/create .gitignore: {}", e))?;

    // Check if file ends with newline, if not add one before our entry
    let needs_newline = if gitignore_path.exists() {
        let metadata = std::fs::metadata(gitignore_path)
            .map_err(|e| format!("Failed to get .gitignore metadata: {}", e))?;

        if metadata.len() > 0 {
            // Read the last character to see if we need a newline
            let content = std::fs::read(gitignore_path)
                .map_err(|e| format!("Failed to read .gitignore: {}", e))?;

            !content.ends_with(b"\n")
        } else {
            false
        }
    } else {
        false
    };

    // Add newline if needed before our section
    if needs_newline {
        writeln!(file).map_err(|e| format!("Failed to write newline to .gitignore: {}", e))?;
    }

    // Add a blank line for separation (if file has content)
    if gitignore_path.exists()
        && let Ok(metadata) = std::fs::metadata(gitignore_path)
        && metadata.len() > 0
    {
        writeln!(file).map_err(|e| format!("Failed to write blank line to .gitignore: {}", e))?;
    }

    // Add comment and .stakpak/session* entry
    writeln!(file, "# Stakpak session files")
        .map_err(|e| format!("Failed to write comment to .gitignore: {}", e))?;
    writeln!(file, ".stakpak/session*")
        .map_err(|e| format!("Failed to write .stakpak/session* to .gitignore: {}", e))?;

    Ok(())
}

pub fn is_git_repo() -> bool {
    let current_dir = std::env::current_dir().unwrap_or_default();
    let git_info = crate::utils::local_context::get_git_info(&current_dir.to_string_lossy());
    git_info.is_git_repo
}
