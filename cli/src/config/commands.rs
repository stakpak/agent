//! Custom commands filtering and definitions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for custom commands: filtering and file-based definitions.
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
/// # These are different from user-created cmd_*.md files in commands/ directories
/// [profiles.default.commands.definitions]
/// security-review = "~/.stakpak/prompts/security-review.md"
/// quick-fix = ".stakpak/prompts/quick-fix.md"
/// explain = "prompts/explain.txt"
/// ```
///
/// # Filtering Logic
///
/// 1. If `include` is set and non-empty, only commands matching at least one pattern are loaded
/// 2. If `exclude` is set and non-empty, commands matching any pattern are filtered out
/// 3. `exclude` takes precedence over `include` (if both match, command is excluded)
/// 4. Supports glob patterns: `*` (any chars), `?` (single char), `[abc]` (char class)
/// 5. Filters apply to both file-based (cmd_*.md) and definition-based commands
///
/// # Precedence
///
/// Definition file references override cmd_*.md files with the same name.
/// Example: `security-review = "..."` overrides `cmd_security-review.md`
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
