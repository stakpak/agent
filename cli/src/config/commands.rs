//! Custom commands filtering configuration.

use serde::{Deserialize, Serialize};

/// Configuration for filtering which custom commands are loaded.
///
/// # Example Configuration
///
/// ```toml
/// [commands]
/// # Include only these commands (supports glob patterns, empty = all allowed)
/// include = ["security-*", "write-rfc", "deploy-*"]
///
/// # Exclude specific commands (supports glob patterns, empty = none excluded)
/// exclude = ["*-deprecated", "test-*", "*-wip"]
/// ```
///
/// # Filtering Logic
///
/// 1. If `include` is set and non-empty, only commands matching at least one pattern are loaded
/// 2. If `exclude` is set and non-empty, commands matching any pattern are filtered out
/// 3. `exclude` takes precedence over `include` (if both match, command is excluded)
/// 4. Supports glob patterns: `*` (any chars), `?` (single char), `[abc]` (char class)
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CommandsConfig {
    /// Include only these commands by name (supports glob patterns, empty = all allowed)
    pub include: Option<Vec<String>>,
    /// Exclude specific commands by name (supports glob patterns, empty = none excluded)
    pub exclude: Option<Vec<String>>,
}
