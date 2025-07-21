use regex::Regex;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use tokio::sync::mpsc;

use crate::services::bash_block::preprocess_terminal_output;

fn is_input_prompt(line: &str) -> bool {
    let trimmed = line.trim();

    // Empty or very short lines are not prompts
    if trimmed.len() < 2 {
        return false;
    }

    // Shell prompts - check for user@host patterns too
    if trimmed.ends_with("$ ")
        || trimmed.ends_with("# ")
        || trimmed.ends_with("> ")
        || trimmed.ends_with("% ")
    {
        return true;
    }

    // Interactive shell patterns
    static SHELL_REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    let shell_regex = SHELL_REGEX
        .get_or_init(|| Regex::new(r"^[a-zA-Z0-9_\-\.]+@[a-zA-Z0-9_\-\.]+[:#\$%>]\s*$").ok());

    if let Some(regex) = shell_regex {
        if regex.is_match(trimmed) {
            return true;
        }
    }

    // Command continuation prompts
    if trimmed == ">" || trimmed == ">>" || trimmed.starts_with("... ") {
        return true;
    }

    // Interactive application prompts
    let lower = trimmed.to_lowercase();
    if lower.starts_with("(") && lower.ends_with(")") && lower.len() > 4 {
        return true; // (Pdb), (gdb), etc.
    }

    // Generic input patterns - be more specific
    if trimmed.ends_with(": ")
        && !lower.contains("error")
        && !lower.contains("warning")
        && !lower.contains("info")
        && !lower.starts_with("http")
        && trimmed.len() < 100
    {
        // Additional check: likely interactive if it's asking for something
        if lower.contains("enter") || lower.contains("input") || lower.contains("type") {
            return true;
        }
    }

    false
}

/// Detects sensitive credential-related prompts
fn contains_sensitive_terms(line: &str) -> bool {
    let lower_line = line.to_lowercase();

    // Must look like a prompt, not just contain sensitive words
    let looks_like_prompt = lower_line.ends_with(": ")
        || lower_line.ends_with("? ")
        || lower_line.contains("enter ")
        || lower_line.contains("provide ")
        || lower_line.contains("input ")
        || lower_line.contains("please ");

    if !looks_like_prompt {
        return false;
    }

    let sensitive_terms = [
        "password",
        "passphrase",
        "secret",
        "pin",
        "code",
        "api_key",
        "api key",
        "access_key",
        "access key",
        "token",
        "auth",
        "authentication",
        "credential",
        "cred",
        "private key",
        "private_key",
        "ssh key",
        "ssh_key",
        "gpg key",
        "gpg_key",
        "pgp key",
        "pgp_key",
        "certificate",
        "cert",
        "pem",
        "p12",
        "pfx",
        "keystore",
        "truststore",
        "wallet",
        "seed phrase",
        "mnemonic",
        "recovery",
        "backup phrase",
        "master password",
        "unlock",
        "decrypt",
        "2fa",
        "mfa",
        "totp",
        "otp",
        "verification",
    ];

    sensitive_terms.iter().any(|term| lower_line.contains(term))
}

/// Detects confirmation prompts (y/n, yes/no, etc.)
fn is_confirmation_prompt(line: &str) -> bool {
    let lower_line = line.to_lowercase();
    let trimmed = line.trim();

    // Direct y/n patterns
    static CONFIRM_REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    let confirm_regex = CONFIRM_REGEX.get_or_init(|| {
        Regex::new(
            r"(?i)\[?\s*([yn])\s*/\s*([yn])\s*\]?:?\s*$|\[?\s*(yes|no)\s*/\s*(yes|no)\s*\]?:?\s*$",
        )
        .ok()
    });

    if let Some(regex) = confirm_regex {
        if regex.is_match(trimmed) {
            return true;
        }
    }

    // Question patterns with confirmation words
    if lower_line.ends_with("?") || lower_line.ends_with("? ") {
        let confirmation_words = [
            "continue",
            "proceed",
            "confirm",
            "sure",
            "agree",
            "delete",
            "remove",
            "overwrite",
            "replace",
            "install",
            "upgrade",
            "downgrade",
            "restart",
            "reboot",
            "shutdown",
            "abort",
            "cancel",
            "skip",
            "retry",
            "force",
            "accept",
            "approve",
            "allow",
            "permit",
            "enable",
            "disable",
            "destroy",
            "purge",
            "reset",
            "clear",
        ];

        if confirmation_words
            .iter()
            .any(|word| lower_line.contains(word))
        {
            return true;
        }
    }

    // Common prompt patterns
    let prompt_patterns = [
        "do you want to",
        "would you like to",
        "are you sure",
        "should i",
        "shall i",
        "may i",
        "can i",
        "press any key",
        "press enter",
        "hit enter",
        "type y",
        "type yes",
        "enter y",
        "enter yes",
        "confirm by typing",
        "to confirm",
    ];

    prompt_patterns
        .iter()
        .any(|pattern| lower_line.contains(pattern))
}

/// Detects various waiting/loading states that might need interaction
fn is_waiting_prompt(line: &str) -> bool {
    let lower_line = line.to_lowercase();
    let trimmed = line.trim();

    // Loading patterns that might pause for input
    let waiting_patterns = [
        "loading...",
        "please wait",
        "processing...",
        "connecting...",
        "downloading...",
        "installing...",
        "updating...",
        "press any key",
        "press enter",
        "hit enter",
        "press return",
        "press space",
        "press esc",
        "press ctrl+c",
        "waiting for",
        "paused",
        "suspended",
        "more --",
        "-- more --",
        "continue?",
        "next?",
        "more?",
        "help?",
        "(press h for help)",
        "(? for help)",
        "enter to continue",
        "space to continue",
        "q to quit",
        "pager:",
        "less:",
        "more:",
        "debugger",
        "breakpoint",
        "debugging",
    ];

    if waiting_patterns
        .iter()
        .any(|pattern| lower_line.contains(pattern))
    {
        return true;
    }

    // Interactive program indicators
    if trimmed.starts_with("(") && trimmed.ends_with(")") {
        let interactive_apps = ["pdb", "gdb", "lldb", "node", "python", "irb", "repl"];
        if interactive_apps.iter().any(|app| lower_line.contains(app)) {
            return true;
        }
    }

    // Progress indicators that might stop
    static PROGRESS_REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    let progress_regex = PROGRESS_REGEX
        .get_or_init(|| Regex::new(r"^\s*\[[\s=>#\-\.]*\]\s*\d+%?\s*$|^\s*\d+%\s*$").ok());

    // Don't flag normal progress bars, only ones that might pause
    if let Some(regex) = progress_regex {
        if regex.is_match(trimmed) && lower_line.contains("pause") {
            return true;
        }
    }

    // Menu selections
    if (trimmed.starts_with("1)")
        || trimmed.starts_with("a)")
        || trimmed.starts_with("[1]")
        || trimmed.starts_with("(1)"))
        && (lower_line.contains("select")
            || lower_line.contains("choose")
            || lower_line.contains("option")
            || lower_line.contains("menu"))
    {
        return true;
    }

    false
}

/// Master function combining all detection methods
fn is_interactive_prompt(line: &str) -> bool {
    // Skip obviously non-interactive lines
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.len() > 500 {
        return false;
    }

    // Skip common log patterns
    let lower = trimmed.to_lowercase();
    if lower.starts_with("info:")
        || lower.starts_with("debug:")
        || lower.starts_with("warn:")
        || lower.starts_with("error:")
        || lower.starts_with("[info]")
        || lower.starts_with("[debug]")
        || lower.starts_with("[warn]")
        || lower.starts_with("[error]")
        || lower.contains("timestamp")
        || lower.contains("iso8601")
    {
        return false;
    }

    // Check all prompt types
    is_input_prompt(line)
        || contains_sensitive_terms(line)
        || is_confirmation_prompt(line)
        || is_waiting_prompt(line)
}

/// The shell prompt prefix used in the TUI
pub const SHELL_PROMPT_PREFIX: &str = "$ ";

#[derive(Debug, Clone)]
pub enum ShellEvent {
    Output(String),
    Error(String),
    InputRequest(String), // For sensitive input prompts (passwords, secrets, keys, etc.)
    Completed(i32),       // Exit code
    Clear,                // Clear the output display
}

#[derive(Clone)]
pub struct ShellCommand {
    pub id: String,
    pub command: String,
    pub stdin_tx: mpsc::Sender<String>,
}

/// Check if a command is a clear command (with optional arguments/whitespace)
fn is_clear_command(command: &str) -> bool {
    let trimmed = command.trim();
    trimmed == "clear" || trimmed.starts_with("clear ") || trimmed.starts_with("clear\t")
}

/// Run a shell command in the background while keeping the TUI active
pub fn run_background_shell_command(
    command: String,
    output_tx: mpsc::Sender<ShellEvent>,
) -> ShellCommand {
    let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(100);
    let command_id = uuid::Uuid::new_v4().to_string();

    let shell_cmd = ShellCommand {
        id: command_id.clone(),
        command: command.clone(),
        stdin_tx: stdin_tx.clone(),
    };

    // Check if this is a clear command
    if is_clear_command(&command) {
        // Send clear event instead of running the command
        let output_tx_clone = output_tx.clone();
        std::thread::spawn(move || {
            let _ = output_tx_clone.blocking_send(ShellEvent::Clear);
            let _ = output_tx_clone.blocking_send(ShellEvent::Completed(0));
        });
        return shell_cmd;
    }

    // Spawn command in a separate thread
    std::thread::spawn(move || {
        let child = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args(["/C", &command])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        } else {
            Command::new("sh")
                .args(["-c", &command])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        };

        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                let _ = output_tx
                    .blocking_send(ShellEvent::Error(format!("Failed to spawn command: {}", e)));
                return;
            }
        };

        // Handle stdin in a separate thread
        if let Some(mut stdin) = child.stdin.take() {
            std::thread::spawn(move || {
                while let Some(input) = stdin_rx.blocking_recv() {
                    if let Err(e) = writeln!(stdin, "{}", input) {
                        eprintln!("Failed to write to stdin: {}", e);
                        break;
                    }
                    if let Err(e) = stdin.flush() {
                        eprintln!("Failed to flush stdin: {}", e);
                        break;
                    }
                }
            });
        }

        // Handle stdout
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            let tx_clone = output_tx.clone();
            std::thread::spawn(move || {
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            let line = preprocess_terminal_output(&line);
                            // Check for sensitive input prompts
                            if is_interactive_prompt(&line) {
                                let _ =
                                    tx_clone.blocking_send(ShellEvent::InputRequest(line.clone()));
                            }
                            let _ = tx_clone.blocking_send(ShellEvent::Output(line));
                        }
                        Err(e) => {
                            let _ = tx_clone
                                .blocking_send(ShellEvent::Error(format!("Read error: {}", e)));
                        }
                    }
                }
            });
        }

        // Handle stderr
        if let Some(stderr) = child.stderr.take() {
            let reader = BufReader::new(stderr);
            let tx_clone = output_tx.clone();
            std::thread::spawn(move || {
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            // Check for sensitive input prompts in stderr too
                            let line = preprocess_terminal_output(&line);
                            if is_interactive_prompt(&line) {
                                let _ =
                                    tx_clone.blocking_send(ShellEvent::InputRequest(line.clone()));
                            }
                            let _ = tx_clone.blocking_send(ShellEvent::Error(line));
                        }
                        Err(e) => {
                            let _ = tx_clone
                                .blocking_send(ShellEvent::Error(format!("Read error: {}", e)));
                        }
                    }
                }
            });
        }

        // Wait for process to complete
        match child.wait() {
            Ok(status) => {
                let code = status.code().unwrap_or(-1);
                let _ = output_tx.blocking_send(ShellEvent::Completed(code));
            }
            Err(e) => {
                let _ = output_tx.blocking_send(ShellEvent::Error(format!("Wait error: {}", e)));
                let _ = output_tx.blocking_send(ShellEvent::Completed(-1));
            }
        }
    });

    shell_cmd
}

#[cfg(unix)]
pub fn run_pty_command(
    command: String,
    output_tx: mpsc::Sender<ShellEvent>,
) -> Result<ShellCommand, Box<dyn std::error::Error>> {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::io::Read;

    let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(100);
    let command_id = uuid::Uuid::new_v4().to_string();

    let shell_cmd = ShellCommand {
        id: command_id.clone(),
        command: command.clone(),
        stdin_tx: stdin_tx.clone(),
    };

    // Check if this is a clear command
    if is_clear_command(&command) {
        // Send clear event instead of running the command
        let output_tx_clone = output_tx.clone();
        std::thread::spawn(move || {
            let _ = output_tx_clone.blocking_send(ShellEvent::Clear);
            let _ = output_tx_clone.blocking_send(ShellEvent::Completed(0));
        });
        return Ok(shell_cmd);
    }

    std::thread::spawn(move || {
        let pty_system = native_pty_system();

        let pair = match pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            Ok(p) => p,
            Err(e) => {
                let _ = output_tx
                    .blocking_send(ShellEvent::Error(format!("Failed to open PTY: {}", e)));
                return;
            }
        };

        let mut cmd = CommandBuilder::new("sh");
        cmd.args(["-c", &command]);

        let mut child = match pair.slave.spawn_command(cmd) {
            Ok(c) => c,
            Err(e) => {
                let _ = output_tx
                    .blocking_send(ShellEvent::Error(format!("Failed to spawn command: {}", e)));
                return;
            }
        };

        // Take the writer for stdin
        let mut writer = match pair.master.take_writer() {
            Ok(w) => w,
            Err(e) => {
                let _ = output_tx.blocking_send(ShellEvent::Error(format!(
                    "Failed to get PTY writer: {}",
                    e
                )));
                return;
            }
        };

        // Handle stdin in a separate thread
        std::thread::spawn(move || {
            while let Some(input) = stdin_rx.blocking_recv() {
                // Don't add newline for password input
                if let Err(e) = write!(writer, "{}", input) {
                    eprintln!("Failed to write to PTY: {}", e);
                    break;
                }
                if let Err(e) = writeln!(writer) {
                    eprintln!("Failed to write newline to PTY: {}", e);
                    break;
                }
                if let Err(e) = writer.flush() {
                    eprintln!("Failed to flush PTY: {}", e);
                    break;
                }
            }
        });

        // Read output - buffer for partial reads
        let mut reader = match pair.master.try_clone_reader() {
            Ok(r) => r,
            Err(e) => {
                let _ = output_tx.blocking_send(ShellEvent::Error(format!(
                    "Failed to clone PTY reader: {}",
                    e
                )));
                return;
            }
        };

        let mut buffer = vec![0u8; 4096];
        let mut accumulated = Vec::new();
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    accumulated.extend_from_slice(&buffer[..n]);

                    // Process accumulated data
                    if let Ok(text) = String::from_utf8(accumulated.clone()) {
                        let text = preprocess_terminal_output(&text);
                        // Look for sensitive input prompt patterns
                        if is_interactive_prompt(&text) && !text.ends_with('\n') {
                            // This is likely a sensitive input prompt without newline
                            let _ = output_tx.blocking_send(ShellEvent::InputRequest(text.clone()));
                            accumulated.clear();
                        } else if text.contains('\n') {
                            // Process complete lines
                            for line in text.lines() {
                                if is_interactive_prompt(line) {
                                    let _ = output_tx
                                        .blocking_send(ShellEvent::InputRequest(line.to_string()));
                                } else {
                                    let _ = output_tx
                                        .blocking_send(ShellEvent::Output(line.to_string()));
                                }
                            }
                            accumulated.clear();
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available, sleep briefly
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => {
                    let _ =
                        output_tx.blocking_send(ShellEvent::Error(format!("Read error: {}", e)));
                    break;
                }
            }
        }

        // Wait for completion
        match child.wait() {
            Ok(status) => {
                let code = status.exit_code() as i32;
                let _ = output_tx.blocking_send(ShellEvent::Completed(code));
            }
            Err(e) => {
                let _ = output_tx.blocking_send(ShellEvent::Error(format!("Wait error: {}", e)));
                let _ = output_tx.blocking_send(ShellEvent::Completed(-1));
            }
        }
    });

    Ok(shell_cmd)
}
