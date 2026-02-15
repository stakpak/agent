//! Predefined slash commands embedded in the binary.
//!
//! These commands are available immediately without any configuration.
//! They use the `/slash:` prefix to distinguish from user-created `/cmd:` commands.

/// Predefined slash commands: (name, content)
/// Available as `/slash:{name}` in the TUI.
pub const PREDEFINED_COMMANDS: &[(&str, &str)] = &[
    ("security-review", include_str!("security-review.md")),
    ("code-review", include_str!("code-review.md")),
    ("explain", include_str!("explain.md")),
    ("quick-fix", include_str!("quick-fix.md")),
    ("write-tests", include_str!("write-tests.md")),
];
