//! Built-in prompts and slash commands.
//!
//! This module contains prompts embedded in the binary for immediate use.

/// Built-in slash commands: (name, content)
/// These are available immediately without any configuration.
pub const BUILTIN_COMMANDS: &[(&str, &str)] = &[
    ("security-review", include_str!("security-review.md")),
    ("code-review", include_str!("code-review.md")),
    ("explain", include_str!("explain.md")),
    ("quick-fix", include_str!("quick-fix.md")),
    ("write-tests", include_str!("write-tests.md")),
];

/// Init prompt for `stakpak init` command
pub const INIT_PROMPT: &str = include_str!("init.v1.md");

/// Session title generator prompt
pub const SESSION_TITLE_GENERATOR: &str = include_str!("session_title_generator.v1.txt");
