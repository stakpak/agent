use serde_json::json;
use std::fs;
use std::path::Path;
use uuid::Uuid;

use crate::utils::gitignore;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SessionInfo {
    pub session_id: Uuid,
    pub checkpoint_id: Option<String>,
}

pub fn write_session_info(session_info: &SessionInfo) -> Result<(), String> {
    // Ensure .stakpak/session directory exists
    let session_dir = Path::new(".stakpak/session");
    fs::create_dir_all(session_dir)
        .map_err(|e| format!("Failed to create session directory: {}", e))?;

    // Create the session file path
    let session_file = session_dir.join(format!("{}.json", "session_details"));

    // Create JSON content
    let json_content = json!({
        "session_id": session_info.session_id,
        "checkpoint_id": session_info.checkpoint_id,
    });

    // Write to file
    let json_string = serde_json::to_string_pretty(&json_content)
        .map_err(|e| format!("Failed to serialize session info: {}", e))?;

    fs::write(&session_file, json_string)
        .map_err(|e| format!("Failed to write session file: {}", e))?;

    Ok(())
}

pub fn write_session_start_info(
    session_id: &Option<Uuid>,
    checkpoint_id: &Option<Uuid>,
) -> Result<(), String> {
    if !gitignore::is_git_repo() {
        return Ok(());
    }
    let session_info = SessionInfo {
        session_id: session_id.unwrap_or_default(), // This will be a placeholder
        checkpoint_id: checkpoint_id.map(|id| id.to_string()),
    };

    // Write the initial session info
    write_session_info(&session_info)?;

    Ok(())
}

// read and return checkpoint id
pub fn read_session_info() -> Result<Option<SessionInfo>, String> {
    let session_dir = Path::new(".stakpak/session");
    let session_file = session_dir.join("session_details.json");

    if !session_file.exists() {
        return Ok(None);
    }

    let session_info = fs::read_to_string(session_file)
        .map_err(|e| format!("Failed to read session info: {}", e))?;
    let session_info: SessionInfo = serde_json::from_str(&session_info)
        .map_err(|e| format!("Failed to deserialize session info: {}", e))?;
    Ok(Some(session_info))
}
