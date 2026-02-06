use std::path::Path;

use crate::tool_container::ToolContainer;
use rmcp::{
    ErrorData as McpError, RoleServer, handler::server::wrapper::Parameters, model::*, schemars,
    service::RequestContext, tool, tool_router,
};
use serde::Deserialize;
use serde_json::json;
use stakpak_shared::local_store::LocalStore;
use uuid::Uuid;

/// Request for creating a dynamic subagent with full control over its configuration.
/// Based on the AOrchestra 4-tuple model: (Instruction, Context, Tools, Model)
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DynamicSubagentRequest {
    /// A short (3-5 word) description of what the task accomplishes
    #[schemars(description = "A short (3-5 word) description of the task")]
    pub description: String,

    /// The task instruction - what the subagent should do (the "I" in the 4-tuple).
    /// Should be specific, actionable, and include success criteria.
    #[schemars(
        description = "The task instruction specifying what the subagent should accomplish. Be specific and include success criteria."
    )]
    pub instruction: String,

    /// Curated context from previous work (the "C" in the 4-tuple).
    /// Include: relevant findings, key artifacts/references, what didn't work.
    /// Exclude: full conversation history, irrelevant tangents, raw tool outputs.
    #[schemars(
        description = "Curated context from previous attempts/findings. Include: relevant discoveries, key references (file paths, URLs, IDs), failed approaches to avoid. Keep concise - don't pass full history."
    )]
    pub context: Option<String>,

    /// Tools to grant the subagent (the "T" in the 4-tuple).
    /// Follow least-privilege: only include tools necessary for the task.
    /// Use tool names like: stakpak__view, stakpak__run_command, stakpak__search_docs, etc.
    #[schemars(
        description = "Array of tool names to grant the subagent. Follow least-privilege principle - only include tools necessary for the task. Examples: stakpak__view, stakpak__run_command, stakpak__search_docs, stakpak__str_replace"
    )]
    pub tools: Vec<String>,

    // /// Model to use (the "M" in the 4-tuple).
    // #[schemars(
    //     description = "Model selection: small cheap models for fast/exploratory/research tasks or large more expensive models for complex reasoning"
    // )]
    // pub model_id: Option<String>,
    /// Maximum steps the subagent can take (default: 30)
    #[schemars(description = "Maximum steps the subagent can take (default: 30)")]
    pub max_steps: Option<usize>,

    /// Enable sandbox mode using warden container isolation.
    /// When enabled, the subagent runs in an isolated Docker container with:
    /// - Read-only access to the current working directory
    /// - Read-only access to cloud credentials (AWS, GCP, Azure, etc.)
    /// - Network isolation and security policies
    /// Use this when the subagent needs to run potentially unsafe commands.
    #[schemars(
        description = "Enable sandbox mode for isolated execution. Runs subagent in a warden container with read-only filesystem access and security policies. Recommended when using run_command tool."
    )]
    #[serde(default)]
    pub enable_sandbox: bool,
}

#[tool_router(router = tool_router_subagent, vis = "pub")]
impl ToolContainer {
    /// Create and execute a dynamic subagent with full control over its configuration.
    /// Based on the AOrchestra 4-tuple model: (Instruction, Context, Tools, Model)
    #[tool(
        description = "Create a dynamic subagent with full control over its configuration. This implements the AOrchestra 4-tuple model (Instruction, Context, Tools, Model) for on-demand agent specialization.

PARAMETERS:
- description: A short (3-5 word) description of the task
- instruction: What the subagent should do - be specific and include success criteria
- context: (Optional) Curated context from previous work - include relevant findings, key references, failed approaches
- tools: Array of tool names to grant (follow least-privilege - minimum tools required)
- max_steps: (Optional) Maximum steps, default 30
- enable_sandbox: (Optional) Run in isolated warden container with security policies

WHEN TO USE:
- When you need fine-grained control over subagent capabilities
- When passing context from previous attempts would help
- When the pre-defined subagent types don't fit your needs

CONTEXT GUIDELINES (the key differentiator):
Include:
- Relevant findings from previous attempts ('Found that config is in /etc/app/config.yaml')
- Key references discovered (file paths, URLs, IDs, names)
- Failed approaches to avoid ('API v1 endpoint returned 404, use v2')
- Constraints or clarifications

Exclude:
- Full conversation history (causes context degradation)
- Raw tool outputs (summarize instead)
- Irrelevant tangents from other subtasks

TOOL SELECTION (least-privilege):
- Always prefer read only tools / tasks for subagents

SANDBOX MODE (enable_sandbox=true):
- Runs subagent in isolated Docker container via warden
- Read-only access to working directory and cloud credentials
- Recommended when using run_command tool for untrusted operations
- Adds ~5-10s startup overhead for container initialization

The subagent runs asynchronously. Use get_task_details to monitor progress."
    )]
    pub async fn dynamic_subagent_task(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(DynamicSubagentRequest {
            description,
            instruction,
            context,
            tools,
            max_steps,
            enable_sandbox,
        }): Parameters<DynamicSubagentRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Validate tools array is not empty
        if tools.is_empty() {
            return Ok(CallToolResult::error(vec![Content::text(
                "VALIDATION_ERROR: tools array cannot be empty. Provide at least one tool for the subagent.",
            )]));
        }

        let session_id = self.get_session_id(&ctx);
        let max_steps = max_steps.unwrap_or(30);

        let model = if let Some(serde_json::Value::String(model_id)) = ctx.meta.get("model_id") {
            if model_id.contains("claude-opus-4-6") {
                model_id.replace("opus-4-6", "haiku-4-5")
            } else if model_id.contains("claude-opus") {
                model_id.replace("opus", "haiku")
            } else if model_id.contains("claude-sonnet") {
                model_id.replace("sonnet", "haiku")
            } else {
                model_id.clone()
            }
        } else {
            "claude-haiku-4-5".to_string()
        };

        // Build the dynamic subagent command
        let subagent_command = match self.build_dynamic_subagent_command(
            &instruction,
            context.as_deref(),
            &tools,
            &model,
            max_steps,
            enable_sandbox,
            session_id.as_deref(),
        ) {
            Ok(command) => command,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "COMMAND_BUILD_FAILED: Failed to build dynamic subagent command: {}",
                    e
                ))]));
            }
        };

        // Start the subagent as a background task
        let task_info = match self
            .get_task_manager()
            .start_task(subagent_command, Some(description.clone()), None, None)
            .await
        {
            Ok(task_info) => task_info,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "TASK_START_FAILED: Failed to start dynamic subagent task: {}",
                    e
                ))]));
            }
        };

        // Format tools list for display
        let tools_display = tools.join(", ");
        let context_display = context
            .as_ref()
            .map(|c| format!("\nContext: {} chars provided", c.len()))
            .unwrap_or_default();
        let sandbox_display = if enable_sandbox {
            "\nSandbox: enabled (warden isolation)"
        } else {
            ""
        };

        Ok(CallToolResult::success(vec![Content::text(format!(
            "🤖 Dynamic Subagent Created\n\n\
            Task ID: {}\n\
            Description: {}\n\
            Model: {}\n\
            Tools: [{}]\n\
            Max Steps: {}{}{}\n\
            Status: {:?}\n\n\
            The subagent is now running in the background with the specified configuration.\n\
            Use get_task_details to monitor progress and get results.",
            task_info.id,
            description,
            model,
            tools_display,
            max_steps,
            context_display,
            sandbox_display,
            task_info.status
        ))]))
    }

    /// Build command for dynamic subagent with full 4-tuple configuration
    fn build_dynamic_subagent_command(
        &self,
        instruction: &str,
        context: Option<&str>,
        tools: &[String],
        model: &str,
        max_steps: usize,
        enable_sandbox: bool,
        session_id: Option<&str>,
    ) -> Result<String, McpError> {
        // Combine instruction and context into the prompt
        let full_prompt = match context {
            Some(ctx) if !ctx.is_empty() => {
                format!(
                    "=== CONTEXT (from previous work) ===\n{}\n\n=== YOUR TASK ===\n{}",
                    ctx, instruction
                )
            }
            _ => instruction.to_string(),
        };

        // Write prompt to file
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

        let prompt_file_path = LocalStore::write_session_data(&prompt_subpath, &full_prompt)
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to create prompt file",
                    Some(json!({"error": e.to_string()})),
                )
            })?;

        // Build the base stakpak command
        let mut command = format!(
            r#"stakpak -a --prompt-file {} --max-steps {} --model {}"#,
            prompt_file_path, max_steps, model
        );

        // Add each tool
        for tool in tools {
            command.push_str(&format!(" -t {}", tool));
        }

        // If sandbox mode is enabled, wrap the command in warden
        if enable_sandbox {
            let stakpak_image = format!(
                "ghcr.io/stakpak/agent-warden:v{}",
                env!("CARGO_PKG_VERSION")
            );

            let mut warden_command = format!("stakpak warden run --image {}", stakpak_image);

            // Mount the prompt file into the container
            let warden_prompt_path = format!("/tmp/{}", prompt_filename);
            warden_command.push_str(&format!(" -v {}:{}", prompt_file_path, warden_prompt_path));

            // Add default sandbox volumes for read-only access
            // Working directory (read-only)
            warden_command.push_str(" -v ./:/agent:ro");
            // Session data directory (read-write for subagent state)
            warden_command.push_str(" -v ./.stakpak:/agent/.stakpak");

            // Cloud credentials (read-only) - only mount if they exist
            let cloud_volumes = [
                ("~/.aws", "/home/agent/.aws:ro"),
                ("~/.config/gcloud", "/home/agent/.config/gcloud:ro"),
                ("~/.azure", "/home/agent/.azure:ro"),
                ("~/.kube", "/home/agent/.kube:ro"),
            ];

            for (host_path, container_path) in cloud_volumes {
                // Expand ~ to home directory
                let expanded_path = if host_path.starts_with("~/") {
                    if let Ok(home) = std::env::var("HOME") {
                        host_path.replacen("~", &home, 1)
                    } else {
                        continue;
                    }
                } else {
                    host_path.to_string()
                };

                // Only add volume if the path exists
                if Path::new(&expanded_path).exists() {
                    warden_command.push_str(&format!(" -v {}:{}", expanded_path, container_path));
                }
            }

            // Wrap the stakpak command, replacing the prompt path with the container path
            command = format!(
                "{} '{}'",
                warden_command,
                command.replace(&prompt_file_path, &warden_prompt_path)
            );
        }

        Ok(command)
    }
}
