//! Remote Shell Session Implementation
//!
//! Provides persistent remote shell sessions over SSH using `russh` with PTY allocation
//! for state persistence across commands.

use super::ShellSessionManager;
use super::clean_shell_output;
use super::session::{CommandOutput, ShellSession, ShellSessionError};
use crate::remote_connection::RemoteConnectionInfo;
use async_trait::async_trait;
use russh::client::{self, Handler};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tracing::{debug, trace};

/// Marker prefix for command completion detection
const MARKER_PREFIX: &str = "__STAKPAK_CMD_END_";
const MARKER_SUFFIX: &str = "__";

/// Default PTY size for remote sessions
const DEFAULT_ROWS: u32 = 24;
const DEFAULT_COLS: u32 = 80;

/// SSH client handler for remote shell sessions
struct RemoteShellHandler;

impl Handler for RemoteShellHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        // Accept all keys (same as RemoteConnection behavior)
        Ok(true)
    }
}

/// Remote shell session using SSH with PTY allocation
pub struct RemoteShellSession {
    session_id: String,
    connection_info: RemoteConnectionInfo,
    inner: Arc<Mutex<RemoteShellSessionInner>>,
}

struct RemoteShellSessionInner {
    channel: russh::Channel<russh::client::Msg>,
    #[allow(dead_code)]
    session: client::Handle<RemoteShellHandler>,
    closed: bool,
}

impl RemoteShellSession {
    /// Create a new remote shell session
    ///
    /// # Arguments
    /// * `connection_info` - SSH connection details
    /// * `rows` - PTY rows (default: 24)
    /// * `cols` - PTY columns (default: 80)
    pub async fn new(
        connection_info: RemoteConnectionInfo,
        rows: Option<u32>,
        cols: Option<u32>,
    ) -> Result<Self, ShellSessionError> {
        let session_id = ShellSessionManager::generate_session_id("remote");

        debug!(
            session_id = %session_id,
            connection = %connection_info.connection_string,
            "Creating remote shell session"
        );

        // Parse connection string
        let parsed = Self::parse_connection_string(&connection_info.connection_string)?;

        // Create SSH session
        let config = client::Config::default();
        let mut session = client::connect(
            config.into(),
            (parsed.hostname.as_str(), parsed.port),
            RemoteShellHandler {},
        )
        .await
        .map_err(|e| ShellSessionError::SshError(format!("Connection failed: {}", e)))?;

        // Authenticate
        Self::authenticate(&mut session, &parsed.username, &connection_info).await?;

        // Open channel with PTY
        let channel = session
            .channel_open_session()
            .await
            .map_err(|e| ShellSessionError::SshError(format!("Failed to open channel: {}", e)))?;

        // Request PTY
        channel
            .request_pty(
                true,
                "xterm-256color",
                cols.unwrap_or(DEFAULT_COLS),
                rows.unwrap_or(DEFAULT_ROWS),
                0,
                0,
                &[],
            )
            .await
            .map_err(|e| ShellSessionError::SshError(format!("Failed to request PTY: {}", e)))?;

        // Request shell
        channel
            .request_shell(true)
            .await
            .map_err(|e| ShellSessionError::SshError(format!("Failed to request shell: {}", e)))?;

        // Wait for initial prompt
        tokio::time::sleep(Duration::from_millis(500)).await;

        let inner = RemoteShellSessionInner {
            channel,
            session,
            closed: false,
        };

        debug!(session_id = %session_id, "Remote shell session created");

        Ok(Self {
            session_id,
            connection_info,
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    /// Parse connection string into components
    fn parse_connection_string(
        connection_string: &str,
    ) -> Result<ParsedConnection, ShellSessionError> {
        let (username, host_port) = connection_string.split_once('@').ok_or_else(|| {
            ShellSessionError::SshError(
                "Invalid connection string format. Expected: user@host or user@host:port"
                    .to_string(),
            )
        })?;

        let (hostname, port) = if let Some((host, port_str)) = host_port.split_once(':') {
            let port = port_str
                .parse::<u16>()
                .map_err(|_| ShellSessionError::SshError(format!("Invalid port: {}", port_str)))?;
            (host.to_string(), port)
        } else {
            (host_port.to_string(), 22)
        };

        Ok(ParsedConnection {
            username: username.to_string(),
            hostname,
            port,
        })
    }

    /// Authenticate the SSH session
    async fn authenticate(
        session: &mut client::Handle<RemoteShellHandler>,
        username: &str,
        connection_info: &RemoteConnectionInfo,
    ) -> Result<(), ShellSessionError> {
        if let Some(password) = &connection_info.password {
            let auth_result = session
                .authenticate_password(username, password)
                .await
                .map_err(|e| ShellSessionError::SshError(format!("Password auth failed: {}", e)))?;

            match auth_result {
                russh::client::AuthResult::Success => Ok(()),
                _ => Err(ShellSessionError::SshError(
                    "Password authentication failed".to_string(),
                )),
            }
        } else {
            // Use public key authentication
            let private_key_path = if let Some(path) = &connection_info.private_key_path {
                crate::remote_connection::RemoteConnection::canonicalize_key_path(path)
                    .map_err(|e| ShellSessionError::SshError(format!("Key path error: {}", e)))?
            } else {
                crate::remote_connection::RemoteConnection::get_default_key_files()
                    .map_err(|e| ShellSessionError::SshError(format!("No SSH key found: {}", e)))?
                    .0
            };

            let keypair = russh::keys::load_secret_key(&private_key_path, None)
                .map_err(|e| ShellSessionError::SshError(format!("Failed to load key: {}", e)))?;

            let auth_result = session
                .authenticate_publickey(
                    username,
                    russh::keys::PrivateKeyWithHashAlg::new(
                        Arc::new(keypair),
                        Some(russh::keys::HashAlg::Sha256),
                    ),
                )
                .await
                .map_err(|e| {
                    ShellSessionError::SshError(format!("Public key auth failed: {}", e))
                })?;

            match auth_result {
                russh::client::AuthResult::Success => Ok(()),
                _ => Err(ShellSessionError::SshError(
                    "Public key authentication failed".to_string(),
                )),
            }
        }
    }

    /// Generate a unique marker for command completion detection
    fn generate_marker() -> String {
        let uuid = uuid::Uuid::new_v4().to_string().replace("-", "");
        format!("{}{}{}", MARKER_PREFIX, &uuid[..16], MARKER_SUFFIX)
    }

    /// Execute command with marker-based completion detection
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

        // Send command followed by marker echo
        let full_command = format!("{}\necho \"{}\"\n", command.trim(), marker);

        inner
            .channel
            .data(full_command.as_bytes())
            .await
            .map_err(|e| ShellSessionError::SshError(format!("Failed to send command: {}", e)))?;

        trace!(command = %command, marker = %marker, "Sent command to remote PTY");

        // Read output until marker is found
        let timeout_duration = timeout.unwrap_or(Duration::from_secs(300));
        let mut output = String::new();

        loop {
            if start.elapsed() > timeout_duration {
                return Err(ShellSessionError::Timeout(timeout_duration));
            }

            // Wait for channel messages with timeout
            let wait_result =
                tokio::time::timeout(Duration::from_millis(100), inner.channel.wait()).await;

            match wait_result {
                Ok(Some(msg)) => {
                    match msg {
                        russh::ChannelMsg::Data { data } => {
                            let chunk = String::from_utf8_lossy(&data);
                            output.push_str(&chunk);

                            // Check if marker is in output
                            if output.contains(&marker) {
                                break;
                            }
                        }
                        russh::ChannelMsg::ExtendedData { data, .. } => {
                            let chunk = String::from_utf8_lossy(&data);
                            output.push_str(&chunk);

                            if output.contains(&marker) {
                                break;
                            }
                        }
                        russh::ChannelMsg::Eof => {
                            return Err(ShellSessionError::SessionDead(
                                "Remote shell closed".to_string(),
                            ));
                        }
                        russh::ChannelMsg::Close => {
                            inner.closed = true;
                            return Err(ShellSessionError::SessionDead(
                                "Remote channel closed".to_string(),
                            ));
                        }
                        _ => {}
                    }
                }
                Ok(None) => {
                    // Channel closed
                    inner.closed = true;
                    return Err(ShellSessionError::SessionDead(
                        "Remote channel closed unexpectedly".to_string(),
                    ));
                }
                Err(_) => {
                    // Timeout on wait, continue loop
                    continue;
                }
            }
        }

        let duration = start.elapsed();

        // Clean up output
        let cleaned_output = Self::clean_output(&output, command, &marker);

        debug!(
            session_id = %self.session_id,
            duration_ms = duration.as_millis(),
            output_len = cleaned_output.len(),
            "Remote command completed"
        );

        Ok(CommandOutput {
            output: cleaned_output,
            exit_code: None, // PTY doesn't easily give us exit codes
            duration,
        })
    }

    /// Clean output by removing command echo, marker, and shell artifacts
    fn clean_output(raw_output: &str, command: &str, marker: &str) -> String {
        // First, strip ANSI escape codes
        let stripped = console::strip_ansi_codes(raw_output);

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
            !(trimmed == "$"
                || trimmed == "#"
                || trimmed == ">"
                || trimmed.ends_with("$ ")
                || trimmed.ends_with("# ")
                || trimmed.ends_with("> ")
                || (trimmed.starts_with("[") && trimmed.contains("]$")))
        });

        lines.join("\n")
    }
}

struct ParsedConnection {
    username: String,
    hostname: String,
    port: u16,
}

#[async_trait]
impl ShellSession for RemoteShellSession {
    async fn execute(
        &self,
        command: &str,
        timeout: Option<Duration>,
    ) -> Result<CommandOutput, ShellSessionError> {
        self.execute_with_marker(command, timeout).await
    }

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

        // Check if session is closed and send command
        {
            let inner = self.inner.lock().await;
            if inner.closed {
                return Err(ShellSessionError::SessionClosed);
            }

            // Send command followed by marker echo
            let full_command = format!("{}\necho \"{}\"\n", command.trim(), marker);
            inner
                .channel
                .data(full_command.as_bytes())
                .await
                .map_err(|e| {
                    ShellSessionError::SshError(format!("Failed to send command: {}", e))
                })?;
        }

        trace!(command = %command, marker = %marker, "Sent command to remote for streaming");

        // Clone what we need for the spawned task
        let inner_arc = self.inner.clone();
        let session_id = self.session_id.clone();
        let command_owned = command.to_string();
        let marker_clone = marker.clone();

        // Spawn task to read output and stream it
        let handle = tokio::spawn(async move {
            let mut output = String::new();
            let mut last_streamed_len = 0usize; // Track what we've already streamed

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

                let mut inner = inner_arc.lock().await;

                if inner.closed {
                    let _ = tx
                        .send(OutputChunk {
                            text: "\n[SESSION CLOSED]\n".to_string(),
                            is_final: true,
                        })
                        .await;
                    return Err(ShellSessionError::SessionClosed);
                }

                // Wait for data with timeout
                match tokio::time::timeout(Duration::from_millis(100), inner.channel.wait()).await {
                    Ok(Some(msg)) => {
                        match msg {
                            russh::ChannelMsg::Data { data } => {
                                let chunk = String::from_utf8_lossy(&data);
                                output.push_str(&chunk);

                                // Clean the entire accumulated output and stream only new complete lines
                                let cleaned_so_far =
                                    clean_shell_output(&output, &command_owned, &marker_clone);
                                if cleaned_so_far.len() > last_streamed_len {
                                    let new_content = &cleaned_so_far[last_streamed_len..];
                                    // Only stream complete lines to avoid flicker
                                    if let Some(last_newline) = new_content.rfind('\n') {
                                        let complete_lines = &new_content[..=last_newline];
                                        if !complete_lines.trim().is_empty() {
                                            let _ = tx
                                                .send(OutputChunk {
                                                    text: complete_lines.to_string(),
                                                    is_final: false,
                                                })
                                                .await;
                                        }
                                        last_streamed_len += last_newline + 1;
                                    }
                                }

                                // Check for marker completion
                                let marker_count = output.matches(&marker_clone).count();
                                if marker_count >= 2 {
                                    break;
                                }
                            }
                            russh::ChannelMsg::ExtendedData { data, .. } => {
                                let chunk = String::from_utf8_lossy(&data);
                                output.push_str(&chunk);

                                // Clean and stream stderr using same approach
                                let cleaned_so_far =
                                    clean_shell_output(&output, &command_owned, &marker_clone);
                                if cleaned_so_far.len() > last_streamed_len {
                                    let new_content = &cleaned_so_far[last_streamed_len..];
                                    // Only stream complete lines to avoid flicker
                                    if let Some(last_newline) = new_content.rfind('\n') {
                                        let complete_lines = &new_content[..=last_newline];
                                        if !complete_lines.trim().is_empty() {
                                            let _ = tx
                                                .send(OutputChunk {
                                                    text: complete_lines.to_string(),
                                                    is_final: false,
                                                })
                                                .await;
                                        }
                                        last_streamed_len += last_newline + 1;
                                    }
                                }
                            }
                            russh::ChannelMsg::Eof => {
                                let _ = tx
                                    .send(OutputChunk {
                                        text: "\n[EOF]\n".to_string(),
                                        is_final: true,
                                    })
                                    .await;
                                return Err(ShellSessionError::SessionDead(
                                    "Remote shell closed".to_string(),
                                ));
                            }
                            russh::ChannelMsg::Close => {
                                inner.closed = true;
                                let _ = tx
                                    .send(OutputChunk {
                                        text: "\n[CHANNEL CLOSED]\n".to_string(),
                                        is_final: true,
                                    })
                                    .await;
                                return Err(ShellSessionError::SessionDead(
                                    "Remote channel closed".to_string(),
                                ));
                            }
                            _ => {}
                        }
                    }
                    Ok(None) => {
                        inner.closed = true;
                        let _ = tx
                            .send(OutputChunk {
                                text: "\n[CHANNEL CLOSED]\n".to_string(),
                                is_final: true,
                            })
                            .await;
                        return Err(ShellSessionError::SessionDead(
                            "Remote channel closed unexpectedly".to_string(),
                        ));
                    }
                    Err(_) => {
                        // Timeout on wait, continue loop
                        continue;
                    }
                }
            }

            let duration = start.elapsed();

            // Clean up output
            let cleaned_output =
                RemoteShellSession::clean_output(&output, &command_owned, &marker_clone);

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
                "Remote streaming command completed"
            );

            Ok(CommandOutput {
                output: cleaned_output,
                exit_code: None,
                duration,
            })
        });

        Ok((rx, handle))
    }

    async fn is_alive(&self) -> bool {
        let inner = self.inner.lock().await;
        !inner.closed
    }

    fn session_id(&self) -> &str {
        &self.session_id
    }

    fn description(&self) -> String {
        format!(
            "remote:{}:{}",
            self.connection_info.connection_string, self.session_id
        )
    }

    async fn close(&mut self) -> Result<(), ShellSessionError> {
        let mut inner = self.inner.lock().await;
        if inner.closed {
            return Ok(());
        }

        // Send exit command
        let _ = inner.channel.data(&b"exit\n"[..]).await;

        // Close channel
        let _ = inner.channel.close().await;

        inner.closed = true;
        debug!(session_id = %self.session_id, "Remote shell session closed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_generation() {
        let marker1 = RemoteShellSession::generate_marker();
        let marker2 = RemoteShellSession::generate_marker();

        assert!(marker1.starts_with(MARKER_PREFIX));
        assert!(marker1.ends_with(MARKER_SUFFIX));
        assert_ne!(marker1, marker2, "Markers should be unique");
    }

    #[test]
    fn test_clean_output() {
        let raw = "echo hello\nhello\n__STAKPAK_CMD_END_abc123__\n$ ";
        let cleaned =
            RemoteShellSession::clean_output(raw, "echo hello", "__STAKPAK_CMD_END_abc123__");
        assert_eq!(cleaned.trim(), "hello");
    }

    #[test]
    fn test_clean_output_with_ansi_codes() {
        // Test output with ANSI escape codes (common in remote shells)
        let raw = "\x1b[32mecho hello\x1b[0m\nhello\n__STAKPAK_CMD_END_abc123__\n\x1b[1m$ \x1b[0m";
        let cleaned =
            RemoteShellSession::clean_output(raw, "echo hello", "__STAKPAK_CMD_END_abc123__");
        assert!(
            cleaned.contains("hello"),
            "Should extract hello from ANSI-coded output, got: '{}'",
            cleaned
        );
    }

    #[test]
    fn test_clean_output_multiline() {
        let raw = "ls -la\nfile1.txt\nfile2.txt\nfile3.txt\n__STAKPAK_CMD_END_xyz789__\n$ ";
        let cleaned = RemoteShellSession::clean_output(raw, "ls -la", "__STAKPAK_CMD_END_xyz789__");
        assert!(cleaned.contains("file1.txt"), "Should contain file1.txt");
        assert!(cleaned.contains("file2.txt"), "Should contain file2.txt");
        assert!(cleaned.contains("file3.txt"), "Should contain file3.txt");
        assert!(
            !cleaned.contains("__STAKPAK_CMD_END"),
            "Should not contain marker"
        );
    }

    #[test]
    fn test_parse_connection_string() {
        let result = RemoteShellSession::parse_connection_string("user@host.com");
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.username, "user");
        assert_eq!(parsed.hostname, "host.com");
        assert_eq!(parsed.port, 22);

        let result = RemoteShellSession::parse_connection_string("admin@server.io:2222");
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.username, "admin");
        assert_eq!(parsed.hostname, "server.io");
        assert_eq!(parsed.port, 2222);
    }

    #[test]
    fn test_parse_connection_string_invalid() {
        // Missing @ symbol
        let result = RemoteShellSession::parse_connection_string("userhost.com");
        assert!(result.is_err(), "Should fail without @ symbol");

        // Invalid port
        let result = RemoteShellSession::parse_connection_string("user@host:notaport");
        assert!(result.is_err(), "Should fail with invalid port");
    }

    #[test]
    fn test_parse_connection_string_edge_cases() {
        // Username with dots
        let result = RemoteShellSession::parse_connection_string("user.name@host.com");
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.username, "user.name");

        // Hostname with subdomain
        let result = RemoteShellSession::parse_connection_string("admin@sub.domain.example.com:22");
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.hostname, "sub.domain.example.com");
        assert_eq!(parsed.port, 22);

        // IP address
        let result = RemoteShellSession::parse_connection_string("root@192.168.1.100:2222");
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.hostname, "192.168.1.100");
        assert_eq!(parsed.port, 2222);
    }
}
