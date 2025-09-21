use crate::local_store::LocalStore;
use crate::remote_connection::RemoteConnection;
use crate::remote_store::RemoteStore;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

/// Manages file backups to the local session store and remote backup locations
pub struct FileBackupManager;

impl FileBackupManager {
    /// Move a local path (file or directory) to backup location in the session store
    pub fn move_local_path_to_backup(path: &str) -> Result<String, String> {
        let path_obj = Path::new(path);

        if !path_obj.exists() {
            return Err(format!("Path does not exist: {}", path));
        }

        let backup_session_id = Uuid::new_v4().to_string();

        let backup_session_path = LocalStore::get_backup_session_path(&backup_session_id);
        let full_backup_dir = LocalStore::get_local_session_store_path().join(&backup_session_path);

        if let Err(e) = std::fs::create_dir_all(&full_backup_dir) {
            return Err(format!("Failed to create backup directory: {}", e));
        }

        let item_name = path_obj
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown_item");
        let backup_path = full_backup_dir.join(item_name);

        match std::fs::rename(path_obj, &backup_path) {
            Ok(()) => Ok(backup_path.to_string_lossy().to_string()),
            Err(e) => Err(format!(
                "Failed to move local path '{}' to backup: {}",
                path, e
            )),
        }
    }

    /// Move a remote path (file or directory) to backup location on the remote machine
    pub async fn move_remote_path_to_backup(
        conn: &Arc<RemoteConnection>,
        path: &str,
    ) -> Result<String, String> {
        let backup_session_id = Uuid::new_v4().to_string();

        let absolute_backup_dir =
            match RemoteStore::get_absolute_backup_session_path(conn, &backup_session_id).await {
                Ok(abs_path) => abs_path,
                Err(e) => return Err(e),
            };

        let item_name = Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown_item");
        let backup_path = format!("{}/{}", absolute_backup_dir, item_name);

        match conn.rename(path, &backup_path).await {
            Ok(()) => Ok(backup_path),
            Err(e) => Err(format!(
                "Failed to move remote path '{}' to backup: {}",
                path, e
            )),
        }
    }

    /// Format backup mapping into XML structure with location type
    pub fn format_backup_xml(
        backup_mapping: &std::collections::HashMap<String, String>,
        location: &str,
    ) -> String {
        let mut inner_content = String::new();

        for (original_path, backup_path) in backup_mapping {
            inner_content.push_str(&format!(
                "\n    <file\n        original_path=\"{}\"\n        backup_path=\"{}\"\n        location=\"{}\"\n    />",
                Self::escape_xml(original_path),
                Self::escape_xml(backup_path),
                location
            ));
        }

        format!("<file_backups>{}\n</file_backups>", inner_content)
    }

    /// Escape XML special characters
    fn escape_xml(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
}
