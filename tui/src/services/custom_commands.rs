//! Custom Slash Commands
//!
//! Loads user-defined slash commands from markdown files in:
//! - `~/.stakpak/commands/` (global)
//! - `.stakpak/commands/`   (project-local, higher priority on name clash)
//!
//! Each `<name>.md` file becomes a `/<name>` command. The file body is the prompt
//! template sent as a user message when the command is invoked.
//!
//! Optional YAML front matter provides a description:
//! ```markdown
//! ---
//! description: Run a security audit
//! ---
//!
//! Perform a comprehensive security audit...
//! ```
//!
//! If no front matter is present, the first non-empty line (truncated to 60 chars)
//! is used as the description. Empty files are skipped.

use crate::app::{CommandSource, HelperCommand};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Maximum description length when derived from the first line of content.
const MAX_DESCRIPTION_LEN: usize = 60;

/// Load custom commands from both global and project-local directories.
///
/// Project-local commands (`.stakpak/commands/`) take priority over global
/// commands (`~/.stakpak/commands/`) when both define the same filename.
///
/// Returns a `Vec<HelperCommand>` with `source: CommandSource::Custom`.
pub fn load_custom_commands() -> Vec<HelperCommand> {
    let mut commands = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();

    // Project-local commands first (higher priority)
    let local_dir = PathBuf::from(".stakpak/commands");
    load_from_directory(&local_dir, &mut commands, &mut seen_names);

    // Global commands second (skipped if name already loaded)
    if let Some(home) = home_dir() {
        let global_dir = home.join(".stakpak/commands");
        load_from_directory(&global_dir, &mut commands, &mut seen_names);
    }

    commands
}

/// Scan a directory for `*.md` files and parse each into a `HelperCommand`.
fn load_from_directory(
    dir: &Path,
    commands: &mut Vec<HelperCommand>,
    seen_names: &mut HashSet<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return, // Directory doesn't exist or not readable — skip silently
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Only process .md files
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("md") {
            continue;
        }

        // Extract command name from filename (without extension)
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        // Validate filename: only alphanumeric, hyphens, underscores
        if !is_valid_command_name(&stem) {
            log::warn!(
                "Skipping custom command file with invalid name: {}",
                path.display()
            );
            continue;
        }

        // Skip if we've already seen this command name (project-local wins)
        let command_name = format!("/{stem}");
        if seen_names.contains(&command_name) {
            continue;
        }

        // Parse the file
        if let Some(helper) = parse_command_file(&path, &command_name) {
            seen_names.insert(command_name);
            commands.push(helper);
        }
    }
}

/// Parse a single `.md` file into a `HelperCommand`.
///
/// Returns `None` if the file is empty or unreadable.
fn parse_command_file(path: &Path, command_name: &str) -> Option<HelperCommand> {
    let content = std::fs::read_to_string(path).ok()?;
    let content = content.trim();

    if content.is_empty() {
        return None;
    }

    let (description, prompt_content) = extract_front_matter(content);

    // Fallback description: first non-empty line, truncated
    let description = description.unwrap_or_else(|| {
        let first_line = prompt_content
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("Custom command");
        let first_line = first_line.trim();
        if first_line.len() > MAX_DESCRIPTION_LEN {
            let truncated: String = first_line.chars().take(MAX_DESCRIPTION_LEN).collect();
            format!("{truncated}...")
        } else {
            first_line.to_string()
        }
    });

    Some(HelperCommand {
        command: command_name.to_string(),
        description,
        source: CommandSource::Custom {
            prompt_content: prompt_content.to_string(),
        },
    })
}

/// Extract YAML front matter from markdown content.
///
/// Front matter is delimited by `---` at the start:
/// ```text
/// ---
/// description: Some description
/// ---
/// Body content here
/// ```
///
/// Returns `(Option<description>, body_content)`.
fn extract_front_matter(content: &str) -> (Option<String>, &str) {
    // Must start with "---"
    if !content.starts_with("---") {
        return (None, content);
    }

    // Find the closing "---" that appears at the start of a line.
    // This avoids matching "---" embedded in description values.
    let after_first = &content[3..];
    let closing = after_first
        .match_indices("---")
        .find(|(pos, _)| {
            *pos == 0 || after_first.as_bytes().get(pos.wrapping_sub(1)) == Some(&b'\n')
        })
        .map(|(pos, _)| pos);

    match closing {
        Some(pos) => {
            let front_matter = after_first[..pos].trim();
            let body = after_first[pos + 3..].trim();

            // Parse description from front matter (simple key: value parsing)
            let description = front_matter.lines().find_map(|line| {
                let line = line.trim();
                if let Some(value) = line.strip_prefix("description:") {
                    let value = value.trim();
                    // Remove surrounding quotes if present
                    let value = value
                        .strip_prefix('"')
                        .and_then(|v| v.strip_suffix('"'))
                        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                        .unwrap_or(value);
                    if !value.is_empty() {
                        Some(value.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

            // If body is empty, use the full content (front matter only files make no sense as prompts)
            if body.is_empty() {
                (description, content)
            } else {
                (description, body)
            }
        }
        None => {
            // No closing --- found, treat entire content as body
            (None, content)
        }
    }
}

/// Validate that a command name contains only allowed characters.
///
/// Allowed: `a-z`, `A-Z`, `0-9`, `-`, `_`
fn is_valid_command_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Get the user's home directory.
fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_command_name() {
        assert!(is_valid_command_name("hello"));
        assert!(is_valid_command_name("hello-world"));
        assert!(is_valid_command_name("hello_world"));
        assert!(is_valid_command_name("Hello123"));
        assert!(!is_valid_command_name(""));
        assert!(!is_valid_command_name("hello world"));
        assert!(!is_valid_command_name("hello.world"));
        assert!(!is_valid_command_name("hello/world"));
    }

    #[test]
    fn test_extract_front_matter_with_description() {
        let content = r#"---
description: Run a security audit
---

Perform a comprehensive security audit of this project."#;

        let (desc, body) = extract_front_matter(content);
        assert_eq!(desc, Some("Run a security audit".to_string()));
        assert_eq!(
            body,
            "Perform a comprehensive security audit of this project."
        );
    }

    #[test]
    fn test_extract_front_matter_with_quoted_description() {
        let content = r#"---
description: "Check health status"
---

Check the health of all services."#;

        let (desc, body) = extract_front_matter(content);
        assert_eq!(desc, Some("Check health status".to_string()));
        assert_eq!(body, "Check the health of all services.");
    }

    #[test]
    fn test_extract_front_matter_no_front_matter() {
        let content = "Just a plain prompt with no front matter.";
        let (desc, body) = extract_front_matter(content);
        assert_eq!(desc, None);
        assert_eq!(body, content);
    }

    #[test]
    fn test_extract_front_matter_unclosed() {
        let content = "---\ndescription: test\nNo closing delimiter";
        let (desc, body) = extract_front_matter(content);
        assert_eq!(desc, None);
        assert_eq!(body, content);
    }

    #[test]
    fn test_extract_front_matter_empty_body() {
        let content = "---\ndescription: test\n---";
        let (desc, body) = extract_front_matter(content);
        assert_eq!(desc, Some("test".to_string()));
        // Empty body falls back to full content
        assert_eq!(body, content);
    }

    #[test]
    fn test_parse_command_file_with_front_matter() {
        let dir = std::env::temp_dir().join("stakpak_test_custom_cmds");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("audit.md");
        std::fs::write(
            &file,
            "---\ndescription: Run security audit\n---\n\nAudit the project.",
        )
        .ok();

        let result = parse_command_file(&file, "/audit");
        assert!(result.is_some());
        let cmd = result.unwrap();
        assert_eq!(cmd.command, "/audit");
        assert_eq!(cmd.description, "Run security audit");
        if let CommandSource::Custom { prompt_content } = &cmd.source {
            assert_eq!(prompt_content, "Audit the project.");
        } else {
            panic!("Expected CommandSource::Custom");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_command_file_no_front_matter() {
        let dir = std::env::temp_dir().join("stakpak_test_custom_cmds_2");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("deploy.md");
        std::fs::write(
            &file,
            "Deploy the application to production.\n\nUse kubectl apply.",
        )
        .ok();

        let result = parse_command_file(&file, "/deploy");
        assert!(result.is_some());
        let cmd = result.unwrap();
        assert_eq!(cmd.command, "/deploy");
        assert_eq!(cmd.description, "Deploy the application to production.");
        if let CommandSource::Custom { prompt_content } = &cmd.source {
            assert!(prompt_content.contains("Deploy the application"));
        } else {
            panic!("Expected CommandSource::Custom");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_command_file_empty() {
        let dir = std::env::temp_dir().join("stakpak_test_custom_cmds_3");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("empty.md");
        std::fs::write(&file, "   \n  \n").ok();

        let result = parse_command_file(&file, "/empty");
        assert!(result.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_description_truncation() {
        let dir = std::env::temp_dir().join("stakpak_test_custom_cmds_4");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("long.md");
        let long_line = "A".repeat(100);
        std::fs::write(&file, &long_line).ok();

        let result = parse_command_file(&file, "/long");
        assert!(result.is_some());
        let cmd = result.unwrap();
        assert!(cmd.description.len() <= MAX_DESCRIPTION_LEN + 3); // +3 for "..."
        assert!(cmd.description.ends_with("..."));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_extract_front_matter_dashes_in_description() {
        let content = "---\ndescription: Run --- diagnostic checks\n---\n\nBody here.";
        let (desc, body) = extract_front_matter(content);
        // The closing --- must be at the start of a line, so "---" inside
        // the description value should not be treated as the closing delimiter.
        assert_eq!(desc, Some("Run --- diagnostic checks".to_string()));
        assert_eq!(body, "Body here.");
    }

    #[test]
    fn test_extract_front_matter_dashes_in_body() {
        let content = "---\ndescription: Check health\n---\n\nUse the --- rule for sections.";
        let (desc, body) = extract_front_matter(content);
        assert_eq!(desc, Some("Check health".to_string()));
        assert_eq!(body, "Use the --- rule for sections.");
    }
}
