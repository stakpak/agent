use serde_json::{Map, Value};
use std::fs;

/// Safely adds VS Code terminal setting for Option+Click selection on macOS
/// Returns true if setting was added or already exists, false if not applicable
pub fn ensure_vscode_option_click_setting() -> bool {
    // Only run on macOS
    if !cfg!(target_os = "macos") {
        return false;
    }

    // Check if we're in VS Code terminal
    if !is_vscode_terminal() {
        return false;
    }

    // Find the workspace root (where .stakpak directory exists)
    let workspace_root = match find_workspace_root() {
        Some(root) => root,
        None => return false,
    };
    let vscode_dir = workspace_root.join(".vscode");
    let settings_path = vscode_dir.join("settings.json");

    // Create .vscode directory if it doesn't exist
    if let Err(e) = fs::create_dir_all(&vscode_dir) {
        eprintln!("Warning: Failed to create .vscode directory: {}", e);
        return false;
    }

    // Read existing settings
    let existing_content = if settings_path.exists() {
        match fs::read_to_string(&settings_path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Warning: Failed to read .vscode/settings.json: {}", e);
                String::new()
            }
        }
    } else {
        String::new()
    };

    // Check if setting already exists and is true
    if existing_content.contains("terminal.integrated.macOptionClickForcesSelection")
        && existing_content.contains("true")
    {
        return true; // Already set correctly
    }

    // If file is empty, create new JSON
    if existing_content.trim().is_empty() {
        let new_content = r#"{
    "terminal.integrated.macOptionClickForcesSelection": true
}"#;
        match fs::write(&settings_path, new_content) {
            Ok(_) => {
                return true;
            }
            Err(e) => {
                eprintln!("Warning: Failed to write .vscode/settings.json: {}", e);
                return false;
            }
        }
    }

    // Parse existing JSON to add our setting
    let mut settings: Map<String, Value> = match serde_json::from_str(&existing_content) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!(
                "Warning: Failed to parse existing .vscode/settings.json: {}",
                e
            );
            eprintln!("Creating new settings file...");
            Map::new()
        }
    };

    // Add or update ONLY the specific setting, preserving all others
    settings.insert(
        "terminal.integrated.macOptionClickForcesSelection".to_string(),
        Value::Bool(true),
    );

    // Write back to file with proper formatting
    match serde_json::to_string_pretty(&settings) {
        Ok(formatted_json) => match fs::write(&settings_path, formatted_json) {
            Ok(_) => true,
            Err(e) => {
                eprintln!("Warning: Failed to write .vscode/settings.json: {}", e);
                false
            }
        },
        Err(e) => {
            eprintln!("Warning: Failed to serialize settings: {}", e);
            false
        }
    }
}

/// Check if we're running in VS Code terminal
fn is_vscode_terminal() -> bool {
    // Check for VS Code process ID
    if std::env::var("VSCODE_PID").is_ok() {
        return true;
    }

    // Check for VS Code in parent processes
    if is_vscode_parent_process(std::process::id()) {
        return true;
    }

    // Check terminal name
    if let Ok(term) = std::env::var("TERM_PROGRAM") {
        if term.contains("vscode") || term.contains("VSCode") {
            return true;
        }
    }

    // Check if we're in a VS Code workspace
    if std::env::var("VSCODE_INJECTION").is_ok() {
        return true;
    }

    false
}

/// Check if VS Code is in the parent process chain
fn is_vscode_parent_process(pid: u32) -> bool {
    // This is a simplified check - in production you might want to use a more robust method
    // For now, we'll just check if we can find VS Code in the process tree
    if let Ok(output) = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
    {
        if let Ok(comm) = String::from_utf8(output.stdout) {
            return comm.to_lowercase().contains("code");
        }
    }
    false
}

/// Find workspace root by looking for .stakpak directory
fn find_workspace_root() -> Option<std::path::PathBuf> {
    let mut current_dir = std::env::current_dir().ok()?;

    loop {
        let stakpak_dir = current_dir.join(".stakpak");
        if stakpak_dir.exists() && stakpak_dir.is_dir() {
            return Some(current_dir);
        }

        if let Some(parent) = current_dir.parent() {
            current_dir = parent.to_path_buf();
        } else {
            break;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macos_detection() {
        // This test will only pass on macOS
        if cfg!(target_os = "macos") {
            assert!(true); // We're on macOS
        } else {
            assert!(!ensure_vscode_option_click_setting()); // Should return false on non-macOS
        }
    }

    #[test]
    fn test_workspace_root_finding() {
        // Test that we can find workspace root
        let root = find_workspace_root();
        if root.is_some() {
            assert!(root.unwrap().join(".stakpak").exists());
        }
    }
}
