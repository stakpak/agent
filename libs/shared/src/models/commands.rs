//! Custom commands types and configuration.
//!
//! This module provides the shared types for custom slash commands used across
//! the CLI, TUI, and API crates. Commands can come from multiple sources:
//! - Predefined (embedded in binary or fetched remotely)
//! - Personal files (~/.stakpak/commands/cmd_*.md)
//! - Project files (./.stakpak/commands/cmd_*.md)
//! - Config definitions (explicit file path mappings)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A loaded command ready for execution.
///
/// Commands are loaded from various sources and normalized into this struct
/// for use in the TUI helper dropdown and command execution.
#[derive(Debug, Clone)]
pub struct CustomCommand {
    /// Command identifier (e.g., "/security-review" or "/cmd:deploy")
    pub id: String,
    /// Human-readable description (extracted from first markdown heading)
    pub description: String,
    /// Full command content (markdown prompt)
    pub content: String,
    /// Where this command was loaded from
    pub source: CommandSource,
}

/// Source of a custom command, used for precedence and debugging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSource {
    /// Embedded in binary (libs/api/src/commands/*.md)
    Predefined,
    /// Fetched from remote API and cached
    PredefinedRemote,
    /// User's personal commands (~/.stakpak/commands/cmd_*.md)
    PersonalFile,
    /// Project-specific commands (./.stakpak/commands/cmd_*.md)
    ProjectFile,
    /// Explicit definition in config file
    ConfigDefinition,
}

/// Configuration for custom commands filtering and definitions.
///
/// This struct is serialized/deserialized from the config file and controls
/// which commands are loaded and how they're resolved.
///
/// # Example Configuration
///
/// ```toml
/// [profiles.default.commands]
/// # Include only these commands (supports glob patterns, empty = all allowed)
/// include = ["security-*", "write-rfc", "deploy-*"]
/// # Exclude specific commands (supports glob patterns, empty = none excluded)
/// exclude = ["*-deprecated", "test-*", "*-wip"]
///
/// # Command definitions: <name> = "<path to file>"
/// # These override cmd_*.md files in commands/ directories
/// [profiles.default.commands.definitions]
/// security-review = "~/.stakpak/prompts/security-review.md"
/// quick-fix = ".stakpak/prompts/quick-fix.md"
/// ```
///
/// # Filtering Logic
///
/// 1. If `include` is set and non-empty, only commands matching at least one pattern are loaded
/// 2. If `exclude` is set and non-empty, commands matching any pattern are filtered out
/// 3. `exclude` takes precedence over `include` (if both match, command is excluded)
/// 4. Supports glob patterns: `*` (any chars), `?` (single char), `[abc]` (char class)
/// 5. Filters apply to both file-based (cmd_*.md) and definition-based commands
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CommandsConfig {
    /// Include only these commands by name (supports glob patterns, empty = all allowed)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    /// Exclude specific commands by name (supports glob patterns, empty = none excluded)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
    /// Command definitions: name → file path
    /// Example: security-review = "~/.stakpak/prompts/security.md" → /cmd:security-review
    /// These override cmd_*.md files in commands/ directories.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub definitions: HashMap<String, String>,
}

impl CommandsConfig {
    /// Check if a command should be loaded based on include/exclude patterns.
    ///
    /// Returns `true` if the command passes both include and exclude filters.
    pub fn should_load(&self, command_name: &str) -> bool {
        self.matches_include(command_name) && self.matches_exclude(command_name)
    }

    fn matches_include(&self, name: &str) -> bool {
        match &self.include {
            Some(patterns) if !patterns.is_empty() => {
                patterns.iter().any(|p| crate::utils::matches_glob(name, p))
            }
            _ => true, // No include filter = allow all
        }
    }

    fn matches_exclude(&self, name: &str) -> bool {
        match &self.exclude {
            Some(patterns) if !patterns.is_empty() => {
                !patterns.iter().any(|p| crate::utils::matches_glob(name, p))
            }
            _ => true, // No exclude filter = exclude none
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_load_no_filters() {
        let config = CommandsConfig::default();
        assert!(config.should_load("any-command"));
        assert!(config.should_load("security-review"));
    }

    #[test]
    fn test_should_load_include_only() {
        let config = CommandsConfig {
            include: Some(vec!["security-*".to_string(), "code-*".to_string()]),
            exclude: None,
            definitions: HashMap::new(),
        };
        assert!(config.should_load("security-review"));
        assert!(config.should_load("code-review"));
        assert!(!config.should_load("write-tests"));
    }

    #[test]
    fn test_should_load_exclude_only() {
        let config = CommandsConfig {
            include: None,
            exclude: Some(vec!["*-deprecated".to_string()]),
            definitions: HashMap::new(),
        };
        assert!(config.should_load("security-review"));
        assert!(!config.should_load("old-deprecated"));
    }

    #[test]
    fn test_should_load_exclude_takes_precedence() {
        let config = CommandsConfig {
            include: Some(vec!["security-*".to_string()]),
            exclude: Some(vec!["security-old".to_string()]),
            definitions: HashMap::new(),
        };
        assert!(config.should_load("security-review"));
        assert!(!config.should_load("security-old")); // Excluded even though it matches include
    }
}
