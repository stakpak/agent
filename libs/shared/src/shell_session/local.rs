//! Local Shell Session Implementation
//!
//! Provides persistent local shell sessions using `portable-pty` for PTY-based
//! command execution with state persistence across commands.

use super::session::{CommandOutput, ShellSession, ShellSessionError};
use super::{MARKER_PREFIX, MARKER_SUFFIX, ShellSessionManager, clean_shell_output};
use async_trait::async_trait;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, trace};

#[cfg(any(unix, windows))]
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

/// Default PTY size
const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;

/// Local shell session using PTY for persistent state
pub struct LocalShellSession {
    session_id: String,
    #[cfg(any(unix, windows))]
    inner: Arc<Mutex<LocalShellSessionInner>>,
    #[cfg(not(any(unix, windows)))]
    _phantom: std::marker::PhantomData<()>,
}

#[cfg(any(unix, windows))]
struct LocalShellSessionInner {
    writer: Box<dyn Write + Send>,
    reader: Box<dyn Read + Send>,
    #[allow(dead_code)]
    child: Box<dyn portable_pty::Child + Send + Sync>,
    #[allow(dead_code)]
    shell_path: String,
    closed: bool,
}

impl LocalShellSession {
    /// Create a new local shell session
    ///
    /// # Arguments
    /// * `shell` - Optional shell path (auto-detects if None)
    /// * `cwd` - Optional working directory (uses current dir if None)
    /// * `rows` - PTY rows (default: 24)
    /// * `cols` - PTY columns (default: 80)
    #[cfg(any(unix, windows))]
    pub fn new(
        shell: Option<&str>,
        cwd: Option<&std::path::Path>,
        rows: Option<u16>,
        cols: Option<u16>,
    ) -> Result<Self, ShellSessionError> {
        let session_id = ShellSessionManager::generate_session_id("local");
        let shell_path = shell.map(String::from).unwrap_or_else(Self::detect_shell);

        debug!(
            session_id = %session_id,
            shell = %shell_path,
            "Creating local shell session"
        );

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: rows.unwrap_or(DEFAULT_ROWS),
                cols: cols.unwrap_or(DEFAULT_COLS),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| ShellSessionError::PtyError(format!("Failed to open PTY: {}", e)))?;

        let current_dir = cwd.map(|p| p.to_path_buf()).unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| std::path::PathBuf::from("/"))
            })
        });

        let mut cmd = CommandBuilder::new(&shell_path);
        cmd.cwd(&current_dir);

        // Start interactive shell
        #[cfg(not(windows))]
        cmd.args(["-i"]);

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| ShellSessionError::SpawnFailed(format!("Failed to spawn shell: {}", e)))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| ShellSessionError::PtyError(format!("Failed to get PTY writer: {}", e)))?;

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| ShellSessionError::PtyError(format!("Failed to get PTY reader: {}", e)))?;

        let inner = LocalShellSessionInner {
            writer,
            reader,
            child,
            shell_path,
            closed: false,
        };

        Ok(Self {
            session_id,
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    #[cfg(not(any(unix, windows)))]
    pub fn new(
        _shell: Option<&str>,
        _cwd: Option<&std::path::Path>,
        _rows: Option<u16>,
        _cols: Option<u16>,
    ) -> Result<Self, ShellSessionError> {
        Err(ShellSessionError::PtyError(
            "PTY not supported on this platform".to_string(),
        ))
    }

    /// Detect the default shell for the current platform
    fn detect_shell() -> String {
        #[cfg(windows)]
        {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
        }
        #[cfg(not(windows))]
        {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
        }
    }

    /// Generate a unique marker for command completion detection
    fn generate_marker() -> String {
        let uuid = uuid::Uuid::new_v4().to_string().replace("-", "");
        format!("{}{}{}", MARKER_PREFIX, &uuid[..16], MARKER_SUFFIX)
    }

    /// Execute command with marker-based completion detection
    #[cfg(any(unix, windows))]
    async fn execute_with_marker(
        &self,
        command: &str,
        timeout: Option<Duration>,
    ) -> Result<CommandOutput, ShellSessionError> {
        let marker = Self::generate_marker();
        let start = Instant::now();

        let mut inner = self.inner.lock().await;

        if inner.closed {
            return Err(ShellSessionError::SessionClosed);
        }

        // Note: We don't drain pending output here because the PTY reader is blocking.
        // Instead, we rely on the marker-based detection to find our command's output.

        // Send command followed by marker echo
        // Use a subshell to capture exit code
        let full_command = format!("{}\necho \"{}\"\n", command.trim(), marker);

        inner
            .writer
            .write_all(full_command.as_bytes())
            .map_err(ShellSessionError::IoError)?;
        inner.writer.flush().map_err(ShellSessionError::IoError)?;

        trace!(command = %command, marker = %marker, "Sent command to PTY");

        // Read output until marker is found
        // Note: PTY reader is blocking, so we use spawn_blocking to avoid blocking the async runtime
        let timeout_duration = timeout.unwrap_or(Duration::from_secs(300));
        let mut output = String::new();

        // Set a read timeout on the reader if possible (platform-specific)
        // For now, we'll use a polling approach with small reads

        // Read output until marker is found
        // Use polling with small sleeps to avoid blocking the async runtime
        // Drop the lock and use spawn_blocking for the blocking PTY read
        drop(inner);

        loop {
            if start.elapsed() > timeout_duration {
                return Err(ShellSessionError::Timeout(timeout_duration));
            }

            // Clone Arc for the blocking task
            let inner_arc = self.inner.clone();
            let _marker_clone = marker.clone();

            // Perform blocking read in a separate thread
            let read_result = tokio::task::spawn_blocking(move || {
                // Use tokio's block_in_place alternative - just block on the lock
                let rt = tokio::runtime::Handle::current();
                let mut inner_guard = rt.block_on(inner_arc.lock());

                if inner_guard.closed {
                    return Ok((Vec::new(), true, false)); // (data, is_closed, found_marker)
                }

                let mut buf = [0u8; 4096];
                match inner_guard.reader.read(&mut buf) {
                    Ok(0) => Ok((Vec::new(), false, false)), // EOF
                    Ok(n) => {
                        let data = buf[..n].to_vec();
                        Ok((data, false, false))
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        Ok((Vec::new(), false, false)) // No data, but not EOF
                    }
                    Err(e) => Err(e),
                }
            })
            .await;

            match read_result {
                Ok(Ok((data, is_closed, _))) => {
                    if is_closed {
                        return Err(ShellSessionError::SessionClosed);
                    }
                    if data.is_empty() {
                        // No data yet or WouldBlock, sleep and retry
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }

                    let chunk = String::from_utf8_lossy(&data);
                    output.push_str(&chunk);

                    // Count occurrences of the marker in output
                    // First occurrence is the echo of our command: echo "__MARKER__"
                    // Second occurrence is the actual output of that echo command
                    let marker_count = output.matches(&marker).count();
                    if marker_count >= 2 {
                        // We've seen both the command echo and the actual marker output
                        break;
                    }
                }
                Ok(Err(e)) => {
                    return Err(ShellSessionError::IoError(e));
                }
                Err(e) => {
                    return Err(ShellSessionError::ExecutionFailed(format!(
                        "Read task failed: {}",
                        e
                    )));
                }
            }
        }

        let duration = start.elapsed();

        // Clean up output: remove the command echo, marker, and prompt artifacts
        trace!(raw_output = %output, "Raw output before cleaning");
        let cleaned_output = clean_shell_output(&output, command, &marker);

        debug!(
            session_id = %self.session_id,
            duration_ms = duration.as_millis(),
            output_len = cleaned_output.len(),
            raw_len = output.len(),
            "Command completed"
        );

        Ok(CommandOutput {
            output: cleaned_output,
            exit_code: None, // PTY doesn't easily give us exit codes
            duration,
        })
    }
}

#[async_trait]
impl ShellSession for LocalShellSession {
    #[cfg(any(unix, windows))]
    async fn execute(
        &self,
        command: &str,
        timeout: Option<Duration>,
    ) -> Result<CommandOutput, ShellSessionError> {
        self.execute_with_marker(command, timeout).await
    }

    #[cfg(not(any(unix, windows)))]
    async fn execute(
        &self,
        _command: &str,
        _timeout: Option<Duration>,
    ) -> Result<CommandOutput, ShellSessionError> {
        Err(ShellSessionError::PtyError(
            "PTY not supported on this platform".to_string(),
        ))
    }

    #[cfg(any(unix, windows))]
    async fn execute_streaming(
        &self,
        command: &str,
        timeout: Option<Duration>,
    ) -> Result<
        (
            super::session::OutputReceiver,
            tokio::task::JoinHandle<Result<CommandOutput, ShellSessionError>>,
        ),
        ShellSessionError,
    > {
        use super::session::{OutputChunk, OutputSender};
        use tokio::sync::mpsc;

        let marker = Self::generate_marker();
        let start = Instant::now();
        let timeout_duration = timeout.unwrap_or(Duration::from_secs(300));

        // Create channel for streaming output
        let (tx, rx): (OutputSender, _) = mpsc::channel(100);

        // Check if session is closed before starting
        {
            let inner = self.inner.lock().await;
            if inner.closed {
                return Err(ShellSessionError::SessionClosed);
            }
        }

        // Send command to PTY
        {
            let mut inner = self.inner.lock().await;
            let full_command = format!("{}\necho \"{}\"\n", command.trim(), marker);
            inner
                .writer
                .write_all(full_command.as_bytes())
                .map_err(ShellSessionError::IoError)?;
            inner.writer.flush().map_err(ShellSessionError::IoError)?;
        }

        trace!(command = %command, marker = %marker, "Sent command to PTY for streaming");

        // Clone what we need for the spawned task
        let inner_arc = self.inner.clone();
        let session_id = self.session_id.clone();
        let command_owned = command.to_string();
        let marker_clone = marker.clone();

        // Spawn task to read output and stream it
        let handle = tokio::spawn(async move {
            let mut output = String::new();
            let mut last_streamed_len = 0usize; // Track what we've already streamed
            let mut pending_partial = String::new(); // Buffer for incomplete lines
            let mut last_send = Instant::now();

            // Debounce: send partial content after this delay if no newline arrives
            // This makes progress bars, prompts, etc. appear naturally
            const PARTIAL_LINE_DELAY: Duration = Duration::from_millis(100);
            // Minimum time between sends to avoid overwhelming the TUI (~60fps)
            const MIN_SEND_INTERVAL: Duration = Duration::from_millis(16);

            loop {
                if start.elapsed() > timeout_duration {
                    let _ = tx
                        .send(OutputChunk {
                            text: "\n[TIMEOUT]\n".to_string(),
                            is_final: true,
                        })
                        .await;
                    return Err(ShellSessionError::Timeout(timeout_duration));
                }

                // Clone Arc for the blocking task
                let inner_arc_clone = inner_arc.clone();

                // Perform blocking read in a separate thread
                let read_result = tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    let mut inner_guard = rt.block_on(inner_arc_clone.lock());

                    if inner_guard.closed {
                        return Ok((Vec::new(), true)); // (data, is_closed)
                    }

                    let mut buf = [0u8; 4096];
                    match inner_guard.reader.read(&mut buf) {
                        Ok(0) => Ok((Vec::new(), false)), // EOF
                        Ok(n) => {
                            let data = buf[..n].to_vec();
                            Ok((data, false))
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            Ok((Vec::new(), false))
                        }
                        Err(e) => Err(e),
                    }
                })
                .await;

                match read_result {
                    Ok(Ok((data, is_closed))) => {
                        if is_closed {
                            let _ = tx
                                .send(OutputChunk {
                                    text: "\n[SESSION CLOSED]\n".to_string(),
                                    is_final: true,
                                })
                                .await;
                            return Err(ShellSessionError::SessionClosed);
                        }

                        let has_new_data = !data.is_empty();

                        if has_new_data {
                            let chunk = String::from_utf8_lossy(&data);
                            output.push_str(&chunk);
                        }

                        // Clean accumulated output
                        let cleaned_so_far =
                            clean_shell_output(&output, &command_owned, &marker_clone);

                        if cleaned_so_far.len() > last_streamed_len {
                            let new_content = &cleaned_so_far[last_streamed_len..];

                            // Check for complete lines
                            if let Some(last_newline) = new_content.rfind('\n') {
                                let complete_lines = &new_content[..=last_newline];
                                let remainder = &new_content[last_newline + 1..];

                                // Send complete lines immediately (respecting min interval)
                                if !complete_lines.trim().is_empty()
                                    && last_send.elapsed() >= MIN_SEND_INTERVAL
                                {
                                    let _ = tx
                                        .send(OutputChunk {
                                            text: complete_lines.to_string(),
                                            is_final: false,
                                        })
                                        .await;
                                    last_send = Instant::now();
                                }

                                last_streamed_len += last_newline + 1;
                                pending_partial = remainder.to_string();
                            } else {
                                // No newline - accumulate as pending partial
                                pending_partial = new_content.to_string();
                            }
                        }

                        // Send partial lines after debounce delay
                        // This makes progress bars, prompts, etc. appear naturally
                        if !pending_partial.is_empty() && last_send.elapsed() >= PARTIAL_LINE_DELAY
                        {
                            let _ = tx
                                .send(OutputChunk {
                                    text: std::mem::take(&mut pending_partial),
                                    is_final: false,
                                })
                                .await;
                            last_streamed_len = cleaned_so_far.len();
                            last_send = Instant::now();
                        }

                        // Check for marker completion
                        let marker_count = output.matches(&marker_clone).count();
                        if marker_count >= 2 {
                            // Send any remaining partial content
                            if !pending_partial.is_empty() {
                                let _ = tx
                                    .send(OutputChunk {
                                        text: pending_partial,
                                        is_final: false,
                                    })
                                    .await;
                            }
                            break;
                        }

                        // Adaptive sleep: shorter when actively receiving data
                        if !has_new_data {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    }
                    Ok(Err(e)) => {
                        let _ = tx
                            .send(OutputChunk {
                                text: format!("\n[IO ERROR: {}]\n", e),
                                is_final: true,
                            })
                            .await;
                        return Err(ShellSessionError::IoError(e));
                    }
                    Err(e) => {
                        let _ = tx
                            .send(OutputChunk {
                                text: format!("\n[TASK ERROR: {}]\n", e),
                                is_final: true,
                            })
                            .await;
                        return Err(ShellSessionError::ExecutionFailed(format!(
                            "Read task failed: {}",
                            e
                        )));
                    }
                }
            }

            let duration = start.elapsed();

            // Clean up output
            let cleaned_output = clean_shell_output(&output, &command_owned, &marker_clone);

            // Send final chunk
            let _ = tx
                .send(OutputChunk {
                    text: String::new(),
                    is_final: true,
                })
                .await;

            debug!(
                session_id = %session_id,
                duration_ms = duration.as_millis(),
                output_len = cleaned_output.len(),
                "Streaming command completed"
            );

            Ok(CommandOutput {
                output: cleaned_output,
                exit_code: None,
                duration,
            })
        });

        Ok((rx, handle))
    }

    #[cfg(not(any(unix, windows)))]
    async fn execute_streaming(
        &self,
        _command: &str,
        _timeout: Option<Duration>,
    ) -> Result<
        (
            super::session::OutputReceiver,
            tokio::task::JoinHandle<Result<CommandOutput, ShellSessionError>>,
        ),
        ShellSessionError,
    > {
        Err(ShellSessionError::PtyError(
            "PTY not supported on this platform".to_string(),
        ))
    }

    #[cfg(any(unix, windows))]
    async fn is_alive(&self) -> bool {
        let inner = self.inner.lock().await;
        !inner.closed
    }

    #[cfg(not(any(unix, windows)))]
    async fn is_alive(&self) -> bool {
        false
    }

    fn session_id(&self) -> &str {
        &self.session_id
    }

    fn description(&self) -> String {
        format!("local:{}", self.session_id)
    }

    #[cfg(any(unix, windows))]
    async fn close(&mut self) -> Result<(), ShellSessionError> {
        let mut inner = self.inner.lock().await;
        if inner.closed {
            return Ok(());
        }

        // Send exit command
        let _ = inner.writer.write_all(b"exit\n");
        let _ = inner.writer.flush();

        inner.closed = true;
        debug!(session_id = %self.session_id, "Local shell session closed");
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    async fn close(&mut self) -> Result<(), ShellSessionError> {
        Ok(())
    }
}

#[cfg(test)]
#[cfg(any(unix, windows))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_session_creation() {
        let session = LocalShellSession::new(None, None, None, None);
        assert!(session.is_ok(), "Should create local session");

        let session = session.unwrap();
        assert!(session.session_id().starts_with("local-"));
        assert!(session.is_alive().await);
    }

    #[tokio::test]
    async fn test_marker_generation() {
        let marker1 = LocalShellSession::generate_marker();
        let marker2 = LocalShellSession::generate_marker();

        assert!(marker1.starts_with(MARKER_PREFIX));
        assert!(marker1.ends_with(MARKER_SUFFIX));
        assert_ne!(marker1, marker2, "Markers should be unique");
    }

    #[tokio::test]
    async fn test_clean_output() {
        use super::clean_shell_output;
        let raw = "echo hello\nhello\n__STAKPAK_CMD_END_abc123__\n$ ";
        let cleaned = clean_shell_output(raw, "echo hello", "__STAKPAK_CMD_END_abc123__");
        assert_eq!(cleaned.trim(), "hello");
    }

    #[tokio::test]
    async fn test_execute_simple_command() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        // No artificial delays - shell should be ready immediately
        let result = session.execute("echo hello", None).await;

        assert!(result.is_ok(), "Command should succeed: {:?}", result.err());
        let output = result.unwrap();
        assert!(
            output.output.contains("hello"),
            "Output should contain 'hello', got: '{}'",
            output.output
        );
        // Verify command completed quickly (under 5 seconds for simple echo)
        assert!(
            output.duration.as_secs() < 5,
            "Simple echo should complete quickly, took: {:?}",
            output.duration
        );
    }

    #[tokio::test]
    async fn test_environment_persistence() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        // Set an environment variable - no timeout needed for fast commands
        let set_result = session
            .execute("export TEST_VAR=persistent_value", None)
            .await;
        assert!(set_result.is_ok(), "Setting env var should succeed");

        // Read it back in a subsequent command
        let get_result = session.execute("echo $TEST_VAR", None).await;

        assert!(get_result.is_ok(), "Reading env var should succeed");
        let output = get_result.unwrap();
        assert!(
            output.output.contains("persistent_value"),
            "Environment variable should persist, got: '{}'",
            output.output
        );
    }

    #[tokio::test]
    async fn test_working_directory_persistence() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        // Change directory - no timeout needed
        let cd_result = session.execute("cd /tmp", None).await;
        assert!(cd_result.is_ok(), "cd should succeed");

        // Check we're still in /tmp
        let pwd_result = session.execute("pwd", None).await;

        assert!(pwd_result.is_ok(), "pwd should succeed");
        let output = pwd_result.unwrap();
        assert!(
            output.output.contains("/tmp") || output.output.contains("/private/tmp"),
            "Working directory should persist to /tmp, got: '{}'",
            output.output
        );
    }

    #[tokio::test]
    async fn test_multiple_commands_in_sequence() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        // Run multiple commands in sequence to verify session stays responsive
        for i in 1..=5 {
            let result = session
                .execute(&format!("echo iteration_{}", i), None)
                .await;
            assert!(
                result.is_ok(),
                "Command {} should succeed: {:?}",
                i,
                result.err()
            );
            let output = result.unwrap();
            assert!(
                output.output.contains(&format!("iteration_{}", i)),
                "Output should contain 'iteration_{}', got: '{}'",
                i,
                output.output
            );
        }
    }

    #[tokio::test]
    async fn test_markers_are_transparent_across_commands() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        // Run multiple commands and verify NO markers leak into output
        let commands = vec![
            "echo first_command",
            "echo second_command",
            "ls -la /tmp | head -3",
            "echo third_command",
            "pwd",
        ];

        for (i, cmd) in commands.iter().enumerate() {
            let result = session.execute(cmd, None).await;
            assert!(
                result.is_ok(),
                "Command {} '{}' should succeed: {:?}",
                i,
                cmd,
                result.err()
            );

            let output = result.unwrap();

            // CRITICAL: Verify no markers leak into output
            assert!(
                !output.output.contains("__STAKPAK_CMD_END_"),
                "Marker should NOT appear in output for command '{}', got: '{}'",
                cmd,
                output.output
            );
            assert!(
                !output.output.contains(MARKER_PREFIX),
                "Marker prefix should NOT appear in output for command '{}', got: '{}'",
                cmd,
                output.output
            );
            assert!(
                !output.output.contains(MARKER_SUFFIX),
                "Marker suffix should NOT appear in output for command '{}', got: '{}'",
                cmd,
                output.output
            );
        }
    }

    #[tokio::test]
    async fn test_output_clean_no_marker_contamination() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        // Set env var, change dir, then verify output is clean
        let _ = session.execute("export CLEAN_TEST=value123", None).await;
        let _ = session.execute("cd /tmp", None).await;

        // Now run a command that produces output
        let result = session.execute("echo $CLEAN_TEST && pwd", None).await;
        assert!(result.is_ok(), "Command should succeed");

        let output = result.unwrap();

        // Verify expected content is present
        assert!(
            output.output.contains("value123"),
            "Should contain env var value"
        );
        assert!(
            output.output.contains("/tmp") || output.output.contains("/private/tmp"),
            "Should contain pwd output"
        );

        // Verify NO implementation details leak
        assert!(
            !output.output.contains("__STAKPAK"),
            "No internal markers should leak"
        );
        assert!(
            !output.output.contains("STAKPAK_CMD"),
            "No internal markers should leak"
        );

        // Verify output doesn't contain the echo command for the marker
        assert!(
            !output.output.contains("echo \"__STAKPAK"),
            "Marker echo command should not appear in output"
        );
    }

    #[tokio::test]
    async fn test_command_with_multiline_output() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        let result = session
            .execute("echo 'line1'; echo 'line2'; echo 'line3'", None)
            .await;

        assert!(result.is_ok(), "Command should succeed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.output.contains("line1"), "Should contain line1");
        assert!(output.output.contains("line2"), "Should contain line2");
        assert!(output.output.contains("line3"), "Should contain line3");
    }

    #[tokio::test]
    async fn test_command_with_special_characters() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        // Test with quotes and special chars
        let result = session.execute("echo 'hello \"world\"'", None).await;

        assert!(result.is_ok(), "Command should succeed: {:?}", result.err());
        let output = result.unwrap();
        assert!(
            output.output.contains("hello") && output.output.contains("world"),
            "Output should contain quoted text, got: '{}'",
            output.output
        );
    }

    #[tokio::test]
    async fn test_shell_aliases_and_functions() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        // Define a function
        let define_result = session
            .execute("myfunc() { echo \"function output: $1\"; }", None)
            .await;
        assert!(define_result.is_ok(), "Function definition should succeed");

        // Call the function
        let call_result = session.execute("myfunc test_arg", None).await;

        assert!(
            call_result.is_ok(),
            "Function call should succeed: {:?}",
            call_result.err()
        );
        let output = call_result.unwrap();
        assert!(
            output.output.contains("function output: test_arg"),
            "Function should execute correctly, got: '{}'",
            output.output
        );
    }

    #[tokio::test]
    async fn test_session_close_and_reopen() {
        let mut session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        // Execute a command
        let result = session.execute("echo before_close", None).await;
        assert!(result.is_ok(), "Command before close should succeed");

        // Close the session
        let close_result = session.close().await;
        assert!(close_result.is_ok(), "Close should succeed");

        // Session should report as not alive
        assert!(
            !session.is_alive().await,
            "Session should not be alive after close"
        );

        // Commands should fail on closed session
        let result = session.execute("echo after_close", None).await;
        assert!(result.is_err(), "Command on closed session should fail");
    }

    #[tokio::test]
    async fn test_command_with_exit_code() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        // Run a command that succeeds
        let result = session.execute("true", None).await;
        assert!(result.is_ok(), "true command should succeed");

        // Run a command that fails (false returns exit code 1)
        let result = session.execute("false", None).await;
        // Note: PTY doesn't easily give us exit codes, so this should still "succeed"
        // in terms of execution, but the command itself returns non-zero
        assert!(
            result.is_ok(),
            "false command should execute (even if exit code is non-zero)"
        );
    }

    #[tokio::test]
    async fn test_empty_command() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        // Empty command should still work (just produces empty output)
        let result = session.execute("", None).await;
        assert!(result.is_ok(), "Empty command should succeed");
    }

    #[tokio::test]
    async fn test_command_with_pipes() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        let result = session
            .execute("echo 'hello world' | tr 'a-z' 'A-Z'", None)
            .await;

        assert!(
            result.is_ok(),
            "Piped command should succeed: {:?}",
            result.err()
        );
        let output = result.unwrap();
        assert!(
            output.output.contains("HELLO WORLD"),
            "Pipe should transform output, got: '{}'",
            output.output
        );
    }

    #[tokio::test]
    async fn test_command_with_subshell() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Should create session");

        let result = session.execute("echo $(echo nested)", None).await;

        assert!(
            result.is_ok(),
            "Subshell command should succeed: {:?}",
            result.err()
        );
        let output = result.unwrap();
        assert!(
            output.output.contains("nested"),
            "Subshell should execute, got: '{}'",
            output.output
        );
    }

    #[tokio::test]
    async fn test_clean_output_removes_prompts() {
        use super::clean_shell_output;
        // Test standalone prompt patterns are removed
        let raw = "echo test\ntest\n__MARKER__\n$ ";
        let cleaned = clean_shell_output(raw, "echo test", "__MARKER__");
        assert!(
            !cleaned.ends_with("$ "),
            "Should remove standalone prompt, got: '{}'",
            cleaned
        );
        assert!(cleaned.contains("test"), "Should keep output");

        // Test zsh-style prompt
        let raw2 = "echo test\ntest\n__MARKER__\n% ";
        let cleaned2 = clean_shell_output(raw2, "echo test", "__MARKER__");
        assert!(
            !cleaned2.ends_with("% "),
            "Should remove zsh prompt, got: '{}'",
            cleaned2
        );
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_streaming_basic() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Failed to create session");

        let (mut rx, handle) = session
            .execute_streaming("echo 'hello streaming'", Some(Duration::from_secs(10)))
            .await
            .expect("Failed to start streaming");

        // Collect all chunks
        let mut chunks = Vec::new();
        while let Some(chunk) = rx.recv().await {
            println!("Received chunk: {:?}", chunk);
            chunks.push(chunk.clone());
            if chunk.is_final {
                break;
            }
        }

        // Wait for completion
        let result = handle.await.expect("Task panicked");
        let output = result.expect("Command failed");

        println!("Final cleaned output: {:?}", output.output);

        // Verify the final output is clean (no ANSI codes, no prompts)
        assert!(
            output.output.contains("hello streaming"),
            "Output should contain 'hello streaming', got: {}",
            output.output
        );
        assert!(
            !output.output.contains("\x1b["),
            "Output should not contain ANSI codes"
        );
        assert!(
            !output.output.contains("@"),
            "Output should not contain prompt (user@host)"
        );
        assert!(
            !chunks.is_empty(),
            "Should have received at least one chunk"
        );

        // Verify streamed chunks are also clean
        for chunk in &chunks {
            if !chunk.is_final && !chunk.text.is_empty() {
                assert!(
                    !chunk.text.contains("\x1b["),
                    "Streamed chunk should not contain ANSI codes: {:?}",
                    chunk.text
                );
            }
        }
    }

    #[tokio::test]
    async fn test_execute_streaming_multiline() {
        let session =
            LocalShellSession::new(None, None, None, None).expect("Failed to create session");

        let (mut rx, handle) = session
            .execute_streaming(
                "for i in 1 2 3; do echo \"line $i\"; done",
                Some(Duration::from_secs(10)),
            )
            .await
            .expect("Failed to start streaming");

        // Collect chunks
        let mut all_text = String::new();
        while let Some(chunk) = rx.recv().await {
            all_text.push_str(&chunk.text);
            if chunk.is_final {
                break;
            }
        }

        let result = handle.await.expect("Task panicked");
        let output = result.expect("Command failed");

        println!("Streamed text: {:?}", all_text);
        println!("Final output: {:?}", output.output);

        assert!(output.output.contains("line 1"), "Should contain line 1");
        assert!(output.output.contains("line 2"), "Should contain line 2");
        assert!(output.output.contains("line 3"), "Should contain line 3");
    }
}
