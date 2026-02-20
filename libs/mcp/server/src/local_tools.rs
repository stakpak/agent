use crate::tool_container::ToolContainer;
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, handler::server::wrapper::Parameters, model::*, schemars, tool};
use rmcp::{RoleServer, tool_router};
use serde::{Deserialize, Deserializer};
use stakpak_shared::file_backup_manager::FileBackupManager;
use stakpak_shared::remote_connection::{
    PathLocation, RemoteConnection, RemoteConnectionInfo, RemoteFileSystemProvider,
};

use globset::Glob;
use grep_regex::RegexMatcher;
use grep_searcher::Searcher;
use grep_searcher::sinks::UTF8;
use html2md;
use ignore::WalkBuilder;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde_json::json;
use similar::TextDiff;
use stakpak_shared::models::async_manifest::{AsyncManifest, PendingToolCall};
use stakpak_shared::models::integrations::mcp::CallToolResultExt;
use stakpak_shared::models::integrations::openai::{
    ProgressType, TaskPauseInfo, TaskUpdate, ToolCallResultProgress,
};
use stakpak_shared::task_manager::TaskInfo;
use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};
use stakpak_shared::utils::{
    LocalFileSystemProvider, generate_directory_tree, handle_large_output, sanitize_text_output,
};
use std::fs::{self};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::{Duration, sleep, timeout as tokio_timeout};
use tracing::error;
use url;
use uuid::Uuid;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RunCommandRequest {
    #[schemars(description = "The shell command to execute")]
    pub command: String,
    #[schemars(description = "Optional description of the command to execute")]
    pub description: Option<String>,
    #[schemars(description = "Optional timeout for the command execution in seconds")]
    pub timeout: Option<u64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_nonempty_trimmed_string"
    )]
    #[schemars(
        description = "Optional remote connection string (format: user@host or user@host:port). Omit this field for local execution; do not send an empty string."
    )]
    pub remote: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_nonempty_preserved_string"
    )]
    #[schemars(description = "Optional password for remote connection")]
    pub password: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_nonempty_trimmed_string"
    )]
    #[schemars(description = "Optional path to private key for remote connection")]
    pub private_key_path: Option<String>,
}

#[derive(Debug)]
pub struct CommandResult {
    pub output: String,
    pub exit_code: i32,
}

fn deserialize_optional_nonempty_trimmed_string<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }))
}

fn deserialize_optional_nonempty_preserved_string<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.and_then(|raw| {
        if raw.trim().is_empty() {
            None
        } else {
            Some(raw)
        }
    }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TaskStatusRequest {
    #[schemars(description = "The task ID to get status for")]
    pub task_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetTaskDetailsRequest {
    #[schemars(description = "The task ID to get details for")]
    pub task_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetAllTasksRequest {
    #[schemars(description = "View parameter (required for compatibility, any value works)")]
    pub view: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AwaitTasksRequest {
    #[schemars(description = "Space-separated list of task IDs to wait for completion")]
    pub task_ids: String,
    #[schemars(description = "Optional timeout in seconds. If not specified, waits indefinitely")]
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ViewRequest {
    #[schemars(
        description = "The path to the file or directory to view. For remote files, use format: user@host:/path or user@host#port:/path (use ABSOLUTE paths for remote files)"
    )]
    pub path: String,
    #[schemars(
        description = "Optional line range to view [start_line, end_line]. Line numbers are 1-indexed. Use -1 for end_line to read to end of file."
    )]
    pub view_range: Option<[i32; 2]>,
    #[schemars(
        description = "Regex pattern to search for in file contents. Returns matching lines with line numbers. For directories, searches all files recursively (respects .gitignore)."
    )]
    pub grep: Option<String>,
    #[schemars(
        description = "Glob pattern to filter files when viewing directories (e.g., '*.rs', '**/*.ts', 'src/**/*.go'). Only applies to directory views."
    )]
    pub glob: Option<String>,
    #[schemars(description = "Optional password for remote connection (if path is remote)")]
    pub password: Option<String>,
    #[schemars(
        description = "Optional path to private key for remote connection (if path is remote)"
    )]
    pub private_key_path: Option<String>,
    #[schemars(description = "Display directory as a nested tree structure (default: false)")]
    pub tree: Option<bool>,
}

/// Options for viewing files/directories (used internally to reduce function arguments)
#[derive(Debug, Clone)]
pub struct ViewOptions<'a> {
    pub view_range: Option<[i32; 2]>,
    pub max_lines: usize,
    pub tree: Option<bool>,
    pub grep: Option<&'a str>,
    pub glob: Option<&'a str>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StrReplaceRequest {
    #[schemars(
        description = "The path to the file to modify. For remote files, use format: user@host:/path or user@host#port:/path (use ABSOLUTE paths for remote files)"
    )]
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
    #[schemars(description = "Optional password for remote connection (if path is remote)")]
    pub password: Option<String>,
    #[schemars(
        description = "Optional path to private key for remote connection (if path is remote)"
    )]
    pub private_key_path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateRequest {
    #[schemars(
        description = "The path where the new file should be created. For remote files, use format: user@host:/path or user@host#port:/path (use ABSOLUTE paths for remote files)"
    )]
    pub path: String,
    #[schemars(
        description = "The content to write to the new file, when creating code, ensure the new text has proper syntax, indentation, and follows the codebase style."
    )]
    pub file_text: String,
    #[schemars(description = "Optional password for remote connection (if path is remote)")]
    pub password: Option<String>,
    #[schemars(
        description = "Optional path to private key for remote connection (if path is remote)"
    )]
    pub private_key_path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GeneratePasswordRequest {
    #[schemars(description = "The length of the password to generate")]
    pub length: Option<usize>,
    #[schemars(description = "Whether to disallow symbols in the password (default: false)")]
    pub no_symbols: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RemoveRequest {
    #[schemars(
        description = "The path to the file or directory to remove. For remote files, use format: user@host:/path or user@host#port:/path (use ABSOLUTE paths for remote files)"
    )]
    pub path: String,
    #[schemars(
        description = "Whether to remove directories recursively (required for non-empty directories, default: false)"
    )]
    pub recursive: Option<bool>,
    #[schemars(description = "Optional password for remote connection (if path is remote)")]
    pub password: Option<String>,
    #[schemars(
        description = "Optional path to private key for remote connection (if path is remote)"
    )]
    pub private_key_path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ViewWebPageRequest {
    #[schemars(description = "The HTTPS URL of the web page to fetch and convert to markdown")]
    pub url: String,
}

use stakpak_shared::models::tools::ask_user::AskUserRequest;

#[tool_router(router = tool_router_local, vis = "pub")]
impl ToolContainer {
    #[tool(
        description = "A system command execution tool that allows running shell commands with full system access on local or remote systems via SSH.

REMOTE EXECUTION:
- Set 'remote' parameter to 'user@host' or 'user@host:port' for SSH execution
- Use 'password' for password authentication or 'private_key_path' for key-based auth
- Automatic SSH key discovery from ~/.ssh/ (id_ed25519, id_rsa, etc.) if no credentials provided
- Examples:
  * 'user@server.com' (uses default port 22 and auto-discovered keys)
  * 'user@server.com:2222' with password authentication
  * Remote paths: 'ssh://user@host/path' or 'user@host:/path'

If the command's output exceeds 300 lines the result will be truncated and the full output will be saved to a file in the current directory"
    )]
    pub async fn run_command(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(RunCommandRequest {
            command,
            description: _,
            timeout,
            remote,
            password,
            private_key_path,
        }): Parameters<RunCommandRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Use unified command execution helper
        match self
            .execute_command_unified(&command, timeout, remote, password, private_key_path, &ctx)
            .await
        {
            Ok(mut command_result) => {
                command_result.output =
                    match handle_large_output(&command_result.output, "command.output", 300, false)
                    {
                        Ok(result) => result,
                        Err(e) => {
                            return Ok(CallToolResult::error(vec![
                                Content::text("OUTPUT_HANDLING_ERROR"),
                                Content::text(format!("Failed to handle command output: {}", e)),
                            ]));
                        }
                    };

                if command_result.output.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text("No output")]));
                }

                if command_result.exit_code != 0 {
                    return Ok(CallToolResult::error(vec![
                        Content::text("COMMAND_FAILED"),
                        Content::text(&command_result.output),
                    ]));
                }
                Ok(CallToolResult::success(vec![Content::text(
                    &command_result.output,
                )]))
            }
            Err(error_result) => Ok(error_result),
        }
    }

    #[tool(
        description = "Execute a shell command asynchronously in the background on LOCAL OR REMOTE systems and return immediately with task information without waiting for completion.

REMOTE EXECUTION SUPPORT:
- Set 'remote' parameter to 'user@host' or 'user@host:port' for SSH background execution
- Use 'password' for password authentication or 'private_key_path' for key-based auth
- Automatic SSH key discovery from ~/.ssh/ if no credentials provided
- Examples:
  * 'user@server.com' - Remote background task with auto-discovered keys
  * 'user@server.com:2222' - Remote background task with custom port

Use this for port-forwarding, starting servers, tailing logs, or other long-running commands that you want to monitor separately, or whenever the user wants to run a command in the background.

PARAMETERS:
- command: The shell command to execute (locally or remotely)
- description: Optional description of the command (not used in execution)
- timeout: Optional timeout in seconds after which the task will be terminated
- remote: Optional remote connection string for SSH execution
- password: Optional password for remote authentication
- private_key_path: Optional path to private key for remote authentication

RETURNS:
- task_id: Unique identifier for the background task
- status: Current task status (will be 'Running' initially)
- start_time: When the task was started

Use the get_all_tasks tool to monitor task progress, or the cancel_task tool to cancel a task."
    )]
    pub async fn run_command_task(
        &self,
        _ctx: RequestContext<RoleServer>,
        Parameters(RunCommandRequest {
            command,
            description,
            timeout,
            remote,
            password,
            private_key_path,
        }): Parameters<RunCommandRequest>,
    ) -> Result<CallToolResult, McpError> {
        let timeout_duration = timeout.map(std::time::Duration::from_secs);

        // Handle both local and remote async commands using TaskManager
        let result = if let Some(remote_str) = remote {
            // Remote async command
            let remote_connection = RemoteConnectionInfo {
                connection_string: remote_str,
                password,
                private_key_path,
            };

            self.get_task_manager()
                .start_task(
                    command,
                    description,
                    timeout_duration,
                    Some(remote_connection),
                )
                .await
        } else {
            // Local async command (existing logic)
            self.get_task_manager()
                .start_task(command, description, timeout_duration, None)
                .await
        };

        match result {
            Ok(task_info) => {
                let output = serde_json::to_string_pretty(&task_info)
                    .unwrap_or_else(|_| format!("Task started: {}", task_info.id));

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Background task started:\n{}",
                    output
                ))]))
            }
            Err(e) => {
                error!("Failed to start background task: {}", e);

                Ok(CallToolResult::error(vec![
                    Content::text("RUN_COMMAND_TASK_ERROR"),
                    Content::text(format!("Failed to start background task: {}", e)),
                ]))
            }
        }
    }

    #[tool(
        description = "Get the status of all background tasks started with run_command_task.

RETURNS:
- A markdown-formatted table showing all background tasks with:
  - Task ID: Full unique identifier (required for cancel_task)
  - Status: Current status (Running, Completed, Failed, Cancelled, TimedOut)
  - Start Time: When the task was started
  - Duration: How long the task has been running or took to complete
  - Output: Command output preview (truncated to 80 chars)

This tool provides a clean tabular overview of all background tasks and their current state.
Use the full Task ID from this output with cancel_task to cancel specific tasks."
    )]
    pub async fn get_all_tasks(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(GetAllTasksRequest { view: _ }): Parameters<GetAllTasksRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.get_task_manager().get_all_tasks().await {
            Ok(tasks) => {
                if tasks.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(
                        "No background tasks found.",
                    )]));
                }

                // Send progress notifications for any paused tasks so the TUI
                // caches their pause_info for the approval display.
                let paused_updates: Vec<TaskUpdate> = tasks
                    .iter()
                    .filter(|t| {
                        matches!(t.status, stakpak_shared::task_manager::TaskStatus::Paused)
                    })
                    .map(Self::build_task_update)
                    .collect();

                if !paused_updates.is_empty() {
                    let progress_id = uuid::Uuid::new_v4();
                    let _ = ctx
                        .peer
                        .notify_progress(ProgressNotificationParam {
                            progress_token: ProgressToken(NumberOrString::Number(0)),
                            progress: 100.0,
                            total: Some(100.0),
                            message: Some(
                                serde_json::to_string(&ToolCallResultProgress {
                                    id: progress_id,
                                    message: String::new(),
                                    progress_type: Some(ProgressType::TaskWait),
                                    task_updates: Some(paused_updates),
                                    progress: Some(100.0),
                                })
                                .unwrap_or_default(),
                            ),
                        })
                        .await;
                }

                // Create markdown table format
                let mut table = String::new();
                table.push_str("# Background Tasks\n\n");

                // Markdown table header
                table.push_str("| Task ID | Status | Command | Start Time | Duration | Output |\n");
                table.push_str("|---------|--------|------------|----------|--------|--------|\n");

                // Markdown table rows
                for task in &tasks {
                    let task_id = task.id.clone();
                    let status = format!("{:?}", task.status);
                    let start_time = task.start_time.to_rfc3339();
                    let duration = if let Some(duration) = task.duration {
                        format!("{:.2}s", duration.as_secs_f64())
                    } else {
                        "N/A".to_string()
                    };

                    let output_str = if let Some(ref out) = task.output {
                        out.clone()
                    } else {
                        "No output yet".to_string()
                    };

                    let escaped_command = task
                        .command
                        .chars()
                        .take(100)
                        .collect::<String>()
                        .replace('|', "\\|")
                        .replace('\n', " ");
                    let escaped_output = output_str
                        .chars()
                        .take(100)
                        .collect::<String>()
                        .replace('|', "\\|")
                        .replace('\n', " ");

                    table.push_str(&format!(
                        "| {} | {} | {} | {} | {} | {} |\n",
                        task_id, status, escaped_command, start_time, duration, escaped_output
                    ));
                }

                table.push_str(&format!("\n**Total: {} task(s)**", tasks.len()));

                Ok(CallToolResult::success(vec![Content::text(table)]))
            }
            Err(e) => {
                error!("Failed to get all tasks: {}", e);

                Ok(CallToolResult::error(vec![
                    Content::text("GET_ALL_TASKS_ERROR"),
                    Content::text(format!("Failed to get all tasks: {}", e)),
                ]))
            }
        }
    }

    #[tool(
        description = "Cancel a running asynchronous background task started with run_command_task.

PARAMETERS:
- task_id: The unique identifier of the task to cancel. Use the get_all_tasks tool to get the task ID.

This will immediately terminate the background task and update the task status to 'Cancelled'.
The task will be removed from the active tasks list."
    )]
    pub async fn cancel_task(
        &self,
        _ctx: RequestContext<RoleServer>,
        Parameters(TaskStatusRequest { task_id }): Parameters<TaskStatusRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.get_task_manager().cancel_task(task_id.clone()).await {
            Ok(task_info) => {
                let output = serde_json::to_string_pretty(&task_info)
                    .unwrap_or_else(|_| format!("Task cancelled: {}", task_info.id));

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Task cancelled:\n{}",
                    output
                ))]))
            }
            Err(e) => {
                error!("Failed to cancel task: {}", e);

                Ok(CallToolResult::error(vec![
                    Content::text("CANCEL_TASK_ERROR"),
                    Content::text(format!("Failed to cancel task: {}", e)),
                ]))
            }
        }
    }

    #[tool(
        description = "Wait for one or more background tasks to complete or fail, then return the status of all tasks.

PARAMETERS:
- task_ids: Space-separated list of task IDs to wait for completion (e.g., \"abc123 def456 ghi789\")
- timeout: Optional timeout in seconds. If not specified, waits indefinitely

BEHAVIOR:
- Waits until ALL specified tasks reach a final state (Completed, Failed, Cancelled, or TimedOut)
- If timeout is specified, returns an error if tasks don't complete within that time
- Returns the same format as get_all_tasks showing all background tasks after waiting
- If any task ID doesn't exist, returns an error immediately
- This is useful for coordinating async tasks and getting results once they're done

EXAMPLE USAGE:
1. Start multiple async tasks with run_command_task
2. Use wait_for_tasks with those IDs to wait for completion
3. Process the results from all tasks

This tool enables proper task synchronization and coordination in complex workflows."
    )]
    pub async fn wait_for_tasks(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(AwaitTasksRequest { task_ids, timeout }): Parameters<AwaitTasksRequest>,
    ) -> Result<CallToolResult, McpError> {
        let task_ids: Vec<String> = task_ids.split_whitespace().map(|s| s.to_string()).collect();

        if task_ids.is_empty() {
            return Ok(CallToolResult::error(vec![
                Content::text("AWAIT_TASKS_ERROR"),
                Content::text(
                    "No task IDs provided. Please provide a space-separated list of task IDs.",
                ),
            ]));
        }

        let timeout = timeout.map(std::time::Duration::from_secs);

        match self
            .wait_for_tasks_with_streaming(&task_ids, timeout, &ctx)
            .await
        {
            Ok(tasks) => {
                let table = self.format_tasks_table(&tasks, &task_ids);

                Ok(CallToolResult::success(vec![Content::text(table)]))
            }
            Err(e) => {
                error!("Failed to await tasks: {}", e);

                Ok(CallToolResult::error(vec![
                    Content::text("AWAIT_TASKS_ERROR"),
                    Content::text(format!("Failed to await tasks: {}", e)),
                ]))
            }
        }
    }

    #[tool(
        description = "Get detailed information about a specific background task by its ID.

This tool provides comprehensive details about a background task started with run_command_task, including:
- Current status (Running, Completed, Failed, Cancelled, TimedOut, Pending)
- Task ID and start time
- Duration (elapsed time for running tasks, total time for completed tasks)
- Complete command output
- Error information if the task failed

If the task output exceeds 300 lines the result will be truncated and the full output will be saved to a file in the current directory.

Use this tool to check the progress and results of long-running background tasks."
    )]
    pub async fn get_task_details(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(GetTaskDetailsRequest { task_id }): Parameters<GetTaskDetailsRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .get_task_manager()
            .get_task_details(task_id.clone())
            .await
        {
            Ok(Some(task_info)) => {
                let duration_str = if let Some(duration) = task_info.duration {
                    format!("{:.2}s", duration.as_secs_f64())
                } else {
                    "N/A".to_string()
                };

                // If the task is paused, send a progress notification so the TUI
                // caches the pause_info (pending tool calls) for the approval display.
                // Without this, only wait_for_tasks populates the cache and
                // resume_subagent_task shows a generic "Resume subagent task" message.
                if matches!(
                    task_info.status,
                    stakpak_shared::task_manager::TaskStatus::Paused
                ) {
                    let task_update = Self::build_task_update(&task_info);
                    let progress_id = uuid::Uuid::new_v4();
                    let _ = ctx
                        .peer
                        .notify_progress(ProgressNotificationParam {
                            progress_token: ProgressToken(NumberOrString::Number(0)),
                            progress: 100.0,
                            total: Some(100.0),
                            message: Some(
                                serde_json::to_string(&ToolCallResultProgress {
                                    id: progress_id,
                                    message: String::new(),
                                    progress_type: Some(ProgressType::TaskWait),
                                    task_updates: Some(vec![task_update]),
                                    progress: Some(100.0),
                                })
                                .unwrap_or_default(),
                            ),
                        })
                        .await;
                }

                // Try to parse output as AsyncManifest (subagent JSON output)
                // If successful, format it in a human/LLM-friendly way
                let output_str = if let Some(ref output) = task_info.output {
                    if let Some(manifest) = AsyncManifest::try_parse(output) {
                        // Subagent output - use Display impl for LLM-friendly formatting
                        manifest.to_string()
                    } else {
                        // Regular task output - use standard handling
                        match handle_large_output(output, "task.output", 300, false) {
                            Ok(result) => result,
                            Err(e) => {
                                return Ok(CallToolResult::error(vec![
                                    Content::text("OUTPUT_HANDLING_ERROR"),
                                    Content::text(format!("Failed to handle task output: {}", e)),
                                ]));
                            }
                        }
                    }
                } else {
                    "No output available".to_string()
                };

                let output = format!(
                    "# Task Details: {}\n\nStatus: {:?}\nTask ID: {}\nStarted: {}\nDuration: {}\n\n## Output:\n{}",
                    task_info.id,
                    task_info.status,
                    task_info.id,
                    task_info.start_time.format("%Y-%m-%d %H:%M:%S UTC"),
                    duration_str,
                    output_str
                );

                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            Ok(None) => Ok(CallToolResult::error(vec![
                Content::text("TASK_NOT_FOUND"),
                Content::text(format!("Task not found: {}", task_id)),
            ])),
            Err(e) => {
                error!("Failed to get task details: {}", e);

                Ok(CallToolResult::error(vec![
                    Content::text("GET_TASK_DETAILS_ERROR"),
                    Content::text(format!("Failed to get task details: {}", e)),
                ]))
            }
        }
    }

    #[tool(
        description = "View the contents of a local or remote file/directory. Can read entire files or specific line ranges.

REMOTE FILE ACCESS:
- Use path formats: 'user@host:/path' or 'user@host#port:/path' for remote files
- IMPORTANT: Use ABSOLUTE paths for remote files/directories (e.g., '/etc/config' not 'config')
- Use 'password' for password authentication or 'private_key_path' for key-based auth
- Automatic SSH key discovery from ~/.ssh/ if no credentials provided
- Examples:
  * 'user@server.com:/etc/nginx/nginx.conf' - Remote file with auto-discovered keys
  * 'ssh://user@server.com/var/log/app.log' - Remote file with SSH URL format
  * 'user@server.com:/home/user/documents' - Remote directory listing
  * '/local/path/file.txt' - Local file (default behavior)

For directories:
- Default behavior: Lists immediate directory contents
- With tree=true: Displays nested directory structure as a tree (limited to 3 levels deep)

GREP (Content Search):
- Use 'grep' parameter with a regex pattern to search file contents
- For files: Returns matching lines with line numbers (format: line_num:content)
- For directories: Recursively searches all files, respects .gitignore (format: file:line_num:content)
- Examples:
  * grep='TODO|FIXME' - Find all TODO/FIXME comments
  * grep='fn\\s+\\w+' - Find Rust function definitions
  * grep='error' - Simple text search

GLOB (File Filtering):
- Use 'glob' parameter to filter files in directories by pattern
- Supports standard glob syntax: *, ?, [abc], **
- Examples:
  * glob='*.rs' - All Rust files
  * glob='**/*.ts' - All TypeScript files (recursive)
  * glob='test_*.py' - Python test files

A maximum of 300 lines will be shown at a time, the rest will be truncated."
    )]
    pub async fn view(
        &self,
        Parameters(ViewRequest {
            path,
            view_range,
            grep,
            glob,
            password,
            private_key_path,
            tree,
        }): Parameters<ViewRequest>,
    ) -> Result<CallToolResult, McpError> {
        const MAX_LINES: usize = 300;

        // Check if this is a remote path
        if Self::is_remote_path(&path) {
            // Handle remote file/directory viewing
            match self
                .get_remote_connection(&path, password, private_key_path)
                .await
            {
                Ok((conn, remote_path)) => {
                    let opts = ViewOptions {
                        view_range,
                        max_lines: MAX_LINES,
                        tree,
                        grep: grep.as_deref(),
                        glob: glob.as_deref(),
                    };
                    self.view_remote_path(&conn, &remote_path, &path, &opts)
                        .await
                }
                Err(error_result) => Ok(error_result),
            }
        } else {
            // Handle local file/directory viewing
            let opts = ViewOptions {
                view_range,
                max_lines: MAX_LINES,
                tree,
                grep: grep.as_deref(),
                glob: glob.as_deref(),
            };
            self.view_local_path(&path, &opts).await
        }
    }

    #[tool(
        description = "Replace a specific string in a local or remote file with new text. The old_str must match exactly including whitespace and indentation.

REMOTE FILE EDITING:
- Use path formats: 'user@host:/path' or 'user@host#port:/path' for remote files
- IMPORTANT: Use ABSOLUTE paths for remote files (e.g., '/etc/config' not 'config')
- Use 'password' for password authentication or 'private_key_path' for key-based auth
- Automatic SSH key discovery from ~/.ssh/ if no credentials provided
- Examples:
  * 'user@server.com:/etc/nginx/sites-available/default' - Edit remote config
  * 'ssh://user@server.com/var/www/app/config.php' - Edit remote application config
  * '/local/path/file.txt' - Edit local file (default behavior)

When replacing code, ensure the new text maintains proper syntax, indentation, and follows the codebase style."
    )]
    pub async fn str_replace(
        &self,
        Parameters(StrReplaceRequest {
            path,
            old_str,
            new_str,
            replace_all,
            password,
            private_key_path,
        }): Parameters<StrReplaceRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Check if this is a remote path
        if Self::is_remote_path(&path) {
            // Handle remote file replacement
            match self
                .get_remote_connection(&path, password, private_key_path)
                .await
            {
                Ok((conn, remote_path)) => {
                    self.str_replace_remote(
                        &conn,
                        &remote_path,
                        &path,
                        &old_str,
                        &new_str,
                        replace_all,
                    )
                    .await
                }
                Err(error_result) => Ok(error_result),
            }
        } else {
            // Handle local file replacement
            self.str_replace_local(&path, &old_str, &new_str, replace_all)
                .await
        }
    }

    #[tool(
        description = "Create a new local or remote file with the specified content. Will fail if file already exists. When creating code, ensure the new text has proper syntax, indentation, and follows the codebase style. Parent directories will be created automatically if they don't exist.

REMOTE FILE CREATION:
- Use path formats: 'user@host:/path' or 'user@host#port:/path' for remote files
- IMPORTANT: Use ABSOLUTE paths for remote files (e.g., '/tmp/script.sh' not 'script.sh')
- Use 'password' for password authentication or 'private_key_path' for key-based auth
- Automatic SSH key discovery from ~/.ssh/ if no credentials provided
- Parent directories will be created automatically on remote systems
- Examples:
  * 'user@server.com:/tmp/script.sh' - Create remote script
  * 'ssh://user@server.com/var/www/new-config.json' - Create remote config
  * '/local/path/file.txt' - Create local file (default behavior)"
    )]
    pub async fn create(
        &self,
        Parameters(CreateRequest {
            path,
            file_text,
            password,
            private_key_path,
        }): Parameters<CreateRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Check if this is a remote path
        if Self::is_remote_path(&path) {
            // Handle remote file creation
            match self
                .get_remote_connection(&path, password, private_key_path)
                .await
            {
                Ok((conn, remote_path)) => {
                    self.create_remote(&conn, &remote_path, &path, &file_text)
                        .await
                }
                Err(error_result) => Ok(error_result),
            }
        } else {
            // Handle local file creation
            self.create_local(&path, &file_text)
        }
    }

    #[tool(
        description = "Generate a cryptographically secure password with the specified constraints.

PARAMETERS:
- length: The length of the password to generate (default: 15 characters)
- no_symbols: Whether to exclude symbols from the password (default: false, includes symbols)

CHARACTER SETS:
- Letters: A-Z, a-z (always included)
- Numbers: 0-9 (always included)
- Symbols: !@#$%^&*()_+-=[]{}|;:,.<>? (included unless no_symbols=true)

SECURITY FEATURES:
- Uses cryptographically secure random number generation
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

        let password = stakpak_shared::utils::generate_password(length, no_symbols);

        Ok(CallToolResult::success(vec![Content::text(&password)]))
    }

    #[tool(
        description = "Fetch and view the text content of a web page by converting its HTML to markdown format.

SECURITY FEATURES:
- Only allows HTTPS URLs for secure connections
- Follows redirects safely with limits

The tool fetches the HTML content from the specified URL and converts it to clean, readable markdown. This is useful for reading web articles, documentation, or any web content in a text-friendly format.

The response will be truncated if it exceeds 300 lines, with the full content saved to a local file."
    )]
    pub async fn view_web_page(
        &self,
        _ctx: RequestContext<RoleServer>,
        Parameters(ViewWebPageRequest { url }): Parameters<ViewWebPageRequest>,
    ) -> Result<CallToolResult, McpError> {
        let parsed_url = match url::Url::parse(&url) {
            Ok(u) => u,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("INVALID_URL"),
                    Content::text(format!("Invalid URL format: {}", e)),
                ]));
            }
        };

        if parsed_url.scheme() != "https" {
            return Ok(CallToolResult::error(vec![
                Content::text("INSECURE_URL"),
                Content::text("Only HTTPS URLs are allowed for security reasons"),
            ]));
        }

        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("Mozilla/5.0 (compatible; StakPak-MCP-Bot/1.0)"),
        );

        let client = match create_tls_client(TlsClientConfig::default().with_headers(headers)) {
            Ok(client) => client,
            Err(e) => {
                error!("Failed to create HTTP client: {}", e);
                return Ok(CallToolResult::error(vec![
                    Content::text("HTTP_CLIENT_ERROR"),
                    Content::text(format!("Failed to create HTTP client: {}", e)),
                ]));
            }
        };

        let response = match client.get(&url).send().await {
            Ok(response) => response,
            Err(e) => {
                error!("Failed to fetch web page: {}", e);
                return Ok(CallToolResult::error(vec![
                    Content::text("FAILED_TO_FETCH_WEB_PAGE"),
                    Content::text(format!("Failed to fetch web page: {}", e)),
                ]));
            }
        };

        if !response.status().is_success() {
            return Ok(CallToolResult::error(vec![
                Content::text("HTTP_ERROR"),
                Content::text(format!(
                    "HTTP request failed with status: {}",
                    response.status()
                )),
            ]));
        }

        let html_bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(e) => {
                error!("Failed to read response body: {}", e);
                return Ok(CallToolResult::error(vec![
                    Content::text("RESPONSE_READ_ERROR"),
                    Content::text(format!("Failed to read response body: {}", e)),
                ]));
            }
        };

        let html_content = String::from_utf8_lossy(&html_bytes).to_string();
        let markdown_content = html2md::rewrite_html(&html_content, false);
        let sanitized_content = sanitize_text_output(&markdown_content);

        let result = match handle_large_output(&sanitized_content, "webpage", 300, false) {
            Ok(result) => result,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("OUTPUT_HANDLING_ERROR"),
                    Content::text(format!("Failed to handle output: {}", e)),
                ]));
            }
        };

        let formatted_output = format!("# Web Page Content: {}\n\n{}", url, result);

        Ok(CallToolResult::success(vec![Content::text(
            &formatted_output,
        )]))
    }

    #[tool(
        description = "Remove/delete a local or remote file or directory. Files are automatically backed up before removal and can be recovered.

REMOTE FILE REMOVAL:
- Supports SSH connections for remote file operations
- Use format: 'user@host:/path' or 'user@host#port:/path'
- IMPORTANT: Use ABSOLUTE paths for remote files (e.g., '/tmp/file.txt' not 'file.txt')
- Use 'password' for password authentication or 'private_key_path' for key-based auth
- Automatic SSH key discovery from ~/.ssh/ if no credentials provided
- Examples:
  * 'user@server.com:/tmp/old-file.txt' - Remove remote file
  * 'user@server.com#2222:/var/log/old-logs/' - Remove remote directory (with recursive=true)
  * '/local/path/file.txt' - Remove local file (default behavior)

DIRECTORY REMOVAL:
- Use 'recursive=true' to remove directories and their contents
- Files can be removed without the recursive flag

BACKUP & RECOVERY:
- ALL removed files and directories are automatically backed up before deletion
- Local files: Moved to '.stakpak/session/backups/{uuid}/' on the local machine
- Remote files: Moved to '.stakpak/session/backups/{uuid}/' on the remote machine
- Backup paths are returned in XML format showing original and backup locations
- Files are moved (not copied) to backup location, making removal efficient
- Both files and entire directories can be recovered from backup locations

SAFETY NOTES:
- Files are moved to backup location (not permanently deleted)
- Backup locations are preserved until manually cleaned up
- Use backup paths from XML output to restore files if needed"
    )]
    pub async fn remove(
        &self,
        _ctx: RequestContext<RoleServer>,
        Parameters(RemoveRequest {
            path,
            recursive,
            password,
            private_key_path,
        }): Parameters<RemoveRequest>,
    ) -> Result<CallToolResult, McpError> {
        let recursive = recursive.unwrap_or(false);

        if Self::is_remote_path(&path) {
            match self
                .get_remote_connection(&path, password, private_key_path)
                .await
            {
                Ok((conn, remote_path)) => {
                    self.remove_remote_path(&conn, &remote_path, &path, recursive)
                        .await
                }
                Err(error_result) => Ok(error_result),
            }
        } else {
            self.remove_local_path(&path, recursive).await
        }
    }

    /// Get remote connection for a path, handling authentication
    async fn get_remote_connection(
        &self,
        path: &str,
        password: Option<String>,
        private_key_path: Option<String>,
    ) -> Result<(Arc<RemoteConnection>, String), CallToolResult> {
        let path_location = PathLocation::parse(path).map_err(|e| {
            CallToolResult::error(vec![
                Content::text("INVALID_PATH"),
                Content::text(format!("Failed to parse path: {}", e)),
            ])
        })?;

        match path_location {
            PathLocation::Remote {
                mut connection,
                path: remote_path,
            } => {
                // Override connection details if provided
                if let Some(pwd) = password {
                    connection.password = Some(pwd);
                }
                if let Some(key_path) = private_key_path {
                    connection.private_key_path = Some(key_path);
                }

                let connection_manager = self.get_remote_connection_manager();
                let conn = connection_manager
                    .get_connection(&connection)
                    .await
                    .map_err(|e| {
                        error!("Failed to establish remote connection: {}", e);
                        CallToolResult::error(vec![
                            Content::text("REMOTE_CONNECTION_ERROR"),
                            Content::text(format!("Failed to connect to remote host: {}", e)),
                        ])
                    })?;

                Ok((conn, remote_path))
            }
            PathLocation::Local(_) => Err(CallToolResult::error(vec![
                Content::text("NOT_REMOTE"),
                Content::text("This helper is for remote connections only"),
            ])),
        }
    }

    /// Check if a path is remote
    fn is_remote_path(path: &str) -> bool {
        PathLocation::parse(path)
            .map(|loc| loc.is_remote())
            .unwrap_or(false)
    }

    /// Execute command either locally or remotely based on parameters
    async fn execute_command_unified(
        &self,
        command: &str,
        timeout: Option<u64>,
        remote: Option<String>,
        password: Option<String>,
        private_key_path: Option<String>,
        ctx: &RequestContext<RoleServer>,
    ) -> Result<CommandResult, CallToolResult> {
        if let Some(remote_str) = &remote {
            // Remote execution
            let connection_info = RemoteConnectionInfo {
                connection_string: remote_str.clone(),
                password: password.clone(),
                private_key_path: private_key_path.clone(),
            };

            let connection_manager = self.get_remote_connection_manager();
            let connection = connection_manager
                .get_connection(&connection_info)
                .await
                .map_err(|e| {
                    error!("Failed to establish remote connection: {}", e);
                    CallToolResult::error(vec![
                        Content::text("REMOTE_CONNECTION_ERROR"),
                        Content::text(format!("Failed to connect to remote host: {}", e)),
                    ])
                })?;

            let timeout_duration = timeout.map(std::time::Duration::from_secs);
            let (output, exit_code) = connection
                .execute_command(command, timeout_duration, Some(ctx))
                .await
                .map_err(|e| {
                    error!("Failed to execute remote command: {}", e);
                    CallToolResult::error(vec![
                        Content::text("REMOTE_COMMAND_ERROR"),
                        Content::text(format!("Failed to execute remote command: {}", e)),
                    ])
                })?;

            let mut result = output;
            if exit_code != 0 {
                result.push_str(&format!("\nCommand exited with code {}", exit_code));
            }

            Ok(CommandResult {
                output: result,
                exit_code,
            })
        } else {
            // Local execution - existing logic
            self.execute_local_command(command, timeout, ctx).await
        }
    }

    /// Execute local command with existing logic extracted to avoid duplication
    async fn execute_local_command(
        &self,
        actual_command: &str,
        timeout: Option<u64>,
        ctx: &RequestContext<RoleServer>,
    ) -> Result<CommandResult, CallToolResult> {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(actual_command)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(unix)]
        {
            cmd.env("DEBIAN_FRONTEND", "noninteractive")
                .env("SUDO_ASKPASS", "/bin/false")
                .process_group(0);
        }
        #[cfg(windows)]
        {
            // On Windows, create a new process group
            cmd.creation_flags(0x00000200); // CREATE_NEW_PROCESS_GROUP
        }

        let mut child = cmd.spawn().map_err(|e| {
            error!("Failed to run command: {}", e);
            CallToolResult::error(vec![
                Content::text("COMMAND_ERROR"),
                Content::text(format!("Failed to run command: {}", e)),
            ])
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

        // Stall detection: track last output time and stall start time for incrementing counter
        let mut last_output_time = std::time::Instant::now();
        let mut stall_start_time: Option<std::time::Instant> = None;
        const STALL_TIMEOUT_SECS: u64 = 5;

        // Helper function to stream output and wait for process completion
        let stream_and_wait = async {
            // Stall check interval
            let mut stall_check_interval = tokio::time::interval(Duration::from_secs(1));
            stall_check_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            macro_rules! handle_output {
                ($read_result:expr, $buf:expr) => {
                    match $read_result {
                        Ok(Ok(0)) => break, // EOF
                        Ok(Ok(_)) => {
                            last_output_time = std::time::Instant::now();
                            stall_start_time = None; // Reset stall tracking on output
                            let line = $buf.trim_end_matches('\n').to_string();
                            $buf.clear();
                            result.push_str(&format!("{}\n", line));
                            let _ = ctx
                                .peer
                                .notify_progress(ProgressNotificationParam {
                                    progress_token: ProgressToken(NumberOrString::Number(0)),
                                    progress: 50.0,
                                    total: Some(100.0),
                                    message: Some(
                                        serde_json::to_string(&ToolCallResultProgress {
                                            id: progress_id,
                                            message: format!("{}\n", line),
                                            progress_type: Some(ProgressType::CommandOutput),
                                            task_updates: None,
                                            progress: None,
                                        })
                                        .unwrap_or_default(),
                                    ),
                                })
                                .await;
                        }
                        Ok(Err(_)) => break, // Read error
                        Err(_) => {}         // Timeout - continue loop
                    }
                };
            }

            // Read from both streams concurrently
            loop {
                // Use biased selection so interval gets priority
                tokio::select! {
                    biased;

                    _ = stall_check_interval.tick() => {
                        // Check for stall condition: no output for 5 seconds
                        let elapsed_since_output = last_output_time.elapsed().as_secs();
                        if elapsed_since_output >= STALL_TIMEOUT_SECS {
                            // Initialize stall start time on first detection
                            if stall_start_time.is_none() {
                                stall_start_time = Some(std::time::Instant::now());
                            }

                            // Calculate running time (stall duration + initial 5s threshold)
                            let stall_duration = stall_start_time.map(|t| t.elapsed().as_secs()).unwrap_or(0);
                            let running_secs = STALL_TIMEOUT_SECS + stall_duration;

                            // Send stall notification with incrementing counter
                            let stall_msg = format!("__INTERACTIVE_STALL__: Running for {}s . ctrl+r to re-run in shell mode", running_secs);
                            let _ = ctx.peer.notify_progress(ProgressNotificationParam {
                                progress_token: ProgressToken(NumberOrString::Number(0)),
                                progress: 50.0,
                                total: Some(100.0),
                                message: Some(serde_json::to_string(&ToolCallResultProgress {
                                    id: progress_id,
                                    message: stall_msg,
                                    progress_type: Some(ProgressType::CommandOutput),
                                    task_updates: None,
                                    progress: None,
                                }).unwrap_or_default()),
                            }).await;
                        }
                    }

                    read_result = tokio::time::timeout(Duration::from_millis(100), stderr_reader.read_line(&mut stderr_buf)) => {
                        handle_output!(read_result, stderr_buf);
                    }

                    read_result = tokio::time::timeout(Duration::from_millis(100), stdout_reader.read_line(&mut stdout_buf)) => {
                        handle_output!(read_result, stdout_buf);
                    }
                }

                // Check if process has exited
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
            }

            // Wait for the process to complete
            child.wait().await
        };

        // Execute with timeout and cancellation support
        let execution_result = if let Some(timeout_secs) = timeout {
            let timeout_duration = std::time::Duration::from_secs(timeout_secs);

            tokio::select! {
                result = tokio::time::timeout(timeout_duration, stream_and_wait) => result,
                _ = ctx.ct.cancelled() => {
                    // Cancellation occurred, kill the process
                    let _ = child.kill().await;
                    return Err(CallToolResult::cancel(Some(&vec![
                        Content::text("COMMAND_CANCELLED"),
                        Content::text("Command execution was cancelled"),
                    ])));
                }
            }
        } else {
            tokio::select! {
                result = stream_and_wait => Ok(result),
                _ = ctx.ct.cancelled() => {
                    let _ = child.kill().await;
                    return Err(CallToolResult::cancel(Some(&vec![
                        Content::text("COMMAND_CANCELLED"),
                        Content::text("Command execution was cancelled"),
                    ])));
                }
            }
        };

        let exit_code = match execution_result {
            Ok(Ok(exit_status)) => exit_status.code().unwrap_or(-1),
            Ok(Err(e)) => {
                return Err(CallToolResult::error(vec![
                    Content::text("COMMAND_ERROR"),
                    Content::text(format!("Failed to wait for command: {}", e)),
                ]));
            }
            Err(_) => {
                // Timeout occurred, kill the process
                let _ = child.kill().await;
                result.push_str(&format!(
                    "Command timed out after {} seconds\n",
                    timeout.unwrap_or_default()
                ));
                -1
            }
        };

        if exit_code != 0 {
            result.push_str(&format!("Command exited with code {}\n", exit_code));
        }

        Ok(CommandResult {
            output: result,
            exit_code,
        })
    }

    /// View the contents of a local file or directory
    async fn view_local_path(
        &self,
        path: &str,
        opts: &ViewOptions<'_>,
    ) -> Result<CallToolResult, McpError> {
        let path_obj = Path::new(path);

        if !path_obj.exists() {
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_NOT_FOUND"),
                Content::text(format!("File or directory not found: {}", path)),
            ]));
        }

        if path_obj.is_dir() {
            // Handle combined glob + grep: filter files by glob, then search content
            if let (Some(glob_pattern), Some(grep_pattern)) = (opts.glob, opts.grep) {
                return self
                    .grep_local_directory_with_glob(
                        path,
                        grep_pattern,
                        glob_pattern,
                        opts.max_lines,
                    )
                    .await;
            }

            // Handle glob pattern filtering for directories (list files only)
            if let Some(glob_pattern) = opts.glob {
                return self
                    .view_local_dir_with_glob(path, glob_pattern, opts.max_lines)
                    .await;
            }

            // Handle grep search in directory (all files)
            if let Some(grep_pattern) = opts.grep {
                return self
                    .grep_local_directory(path, grep_pattern, opts.max_lines)
                    .await;
            }

            // Default directory tree view
            let depth = if opts.tree.unwrap_or(false) { 3 } else { 1 };
            let provider = LocalFileSystemProvider;
            let path_str = path_obj.to_string_lossy();

            match generate_directory_tree(&provider, &path_str, "", depth, 0).await {
                Ok(tree_content) => {
                    let result = format!(
                        "Directory tree for \"{}\":\n{}\n{}",
                        path,
                        path_obj
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| path.to_string()),
                        tree_content
                    );
                    Ok(CallToolResult::success(vec![Content::text(result)]))
                }
                Err(e) => Ok(CallToolResult::error(vec![
                    Content::text("READ_ERROR"),
                    Content::text(format!("Cannot read directory: {}", e)),
                ])),
            }
        } else {
            // Handle grep search in single file
            if let Some(grep_pattern) = opts.grep {
                return self.grep_local_file(path, grep_pattern, opts.max_lines);
            }

            // Read file contents
            match fs::read_to_string(path) {
                Ok(content) => {
                    let result = match self.format_file_content(
                        &content,
                        path,
                        opts.view_range,
                        opts.max_lines,
                        "File",
                    ) {
                        Ok(result) => result,
                        Err(e) => {
                            return Ok(CallToolResult::error(vec![
                                Content::text("FORMAT_ERROR"),
                                Content::text(format!("Failed to format file content: {}", e)),
                            ]));
                        }
                    };

                    Ok(CallToolResult::success(vec![Content::text(&result)]))
                }
                Err(e) => Ok(CallToolResult::error(vec![
                    Content::text("READ_ERROR"),
                    Content::text(format!("Cannot read file: {}", e)),
                ])),
            }
        }
    }

    /// View directory contents filtered by glob pattern
    async fn view_local_dir_with_glob(
        &self,
        path: &str,
        glob_pattern: &str,
        max_lines: usize,
    ) -> Result<CallToolResult, McpError> {
        // Build the glob matcher
        let glob = match Glob::new(glob_pattern) {
            Ok(g) => g.compile_matcher(),
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("INVALID_GLOB"),
                    Content::text(format!("Invalid glob pattern '{}': {}", glob_pattern, e)),
                ]));
            }
        };

        // Use ignore crate's WalkBuilder for gitignore-aware traversal
        let walker = WalkBuilder::new(path)
            .hidden(false) // Show hidden files
            .git_ignore(true) // Respect .gitignore
            .build();

        let mut matches: Vec<String> = Vec::new();
        let base_path = Path::new(path);

        for entry in walker.flatten() {
            let entry_path = entry.path();

            // Get relative path for glob matching
            let relative = match entry_path.strip_prefix(base_path) {
                Ok(r) => r.to_string_lossy().to_string(),
                Err(_) => continue,
            };

            // Skip the root directory itself
            if relative.is_empty() {
                continue;
            }

            // Check if the path matches the glob pattern
            if glob.is_match(&relative) || glob.is_match(entry_path.file_name().unwrap_or_default())
            {
                let prefix = if entry_path.is_dir() {
                    "📁 "
                } else {
                    "📄 "
                };
                matches.push(format!("{}{}", prefix, relative));
            }
        }

        if matches.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No files matching '{}' found in {}",
                glob_pattern, path
            ))]));
        }

        // Sort and truncate
        matches.sort();
        let total = matches.len();
        let truncated = matches.len() > max_lines;
        if truncated {
            matches.truncate(max_lines);
        }

        let mut result = format!(
            "Files matching '{}' in \"{}\" ({} matches):\n\n{}",
            glob_pattern,
            path,
            total,
            matches.join("\n")
        );

        if truncated {
            result.push_str(&format!("\n\n... and {} more files", total - max_lines));
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Grep search in a single local file
    fn grep_local_file(
        &self,
        path: &str,
        pattern: &str,
        max_lines: usize,
    ) -> Result<CallToolResult, McpError> {
        let matcher = match RegexMatcher::new(pattern) {
            Ok(m) => m,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("INVALID_REGEX"),
                    Content::text(format!("Invalid regex pattern '{}': {}", pattern, e)),
                ]));
            }
        };

        let mut matches: Vec<String> = Vec::new();
        let mut searcher = Searcher::new();

        let sink_result = searcher.search_path(
            &matcher,
            path,
            UTF8(|line_num, line| {
                if matches.len() < max_lines {
                    matches.push(format!("{}:{}", line_num, line.trim_end()));
                }
                Ok(true)
            }),
        );

        if let Err(e) = sink_result {
            return Ok(CallToolResult::error(vec![
                Content::text("GREP_ERROR"),
                Content::text(format!("Error searching file: {}", e)),
            ]));
        }

        if matches.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No matches for '{}' in {}",
                pattern, path
            ))]));
        }

        let total = matches.len();
        let result = format!(
            "Grep results for '{}' in \"{}\" ({} matches in 1 file):\n\n{}",
            pattern,
            path,
            total,
            matches.join("\n")
        );

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Grep search across a directory (recursive, respects .gitignore)
    async fn grep_local_directory(
        &self,
        path: &str,
        pattern: &str,
        max_lines: usize,
    ) -> Result<CallToolResult, McpError> {
        let matcher = match RegexMatcher::new(pattern) {
            Ok(m) => m,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("INVALID_REGEX"),
                    Content::text(format!("Invalid regex pattern '{}': {}", pattern, e)),
                ]));
            }
        };

        // Use ignore crate for gitignore-aware traversal
        let walker = WalkBuilder::new(path)
            .hidden(false)
            .git_ignore(true)
            .build();

        let mut all_matches: Vec<String> = Vec::new();
        let mut files_with_matches = 0;
        let base_path = Path::new(path);

        for entry in walker.flatten() {
            if all_matches.len() >= max_lines {
                break;
            }

            let entry_path = entry.path();
            if !entry_path.is_file() {
                continue;
            }

            let relative = entry_path
                .strip_prefix(base_path)
                .map(|r| r.to_string_lossy().to_string())
                .unwrap_or_else(|_| entry_path.to_string_lossy().to_string());

            let mut file_matches: Vec<String> = Vec::new();
            let mut searcher = Searcher::new();

            let _ = searcher.search_path(
                &matcher,
                entry_path,
                UTF8(|line_num, line| {
                    if all_matches.len() + file_matches.len() < max_lines {
                        file_matches.push(format!("{}:{}:{}", relative, line_num, line.trim_end()));
                    }
                    Ok(true)
                }),
            );

            if !file_matches.is_empty() {
                files_with_matches += 1;
                all_matches.extend(file_matches);
            }
        }

        if all_matches.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No matches for '{}' in {}",
                pattern, path
            ))]));
        }

        let truncated = all_matches.len() >= max_lines;
        let result = format!(
            "Grep results for '{}' in \"{}\" ({} matches in {} files):\n\n{}{}",
            pattern,
            path,
            all_matches.len(),
            files_with_matches,
            all_matches.join("\n"),
            if truncated { "\n\n... (truncated)" } else { "" }
        );

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Grep search across a directory filtered by glob pattern
    async fn grep_local_directory_with_glob(
        &self,
        path: &str,
        pattern: &str,
        glob_pattern: &str,
        max_lines: usize,
    ) -> Result<CallToolResult, McpError> {
        let matcher = match RegexMatcher::new(pattern) {
            Ok(m) => m,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("INVALID_REGEX"),
                    Content::text(format!("Invalid regex pattern '{}': {}", pattern, e)),
                ]));
            }
        };

        let glob = match Glob::new(glob_pattern) {
            Ok(g) => g.compile_matcher(),
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("INVALID_GLOB"),
                    Content::text(format!("Invalid glob pattern '{}': {}", glob_pattern, e)),
                ]));
            }
        };

        // Use ignore crate for gitignore-aware traversal
        let walker = WalkBuilder::new(path)
            .hidden(false)
            .git_ignore(true)
            .build();

        let mut all_matches: Vec<String> = Vec::new();
        let mut files_with_matches = 0;
        let base_path = Path::new(path);

        for entry in walker.flatten() {
            if all_matches.len() >= max_lines {
                break;
            }

            let entry_path = entry.path();
            if !entry_path.is_file() {
                continue;
            }

            // Check if file matches glob pattern
            let relative = entry_path
                .strip_prefix(base_path)
                .map(|r| r.to_string_lossy().to_string())
                .unwrap_or_else(|_| entry_path.to_string_lossy().to_string());

            let matches_glob = glob.is_match(&relative)
                || glob.is_match(entry_path.file_name().unwrap_or_default());

            if !matches_glob {
                continue;
            }

            let mut file_matches: Vec<String> = Vec::new();
            let mut searcher = Searcher::new();

            let _ = searcher.search_path(
                &matcher,
                entry_path,
                UTF8(|line_num, line| {
                    if all_matches.len() + file_matches.len() < max_lines {
                        file_matches.push(format!("{}:{}:{}", relative, line_num, line.trim_end()));
                    }
                    Ok(true)
                }),
            );

            if !file_matches.is_empty() {
                files_with_matches += 1;
                all_matches.extend(file_matches);
            }
        }

        if all_matches.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No matches for '{}' in {} (filtered by glob '{}')",
                pattern, path, glob_pattern
            ))]));
        }

        let truncated = all_matches.len() >= max_lines;
        let result = format!(
            "Grep results for '{}' in \"{}\" (glob: '{}') ({} matches in {} files):\n\n{}{}",
            pattern,
            path,
            glob_pattern,
            all_matches.len(),
            files_with_matches,
            all_matches.join("\n"),
            if truncated { "\n\n... (truncated)" } else { "" }
        );

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// View the contents of a remote file or directory
    async fn view_remote_path(
        &self,
        conn: &Arc<RemoteConnection>,
        remote_path: &str,
        original_path: &str,
        opts: &ViewOptions<'_>,
    ) -> Result<CallToolResult, McpError> {
        if !conn.exists(remote_path).await {
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_NOT_FOUND"),
                Content::text(format!(
                    "Remote file or directory not found: {}",
                    original_path
                )),
            ]));
        }

        if conn.is_directory(remote_path).await {
            // Handle combined glob + grep for remote directories
            if let (Some(glob_pattern), Some(grep_pattern)) = (opts.glob, opts.grep) {
                return self
                    .grep_remote_directory_with_glob(
                        conn,
                        remote_path,
                        original_path,
                        grep_pattern,
                        glob_pattern,
                        opts.max_lines,
                    )
                    .await;
            }

            // Handle glob pattern filtering for remote directories
            if let Some(glob_pattern) = opts.glob {
                return self
                    .view_remote_dir_with_glob(
                        conn,
                        remote_path,
                        original_path,
                        glob_pattern,
                        opts.max_lines,
                    )
                    .await;
            }

            // Handle grep search in remote directory
            if let Some(grep_pattern) = opts.grep {
                return self
                    .grep_remote_directory(
                        conn,
                        remote_path,
                        original_path,
                        grep_pattern,
                        opts.max_lines,
                    )
                    .await;
            }

            // Default directory tree view
            let depth = if opts.tree.unwrap_or(false) { 3 } else { 1 };
            let provider = RemoteFileSystemProvider::new(conn.clone());

            match generate_directory_tree(&provider, remote_path, "", depth, 0).await {
                Ok(tree_content) => {
                    let result = format!(
                        "Remote directory tree for \"{}\":\n{}\n{}",
                        original_path,
                        remote_path.split('/').next_back().unwrap_or(remote_path),
                        tree_content
                    );
                    Ok(CallToolResult::success(vec![Content::text(result)]))
                }
                Err(e) => Ok(CallToolResult::error(vec![
                    Content::text("READ_ERROR"),
                    Content::text(format!("Cannot read remote directory: {}", e)),
                ])),
            }
        } else {
            // Handle grep search in single remote file
            if let Some(grep_pattern) = opts.grep {
                return self
                    .grep_remote_file(
                        conn,
                        remote_path,
                        original_path,
                        grep_pattern,
                        opts.max_lines,
                    )
                    .await;
            }

            // Read remote file contents
            match conn.read_file_to_string(remote_path).await {
                Ok(content) => {
                    let result = match self.format_file_content(
                        &content,
                        original_path,
                        opts.view_range,
                        opts.max_lines,
                        "Remote file",
                    ) {
                        Ok(result) => result,
                        Err(e) => {
                            return Ok(CallToolResult::error(vec![
                                Content::text("FORMAT_ERROR"),
                                Content::text(format!(
                                    "Failed to format remote file content: {}",
                                    e
                                )),
                            ]));
                        }
                    };

                    Ok(CallToolResult::success(vec![Content::text(&result)]))
                }
                Err(e) => Ok(CallToolResult::error(vec![
                    Content::text("READ_ERROR"),
                    Content::text(format!("Cannot read remote file: {}", e)),
                ])),
            }
        }
    }

    /// View remote directory contents filtered by glob pattern using find command
    async fn view_remote_dir_with_glob(
        &self,
        conn: &Arc<RemoteConnection>,
        remote_path: &str,
        original_path: &str,
        glob_pattern: &str,
        max_lines: usize,
    ) -> Result<CallToolResult, McpError> {
        // Escape for double quotes (the command will be wrapped in bash -c '...' which uses double quotes internally)
        let escaped_pattern = glob_pattern
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('`', "\\`");

        // Use double quotes for pattern since single quotes conflict with bash -c wrapper
        let command = format!(
            "find {} -name \"{}\" 2>/dev/null | head -n {}",
            remote_path,
            escaped_pattern,
            max_lines + 1 // +1 to detect truncation
        );

        match conn.execute_command(&command, None, None).await {
            Ok((output, exit_code)) => {
                if exit_code != 0 && output.trim().is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "No files matching '{}' found in {}",
                        glob_pattern, original_path
                    ))]));
                }

                let lines: Vec<&str> = output.lines().collect();
                let truncated = lines.len() > max_lines;
                let display_lines: Vec<&str> = lines.into_iter().take(max_lines).collect();

                if display_lines.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "No files matching '{}' found in {}",
                        glob_pattern, original_path
                    ))]));
                }

                let mut result = format!(
                    "Remote files matching '{}' in \"{}\":\n\n{}",
                    glob_pattern,
                    original_path,
                    display_lines.join("\n")
                );

                if truncated {
                    result.push_str("\n\n... (truncated)");
                }

                Ok(CallToolResult::success(vec![Content::text(result)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![
                Content::text("REMOTE_GLOB_ERROR"),
                Content::text(format!("Failed to search remote directory: {}", e)),
            ])),
        }
    }

    /// Grep search in a single remote file
    async fn grep_remote_file(
        &self,
        conn: &Arc<RemoteConnection>,
        remote_path: &str,
        original_path: &str,
        pattern: &str,
        max_lines: usize,
    ) -> Result<CallToolResult, McpError> {
        // Escape for double quotes (the command will be wrapped in bash -c '...' which uses double quotes internally)
        // Need to escape: \ " $ `
        let escaped_pattern = pattern
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('`', "\\`");

        // Use grep -E for extended regex (supports |, +, ?, etc.)
        // Use double quotes for pattern since single quotes conflict with bash -c wrapper
        let command = format!(
            "grep -En \"{}\" {} 2>/dev/null | head -n {}",
            escaped_pattern,
            remote_path,
            max_lines + 1
        );

        match conn.execute_command(&command, None, None).await {
            Ok((output, _exit_code)) => {
                // grep returns exit code 1 for no matches, which is fine
                let lines: Vec<&str> = output.lines().collect();

                if lines.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "No matches for '{}' in {}",
                        pattern, original_path
                    ))]));
                }

                let truncated = lines.len() > max_lines;
                let display_lines: Vec<&str> = lines.into_iter().take(max_lines).collect();

                let mut result = format!(
                    "Grep results for '{}' in \"{}\" ({} matches in 1 file):\n\n{}",
                    pattern,
                    original_path,
                    display_lines.len(),
                    display_lines.join("\n")
                );

                if truncated {
                    result.push_str("\n\n... (truncated)");
                }

                Ok(CallToolResult::success(vec![Content::text(result)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![
                Content::text("REMOTE_GREP_ERROR"),
                Content::text(format!("Failed to grep remote file: {}", e)),
            ])),
        }
    }

    /// Grep search across a remote directory using grep -rE
    async fn grep_remote_directory(
        &self,
        conn: &Arc<RemoteConnection>,
        remote_path: &str,
        original_path: &str,
        pattern: &str,
        max_lines: usize,
    ) -> Result<CallToolResult, McpError> {
        // Escape for double quotes (the command will be wrapped in bash -c '...' which uses double quotes internally)
        let escaped_pattern = pattern
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('`', "\\`");

        // Use grep -rEn for recursive search with extended regex and line numbers
        // Use double quotes for pattern since single quotes conflict with bash -c wrapper
        let command = format!(
            "grep -rEn --include=\"*\" \"{}\" {} 2>/dev/null | head -n {}",
            escaped_pattern,
            remote_path,
            max_lines + 1
        );

        match conn.execute_command(&command, None, None).await {
            Ok((output, _exit_code)) => {
                let lines: Vec<&str> = output.lines().collect();

                if lines.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "No matches for '{}' in {}",
                        pattern, original_path
                    ))]));
                }

                let truncated = lines.len() > max_lines;
                let display_lines: Vec<&str> = lines.into_iter().take(max_lines).collect();

                // Count unique files
                let files_with_matches: std::collections::HashSet<&str> = display_lines
                    .iter()
                    .filter_map(|line| line.split(':').next())
                    .collect();

                let mut result = format!(
                    "Grep results for '{}' in \"{}\" ({} matches in {} files):\n\n{}",
                    pattern,
                    original_path,
                    display_lines.len(),
                    files_with_matches.len(),
                    display_lines.join("\n")
                );

                if truncated {
                    result.push_str("\n\n... (truncated)");
                }

                Ok(CallToolResult::success(vec![Content::text(result)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![
                Content::text("REMOTE_GREP_ERROR"),
                Content::text(format!("Failed to grep remote directory: {}", e)),
            ])),
        }
    }

    /// Grep search across a remote directory filtered by glob pattern
    async fn grep_remote_directory_with_glob(
        &self,
        conn: &Arc<RemoteConnection>,
        remote_path: &str,
        original_path: &str,
        pattern: &str,
        glob_pattern: &str,
        max_lines: usize,
    ) -> Result<CallToolResult, McpError> {
        // Escape for double quotes (the command will be wrapped in bash -c '...' which uses double quotes internally)
        let escaped_pattern = pattern
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('`', "\\`");
        let escaped_glob = glob_pattern
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('`', "\\`");

        // Use find with -name to filter files, then grep -E for extended regex
        // Use double quotes for patterns since single quotes conflict with bash -c wrapper
        let command = format!(
            "find {} -name \"{}\" -type f -exec grep -EHn \"{}\" {{}} \\; 2>/dev/null | head -n {}",
            remote_path,
            escaped_glob,
            escaped_pattern,
            max_lines + 1
        );

        match conn.execute_command(&command, None, None).await {
            Ok((output, _exit_code)) => {
                let lines: Vec<&str> = output.lines().collect();

                if lines.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "No matches for '{}' in {} (filtered by glob '{}')",
                        pattern, original_path, glob_pattern
                    ))]));
                }

                let truncated = lines.len() > max_lines;
                let display_lines: Vec<&str> = lines.into_iter().take(max_lines).collect();

                // Count unique files
                let files_with_matches: std::collections::HashSet<&str> = display_lines
                    .iter()
                    .filter_map(|line| line.split(':').next())
                    .collect();

                let mut result = format!(
                    "Grep results for '{}' in \"{}\" (glob: '{}') ({} matches in {} files):\n\n{}",
                    pattern,
                    original_path,
                    glob_pattern,
                    display_lines.len(),
                    files_with_matches.len(),
                    display_lines.join("\n")
                );

                if truncated {
                    result.push_str("\n\n... (truncated)");
                }

                Ok(CallToolResult::success(vec![Content::text(result)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![
                Content::text("REMOTE_GREP_ERROR"),
                Content::text(format!("Failed to grep remote directory: {}", e)),
            ])),
        }
    }

    /// Format file content with line numbers and truncation - shared logic
    fn format_file_content(
        &self,
        content: &str,
        path: &str,
        view_range: Option<[i32; 2]>,
        max_lines: usize,
        prefix: &str,
    ) -> Result<String, McpError> {
        let result = if let Some([start, end]) = view_range {
            let lines: Vec<&str> = content.lines().collect();
            let start_idx = if start <= 0 { 0 } else { (start - 1) as usize };
            let end_idx = if end == -1 {
                lines.len()
            } else {
                std::cmp::min(end as usize, lines.len())
            };

            if start_idx >= lines.len() {
                return Err(McpError::internal_error(
                    "Invalid range",
                    Some(json!({
                        "error": format!("Start line {} is beyond file length {}", start, lines.len())
                    })),
                ));
            }

            let selected_lines = &lines[start_idx..end_idx];
            if selected_lines.len() <= max_lines {
                format!(
                    "{}: {} (lines {}-{})\n{}",
                    prefix,
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
                let selected_lines = selected_lines.iter().take(max_lines).collect::<Vec<_>>();
                format!(
                    "{}: {} (showing lines {}-{}, only the first {} lines of your view range)\n{}\n...",
                    prefix,
                    path,
                    start_idx + 1,
                    start_idx + 1 + max_lines,
                    max_lines,
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
            if lines.len() <= max_lines {
                format!(
                    "{}: {} ({} lines)\n{}",
                    prefix,
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
                let selected_lines = lines.iter().take(max_lines).collect::<Vec<_>>();
                format!(
                    "{}: {} (showing {} / {} lines)\n{}\n...",
                    prefix,
                    path,
                    max_lines,
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

        Ok(result)
    }

    fn create_unified_diff(
        &self,
        original: &str,
        modified: &str,
        from_file: &str,
        to_file: &str,
    ) -> String {
        let text_diff = TextDiff::from_lines(original, modified);
        format!(
            "{}",
            text_diff
                .unified_diff()
                .context_radius(0)
                .header(from_file, to_file)
        )
    }

    /// Replace a specific string in a remote file
    async fn str_replace_remote(
        &self,
        conn: &Arc<RemoteConnection>,
        remote_path: &str,
        original_path: &str,
        old_str: &str,
        new_str: &str,
        replace_all: Option<bool>,
    ) -> Result<CallToolResult, McpError> {
        if old_str == new_str {
            return Ok(CallToolResult::error(vec![
                Content::text("OLD_STR_NEW_STR_IDENTICAL"),
                Content::text(
                    "The old string and new string are identical - no replacement needed",
                ),
            ]));
        }

        let content = match conn.read_file_to_string(remote_path).await {
            Ok(content) => content,
            Err(e) => {
                error!("Failed to read remote file for str_replace: {}", e);
                return Ok(CallToolResult::error(vec![
                    Content::text("REMOTE_FILE_READ_ERROR"),
                    Content::text(format!("Failed to read remote file: {}", e)),
                ]));
            }
        };

        if !content.contains(old_str) {
            return Ok(CallToolResult::error(vec![
                Content::text("STRING_NOT_FOUND"),
                Content::text("The string old_str was not found in the file"),
            ]));
        }

        let new_content = if replace_all.unwrap_or(false) {
            content.replace(old_str, new_str)
        } else {
            content.replacen(old_str, new_str, 1)
        };

        let replaced_count = if replace_all.unwrap_or(false) {
            content.matches(old_str).count()
        } else if content.contains(old_str) {
            1
        } else {
            0
        };

        if let Err(e) = conn.write_file(remote_path, new_content.as_bytes()).await {
            error!("Failed to write remote file for str_replace: {}", e);
            return Ok(CallToolResult::error(vec![
                Content::text("REMOTE_FILE_WRITE_ERROR"),
                Content::text(format!("Failed to write remote file: {}", e)),
            ]));
        }

        let unified_diff =
            self.create_unified_diff(&content, &new_content, original_path, original_path);

        let output = format!(
            "Successfully replaced {} occurrences of text (remote)\n\n```diff\n{}\n```",
            replaced_count, unified_diff
        );

        Ok(CallToolResult::success(vec![Content::text(&output)]))
    }

    /// Replace a specific string in a local file
    async fn str_replace_local(
        &self,
        path: &str,
        old_str: &str,
        new_str: &str,
        replace_all: Option<bool>,
    ) -> Result<CallToolResult, McpError> {
        if old_str == new_str {
            return Ok(CallToolResult::error(vec![
                Content::text("OLD_STR_NEW_STR_IDENTICAL"),
                Content::text(
                    "The old string and new string are identical - no replacement needed",
                ),
            ]));
        }

        let original_content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) => {
                error!("Failed to read local file for str_replace: {}", e);
                return Ok(CallToolResult::error(vec![
                    Content::text("FILE_READ_ERROR"),
                    Content::text(format!("Failed to read local file: {}", e)),
                ]));
            }
        };

        // Try exact match first, then fall back to Unicode-normalized matching.
        // LLMs commonly normalize curly quotes to straight quotes, en-dashes to
        // hyphens, etc. The fallback finds the original substring in the file by
        // normalizing both sides to ASCII and using char-position mapping.
        let (new_content, replaced_count) = if original_content.contains(old_str) {
            // Exact match — fast path.  Use `replacen` for single or
            // `replace` for all, and derive the count from the result to
            // avoid scanning the string twice.
            if replace_all.unwrap_or(false) {
                let result = original_content.replace(old_str, new_str);
                // Derive count from the length difference.
                let old_len = old_str.len();
                let new_len = new_str.len();
                let count = if old_len == new_len {
                    // Length-neutral replacement — count via matches (unavoidable).
                    original_content.matches(old_str).count()
                } else {
                    let orig = original_content.len();
                    let after = result.len();
                    // diff = count * (new_len - old_len), signed arithmetic
                    let diff = after as isize - orig as isize;
                    let step = new_len as isize - old_len as isize;
                    (diff / step) as usize
                };
                (result, count)
            } else {
                (original_content.replacen(old_str, new_str, 1), 1)
            }
        } else if let Some(result) = unicode_normalized_replace(
            &original_content,
            old_str,
            new_str,
            replace_all.unwrap_or(false),
        ) {
            // Unicode-normalized fallback matched
            result
        } else {
            return Ok(CallToolResult::error(vec![
                Content::text("STRING_NOT_FOUND"),
                Content::text("The string old_str was not found in the file"),
            ]));
        };

        let unified_diff = self.create_unified_diff(&original_content, &new_content, path, path);

        if let Err(e) = fs::write(path, &new_content) {
            error!("Failed to write local file for str_replace: {}", e);
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_WRITE_ERROR"),
                Content::text(format!("Failed to write local file: {}", e)),
            ]));
        }

        let output = format!(
            "Successfully replaced {} occurrences of text (local)\n\n```diff\n{}\n```",
            replaced_count, unified_diff
        );

        Ok(CallToolResult::success(vec![Content::text(&output)]))
    }

    /// Create a remote file with the specified content
    async fn create_remote(
        &self,
        conn: &Arc<RemoteConnection>,
        remote_path: &str,
        original_path: &str,
        file_text: &str,
    ) -> Result<CallToolResult, McpError> {
        if conn.exists(remote_path).await {
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_EXISTS"),
                Content::text(format!("Remote file already exists: {}", original_path)),
            ]));
        }

        // Create parent directories if needed
        if let Some(parent) = std::path::Path::new(remote_path).parent() {
            let parent_str = parent.to_string_lossy().to_string();
            if !parent_str.is_empty()
                && !conn.exists(&parent_str).await
                && let Err(e) = conn.create_directories(&parent_str).await
            {
                error!(
                    "Failed to create remote parent directories '{}': {}",
                    parent_str, e
                );
                return Ok(CallToolResult::error(vec![
                    Content::text("CREATE_DIR_ERROR"),
                    Content::text(format!(
                        "Failed to create remote parent directories '{}': {}",
                        parent_str, e
                    )),
                ]));
            }
        }

        // Create the file using the correct SFTP method
        if let Err(e) = conn.create_file(remote_path, file_text.as_bytes()).await {
            error!("Failed to create remote file '{}': {}", remote_path, e);
            return Ok(CallToolResult::error(vec![
                Content::text("CREATE_ERROR"),
                Content::text(format!(
                    "Failed to create remote file '{}': {}",
                    remote_path, e
                )),
            ]));
        }

        let lines = file_text.lines().count();
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Successfully created remote file {} with {} lines",
            original_path, lines
        ))]))
    }

    /// Create a local file with the specified content
    fn create_local(&self, path: &str, file_text: &str) -> Result<CallToolResult, McpError> {
        let path_obj = Path::new(&path);

        if path_obj.exists() {
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_EXISTS"),
                Content::text(format!("File already exists: {}", path)),
            ]));
        }

        // Create parent directories if they don't exist
        if let Some(parent) = path_obj.parent()
            && !parent.exists()
            && let Err(e) = fs::create_dir_all(parent)
        {
            return Ok(CallToolResult::error(vec![
                Content::text("CREATE_DIR_ERROR"),
                Content::text(format!("Cannot create parent directories: {}", e)),
            ]));
        }

        match fs::write(path, file_text) {
            Ok(_) => {
                let lines = fs::read_to_string(path)
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

    /// Remove a remote file or directory using native SFTP operations where possible
    async fn remove_remote_path(
        &self,
        conn: &Arc<RemoteConnection>,
        remote_path: &str,
        original_path: &str,
        recursive: bool,
    ) -> Result<CallToolResult, McpError> {
        if !conn.exists(remote_path).await {
            return Ok(CallToolResult::error(vec![
                Content::text("PATH_NOT_FOUND"),
                Content::text(format!("Path does not exist: {}", original_path)),
            ]));
        }

        let is_directory = conn.is_directory(remote_path).await;

        let ssh_prefix = match conn.get_ssh_prefix() {
            Ok(prefix) => prefix,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("CONNECTION_ERROR"),
                    Content::text(format!("Failed to get SSH connection info: {}", e)),
                ]));
            }
        };

        let canonical_original_path = match conn.canonicalize(remote_path).await {
            Ok(abs_path) => abs_path,
            Err(_) => remote_path.to_string(),
        };
        let ssh_prefixed_original_path = format!("{}{}", ssh_prefix, canonical_original_path);

        // Move the entire path (file or directory) to backup location - this IS the removal
        let backup_path =
            match FileBackupManager::move_remote_path_to_backup(conn, remote_path).await {
                Ok(backup_path) => backup_path,
                Err(e) => {
                    return Ok(CallToolResult::error(vec![
                        Content::text("BACKUP_ERROR"),
                        Content::text(format!("Failed to move remote path to backup: {}", e)),
                    ]));
                }
            };

        let ssh_prefixed_backup_path = format!("{}{}", ssh_prefix, backup_path);

        let mut backup_mapping = std::collections::HashMap::new();
        backup_mapping.insert(ssh_prefixed_original_path, ssh_prefixed_backup_path);

        let item_type = if is_directory { "directory" } else { "file" };
        let recursive_note = if is_directory && recursive {
            " (recursively)"
        } else {
            ""
        };

        let backup_xml = FileBackupManager::format_backup_xml(&backup_mapping, "remote");
        let output = format!(
            "Successfully removed {} '{}'{}\n\n{}",
            item_type, original_path, recursive_note, backup_xml
        );

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Remove a local file or directory
    async fn remove_local_path(
        &self,
        path: &str,
        recursive: bool,
    ) -> Result<CallToolResult, McpError> {
        let local_path = Path::new(path);

        if !local_path.exists() {
            return Ok(CallToolResult::error(vec![
                Content::text("PATH_NOT_FOUND"),
                Content::text(format!("Path does not exist: {}", path)),
            ]));
        }

        let is_directory = local_path.is_dir();

        let absolute_original_path = match local_path.canonicalize() {
            Ok(abs_path) => abs_path.to_string_lossy().to_string(),
            Err(_) => path.to_string(),
        };

        // Move the entire path (file or directory) to backup location - this IS the removal
        let backup_path = match FileBackupManager::move_local_path_to_backup(path) {
            Ok(backup_path) => backup_path,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("BACKUP_ERROR"),
                    Content::text(format!("Failed to move local path to backup: {}", e)),
                ]));
            }
        };

        let mut backup_mapping = std::collections::HashMap::new();
        backup_mapping.insert(absolute_original_path, backup_path);

        let item_type = if is_directory { "directory" } else { "file" };
        let recursive_note = if is_directory && recursive {
            " (recursively)"
        } else {
            ""
        };

        let backup_xml = FileBackupManager::format_backup_xml(&backup_mapping, "local");
        let output = format!(
            "Successfully removed {} '{}'{}\n\n{}",
            item_type, path, recursive_note, backup_xml
        );

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }
    /// Build a TaskUpdate from a TaskInfo, extracting pause_info from raw_output if present.
    /// Used by both get_task_details and wait_for_tasks_with_streaming to populate
    /// the TUI's subagent_pause_info cache.
    fn build_task_update(task_info: &TaskInfo) -> TaskUpdate {
        let duration_secs = task_info.duration.map(|d| d.as_secs_f64());
        let output_preview = task_info.output.as_ref().and_then(|o| {
            let lines: Vec<&str> = o.lines().collect();
            if lines.is_empty() {
                None
            } else {
                lines.iter().rev().find(|l| !l.is_empty()).map(|l| {
                    if l.chars().count() > 50 {
                        let truncated: String = l.chars().take(50).collect();
                        format!("{}...", truncated)
                    } else {
                        l.to_string()
                    }
                })
            }
        });

        let pause_info = task_info.pause_info.as_ref().and_then(|pi| {
            pi.raw_output.as_ref().and_then(|raw| {
                serde_json::from_str::<serde_json::Value>(raw)
                    .ok()
                    .and_then(|json| {
                        let agent_message = json
                            .get("agent_message")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        let pending_tool_calls = json
                            .get("pause_reason")
                            .and_then(|pr| pr.get("pending_tool_calls"))
                            .and_then(|ptc| ptc.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|tc| {
                                        Some(PendingToolCall {
                                            id: tc.get("id")?.as_str()?.to_string(),
                                            name: tc.get("name")?.as_str()?.to_string(),
                                            arguments: tc
                                                .get("arguments")
                                                .cloned()
                                                .unwrap_or(serde_json::Value::Null),
                                        })
                                    })
                                    .collect()
                            });

                        if agent_message.is_some() || pending_tool_calls.is_some() {
                            Some(TaskPauseInfo {
                                agent_message,
                                pending_tool_calls,
                            })
                        } else {
                            None
                        }
                    })
            })
        });

        TaskUpdate {
            task_id: task_info.id.clone(),
            status: format!("{:?}", task_info.status),
            description: task_info.description.clone(),
            duration_secs,
            output_preview,
            is_target: true,
            pause_info,
        }
    }

    async fn wait_for_tasks_with_streaming(
        &self,
        task_ids: &[String],
        timeout: Option<std::time::Duration>,
        ctx: &RequestContext<RoleServer>,
    ) -> Result<Vec<TaskInfo>, stakpak_shared::task_manager::TaskError> {
        let mut missing_tasks: Vec<String> = Vec::new();
        for task_id in task_ids {
            let task_status = self
                .get_task_manager()
                .get_task_status(task_id.clone())
                .await?;
            if task_status.is_none() {
                missing_tasks.push(task_id.clone());
            }
        }

        if !missing_tasks.is_empty() {
            return Err(stakpak_shared::task_manager::TaskError::TaskNotFound(
                format!("Tasks not found: {}", missing_tasks.join(", ")),
            ));
        }

        let progress_id = Uuid::new_v4();

        let wait_operation = async {
            loop {
                let all_tasks = self.get_task_manager().get_all_tasks().await?;

                // Calculate real progress based on completed target tasks
                let mut completed_count = 0;
                let mut target_tasks_completed = true;

                for task_id in task_ids {
                    if let Some(task) = all_tasks.iter().find(|t| &t.id == task_id) {
                        match task.status {
                            stakpak_shared::task_manager::TaskStatus::Pending
                            | stakpak_shared::task_manager::TaskStatus::Running => {
                                target_tasks_completed = false;
                            }
                            _ => {
                                completed_count += 1;
                            }
                        }
                    }
                }

                // Calculate progress percentage
                let progress_pct = if task_ids.is_empty() {
                    100.0
                } else {
                    (completed_count as f64 / task_ids.len() as f64) * 100.0
                };

                // Build structured task updates
                let task_updates: Vec<TaskUpdate> = all_tasks
                    .iter()
                    .filter(|t| task_ids.contains(&t.id))
                    .map(Self::build_task_update)
                    .collect();

                // Also include fallback message for backwards compatibility
                let progress_table = self.format_tasks_table(&all_tasks, task_ids);

                let _ = ctx
                    .peer
                    .notify_progress(ProgressNotificationParam {
                        progress_token: ProgressToken(NumberOrString::Number(0)),
                        progress: progress_pct,
                        total: Some(100.0),
                        message: Some(
                            serde_json::to_string(&ToolCallResultProgress {
                                id: progress_id,
                                message: progress_table,
                                progress_type: Some(ProgressType::TaskWait),
                                task_updates: Some(task_updates),
                                progress: Some(progress_pct),
                            })
                            .unwrap_or_default(),
                        ),
                    })
                    .await;

                if target_tasks_completed {
                    return Ok(all_tasks);
                }

                sleep(Duration::from_millis(1000)).await;
            }
        };

        // Apply timeout if specified
        if let Some(timeout_duration) = timeout {
            match tokio_timeout(timeout_duration, wait_operation).await {
                Ok(result) => result,
                Err(_) => Err(stakpak_shared::task_manager::TaskError::TaskTimeout),
            }
        } else {
            wait_operation.await
        }
    }

    fn format_tasks_table(&self, tasks: &[TaskInfo], target_task_ids: &[String]) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};

        let mut table = String::new();

        // Add timestamp and clear separator for streaming
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| std::time::Duration::from_secs(0))
            .as_secs();
        let time_str = chrono::DateTime::from_timestamp(timestamp as i64, 0)
            .unwrap_or_else(chrono::Utc::now)
            .format("%H:%M:%S");

        table.push_str(&format!("═══ Background Tasks Update [{}] ═══\n", time_str));
        table.push_str(&format!("Waiting for: {}\n", target_task_ids.join(", ")));

        if tasks.is_empty() {
            table.push_str("No background tasks found.\n");
            table.push_str("═══════════════════════════════════════\n\n");
            return table;
        }

        // Sort tasks by start time (newest first)
        let mut sorted_tasks = tasks.to_vec();
        sorted_tasks.sort_by(|a, b| b.start_time.cmp(&a.start_time));

        // Compact format for streaming - one line per task
        for task in &sorted_tasks {
            let task_id = &task.id;
            let status = format!("{:?}", task.status);
            let duration = if let Some(duration) = task.duration {
                format!("{:.1}s", duration.as_secs_f64())
            } else {
                "running".to_string()
            };

            let truncated_command = task
                .command
                .chars()
                .take(30)
                .collect::<String>()
                .replace('\n', " ");

            // Highlight target tasks and show status
            let marker = if target_task_ids.contains(task_id) {
                ">"
            } else {
                " "
            };
            let status_icon = match status.as_str() {
                "Running" => "[RUN]",
                "Completed" => "[OK]",
                "Failed" => "[ERR]",
                _ => "[---]",
            };

            table.push_str(&format!(
                "{} {} {} [{}] {} - {}\n",
                marker, status_icon, task_id, duration, status, truncated_command
            ));
        }

        table.push_str(&format!(
            "Total: {} tasks | Waiting for: {}\n",
            sorted_tasks.len(),
            target_task_ids.len()
        ));
        table.push_str("═══════════════════════════════════════\n\n");

        table
    }

    #[tool(
        description = "Ask the user one or more questions with predefined options. Use this when you need user input to make decisions or gather preferences.

WHEN TO USE:
- When you need to clarify requirements before proceeding
- When there are multiple valid approaches and user preference matters
- When confirming / gathering information
- It's easier / faster for the user than prompting them for a full text response
"
    )]
    pub async fn ask_user(
        &self,
        _ctx: RequestContext<RoleServer>,
        Parameters(_request): Parameters<AskUserRequest>,
    ) -> Result<CallToolResult, McpError> {
        // This tool is handled specially by the TUI - it should never reach here
        // If it does, return an error indicating the tool requires interactive mode
        Ok(CallToolResult::error(vec![
            Content::text("INTERACTIVE_REQUIRED"),
            Content::text(
                "The ask_user tool requires interactive mode. It cannot be used in headless/async execution.",
            ),
        ]))
    }
}

/// Normalize a single character: map common Unicode "fancy" characters to their
/// ASCII equivalents.  Most mappings are 1-to-1, but some are 1-to-many (e.g.
/// `…` → `...`).  Returns `None` when the character requires no normalisation.
fn normalize_unicode_char(c: char) -> Option<&'static str> {
    match c {
        // Quotation marks
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{2039}' | '\u{203A}' => Some("'"), // ' ' ‚ ‹ ›  → '
        '\u{FF07}' => Some("'"), // fullwidth apostrophe → '
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{00AB}' | '\u{00BB}' => Some("\""), // " " „ « »  → "

        // Dashes
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}' => Some("-"), // ‐ ‑ ‒ – — ―  → -

        // Spaces
        '\u{00A0}' | '\u{2002}' | '\u{2003}' | '\u{2009}' | '\u{200A}' | '\u{202F}' => Some(" "), // NBSP, en/em/thin/hair/nnbsp → space

        // Dots / ellipsis  (1-to-many: one char → three chars)
        '\u{2026}' => Some("..."), // … → ...

        // Other common normalizations
        '\u{2022}' => Some("*"), // bullet → *
        '\u{00B7}' => Some("."), // middle dot → .

        _ => None,
    }
}

/// Normalize a string by mapping each character through [`normalize_unicode_char`].
///
/// Because some mappings are 1-to-many (e.g. `…` → `...`), the returned string
/// may have a **different** character count than the input.  Use
/// [`normalize_with_byte_mapping`] when you need to map positions back.
fn normalize_unicode_to_ascii(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match normalize_unicode_char(c) {
            Some(replacement) => out.push_str(replacement),
            None => out.push(c),
        }
    }
    out
}

/// Result of normalizing a string with byte-position tracking.
struct NormalizedWithMapping {
    /// The normalized string.
    text: String,
    /// For each char-boundary byte offset in `text`, the corresponding byte
    /// offset in the **original** string.
    ///
    /// Indexed by the char-boundary byte offset in `text`. There is one entry
    /// per character plus a sentinel at the end equal to the original string's
    /// byte length. This lets us translate match byte ranges from
    /// [`str::find`] directly to original byte ranges without an intermediate
    /// char-index conversion.
    norm_byte_to_orig_byte: Vec<usize>,
    /// All char-boundary byte offsets in `text` (sorted).
    /// Used to locate the *end* of a match: given the match start byte +
    /// pattern byte length, we round to the next char boundary via binary
    /// search.  Only needed when the normalized text contains multi-byte
    /// characters (e.g. non-mapped Unicode like `é`).
    char_boundaries: Vec<usize>,
}

/// Normalize `s` while building a byte-level mapping from positions in the
/// normalized output back to positions in `s`.
fn normalize_with_byte_mapping(s: &str) -> NormalizedWithMapping {
    let mut text = String::with_capacity(s.len());
    let mut norm_byte_to_orig_byte: Vec<usize> = Vec::with_capacity(s.len() + 1);
    let mut char_boundaries: Vec<usize> = Vec::with_capacity(s.len() + 1);

    for (byte_idx, c) in s.char_indices() {
        match normalize_unicode_char(c) {
            Some(replacement) => {
                for rc in replacement.chars() {
                    char_boundaries.push(text.len());
                    norm_byte_to_orig_byte.push(byte_idx);
                    text.push(rc);
                }
            }
            None => {
                char_boundaries.push(text.len());
                norm_byte_to_orig_byte.push(byte_idx);
                text.push(c);
            }
        }
    }

    // Sentinel: one past the last character.
    char_boundaries.push(text.len());
    norm_byte_to_orig_byte.push(s.len());

    NormalizedWithMapping {
        text,
        norm_byte_to_orig_byte,
        char_boundaries,
    }
}

impl NormalizedWithMapping {
    /// Convert a byte offset in the normalized `text` to the corresponding
    /// byte offset in the original string.  Returns `None` if `norm_byte` does
    /// not fall on a character boundary (should never happen for offsets
    /// returned by [`str::find`]).
    fn orig_byte_at(&self, norm_byte: usize) -> Option<usize> {
        let idx = self.char_boundaries.binary_search(&norm_byte).ok()?;
        Some(self.norm_byte_to_orig_byte[idx])
    }
}

/// Attempt to find `old_str` in `content` using Unicode-normalized matching,
/// then perform the replacement on the *original* content preserving its
/// encoding.
///
/// Returns `Some((new_content, replaced_count))` on success, `None` if the
/// normalized old_str is still not found.
///
/// Supports 1-to-many normalizations (e.g. `…` → `...`) by building a byte-
/// position mapping from the normalized characters back to original byte
/// ranges.
///
/// Uses Rust's built-in [`str::find`] (Two-Way algorithm) for O(n + m)
/// substring search instead of a naive O(n × m) character-by-character scan.
#[allow(clippy::string_slice)] // all indices from find() on normalized text + orig_byte_at() which maps to char_indices() boundaries
fn unicode_normalized_replace(
    content: &str,
    old_str: &str,
    new_str: &str,
    replace_all: bool,
) -> Option<(String, usize)> {
    let norm_old = normalize_unicode_to_ascii(old_str);

    if norm_old.is_empty() {
        return None;
    }

    // Cheap pre-check: normalize content without building the mapping.
    // If the pattern doesn't appear in the normalized content at all,
    // skip the heavier mapping allocation.
    let norm_content_quick = normalize_unicode_to_ascii(content);
    if !norm_content_quick.contains(&norm_old) {
        return None;
    }
    drop(norm_content_quick);

    // Pattern is present — build the full mapping.
    let norm_content = normalize_with_byte_mapping(content);

    // Use Rust's optimized string search (Two-Way algorithm, O(n + m)).
    // Matches are collected as (orig_byte_start, orig_byte_end) pairs.
    let norm_old_byte_len = norm_old.len();
    let mut match_orig_ranges: Vec<(usize, usize)> = Vec::new();

    if replace_all {
        let mut search_byte = 0usize;
        while search_byte + norm_old_byte_len <= norm_content.text.len() {
            if let Some(rel) = norm_content.text[search_byte..].find(&norm_old) {
                let match_start = search_byte + rel;
                let match_end = match_start + norm_old_byte_len;

                if let (Some(orig_start), Some(orig_end)) = (
                    norm_content.orig_byte_at(match_start),
                    norm_content.orig_byte_at(match_end),
                ) {
                    match_orig_ranges.push((orig_start, orig_end));
                }
                search_byte = match_end;
            } else {
                break;
            }
        }
    } else if let Some(match_start) = norm_content.text.find(&norm_old) {
        let match_end = match_start + norm_old_byte_len;

        if let (Some(orig_start), Some(orig_end)) = (
            norm_content.orig_byte_at(match_start),
            norm_content.orig_byte_at(match_end),
        ) {
            match_orig_ranges.push((orig_start, orig_end));
        }
    }

    if match_orig_ranges.is_empty() {
        return None;
    }

    let replaced_count = match_orig_ranges.len();

    // Build the result by splicing in new_str at each matched byte range.
    let mut result = String::with_capacity(content.len());
    let mut prev_byte_end = 0usize;

    for &(orig_start, orig_end) in &match_orig_ranges {
        result.push_str(&content[prev_byte_end..orig_start]);
        result.push_str(new_str);
        prev_byte_end = orig_end;
    }
    result.push_str(&content[prev_byte_end..]);

    Some((result, replaced_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_command_request_empty_remote_is_none() {
        let request: RunCommandRequest = serde_json::from_value(serde_json::json!({
            "command": "echo hello",
            "remote": "   "
        }))
        .expect("run command request should deserialize");

        assert!(request.remote.is_none());
    }

    #[test]
    fn run_command_request_password_preserves_whitespace() {
        let request: RunCommandRequest = serde_json::from_value(serde_json::json!({
            "command": "echo hello",
            "password": "  pass with spaces  "
        }))
        .expect("run command request should deserialize");

        assert_eq!(request.password.as_deref(), Some("  pass with spaces  "));
    }

    // ---------------------------------------------------------------
    // normalize_unicode_char / normalize_unicode_to_ascii
    // ---------------------------------------------------------------

    #[test]
    fn test_normalize_straight_quotes_unchanged() {
        assert_eq!(normalize_unicode_to_ascii("it's fine"), "it's fine");
    }

    #[test]
    fn test_normalize_curly_single_quotes() {
        // U+2018 LEFT SINGLE QUOTATION MARK
        assert_eq!(normalize_unicode_to_ascii("it\u{2018}s"), "it's");
        // U+2019 RIGHT SINGLE QUOTATION MARK
        assert_eq!(normalize_unicode_to_ascii("shouldn\u{2019}t"), "shouldn't");
    }

    #[test]
    fn test_normalize_curly_double_quotes() {
        // U+201C / U+201D
        assert_eq!(
            normalize_unicode_to_ascii("\u{201C}hello\u{201D}"),
            "\"hello\""
        );
    }

    #[test]
    fn test_normalize_en_dash() {
        assert_eq!(normalize_unicode_to_ascii("a\u{2013}b"), "a-b");
    }

    #[test]
    fn test_normalize_em_dash() {
        assert_eq!(normalize_unicode_to_ascii("a\u{2014}b"), "a-b");
    }

    #[test]
    fn test_normalize_figure_dash() {
        assert_eq!(normalize_unicode_to_ascii("a\u{2012}b"), "a-b");
    }

    #[test]
    fn test_normalize_hyphen_unicode() {
        // U+2010 HYPHEN
        assert_eq!(normalize_unicode_to_ascii("a\u{2010}b"), "a-b");
    }

    #[test]
    fn test_normalize_nbsp() {
        assert_eq!(
            normalize_unicode_to_ascii("hello\u{00A0}world"),
            "hello world"
        );
    }

    #[test]
    fn test_normalize_preserves_char_count_for_1to1() {
        // Only 1-to-1 mappings in this input — char count preserved
        let input = "\u{201C}shouldn\u{2019}t\u{201D} \u{2013} done";
        let normalized = normalize_unicode_to_ascii(input);
        assert_eq!(input.chars().count(), normalized.chars().count());
    }

    #[test]
    fn test_normalize_ellipsis_expands() {
        // Ellipsis is 1-to-3 mapping
        assert_eq!(normalize_unicode_to_ascii("wait\u{2026}"), "wait...");
        // Char count grows: 5 input chars → 7 output chars
        assert_eq!("wait\u{2026}".chars().count(), 5);
        assert_eq!(
            normalize_unicode_to_ascii("wait\u{2026}").chars().count(),
            7
        );
    }

    #[test]
    fn test_normalize_pure_ascii_passthrough() {
        let ascii = "The quick brown fox jumps over the lazy dog. 0123456789 !@#$%^&*()";
        assert_eq!(normalize_unicode_to_ascii(ascii), ascii);
    }

    #[test]
    fn test_normalize_bullet() {
        assert_eq!(normalize_unicode_to_ascii("\u{2022} item"), "* item");
    }

    #[test]
    fn test_normalize_non_breaking_hyphen() {
        assert_eq!(
            normalize_unicode_to_ascii("non\u{2011}breaking"),
            "non-breaking"
        );
    }

    #[test]
    fn test_normalize_guillemets() {
        assert_eq!(
            normalize_unicode_to_ascii("\u{00AB}quoted\u{00BB}"),
            "\"quoted\""
        );
    }

    #[test]
    fn test_normalize_fullwidth_apostrophe() {
        assert_eq!(normalize_unicode_to_ascii("it\u{FF07}s"), "it's");
    }

    #[test]
    fn test_normalize_mixed_unicode_and_ascii() {
        let input = "It\u{2019}s a \u{201C}test\u{201D} \u{2013} really";
        let expected = "It's a \"test\" - really";
        assert_eq!(normalize_unicode_to_ascii(input), expected);
    }

    // ---------------------------------------------------------------
    // normalize_with_byte_mapping
    // ---------------------------------------------------------------

    #[test]
    fn test_byte_mapping_ascii() {
        let m = normalize_with_byte_mapping("abc");
        assert_eq!(m.text, "abc");
        assert_eq!(m.char_boundaries, vec![0, 1, 2, 3]); // includes sentinel
        assert_eq!(m.norm_byte_to_orig_byte, vec![0, 1, 2, 3]); // includes sentinel
    }

    #[test]
    fn test_byte_mapping_ellipsis() {
        // … is 3 bytes in UTF-8, maps to 3 normalized chars "..."
        let m = normalize_with_byte_mapping("x\u{2026}y");
        assert_eq!(m.text, "x...y");
        // char boundaries: x=0, .=1, .=2, .=3, y=4, sentinel=5
        assert_eq!(m.char_boundaries, vec![0, 1, 2, 3, 4, 5]);
        // 'x' at orig 0, all three '.' at orig 1 (start of …), 'y' at orig 4, sentinel=5
        assert_eq!(m.norm_byte_to_orig_byte, vec![0, 1, 1, 1, 4, 5]);
    }

    #[test]
    fn test_byte_mapping_curly_quote() {
        // U+2019 is 3 bytes in UTF-8
        let m = normalize_with_byte_mapping("a\u{2019}b");
        assert_eq!(m.text, "a'b");
        assert_eq!(m.char_boundaries, vec![0, 1, 2, 3]);
        assert_eq!(m.norm_byte_to_orig_byte, vec![0, 1, 4, 5]);
    }

    #[test]
    fn test_byte_mapping_multibyte_passthrough() {
        // 'é' (2 bytes) is NOT in the normalization map — preserved as-is
        let m = normalize_with_byte_mapping("café");
        assert_eq!(m.text, "café");
        // c=0, a=1, f=2, é=3 (norm byte), sentinel=5 (norm byte, since é is 2 bytes)
        assert_eq!(m.char_boundaries, vec![0, 1, 2, 3, 5]);
        // c→0, a→1, f→2, é→3, sentinel→5 (orig len)
        assert_eq!(m.norm_byte_to_orig_byte, vec![0, 1, 2, 3, 5]);
    }

    // ---------------------------------------------------------------
    // unicode_normalized_replace — exact match still works
    // ---------------------------------------------------------------

    #[test]
    fn test_normalized_replace_exact_ascii() {
        let content = "hello world";
        let result = unicode_normalized_replace(content, "world", "rust", false);
        assert_eq!(result, Some(("hello rust".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_no_match() {
        let content = "hello world";
        assert_eq!(
            unicode_normalized_replace(content, "xyz", "abc", false),
            None
        );
    }

    // ---------------------------------------------------------------
    // unicode_normalized_replace — curly quote fallback
    // ---------------------------------------------------------------

    #[test]
    fn test_normalized_replace_curly_apostrophe() {
        // File has curly quote, LLM sends straight quote
        let content = "Infrastructure shouldn\u{2019}t be this hard.";
        let old_str = "Infrastructure shouldn't be this hard.";
        let new_str = "Infra is easy.";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("Infra is easy.".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_preserves_surrounding_content() {
        let content = "before shouldn\u{2019}t after";
        let old_str = "shouldn't";
        let new_str = "REPLACED";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("before REPLACED after".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_curly_double_quotes() {
        let content = "She said \u{201C}hello\u{201D} loudly";
        let old_str = "\"hello\"";
        let new_str = "\"hi\"";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("She said \"hi\" loudly".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_en_dash() {
        let content = "pages 10\u{2013}20 of the book";
        let old_str = "10-20";
        let new_str = "10-30";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("pages 10-30 of the book".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_em_dash() {
        let content = "word\u{2014}another word";
        let old_str = "word-another";
        let new_str = "one-two";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("one-two word".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_nbsp_to_space() {
        let content = "hello\u{00A0}world";
        let old_str = "hello world";
        let new_str = "hi there";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("hi there".to_string(), 1)));
    }

    // ---------------------------------------------------------------
    // unicode_normalized_replace — ellipsis (1-to-many mapping)
    // ---------------------------------------------------------------

    #[test]
    fn test_normalized_replace_ellipsis_in_content() {
        // File has … (1 char), LLM sends ... (3 chars)
        let content = "wait\u{2026} what?";
        let old_str = "wait... what?";
        let new_str = "oh!";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("oh!".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_ellipsis_in_old_str() {
        // File has ... (3 chars), LLM sends … (1 char, normalizes to ...)
        let content = "wait... what?";
        let old_str = "wait\u{2026} what?";
        let new_str = "oh!";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("oh!".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_ellipsis_preserves_surroundings() {
        let content = "before\u{2026}after";
        let old_str = "...";
        let new_str = "---";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("before---after".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_ellipsis_replace_all() {
        let content = "one\u{2026}two\u{2026}three";
        let old_str = "...";
        let new_str = " ";
        let result = unicode_normalized_replace(content, old_str, new_str, true);
        assert_eq!(result, Some(("one two three".to_string(), 2)));
    }

    #[test]
    fn test_normalized_replace_ellipsis_with_other_unicode() {
        // Mix of ellipsis and curly quotes
        let content = "She said \u{201C}wait\u{2026}\u{201D}";
        let old_str = "\"wait...\"";
        let new_str = "\"go!\"";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("She said \"go!\"".to_string(), 1)));
    }

    // ---------------------------------------------------------------
    // unicode_normalized_replace — additional Unicode chars
    // ---------------------------------------------------------------

    #[test]
    fn test_normalized_replace_figure_dash() {
        let content = "pages 10\u{2012}20";
        let old_str = "10-20";
        let new_str = "10-30";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("pages 10-30".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_unicode_hyphen() {
        let content = "non\u{2010}breaking";
        let old_str = "non-breaking";
        let new_str = "unbreakable";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("unbreakable".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_fullwidth_apostrophe() {
        let content = "it\u{FF07}s fine";
        let old_str = "it's fine";
        let new_str = "all good";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("all good".to_string(), 1)));
    }

    // ---------------------------------------------------------------
    // unicode_normalized_replace — replace_all
    // ---------------------------------------------------------------

    #[test]
    fn test_normalized_replace_all_multiple() {
        let content = "shouldn\u{2019}t and shouldn\u{2019}t again";
        let old_str = "shouldn't";
        let new_str = "should not";
        let result = unicode_normalized_replace(content, old_str, new_str, true);
        assert_eq!(
            result,
            Some(("should not and should not again".to_string(), 2))
        );
    }

    #[test]
    fn test_normalized_replace_all_false_stops_at_first() {
        let content = "shouldn\u{2019}t and shouldn\u{2019}t again";
        let old_str = "shouldn't";
        let new_str = "should not";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(
            result,
            Some(("should not and shouldn\u{2019}t again".to_string(), 1))
        );
    }

    // ---------------------------------------------------------------
    // unicode_normalized_replace — multiple different Unicode chars
    // ---------------------------------------------------------------

    #[test]
    fn test_normalized_replace_mixed_unicode() {
        // File has: curly quotes + en-dash
        let content = "\u{201C}10\u{2013}20\u{201D}";
        let old_str = "\"10-20\"";
        let new_str = "range";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("range".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_readme_scenario() {
        // The exact scenario from the bug: file has U+2019 in "shouldn't"
        // and U+2013 en-dashes elsewhere. LLM normalizes to ASCII.
        let content = concat!(
            "Infrastructure shouldn\u{2019}t be this hard.\n",
            "- `--disable-secret-redaction` \u{2013} **not recommended**\n",
            "- `--privacy-mode` \u{2013} redacts additional data\n",
        );

        // LLM sends old_str with the first line only (ASCII apostrophe)
        let old_str = "Infrastructure shouldn't be this hard.";
        let new_str = "Infrastructure is easy.";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert!(result.is_some());
        let (new_content, count) = result.unwrap();
        assert_eq!(count, 1);
        assert!(new_content.starts_with("Infrastructure is easy.\n"));
        // Rest of the file (with en-dashes) should be untouched
        assert!(new_content.contains("\u{2013}"));
    }

    // ---------------------------------------------------------------
    // unicode_normalized_replace — edge cases
    // ---------------------------------------------------------------

    #[test]
    fn test_normalized_replace_empty_old_str() {
        assert_eq!(unicode_normalized_replace("content", "", "x", false), None);
    }

    #[test]
    fn test_normalized_replace_empty_content() {
        assert_eq!(unicode_normalized_replace("", "hello", "x", false), None);
    }

    #[test]
    fn test_normalized_replace_entire_content() {
        let content = "shouldn\u{2019}t";
        let old_str = "shouldn't";
        let new_str = "should not";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("should not".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_at_start() {
        let content = "\u{201C}hello\u{201D} world";
        let old_str = "\"hello\"";
        let new_str = "\"hi\"";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("\"hi\" world".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_at_end() {
        let content = "world \u{201C}hello\u{201D}";
        let old_str = "\"hello\"";
        let new_str = "\"hi\"";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("world \"hi\"".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_no_false_positive_on_ascii() {
        // When both content and old_str are pure ASCII and don't match,
        // the normalized path should also return None.
        assert_eq!(
            unicode_normalized_replace("hello world", "goodbye", "x", false),
            None
        );
    }

    #[test]
    fn test_normalized_replace_preserves_other_unicode() {
        // Unicode that is NOT in the normalization map should be preserved
        let content = "café shouldn\u{2019}t break";
        let old_str = "shouldn't";
        let new_str = "should not";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("café should not break".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_adjacent_unicode_chars() {
        // Multiple unicode chars right next to each other
        let content = "\u{201C}\u{2019}\u{2013}\u{201D}";
        let old_str = "\"'-\"";
        let new_str = "X";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("X".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_only_unicode_differs() {
        // Content and old_str are identical except for one Unicode char
        let content = "a\u{00A0}b"; // non-breaking space
        let old_str = "a b"; // regular space
        let new_str = "a_b";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert_eq!(result, Some(("a_b".to_string(), 1)));
    }

    #[test]
    fn test_normalized_replace_large_multiline() {
        // Simulates a realistic str_replace with a large multi-line old_str
        let content = concat!(
            "# Title\n\n",
            "Some text before.\n\n",
            "Infrastructure shouldn\u{2019}t be this hard. Stakpak lets developers secure, deploy, and run infra.\n\n",
            "## Features\n\n",
            "- Feature 1 \u{2013} description\n",
            "- Feature 2 \u{2013} description\n",
            "\nMore text after.\n",
        );

        let old_str = concat!(
            "Infrastructure shouldn't be this hard. Stakpak lets developers secure, deploy, and run infra.\n\n",
            "## Features\n\n",
            "- Feature 1 - description\n",
            "- Feature 2 - description\n",
        );

        let new_str = "## Simplified\n\nJust works.\n";

        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert!(result.is_some());
        let (new_content, count) = result.unwrap();
        assert_eq!(count, 1);
        assert!(new_content.contains("# Title"));
        assert!(new_content.contains("Some text before."));
        assert!(new_content.contains("## Simplified\n\nJust works.\n"));
        assert!(new_content.contains("More text after."));
        // Original unicode chars that were NOT in old_str are preserved
        assert!(!new_content.contains("\u{2019}"));
        assert!(!new_content.contains("\u{2013}"));
    }

    #[test]
    fn test_normalized_replace_all_non_overlapping() {
        let content = "a\u{2013}b c\u{2013}d";
        let old_str = "-";
        // This should match the normalized dashes
        let new_str = "=";
        let result = unicode_normalized_replace(content, old_str, new_str, true);
        assert_eq!(result, Some(("a=b c=d".to_string(), 2)));
    }

    // ---------------------------------------------------------------
    // unicode_normalized_replace — ellipsis in multi-line realistic scenario
    // ---------------------------------------------------------------

    #[test]
    fn test_normalized_replace_ellipsis_multiline() {
        let content = concat!("Loading\u{2026}\n", "Please wait\u{2026}\n", "Done!\n",);
        let old_str = concat!("Loading...\n", "Please wait...\n",);
        let new_str = "Loaded!\n";
        let result = unicode_normalized_replace(content, old_str, new_str, false);
        assert!(result.is_some());
        let (new_content, count) = result.unwrap();
        assert_eq!(count, 1);
        assert_eq!(new_content, "Loaded!\nDone!\n");
    }

    #[test]
    fn test_normalized_replace_multiple_ellipsis_replace_all() {
        let content = "a\u{2026}b\u{2026}c";
        let old_str = "...";
        let new_str = "***";
        let result = unicode_normalized_replace(content, old_str, new_str, true);
        assert_eq!(result, Some(("a***b***c".to_string(), 2)));
    }
}
