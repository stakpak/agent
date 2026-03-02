//! Shell command parsing and hierarchical scope resolution.
//!
//! This crate parses shell command strings using `tree-sitter-bash`

mod matcher;
mod parse;

pub use matcher::matches_pattern;
pub use parse::{ParsedCommand, parse};
