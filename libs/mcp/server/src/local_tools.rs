use crate::tool_container::ToolContainer;
use linux_sandbox::SandboxPolicy;
use rand::Rng;
use rmcp::service::RequestContext;
use rmcp::{Error as McpError, handler::server::tool::Parameters, model::*, schemars, tool};
use rmcp::{RoleServer, tool_router};
use serde::Deserialize;
use stakpak_shared::file_backup_manager::FileBackupManager;
use stakpak_shared::remote_connection::{
    PathLocation, RemoteConnection, RemoteConnectionInfo, RemoteFileSystemProvider,
};

use html2md;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde_json::json;
use similar::TextDiff;
use stakpak_shared::local_store::LocalStore;
use stakpak_shared::models::integrations::mcp::CallToolResultExt;
use stakpak_shared::models::integrations::openai::ToolCallResultProgress;
use stakpak_shared::task_manager::TaskInfo;
use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};
use stakpak_shared::utils::{LocalFileSystemProvider, generate_directory_tree};
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
    #[schemars(
        description = "Optional remote connection string (format: user@host or user@host:port)"
    )]
    pub remote: Option<String>,
    #[schemars(description = "Optional password for remote connection")]
    pub password: Option<String>,
    #[schemars(description = "Optional path to private key for remote connection")]
    pub private_key_path: Option<String>,
}

#[derive(Debug)]
pub struct CommandResult {
    pub output: String,
    pub exit_code: i32,
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
    #[schemars(description = "Optional password for remote connection (if path is remote)")]
    pub password: Option<String>,
    #[schemars(
        description = "Optional path to private key for remote connection (if path is remote)"
    )]
    pub private_key_path: Option<String>,
    #[schemars(description = "Display directory as a nested tree structure (default: false)")]
    pub tree: Option<bool>,
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

SECRET HANDLING: 
- Output containing secrets will be redacted and shown as placeholders like [REDACTED_SECRET:rule-id:hash]
- You can use these placeholders in subsequent commands - they will be automatically restored to actual values before execution
- Example: If you see 'export API_KEY=[REDACTED_SECRET:api-key:abc123]', you can use '[REDACTED_SECRET:api-key:abc123]' in later commands

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
                    match handle_large_output(&command_result.output, "command.output") {
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

                let redacted_output = self
                    .get_secret_manager()
                    .redact_and_store_secrets(&command_result.output, None);

                if command_result.exit_code != 0 {
                    return Ok(CallToolResult::error(vec![
                        Content::text("COMMAND_FAILED"),
                        Content::text(redacted_output),
                    ]));
                }
                Ok(CallToolResult::success(vec![Content::text(
                    &redacted_output,
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

SECRET HANDLING:
- Commands containing secrets will have them restored before execution
- Task output will be redacted when retrieved
- Use secret placeholders like [REDACTED_SECRET:rule-id:hash] in commands

Use the get_all_tasks tool to monitor task progress, or the cancel_task tool to cancel a task."
    )]
    pub async fn run_command_task(
        &self,
        _ctx: RequestContext<RoleServer>,
        Parameters(RunCommandRequest {
            command,
            description: _,
            timeout,
            remote,
            password,
            private_key_path,
        }): Parameters<RunCommandRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Restore secrets in the command before execution
        let actual_command = self
            .get_secret_manager()
            .restore_secrets_in_string(&command);

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
                .start_task(actual_command, timeout_duration, Some(remote_connection))
                .await
        } else {
            // Local async command (existing logic)
            self.get_task_manager()
                .start_task(actual_command, timeout_duration, None)
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
  - Output: Command output preview (truncated to 80 chars, redacted for security)

This tool provides a clean tabular overview of all background tasks and their current state.
Use the full Task ID from this output with cancel_task to cancel specific tasks."
    )]
    pub async fn get_all_tasks(
        &self,
        _ctx: RequestContext<RoleServer>,
        Parameters(GetAllTasksRequest { view: _ }): Parameters<GetAllTasksRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.get_task_manager().get_all_tasks().await {
            Ok(tasks) => {
                if tasks.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(
                        "No background tasks found.",
                    )]));
                }

                let redacted_tasks: Vec<TaskInfo> = tasks
                    .into_iter()
                    .map(|mut task| {
                        if let Some(ref output) = task.output {
                            task.output = Some(
                                self.get_secret_manager()
                                    .redact_and_store_secrets(output, None),
                            );
                        }
                        task
                    })
                    .collect();

                // Create markdown table format
                let mut table = String::new();
                table.push_str("# Background Tasks\n\n");

                // Markdown table header
                table.push_str("| Task ID | Status | Command | Start Time | Duration | Output |\n");
                table.push_str("|---------|--------|------------|----------|--------|--------|\n");

                // Markdown table rows
                for task in &redacted_tasks {
                    let task_id = task.id.clone();
                    let status = format!("{:?}", task.status);
                    let start_time = task.start_time.to_rfc3339();
                    let duration = if let Some(duration) = task.duration {
                        format!("{:.2}s", duration.as_secs_f64())
                    } else {
                        "N/A".to_string()
                    };

                    let redacted_command = self
                        .get_secret_manager()
                        .redact_and_store_secrets(&task.command, None);
                    let redacted_output = if let Some(ref out) = task.output {
                        self.get_secret_manager()
                            .redact_and_store_secrets(out, None)
                    } else {
                        "No output yet".to_string()
                    };

                    let escaped_command = redacted_command
                        .chars()
                        .take(100)
                        .collect::<String>()
                        .replace('|', "\\|")
                        .replace('\n', " ");
                    let escaped_output = redacted_output
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

                table.push_str(&format!("\n**Total: {} task(s)**", redacted_tasks.len()));

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
                let redacted_tasks: Vec<TaskInfo> = tasks
                    .into_iter()
                    .map(|mut task| {
                        if let Some(ref output) = task.output {
                            task.output = Some(
                                self.get_secret_manager()
                                    .redact_and_store_secrets(output, None),
                            );
                        }
                        task
                    })
                    .collect();

                let table = self.format_tasks_table(&redacted_tasks, &task_ids);

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
- Complete command output with secret redaction
- Error information if the task failed

If the task output exceeds 300 lines the result will be truncated and the full output will be saved to a file in the current directory.

Use this tool to check the progress and results of long-running background tasks."
    )]
    pub async fn get_task_details(
        &self,
        _ctx: RequestContext<RoleServer>,
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

                let redacted_command = self
                    .get_secret_manager()
                    .redact_and_store_secrets(&task_info.command, None);

                let redacted_output = if let Some(ref output) = task_info.output {
                    match handle_large_output(
                        &self
                            .get_secret_manager()
                            .redact_and_store_secrets(output, None),
                        "task.output",
                    ) {
                        Ok(result) => result,
                        Err(e) => {
                            return Ok(CallToolResult::error(vec![
                                Content::text("OUTPUT_HANDLING_ERROR"),
                                Content::text(format!("Failed to handle task output: {}", e)),
                            ]));
                        }
                    }
                } else {
                    "No output available".to_string()
                };

                let output = format!(
                    "# Task Details: {}\n\nStatus: {:?}\nTask ID: {}\nStarted: {}\nDuration: {}\nCommand: \n```\n{}\n```\n\n## Output:\n```\n{}\n```",
                    task_info.id,
                    task_info.status,
                    task_info.id,
                    task_info.start_time.format("%Y-%m-%d %H:%M:%S UTC"),
                    duration_str,
                    redacted_command,
                    redacted_output
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

SECRET HANDLING:
- File contents containing secrets will be redacted and shown as placeholders like [REDACTED_SECRET:rule-id:hash]
- These placeholders represent actual secret values that are safely stored for later use
- You can reference these placeholders when working with the file content

A maximum of 300 lines will be shown at a time, the rest will be truncated."
    )]
    pub async fn view(
        &self,
        Parameters(ViewRequest {
            path,
            view_range,
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
                    self.view_remote_path(&conn, &remote_path, &path, view_range, MAX_LINES, tree)
                        .await
                }
                Err(error_result) => Ok(error_result),
            }
        } else {
            // Handle local file/directory viewing
            self.view_local_path(&path, view_range, MAX_LINES, tree)
                .await
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

SECRET HANDLING:
- You can use secret placeholders like [REDACTED_SECRET:rule-id:hash] in both old_str and new_str parameters
- These placeholders will be automatically restored to actual secret values before performing the replacement
- This allows you to safely work with secret values without exposing them

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
  * '/local/path/file.txt' - Create local file (default behavior)

SECRET HANDLING:
- File content containing secrets will have them restored before writing to ensure functionality
- Use secret placeholders like [REDACTED_SECRET:rule-id:hash] in file_text parameter"
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
        description = "Generate a cryptographically secure password with the specified constraints. The generated password will be automatically redacted in the response for security.

PARAMETERS:
- length: The length of the password to generate (default: 15 characters)
- no_symbols: Whether to exclude symbols from the password (default: false, includes symbols)

CHARACTER SETS:
- Letters: A-Z, a-z (always included)
- Numbers: 0-9 (always included)  
- Symbols: !@#$%^&*()_+-=[]{}|;:,.<>? (included unless no_symbols=true)

SECURITY FEATURES:
- Uses cryptographically secure random number generation
- Output is automatically redacted and stored as [REDACTED_SECRET:password:hash]
- The redacted placeholder can be used in subsequent commands where actual password will be restored
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

        let redacted_password = self
            .get_secret_manager()
            .redact_and_store_password(&password, &password);

        Ok(CallToolResult::success(vec![Content::text(
            &redacted_password,
        )]))
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

        let html_content = match response.text().await {
            Ok(content) => content,
            Err(e) => {
                error!("Failed to read response body: {}", e);
                return Ok(CallToolResult::error(vec![
                    Content::text("RESPONSE_READ_ERROR"),
                    Content::text(format!("Failed to read response body: {}", e)),
                ]));
            }
        };

        // is this enough? or do we need to sanitize the html before turning it to markdown
        let markdown_content = html2md::rewrite_html(&html_content, false);

        let result = match handle_large_output(&markdown_content, "webpage") {
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
        let actual_command = self.get_secret_manager().restore_secrets_in_string(command);

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
                .execute_command(&actual_command, timeout_duration, Some(ctx))
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
            self.execute_local_command(&actual_command, timeout, ctx)
                .await
        }
    }

    /// Check if a command requires network access
    fn command_requires_network(&self, command: &str) -> bool {
        let network_commands = [
            "curl",
            "wget",
            "ping",
            "nc",
            "netcat",
            "ssh",
            "telnet",
            "git clone",
            "git fetch",
            "git pull",
            "git push",
            "docker pull",
            "docker push",
            "kubectl",
            "aws",
            "gcloud",
            "az",
            "terraform",
            "http",
            "https://",
            "ftp",
            "sftp",
            "rsync",
            "scp",
            "mtr",
        ];

        let lower_command = command.to_lowercase();
        network_commands
            .iter()
            .any(|&cmd| lower_command.contains(cmd))
    }

    /// Execute local command with existing logic extracted to avoid duplication
    async fn execute_local_command(
        &self,
        actual_command: &str,
        timeout: Option<u64>,
        ctx: &RequestContext<RoleServer>,
    ) -> Result<CommandResult, CallToolResult> {
        // Check if sandbox is enabled and apply policy
        if self.sandbox_enabled {
            tracing::info!(
                "Sandbox mode enabled, applying policy to command: {}",
                actual_command
            );

            // Load default sandbox policy
            let policy = SandboxPolicy::default();

            // Evaluate command against policy
            let allow_network = policy.should_allow_network(actual_command);
            let is_destructive = policy.is_destructive(actual_command);

            tracing::info!(
                "Policy evaluation - command: {}, allow_network: {}, is_destructive: {}",
                actual_command,
                allow_network,
                is_destructive
            );

            // Check if command requires network
            let requires_network = self.command_requires_network(actual_command);

            // Block ALL destructive commands when sandbox is enabled
            if is_destructive {
                tracing::warn!("Policy BLOCKED destructive command: {}", actual_command);
                let error_msg = format!(
                    "Command blocked by sandbox policy. Destructive commands are not allowed: {}",
                    actual_command
                );
                return Err(CallToolResult::error(vec![
                    Content::text("SANDBOX_POLICY_VIOLATION"),
                    Content::text(error_msg),
                ]));
            }

            // Block if policy says no network and command requires it
            if !allow_network && requires_network {
                tracing::warn!(
                    "Policy BLOCKED network access for command: {}",
                    actual_command
                );
                let error_msg = format!(
                    "Command blocked by sandbox policy. Network access denied for: {}",
                    actual_command
                );
                return Err(CallToolResult::error(vec![
                    Content::text("SANDBOX_POLICY_VIOLATION"),
                    Content::text(error_msg),
                ]));
            }
        }

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

        // Helper function to stream output and wait for process completion
        let stream_and_wait = async {
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
        view_range: Option<[i32; 2]>,
        max_lines: usize,
        tree: Option<bool>,
    ) -> Result<CallToolResult, McpError> {
        let path_obj = Path::new(path);

        if !path_obj.exists() {
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_NOT_FOUND"),
                Content::text(format!("File or directory not found: {}", path)),
            ]));
        }

        if path_obj.is_dir() {
            let depth = if tree.unwrap_or(false) { 3 } else { 1 };
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
            // Read file contents
            match fs::read_to_string(path) {
                Ok(content) => {
                    let result = match self
                        .format_file_content(&content, path, view_range, max_lines, "File")
                    {
                        Ok(result) => result,
                        Err(e) => {
                            return Ok(CallToolResult::error(vec![
                                Content::text("FORMAT_ERROR"),
                                Content::text(format!("Failed to format file content: {}", e)),
                            ]));
                        }
                    };

                    let redacted_result = self
                        .get_secret_manager()
                        .redact_and_store_secrets(&result, Some(path));
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

    /// View the contents of a remote file or directory
    async fn view_remote_path(
        &self,
        conn: &Arc<RemoteConnection>,
        remote_path: &str,
        original_path: &str,
        view_range: Option<[i32; 2]>,
        max_lines: usize,
        tree: Option<bool>,
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
            let depth = if tree.unwrap_or(false) { 3 } else { 1 };
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
            // Read remote file contents
            match conn.read_file_to_string(remote_path).await {
                Ok(content) => {
                    let result = match self.format_file_content(
                        &content,
                        original_path,
                        view_range,
                        max_lines,
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

                    let redacted_result = self
                        .get_secret_manager()
                        .redact_and_store_secrets(&result, Some(original_path));
                    Ok(CallToolResult::success(vec![Content::text(
                        &redacted_result,
                    )]))
                }
                Err(e) => Ok(CallToolResult::error(vec![
                    Content::text("READ_ERROR"),
                    Content::text(format!("Cannot read remote file: {}", e)),
                ])),
            }
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
        let actual_old_str = self.get_secret_manager().restore_secrets_in_string(old_str);
        let actual_new_str = self.get_secret_manager().restore_secrets_in_string(new_str);

        if actual_old_str == actual_new_str {
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

        if !content.contains(&actual_old_str) {
            return Ok(CallToolResult::error(vec![
                Content::text("STRING_NOT_FOUND"),
                Content::text("The string old_str was not found in the file"),
            ]));
        }

        let new_content = if replace_all.unwrap_or(false) {
            content.replace(&actual_old_str, &actual_new_str)
        } else {
            content.replacen(&actual_old_str, &actual_new_str, 1)
        };

        let replaced_count = if replace_all.unwrap_or(false) {
            content.matches(&actual_old_str).count()
        } else if content.contains(&actual_old_str) {
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

        let redacted_output = self
            .get_secret_manager()
            .redact_and_store_secrets(&output, Some(original_path));

        Ok(CallToolResult::success(vec![Content::text(
            redacted_output,
        )]))
    }

    /// Replace a specific string in a local file
    async fn str_replace_local(
        &self,
        path: &str,
        old_str: &str,
        new_str: &str,
        replace_all: Option<bool>,
    ) -> Result<CallToolResult, McpError> {
        let actual_old_str = self.get_secret_manager().restore_secrets_in_string(old_str);
        let actual_new_str = self.get_secret_manager().restore_secrets_in_string(new_str);

        if actual_old_str == actual_new_str {
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

        if !original_content.contains(&actual_old_str) {
            return Ok(CallToolResult::error(vec![
                Content::text("STRING_NOT_FOUND"),
                Content::text("The string old_str was not found in the file"),
            ]));
        }

        let new_content = if replace_all.unwrap_or(false) {
            original_content.replace(&actual_old_str, &actual_new_str)
        } else {
            original_content.replacen(&actual_old_str, &actual_new_str, 1)
        };

        let replaced_count = if replace_all.unwrap_or(false) {
            original_content.matches(&actual_old_str).count()
        } else if original_content.contains(&actual_old_str) {
            1
        } else {
            0
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

        let redacted_output = self
            .get_secret_manager()
            .redact_and_store_secrets(&output, Some(path));

        Ok(CallToolResult::success(vec![Content::text(
            redacted_output,
        )]))
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

        // Restore secrets in the file content before writing
        let actual_file_text = self
            .get_secret_manager()
            .restore_secrets_in_string(file_text);

        // Create the file using the correct SFTP method
        if let Err(e) = conn
            .create_file(remote_path, actual_file_text.as_bytes())
            .await
        {
            error!("Failed to create remote file '{}': {}", remote_path, e);
            return Ok(CallToolResult::error(vec![
                Content::text("CREATE_ERROR"),
                Content::text(format!(
                    "Failed to create remote file '{}': {}",
                    remote_path, e
                )),
            ]));
        }

        let lines = actual_file_text.lines().count();
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

        // Restore secrets in the file content before writing
        let actual_file_text = self
            .get_secret_manager()
            .restore_secrets_in_string(file_text);

        match fs::write(path, actual_file_text) {
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
                let mut target_tasks_completed = true;

                for task_id in task_ids {
                    if let Some(task) = all_tasks.iter().find(|t| &t.id == task_id) {
                        match task.status {
                            stakpak_shared::task_manager::TaskStatus::Pending
                            | stakpak_shared::task_manager::TaskStatus::Running => {
                                target_tasks_completed = false;
                                break;
                            }
                            _ => {}
                        }
                    }
                }

                let progress_table = self.format_tasks_table(&all_tasks, task_ids);

                let _ = ctx
                    .peer
                    .notify_progress(ProgressNotificationParam {
                        progress_token: ProgressToken(NumberOrString::Number(0)),
                        progress: if target_tasks_completed { 100 } else { 50 },
                        total: Some(100),
                        message: Some(
                            serde_json::to_string(&ToolCallResultProgress {
                                id: progress_id,
                                message: progress_table,
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

            let redacted_command = self
                .get_secret_manager()
                .redact_and_store_secrets(&task.command, None);

            let truncated_command = redacted_command
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
}

/// Helper method to handle large output by truncating and saving to file
fn handle_large_output(output: &str, file_prefix: &str) -> Result<String, McpError> {
    const MAX_LINES: usize = 300;

    let output_lines = output.lines().collect::<Vec<_>>();

    if output_lines.len() >= MAX_LINES {
        // Create a output file to store the full output
        let output_file = format!(
            "{}.{:06x}.txt",
            file_prefix,
            rand::rng().random_range(0..=0xFFFFFF)
        );
        let output_file_path = match LocalStore::write_session_data(&output_file, output) {
            Ok(path) => path,
            Err(e) => {
                error!("Failed to write session data to {}: {}", output_file, e);
                return Err(McpError::internal_error(
                    "Failed to write session data",
                    Some(json!({ "error": e.to_string() })),
                ));
            }
        };

        Ok(format!(
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
        ))
    } else {
        Ok(output.to_string())
    }
}
