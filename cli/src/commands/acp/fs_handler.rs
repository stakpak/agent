use agent_client_protocol::{self as acp, Client as AcpClient};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// Filesystem operation requests for ACP native protocol
pub enum FsOperation {
    ReadTextFile {
        session_id: acp::SessionId,
        path: std::path::PathBuf,
        line: Option<u32>,
        limit: Option<u32>,
        response_tx: oneshot::Sender<Result<String, String>>,
    },
    WriteTextFile {
        session_id: acp::SessionId,
        path: std::path::PathBuf,
        content: String,
        response_tx: oneshot::Sender<Result<(), String>>,
    },
}

/// Helper function to resolve a path to an absolute path
fn resolve_absolute_path(path: &str) -> std::path::PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        // Get current working directory and join with the relative path
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(path)
    }
}

/// Spawn a background task to handle filesystem operations via ACP connection
pub fn spawn_fs_handler(
    conn: Arc<acp::AgentSideConnection>,
    mut fs_operation_rx: mpsc::UnboundedReceiver<FsOperation>,
) {
    tokio::task::spawn_local(async move {
        while let Some(operation) = fs_operation_rx.recv().await {
            match operation {
                FsOperation::ReadTextFile {
                    session_id,
                    path,
                    line,
                    limit,
                    response_tx,
                } => {
                    log::info!("Processing ACP read_text_file: {:?}", path);
                    let request = acp::ReadTextFileRequest::new(session_id, path)
                        .line(line)
                        .limit(limit);
                    let result = match conn.read_text_file(request).await {
                        Ok(response) => Ok(response.content),
                        Err(e) => Err(format!("ACP read_text_file failed: {}", e)),
                    };
                    let _ = response_tx.send(result);
                }
                FsOperation::WriteTextFile {
                    session_id,
                    path,
                    content,
                    response_tx,
                } => {
                    log::info!("Processing ACP write_text_file: {:?}", path);
                    let request = acp::WriteTextFileRequest::new(session_id, path, content);
                    let result = match conn.write_text_file(request).await {
                        Ok(_) => Ok(()),
                        Err(e) => Err(format!("ACP write_text_file failed: {}", e)),
                    };
                    let _ = response_tx.send(result);
                }
            }
        }
    });
}

/// Execute filesystem tool using native ACP protocol via channel
pub async fn execute_acp_fs_tool(
    fs_tx: &mpsc::UnboundedSender<FsOperation>,
    tool_call: &stakpak_shared::models::integrations::openai::ToolCall,
    session_id: &acp::SessionId,
) -> Result<Option<rmcp::model::CallToolResult>, String> {
    let args: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
        .map_err(|e| format!("Failed to parse tool arguments: {}", e))?;

    use super::tool_names;
    let stripped_name = super::utils::strip_tool_name(&tool_call.function.name);
    match stripped_name {
        tool_names::VIEW => {
            let path = args
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| "Missing 'path' parameter".to_string())?;

            let line = args
                .get("view_range")
                .and_then(|r| r.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);

            let limit = args
                .get("view_range")
                .and_then(|r| r.as_array())
                .and_then(|arr| arr.get(1))
                .and_then(|v| v.as_i64())
                .and_then(|v| if v == -1 { None } else { Some(v as u32) });

            log::info!(
                "Reading file via ACP: {} (line: {:?}, limit: {:?})",
                path,
                line,
                limit
            );

            let (response_tx, response_rx) = oneshot::channel();
            let absolute_path = resolve_absolute_path(path);
            log::info!(
                "Resolved path '{}' to absolute path: {:?}",
                path,
                absolute_path
            );
            fs_tx
                .send(FsOperation::ReadTextFile {
                    session_id: session_id.clone(),
                    path: absolute_path,
                    line,
                    limit,
                    response_tx,
                })
                .map_err(|_| "Failed to send filesystem operation".to_string())?;

            let content = response_rx
                .await
                .map_err(|_| "Filesystem operation cancelled".to_string())??;

            Ok(Some(rmcp::model::CallToolResult {
                content: vec![rmcp::model::Content::text(content)],
                is_error: Some(false),
                meta: None,
                structured_content: None,
            }))
        }
        tool_names::CREATE => {
            let path = args
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| "Missing 'path' parameter".to_string())?;

            let content = args
                .get("file_text")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "Missing 'file_text' parameter".to_string())?;

            log::info!("Creating file via ACP: {}", path);

            let (response_tx, response_rx) = oneshot::channel();
            let absolute_path = resolve_absolute_path(path);
            log::info!(
                "Resolved path '{}' to absolute path: {:?}",
                path,
                absolute_path
            );
            fs_tx
                .send(FsOperation::WriteTextFile {
                    session_id: session_id.clone(),
                    path: absolute_path,
                    content: content.to_string(),
                    response_tx,
                })
                .map_err(|_| "Failed to send filesystem operation".to_string())?;

            response_rx
                .await
                .map_err(|_| "Filesystem operation cancelled".to_string())??;

            Ok(Some(rmcp::model::CallToolResult {
                content: vec![rmcp::model::Content::text(format!(
                    "Successfully created file: {}",
                    path
                ))],
                is_error: Some(false),
                meta: None,
                structured_content: None,
            }))
        }
        tool_names::STR_REPLACE => {
            let path = args
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| "Missing 'path' parameter".to_string())?;

            let old_str = args
                .get("old_str")
                .and_then(|s| s.as_str())
                .ok_or_else(|| "Missing 'old_str' parameter".to_string())?;

            let new_str = args
                .get("new_str")
                .and_then(|s| s.as_str())
                .ok_or_else(|| "Missing 'new_str' parameter".to_string())?;

            let replace_all = args
                .get("replace_all")
                .and_then(|b| b.as_bool())
                .unwrap_or(false);

            log::info!("Replacing text in file via ACP: {}", path);

            // Read current content
            let (read_tx, read_rx) = oneshot::channel();
            let absolute_path = resolve_absolute_path(path);
            log::info!(
                "Resolved path '{}' to absolute path: {:?}",
                path,
                absolute_path
            );
            fs_tx
                .send(FsOperation::ReadTextFile {
                    session_id: session_id.clone(),
                    path: absolute_path.clone(),
                    line: None,
                    limit: None,
                    response_tx: read_tx,
                })
                .map_err(|_| "Failed to send filesystem operation".to_string())?;

            let content = read_rx
                .await
                .map_err(|_| "Filesystem operation cancelled".to_string())??;

            // Check if old_str exists
            if !content.contains(old_str) {
                return Err(format!(
                    "STRING_NOT_FOUND: '{}' not found in file '{}'",
                    old_str, path
                ));
            }

            // Perform replacement
            let new_content = if replace_all {
                content.replace(old_str, new_str)
            } else {
                content.replacen(old_str, new_str, 1)
            };

            // Write back
            let (write_tx, write_rx) = oneshot::channel();
            fs_tx
                .send(FsOperation::WriteTextFile {
                    session_id: session_id.clone(),
                    path: absolute_path,
                    content: new_content,
                    response_tx: write_tx,
                })
                .map_err(|_| "Failed to send filesystem operation".to_string())?;

            write_rx
                .await
                .map_err(|_| "Filesystem operation cancelled".to_string())??;

            let replacement_count = if replace_all {
                content.matches(old_str).count()
            } else {
                1
            };

            Ok(Some(rmcp::model::CallToolResult {
                content: vec![rmcp::model::Content::text(format!(
                    "Successfully replaced {} occurrence(s) in file: {}",
                    replacement_count, path
                ))],
                is_error: Some(false),
                meta: None,
                structured_content: None,
            }))
        }
        _ => Err(format!(
            "Unknown filesystem tool: {}",
            tool_call.function.name
        )),
    }
}
