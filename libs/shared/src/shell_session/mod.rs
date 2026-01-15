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

    // Remove control characters that PTY emits
    let cleaned = stripped.replace('\r', "").replace('\x08', ""); // backspace

    let mut lines: Vec<&str> = cleaned.lines().collect();

    // Remove lines containing ANY marker (current or leftover from previous commands)
    // This ensures our implementation is transparent even if buffer has leftover data
    lines.retain(|line| {
        !line.contains(marker)
            && !line.contains(MARKER_PREFIX)
            && !line.contains("__STAKPAK_CMD_END_")
    });

    // Remove the echoed command (first line often contains it)
    if let Some(first) = lines.first()
        && (first.trim() == command.trim() || first.contains(command.trim()))
    {
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
        // Skip typical prompt patterns
        !is_prompt_line(trimmed)
    });

    lines.join("\n")
}

/// Check if a line looks like a shell prompt or contains prompt + echoed command
fn is_prompt_line(trimmed: &str) -> bool {
    // Empty or whitespace-only
    if trimmed.is_empty() {
        return false; // Let caller decide on empty lines
    }

    // Lines that are just whitespace/control chars (leftover from ANSI stripping)
    if trimmed.chars().all(|c| c.is_whitespace() || c == '\r') {
        return true;
    }

    // Standalone prompt characters
    if trimmed == "$" || trimmed == "#" || trimmed == ">" || trimmed == "%" {
        return true;
    }

    // Check for user@host pattern which indicates a prompt line
    // This catches: "user@host dir % command" or "user@host:dir$ command"
    if trimmed.contains('@') {
        // Look for prompt endings: % $ # >
        // The prompt is typically: user@host path % or user@host:path$
        for prompt_char in ['%', '$', '#'] {
            if let Some(pos) = trimmed.rfind(prompt_char) {
                // Check if this looks like a prompt (has @ before the prompt char)
                let before_prompt = &trimmed[..pos];
                if before_prompt.contains('@') {
                    return true;
                }
            }
        }
    }

    // Lines ending with prompt patterns (standalone prompts)
    if trimmed.ends_with("$ ")
        || trimmed.ends_with("# ")
        || trimmed.ends_with("> ")
        || trimmed.ends_with("% ")
        || trimmed.ends_with('$')
        || trimmed.ends_with('#')
        || trimmed.ends_with('%')
    {
        // Short lines ending with prompt chars are likely prompts
        if trimmed.len() < 50 || trimmed.starts_with('[') {
            return true;
        }
    }

    // Bash-style [user@host dir]$ pattern
    if trimmed.starts_with('[') && (trimmed.contains("]$") || trimmed.contains("]#")) {
        return true;
    }

    false
}
