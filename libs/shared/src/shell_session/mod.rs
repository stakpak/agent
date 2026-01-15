//! Persistent Shell Session Management
//!
//! This module provides persistent shell sessions that maintain state (environment variables,
//! working directory, aliases) across multiple command executions.
//!
//! # Architecture
//!
//! - `ShellSession` trait: Common interface for local and remote sessions
//! - `LocalShellSession`: PTY-based local shell using `portable-pty`
//! - `RemoteShellSession`: SSH-based remote shell using `russh` with PTY allocation
//! - `ShellSessionManager`: Central manager for session lifecycle

mod local;
mod manager;
mod remote;
mod session;

pub use local::LocalShellSession;
pub use manager::{SessionInfo, ShellSessionConfig, ShellSessionManager};
pub use remote::RemoteShellSession;
pub use session::{CommandOutput, ShellSession, ShellSessionError};

/// Marker prefix for command completion detection (shared between local and remote)
pub(crate) const MARKER_PREFIX: &str = "__STAKPAK_CMD_END_";
pub(crate) const MARKER_SUFFIX: &str = "__";

/// Strip ANSI escape codes from a string
///
/// Handles CSI sequences (ESC [ ...) and OSC sequences (ESC ] ...)
pub(crate) fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Start of escape sequence
            if let Some(&next) = chars.peek() {
                if next == '[' {
                    // CSI sequence - consume until final byte (0x40-0x7E)
                    chars.next(); // consume '['
                    while let Some(&param) = chars.peek() {
                        if (0x40..=0x7E).contains(&(param as u32)) {
                            chars.next(); // consume final byte
                            break;
                        }
                        chars.next(); // consume parameter byte
                    }
                    continue;
                } else if next == ']' {
                    // OSC sequence - consume until ST or BEL
                    chars.next(); // consume ']'
                    while let Some(osc_char) = chars.next() {
                        if osc_char == '\x07' || osc_char == '\x1b' {
                            if osc_char == '\x1b' {
                                chars.next(); // consume '\\' of ST
                            }
                            break;
                        }
                    }
                    continue;
                }
            }
        }
        result.push(c);
    }

    result
}

/// Clean shell output by removing command echo, markers, and shell artifacts
///
/// This is shared logic used by both local and remote sessions
pub(crate) fn clean_shell_output(raw_output: &str, command: &str, marker: &str) -> String {
    // First, strip ANSI escape codes
    let stripped = strip_ansi_codes(raw_output);

    let mut lines: Vec<&str> = stripped.lines().collect();

    // Remove lines containing ANY marker (current or leftover from previous commands)
    // This ensures our implementation is transparent even if buffer has leftover data
    lines.retain(|line| {
        !line.contains(marker)
            && !line.contains(MARKER_PREFIX)
            && !line.contains("__STAKPAK_CMD_END_")
    });

    // Remove the echoed command (first line often contains it)
    if let Some(first) = lines.first()
        && (first.trim() == command.trim() || first.contains(command.trim())) {
            lines.remove(0);
        }

    // Remove empty lines at start and end
    while lines.first().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.remove(0);
    }
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }

    // Remove shell prompt lines (common patterns)
    lines.retain(|line| {
        let trimmed = line.trim();
        // Skip typical prompt patterns - check for standalone prompts
        !(trimmed == "$"
            || trimmed == "#"
            || trimmed == ">"
            || trimmed == "%"
            || trimmed.ends_with("$ ")
            || trimmed.ends_with("# ")
            || trimmed.ends_with("> ")
            || trimmed.ends_with("% ")
            || (trimmed.starts_with("[") && trimmed.contains("]$"))
            || trimmed.contains(" % ")) // zsh prompt pattern
    });

    lines.join("\n")
}
