use rand::Rng;
use rmcp::{
    Error as McpError, RoleServer, ServerHandler, model::*, schemars, service::RequestContext, tool,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use stakpak_api::{Client, ClientConfig};
use stakpak_api::{GenerationResult, ToolsCallParams};
use stakpak_shared::local_store::LocalStore;
use stakpak_shared::secrets::{redact_secrets, restore_secrets};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{error, warn};
use uuid::Uuid;

use stakpak_shared::models::integrations::openai::ToolCallResultProgress;

#[derive(Clone)]
pub struct Tools {
    api_config: ClientConfig,
    redact_secrets: bool,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone, JsonSchema)]
pub enum Provisioner {
    #[serde(rename = "Terraform")]
    Terraform,
    #[serde(rename = "Kubernetes")]
    Kubernetes,
    #[serde(rename = "Dockerfile")]
    Dockerfile,
    #[serde(rename = "GithubActions")]
    GithubActions,
    #[serde(rename = "None")]
    None,
}

impl std::fmt::Display for Provisioner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provisioner::Terraform => write!(f, "Terraform"),
            Provisioner::Kubernetes => write!(f, "Kubernetes"),
            Provisioner::Dockerfile => write!(f, "Dockerfile"),
            Provisioner::GithubActions => write!(f, "GithubActions"),
            Provisioner::None => write!(f, "None"),
        }
    }
}

#[tool(tool_box)]
impl Tools {
    pub fn new(api_config: ClientConfig, redact_secrets: bool) -> Self {
        Self {
            api_config,
            redact_secrets,
        }
    }

    /// Load the redaction map from the session file
    fn load_session_redaction_map(&self) -> HashMap<String, String> {
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

    /// Save the redaction map to the session file
    fn save_session_redaction_map(&self, redaction_map: &HashMap<String, String>) {
        match serde_json::to_string_pretty(redaction_map) {
            Ok(json_content) => {
                if let Err(e) = LocalStore::write_session_data("secrets.json", &json_content) {
                    error!("Failed to save session redaction map: {}", e);
                }
            }
            Err(e) => {
                error!("Failed to serialize session redaction map to JSON: {}", e);
            }
        }
    }

    /// Add new redactions to the session map
    fn add_to_session_redaction_map(&self, new_redactions: &HashMap<String, String>) {
        if new_redactions.is_empty() {
            return;
        }

        let mut existing_map = self.load_session_redaction_map();
        existing_map.extend(new_redactions.clone());
        self.save_session_redaction_map(&existing_map);
    }

    /// Restore secrets in a string using the session redaction map
    fn restore_secrets_in_string(&self, input: &str) -> String {
        let redaction_map = self.load_session_redaction_map();
        if redaction_map.is_empty() {
            return input.to_string();
        }
        restore_secrets(input, &redaction_map)
    }

    /// Redact secrets and add to session map
    fn redact_and_store_secrets(&self, content: &str, path: Option<&str>) -> String {
        if !self.redact_secrets {
            return content.to_string();
        }

        // TODO: this is not thread safe, we need to use a mutex or an actor to protect the redaction map
        let existing_redaction_map = self.load_session_redaction_map();
        let redaction_result = redact_secrets(content, path, &existing_redaction_map);

        // Add new redactions to session map
        self.add_to_session_redaction_map(&redaction_result.redaction_map);

        redaction_result.redacted_string
    }

    fn _create_resource_text(&self, uri: &str, name: &str) -> Resource {
        RawResource::new(uri, name.to_string()).no_annotation()
    }

    #[tool(
        description = "A system command execution tool that allows running shell commands with full system access. 

SECRET HANDLING: 
- Output containing secrets will be redacted and shown as placeholders like [REDACTED_SECRET:rule-id:hash]
- You can use these placeholders in subsequent commands - they will be automatically restored to actual values before execution
- Example: If you see 'export API_KEY=[REDACTED_SECRET:api-key:abc123]', you can use '[REDACTED_SECRET:api-key:abc123]' in later commands

If the command's output exceeds 300 lines the result will be truncated and the full output will be saved to a file in the current directory"
    )]
    async fn run_command(
        &self,
        peer: rmcp::Peer<RoleServer>,
        #[tool(param)]
        #[schemars(description = "The shell command to execute")]
        command: String,
        #[tool(param)]
        #[schemars(description = "Optional working directory for command execution")]
        work_dir: Option<String>,
    ) -> Result<CallToolResult, McpError> {
        const MAX_LINES: usize = 300;

        let command_clone = command.clone();

        // Restore secrets in the command before execution
        let actual_command = self.restore_secrets_in_string(&command);

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

        let redacted_output = self.redact_and_store_secrets(&result, None);

        Ok(CallToolResult::success(vec![Content::text(
            &redacted_output,
        )]))
    }

    #[tool(
        description = "Generate configurations and infrastructure as code with suggested file names using a given prompt. This code generation only works for Terraform, Kubernetes, Dockerfile, and Github Actions. If save_files is true, the generated files will be saved to the filesystem. The printed shell output will redact any secrets, will be replaced with a placeholder [REDACTED_SECRET:rule-id:short-hash]"
    )]
    async fn generate_code(
        &self,
        #[tool(param)]
        #[schemars(
            description = "Prompt to use to generate code, this should be as detailed as possible. Make sure to specify output file paths if you want to save the files to the filesystem."
        )]
        prompt: String,
        #[tool(param)]
        #[schemars(
            description = "Type of code to generate one of Dockerfile, Kubernetes, Terraform, GithubActions"
        )]
        provisioner: Provisioner,
        #[tool(param)]
        #[schemars(
            description = "Whether to save the generated files to the filesystem (default: false)"
        )]
        save_files: Option<bool>,
    ) -> Result<CallToolResult, McpError> {
        let client = Client::new(&self.api_config).map_err(|e| {
            error!("Failed to create client: {}", e);
            McpError::internal_error(
                "Failed to create client",
                Some(json!({ "error": e.to_string() })),
            )
        })?;

        let output_format = if save_files.unwrap_or(false) {
            "json"
        } else {
            "markdown"
        };

        let response = match client
            .call_mcp_tool(&ToolsCallParams {
                name: "generate_code".to_string(),
                arguments: json!({
                    "prompt": prompt,
                    "provisioner": provisioner.to_string(),
                    "context": Vec::<serde_json::Value>::new(),
                    "output_format": output_format,
                }),
            })
            .await
        {
            Ok(response) => response,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("GENERATE_CODE_ERROR"),
                    Content::text(format!("Failed to generate code: {}", e)),
                ]));
            }
        };

        if save_files.unwrap_or(false) {
            let mut result_report = String::new();

            let response_text = response
                .iter()
                .map(|r| {
                    if let Some(RawTextContent { text }) = r.as_text() {
                        text.clone()
                    } else {
                        "".to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("");

            let generation_result: GenerationResult = serde_json::from_str(&response_text)
                .map_err(|e| {
                    error!("Failed to parse generation result: {}", e);
                    McpError::internal_error(
                        "Failed to parse generation result",
                        Some(json!({ "error": e.to_string() })),
                    )
                })?;

            // Group blocks by document_uri
            let mut grouped_blocks: HashMap<String, Vec<&stakpak_api::Block>> = HashMap::new();
            for block in &generation_result.created_blocks {
                grouped_blocks
                    .entry(block.document_uri.clone())
                    .or_default()
                    .push(block);
            }

            // Process each file
            for (document_uri, mut blocks) in grouped_blocks {
                // Sort blocks by start line number
                blocks.sort_by(|a, b| a.start_point.row.cmp(&b.start_point.row));

                // Concatenate the code blocks
                let file_content = blocks
                    .iter()
                    .map(|block| block.code.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");

                // Strip file:/// prefix from document_uri to get the file path
                let file_path = document_uri
                    .strip_prefix("file:///")
                    .unwrap_or(&document_uri);

                // Create parent directories if they don't exist
                if let Some(parent) = Path::new(file_path).parent() {
                    if !parent.exists() {
                        if let Err(e) = fs::create_dir_all(parent) {
                            error!("Failed to create directory {}: {}", parent.display(), e);
                            continue;
                        }
                    }
                }

                // Write the file
                let redacted_content =
                    self.redact_and_store_secrets(&file_content, Some(file_path));
                match fs::write(file_path, &file_content) {
                    Ok(_) => {
                        result_report.push_str(&format!(
                            "Created file {}\n```\n{}\n```\n\n",
                            file_path, redacted_content
                        ));
                    }
                    Err(e) => {
                        result_report.push_str(&format!(
                            "Failed to create file {} with error: {}\n```\n{}\n```\n\n",
                            file_path, e, redacted_content
                        ));
                    }
                }
            }

            // ignore modified blocks
            for block in generation_result.modified_blocks {
                result_report.push_str(&format!(
                    "Ignored modified block:\n{}\n```\n{}\n```\n\n",
                    block.document_uri, block.code
                ));
            }

            // ignore removed blocks
            for block in generation_result.removed_blocks {
                result_report.push_str(&format!(
                    "Ignored removed block:\n{}\n```\n{}\n```\n\n",
                    block.document_uri, block.code
                ));
            }

            Ok(CallToolResult::success(vec![Content::text(result_report)]))
        } else {
            Ok(CallToolResult::success(response))
        }
    }

    #[tool(
        description = "Query remote configurations and infrastructure as code indexed in Stakpak using natural language. This function uses a smart retrival system to find relevant code blocks with a relevance score, not just keyword matching. This function is useful for finding code blocks that are not in your local filesystem."
    )]
    async fn smart_search_code(
        &self,
        #[tool(param)]
        #[schemars(
            description = "The natural language query to find relevant code blocks, the more detailed the query the better the results will be"
        )]
        query: String,
        // #[tool(param)]
        // #[schemars(
        //     description = "The flow reference in the format owner/name/version, this allows you to limit the search scopre to a specific project (optional)"
        // )]
        // flow_ref: Option<String>,
        #[tool(param)]
        #[schemars(description = "The maximum number of results to return (default: 10)")]
        limit: Option<u32>,
    ) -> Result<CallToolResult, McpError> {
        let client = Client::new(&self.api_config).map_err(|e| {
            error!("Failed to create client: {}", e);
            McpError::internal_error(
                "Failed to create client",
                Some(json!({ "error": e.to_string() })),
            )
        })?;

        let response = match client
            .call_mcp_tool(&ToolsCallParams {
                name: "smart_search_code".to_string(),
                arguments: json!({
                    "query": query,
                    "limit": limit,
                }),
            })
            .await
        {
            Ok(response) => response,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("SMART_SEARCH_CODE_ERROR"),
                    Content::text(format!("Failed to search for code: {}", e)),
                ]));
            }
        };

        Ok(CallToolResult::success(response))
    }

    #[tool(
        description = "View the contents of a file or list the contents of a directory. Can read entire files or specific line ranges.

SECRET HANDLING:
- File contents containing secrets will be redacted and shown as placeholders like [REDACTED_SECRET:rule-id:hash]
- These placeholders represent actual secret values that are safely stored for later use
- You can reference these placeholders when working with the file content

A maximum of 300 lines will be shown at a time, the rest will be truncated."
    )]
    fn view(
        &self,
        #[tool(param)]
        #[schemars(description = "The path to the file or directory to view")]
        path: String,
        #[tool(param)]
        #[schemars(
            description = "Optional line range to view [start_line, end_line]. Line numbers are 1-indexed. Use -1 for end_line to read to end of file."
        )]
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

                    let redacted_result = self.redact_and_store_secrets(&result, Some(&path));
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
    fn str_replace(
        &self,
        #[tool(param)]
        #[schemars(description = "The path to the file to modify")]
        path: String,
        #[tool(param)]
        #[schemars(
            description = "The exact text to replace (must match exactly, including whitespace and indentation)"
        )]
        old_str: String,
        #[tool(param)]
        #[schemars(
            description = "The new text to insert in place of the old text. When replacing code, ensure the new text maintains proper syntax, indentation, and follows the codebase style."
        )]
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
        let actual_old_str = self.restore_secrets_in_string(&old_str);
        let actual_new_str = self.restore_secrets_in_string(&new_str);

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

    #[tool(
        description = "Create a new file with the specified content. Will fail if file already exists. When creating code, ensure the new text has proper syntax, indentation, and follows the codebase style. Parent directories will be created automatically if they don't exist."
    )]
    fn create(
        &self,
        #[tool(param)]
        #[schemars(description = "The path where the new file should be created")]
        path: String,
        #[tool(param)]
        #[schemars(
            description = "The content to write to the new file, when creating code, ensure the new text has proper syntax, indentation, and follows the codebase style."
        )]
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
        let actual_file_text = self.restore_secrets_in_string(&file_text);

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
        description = "Insert text at a specific line number in a file. Line numbers are 1-indexed."
    )]
    fn insert(
        &self,
        #[tool(param)]
        #[schemars(description = "The path to the file to modify")]
        path: String,
        #[tool(param)]
        #[schemars(description = "The line number where text should be inserted (1-indexed)")]
        insert_line: u32,
        #[tool(param)]
        #[schemars(description = "The text to insert")]
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
                let actual_new_str = self.restore_secrets_in_string(&new_str);

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
impl ServerHandler for Tools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "This server provides a tool that can run commands on the system.".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_redaction_map() {
        // Create a Tools instance with secret redaction enabled
        let api_config = ClientConfig {
            api_key: Some("test".to_string()),
            api_endpoint: "https://test.com".to_string(),
        };
        let tools = Tools::new(api_config, true);

        // Test that session file path is as expected
        let path = LocalStore::get_local_session_store_path().join("secrets.json");

        // Clean up any existing session file before test
        let _ = std::fs::remove_file(path.clone());

        // Test adding redactions to session map
        let mut test_redactions = HashMap::new();
        test_redactions.insert(
            "[REDACTED_SECRET:api-key:abc123]".to_string(),
            "secret_value_123".to_string(),
        );
        test_redactions.insert(
            "[REDACTED_SECRET:token:def456]".to_string(),
            "token_value_456".to_string(),
        );

        tools.add_to_session_redaction_map(&test_redactions);

        // Verify the session file was created and contains valid JSON
        assert!(path.exists(), "Session file should be created");

        let file_content =
            std::fs::read_to_string(&path).expect("Should be able to read session file");
        let json_value: serde_json::Value =
            serde_json::from_str(&file_content).expect("Session file should contain valid JSON");
        assert!(
            json_value.is_object(),
            "Session file should contain a JSON object"
        );

        // Test loading redaction map
        let loaded_map = tools.load_session_redaction_map();
        assert_eq!(loaded_map.len(), 2);
        assert_eq!(
            loaded_map.get("[REDACTED_SECRET:api-key:abc123]"),
            Some(&"secret_value_123".to_string())
        );
        assert_eq!(
            loaded_map.get("[REDACTED_SECRET:token:def456]"),
            Some(&"token_value_456".to_string())
        );

        // Test secret restoration
        let input_with_placeholders = "echo '[REDACTED_SECRET:api-key:abc123]' > file.txt && curl -H 'Authorization: Bearer [REDACTED_SECRET:token:def456]'";
        let restored = tools.restore_secrets_in_string(input_with_placeholders);
        let expected =
            "echo 'secret_value_123' > file.txt && curl -H 'Authorization: Bearer token_value_456'";
        assert_eq!(restored, expected);

        // Clean up the test session file
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_redact_and_store_secrets() {
        // Create a Tools instance with secret redaction enabled
        let api_config = ClientConfig {
            api_key: Some("test".to_string()),
            api_endpoint: "https://test.com".to_string(),
        };
        let tools = Tools::new(api_config, true);

        // Test content with secrets
        let content_with_secrets =
            "export API_KEY=abc123def456ghi789jklmnop\nexport TOKEN=xyz789uvw012";
        let redacted = tools.redact_and_store_secrets(content_with_secrets, None);

        // Should contain redaction placeholders
        assert!(redacted.contains("[REDACTED_SECRET:"));

        // Should have stored the redactions in session map
        let session_map = tools.load_session_redaction_map();
        assert!(!session_map.is_empty());

        // Should be able to restore the original content
        let restored = tools.restore_secrets_in_string(&redacted);
        // Note: The restored content might not be exactly the same as original due to redaction rules,
        // but it should contain the original secret values
        assert!(
            restored.contains("abc123def456ghi789jklmnop") || restored.contains("xyz789uvw012")
        );

        // Clean up the test session file
        let path = LocalStore::get_local_session_store_path().join("secrets.json");
        let _ = std::fs::remove_file(&path);
    }
}
