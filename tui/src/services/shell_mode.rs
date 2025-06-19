use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum ShellEvent {
    Output(String),
    Error(String),
    InputRequest(String), // For password prompts
    Completed(i32),       // Exit code
}

#[derive(Clone)]
pub struct ShellCommand {
    pub id: String,
    pub command: String,
    pub stdin_tx: mpsc::Sender<String>,
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
                            // Check for password prompts
                            if line.contains("password") || line.contains("Password") {
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
                            // Check for password prompts in stderr too
                            if line.contains("password") || line.contains("Password") {
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
    use std::io::Read;

    let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(100);
    let command_id = uuid::Uuid::new_v4().to_string();

    let shell_cmd = ShellCommand {
        id: command_id.clone(),
        command: command.clone(),
        stdin_tx: stdin_tx.clone(),
    };

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
                        // Look for password prompt patterns
                        if text.to_lowercase().contains("password") && !text.ends_with('\n') {
                            // This is likely a password prompt without newline
                            let _ = output_tx.blocking_send(ShellEvent::InputRequest(text.clone()));
                            accumulated.clear();
                        } else if text.contains('\n') {
                            // Process complete lines
                            for line in text.lines() {
                                if line.to_lowercase().contains("password") {
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
