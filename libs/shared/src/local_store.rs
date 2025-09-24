use std::{fs, path::PathBuf};

pub struct LocalStore {}

impl LocalStore {
    pub fn get_local_session_store_path() -> PathBuf {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".stakpak")
            .join("session")
    }

    pub fn get_backup_session_path(session_id: &str) -> PathBuf {
        PathBuf::from("backups").join(session_id)
    }

    pub fn write_session_data(path: &str, data: &str) -> Result<String, String> {
        let session_dir = Self::get_local_session_store_path();
        if !session_dir.exists() {
            std::fs::create_dir_all(&session_dir)
                .map_err(|e| format!("Failed to create session directory: {}", e))?;
        }

        let full_path = Self::get_local_session_store_path().join(path);

        // Create parent directories if they don't exist
        if let Some(parent_dir) = full_path.parent()
            && !parent_dir.exists()
        {
            std::fs::create_dir_all(parent_dir).map_err(|e| {
                format!(
                    "Failed to create parent directory {}: {}",
                    parent_dir.display(),
                    e
                )
            })?;
        }

        std::fs::write(&full_path, data).map_err(|e| {
            format!(
                "Failed to write session data to {}: {}",
                full_path.display(),
                e
            )
        })?;
        Ok(full_path.to_string_lossy().to_string())
    }

    pub fn read_session_data(path: &str) -> Result<String, String> {
        let path = Self::get_local_session_store_path().join(path);
        fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read session data from {}: {}", path.display(), e))
    }
}
