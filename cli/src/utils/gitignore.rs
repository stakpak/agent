use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::Path,
};

use crate::config::AppConfig;

/// Appends `.stakpak` to the `.gitignore` file in the current directory if:
/// 1. The feature is enabled in config (auto_append_gitignore is true)
/// 2. We're in a git repository
/// 3. `.stakpak` is not already in the `.gitignore` file
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

    // Check if .stakpak is already in .gitignore
    if gitignore_path.exists() && stakpak_already_in_gitignore(&gitignore_path)? {
        return Ok(false);
    }

    // Append .stakpak to .gitignore
    append_stakpak_to_gitignore(&gitignore_path)?;
    Ok(true)
}

/// Checks if `.stakpak` is already present in the `.gitignore` file
fn stakpak_already_in_gitignore(gitignore_path: &Path) -> Result<bool, String> {
    let file =
        File::open(gitignore_path).map_err(|e| format!("Failed to open .gitignore: {}", e))?;

    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line.map_err(|e| format!("Failed to read line from .gitignore: {}", e))?;
        let trimmed = line.trim();

        // Check for exact matches or patterns that would cover .stakpak
        if trimmed == ".stakpak" || trimmed == ".stakpak/" {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Appends `.stakpak` to the `.gitignore` file, creating it if it doesn't exist
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
    if gitignore_path.exists() {
        if let Ok(metadata) = std::fs::metadata(gitignore_path) {
            if metadata.len() > 0 {
                writeln!(file)
                    .map_err(|e| format!("Failed to write blank line to .gitignore: {}", e))?;
            }
        }
    }

    // Add comment and .stakpak entry
    writeln!(file, "# Stakpak local files")
        .map_err(|e| format!("Failed to write comment to .gitignore: {}", e))?;
    writeln!(file, ".stakpak")
        .map_err(|e| format!("Failed to write .stakpak to .gitignore: {}", e))?;

    Ok(())
}
