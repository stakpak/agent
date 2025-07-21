use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use tokio::sync::mpsc;

/// Check if a line contains sensitive terms that might require user input
fn contains_sensitive_terms(line: &str) -> bool {
    let lower_line = line.to_lowercase();
    let sensitive_terms = [
        "password",
        "passphrase",
        "secret",
        "api_key",
        "api key",
        "token",
        "key",
        "credential",
        "private key",
        "private_key",
        "ssh key",
        "ssh_key",
        "gpg key",
        "gpg_key",
        "certificate",
        "cert",
        "pem",
        "p12",
        "keystore",
        "truststore",
    ];

    sensitive_terms.iter().any(|term| lower_line.contains(term))
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
                            // Check for sensitive input prompts
                            if contains_sensitive_terms(&line) {
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
                            if contains_sensitive_terms(&line) {
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
                        // Look for sensitive input prompt patterns
                        if contains_sensitive_terms(&text) && !text.ends_with('\n') {
                            // This is likely a sensitive input prompt without newline
                            let _ = output_tx.blocking_send(ShellEvent::InputRequest(text.clone()));
                            accumulated.clear();
                        } else if text.contains('\n') {
                            // Process complete lines
                            for line in text.lines() {
                                if contains_sensitive_terms(line) {
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
