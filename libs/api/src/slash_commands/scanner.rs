//! Command scanning from multiple sources.
//!
//! This module provides the `scan_commands` function that loads custom commands
//! from predefined, personal, project, and config definition sources.

use stakpak_shared::models::commands::{CommandSource, CommandsConfig, CustomCommand};
use std::collections::HashMap;

/// Max size for custom command files (64 KiB)
const MAX_CUSTOM_COMMAND_BYTES: u64 = 64 * 1024;

/// File prefix for custom command markdown files (on disk). Aligns with CMD_PREFIX: cmd_*.md → /cmd:*
const CMD_FILE_PREFIX: &str = "cmd_";

/// Slash prefix for user-created commands in the TUI (display and id)
pub const CMD_PREFIX: &str = "/cmd:";

/// Scan for custom commands from four sources (in order of precedence):
/// 1. Predefined commands: Embedded in binary as /* (e.g., /security-review) - lowest precedence
/// 2. Personal files: ~/.stakpak/commands/cmd_*.md as /cmd:*
/// 3. Project files: ./.stakpak/commands/cmd_*.md as /cmd:* (overrides personal)
/// 4. Config definitions: config.definitions file references as /cmd:* (highest precedence)
///
/// Predefined commands use no prefix (like /init), user commands use /cmd: prefix.
/// User can override predefined by creating cmd_security-review.md → /cmd:security-review
///
/// If `commands_config` is `Some`, filtering is applied based on include/exclude patterns.
/// Filters apply to predefined and user commands (but NOT to /init, /help, etc.).
pub fn scan_commands(commands_config: Option<&CommandsConfig>) -> Vec<CustomCommand> {
    let mut by_id: HashMap<String, CustomCommand> = HashMap::new();

    // 1. Load predefined commands (lowest precedence, no prefix like /init)
    for (name, content) in super::PREDEFINED_COMMANDS {
        // Apply filters to predefined commands
        if let Some(config) = commands_config
            && !config.should_load(name)
        {
            continue;
        }

        let id = format!("/{}", name); // No prefix, like /init
        let description = extract_markdown_title(content).unwrap_or_else(|| name.to_string());

        by_id.insert(
            id.clone(),
            CustomCommand {
                id,
                description,
                content: content.to_string(),
                source: CommandSource::Predefined,
            },
        );
    }

    // 2. Load from files: personal first, then project (project overwrites personal)
    // These use /cmd: prefix (user-created)
    let personal = dirs::home_dir().map(|h| h.join(".stakpak").join("commands"));
    let project = std::env::current_dir()
        .ok()
        .map(|cwd| cwd.join(".stakpak").join("commands"));

    // Process personal files first
    if let Some(dir) = personal {
        load_commands_from_dir(
            &dir,
            commands_config,
            CommandSource::PersonalFile,
            &mut by_id,
        );
    }

    // Process project files (overrides personal)
    if let Some(dir) = project {
        load_commands_from_dir(
            &dir,
            commands_config,
            CommandSource::ProjectFile,
            &mut by_id,
        );
    }

    // 3. Load from definitions (file references, highest precedence - override everything)
    if let Some(config) = commands_config {
        for (name, file_path) in &config.definitions {
            // Apply filters to definition-based commands too
            if !config.should_load(name) {
                continue;
            }

            // Expand ~ to home directory
            let expanded_path = if let Some(stripped) = file_path.strip_prefix("~/") {
                if let Some(home) = dirs::home_dir() {
                    home.join(stripped)
                } else {
                    std::path::PathBuf::from(file_path)
                }
            } else {
                std::path::PathBuf::from(file_path)
            };

            // Read file content
            let content = match std::fs::read_to_string(&expanded_path) {
                Ok(c) => c,
                Err(_) => continue, // Skip if file can't be read
            };

            let content = content.trim();
            if content.is_empty() {
                continue;
            }

            let id = format!("{}{}", CMD_PREFIX, name);
            let description = extract_markdown_title(content).unwrap_or_else(|| name.to_string());

            // Definitions override file-based and built-in commands
            by_id.insert(
                id.clone(),
                CustomCommand {
                    id,
                    description,
                    content: content.to_string(),
                    source: CommandSource::ConfigDefinition,
                },
            );
        }
    }

    let mut commands: Vec<_> = by_id.into_values().collect();
    commands.sort_by(|a, b| a.id.cmp(&b.id));
    commands
}

/// Load commands from a directory of cmd_*.md files
fn load_commands_from_dir(
    dir: &std::path::Path,
    commands_config: Option<&CommandsConfig>,
    source: CommandSource,
    by_id: &mut HashMap<String, CustomCommand>,
) {
    if !dir.is_dir() {
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };

        // Only process files with cmd_ prefix
        if !filename.starts_with(CMD_FILE_PREFIX) {
            continue;
        }

        // Extract command name: "cmd_create-component.md" -> "create-component"
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(command_name) = stem.strip_prefix(CMD_FILE_PREFIX) else {
            continue;
        };
        if command_name.is_empty() {
            continue;
        }

        let id = format!("{}{command_name}", CMD_PREFIX);

        // Apply commands config filter if present
        if let Some(config) = commands_config
            && !config.should_load(command_name)
        {
            continue;
        }

        // Size check via metadata (avoids reading large files)
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.len() > MAX_CUSTOM_COMMAND_BYTES {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if content.trim().is_empty() {
            continue;
        }

        // Extract title from first markdown heading (# Title) if present
        let description =
            extract_markdown_title(&content).unwrap_or_else(|| command_name.to_string());

        let cmd = CustomCommand {
            id: id.clone(),
            description,
            content: content.trim().to_string(),
            source,
        };
        by_id.insert(id, cmd);
    }
}

/// Extract the first markdown heading (# Title) from content as the command title
fn extract_markdown_title(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("# ") {
            let title = title.trim();
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_markdown_title() {
        assert_eq!(
            extract_markdown_title("# Security Review\n\nContent here"),
            Some("Security Review".to_string())
        );
        assert_eq!(
            extract_markdown_title("  # Trimmed Title  \n\nContent"),
            Some("Trimmed Title".to_string())
        );
        assert_eq!(extract_markdown_title("No heading here"), None);
        assert_eq!(extract_markdown_title("## Not H1"), None);
    }

    #[test]
    fn test_scan_commands_loads_predefined() {
        let commands = scan_commands(None);
        // Should have at least the predefined commands
        assert!(!commands.is_empty());
        // Check for a known predefined command
        assert!(commands.iter().any(|c| c.id == "/security-review"));
    }

    #[test]
    fn test_scan_commands_with_exclude_filter() {
        let config = CommandsConfig {
            include: None,
            exclude: Some(vec!["security-*".to_string()]),
            definitions: std::collections::HashMap::new(),
        };
        let commands = scan_commands(Some(&config));
        // security-review should be excluded
        assert!(!commands.iter().any(|c| c.id == "/security-review"));
        // Other commands should still be present
        assert!(commands.iter().any(|c| c.id == "/code-review"));
    }
}
