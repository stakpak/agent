//! External editor integration
//!
//! Provides functionality to open files in external editors (vim, nvim, nano)
//! with proper terminal state management.

use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::Terminal;
use std::io::stdout;
use std::process::Command;

/// Configuration for the external editor


/// Detect available editor on the system
/// 
/// Priority order:
/// 1. Preferred editor (if specified)
/// 2. vim
/// 3. nvim
/// 4. nano
/// 
/// Returns None if no editor is found
pub fn detect_editor(preferred: Option<String>) -> Option<String> {
    // If preferred editor is specified, check if it exists
    if let Some(editor) = preferred {
        if is_editor_available(&editor) {
            return Some(editor);
        }
    }

    // Try editors in order of preference
    for editor in &["vim", "nvim", "nano"] {
        if is_editor_available(editor) {
            return Some(editor.to_string());
        }
    }

    None
}

/// Check if an editor command is available in PATH
fn is_editor_available(editor: &str) -> bool {
    Command::new("which")
        .arg(editor)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Open a file in an external editor
/// 
/// This function:
/// 1. Suspends the TUI (leaves alternate screen, disables raw mode)
/// 2. Spawns the editor with full terminal control
/// 3. Waits for the editor to close
/// 4. Restores the TUI (enters alternate screen, enables raw mode, clears)
/// 
/// # Arguments
/// * `terminal` - The ratatui terminal instance
/// * `editor` - The editor command to use
/// * `file_path` - Path to the file to edit
/// * `line_number` - Optional line number to jump to
pub fn open_in_editor<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    editor: &str,
    file_path: &str,
    line_number: Option<usize>,
) -> Result<(), String> {
    // Suspend TUI and show cursor for the editor
    execute!(stdout(), LeaveAlternateScreen, Show).map_err(|e| format!("Failed to leave alternate screen: {}", e))?;
    disable_raw_mode().map_err(|e| format!("Failed to disable raw mode: {}", e))?;

    // Build editor command
    let mut cmd = Command::new(editor);
    cmd.arg(file_path);

    // Add line number if specified (all three editors support +LINE syntax)
    if let Some(line) = line_number {
        cmd.arg(format!("+{}", line));
    }

    // Spawn editor and wait for it to close
    let result = cmd.status();

    // Always restore TUI, even if editor failed
    let restore_result = restore_tui(terminal);

    // Check if editor ran successfully
    match result {
        Ok(status) if status.success() => restore_result,
        Ok(status) => Err(format!("Editor exited with code: {:?}", status.code())),
        Err(e) => {
            restore_result?;
            Err(format!("Failed to run editor: {}", e))
        }
    }
}

/// Restore TUI state after external editor closes
fn restore_tui<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>) -> Result<(), String> {
    execute!(stdout(), EnterAlternateScreen, Hide).map_err(|e| format!("Failed to enter alternate screen: {}", e))?;
    enable_raw_mode().map_err(|e| format!("Failed to enable raw mode: {}", e))?;
    terminal.clear().map_err(|e| format!("Failed to clear terminal: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_editor() {
        // Should find at least one editor on most systems
        let editor = detect_editor(None);
        assert!(editor.is_some());
    }

    #[test]
    fn test_is_editor_available() {
        // 'ls' should always be available on Unix systems
        #[cfg(unix)]
        assert!(is_editor_available("ls"));
        
        // 'nonexistent_editor_xyz' should not be available
        assert!(!is_editor_available("nonexistent_editor_xyz"));
    }
}
