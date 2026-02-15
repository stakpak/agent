//! Custom slash commands system.
//!
//! This module provides:
//! - Predefined slash commands embedded in the binary
//! - Command scanning from multiple sources (files, config definitions)
//! - Command caching for remote-fetched predefined commands (future)
//!
//! # Command Sources (Precedence Order)
//!
//! 1. Config definitions (highest) - explicit file path mappings in config
//! 2. Project files - `./.stakpak/commands/cmd_*.md`
//! 3. Personal files - `~/.stakpak/commands/cmd_*.md`
//! 4. Predefined commands (lowest) - embedded in binary
//!
//! # Example
//!
//! ```ignore
//! use stakpak_api::slash_commands::scan_commands;
//! use stakpak_shared::models::commands::CommandsConfig;
//!
//! let config = CommandsConfig::default();
//! let commands = scan_commands(Some(&config));
//! ```

mod predefined;
pub mod scanner;

pub use predefined::PREDEFINED_COMMANDS;
pub use scanner::{CMD_PREFIX, scan_commands};
