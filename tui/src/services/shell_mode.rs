use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;

// Global process registry to track running commands
static PROCESS_REGISTRY: std::sync::OnceLock<Arc<Mutex<HashMap<String, u32>>>> =
    std::sync::OnceLock::new();

fn get_process_registry() -> Arc<Mutex<HashMap<String, u32>>> {
    PROCESS_REGISTRY
        .get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone()
}

pub const SHELL_PROMPT_PREFIX: &str = "$ ";

#[derive(Debug, Clone)]
pub enum ShellEvent {
    Output(String),
    Error(String),
    Completed(i32), // Exit code
    Clear,          // Clear the output display
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

impl ShellCommand {
    /// Kill the running command by sending Ctrl+C and then using system kill
    pub fn kill(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Try Ctrl+C through stdin multiple times
        for _i in 0..3 {
            let _ = self.stdin_tx.try_send("\x03".to_string());
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Also try sending Ctrl+C with newline
        let _ = self.stdin_tx.try_send("\x03\n".to_string());

        // Try to kill the process directly using the registry
        let registry = get_process_registry();
        if let Ok(registry) = registry.lock()
            && let Some(&pid) = registry.get(&self.id)
        {
            #[cfg(unix)]
            {
                use std::process::Command;
                // First try SIGTERM (graceful)
                let _ = Command::new("kill").args([&pid.to_string()]).output();
                std::thread::sleep(std::time::Duration::from_millis(100));
                // Then try SIGKILL (forceful)
                let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
            }
            #[cfg(windows)]
            {
                use std::process::Command;
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .output();
            }
        }

        Ok(())
    }
}

#[allow(dead_code)]
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
        // Get the current working directory
        let current_dir = std::env::current_dir().unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("/"))
        });

        let child = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args(["/C", &command])
                .current_dir(&current_dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        } else {
            Command::new("sh")
                .args(["-c", &command])
                .current_dir(&current_dir)
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

        // Register the PID in the global registry
        let child_pid = child.id();
        let registry = get_process_registry();
        if let Ok(mut registry) = registry.lock() {
            registry.insert(command_id.clone(), child_pid);
        }

        // Handle stdin in a separate thread
        if let Some(mut stdin) = child.stdin.take() {
            std::thread::spawn(move || {
                while let Some(input) = stdin_rx.blocking_recv() {
                    if let Err(_e) = writeln!(stdin, "{}", input) {
                        // eprintln!("Failed to write to stdin: {}", e);
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
                            // Always send the output so user can see the prompt
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
                            // Check for interactive prompts in stderr too

                            // Check if this stderr line is actually an error or just progress info
                            let lower_line = line.to_lowercase();
                            let is_actual_error = lower_line.contains("error")
                                || lower_line.contains("failed")
                                || lower_line.contains("fatal")
                                || lower_line.contains("exception")
                                || lower_line.contains("panic")
                                || lower_line.starts_with("error:")
                                || lower_line.starts_with("fatal:")
                                || lower_line.starts_with("exception:");

                            if is_actual_error {
                                let _ = tx_clone.blocking_send(ShellEvent::Error(line));
                            } else {
                                // Treat as normal output if it's not an actual error
                                // Always send the output so user can see the prompt
                                let _ = tx_clone.blocking_send(ShellEvent::Output(line));
                            }
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
                // Give stdout/stderr threads a moment to finish sending their events
                std::thread::sleep(std::time::Duration::from_millis(10));
                // Clean up the registry
                let registry = get_process_registry();
                if let Ok(mut registry) = registry.lock() {
                    registry.remove(&command_id);
                }
                let _ = output_tx.blocking_send(ShellEvent::Completed(code));
            }
            Err(e) => {
                let _ = output_tx.blocking_send(ShellEvent::Error(format!("Wait error: {}", e)));
                // Clean up the registry
                let registry = get_process_registry();
                if let Ok(mut registry) = registry.lock() {
                    registry.remove(&command_id);
                }
                let _ = output_tx.blocking_send(ShellEvent::Completed(-1));
            }
        }
    });

    shell_cmd
}

#[cfg(unix)]
pub fn run_pty_command(
    command: String,
    command_to_execute: Option<String>,  // If Some, this command is typed after prompt appears
    output_tx: mpsc::Sender<ShellEvent>,
    rows: u16,
    cols: u16,
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

    std::thread::spawn(move || {
        let pty_system = native_pty_system();

        let pair = match pty_system.openpty(PtySize {
            rows,
            cols,
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

        // Get the current working directory
        let current_dir = std::env::current_dir().unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("/"))
        });

        let shell = std::env::var("SHELL").unwrap_or("sh".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        // Start interactive login shell to get full prompt configuration (git branch, etc)
        cmd.args(["-il"]);
        cmd.cwd(&current_dir);

        let mut child = match pair.slave.spawn_command(cmd) {
            Ok(c) => c,
            Err(e) => {
                let _ = output_tx
                    .blocking_send(ShellEvent::Error(format!("Failed to spawn command: {}", e)));
                return;
            }
        };

        // Register the PID in the global registry
        if let Some(child_pid) = child.process_id() {
            let registry = get_process_registry();
            if let Ok(mut registry) = registry.lock() {
                registry.insert(command_id.clone(), child_pid);
            }
        }

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

        // Clone command for typing later (only if provided)
        let command_to_type = command_to_execute;
        
        // Channel to signal when prompt is ready (first output received)
        let (prompt_ready_tx, prompt_ready_rx) = std::sync::mpsc::channel::<()>();
        
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

        // Start the reader thread - sends signal when first output received
        let output_tx_clone = output_tx.clone();
        std::thread::spawn(move || {
            let mut buffer = vec![0u8; 4096];
            let mut first_output = true;
            
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        // Signal that prompt is ready on first output
                        if first_output {
                            let _ = prompt_ready_tx.send(());
                            first_output = false;
                        }
                        
                        // Process data
                        if let Ok(text) = String::from_utf8(buffer[..n].to_vec()) {
                            let _ = output_tx_clone.blocking_send(ShellEvent::Output(text));
                        } else {
                            // Lossy conversion for non-utf8
                            let text = String::from_utf8_lossy(&buffer[..n]).to_string();
                            let _ = output_tx_clone.blocking_send(ShellEvent::Output(text));
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        // No data available, sleep briefly
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(e) => {
                        let _ = output_tx_clone
                            .blocking_send(ShellEvent::Error(format!("Read error: {}", e)));
                        break;
                    }
                }
            }
        });
        
        // Handle stdin in a separate thread - waits for prompt before typing command
        std::thread::spawn(move || {
            // Only wait for prompt and type command if we have one
            if let Some(cmd) = command_to_type {
                // Wait for shell to output prompt (with timeout)
                let timeout = std::time::Duration::from_secs(5);
                if prompt_ready_rx.recv_timeout(timeout).is_err() {
                    eprintln!("Timeout waiting for shell prompt");
                }
                
                // Give a bit more time for prompt to fully render
                std::thread::sleep(std::time::Duration::from_millis(300));
                
                // Type the command followed by Enter
                if let Err(e) = writeln!(writer, "{}", cmd) {
                    eprintln!("Failed to type command to PTY: {}", e);
                }
                if let Err(e) = writer.flush() {
                    eprintln!("Failed to flush PTY: {}", e);
                }
            }
            
            // Now handle user input from the channel
            while let Some(input) = stdin_rx.blocking_recv() {
                // Don't add newline for password input
                if let Err(e) = write!(writer, "{}", input) {
                    eprintln!("Failed to write to PTY: {}", e);
                    break;
                }

                if let Err(e) = writer.flush() {
                    eprintln!("Failed to flush PTY: {}", e);
                    break;
                }
            }
        });

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
