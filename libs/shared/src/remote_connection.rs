use crate::utils::{DirectoryEntry, FileSystemProvider};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use russh::client::{self, Handler};
use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::{self, Display},
    fs,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tracing::debug;
use uuid;

#[derive(Debug)]
struct ParsedConnection {
    username: String,
    hostname: String,
    port: u16,
}

pub struct CommandOptions {
    pub timeout: Option<Duration>,
    pub with_progress: bool,
    pub simple: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConnectionInfo {
    pub connection_string: String, // format: user@host:port
    pub password: Option<String>,
    pub private_key_path: Option<String>,
}

impl RemoteConnectionInfo {
    fn parse_connection_string(&self) -> Result<ParsedConnection> {
        let (username, host_port) = self.connection_string.split_once('@').ok_or_else(|| {
            anyhow!("Invalid connection string format. Expected: user@host or user@host:port")
        })?;

        let (hostname, port) = if let Some((host, port_str)) = host_port.split_once(':') {
            let port = port_str
                .parse::<u16>()
                .map_err(|_| anyhow!("Invalid port number: {}", port_str))?;
            (host, port)
        } else {
            (host_port, 22)
        };

        Ok(ParsedConnection {
            username: username.to_string(),
            hostname: hostname.to_string(),
            port,
        })
    }
}

pub struct SSHClient;

impl Handler for SSHClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        // In production, you might want to verify the server key against known hosts
        // For now, we accept all keys to avoid "Unknown server key" errors
        Ok(true)
    }
}

pub struct RemoteConnection {
    sftp: SftpSession,
    connection_info: RemoteConnectionInfo,
}

impl RemoteConnection {
    fn map_ssh_error(error: russh::Error, context: &str) -> anyhow::Error {
        anyhow!("SSH {}: {}", context, error)
    }

    fn map_auth_error(result: russh::client::AuthResult, method: &str) -> Result<()> {
        match result {
            russh::client::AuthResult::Success => Ok(()),
            _ => Err(anyhow!("{} authentication failed", method)),
        }
    }

    async fn create_authenticated_session_static(
        connection_info: &RemoteConnectionInfo,
    ) -> Result<client::Handle<SSHClient>> {
        let parsed = connection_info.parse_connection_string()?;

        debug!(
            "Connecting to {}@{}:{}",
            parsed.username, parsed.hostname, parsed.port
        );

        let config = client::Config::default();
        let mut session = client::connect(
            config.into(),
            (parsed.hostname.as_str(), parsed.port),
            SSHClient {},
        )
        .await
        .map_err(|e| Self::map_ssh_error(e, "connection failed"))?;

        Self::authenticate_session_static(&mut session, &parsed.username, connection_info).await?;
        Ok(session)
    }

    async fn authenticate_session_static(
        session: &mut client::Handle<SSHClient>,
        username: &str,
        connection_info: &RemoteConnectionInfo,
    ) -> Result<()> {
        if let Some(password) = &connection_info.password {
            debug!("Authenticating with password");
            let auth_result = session
                .authenticate_password(username, password)
                .await
                .map_err(|e| Self::map_ssh_error(e, "password authentication"))?;
            Self::map_auth_error(auth_result, "Password")?;
        } else {
            debug!("Authenticating with public key");
            let private_key_path = if let Some(path) = &connection_info.private_key_path {
                Self::canonicalize_key_path(path)?
            } else {
                Self::get_default_key_files()?.0
            };

            let keypair = russh::keys::load_secret_key(&private_key_path, None).map_err(|e| {
                anyhow!(
                    "Failed to load private key from {}: {}",
                    private_key_path.display(),
                    e
                )
            })?;

            let auth_result = session
                .authenticate_publickey(
                    username,
                    russh::keys::PrivateKeyWithHashAlg::new(
                        Arc::new(keypair),
                        Some(russh::keys::HashAlg::Sha256),
                    ),
                )
                .await
                .map_err(|e| Self::map_ssh_error(e, "public key authentication"))?;
            Self::map_auth_error(auth_result, "Public key")?;
        }
        Ok(())
    }

    pub fn get_default_key_files() -> Result<(PathBuf, PathBuf)> {
        let home_dir = dirs::home_dir().ok_or_else(|| anyhow!("Home directory not found"))?;
        let ssh_dir = home_dir.join(".ssh");

        if !ssh_dir.is_dir() {
            return Err(anyhow!("SSH directory not found: {}", ssh_dir.display()));
        }

        // Try common key file names in order of preference
        let key_names = ["id_ed25519", "id_rsa", "id_ecdsa", "id_dsa"];

        for key_name in &key_names {
            let private_key = ssh_dir.join(key_name);
            let public_key = ssh_dir.join(format!("{}.pub", key_name));

            if private_key.is_file() {
                return Ok((private_key, public_key));
            }
        }

        Err(anyhow!("No SSH private key found in {}", ssh_dir.display()))
    }

    /// Canonicalize a key path, handling both absolute and relative paths
    pub fn canonicalize_key_path(path: &str) -> Result<PathBuf> {
        let path_buf = PathBuf::from(path);

        // If it's already absolute, try to canonicalize directly
        if path_buf.is_absolute() {
            return fs::canonicalize(&path_buf)
                .map_err(|e| anyhow!("Failed to access private key at {}: {}", path, e));
        }

        // For relative paths, try current directory first
        if let Ok(canonical) = fs::canonicalize(&path_buf) {
            return Ok(canonical);
        }

        // If that fails, try relative to ~/.ssh/
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow!("Home directory not found for relative key path"))?;
        let ssh_relative_path = home_dir.join(".ssh").join(&path_buf);

        if ssh_relative_path.exists() {
            return fs::canonicalize(ssh_relative_path)
                .map_err(|e| anyhow!("Failed to access private key at ~/.ssh/{}: {}", path, e));
        }

        // If still not found, try to expand ~ manually
        if let Some(stripped) = path.strip_prefix("~/") {
            let expanded_path = home_dir.join(stripped);
            return fs::canonicalize(expanded_path)
                .map_err(|e| anyhow!("Failed to access private key at {}: {}", path, e));
        }

        Err(anyhow!(
            "Private key not found at {} (tried current directory and ~/.ssh/)",
            path
        ))
    }

    pub async fn new(connection_info: RemoteConnectionInfo) -> Result<Self> {
        let session = Self::create_authenticated_session_static(&connection_info).await?;

        // Open SFTP channel
        let channel = session
            .channel_open_session()
            .await
            .map_err(|e| Self::map_ssh_error(e, "failed to open SSH channel"))?;

        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| Self::map_ssh_error(e, "failed to request SFTP subsystem"))?;

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| anyhow!("Failed to create SFTP session: {}", e))?;

        debug!("SFTP connection established successfully");

        Ok(Self {
            sftp,
            connection_info,
        })
    }

    pub async fn separator(&self) -> Result<char> {
        // Try to determine the path separator by canonicalizing root
        let canonical_path = self.sftp.canonicalize("/").await?;
        Ok(if canonical_path.contains('\\') {
            '\\'
        } else {
            '/'
        })
    }

    pub async fn canonicalize(&self, path: &str) -> Result<String> {
        self.sftp
            .canonicalize(path)
            .await
            .map_err(|e| anyhow!("Failed to canonicalize path {}: {}", path, e))
    }

    /// Get the SSH connection string in the format user@host: or user@host#port:
    /// Uses # as port separator to distinguish from path separators in SSH URLs
    pub fn get_ssh_prefix(&self) -> Result<String> {
        let parsed = self.connection_info.parse_connection_string()?;
        if parsed.port == 22 {
            Ok(format!("{}@{}:", parsed.username, parsed.hostname))
        } else {
            Ok(format!(
                "{}@{}#{}:",
                parsed.username, parsed.hostname, parsed.port
            ))
        }
    }

    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        self.sftp
            .read(path)
            .await
            .map_err(|e| anyhow!("Failed to read file {}: {}", path, e))
    }

    pub async fn read_file_to_string(&self, path: &str) -> Result<String> {
        let content = self.read_file(path).await?;
        String::from_utf8(content)
            .map_err(|e| anyhow!("File {} contains invalid UTF-8: {}", path, e))
    }

    pub async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        self.sftp
            .write(path, data)
            .await
            .map_err(|e| anyhow!("Failed to write file {}: {}", path, e))
    }

    pub async fn create_file(&self, path: &str, data: &[u8]) -> Result<()> {
        // Create the file and get a handle
        let mut file_handle = self
            .sftp
            .create(path)
            .await
            .map_err(|e| anyhow!("Failed to create file {}: {}", path, e))?;

        // Write data to the file handle
        file_handle
            .write_all(data)
            .await
            .map_err(|e| anyhow!("Failed to write data to file {}: {}", path, e))?;

        // File handle is automatically closed when dropped
        Ok(())
    }

    pub async fn create_directories(&self, path: &str) -> Result<()> {
        let path_buf = PathBuf::from(path);
        let mut current_path = PathBuf::new();

        for component in path_buf.components() {
            current_path.push(component);
            let path_str = current_path.to_string_lossy().to_string();

            if self.sftp.read_dir(&path_str).await.is_err() {
                self.sftp
                    .create_dir(&path_str)
                    .await
                    .map_err(|e| anyhow!("Failed to create directory {}: {}", path_str, e))?;
            }
        }

        Ok(())
    }

    pub async fn list_directory(&self, path: &str) -> Result<Vec<String>> {
        let entries = self
            .sftp
            .read_dir(path)
            .await
            .map_err(|e| anyhow!("Failed to read directory {}: {}", path, e))?;

        let separator = self.separator().await?;
        let mut result = Vec::new();

        for entry in entries {
            let entry_path = if path.ends_with(separator) {
                format!("{}{}", path, entry.file_name())
            } else {
                format!("{}{}{}", path, separator, entry.file_name())
            };
            result.push(entry_path);
        }

        result.sort();
        Ok(result)
    }

    /// List directory with file type information (more efficient for tree generation)
    pub async fn list_directory_with_types(&self, path: &str) -> Result<Vec<(String, bool)>> {
        let entries = self
            .sftp
            .read_dir(path)
            .await
            .map_err(|e| anyhow!("Failed to read directory {}: {}", path, e))?;

        let separator = self.separator().await?;
        let mut result = Vec::new();

        for entry in entries {
            let entry_path = if path.ends_with(separator) {
                format!("{}{}", path, entry.file_name())
            } else {
                format!("{}{}{}", path, separator, entry.file_name())
            };
            let is_directory = entry.metadata().is_dir();
            result.push((entry_path, is_directory));
        }

        result.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(result)
    }

    pub async fn is_file(&self, path: &str) -> bool {
        self.sftp
            .metadata(path)
            .await
            .map(|metadata| !metadata.is_dir())
            .unwrap_or(false)
    }

    pub async fn is_directory(&self, path: &str) -> bool {
        self.sftp
            .metadata(path)
            .await
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
    }

    pub async fn exists(&self, path: &str) -> bool {
        self.sftp.metadata(path).await.is_ok()
    }

    pub async fn file_size(&self, path: &str) -> Result<u64> {
        let metadata = self
            .sftp
            .metadata(path)
            .await
            .map_err(|e| anyhow!("Failed to get metadata for {}: {}", path, e))?;

        Ok(metadata.len())
    }

    pub async fn rename(&self, old_path: &str, new_path: &str) -> Result<()> {
        self.sftp
            .rename(old_path, new_path)
            .await
            .map_err(|e| anyhow!("Failed to rename '{}' to '{}': {}", old_path, new_path, e))
    }

    pub async fn execute_command_unified(
        &self,
        command: &str,
        options: CommandOptions,
        cancel_rx: &mut tokio::sync::oneshot::Receiver<()>,
        progress_callback: Option<impl Fn(String) + Send + Sync + 'static>,
        ctx: Option<&rmcp::service::RequestContext<rmcp::RoleServer>>,
    ) -> Result<(String, i32)> {
        use regex::Regex;

        let session = Self::create_authenticated_session_static(&self.connection_info).await?;

        // Execute command
        let mut channel = session
            .channel_open_session()
            .await
            .map_err(|e| Self::map_ssh_error(e, "failed to open channel"))?;

        // Wrap the command to get the PID if we need it for cancellation (when not simple)
        let wrapped_command = if options.simple {
            command.to_string()
        } else {
            // Escape characters that have special meaning inside double quotes in bash:
            // \ " $ ` ! need escaping, and | needs escaping to prevent pipe interpretation
            let escaped_command = command
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('$', "\\$")
                .replace('`', "\\`")
                .replace('!', "\\!");
            format!(
                "bash -c 'echo \"PID:$$\"; exec bash -c \"{}\"'",
                escaped_command
            )
        };

        channel
            .exec(true, wrapped_command.as_str())
            .await
            .map_err(|e| Self::map_ssh_error(e, "failed to execute command"))?;

        let mut output = String::new();
        let mut exit_code = 0i32;
        let mut remote_pid: Option<String> = None;
        let progress_id = uuid::Uuid::new_v4();

        // Compile regex for PID extraction if needed
        let pid_regex = if !options.simple {
            Some(Regex::new(r"PID:(\d+)").expect("Invalid PID regex"))
        } else {
            None
        };

        // Stream output with progress notifications
        let command_execution = async {
            while let Some(msg) = channel.wait().await {
                match msg {
                    russh::ChannelMsg::Data { data } => {
                        let text = String::from_utf8_lossy(&data).to_string();

                        // Extract PID from the output using regex for non-simple commands
                        if let Some(ref regex) = pid_regex
                            && remote_pid.is_none()
                            && let Some(captures) = regex.captures(&text)
                            && let Some(pid_match) = captures.get(1)
                        {
                            remote_pid = Some(pid_match.as_str().to_string());
                            // Remove the PID line from output
                            let cleaned_text = regex.replace_all(&text, "").to_string();
                            if !cleaned_text.trim().is_empty() {
                                output.push_str(&cleaned_text);
                                if let Some(ref callback) = progress_callback {
                                    callback(cleaned_text);
                                }
                            }
                            continue;
                        }

                        // Normal output processing
                        output.push_str(&text);
                        if let Some(ref callback) = progress_callback {
                            callback(text.clone());
                        }

                        // Send MCP progress notification if context is provided
                        if let Some(ctx) = &ctx
                            && options.with_progress
                            && !text.trim().is_empty()
                        {
                            let _ = ctx.peer.notify_progress(rmcp::model::ProgressNotificationParam {
                                    progress_token: rmcp::model::ProgressToken(rmcp::model::NumberOrString::Number(0)),
                                    progress: 50.0,
                                    total: Some(100.0),
                                    message: Some(serde_json::to_string(&crate::models::integrations::openai::ToolCallResultProgress {
                                        id: progress_id,
                                        message: text,
                                        progress_type: None,
                                        task_updates: None,
                                        progress: None,
                                    }).unwrap_or_default()),
                                }).await;
                        }
                    }
                    russh::ChannelMsg::ExtendedData { data, ext: _ } => {
                        let text = String::from_utf8_lossy(&data).to_string();
                        output.push_str(&text);
                        if let Some(ref callback) = progress_callback {
                            callback(text.clone());
                        }

                        // Send MCP progress notification for stderr if context is provided
                        if let Some(ctx) = &ctx
                            && options.with_progress
                            && !text.trim().is_empty()
                        {
                            let _ = ctx.peer.notify_progress(rmcp::model::ProgressNotificationParam {
                                    progress_token: rmcp::model::ProgressToken(rmcp::model::NumberOrString::Number(0)),
                                    progress: 50.0,
                                    total: Some(100.0),
                                    message: Some(serde_json::to_string(&crate::models::integrations::openai::ToolCallResultProgress {
                                        id: progress_id,
                                        message: text,
                                        progress_type: None,
                                        task_updates: None,
                                        progress: None,
                                    }).unwrap_or_default()),
                                }).await;
                        }
                    }
                    russh::ChannelMsg::ExitStatus { exit_status } => {
                        exit_code = exit_status as i32;
                    }
                    russh::ChannelMsg::Eof => {
                        break;
                    }
                    _ => {}
                }
            }
        };

        // Macro to handle cancellation cleanup - avoids lifetime issues
        macro_rules! handle_cancellation {
            ($error_msg:expr) => {{
                // Kill the remote process before closing the channel if we have the PID
                if let Some(pid) = &remote_pid {
                    let kill_cmd = format!("kill -9 {}", pid);
                    if let Ok(kill_channel) = session.channel_open_session().await {
                        let _ = kill_channel.exec(true, kill_cmd.as_str()).await;
                        let _ = kill_channel.close().await;
                    }
                }
                let _ = channel.close().await;
                Err(anyhow!($error_msg))
            }};
        }

        // Execute with unified select handling timeout and cancellation
        tokio::select! {
            // Main command execution
            _ = command_execution => Ok((output, exit_code)),

            // Timeout handling (only if timeout is specified)
            _ = async {
                if let Some(timeout_duration) = options.timeout {
                    tokio::time::sleep(timeout_duration).await;
                } else {
                    // If no timeout, wait forever
                    std::future::pending::<()>().await;
                }
            } => {
                handle_cancellation!(format!("Command timed out after {:?}", options.timeout))
            },

            // Context cancellation (only if ctx is provided)
            _ = async {
                if let Some(ctx) = &ctx {
                    ctx.ct.cancelled().await;
                } else {
                    // If no context, wait forever
                    std::future::pending::<()>().await;
                }
            } => {
                handle_cancellation!("Command was cancelled")
            },

            // Manual cancellation via channel
            _ = cancel_rx => {
                handle_cancellation!("Command was cancelled")
            }
        }
    }

    pub async fn execute_command(
        &self,
        command: &str,
        timeout: Option<Duration>,
        ctx: Option<&rmcp::service::RequestContext<rmcp::RoleServer>>,
    ) -> Result<(String, i32)> {
        let options = CommandOptions {
            timeout,
            with_progress: true,
            simple: false,
        };

        let (_cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();

        self.execute_command_unified(command, options, &mut cancel_rx, None::<fn(String)>, ctx)
            .await
    }

    pub async fn execute_command_with_streaming<F>(
        &self,
        command: &str,
        timeout: Option<Duration>,
        cancel_rx: &mut tokio::sync::oneshot::Receiver<()>,
        progress_callback: F,
    ) -> Result<(String, i32)>
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        let options = CommandOptions {
            timeout,
            with_progress: false,
            simple: false,
        };

        self.execute_command_unified(command, options, cancel_rx, Some(progress_callback), None)
            .await
    }

    pub fn connection_string(&self) -> &str {
        &self.connection_info.connection_string
    }
}

/// Remote file system provider implementation for tree generation
pub struct RemoteFileSystemProvider {
    connection: Arc<RemoteConnection>,
}

impl RemoteFileSystemProvider {
    pub fn new(connection: Arc<RemoteConnection>) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl FileSystemProvider for RemoteFileSystemProvider {
    type Error = String;

    async fn list_directory(&self, path: &str) -> Result<Vec<DirectoryEntry>, Self::Error> {
        // Reduce timeout for better responsiveness in tree operations
        let timeout_duration = std::time::Duration::from_secs(10);

        let entries = tokio::time::timeout(
            timeout_duration,
            self.connection.list_directory_with_types(path),
        )
        .await
        .map_err(|_| format!("Timeout listing remote directory: {}", path))?
        .map_err(|e| format!("Failed to list remote directory: {}", e))?;

        let mut result = Vec::new();
        for (entry_path, is_directory) in entries {
            let name = entry_path
                .split('/')
                .next_back()
                .unwrap_or(&entry_path)
                .to_string();

            result.push(DirectoryEntry {
                name,
                path: entry_path,
                is_directory,
            });
        }

        Ok(result)
    }
}

impl Display for RemoteConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SSH:{}", self.connection_info.connection_string)
    }
}

// Global connection manager
pub struct RemoteConnectionManager {
    connections: RwLock<HashMap<String, Arc<RemoteConnection>>>,
}

impl RemoteConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
        }
    }

    pub async fn get_connection(
        &self,
        connection_info: &RemoteConnectionInfo,
    ) -> Result<Arc<RemoteConnection>> {
        let key = connection_info.connection_string.clone();

        // Check if connection already exists
        {
            let connections = self.connections.read().await;
            if let Some(conn) = connections.get(&key) {
                return Ok(conn.clone());
            }
        }

        // Create new connection
        let connection = RemoteConnection::new(connection_info.clone()).await?;
        let arc_connection = Arc::new(connection);

        // Store connection
        {
            let mut connections = self.connections.write().await;
            connections.insert(key, arc_connection.clone());
        }

        Ok(arc_connection)
    }

    pub async fn remove_connection(&self, connection_string: &str) {
        let mut connections = self.connections.write().await;
        connections.remove(connection_string);
    }

    pub async fn list_connections(&self) -> Vec<String> {
        let connections = self.connections.read().await;
        connections.keys().cloned().collect()
    }
}

impl Default for RemoteConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub enum PathLocation {
    Local(String),
    Remote {
        connection: RemoteConnectionInfo,
        path: String,
    },
}

impl PathLocation {
    /// Parse a path that might be local or remote
    /// Remote paths are in the format: ssh://user@host:port/path or user@host:/path
    pub fn parse(path_str: &str) -> Result<Self> {
        if let Some(without_scheme) = path_str.strip_prefix("ssh://") {
            // Format: ssh://user@host:port/path

            if let Some((connection_part, path_part)) = without_scheme.split_once('/') {
                let connection_info = RemoteConnectionInfo {
                    connection_string: connection_part.to_string(),
                    password: None,
                    private_key_path: None,
                };

                return Ok(PathLocation::Remote {
                    connection: connection_info,
                    path: format!("/{}", path_part),
                });
            }
        } else if path_str.contains('@') && path_str.contains(':') {
            // Format: user@host:/path (traditional SCP format)
            if let Some((connection_part, path_part)) = path_str.split_once(':')
                && path_part.starts_with('/')
            {
                let connection_info = RemoteConnectionInfo {
                    connection_string: connection_part.to_string(),
                    password: None,
                    private_key_path: None,
                };

                return Ok(PathLocation::Remote {
                    connection: connection_info,
                    path: path_part.to_string(),
                });
            }
        }

        // Default to local path
        Ok(PathLocation::Local(path_str.to_string()))
    }

    pub fn is_remote(&self) -> bool {
        matches!(self, PathLocation::Remote { .. })
    }

    pub fn as_local_path(&self) -> Option<&str> {
        match self {
            PathLocation::Local(path) => Some(path),
            PathLocation::Remote { .. } => None,
        }
    }

    pub fn as_remote_info(&self) -> Option<(&RemoteConnectionInfo, &str)> {
        match self {
            PathLocation::Local(_) => None,
            PathLocation::Remote { connection, path } => Some((connection, path)),
        }
    }
}
