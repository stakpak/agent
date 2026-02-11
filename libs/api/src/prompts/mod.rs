//! Built-in prompts and slash commands.
//!
//! This module contains prompts embedded in the binary for immediate use.

/// Init prompt for `stakpak init` command
pub const INIT_PROMPT: &str = include_str!("init.v1.md");

/// Session title generator prompt
pub const SESSION_TITLE_GENERATOR: &str = include_str!("session_title_generator.v1.txt");
