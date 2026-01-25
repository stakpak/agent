//! Shell Session Manager
//!
//! Central manager for creating, tracking, and destroying persistent shell sessions.

use super::session::{ShellSession, ShellSessionError};
use crate::helper::generate_simple_id;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Configuration for shell sessions
#[derive(Debug, Clone)]
pub struct ShellSessionConfig {
    /// Enable persistent shell sessions
    pub enabled: bool,

    /// Default shell for local sessions (auto-detect if None)
    pub default_shell: Option<String>,

    /// Session timeout in seconds (0 = no timeout)
    pub session_timeout: Duration,

    /// Maximum concurrent sessions
    pub max_sessions: usize,

    /// Default command timeout in seconds
    pub command_timeout: Duration,
}

impl Default for ShellSessionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_shell: None,
            session_timeout: Duration::from_secs(3600), // 1 hour
            max_sessions: 10,
            command_timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Information about a managed session
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub description: String,
    pub is_remote: bool,
    pub created_at: std::time::Instant,
    pub last_used: std::time::Instant,
}

/// Central manager for shell sessions
///
/// Handles session lifecycle including creation, lookup, and cleanup.
/// Thread-safe and can be shared across multiple tool invocations.
pub struct ShellSessionManager {
    /// Active sessions indexed by session ID
    sessions: RwLock<HashMap<String, Arc<RwLock<Box<dyn ShellSession>>>>>,

    /// Session metadata for quick lookups without locking sessions
    session_info: RwLock<HashMap<String, SessionInfo>>,

    /// Configuration
    config: ShellSessionConfig,
}

impl ShellSessionManager {
    /// Create a new session manager with the given configuration
    pub fn new(config: ShellSessionConfig) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            session_info: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Create a new session manager with default configuration
    pub fn with_defaults() -> Self {
        Self::new(ShellSessionConfig::default())
    }

    /// Get the current configuration
    pub fn config(&self) -> &ShellSessionConfig {
        &self.config
    }

    /// Check if sessions are enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the number of active sessions
    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Default session ID for local commands
    const DEFAULT_LOCAL_SESSION_ID: &'static str = "default-local";

    /// Default session ID prefix for remote commands
    const DEFAULT_REMOTE_SESSION_PREFIX: &'static str = "default-remote-";

    /// Get or create the default local session
    ///
    /// This provides automatic persistent shell behavior without explicit session management.
    pub async fn get_or_create_default_local_session(&self) -> Result<String, ShellSessionError> {
        // Check if default session already exists
        if self
            .get_session(Self::DEFAULT_LOCAL_SESSION_ID)
            .await
            .is_some()
        {
            return Ok(Self::DEFAULT_LOCAL_SESSION_ID.to_string());
        }

        // Create new default local session
        let session = super::LocalShellSession::new(
            self.config.default_shell.as_deref(),
            None, // Use current directory
            None, // Default rows
            None, // Default cols
        )?;

        // Override the session ID to use our default ID
        let session_with_id = DefaultIdLocalSession {
            inner: session,
            id: Self::DEFAULT_LOCAL_SESSION_ID.to_string(),
        };

        self.register_session(Box::new(session_with_id), false)
            .await?;
        Ok(Self::DEFAULT_LOCAL_SESSION_ID.to_string())
    }

    /// Get or create a default remote session for a given connection string
    ///
    /// Each unique remote connection gets its own default session.
    pub async fn get_or_create_default_remote_session(
        &self,
        connection_string: &str,
        password: Option<String>,
        private_key_path: Option<String>,
    ) -> Result<String, ShellSessionError> {
        // Create a deterministic session ID based on connection string
        let session_id = format!(
            "{}{}",
            Self::DEFAULT_REMOTE_SESSION_PREFIX,
            connection_string.replace(['@', ':', '.'], "-")
        );

        // Check if session already exists
        if self.get_session(&session_id).await.is_some() {
            return Ok(session_id);
        }

        // Create new remote session
        let connection_info = crate::remote_connection::RemoteConnectionInfo {
            connection_string: connection_string.to_string(),
            password,
            private_key_path,
        };

        let session = super::RemoteShellSession::new(connection_info, None, None).await?;

        // Override the session ID to use our default ID
        let session_with_id = DefaultIdRemoteSession {
            inner: session,
            id: session_id.clone(),
        };

        self.register_session(Box::new(session_with_id), true)
            .await?;
        Ok(session_id)
    }

    /// Register a new session with the manager
    ///
    /// # Arguments
    /// * `session` - The session to register
    /// * `is_remote` - Whether this is a remote (SSH) session
    ///
    /// # Returns
    /// * `Ok(session_id)` - The ID assigned to the session
    /// * `Err` - If max sessions reached or registration fails
    pub async fn register_session(
        &self,
        session: Box<dyn ShellSession>,
        is_remote: bool,
    ) -> Result<String, ShellSessionError> {
        // Check session limit
        let current_count = self.session_count().await;
        if current_count >= self.config.max_sessions {
            return Err(ShellSessionError::ExecutionFailed(format!(
                "Maximum session limit ({}) reached. Close unused sessions first.",
                self.config.max_sessions
            )));
        }

        let session_id = session.session_id().to_string();
        let description = session.description();
        let now = std::time::Instant::now();

        let info = SessionInfo {
            id: session_id.clone(),
            description,
            is_remote,
            created_at: now,
            last_used: now,
        };

        // Store session and info
        {
            let mut sessions = self.sessions.write().await;
            let mut session_info = self.session_info.write().await;

            sessions.insert(session_id.clone(), Arc::new(RwLock::new(session)));
            session_info.insert(session_id.clone(), info);
        }

        info!(session_id = %session_id, is_remote = is_remote, "Registered new shell session");
        Ok(session_id)
    }

    /// Get a session by ID
    ///
    /// Returns None if session doesn't exist or has been closed.
    pub async fn get_session(
        &self,
        session_id: &str,
    ) -> Option<Arc<RwLock<Box<dyn ShellSession>>>> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// Execute a command in a specific session
    ///
    /// Updates the last_used timestamp on successful execution.
    pub async fn execute_in_session(
        &self,
        session_id: &str,
        command: &str,
        timeout: Option<Duration>,
    ) -> Result<super::session::CommandOutput, ShellSessionError> {
        let session = self.get_session(session_id).await.ok_or_else(|| {
            ShellSessionError::SessionDead(format!("Session {} not found", session_id))
        })?;

        let timeout = timeout.unwrap_or(self.config.command_timeout);

        // Execute command
        let result = {
            let session_guard = session.read().await;
            session_guard.execute(command, Some(timeout)).await
        };

        // Update last_used on success
        if result.is_ok() {
            let mut session_info = self.session_info.write().await;
            if let Some(info) = session_info.get_mut(session_id) {
                info.last_used = std::time::Instant::now();
            }
        }

        result
    }

    /// Execute a command in a session with streaming output
    ///
    /// Returns a receiver for output chunks and a join handle for the final result.
    pub async fn execute_in_session_streaming(
        &self,
        session_id: &str,
        command: &str,
        timeout: Option<Duration>,
    ) -> Result<
        (
            super::session::OutputReceiver,
            tokio::task::JoinHandle<Result<super::session::CommandOutput, ShellSessionError>>,
        ),
        ShellSessionError,
    > {
        let session = self.get_session(session_id).await.ok_or_else(|| {
            ShellSessionError::SessionDead(format!("Session {} not found", session_id))
        })?;

        let timeout = timeout.unwrap_or(self.config.command_timeout);

        // Start streaming execution
        let (rx, handle) = {
            let session_guard = session.read().await;
            session_guard
                .execute_streaming(command, Some(timeout))
                .await?
        };

        Ok((rx, handle))
    }

    /// Close and remove a session
    pub async fn close_session(&self, session_id: &str) -> Result<(), ShellSessionError> {
        // Remove from maps
        let session = {
            let mut sessions = self.sessions.write().await;
            let mut session_info = self.session_info.write().await;

            session_info.remove(session_id);
            sessions.remove(session_id)
        };

        // Close the session if it existed
        if let Some(session) = session {
            let mut session_guard = session.write().await;
            session_guard.close().await?;
            info!(session_id = %session_id, "Closed shell session");
            Ok(())
        } else {
            Err(ShellSessionError::SessionDead(format!(
                "Session {} not found",
                session_id
            )))
        }
    }

    /// List all active sessions
    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        let session_info = self.session_info.read().await;
        session_info.values().cloned().collect()
    }

    /// Close all sessions
    pub async fn close_all_sessions(&self) {
        let session_ids: Vec<String> = {
            let sessions = self.sessions.read().await;
            sessions.keys().cloned().collect()
        };

        for session_id in session_ids {
            if let Err(e) = self.close_session(&session_id).await {
                warn!(session_id = %session_id, error = %e, "Failed to close session during cleanup");
            }
        }

        info!("Closed all shell sessions");
    }

    /// Clean up timed-out sessions
    ///
    /// Should be called periodically to remove stale sessions.
    pub async fn cleanup_timed_out_sessions(&self) {
        if self.config.session_timeout.is_zero() {
            return; // No timeout configured
        }

        let now = std::time::Instant::now();
        let timeout = self.config.session_timeout;

        let timed_out: Vec<String> = {
            let session_info = self.session_info.read().await;
            session_info
                .iter()
                .filter(|(_, info)| now.duration_since(info.last_used) > timeout)
                .map(|(id, _)| id.clone())
                .collect()
        };

        for session_id in timed_out {
            debug!(session_id = %session_id, "Closing timed-out session");
            if let Err(e) = self.close_session(&session_id).await {
                warn!(session_id = %session_id, error = %e, "Failed to close timed-out session");
            }
        }
    }

    /// Generate a unique session ID
    pub fn generate_session_id(prefix: &str) -> String {
        format!("{}-{}", prefix, generate_simple_id(8))
    }
}

impl Drop for ShellSessionManager {
    fn drop(&mut self) {
        // Note: We can't do async cleanup in Drop, but sessions will be cleaned up
        // when their Arc references are dropped. For proper cleanup, call close_all_sessions()
        // before dropping the manager.
        debug!("ShellSessionManager dropped");
    }
}

/// Wrapper to override session ID for default local sessions
struct DefaultIdLocalSession {
    inner: super::LocalShellSession,
    id: String,
}

#[async_trait::async_trait]
impl ShellSession for DefaultIdLocalSession {
    async fn execute(
        &self,
        command: &str,
        timeout: Option<Duration>,
    ) -> Result<super::session::CommandOutput, ShellSessionError> {
        self.inner.execute(command, timeout).await
    }

    async fn is_alive(&self) -> bool {
        self.inner.is_alive().await
    }

    fn session_id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> String {
        format!("Default local shell ({})", self.inner.description())
    }

    async fn close(&mut self) -> Result<(), ShellSessionError> {
        self.inner.close().await
    }

    async fn execute_streaming(
        &self,
        command: &str,
        timeout: Option<Duration>,
    ) -> Result<
        (
            super::session::OutputReceiver,
            tokio::task::JoinHandle<Result<super::session::CommandOutput, ShellSessionError>>,
        ),
        ShellSessionError,
    > {
        self.inner.execute_streaming(command, timeout).await
    }
}

/// Wrapper to override session ID for default remote sessions
struct DefaultIdRemoteSession {
    inner: super::RemoteShellSession,
    id: String,
}

#[async_trait::async_trait]
impl ShellSession for DefaultIdRemoteSession {
    async fn execute(
        &self,
        command: &str,
        timeout: Option<Duration>,
    ) -> Result<super::session::CommandOutput, ShellSessionError> {
        self.inner.execute(command, timeout).await
    }

    async fn is_alive(&self) -> bool {
        self.inner.is_alive().await
    }

    fn session_id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> String {
        format!("Default remote shell ({})", self.inner.description())
    }

    async fn close(&mut self) -> Result<(), ShellSessionError> {
        self.inner.close().await
    }

    async fn execute_streaming(
        &self,
        command: &str,
        timeout: Option<Duration>,
    ) -> Result<
        (
            super::session::OutputReceiver,
            tokio::task::JoinHandle<Result<super::session::CommandOutput, ShellSessionError>>,
        ),
        ShellSessionError,
    > {
        self.inner.execute_streaming(command, timeout).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ShellSessionConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_sessions, 10);
        assert_eq!(config.session_timeout, Duration::from_secs(3600));
        assert_eq!(config.command_timeout, Duration::from_secs(300));
    }

    #[test]
    fn test_generate_session_id() {
        let id1 = ShellSessionManager::generate_session_id("local");
        let id2 = ShellSessionManager::generate_session_id("local");

        assert!(id1.starts_with("local-"));
        assert!(id2.starts_with("local-"));
        assert_ne!(id1, id2); // Should be unique
    }

    #[tokio::test]
    async fn test_manager_creation() {
        let manager = ShellSessionManager::with_defaults();
        assert!(manager.is_enabled());
        assert_eq!(manager.session_count().await, 0);
    }

    #[tokio::test]
    async fn test_default_local_session_creation() {
        let manager = ShellSessionManager::with_defaults();

        // Create default local session
        let session_id = manager.get_or_create_default_local_session().await;
        assert!(
            session_id.is_ok(),
            "Should create default local session: {:?}",
            session_id.err()
        );

        let session_id = session_id.unwrap();
        assert_eq!(
            session_id, "default-local",
            "Should use default local session ID"
        );
        assert_eq!(manager.session_count().await, 1);

        // Getting again should return same session
        let session_id2 = manager.get_or_create_default_local_session().await;
        assert!(session_id2.is_ok());
        assert_eq!(session_id2.unwrap(), "default-local");
        assert_eq!(
            manager.session_count().await,
            1,
            "Should not create duplicate session"
        );
    }

    #[tokio::test]
    async fn test_execute_in_session() {
        let manager = ShellSessionManager::with_defaults();

        let session_id = manager
            .get_or_create_default_local_session()
            .await
            .expect("Should create session");

        // Execute a command
        let result = manager
            .execute_in_session(&session_id, "echo test_output", None)
            .await;
        assert!(result.is_ok(), "Command should succeed: {:?}", result.err());

        let output = result.unwrap();
        assert!(
            output.output.contains("test_output"),
            "Output should contain test_output, got: '{}'",
            output.output
        );
    }

    #[tokio::test]
    async fn test_session_state_persistence_via_manager() {
        let manager = ShellSessionManager::with_defaults();

        let session_id = manager
            .get_or_create_default_local_session()
            .await
            .expect("Should create session");

        // Set environment variable
        let set_result = manager
            .execute_in_session(&session_id, "export MANAGER_TEST_VAR=manager_value", None)
            .await;
        assert!(set_result.is_ok(), "Setting env var should succeed");

        // Read it back
        let get_result = manager
            .execute_in_session(&session_id, "echo $MANAGER_TEST_VAR", None)
            .await;
        assert!(get_result.is_ok(), "Reading env var should succeed");

        let output = get_result.unwrap();
        assert!(
            output.output.contains("manager_value"),
            "Environment variable should persist through manager, got: '{}'",
            output.output
        );
    }

    #[tokio::test]
    async fn test_close_session() {
        let manager = ShellSessionManager::with_defaults();

        let session_id = manager
            .get_or_create_default_local_session()
            .await
            .expect("Should create session");

        assert_eq!(manager.session_count().await, 1);

        // Close the session
        let close_result = manager.close_session(&session_id).await;
        assert!(close_result.is_ok(), "Close should succeed");
        assert_eq!(manager.session_count().await, 0);

        // Executing on closed session should fail
        let result = manager
            .execute_in_session(&session_id, "echo test", None)
            .await;
        assert!(result.is_err(), "Should fail on closed session");
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let manager = ShellSessionManager::with_defaults();

        // Initially empty
        let sessions = manager.list_sessions().await;
        assert!(sessions.is_empty());

        // Create a session
        let _ = manager
            .get_or_create_default_local_session()
            .await
            .expect("Should create session");

        let sessions = manager.list_sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "default-local");
        assert!(!sessions[0].is_remote);
    }

    #[tokio::test]
    async fn test_max_sessions_limit() {
        let config = ShellSessionConfig {
            max_sessions: 2,
            ..Default::default()
        };
        let manager = ShellSessionManager::new(config);

        // Create first session
        let _ = manager
            .get_or_create_default_local_session()
            .await
            .expect("Should create first session");

        // Create second session by registering directly
        let session2 = super::super::LocalShellSession::new(None, None, None, None)
            .expect("Should create local session");
        let result = manager.register_session(Box::new(session2), false).await;
        assert!(result.is_ok(), "Should create second session");

        // Third session should fail
        let session3 = super::super::LocalShellSession::new(None, None, None, None)
            .expect("Should create local session");
        let result = manager.register_session(Box::new(session3), false).await;
        assert!(result.is_err(), "Should fail when max sessions reached");
    }

    #[tokio::test]
    async fn test_close_all_sessions() {
        let manager = ShellSessionManager::with_defaults();

        // Create default session
        let _ = manager
            .get_or_create_default_local_session()
            .await
            .expect("Should create session");

        // Create another session
        let session2 = super::super::LocalShellSession::new(None, None, None, None)
            .expect("Should create local session");
        let _ = manager
            .register_session(Box::new(session2), false)
            .await
            .expect("Should register session");

        assert_eq!(manager.session_count().await, 2);

        // Close all
        manager.close_all_sessions().await;
        assert_eq!(manager.session_count().await, 0);
    }
}
