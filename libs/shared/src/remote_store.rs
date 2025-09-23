use crate::remote_connection::RemoteConnection;
use std::path::PathBuf;
use std::sync::Arc;

pub struct RemoteStore {}

impl RemoteStore {
    /// Get the remote session store path (relative to remote working directory)
    pub fn get_remote_session_store_path() -> PathBuf {
        PathBuf::from(".stakpak").join("session")
    }

    /// Get the absolute remote session store path by canonicalizing on the remote host
    pub async fn get_absolute_remote_session_store_path(
        conn: &Arc<RemoteConnection>,
    ) -> Result<String, String> {
        let relative_path = Self::get_remote_session_store_path();
        let relative_path_str = relative_path.to_string_lossy().to_string();

        if let Err(e) = conn
            .execute_command(&format!("mkdir -p '{}'", relative_path_str), None, None)
            .await
        {
            return Err(format!("Failed to create remote session directory: {}", e));
        }

        match conn.canonicalize(&relative_path_str).await {
            Ok(abs_path) => Ok(abs_path),
            Err(e) => Err(format!("Failed to canonicalize remote session path: {}", e)),
        }
    }

    /// Get the backup directory path relative to session store
    pub fn get_backup_dir_path() -> PathBuf {
        PathBuf::from("backups")
    }

    /// Get the full backup directory path for a given session ID
    pub fn get_backup_session_path(session_id: &str) -> PathBuf {
        Self::get_backup_dir_path().join(session_id)
    }

    /// Get the backup directory path as a string (for remote operations)
    pub fn get_backup_dir_string() -> String {
        Self::get_remote_session_store_path()
            .join(Self::get_backup_dir_path())
            .to_string_lossy()
            .to_string()
    }

    /// Get the full backup directory path as a string for a given session ID (for remote operations)
    pub fn get_backup_session_string(session_id: &str) -> String {
        Self::get_remote_session_store_path()
            .join(Self::get_backup_session_path(session_id))
            .to_string_lossy()
            .to_string()
    }

    /// Get the absolute backup session path on the remote host
    pub async fn get_absolute_backup_session_path(
        conn: &Arc<RemoteConnection>,
        session_id: &str,
    ) -> Result<String, String> {
        let relative_backup_path = Self::get_backup_session_string(session_id);

        if let Err(e) = conn
            .execute_command(&format!("mkdir -p '{}'", relative_backup_path), None, None)
            .await
        {
            return Err(format!("Failed to create remote backup directory: {}", e));
        }

        match conn.canonicalize(&relative_backup_path).await {
            Ok(abs_path) => Ok(abs_path),
            Err(e) => Err(format!("Failed to canonicalize remote backup path: {}", e)),
        }
    }
}
