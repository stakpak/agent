use std::path::Path;

use crate::tool_container::ToolContainer;
use rmcp::{
    ErrorData as McpError, RoleServer, handler::server::wrapper::Parameters, model::*, schemars,
    service::RequestContext, tool, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use stakpak_shared::local_store::LocalStore;
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
        ctx: RequestContext<RoleServer>,
        Parameters(TaskRequest {
            description,
            prompt,
            subagent_type,
        }): Parameters<TaskRequest>,
    ) -> Result<CallToolResult, McpError> {
        let session_id = self.get_session_id(&ctx);
        let subagent_command =
            match self.build_subagent_command(&prompt, &subagent_type, session_id.as_deref()) {
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

    fn build_subagent_command(
        &self,
        prompt: &str,
        subagent_type: &str,
        session_id: Option<&str>,
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
        let prompt_subpath = match session_id {
            Some(sid) => Path::new(sid)
                .join("subagents")
                .join(&prompt_filename)
                .to_string_lossy()
                .to_string(),
            None => Path::new("subagents")
                .join(&prompt_filename)
                .to_string_lossy()
                .to_string(),
        };
        let prompt_file_path =
            LocalStore::write_session_data(&prompt_subpath, prompt).map_err(|e| {
                McpError::internal_error(
                    "Failed to create prompt file",
                    Some(json!({"error": e.to_string()})),
                )
            })?;

        let mut command = format!(
            r#"stakpak -a --prompt-file {} --max-steps {}"#,
            prompt_file_path, subagent_config.max_steps
        );

        // Add model flag if specified
        if let Some(model) = &subagent_config.model {
            command.push_str(&format!(" --model {}", model));
        }

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
