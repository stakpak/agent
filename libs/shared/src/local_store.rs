use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct LocalStore {}

impl LocalStore {
    pub fn get_local_session_store_path() -> PathBuf {
        Path::new(".stakpak").join("session")
    }

    pub fn write_session_data(path: &str, data: &str) -> Result<String, String> {
        let session_dir = Self::get_local_session_store_path();
        if !session_dir.exists() {
            std::fs::create_dir_all(&session_dir)
                .map_err(|e| format!("Failed to create session directory: {}", e))?;
        }

        let path = Self::get_local_session_store_path().join(path);
        std::fs::write(&path, data)
            .map_err(|e| format!("Failed to write session data to {}: {}", path.display(), e))?;
        Ok(path.to_string_lossy().to_string())
    }

    pub fn read_session_data(path: &str) -> Result<String, String> {
        let path = Self::get_local_session_store_path().join(path);
        fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read session data from {}: {}", path.display(), e))
    }
}
