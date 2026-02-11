//! Custom commands filtering and inline definitions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for custom commands: filtering and inline definitions.
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
/// # Inline command definitions (override file-based commands)
/// [profiles.default.commands.definitions]
/// security-review = """
/// # Security Review
/// Perform a comprehensive security review of the codebase...
/// """
/// quick-fix = """
/// # Quick Fix
/// Analyze the error and provide a targeted fix...
/// """
/// ```
///
/// # Filtering Logic
///
/// 1. If `include` is set and non-empty, only commands matching at least one pattern are loaded
/// 2. If `exclude` is set and non-empty, commands matching any pattern are filtered out
/// 3. `exclude` takes precedence over `include` (if both match, command is excluded)
/// 4. Supports glob patterns: `*` (any chars), `?` (single char), `[abc]` (char class)
/// 5. Filters apply to both file-based and inline commands
///
/// # Precedence
///
/// Inline definitions override file-based commands with the same name.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CommandsConfig {
    /// Include only these commands by name (supports glob patterns, empty = all allowed)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    /// Exclude specific commands by name (supports glob patterns, empty = none excluded)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
    /// Inline command definitions: command name → markdown content
    /// These override file-based commands with the same name.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub definitions: HashMap<String, String>,
}
