use crate::tool_container::ToolContainer;
use rand::Rng;
use rmcp::service::RequestContext;
use rmcp::{Error as McpError, handler::server::tool::Parameters, model::*, schemars, tool};
use rmcp::{RoleServer, tool_router};
use serde::Deserialize;

use serde_json::json;
use stakpak_shared::local_store::LocalStore;
use stakpak_shared::models::integrations::openai::ToolCallResultProgress;
use std::fs;

use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::error;
use uuid::Uuid;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RunCommandRequest {
    #[schemars(description = "The shell command to execute")]
    pub command: String,
    #[schemars(description = "Optional working directory for command execution")]
    pub work_dir: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ViewRequest {
    #[schemars(description = "The path to the file or directory to view")]
    pub path: String,
    #[schemars(
        description = "Optional line range to view [start_line, end_line]. Line numbers are 1-indexed. Use -1 for end_line to read to end of file."
    )]
    pub view_range: Option<[i32; 2]>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StrReplaceRequest {
    #[schemars(description = "The path to the file to modify")]
    pub path: String,
    #[schemars(
        description = "The exact text to replace (must match exactly, including whitespace and indentation)"
    )]
    pub old_str: String,
    #[schemars(
        description = "The new text to insert in place of the old text. When replacing code, ensure the new text maintains proper syntax, indentation, and follows the codebase style."
    )]
    pub new_str: String,
    #[schemars(
        description = "Whether to replace all occurrences of the old text in the file (default: false)"
    )]
    pub replace_all: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateRequest {
    #[schemars(description = "The path where the new file should be created")]
    pub path: String,
    #[schemars(
        description = "The content to write to the new file, when creating code, ensure the new text has proper syntax, indentation, and follows the codebase style."
    )]
    pub file_text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GeneratePasswordRequest {
    #[schemars(description = "The length of the password to generate")]
    pub length: Option<usize>,
    #[schemars(description = "Whether to disallow symbols in the password (default: false)")]
    pub no_symbols: Option<bool>,
}

#[tool_router(router = tool_router_local, vis = "pub")]
impl ToolContainer {
    #[tool(
        description = "A system command execution tool that allows running shell commands with full system access. 

SECRET HANDLING: 
- Output containing secrets will be redacted and shown as placeholders like [REDACTED_SECRET:rule-id:hash]
- You can use these placeholders in subsequent commands - they will be automatically restored to actual values before execution
- Example: If you see 'export API_KEY=[REDACTED_SECRET:api-key:abc123]', you can use '[REDACTED_SECRET:api-key:abc123]' in later commands

If the command's output exceeds 300 lines the result will be truncated and the full output will be saved to a file in the current directory"
    )]
    pub async fn run_command(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(RunCommandRequest { command, work_dir }): Parameters<RunCommandRequest>,
    ) -> Result<CallToolResult, McpError> {
        const MAX_LINES: usize = 300;

        let command_clone = command.clone();

        // Restore secrets in the command before execution
        let actual_command = self
            .get_secret_manager()
            .restore_secrets_in_string(&command);

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
                    let _ = ctx.peer.notify_progress(ProgressNotificationParam {
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
                    let _ = ctx.peer.notify_progress(ProgressNotificationParam {
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

        let redacted_output = self
            .get_secret_manager()
            .redact_and_store_secrets(&result, None);

        Ok(CallToolResult::success(vec![Content::text(
            &redacted_output,
        )]))
    }

    #[tool(
        description = "View the contents of a file or list the contents of a directory. Can read entire files or specific line ranges.

SECRET HANDLING:
- File contents containing secrets will be redacted and shown as placeholders like [REDACTED_SECRET:rule-id:hash]
- These placeholders represent actual secret values that are safely stored for later use
- You can reference these placeholders when working with the file content

A maximum of 300 lines will be shown at a time, the rest will be truncated."
    )]
    pub fn view(
        &self,
        Parameters(ViewRequest { path, view_range }): Parameters<ViewRequest>,
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
                        .get_secret_manager()
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

    #[tool(
        description = "Replace a specific string in a file with new text. The old_str must match exactly including whitespace and indentation.

SECRET HANDLING:
- You can use secret placeholders like [REDACTED_SECRET:rule-id:hash] in both old_str and new_str parameters
- These placeholders will be automatically restored to actual secret values before performing the replacement
- This allows you to safely work with secret values without exposing them

When replacing code, ensure the new text maintains proper syntax, indentation, and follows the codebase style."
    )]
    pub fn str_replace(
        &self,
        Parameters(StrReplaceRequest {
            path,
            old_str,
            new_str,
            replace_all,
        }): Parameters<StrReplaceRequest>,
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
        let actual_old_str = self
            .get_secret_manager()
            .restore_secrets_in_string(&old_str);
        let actual_new_str = self
            .get_secret_manager()
            .restore_secrets_in_string(&new_str);

        match fs::read_to_string(&path) {
            Ok(content) => {
                let matches: Vec<_> = content.match_indices(&actual_old_str).collect();

                match (matches.len(), replace_all) {
                    (0, _) => Ok(CallToolResult::error(vec![
                        Content::text("NO_MATCH"),
                        Content::text(
                            "No match found for replacement text. Please check your text and try again.",
                        ),
                    ])),
                    (1, _) => {
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
                    (n, Some(true)) => {
                        let new_content = content.replace(&actual_old_str, &actual_new_str);
                        match fs::write(&path, new_content) {
                            Ok(_) => Ok(CallToolResult::success(vec![Content::text(format!(
                                "Successfully replaced {} occurrences of text in {}",
                                n, path
                            ))])),
                            Err(e) => Ok(CallToolResult::error(vec![
                                Content::text("WRITE_ERROR"),
                                Content::text(format!("Cannot write to file: {}", e)),
                            ])),
                        }
                    }
                    (n, _) => Ok(CallToolResult::error(vec![
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

    #[tool(
        description = "Create a new file with the specified content. Will fail if file already exists. When creating code, ensure the new text has proper syntax, indentation, and follows the codebase style. Parent directories will be created automatically if they don't exist."
    )]
    pub fn create(
        &self,
        Parameters(CreateRequest { path, file_text }): Parameters<CreateRequest>,
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
        let actual_file_text = self
            .get_secret_manager()
            .restore_secrets_in_string(&file_text);

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

    #[tool(
        description = "Generate a secure password with the specified constraints. The password will be generated using the following constraints:
- Length of the password (default: 15)
- No symbols (default: false)
"
    )]
    pub async fn generate_password(
        &self,
        Parameters(GeneratePasswordRequest { length, no_symbols }): Parameters<
            GeneratePasswordRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let length = length.unwrap_or(15);
        let no_symbols = no_symbols.unwrap_or(false);

        let mut config = npwg::PasswordGeneratorConfig::default();
        config.length = length;

        if no_symbols {
            config.excluded_chars = npwg::config::DEFINE
                .iter()
                .find(|(name, _)| *name == "symbol3")
                .map(|(_, chars)| chars.chars().collect())
                .unwrap_or_default();
        }

        let password = if let Ok(password) = npwg::generator::generate_password(&config).await {
            password
        } else {
            return Ok(CallToolResult::error(vec![
                Content::text("FAILED_TO_GENERATE_PASSWORD"),
                Content::text(format!("Failed to generate password")),
            ]));
        };

        let redacted_password = self
            .get_secret_manager()
            .redact_and_store_password(&password, &password);

        Ok(CallToolResult::success(vec![Content::text(
            &redacted_password,
        )]))
    }
}
