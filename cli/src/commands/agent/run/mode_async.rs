use crate::agent::run::helpers::system_message;
use crate::commands::agent::run::helpers::{
    add_agents_md, add_local_context, add_rulebooks, build_resume_command, tool_result,
    user_message,
};
use crate::commands::agent::run::mcp_init::{McpInitConfig, initialize_mcp_server_and_tools};
use crate::commands::agent::run::pause::{
    AsyncOutcome, ResumeInput, build_resume_hint, detect_pending_tool_calls, write_pause_manifest,
};
use crate::commands::agent::run::renderer::{OutputFormat, OutputRenderer};
use crate::commands::agent::run::tooling::run_tool_call;
use crate::config::AppConfig;
use crate::utils::agents_md::AgentsMdInfo;
use crate::utils::local_context::LocalContext;
use stakpak_api::{
    AgentClient, AgentClientConfig, AgentProvider, Model, SessionStorage, models::ListRuleBook,
};
use stakpak_mcp_server::EnabledToolsConfig;
use stakpak_shared::local_store::LocalStore;
use stakpak_shared::models::async_manifest::{AsyncManifest, PauseReason, PendingToolCall};
use stakpak_shared::models::integrations::openai::{ChatMessage, MessageContent, Role};
use stakpak_shared::models::llm::LLMTokenUsage;
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

pub struct RunAsyncConfig {
    pub prompt: String,
    pub checkpoint_id: Option<String>,
    pub session_id: Option<String>,
    pub local_context: Option<LocalContext>,
    pub verbose: bool,
    pub redact_secrets: bool,
    pub privacy_mode: bool,
    pub rulebooks: Option<Vec<ListRuleBook>>,
    pub enable_subagents: bool,
    pub max_steps: Option<usize>,
    pub output_format: OutputFormat,
    pub allowed_tools: Option<Vec<String>>,
    pub enable_mtls: bool,
    pub system_prompt: Option<String>,
    pub enabled_tools: EnabledToolsConfig,
    pub model: Model,
    pub agents_md: Option<AgentsMdInfo>,
    /// When true, respect auto-approve config and pause when tools require approval.
    pub pause_on_approval: bool,
    /// Resume input (tool decisions or text prompt) when resuming from a paused checkpoint.
    pub resume_input: Option<ResumeInput>,
    /// Auto-approve tool overrides from profile config.
    pub auto_approve_tools: Option<Vec<String>>,
}

// All print functions have been moved to the renderer module and are no longer needed here

/// Simple auto-approve policy for async mode.
/// Mirrors the TUI's AutoApprovePolicy without depending on the TUI crate.
#[derive(Debug, Clone, PartialEq)]
enum AsyncApprovePolicy {
    Auto,
    Prompt,
    Never,
}

/// Lightweight auto-approve config for async mode.
struct AsyncAutoApproveConfig {
    enabled: bool,
    default_policy: AsyncApprovePolicy,
    tools: HashMap<String, AsyncApprovePolicy>,
}

impl AsyncAutoApproveConfig {
    fn new(auto_approve_tools: Option<&Vec<String>>) -> Self {
        let mut tools = HashMap::new();

        // Auto-approve tools (read-only, safe):
        for name in &[
            "view",
            "generate_password",
            "search_docs",
            "search_memory",
            "read_rulebook",
            "local_code_search",
            "get_all_tasks",
            "get_task_details",
            "wait_for_tasks",
        ] {
            tools.insert(name.to_string(), AsyncApprovePolicy::Auto);
        }

        // Prompt tools (mutating, require approval):
        for name in &[
            "create",
            "str_replace",
            "generate_code",
            "run_command",
            "run_command_task",
            "subagent_task",
            "dynamic_subagent_task",
            "cancel_task",
            "remove",
        ] {
            tools.insert(name.to_string(), AsyncApprovePolicy::Prompt);
        }

        // Apply profile overrides
        if let Some(profile_tools) = auto_approve_tools {
            for tool_name in profile_tools {
                tools.insert(tool_name.clone(), AsyncApprovePolicy::Auto);
            }
        }

        // Try to load session config from disk
        let config_path = std::path::Path::new(".stakpak/session/auto_approve.json");
        if config_path.exists()
            && let Ok(content) = std::fs::read_to_string(config_path)
            && let Ok(session_config) = serde_json::from_str::<serde_json::Value>(&content)
            && let Some(session_tools) = session_config.get("tools").and_then(|t| t.as_object())
        {
            for (name, policy_val) in session_tools {
                // Don't override profile-specified tools
                if auto_approve_tools
                    .map(|pt| pt.contains(name))
                    .unwrap_or(false)
                {
                    continue;
                }
                let policy = match policy_val.as_str() {
                    Some("Auto") => AsyncApprovePolicy::Auto,
                    Some("Never") => AsyncApprovePolicy::Never,
                    _ => AsyncApprovePolicy::Prompt,
                };
                tools.insert(name.clone(), policy);
            }
        }

        AsyncAutoApproveConfig {
            enabled: true,
            default_policy: AsyncApprovePolicy::Prompt,
            tools,
        }
    }

    /// Strip MCP server prefix from tool name (e.g., "stakpak__run_command" -> "run_command").
    fn strip_prefix(name: &str) -> &str {
        if let Some(pos) = name.find("__")
            && pos + 2 < name.len()
        {
            return &name[pos + 2..];
        }
        name
    }

    fn get_policy(&self, tool_name: &str) -> &AsyncApprovePolicy {
        let stripped = Self::strip_prefix(tool_name);
        self.tools.get(stripped).unwrap_or(&self.default_policy)
    }

    fn should_auto_approve(&self, tool_name: &str) -> bool {
        if !self.enabled {
            return false;
        }
        matches!(self.get_policy(tool_name), AsyncApprovePolicy::Auto)
    }

    /// Check if any tool in the batch requires approval (Prompt or Never policy).
    fn any_requires_approval(&self, tool_names: &[&str]) -> bool {
        tool_names
            .iter()
            .any(|name| !self.should_auto_approve(name))
    }
}

pub async fn run_async(ctx: AppConfig, config: RunAsyncConfig) -> Result<AsyncOutcome, String> {
    let start_time = Instant::now();
    let mut llm_response_time = std::time::Duration::new(0, 0);
    let mut chat_messages: Vec<ChatMessage> = Vec::new();
    let mut total_usage = LLMTokenUsage::default();
    let renderer = OutputRenderer::new(config.output_format.clone(), config.verbose);

    // Build auto-approve config if pause_on_approval is enabled
    let auto_approve = if config.pause_on_approval {
        Some(AsyncAutoApproveConfig::new(
            config.auto_approve_tools.as_ref(),
        ))
    } else {
        None
    };

    print!("{}", renderer.render_title("Stakpak Agent - Async Mode"));
    print!(
        "{}",
        renderer.render_info("Initializing MCP server and client connections...")
    );

    // Initialize MCP server, proxy, and client using the same method as TUI mode
    let mcp_init_config = McpInitConfig {
        redact_secrets: config.redact_secrets,
        privacy_mode: config.privacy_mode,
        enabled_tools: config.enabled_tools.clone(),
        enable_mtls: config.enable_mtls,
        enable_subagents: config.enable_subagents,
        allowed_tools: config.allowed_tools.clone(),
    };
    let mcp_init_result = initialize_mcp_server_and_tools(&ctx, mcp_init_config, None).await?;
    let mcp_client = mcp_init_result.client;
    let mcp_tools = mcp_init_result.mcp_tools;
    let server_shutdown_tx = mcp_init_result.server_shutdown_tx;
    let proxy_shutdown_tx = mcp_init_result.proxy_shutdown_tx;

    // Tools are already filtered by initialize_mcp_server_and_tools
    let tools = mcp_init_result.tools;

    // Build unified AgentClient config
    let providers = ctx.get_llm_provider_config();
    let mut client_config = AgentClientConfig::new().with_providers(providers);

    if let Some(api_key) = ctx.get_stakpak_api_key() {
        client_config = client_config.with_stakpak(
            stakpak_api::StakpakConfig::new(api_key).with_endpoint(ctx.api_endpoint.clone()),
        );
    }
    if let Some(smart_model) = &ctx.smart_model {
        client_config = client_config.with_smart_model(smart_model.clone());
    }
    if let Some(eco_model) = &ctx.eco_model {
        client_config = client_config.with_eco_model(eco_model.clone());
    }
    if let Some(recovery_model) = &ctx.recovery_model {
        client_config = client_config.with_recovery_model(recovery_model.clone());
    }

    let client = AgentClient::new(client_config)
        .await
        .map_err(|e| format!("Failed to create client: {}", e))?;

    let mut current_session_id: Option<Uuid> = None;
    let mut current_checkpoint_id: Option<Uuid> = None;
    let mut current_metadata: Option<serde_json::Value> = None;
    let mut prior_steps: usize = 0;

    // Load checkpoint/session messages if provided
    if let Some(session_id_str) = config.session_id {
        let checkpoint_start = Instant::now();
        let session_uuid = Uuid::parse_str(&session_id_str)
            .map_err(|_| format!("Invalid session ID: {}", session_id_str))?;

        let checkpoint = client
            .get_active_checkpoint(session_uuid)
            .await
            .map_err(|e| format!("Failed to get active checkpoint for session: {}", e))?;

        current_session_id = Some(checkpoint.session_id);
        current_checkpoint_id = Some(checkpoint.id);
        current_metadata = checkpoint.state.metadata;
        chat_messages.extend(checkpoint.state.messages);

        llm_response_time += checkpoint_start.elapsed();
        print!(
            "{}",
            renderer.render_info(&format!("Resuming from session ({})", session_id_str))
        );
    } else if let Some(checkpoint_id_str) = config.checkpoint_id {
        let checkpoint_start = Instant::now();

        // Parse checkpoint UUID
        let checkpoint_uuid = Uuid::parse_str(checkpoint_id_str.as_str())
            .map_err(|_| format!("Invalid checkpoint ID: {}", checkpoint_id_str))?;

        // Get checkpoint with session info
        match client.get_checkpoint(checkpoint_uuid).await {
            Ok(checkpoint) => {
                current_session_id = Some(checkpoint.session_id);
                current_checkpoint_id = Some(checkpoint_uuid);
                current_metadata = checkpoint.state.metadata;
                prior_steps = checkpoint
                    .state
                    .messages
                    .iter()
                    .filter(|m| m.role == Role::Assistant)
                    .count();
                chat_messages.extend(checkpoint.state.messages);
            }
            Err(e) => {
                return Err(format!("Failed to get checkpoint: {}", e));
            }
        }

        llm_response_time += checkpoint_start.elapsed();
        print!(
            "{}",
            renderer.render_info(&format!("Resuming from checkpoint ({})", checkpoint_id_str))
        );
    }

    // Handle resume from paused state
    if let Some(resume_input) = &config.resume_input {
        let pending_tool_calls = detect_pending_tool_calls(&chat_messages);

        if !pending_tool_calls.is_empty() && resume_input.has_tool_decisions() {
            // Resume with tool decisions
            print!(
                "{}",
                renderer.render_info(&format!(
                    "Resuming with tool decisions for {} pending tool call(s)",
                    pending_tool_calls.len()
                ))
            );

            for tool_call in &pending_tool_calls {
                if resume_input.is_approved(&tool_call.id) {
                    // Execute approved tool
                    print!(
                        "{}",
                        renderer.render_tool_execution(
                            &tool_call.function.name,
                            &tool_call.function.arguments,
                            0,
                            1,
                        )
                    );

                    let tool_execution = async {
                        run_tool_call(
                            &mcp_client,
                            &mcp_tools,
                            tool_call,
                            None,
                            current_session_id,
                            Some(config.model.id.clone()),
                        )
                        .await
                    };

                    let result = match tokio::time::timeout(
                        std::time::Duration::from_secs(60 * 60),
                        tool_execution,
                    )
                    .await
                    {
                        Ok(result) => result?,
                        Err(_) => {
                            let error_msg = format!(
                                "Tool '{}' timed out after 60 minutes",
                                tool_call.function.name
                            );
                            print!("{}", renderer.render_error(&error_msg));
                            chat_messages.push(tool_result(tool_call.id.clone(), error_msg));
                            continue;
                        }
                    };

                    if let Some(result) = result {
                        let result_content = result
                            .content
                            .iter()
                            .map(|c| match c.raw.as_text() {
                                Some(text) => text.text.clone(),
                                None => String::new(),
                            })
                            .collect::<Vec<String>>()
                            .join("\n");

                        print!("{}", renderer.render_tool_result(&result_content));
                        chat_messages.push(tool_result(tool_call.id.clone(), result_content));
                    } else {
                        chat_messages
                            .push(tool_result(tool_call.id.clone(), "No result".to_string()));
                    }
                } else {
                    // Reject tool
                    print!(
                        "{}",
                        renderer.render_info(&format!(
                            "Rejected tool call: {} ({})",
                            tool_call.function.name, tool_call.id
                        ))
                    );
                    chat_messages.push(tool_result(
                        tool_call.id.clone(),
                        "TOOL_CALL_REJECTED".to_string(),
                    ));
                }
            }
        } else if let Some(_prompt) = &resume_input.prompt {
            // Resume with text input
            print!("{}", renderer.render_info("Resuming with user input"));
            // Don't add prompt here — it will be added below via the normal prompt path
        }
    }

    if let Some(system_prompt) = config.system_prompt {
        chat_messages.insert(0, system_message(system_prompt));
        print!("{}", renderer.render_info("System prompt loaded"));
    }

    // Add user prompt if provided (and not resuming with tool decisions)
    let should_add_prompt = if let Some(resume_input) = &config.resume_input {
        // When resuming with tool decisions, don't add the prompt as a user message
        // When resuming with text input, the prompt IS the resume text
        !resume_input.has_tool_decisions()
    } else {
        true
    };

    if should_add_prompt && !config.prompt.is_empty() {
        let (user_input, _local_context) =
            add_local_context(&chat_messages, &config.prompt, &config.local_context, false)
                .await
                .map_err(|e| e.to_string())?;

        let (user_input, _rulebooks_text) = if let Some(rulebooks) = &config.rulebooks
            && chat_messages.is_empty()
        {
            add_rulebooks(&user_input, rulebooks)
        } else {
            (user_input, None)
        };

        let user_input = if chat_messages.is_empty()
            && let Some(agents_md) = &config.agents_md
        {
            let (user_input, _agents_md_text) = add_agents_md(&user_input, agents_md);
            user_input
        } else {
            user_input
        };

        chat_messages.push(user_message(user_input));
    }

    let mut step = 0;
    let max_steps = config.max_steps.unwrap_or(50); // Safety limit to prevent infinite loops

    print!("{}", renderer.render_info("Starting execution..."));
    print!("{}", renderer.render_section_break());

    loop {
        step += 1;
        if step > max_steps {
            print!(
                "{}",
                renderer.render_warning(&format!(
                    "Reached maximum steps limit ({}), stopping execution",
                    max_steps
                ))
            );
            break;
        }

        // Make chat completion request
        let llm_start = Instant::now();
        let response = client
            .chat_completion(
                config.model.clone(),
                chat_messages.clone(),
                Some(tools.clone()),
                current_session_id,
                current_metadata.clone(),
            )
            .await
            .map_err(|e| e.to_string())?;
        llm_response_time += llm_start.elapsed();

        // Accumulate token usage
        total_usage.prompt_tokens += response.usage.prompt_tokens;
        total_usage.completion_tokens += response.usage.completion_tokens;
        total_usage.total_tokens += response.usage.total_tokens;
        if let Some(details) = &response.usage.prompt_tokens_details {
            if total_usage.prompt_tokens_details.is_none() {
                total_usage.prompt_tokens_details = Some(Default::default());
            }
            if let Some(ref mut total_details) = total_usage.prompt_tokens_details {
                total_details.input_tokens = Some(
                    total_details.input_tokens.unwrap_or(0) + details.input_tokens.unwrap_or(0),
                );
                total_details.cache_read_input_tokens = Some(
                    total_details.cache_read_input_tokens.unwrap_or(0)
                        + details.cache_read_input_tokens.unwrap_or(0),
                );
                total_details.cache_write_input_tokens = Some(
                    total_details.cache_write_input_tokens.unwrap_or(0)
                        + details.cache_write_input_tokens.unwrap_or(0),
                );
            }
        }

        chat_messages.push(response.choices[0].message.clone());

        // Update metadata from checkpoint state so the next
        // turn sees the latest trimming state.
        if let Some(state_metadata) = response
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("state_metadata"))
        {
            current_metadata = Some(state_metadata.clone());
        }

        // Get session_id and checkpoint_id from the response
        // response.id is the checkpoint_id created by chat_completion
        if let Ok(checkpoint_uuid) = Uuid::parse_str(&response.id) {
            current_checkpoint_id = Some(checkpoint_uuid);

            // Get session_id from checkpoint if we don't have it yet
            if current_session_id.is_none()
                && let Ok(checkpoint) = client.get_checkpoint(checkpoint_uuid).await
            {
                current_session_id = Some(checkpoint.session_id);
            }
        }

        let tool_calls = response.choices[0].message.tool_calls.as_ref();
        let tool_count = tool_calls.map(|t| t.len()).unwrap_or(0);

        print!("{}", renderer.render_step_header(step, tool_count));

        // Extract agent message content
        let agent_message =
            response.choices[0]
                .message
                .content
                .as_ref()
                .map(|content| match content {
                    MessageContent::String(s) => s.clone(),
                    MessageContent::Array(parts) => parts
                        .iter()
                        .filter_map(|part| part.text.as_ref())
                        .map(|text| text.as_str())
                        .filter(|text| !text.starts_with("<checkpoint_id>"))
                        .collect::<Vec<&str>>()
                        .join("\n"),
                });

        // Show assistant response
        if let Some(content_str) = &agent_message
            && !content_str.trim().is_empty()
        {
            print!("{}", renderer.render_assistant_message(content_str, false));
        }

        // Check if there are tool calls to execute
        if let Some(tool_calls) = tool_calls {
            if tool_calls.is_empty() {
                print!(
                    "{}",
                    renderer
                        .render_success("No more tools to execute - agent completed successfully")
                );
                break;
            }

            // Check if pause_on_approval is enabled and any tools require approval
            if let Some(ref auto_approve_config) = auto_approve {
                let tool_names: Vec<&str> = tool_calls
                    .iter()
                    .map(|tc| tc.function.name.as_str())
                    .collect();

                if auto_approve_config.any_requires_approval(&tool_names) {
                    // PAUSE: tools require approval
                    let pending: Vec<PendingToolCall> =
                        tool_calls.iter().map(PendingToolCall::from).collect();

                    let checkpoint_id_str = current_checkpoint_id.map(|id| id.to_string());
                    let session_id_str = current_session_id.map(|id| id.to_string());

                    let pause_reason = PauseReason::ToolApprovalRequired {
                        pending_tool_calls: pending,
                    };

                    let resume_hint = checkpoint_id_str
                        .as_ref()
                        .map(|cid| build_resume_hint(cid, &pause_reason));

                    let manifest = AsyncManifest {
                        outcome: "paused".to_string(),
                        checkpoint_id: checkpoint_id_str.clone(),
                        session_id: session_id_str.clone(),
                        model: config.model.id.clone(),
                        agent_message: agent_message.clone(),
                        steps: step,
                        total_steps: prior_steps + step,
                        usage: total_usage.clone(),
                        pause_reason: Some(pause_reason.clone()),
                        resume_hint,
                    };

                    // Write pause manifest
                    if let Err(e) = write_pause_manifest(&manifest) {
                        print!(
                            "{}",
                            renderer
                                .render_warning(&format!("Failed to write pause manifest: {}", e))
                        );
                    }

                    // Output JSON to stdout if in JSON mode
                    if config.output_format == OutputFormat::Json
                        && let Ok(json) = serde_json::to_string_pretty(&manifest)
                    {
                        println!("{}", json);
                    }

                    print!(
                        "{}",
                        renderer.render_info("Agent paused - tools require approval")
                    );

                    // Shutdown MCP
                    let _ = server_shutdown_tx.send(());
                    let _ = proxy_shutdown_tx.send(());
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                    return Ok(AsyncOutcome::Paused {
                        checkpoint_id: checkpoint_id_str,
                        session_id: session_id_str,
                        pause_reason,
                        agent_message,
                    });
                }
            }

            // Execute all tool calls (either auto-approved or pause_on_approval is disabled)
            for (i, tool_call) in tool_calls.iter().enumerate() {
                // Print tool start with arguments
                print!(
                    "{}",
                    renderer.render_tool_execution(
                        &tool_call.function.name,
                        &tool_call.function.arguments,
                        i,
                        tool_calls.len(),
                    )
                );

                // Add timeout for tool execution
                let tool_execution = async {
                    run_tool_call(
                        &mcp_client,
                        &mcp_tools,
                        tool_call,
                        None,
                        current_session_id,
                        Some(config.model.id.clone()),
                    )
                    .await
                };

                let result = match tokio::time::timeout(
                    std::time::Duration::from_secs(60 * 60), // 60 minute timeout
                    tool_execution,
                )
                .await
                {
                    Ok(result) => result?,
                    Err(_) => {
                        print!(
                            "{}",
                            renderer.render_error(&format!(
                                "Tool '{}' timed out after 60 minutes",
                                tool_call.function.name
                            ))
                        );
                        continue;
                    }
                };

                if let Some(result) = result {
                    let result_content = result
                        .content
                        .iter()
                        .map(|c| match c.raw.as_text() {
                            Some(text) => text.text.clone(),
                            None => String::new(),
                        })
                        .collect::<Vec<String>>()
                        .join("\n");

                    // Print tool result
                    print!("{}", renderer.render_tool_result(&result_content));

                    chat_messages.push(tool_result(tool_call.id.clone(), result_content.clone()));
                } else {
                    print!(
                        "{}",
                        renderer.render_warning(&format!(
                            "Tool '{}' returned no result",
                            tool_call.function.name
                        ))
                    );
                }
            }
        } else {
            print!(
                "{}",
                renderer.render_success("No more tools to execute - agent completed successfully")
            );
            break;
        }
    }

    let elapsed = start_time.elapsed();
    let tool_execution_time = elapsed.saturating_sub(llm_response_time);

    // Build completion output
    let checkpoint_id_str = current_checkpoint_id.map(|id| id.to_string());
    let session_id_str = current_session_id.map(|id| id.to_string());

    // Extract final message
    let final_message = chat_messages
        .iter()
        .rev()
        .find(|m| m.role == Role::Assistant)
        .and_then(|m| m.content.as_ref())
        .map(|content| match content {
            MessageContent::String(s) => s.clone(),
            MessageContent::Array(parts) => parts
                .iter()
                .filter_map(|part| part.text.as_ref())
                .map(|text| text.as_str())
                .filter(|text| !text.starts_with("<checkpoint_id>"))
                .collect::<Vec<&str>>()
                .join("\n"),
        });

    // Use generic renderer functions to build the completion output
    print!("{}", renderer.render_section_break());
    print!("{}", renderer.render_title("Execution Summary"));

    // Explicitly choose the appropriate renderer for each stat
    print!(
        "{}",
        renderer.render_success(&format!(
            "Completed after {} steps in {:.2}s",
            step - 1,
            elapsed.as_secs_f64()
        ))
    );
    print!(
        "{}",
        renderer.render_stat_line(
            "Tool execution time",
            &format!("{:.2}s", tool_execution_time.as_secs_f64())
        )
    );
    print!(
        "{}",
        renderer.render_stat_line(
            "API call time",
            &format!("{:.2}s", llm_response_time.as_secs_f64())
        )
    );
    print!(
        "{}",
        renderer.render_stat_line(
            "Total messages in conversation",
            &format!("{}", chat_messages.len())
        )
    );

    // Output JSON completion manifest if in JSON mode
    if config.output_format == OutputFormat::Json {
        let manifest = AsyncManifest {
            outcome: "completed".to_string(),
            checkpoint_id: checkpoint_id_str.clone(),
            session_id: session_id_str.clone(),
            model: config.model.id.clone(),
            agent_message: final_message.clone(),
            steps: step,
            total_steps: prior_steps + step,
            usage: total_usage.clone(),
            pause_reason: None,
            resume_hint: None,
        };
        if let Ok(json) = serde_json::to_string_pretty(&manifest) {
            println!("{}", json);
        }
    } else {
        print!("{}", renderer.render_final_completion(&chat_messages));
        println!();

        // Print token usage at the end
        print!("{}", renderer.render_token_usage_stats(&total_usage));
    }

    // Save conversation to file
    let conversation_json = serde_json::to_string_pretty(&chat_messages).unwrap_or_default();
    match LocalStore::write_session_data("messages.json", &conversation_json) {
        Ok(path) => {
            print!(
                "{}",
                renderer.render_success(&format!(
                    "Saved {} history messages to {}",
                    chat_messages.len(),
                    path
                ))
            );
        }
        Err(e) => {
            print!(
                "{}",
                renderer.render_error(&format!("Failed to save messages: {}", e))
            );
        }
    }

    // Save checkpoint to file if available
    if let Some(checkpoint_id) = current_checkpoint_id {
        match LocalStore::write_session_data("checkpoint", checkpoint_id.to_string().as_str()) {
            Ok(path) => {
                print!(
                    "{}",
                    renderer
                        .render_success(&format!("Checkpoint {} saved to {}", checkpoint_id, path))
                );
            }
            Err(e) => {
                print!(
                    "{}",
                    renderer.render_error(&format!("Failed to save checkpoint: {}", e))
                );
            }
        }
    } else {
        print!(
            "{}",
            renderer.render_info("No checkpoint available to save")
        );
    }

    if config.output_format != OutputFormat::Json {
        // Print resume command and session ID if available
        if let Some(resume_command) =
            build_resume_command(current_session_id, current_checkpoint_id)
        {
            println!("\nTo resume, run:\n{}\n", resume_command);
        }

        if let Some(session_id) = current_session_id {
            println!("Session ID: {}", session_id);
        }
    }

    // Gracefully shutdown MCP server and proxy
    print!(
        "{}",
        renderer.render_info("Shutting down MCP server and proxy...")
    );
    let _ = server_shutdown_tx.send(());
    let _ = proxy_shutdown_tx.send(());
    // Give the servers a moment to cleanup
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    print!("{}", renderer.render_success("Shutdown complete"));

    let outcome: AsyncOutcome = AsyncOutcome::Completed {
        checkpoint_id: checkpoint_id_str,
        session_id: session_id_str,
        agent_message: final_message,
        steps: step - 1,
    };

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_resume_command_prefers_session_id() {
        let session_id = Uuid::from_u128(0x11111111111111111111111111111111);
        let checkpoint_id = Uuid::from_u128(0x22222222222222222222222222222222);

        let resume_command = build_resume_command(Some(session_id), Some(checkpoint_id));
        assert_eq!(resume_command, Some(format!("stakpak -s {}", session_id)));
    }

    #[test]
    fn build_resume_command_uses_checkpoint_when_no_session() {
        let checkpoint_id = Uuid::from_u128(0x22222222222222222222222222222222);

        let resume_command = build_resume_command(None, Some(checkpoint_id));
        assert_eq!(
            resume_command,
            Some(format!("stakpak -c {}", checkpoint_id))
        );
    }

    #[test]
    fn build_resume_command_returns_none_when_no_ids() {
        let resume_command = build_resume_command(None, None);
        assert_eq!(resume_command, None);
    }
}
