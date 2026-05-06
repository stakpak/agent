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
use stakpak_shared::task_manager::StartTaskOptions;
use tracing::{error, info};
use uuid::Uuid;

/// Default config path inside container (matches ~/.stakpak/config.toml convention).
const CONTAINER_CONFIG_PATH: &str = "/agent/.stakpak/config.toml";

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

    /// Model to use (the "M" in the 4-tuple).
    /// Not exposed to the LLM — resolved automatically from profile config,
    /// built-in defaults for the parent provider, or inherited from the parent model.
    #[serde(default, skip_deserializing)]
    #[schemars(skip)]
    pub model: Option<String>,

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

MODEL RESOLUTION:
1. Profile [subagent].model setting
2. Built-in default for the parent provider
3. Parent model verbatim (silent inherit)

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
- The host working directory is bind-mounted read-only at /agent inside the container
  (the subagent's CWD will be /agent, not the original host path — use relative paths or /agent/...)
- Cloud credentials (~/.aws, ~/.kube, ~/.ssh, etc.) are mounted under /home/agent/
- .stakpak session data is mounted writable at /agent/.stakpak
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
            model,
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
        // Use the main agent's profile and config path, passed explicitly through config structs.
        let profile_name = self.subagent_config.profile_name.clone();
        let config_path = self.subagent_config.config_path.clone();
        let configured_model = self.subagent_config.model.clone();
        let max_steps = max_steps.unwrap_or(30);

        let parent_model_id = ctx
            .meta
            .get("model_id")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);

        let parent_provider = ctx
            .meta
            .get("model_provider")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);

        let resolved_model = resolve_subagent_model(
            model.as_deref(),
            configured_model.as_deref(),
            parent_provider.as_deref(),
            parent_model_id.as_deref(),
        );

        // Build the dynamic subagent command
        let subagent_command = match self.build_dynamic_subagent_command(
            &instructions,
            context.as_deref(),
            &tools,
            resolved_model.model.as_deref(),
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
            .start_task(
                subagent_command,
                StartTaskOptions {
                    description: Some(task_description),
                    ..StartTaskOptions::default()
                },
            )
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

        info!(
            parent = parent_model_id.as_deref().unwrap_or_default(),
            provider = parent_provider.as_deref().unwrap_or_default(),
            step = resolved_model.step,
            resolved = resolved_model.model.as_deref().unwrap_or_default(),
            task_id = %task_info.id,
            "resolved subagent model"
        );

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
        let model_display = resolved_model
            .model
            .unwrap_or_else(|| "(inherited from parent context)".to_string());

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
            model_display,
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
        model: Option<&str>,
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
        let mut args = vec![exe_for_command.clone(), "-a".to_string()];

        // Add profile and config so subagent uses same profile/config as main agent (skip empty to avoid broken command)
        if let Some(profile) = profile_name.filter(|p| !p.is_empty()) {
            args.extend(["--profile".to_string(), profile.to_string()]);
        }
        if let Some(path) = config_path.filter(|p| !p.is_empty()) {
            args.extend(["--config".to_string(), path.to_string()]);
        }

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
        ]);

        if let Some(model) = model.filter(|m| !m.is_empty()) {
            args.extend(["--model".to_string(), model.to_string()]);
        }

        // Add tool flags
        for tool in tools {
            args.push("-t".to_string());
            args.push(tool.clone());
        }

        let mut command = args
            .iter()
            .map(|arg| shell_quote_arg(arg))
            .collect::<Vec<_>>()
            .join(" ");

        // If sandbox mode is enabled, wrap the command in warden.
        //
        // NOTE: We only add subagent-specific volumes here (prompt file, config
        // overlay).  All default mounts (cloud creds, SSH, working dir, aqua
        // cache, etc.) are handled by the `warden wrap` CLI handler which calls
        // `prepare_volumes()` → `stakpak_agent_default_mounts()` automatically.
        if enable_sandbox {
            use stakpak_shared::container::{
                ensure_named_volumes_exist, resolve_ak_store_for_sandbox, stakpak_agent_image,
                warden_ak_store_args,
            };

            ensure_named_volumes_exist();

            let host_knowledge_root = resolve_ak_store_for_sandbox().map_err(|e| {
                McpError::internal_error(
                    "AK_STORE could not be resolved for sandboxed subagent",
                    Some(json!({ "error": e })),
                )
            })?;

            // Pre-create ~/.stakpak/knowledge on the host so docker doesn't
            // materialize the bind-mount source as root-owned at first launch.
            // The AK_STORE-override branch is already create_dir_all'd inside
            // resolve_ak_store_for_sandbox().
            if host_knowledge_root.is_none()
                && let Ok(home) = env::var("HOME")
            {
                let default_root = Path::new(&home).join(".stakpak/knowledge");
                if let Err(e) = std::fs::create_dir_all(&default_root) {
                    error!(
                        "failed to pre-create host knowledge dir {}: {}",
                        default_root.display(),
                        e
                    );
                }
            }

            let stakpak_image = stakpak_agent_image();

            let mut warden_command = format!(
                "{} warden wrap {}",
                shell_quote_arg(&current_exe),
                shell_quote_arg(&stakpak_image)
            );

            let warden_prompt_path = format!("/tmp/{}", prompt_filename);
            warden_command.push_str(&format!(
                " -v {}",
                shell_quote_arg(&format!("{}:{}", prompt_file_path, warden_prompt_path))
            ));

            // Profile-overlay precedence: user `-v` flags are appended after
            // prepare_volumes() defaults, so this overlay wins.
            let container_config_path = config_path.and_then(|p| {
                let path = Path::new(p);
                if path.exists() && path.is_file() {
                    warden_command.push_str(&format!(
                        " -v {}",
                        shell_quote_arg(&format!(
                            "{}:{}:ro",
                            path.display(),
                            CONTAINER_CONFIG_PATH
                        ))
                    ));
                    Some(CONTAINER_CONFIG_PATH.to_string())
                } else {
                    None
                }
            });

            for arg in warden_ak_store_args(host_knowledge_root.as_deref()) {
                warden_command.push(' ');
                warden_command.push_str(&shell_quote_arg(&arg));
            }

            let inner_command = command.replace(&prompt_file_path, &warden_prompt_path);
            let inner_command = if let (Some(host_cfg), Some(ref container_cfg)) =
                (config_path, container_config_path)
            {
                inner_command.replace(host_cfg, container_cfg)
            } else {
                inner_command
            };

            command = format!("{} -- {}", warden_command, inner_command);
        }

        Ok(command)
    }
}

struct ResolvedModel {
    model: Option<String>,
    step: &'static str,
}

fn trimmed_non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn shell_quote_arg(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':' | b'=')
    }) {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

fn default_subagent_model(provider: &str) -> Option<&'static str> {
    match provider {
        "anthropic" => Some("anthropic/claude-haiku-4-5"),
        "openai" => Some("openai/gpt-5-mini"),
        "google" | "gemini" => Some("gemini/gemini-2.0-flash"),
        "amazon-bedrock" => Some("amazon-bedrock/claude-haiku-4-5"),
        "stakpak" => Some("stakpak/anthropic/claude-haiku-4-5"),
        "github-copilot" => None,
        _ => None,
    }
}

fn resolve_subagent_model(
    request_model: Option<&str>,
    configured_model: Option<&str>,
    parent_provider: Option<&str>,
    parent_model_id: Option<&str>,
) -> ResolvedModel {
    if let Some(model) = trimmed_non_empty(request_model) {
        return ResolvedModel {
            model: Some(model.to_string()),
            step: "caller_override",
        };
    }

    if let Some(model) = trimmed_non_empty(configured_model) {
        return ResolvedModel {
            model: Some(model.to_string()),
            step: "profile_config",
        };
    }

    if let Some(provider) = trimmed_non_empty(parent_provider)
        && let Some(model) = default_subagent_model(provider)
    {
        return ResolvedModel {
            model: Some(model.to_string()),
            step: "builtin_default",
        };
    }

    let inherited = match (
        trimmed_non_empty(parent_provider),
        trimmed_non_empty(parent_model_id),
    ) {
        (Some(provider), Some(model_id)) => Some(format!("{provider}/{model_id}")),
        (None, Some(model_id)) => Some(model_id.to_string()),
        _ => None,
    };

    ResolvedModel {
        model: inherited,
        step: "parent_inherit",
    }
}

#[cfg(test)]
mod tests {
    use super::{DynamicSubagentRequest, default_subagent_model, resolve_subagent_model};
    use crate::{EnabledToolsConfig, SubagentConfig, ToolContainer};
    use stakpak_shared::task_manager::TaskManager;

    #[test]
    fn deserialized_dynamic_subagent_request_ignores_caller_model() {
        let request: DynamicSubagentRequest = serde_json::from_value(serde_json::json!({
            "description": "probe",
            "instructions": "do the thing",
            "tools": ["stakpak__view"],
            "model": "evil/model; touch /tmp/pwned",
            "max_steps": 1,
            "enable_sandbox": false
        }))
        .expect("request should deserialize even when model is supplied");

        assert_eq!(request.model, None);
    }

    #[test]
    fn dynamic_subagent_tool_description_does_not_expose_model_parameter() {
        let source = include_str!("subagent_tools.rs");

        assert!(!source.contains(&["- ", "model:"].concat()));
        assert!(!source.contains(&["Caller-supplied", " model override"].concat()));
    }

    #[test]
    fn dynamic_subagent_command_shell_quotes_model_and_tools() {
        let task_manager = TaskManager::new();
        let container = ToolContainer::new(
            None,
            EnabledToolsConfig::default(),
            task_manager.handle(),
            ToolContainer::tool_router_subagent(),
            Vec::new(),
            SubagentConfig::default(),
        )
        .expect("tool container should be constructed");

        let command = container
            .build_dynamic_subagent_command(
                "do the thing",
                None,
                &["stakpak__view; touch /tmp/tool_pwn".to_string()],
                Some("provider/model; touch /tmp/model_pwn"),
                1,
                false,
                None,
                None,
                None,
            )
            .expect("subagent command should be built");

        assert!(command.contains("--model 'provider/model; touch /tmp/model_pwn'"));
        assert!(command.contains("-t 'stakpak__view; touch /tmp/tool_pwn'"));
    }

    #[test]
    fn caller_override_wins_verbatim() {
        let resolved = resolve_subagent_model(
            Some("anthropic/claude-haiku-4-5"),
            Some("openai/gpt-5-mini"),
            Some("anthropic"),
            Some("claude-opus-4-7"),
        );

        assert_eq!(
            resolved.model.as_deref(),
            Some("anthropic/claude-haiku-4-5")
        );
        assert_eq!(resolved.step, "caller_override");
    }

    #[test]
    fn profile_model_wins_when_caller_is_silent() {
        let resolved = resolve_subagent_model(
            None,
            Some("anthropic/claude-haiku-4-5"),
            Some("openai"),
            Some("gpt-4o"),
        );

        assert_eq!(
            resolved.model.as_deref(),
            Some("anthropic/claude-haiku-4-5")
        );
        assert_eq!(resolved.step, "profile_config");
    }

    #[test]
    fn anthropic_parent_uses_builtin_default() {
        let resolved =
            resolve_subagent_model(None, None, Some("anthropic"), Some("claude-opus-4-7"));

        assert_eq!(
            resolved.model.as_deref(),
            Some("anthropic/claude-haiku-4-5")
        );
        assert_eq!(resolved.step, "builtin_default");
    }

    #[test]
    fn openai_gemini_bedrock_and_stakpak_parents_use_builtin_defaults() {
        assert_eq!(default_subagent_model("openai"), Some("openai/gpt-5-mini"));
        assert_eq!(
            default_subagent_model("google"),
            Some("gemini/gemini-2.0-flash")
        );
        assert_eq!(
            default_subagent_model("gemini"),
            Some("gemini/gemini-2.0-flash")
        );
        assert_eq!(
            default_subagent_model("amazon-bedrock"),
            Some("amazon-bedrock/claude-haiku-4-5")
        );
        assert_eq!(
            default_subagent_model("stakpak"),
            Some("stakpak/anthropic/claude-haiku-4-5")
        );
    }

    #[test]
    fn unknown_provider_inherits_parent_model() {
        let resolved = resolve_subagent_model(
            None,
            None,
            Some("litellm-custom"),
            Some("my-org/my-model-v3"),
        );

        assert_eq!(
            resolved.model.as_deref(),
            Some("litellm-custom/my-org/my-model-v3")
        );
        assert_eq!(resolved.step, "parent_inherit");
    }

    #[test]
    fn future_claude_versions_only_depend_on_table_value() {
        let resolved =
            resolve_subagent_model(None, None, Some("anthropic"), Some("claude-opus-4-9"));

        assert_eq!(
            resolved.model.as_deref(),
            default_subagent_model("anthropic")
        );
        assert_eq!(resolved.step, "builtin_default");
    }

    #[test]
    fn empty_strings_fall_through_the_chain() {
        let resolved = resolve_subagent_model(
            Some(""),
            Some("anthropic/claude-haiku-4-5"),
            Some("anthropic"),
            Some("claude-opus-4-7"),
        );
        assert_eq!(
            resolved.model.as_deref(),
            Some("anthropic/claude-haiku-4-5")
        );
        assert_eq!(resolved.step, "profile_config");

        let resolved = resolve_subagent_model(
            Some(""),
            Some(""),
            Some("anthropic"),
            Some("claude-opus-4-7"),
        );
        assert_eq!(
            resolved.model.as_deref(),
            Some("anthropic/claude-haiku-4-5")
        );
        assert_eq!(resolved.step, "builtin_default");
    }
}
