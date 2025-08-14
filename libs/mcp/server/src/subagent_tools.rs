use crate::tool_container::ToolContainer;
use rmcp::{
    Error as McpError, handler::server::tool::Parameters, model::*, schemars, tool, tool_router,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema, Clone)]
pub enum SubagentType {
    #[schemars(description = "General purpose agent")]
    GeneralPurpose,
}

impl std::fmt::Display for SubagentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SubagentType::GeneralPurpose => "GeneralPurpose",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TaskRequest {
    #[schemars(description = "A short (3-5 word) description of the task")]
    pub description: String,
    #[schemars(description = "The task for the agent to perform")]
    pub prompt: String,
    #[schemars(description = "The type of specialized agent to use for this task")]
    pub subagent_type: SubagentType,
}

#[tool_router(router = tool_router_subagent, vis = "pub")]
impl ToolContainer {
    #[tool(
        description = "Execute a task using a specialized subagent. This tool allows you to delegate specific tasks to specialized agents based on the task type and requirements.

PARAMETERS:
- description: A short (3-5 word) description of what the task accomplishes
- prompt: Detailed instructions for the agent to perform the task
- subagent_type: The type of specialized agent to use from the available options

AVAILABLE SUBAGENT TYPES:
- GeneralPurpose: General purpose task execution

USAGE:
Use this tool when you need to delegate a specific task to a specialized agent that can handle the requirements better than general-purpose processing. The subagent will execute the task according to the provided prompt and return the results.

"
    )]
    pub async fn task(
        &self,
        Parameters(TaskRequest {
            description,
            prompt,
            subagent_type,
        }): Parameters<TaskRequest>,
    ) -> Result<CallToolResult, McpError> {
        // TODO: Implement actual task delegation logic
        // This is a skeleton implementation that will be expanded later

        // For now, return a placeholder response indicating the task would be processed
        let response_message = format!(
            "Task '{}' would be delegated to subagent type '{}' with the following prompt:\n\n{}",
            description, subagent_type, prompt
        );

        // In a real implementation, this would:
        // 1. Validate the subagent_type
        // 2. Initialize the appropriate specialized agent
        // 3. Pass the prompt to the subagent
        // 4. Wait for the subagent to complete the task
        // 5. Return the results from the subagent

        Ok(CallToolResult::success(vec![Content::text(format!(
            "🤖 Task Skeleton Response\n\n{}\n\n⚠️  This is a skeleton implementation. The actual task delegation logic needs to be implemented.",
            response_message
        ))]))
    }
}
