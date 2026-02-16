//! Custom slash commands: predefined (embedded), personal/project dirs, config definitions.

mod predefined;
pub mod scanner;

pub use predefined::PREDEFINED_COMMANDS;
pub use scanner::{CMD_PREFIX, scan_commands};
