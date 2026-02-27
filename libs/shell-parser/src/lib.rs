//! Shell command parsing and hierarchical scope resolution.
//!
//! This crate parses shell command strings using `tree-sitter-bash`

mod parse;

pub use parse::{ParsedCommand, parse};
