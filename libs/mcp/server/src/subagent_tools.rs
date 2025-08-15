use crate::tool_container::ToolContainer;
use rmcp::{
    Error as McpError, handler::server::tool::Parameters, model::*, schemars, tool, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use stakpak_shared::local_store::LocalStore;
use std::collections::HashMap;
use std::sync::OnceLock;
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema, Clone)]
pub struct SubagentType(pub String);

impl std::fmt::Display for SubagentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubagentConfig {
    pub max_steps: usize,
    pub allowed_tools: Vec<String>,
}

// Global cache for subagent configurations loaded at compile time
static SUBAGENT_CONFIGS: &str = include_str!("../subagents.toml");
static PARSED_CONFIGS: OnceLock<HashMap<String, SubagentConfig>> = OnceLock::new();

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

The subagent runs asynchronously in the background. This tool returns immediately with a task ID that you can use to monitor progress and get results using the get_task_details and get_all_tasks tools.

AVAILABLE SUBAGENT TYPES:
- ResearchAgent: General purpose research agent focused on research and analysis tasks
  - Max steps: 10
  - Available tools: view, local_code_search, search_docs, search_memory, read_rulebook

EXAMPLES:
- description: 'Generate API documentation', prompt: 'Create comprehensive API docs for the user service endpoints', subagent_type: 'ResearchAgent'
- description: 'Code review analysis', prompt: 'Review the authentication module for security vulnerabilities', subagent_type: 'ResearchAgent'
- description: 'Database optimization', prompt: 'Analyze and optimize the user queries for better performance', subagent_type: 'ResearchAgent'"
    )]
    pub async fn task(
        &self,
        Parameters(TaskRequest {
            description,
            prompt,
            subagent_type,
        }): Parameters<TaskRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Build the subagent execution command that will run in the background
        let subagent_type = SubagentType(subagent_type);
        let subagent_command = self.build_subagent_command(&prompt, &subagent_type)?;

        // Start the subagent as a background task using existing task manager
        let task_info = self
            .get_task_manager()
            .start_task(subagent_command, None, None) // No timeout, no remote
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to start subagent task",
                    Some(json!({"error": e.to_string()})),
                )
            })?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "🤖 Subagent Task Started\n\nTask ID: {}\nDescription: {}\nAgent Type: {}\nStatus: {:?}\n\nThe subagent is now running in the background. Use get_task_details to monitor progress and get results.",
            task_info.id, description, subagent_type, task_info.status
        ))]))
    }

    fn build_subagent_command(
        &self,
        prompt: &str,
        subagent_type: &SubagentType,
    ) -> Result<String, McpError> {
        // Get specialized configuration for this subagent type
        let subagent_config = self.get_subagent_config(subagent_type)?;

        // Write prompt to a temporary file
        let prompt_filename = format!("subagent_prompt_{}.txt", Uuid::new_v4());
        let prompt_file_path =
            LocalStore::write_session_data(&prompt_filename, prompt).map_err(|e| {
                McpError::internal_error(
                    "Failed to create prompt file",
                    Some(json!({"error": e.to_string()})),
                )
            })?;

        let mut command = format!(
            r#"stakpak --async --output text --max-steps {} --prompt-file {}"#,
            subagent_config.max_steps, prompt_file_path
        );

        for tool in &subagent_config.allowed_tools {
            command.push_str(&format!(" -t {}", tool));
        }

        Ok(command)
    }

    fn get_subagent_config(
        &self,
        subagent_type: &SubagentType,
    ) -> Result<SubagentConfig, McpError> {
        let configs = get_subagent_configs();
        configs.get(&subagent_type.0).cloned().ok_or_else(|| {
            McpError::internal_error(
                "Unknown subagent type",
                Some(json!({
                    "subagent_type": subagent_type.0,
                    "available_types": configs.keys().collect::<Vec<_>>()
                })),
            )
        })
    }
}

/// Parse subagent configurations from the embedded TOML content
fn parse_subagent_configs() -> Result<HashMap<String, SubagentConfig>, McpError> {
    toml::from_str(SUBAGENT_CONFIGS).map_err(|e| {
        McpError::internal_error(
            "Failed to parse embedded subagent config TOML",
            Some(json!({"error": e.to_string()})),
        )
    })
}

/// Get the cached subagent configurations
fn get_subagent_configs() -> &'static HashMap<String, SubagentConfig> {
    PARSED_CONFIGS.get_or_init(|| parse_subagent_configs().unwrap_or_default())
}

/// Get the list of available subagent types
pub fn get_available_subagent_types() -> Vec<String> {
    get_subagent_configs().keys().cloned().collect()
}
