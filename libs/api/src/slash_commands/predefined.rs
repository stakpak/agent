//! Predefined slash commands embedded in the binary.
//!
//! This module includes auto-generated code from build.rs that scans
//! `src/slash_commands/*.md` files (excluding `cmd_*.md` user templates).
//!
//! ## Adding New Commands
//!
//! Simply create a new `.md` file in `src/slash_commands/`:
//! - `my-command.md` → becomes `/my-command` in the TUI
//!
//! No need to edit this file or `mod.rs` - the build script handles it automatically.
//!
//! ## File Naming Convention
//!
//! - `foo-bar.md` → `/foo-bar` (predefined command, embedded in binary)
//! - `cmd_*.md` → excluded (user command templates for runtime scanning)

// Include the auto-generated PREDEFINED_COMMANDS constant
include!(concat!(env!("OUT_DIR"), "/predefined_generated.rs"));
