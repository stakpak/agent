use std::path::Path;

use crate::tool_container::ToolContainer;
use rmcp::{
    ErrorData as McpError, handler::server::wrapper::Parameters, model::*, schemars, tool,
    tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use stakpak_shared::local_store::LocalStore;
use tracing::error;
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema, Clone)]
pub struct SubagentType(pub String);

impl std::fmt::Display for SubagentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubagentResult {
    pub success: bool,
    pub description: String,
    pub agent_type: SubagentType,
    pub steps_taken: usize,
    pub artifacts_generated: Vec<String>,
    pub final_response: String,
    pub execution_time_seconds: f64,
    pub checkpoint_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TaskRequest {
    #[schemars(description = "A short (3-5 word) description of the task")]
    pub description: String,
    #[schemars(description = "The task for the agent to perform")]
    pub prompt: String,
    #[schemars(description = "The type of specialized agent to use for this task")]
    pub subagent_type: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ResumeSubagentTaskRequest {
    #[schemars(description = "The task ID of the paused subagent task to resume")]
    pub task_id: String,
    #[schemars(
        description = "Tool call IDs to approve (e.g., [\"tc_1\", \"tc_2\"]). Unspecified tool calls are rejected."
    )]
    pub approve: Option<Vec<String>>,
    #[schemars(description = "Tool call IDs to explicitly reject (e.g., [\"tc_3\"])")]
    pub reject: Option<Vec<String>>,
    #[schemars(
        description = "Approve all pending tool calls (overrides individual approve/reject)"
    )]
    pub approve_all: Option<bool>,
    #[schemars(description = "Reject all pending tool calls")]
    pub reject_all: Option<bool>,
    #[schemars(
        description = "Text input to provide when the subagent paused for input (input_required pause reason)"
    )]
    pub input: Option<String>,
}

#[tool_router(router = tool_router_subagent, vis = "pub")]
impl ToolContainer {
    #[tool(
        description = "Execute a task using a specialized subagent. This tool allows you to delegate specific tasks to specialized agents based on the task type and requirements.

PARAMETERS:
- description: A short (3-5 word) description of what the task accomplishes
- prompt: Detailed instructions for the agent to perform the task
- subagent_type: The type of specialized agent to use from the available options

USAGE:
Use this tool when you need to delegate a specific task to a specialized agent that can handle the requirements better than general-purpose processing. The subagent will execute the task according to the provided prompt and return the results.

The subagent runs asynchronously in the background. This tool returns immediately with a task ID that you can use to monitor progress and get results using the get_task_details and get_all_tasks tools."
    )]
    pub async fn subagent_task(
        &self,
        Parameters(TaskRequest {
            description,
            prompt,
            subagent_type,
        }): Parameters<TaskRequest>,
    ) -> Result<CallToolResult, McpError> {
        let subagent_command = match self.build_subagent_command(&prompt, &subagent_type) {
            Ok(command) => command,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "COMMAND_BUILD_FAILED: Failed to build subagent command: {}",
                    e
                ))]));
            }
        };

        // Start the subagent as a background task using existing task manager
        let task_info = match self
            .get_task_manager()
            .start_task(subagent_command, None, None) // No timeout, no remote
            .await
        {
            Ok(task_info) => task_info,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "TASK_START_FAILED: Failed to start subagent task: {}",
                    e
                ))]));
            }
        };

        Ok(CallToolResult::success(vec![Content::text(format!(
            "🤖 Subagent Task Started\n\nTask ID: {}\nDescription: {}\nAgent Type: {}\nStatus: {:?}\n\nThe subagent is now running in the background. Use get_task_details to monitor progress and get results.",
            task_info.id, description, subagent_type, task_info.status
        ))]))
    }

    #[tool(
        description = "Resume a paused or completed subagent task. Subagent tasks pause with exit code 10 when they need tool approval or user input. Completed subagent tasks can also be resumed to continue the conversation (e.g., provide follow-up instructions or clarification).

PARAMETERS:
- task_id: The task ID of the paused or completed subagent task to resume
- approve: List of tool call IDs to approve. Unspecified tool calls are automatically rejected.
- reject: List of tool call IDs to explicitly reject
- approve_all: Approve all pending tool calls (default: false)
- reject_all: Reject all pending tool calls (default: false)
- input: Text input to continue the conversation (for input_required pauses or completed tasks)

WORKFLOW:
1. Start subagent: subagent_task(prompt, type) — subagents automatically pause on tool approval
2. Monitor with get_task_details — check for status 'Paused' or 'Completed'
3. Read pause_info.raw_output to see pending_tool_calls or the agent's message
4. Resume with approval decisions or follow-up input
5. The subagent continues execution from where it stopped

NOTES:
- Works on tasks with status 'Paused' or 'Completed'
- The checkpoint ID is automatically extracted from the task's internal state
- For tool_approval_required pauses: use approve/reject/approve_all/reject_all
- For input_required pauses or completed tasks: use the input parameter
- Unspecified tool calls are rejected by default"
    )]
    pub async fn resume_subagent_task(
        &self,
        Parameters(ResumeSubagentTaskRequest {
            task_id,
            approve,
            reject,
            approve_all,
            reject_all,
            input,
        }): Parameters<ResumeSubagentTaskRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Look up the paused task to extract checkpoint_id from pause_info
        let task_info = self
            .get_task_manager()
            .get_task_details(task_id.clone())
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to get task details",
                    Some(json!({"error": e.to_string()})),
                )
            })?
            .ok_or_else(|| {
                McpError::invalid_params("Task not found", Some(json!({"task_id": task_id})))
            })?;

        if !matches!(
            task_info.status,
            stakpak_shared::task_manager::TaskStatus::Paused
                | stakpak_shared::task_manager::TaskStatus::Completed
        ) {
            return Ok(CallToolResult::error(vec![
                Content::text("RESUME_TASK_ERROR"),
                Content::text(format!(
                    "Task '{}' cannot be resumed (status: {:?}). Only paused or completed tasks can be resumed.",
                    task_id, task_info.status
                )),
            ]));
        }

        let checkpoint_id = task_info
            .pause_info
            .as_ref()
            .and_then(|pi| pi.checkpoint_id.as_ref())
            .ok_or_else(|| {
                McpError::internal_error(
                    "Paused task has no checkpoint ID in pause_info",
                    Some(json!({"task_id": task_id})),
                )
            })?;

        // Build the stakpak CLI command for resuming
        let mut command = format!("stakpak -a --output json -c {}", checkpoint_id);

        if approve_all.unwrap_or(false) {
            command.push_str(" --approve-all");
        }
        if reject_all.unwrap_or(false) {
            command.push_str(" --reject-all");
        }
        if let Some(approve_ids) = &approve {
            for id in approve_ids {
                command.push_str(&format!(" --approve {}", id));
            }
        }
        if let Some(reject_ids) = &reject {
            for id in reject_ids {
                command.push_str(&format!(" --reject {}", id));
            }
        }
        if let Some(input_text) = &input {
            // Write input to a temp file and pass via --prompt-file to avoid shell escaping issues
            let input_filename = format!("resume_input_{}.txt", Uuid::new_v4());
            match LocalStore::write_session_data(
                &format!("subagents/{}", input_filename),
                input_text,
            ) {
                Ok(path) => {
                    command.push_str(&format!(" --prompt-file {}", path));
                }
                Err(e) => {
                    return Ok(CallToolResult::error(vec![
                        Content::text("RESUME_TASK_ERROR"),
                        Content::text(format!("Failed to write input file: {}", e)),
                    ]));
                }
            }
        }

        match self
            .get_task_manager()
            .resume_task(task_id.clone(), command.clone())
            .await
        {
            Ok(task_info) => Ok(CallToolResult::success(vec![Content::text(format!(
                "🤖 Subagent Task Resumed\n\nTask ID: {}\nStatus: {:?}\n\nThe subagent is now running. Use get_task_details to monitor progress.",
                task_info.id, task_info.status
            ))])),
            Err(e) => {
                error!("Failed to resume subagent task: {}", e);

                Ok(CallToolResult::error(vec![
                    Content::text("RESUME_TASK_ERROR"),
                    Content::text(format!("Failed to resume subagent task: {}", e)),
                ]))
            }
        }
    }

    fn build_subagent_command(
        &self,
        prompt: &str,
        subagent_type: &str,
    ) -> Result<String, McpError> {
        let subagent_config = if let Some(subagent_configs) = self.get_subagent_configs() {
            subagent_configs.get_config(subagent_type)
        } else {
            None
        }
        .ok_or_else(|| {
            McpError::internal_error(
                "Unknown subagent type",
                Some(json!({"subagent_type": subagent_type})),
            )
        })?;

        let prompt_filename = format!("prompt_{}.txt", Uuid::new_v4());
        let prompt_file_path = LocalStore::write_session_data(
            Path::new("subagents")
                .join(&prompt_filename)
                .to_string_lossy()
                .as_ref(),
            prompt,
        )
        .map_err(|e| {
            McpError::internal_error(
                "Failed to create prompt file",
                Some(json!({"error": e.to_string()})),
            )
        })?;

        let mut command = format!(
            r#"stakpak -a --pause-on-approval --output json --prompt-file {} --max-steps {}"#,
            prompt_file_path, subagent_config.max_steps
        );

        for tool in &subagent_config.allowed_tools {
            command.push_str(&format!(" -t {}", tool));
        }

        if let Some(warden) = &subagent_config.warden
            && warden.enabled
        {
            let stakpak_image = format!(
                "ghcr.io/stakpak/agent-warden:v{}",
                env!("CARGO_PKG_VERSION")
            );

            let mut warden_command = format!("stakpak warden run --image {}", stakpak_image);

            let warden_prompt_path = format!("/tmp/{}", prompt_filename);

            warden_command.push_str(&format!(" -v {}:{}", prompt_file_path, warden_prompt_path));

            for volume in &warden.volumes {
                warden_command.push_str(&format!(" -v {}", volume));
            }

            command = format!(
                "{} '{}'",
                warden_command,
                command.replace(&prompt_file_path, &warden_prompt_path)
            );
        }

        Ok(command)
    }
}
