use rand::Rng;
use rmcp::{
    Error as McpError, RoleServer, ServerHandler, model::*, schemars, service::RequestContext, tool,
};

use serde_json::json;
use stakpak_shared::local_store::LocalStore;
use std::fs;

use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::error;
use uuid::Uuid;

use crate::secret_manager::SecretManager;
use crate::tool_descriptions::*;
use stakpak_shared::models::integrations::openai::ToolCallResultProgress;

/// Local tools that work without API access
#[derive(Clone)]
pub struct LocalTools {
    secret_manager: SecretManager,
}

#[tool(tool_box)]
impl LocalTools {
    pub fn new(redact_secrets: bool) -> Self {
        Self {
            secret_manager: SecretManager::new(redact_secrets),
        }
    }

    #[tool(description = RUN_COMMAND_DESCRIPTION)]
    pub async fn run_command(
        &self,
        peer: rmcp::Peer<RoleServer>,
        #[tool(param)]
        #[schemars(description = COMMAND_PARAM_DESCRIPTION)]
        command: String,
        #[tool(param)]
        #[schemars(description = WORK_DIR_PARAM_DESCRIPTION)]
        work_dir: Option<String>,
    ) -> Result<CallToolResult, McpError> {
        const MAX_LINES: usize = 300;

        let command_clone = command.clone();

        // Restore secrets in the command before execution
        let actual_command = self.secret_manager.restore_secrets_in_string(&command);

        let mut child = Command::new("sh")
            .arg("-c")
            .arg(actual_command)
            .current_dir(work_dir.unwrap_or(".".to_string()))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                error!("Failed to run command: {}", e);
                McpError::internal_error(
                    "Failed to run command",
                    Some(json!({
                        "command": command_clone,
                        "error": e.to_string()
                    })),
                )
            })?;

        #[allow(clippy::unwrap_used)]
        let stdout = child.stdout.take().unwrap();
        #[allow(clippy::unwrap_used)]
        let stderr = child.stderr.take().unwrap();

        let mut stdout_reader = BufReader::new(stdout);
        let mut stderr_reader = BufReader::new(stderr);

        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();
        let mut result = String::new();
        let progress_id = Uuid::new_v4();

        // Read from both streams concurrently
        loop {
            tokio::select! {
                Ok(n) = stderr_reader.read_line(&mut stderr_buf) => {
                    if n == 0 {
                        break;
                    }
                    let line = stderr_buf.trim_end_matches('\n').to_string();
                    stderr_buf.clear();
                    result.push_str(&format!("{}\n", line));
                    // Send notification but continue processing
                    let _ = peer.notify_progress(ProgressNotificationParam {
                        progress_token: ProgressToken(NumberOrString::Number(0)),
                        progress: 50,
                        total: Some(100),
                        message: Some(serde_json::to_string(&ToolCallResultProgress {
                            id: progress_id,
                            message: line,
                        }).unwrap_or_default()),
                    }).await;
                }
                Ok(n) = stdout_reader.read_line(&mut stdout_buf) => {
                    if n == 0 {
                        break;
                    }
                    let line = stdout_buf.trim_end_matches('\n').to_string();
                    stdout_buf.clear();
                    result.push_str(&format!("{}\n", line));
                    // Send notification but continue processing
                    // skip if message is empty
                    if line.is_empty() {
                        continue;
                    }
                    let _ = peer.notify_progress(ProgressNotificationParam {
                        progress_token: ProgressToken(NumberOrString::Number(0)),
                        progress: 50,
                        total: Some(100),
                        message: Some(serde_json::to_string(&ToolCallResultProgress {
                            id: progress_id,
                            message: format!("{}\n", line),
                        }).unwrap_or_default()),
                    }).await;
                }
                else => break,
            }
        }

        // Wait for the process to complete
        let exit_code = child
            .wait()
            .await
            .map_err(|e| {
                error!("Failed to wait for command: {}", e);
                McpError::internal_error(
                    "Failed to wait for command",
                    Some(json!({
                        "command": command_clone,
                        "error": e.to_string()
                    })),
                )
            })?
            .code()
            .unwrap_or(-1);

        if exit_code != 0 {
            result.push_str(&format!("Command exited with code {}\n", exit_code));
        }

        let output_lines = result.lines().collect::<Vec<_>>();

        result = if output_lines.len() >= MAX_LINES {
            // Create a output file to store the full output
            let output_file = format!(
                "command.output.{:06x}.txt",
                rand::rng().random_range(0..=0xFFFFFF)
            );
            let output_file_path =
                LocalStore::write_session_data(&output_file, &result).map_err(|e| {
                    error!("Failed to write session data to {}: {}", output_file, e);
                    McpError::internal_error(
                        "Failed to write session data",
                        Some(json!({ "error": e.to_string() })),
                    )
                })?;

            format!(
                "Showing the last {} / {} output lines. Full output saved to {}\n...\n{}",
                MAX_LINES,
                output_lines.len(),
                output_file_path,
                output_lines
                    .into_iter()
                    .rev()
                    .take(MAX_LINES)
                    .rev()
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        } else {
            result
        };

        if result.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text("No output")]));
        }

        let redacted_output = self.secret_manager.redact_and_store_secrets(&result, None);

        Ok(CallToolResult::success(vec![Content::text(
            &redacted_output,
        )]))
    }

    #[tool(description = VIEW_DESCRIPTION)]
    pub fn view(
        &self,
        #[tool(param)]
        #[schemars(description = PATH_PARAM_DESCRIPTION)]
        path: String,
        #[tool(param)]
        #[schemars(description = VIEW_RANGE_PARAM_DESCRIPTION)]
        view_range: Option<[i32; 2]>,
    ) -> Result<CallToolResult, McpError> {
        const MAX_LINES: usize = 300;

        let path_obj = Path::new(&path);

        if !path_obj.exists() {
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_NOT_FOUND"),
                Content::text(format!("File or directory not found: {}", path)),
            ]));
        }

        if path_obj.is_dir() {
            // List directory contents
            match fs::read_dir(&path) {
                Ok(entries) => {
                    let mut result = format!("Directory listing for \"{}\":\n", path);
                    let mut items: Vec<_> = entries.collect();
                    items.sort_by(|a, b| match (a, b) {
                        (Ok(a_entry), Ok(b_entry)) => {
                            match (
                                a_entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false),
                                b_entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false),
                            ) {
                                (true, false) => std::cmp::Ordering::Less,
                                (false, true) => std::cmp::Ordering::Greater,
                                _ => a_entry.file_name().cmp(&b_entry.file_name()),
                            }
                        }
                        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                        (Err(_), Err(_)) => std::cmp::Ordering::Equal,
                    });

                    for (i, entry) in items.iter().enumerate() {
                        let is_last = i == items.len() - 1;
                        let prefix = if is_last { "└── " } else { "├── " };
                        match entry {
                            Ok(entry) => {
                                let suffix = match entry.file_type() {
                                    Ok(ft) if ft.is_dir() => "/",
                                    Ok(_) => "",
                                    Err(_) => "?",
                                };
                                result.push_str(&format!(
                                    "{}{}{}\n",
                                    prefix,
                                    entry.file_name().to_string_lossy(),
                                    suffix
                                ));
                            }
                            Err(e) => {
                                result.push_str(&format!("Error reading entry: {}\n", e));
                            }
                        }
                    }
                    Ok(CallToolResult::success(vec![Content::text(result)]))
                }
                Err(e) => Ok(CallToolResult::error(vec![
                    Content::text("READ_ERROR"),
                    Content::text(format!("Cannot read directory: {}", e)),
                ])),
            }
        } else {
            // Read file contents
            match fs::read_to_string(&path) {
                Ok(content) => {
                    let result = if let Some([start, end]) = view_range {
                        let lines: Vec<&str> = content.lines().collect();
                        let start_idx = if start <= 0 { 0 } else { (start - 1) as usize };
                        let end_idx = if end == -1 {
                            lines.len()
                        } else {
                            std::cmp::min(end as usize, lines.len())
                        };

                        if start_idx >= lines.len() {
                            return Ok(CallToolResult::error(vec![
                                Content::text("INVALID_RANGE"),
                                Content::text(format!(
                                    "Start line {} is beyond file length {}",
                                    start,
                                    lines.len()
                                )),
                            ]));
                        }

                        let selected_lines = &lines[start_idx..end_idx];
                        if selected_lines.len() <= MAX_LINES {
                            format!(
                                "File: {} (lines {}-{})\n{}",
                                path,
                                start_idx + 1,
                                end_idx,
                                selected_lines
                                    .iter()
                                    .enumerate()
                                    .map(|(i, line)| format!("{:3}: {}", start_idx + i + 1, line))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            )
                        } else {
                            // truncate the extra lines
                            let selected_lines =
                                selected_lines.iter().take(MAX_LINES).collect::<Vec<_>>();

                            format!(
                                "File: {} (showing lines {}-{}, only the first {} lines of your view range)\n{}\n...",
                                path,
                                start_idx + 1,
                                start_idx + 1 + MAX_LINES,
                                MAX_LINES,
                                selected_lines
                                    .iter()
                                    .enumerate()
                                    .map(|(i, line)| format!("{:4}: {}", start_idx + i + 1, line))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            )
                        }
                    } else {
                        let lines: Vec<&str> = content.lines().collect();
                        if lines.len() <= MAX_LINES {
                            format!(
                                "File: {} ({} lines)\n{}",
                                path,
                                lines.len(),
                                lines
                                    .iter()
                                    .enumerate()
                                    .map(|(i, line)| format!("{:3}: {}", i + 1, line))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            )
                        } else {
                            // truncate the extra lines
                            let selected_lines = lines.iter().take(MAX_LINES).collect::<Vec<_>>();
                            format!(
                                "File: {} (showing {} / {} lines)\n{}\n...",
                                path,
                                MAX_LINES,
                                lines.len(),
                                selected_lines
                                    .iter()
                                    .enumerate()
                                    .map(|(i, line)| format!("{:3}: {}", i + 1, line))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            )
                        }
                    };

                    let redacted_result = self
                        .secret_manager
                        .redact_and_store_secrets(&result, Some(&path));
                    Ok(CallToolResult::success(vec![Content::text(
                        &redacted_result,
                    )]))
                }
                Err(e) => Ok(CallToolResult::error(vec![
                    Content::text("READ_ERROR"),
                    Content::text(format!("Cannot read file: {}", e)),
                ])),
            }
        }
    }

    #[tool(description = STR_REPLACE_DESCRIPTION)]
    pub fn str_replace(
        &self,
        #[tool(param)]
        #[schemars(description = FILE_PATH_PARAM_DESCRIPTION)]
        path: String,
        #[tool(param)]
        #[schemars(description = OLD_STR_PARAM_DESCRIPTION)]
        old_str: String,
        #[tool(param)]
        #[schemars(description = NEW_STR_PARAM_DESCRIPTION)]
        new_str: String,
    ) -> Result<CallToolResult, McpError> {
        let path_obj = Path::new(&path);

        if !path_obj.exists() {
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_NOT_FOUND"),
                Content::text(format!("File not found: {}", path)),
            ]));
        }

        if path_obj.is_dir() {
            return Ok(CallToolResult::error(vec![
                Content::text("IS_DIRECTORY"),
                Content::text(format!("Cannot edit directory: {}", path)),
            ]));
        }

        // Restore secrets in the input strings
        let actual_old_str = self.secret_manager.restore_secrets_in_string(&old_str);
        let actual_new_str = self.secret_manager.restore_secrets_in_string(&new_str);

        match fs::read_to_string(&path) {
            Ok(content) => {
                let matches: Vec<_> = content.match_indices(&actual_old_str).collect();

                match matches.len() {
                    0 => Ok(CallToolResult::error(vec![
                        Content::text("NO_MATCH"),
                        Content::text(
                            "No match found for replacement text. Please check your text and try again.",
                        ),
                    ])),
                    1 => {
                        let new_content = content.replace(&actual_old_str, &actual_new_str);
                        match fs::write(&path, new_content) {
                            Ok(_) => Ok(CallToolResult::success(vec![Content::text(format!(
                                "Successfully replaced text in {}",
                                path
                            ))])),
                            Err(e) => Ok(CallToolResult::error(vec![
                                Content::text("WRITE_ERROR"),
                                Content::text(format!("Cannot write to file: {}", e)),
                            ])),
                        }
                    }
                    n => Ok(CallToolResult::error(vec![
                        Content::text("MULTIPLE_MATCHES"),
                        Content::text(format!(
                            "Found {} matches for replacement text. Please provide more context to make a unique match.",
                            n
                        )),
                    ])),
                }
            }
            Err(e) => Ok(CallToolResult::error(vec![
                Content::text("READ_ERROR"),
                Content::text(format!("Cannot read file: {}", e)),
            ])),
        }
    }

    #[tool(description = CREATE_DESCRIPTION)]
    pub fn create(
        &self,
        #[tool(param)]
        #[schemars(description = CREATE_PATH_PARAM_DESCRIPTION)]
        path: String,
        #[tool(param)]
        #[schemars(description = FILE_TEXT_PARAM_DESCRIPTION)]
        file_text: String,
    ) -> Result<CallToolResult, McpError> {
        let path_obj = Path::new(&path);

        if path_obj.exists() {
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_EXISTS"),
                Content::text(format!("File already exists: {}", path)),
            ]));
        }

        // Create parent directories if they don't exist
        if let Some(parent) = path_obj.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return Ok(CallToolResult::error(vec![
                        Content::text("CREATE_DIR_ERROR"),
                        Content::text(format!("Cannot create parent directories: {}", e)),
                    ]));
                }
            }
        }

        // Restore secrets in the file content before writing
        let actual_file_text = self.secret_manager.restore_secrets_in_string(&file_text);

        match fs::write(&path, actual_file_text) {
            Ok(_) => {
                let lines = fs::read_to_string(&path)
                    .map(|content| content.lines().count())
                    .unwrap_or(0);
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Successfully created file {} with {} lines",
                    path, lines
                ))]))
            }
            Err(e) => Ok(CallToolResult::error(vec![
                Content::text("WRITE_ERROR"),
                Content::text(format!("Cannot create file: {}", e)),
            ])),
        }
    }

    #[tool(description = INSERT_DESCRIPTION)]
    pub fn insert(
        &self,
        #[tool(param)]
        #[schemars(description = FILE_PATH_PARAM_DESCRIPTION)]
        path: String,
        #[tool(param)]
        #[schemars(description = INSERT_LINE_PARAM_DESCRIPTION)]
        insert_line: u32,
        #[tool(param)]
        #[schemars(description = INSERT_TEXT_PARAM_DESCRIPTION)]
        new_str: String,
    ) -> Result<CallToolResult, McpError> {
        let path_obj = Path::new(&path);

        if !path_obj.exists() {
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_NOT_FOUND"),
                Content::text(format!("File not found: {}", path)),
            ]));
        }

        if path_obj.is_dir() {
            return Ok(CallToolResult::error(vec![
                Content::text("IS_DIRECTORY"),
                Content::text(format!("Cannot edit directory: {}", path)),
            ]));
        }

        match fs::read_to_string(&path) {
            Ok(content) => {
                let mut lines: Vec<&str> = content.lines().collect();
                let insert_idx = if insert_line == 0 {
                    0
                } else {
                    (insert_line - 1) as usize
                };

                if insert_idx > lines.len() {
                    return Ok(CallToolResult::error(vec![
                        Content::text("INVALID_LINE"),
                        Content::text(format!(
                            "Line number {} is beyond file length {}",
                            insert_line,
                            lines.len()
                        )),
                    ]));
                }

                // Restore secrets in the text to insert
                let actual_new_str = self.secret_manager.restore_secrets_in_string(&new_str);

                // Split new_str by lines and insert each line
                let new_lines: Vec<&str> = actual_new_str.lines().collect();
                for (i, line) in new_lines.iter().enumerate() {
                    lines.insert(insert_idx + i, line);
                }

                let new_content = lines.join("\n");
                // Preserve original file ending (with or without final newline)
                let final_content = if content.ends_with('\n') && !new_content.ends_with('\n') {
                    format!("{}\n", new_content)
                } else {
                    new_content
                };

                match fs::write(&path, final_content) {
                    Ok(_) => Ok(CallToolResult::success(vec![Content::text(format!(
                        "Successfully inserted {} lines at line {} in {}",
                        new_lines.len(),
                        insert_line,
                        path
                    ))])),
                    Err(e) => Ok(CallToolResult::error(vec![
                        Content::text("WRITE_ERROR"),
                        Content::text(format!("Cannot write to file: {}", e)),
                    ])),
                }
            }
            Err(e) => Ok(CallToolResult::error(vec![
                Content::text("READ_ERROR"),
                Content::text(format!("Cannot read file: {}", e)),
            ])),
        }
    }
}

#[tool(tool_box)]
impl ServerHandler for LocalTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "This server provides local tools for file operations and command execution."
                    .to_string(),
            ),
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        Ok(self.get_info())
    }
}
