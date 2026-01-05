use crate::local_store::LocalStore;
use crate::models::password::Password;
use crate::secrets::{redact_password, redact_secrets, restore_secrets};
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::error::{SendError, TrySendError};
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::{error, warn};

const DEFAULT_CHANNEL_CAPACITY: usize = 100;

const DEFAULT_OPERATION_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, thiserror::Error)]
pub enum SecretManagerError {
    #[error("Secret manager actor channel closed")]
    ChannelClosed,

    #[error("Secret manager actor channel full")]
    ChannelFull,

    #[error("Secret manager actor dropped response channel")]
    ActorDropped,

    #[error("Secret manager operation timed out after {0} seconds")]
    Timeout(u64),

    #[error("I/O error: {0}")]
    IoError(String),
}

impl<T> From<SendError<T>> for SecretManagerError {
    fn from(_: SendError<T>) -> Self {
        Self::ChannelClosed
    }
}

impl<T> From<TrySendError<T>> for SecretManagerError {
    fn from(err: TrySendError<T>) -> Self {
        match err {
            TrySendError::Full(_) => Self::ChannelFull,
            TrySendError::Closed(_) => Self::ChannelClosed,
        }
    }
}

enum SecretMessage {
    RedactAndStore {
        content: String,
        path: Option<String>,
        resp: oneshot::Sender<String>,
    },
    RedactPassword {
        content: String,
        password: String,
        resp: oneshot::Sender<String>,
    },
    RestoreSecrets {
        input: String,
        resp: oneshot::Sender<String>,
    },
}

struct SecretManager {
    redaction_map: HashMap<String, String>,
    redact_secrets: bool,
    privacy_mode: bool,
    rx: mpsc::Receiver<SecretMessage>,
    shutdown_rx: broadcast::Receiver<()>,
    /// Lazy-loaded gitleaks configuration.
    /// Only loaded on first secret scan to save memory if scanning is never used.
    gitleaks_config: Option<crate::secrets::gitleaks::GitleaksConfig>,
}

impl SecretManager {
    fn new(
        redact_secrets: bool,
        privacy_mode: bool,
        rx: mpsc::Receiver<SecretMessage>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        let redaction_map = Self::load_session_redaction_map_sync();

        Self {
            redaction_map,
            redact_secrets,
            privacy_mode,
            rx,
            shutdown_rx,
            gitleaks_config: None, // Lazy-loaded on first scan
        }
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                msg = self.rx.recv() => {
                    match msg {
                        Some(m) => self.handle_message(m).await,
                        None => {
                            // Main channel closed, all senders dropped - exit
                            warn!("SecretManager actor shutting down: channel closed");
                            break;
                        }
                    }
                }
                result = self.shutdown_rx.recv() => {
                    match result {
                        Ok(_) => {
                            warn!("SecretManager actor shutting down: shutdown signal");
                            break;
                        }
                        Err(_) => {
                        }
                    }
                }
            }
        }
    }

    async fn handle_message(&mut self, msg: SecretMessage) {
        match msg {
            SecretMessage::RedactAndStore {
                content,
                path,
                resp,
            } => {
                let result = self
                    .redact_and_store_secrets_impl(&content, path.as_deref())
                    .await;
                let _ = resp.send(result);
            }
            SecretMessage::RedactPassword {
                content,
                password,
                resp,
            } => {
                let result = self
                    .redact_and_store_password_impl(&content, &password)
                    .await;
                let _ = resp.send(result);
            }
            SecretMessage::RestoreSecrets { input, resp } => {
                let result = restore_secrets(&input, &self.redaction_map);
                let _ = resp.send(result);
            }
        }
    }

    fn load_session_redaction_map_sync() -> HashMap<String, String> {
        match LocalStore::read_session_data("secrets.json") {
            Ok(content) => {
                if content.trim().is_empty() {
                    return HashMap::new();
                }

                match serde_json::from_str::<HashMap<String, String>>(&content) {
                    Ok(map) => map,
                    Err(e) => {
                        error!("Failed to parse session redaction map JSON: {}", e);
                        HashMap::new()
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read session redaction map file: {}", e);
                HashMap::new()
            }
        }
    }

    async fn save_session_redaction_map(&self) {
        let json_content = match serde_json::to_string_pretty(&self.redaction_map) {
            Ok(content) => content,
            Err(e) => {
                error!("Failed to serialize session redaction map to JSON: {}", e);
                return;
            }
        };

        // Use spawn_blocking to avoid blocking the async runtime during file I/O
        let result = tokio::task::spawn_blocking(move || {
            LocalStore::write_session_data("secrets.json", &json_content)
        })
        .await;

        match result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                error!("Failed to save session redaction map: {}", e);
            }
            Err(e) => {
                error!("spawn_blocking panicked while saving redaction map: {}", e);
            }
        }
    }

    async fn redact_and_store_secrets_impl(&mut self, content: &str, path: Option<&str>) -> String {
        if !self.redact_secrets {
            return content.to_string();
        }

        // Lazy-load gitleaks config on first use (saves memory if never needed)
        if self.gitleaks_config.is_none() {
            self.gitleaks_config = Some(crate::secrets::gitleaks::create_gitleaks_config(
                self.privacy_mode,
            ));
        }

        let config = self.gitleaks_config.as_ref().unwrap();
        let redaction_result = redact_secrets(content, path, &self.redaction_map, config);

        let old_len = self.redaction_map.len();
        self.redaction_map.extend(redaction_result.redaction_map);
        if self.redaction_map.len() > old_len {
            self.save_session_redaction_map().await;
        }

        redaction_result.redacted_string
    }

    async fn redact_and_store_password_impl(&mut self, password: Password) -> String {
        if !self.redact_secrets {
            return password.expose_secret().to_string();
        }

        let redaction_result = redact_password(content, password, &self.redaction_map);

        let old_len = self.redaction_map.len();
        self.redaction_map.extend(redaction_result.redaction_map);
        if self.redaction_map.len() > old_len {
            self.save_session_redaction_map().await;
        }

        redaction_result.redacted_string
    }
}

#[derive(Clone, Debug)]
pub struct SecretManagerHandle {
    tx: mpsc::Sender<SecretMessage>,
    shutdown_tx: broadcast::Sender<()>,
    redact_secrets: bool,
}

impl SecretManagerHandle {
    pub fn shutdown(&self) -> Result<(), broadcast::error::SendError<()>> {
        self.shutdown_tx.send(()).map(|_| ())
    }

    async fn await_response(
        resp_rx: oneshot::Receiver<String>,
    ) -> Result<String, SecretManagerError> {
        let timeout_duration = Duration::from_secs(DEFAULT_OPERATION_TIMEOUT_SECS);
        match tokio::time::timeout(timeout_duration, resp_rx).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(_)) => Err(SecretManagerError::ActorDropped),
            Err(_) => Err(SecretManagerError::Timeout(DEFAULT_OPERATION_TIMEOUT_SECS)),
        }
    }

    pub async fn redact_and_store_secrets(
        &self,
        content: &str,
        path: Option<&str>,
    ) -> Result<String, SecretManagerError> {
        // Fast-path optimization: skip message passing if redaction is disabled
        if !self.redact_secrets {
            return Ok(content.to_string());
        }

        let (resp_tx, resp_rx) = oneshot::channel();
        let msg = SecretMessage::RedactAndStore {
            content: content.to_string(),
            path: path.map(|s| s.to_string()),
            resp: resp_tx,
        };

        self.tx.send(msg).await?;

        Self::await_response(resp_rx).await
    }

    /// returns immediately if the channel is full
    pub async fn try_redact_and_store_secrets(
        &self,
        content: &str,
        path: Option<&str>,
    ) -> Result<String, SecretManagerError> {
        // Fast-path optimization: skip message passing if redaction is disabled
        if !self.redact_secrets {
            return Ok(content.to_string());
        }

        let (resp_tx, resp_rx) = oneshot::channel();
        let msg = SecretMessage::RedactAndStore {
            content: content.to_string(),
            path: path.map(|s| s.to_string()),
            resp: resp_tx,
        };

        self.tx.try_send(msg)?;

        Self::await_response(resp_rx).await
    }

    pub async fn redact_and_store_password(
        &self,
        content: &str,
        password: &str,
    ) -> Result<String, SecretManagerError> {
        // Fast-path optimization: skip message passing if redaction is disabled
        if !self.redact_secrets {
            return Ok(content.to_string());
        }

        let (resp_tx, resp_rx) = oneshot::channel();
        let msg = SecretMessage::RedactPassword {
            content: content.to_string(),
            password: password.to_string(),
            resp: resp_tx,
        };

        self.tx.send(msg).await?;

        Self::await_response(resp_rx).await
    }

    pub async fn restore_secrets_in_string(
        &self,
        input: &str,
    ) -> Result<String, SecretManagerError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        let msg = SecretMessage::RestoreSecrets {
            input: input.to_string(),
            resp: resp_tx,
        };

        self.tx.send(msg).await?;

        Self::await_response(resp_rx).await
    }
}

pub fn launch_secret_manager(
    redact_secrets: bool,
    privacy_mode: bool,
    channel_capacity: Option<usize>,
) -> Arc<SecretManagerHandle> {
    let capacity = channel_capacity.unwrap_or(DEFAULT_CHANNEL_CAPACITY);
    let (tx, rx) = mpsc::channel(capacity);
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

    let manager = SecretManager::new(redact_secrets, privacy_mode, rx, shutdown_rx);

    tokio::spawn(async move {
        manager.run().await;
    });

    Arc::new(SecretManagerHandle {
        tx,
        shutdown_tx,
        redact_secrets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_concurrent_secret_operations() {
        // Launch secret manager with redaction disabled for simpler testing
        let handle = launch_secret_manager(false, false, None);

        let mut handles = Vec::new();
        for i in 0..50 {
            let h = Arc::clone(&handle);
            let task = tokio::spawn(async move {
                let content = format!("API_KEY=secret_value_{}", i);
                let result = h.redact_and_store_secrets(&content, None).await;
                assert!(result.is_ok());

                let restore_result = h.restore_secrets_in_string(&content).await;
                assert!(restore_result.is_ok());
            });
            handles.push(task);
        }

        for handle in handles {
            handle.await.expect("Task panicked");
        }
    }

    #[tokio::test]
    async fn test_secret_manager_basic_operations() {
        let handle = launch_secret_manager(true, false, None);

        let content = "export API_KEY=test_secret_12345";
        let result = handle.redact_and_store_secrets(content, None).await;
        assert!(result.is_ok());

        let restore_result = handle.restore_secrets_in_string(content).await;
        assert!(restore_result.is_ok());
    }

    #[tokio::test]
    async fn test_secret_manager_graceful_shutdown() {
        // Use `redact_secrets=true` to test actual channel closure, not the fast-path optimization
        let handle = launch_secret_manager(true, false, None);

        let result = handle.redact_and_store_secrets("test content", None).await;
        assert!(result.is_ok());

        let _ = handle.shutdown();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let result = handle.redact_and_store_secrets("more content", None).await;
        assert!(matches!(result, Err(SecretManagerError::ChannelClosed)));
    }
}
