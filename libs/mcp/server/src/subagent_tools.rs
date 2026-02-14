use std::env;
use std::path::Path;

use crate::tool_container::ToolContainer;
use rmcp::{
    ErrorData as McpError, RoleServer, handler::server::wrapper::Parameters, model::*, schemars,
    service::RequestContext, tool, tool_router,
};
use serde::Deserialize;
use serde_json::json;
use stakpak_shared::local_store::LocalStore;
use tracing::error;
use uuid::Uuid;

/// Request for creating a dynamic subagent with full control over its configuration.
/// Based on the AOrchestra 4-tuple model: (Instruction, Context, Tools, Model)
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DynamicSubagentRequest {
    /// A short (3-5 word) description of what the task accomplishes
    #[schemars(description = "A short (3-5 word) description of the task")]
    pub description: String,

    /// The task instructions - what the subagent should do (the "I" in the 4-tuple).
    /// Should be specific, actionable, and include success criteria.
    #[schemars(
        description = "The task instructions specifying what the subagent should accomplish. Be specific and include success criteria."
    )]
    pub instructions: String,

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
    ///
    /// Use this when the subagent needs to run potentially unsafe commands.
    #[schemars(
        description = "Enable sandbox mode for isolated execution. Runs subagent in a warden container with read-only filesystem access and security policies. Recommended when using run_command tool."
    )]
    #[serde(default)]
    pub enable_sandbox: bool,
}

/// Request for resuming a paused or completed subagent task.
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

/// Get the current executable path for spawning subagents
fn get_current_exe() -> Result<String, McpError> {
    env::current_exe()
        .map_err(|e| {
            McpError::internal_error(
                "Failed to get current executable path",
                Some(json!({"error": e.to_string()})),
            )
        })
        .map(|p| p.to_string_lossy().to_string())
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
- IMPORTANT: Sandbox subagents run AUTONOMOUSLY to completion without pausing for tool approval
  (non-sandbox subagents pause on mutating tools like run_command, requiring master agent approval)

SANDBOX + MUTATING TOOLS PATTERN:
When granting mutating tools (run_command, create, str_replace, remove) to a subagent,
enable sandbox mode so the subagent can execute autonomously while safely isolated.
Without sandbox, the subagent pauses on each mutating tool call waiting for approval,
which blocks progress. Read-only tools (view, search_docs, etc.) never require approval.

The subagent runs asynchronously. Use get_task_details to monitor progress."
    )]
    pub async fn dynamic_subagent_task(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(DynamicSubagentRequest {
            description,
            instructions,
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
        // Use the main agent's profile and config path: the CLI sets STAKPAK_PROFILE and
        // STAKPAK_CONFIG_PATH at startup; the MCP server (spawned by the same CLI) inherits them.
        let profile_name = std::env::var("STAKPAK_PROFILE").ok();
        let config_path = std::env::var("STAKPAK_CONFIG_PATH").ok();
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
            &instructions,
            context.as_deref(),
            &tools,
            &model,
            max_steps,
            enable_sandbox,
            session_id.as_deref(),
            profile_name.as_deref(),
            config_path.as_deref(),
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
        let task_description = if enable_sandbox {
            format!("{} [sandboxed]", description)
        } else {
            description.clone()
        };
        let task_info = match self
            .get_task_manager()
            .start_task(subagent_command, Some(task_description), None, None)
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

    /// Resume a paused or completed subagent task with approval decisions or follow-up input.
    #[tool(
        description = "Resume a paused or completed subagent task. Subagents pause when they need tool approval or user input.

PARAMETERS:
- task_id: The task ID of the paused subagent
- approve: List of tool call IDs to approve
- reject: List of tool call IDs to reject
- approve_all: Approve all pending tool calls
- reject_all: Reject all pending tool calls
- input: Text input to continue the conversation (for input_required pauses or completed tasks)

WORKFLOW:
1. Start subagent: dynamic_subagent_task — subagents automatically pause on tool approval
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

        // Get the current executable path for resuming
        let current_exe = get_current_exe()?;

        // Build the stakpak CLI command for resuming
        let mut command = format!("{} -a --output json -c {}", current_exe, checkpoint_id);

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

    /// Build command for dynamic subagent with full 4-tuple configuration
    #[allow(clippy::too_many_arguments)]
    fn build_dynamic_subagent_command(
        &self,
        instructions: &str,
        context: Option<&str>,
        tools: &[String],
        model: &str,
        max_steps: usize,
        enable_sandbox: bool,
        session_id: Option<&str>,
        profile_name: Option<&str>,
        config_path: Option<&str>,
    ) -> Result<String, McpError> {
        // Combine instruction and context into the prompt
        let full_prompt = match context {
            Some(ctx) if !ctx.is_empty() => {
                format!(
                    "=== CONTEXT (from previous work) ===\n{}\n\n=== YOUR TASK ===\n{}",
                    ctx, instructions
                )
            }
            _ => instructions.to_string(),
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

        // Get the current executable path to use for subagent
        // When running in sandbox (warden container), use "stakpak" since it's in PATH
        // Otherwise use the current executable path
        let current_exe = get_current_exe()?;
        let exe_for_command = if enable_sandbox {
            "stakpak".to_string()
        } else {
            current_exe.clone()
        };

        // Build the stakpak command arguments
        let mut args = vec![exe_for_command.clone()];

        // Add profile and config so subagent uses same profile/config as main agent
        if let Some(profile) = profile_name {
            args.extend(["--profile".to_string(), profile.to_string()]);
        }
        if let Some(path) = config_path {
            args.extend(["--config".to_string(), path.to_string()]);
        }

        args.push("-a".to_string());

        // --pause-on-approval only when NOT in sandbox mode
        if !enable_sandbox {
            args.push("--pause-on-approval".to_string());
        }

        args.extend([
            "--output".to_string(),
            "json".to_string(),
            "--prompt-file".to_string(),
            prompt_file_path.clone(),
            "--max-steps".to_string(),
            max_steps.to_string(),
            "--model".to_string(),
            model.to_string(),
        ]);

        // Add tool flags
        for tool in tools {
            args.push("-t".to_string());
            args.push(tool.clone());
        }

        let mut command = args.join(" ");

        // If sandbox mode is enabled, wrap the command in warden
        if enable_sandbox {
            // Use standard stakpak image (no special warden image needed with wrap)
            let stakpak_image = format!("ghcr.io/stakpak/agent:v{}", env!("CARGO_PKG_VERSION"));

            let mut warden_command = format!("{} warden wrap {}", current_exe, stakpak_image);

            // Mount the prompt file into the container
            let warden_prompt_path = format!("/tmp/{}", prompt_filename);
            warden_command.push_str(&format!(" -v {}:{}", prompt_file_path, warden_prompt_path));

            // When a config path was passed, the host path is not visible inside the container.
            // Mount the config file's parent dir so the subagent can read the same config.
            let config_mount = config_path.and_then(|p| {
                let path = Path::new(p);
                let parent = path.parent()?;
                let filename = path.file_name()?.to_string_lossy().into_owned();
                if parent.exists() {
                    Some((parent.to_path_buf(), filename))
                } else {
                    None
                }
            });
            if let Some((ref parent, ref _filename)) = config_mount {
                warden_command.push_str(&format!(
                    " -v {}:/agent/host-stakpak-config:ro",
                    parent.display()
                ));
            }
            let container_config_path = config_mount
                .map(|(_parent, filename)| format!("/agent/host-stakpak-config/{}", filename));

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

            // Replace host paths in the inner command with container paths
            let inner_command = command.replace(&prompt_file_path, &warden_prompt_path);
            let inner_command = if let (Some(host_cfg), Some(ref container_cfg)) =
                (config_path, container_config_path)
            {
                inner_command.replace(host_cfg, container_cfg)
            } else {
                inner_command
            };

            // wrap uses -- separator before the command
            command = format!("{} -- {}", warden_command, inner_command);
        }

        Ok(command)
    }
}
