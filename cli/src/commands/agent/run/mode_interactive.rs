use crate::agent::run::helpers::system_message;
use crate::commands::agent::run::checkpoint::{
    extract_checkpoint_id_from_messages, extract_checkpoint_messages_and_tool_calls,
    get_checkpoint_messages, resume_session_from_checkpoint,
};
use crate::commands::agent::run::helpers::{
    build_plan_mode_instructions, build_resume_command, extract_last_checkpoint_id,
    is_first_non_system_message, refresh_billing_info, tool_call_history_string, tool_result,
    user_message,
};
use crate::commands::agent::run::mcp_init;
use crate::commands::agent::run::renderer::{OutputFormat, OutputRenderer};
use crate::commands::agent::run::stream::process_responses_stream;
use crate::commands::agent::run::tooling::{list_sessions, run_tool_call};
use crate::commands::agent::run::tui::{send_input_event, send_tool_call};
use crate::commands::warden;
use crate::config::AppConfig;
use crate::utils::agent_context::AgentContext;
use crate::utils::check_update::get_latest_cli_version;
use crate::utils::cli_colors::CliColors;
use reqwest::header::HeaderMap;
use stakpak_api::local::skills::{default_skill_directories, discover_skills};
use stakpak_api::models::{ApiStreamError, Skill};
use stakpak_api::{AgentClient, AgentClientConfig, AgentProvider, Model};

use stakpak_mcp_server::EnabledToolsConfig;
use stakpak_shared::models::integrations::mcp::CallToolResultExt;
use stakpak_shared::models::integrations::openai::{
    ChatMessage, MessageContent, Role, ToolCall, ToolCallResultStatus,
};
use stakpak_shared::models::llm::{LLMTokenUsage, PromptTokensDetails};

/// Bundled infrastructure analysis prompt (embedded at compile time)
/// analyze the infrastructure and provide a summary of the current state
const INIT_PROMPT: &str = include_str!("../../../../../libs/api/src/prompts/init.v4.md");
use stakpak_shared::telemetry::{TelemetryEvent, capture_event};
use stakpak_tui::{InputEvent, LoadingOperation, OutputEvent};
use std::sync::Arc;
use uuid::Uuid;

type ClientTaskResult = Result<
    (
        Vec<ChatMessage>,
        Option<Uuid>,
        Option<AppConfig>,
        LLMTokenUsage,
        String, // resolved model display: "provider/name"
    ),
    String,
>;

async fn start_stream_processing_loading(
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
) -> Result<(), String> {
    send_input_event(
        input_tx,
        InputEvent::StartLoadingOperation(LoadingOperation::StreamProcessing),
    )
    .await
    .map_err(|e| e.to_string())
}

async fn end_tool_execution_loading_if_none(
    has_result: bool,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
) -> Result<(), String> {
    if !has_result {
        send_input_event(
            input_tx,
            InputEvent::EndLoadingOperation(LoadingOperation::ToolExecution),
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Sets current_session_id and notifies the TUI if it was previously unset.
async fn set_session_id(
    current_session_id: &mut Option<Uuid>,
    new_session_id: Uuid,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
) -> Result<(), String> {
    let was_none = current_session_id.is_none();
    *current_session_id = Some(new_session_id);
    if was_none {
        send_input_event(
            input_tx,
            InputEvent::SetSessionId(new_session_id.to_string()),
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Returns the IDs of tool_calls from the last assistant message that don't have corresponding tool_results.
/// This is used to add cancelled tool_results before inserting a user message.
fn get_unresolved_tool_call_ids(messages: &[ChatMessage]) -> Vec<String> {
    // Find the last assistant message and check if it has tool_calls
    if let Some(last_assistant_msg) = messages.iter().rev().find(|m| m.role == Role::Assistant)
        && let Some(tool_calls) = &last_assistant_msg.tool_calls
        && !tool_calls.is_empty()
    {
        // Collect all tool_result IDs from messages
        let tool_result_ids: std::collections::HashSet<_> = messages
            .iter()
            .filter(|m| m.role == Role::Tool && m.tool_call_id.is_some())
            .filter_map(|m| m.tool_call_id.as_ref())
            .collect();

        // Return tool_call IDs that don't have corresponding tool_results
        return tool_calls
            .iter()
            .filter(|tc| !tool_result_ids.contains(&tc.id))
            .map(|tc| tc.id.clone())
            .collect();
    }

    Vec::new()
}

/// Checks if there are pending tool calls that don't have corresponding tool_results.
/// This is used to prevent sending messages to the API when tool_use blocks would be orphaned,
/// which causes Anthropic API 400 errors.
fn has_pending_tool_calls(messages: &[ChatMessage], tools_queue: &[ToolCall]) -> bool {
    // If there are tools in the queue waiting to be processed, we have pending tool calls
    if !tools_queue.is_empty() {
        return true;
    }

    // Check if there are unresolved tool_calls in the messages
    !get_unresolved_tool_call_ids(messages).is_empty()
}

/// Find the index in the messages Vec of the nth user message (1-indexed).
/// Used for reverting to a specific user message by truncating the messages array.
fn find_nth_user_message_index(messages: &[ChatMessage], n: usize) -> Option<usize> {
    let mut count = 0;
    for (idx, msg) in messages.iter().enumerate() {
        if msg.role == Role::User {
            count += 1;
            if count == n {
                return Some(idx);
            }
        }
    }
    None
}

/// Send the next tool call from the queue to the TUI.
/// If the tool is `ask_user`, auto-approve it by sending `ShowAskUserPopup` directly
/// (bypassing the approval bar). This eliminates the async gap where the user sees
/// a dead "Ask User" placeholder and Enter does nothing.
async fn send_next_tool_from_queue(
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
    tool_call: &ToolCall,
) -> Result<(), String> {
    let tool_name = tool_call
        .function
        .name
        .strip_prefix("stakpak__")
        .unwrap_or(&tool_call.function.name);

    // Auto-approve ask_user — show popup directly, skip the approval bar.
    // If parsing fails, fall through to normal approval flow.
    if tool_name == "ask_user"
        && let Ok(request) = serde_json::from_str::<
            stakpak_shared::models::integrations::openai::AskUserRequest,
        >(&tool_call.function.arguments)
    {
        send_input_event(
            input_tx,
            InputEvent::ShowAskUserPopup(tool_call.clone(), request.questions),
        )
        .await?;
        return Ok(());
    }

    send_tool_call(input_tx, tool_call).await?;
    Ok(())
}

pub struct RunInteractiveConfig {
    pub checkpoint_id: Option<String>,
    pub session_id: Option<String>,
    pub agent_context: Option<AgentContext>,
    pub redact_secrets: bool,
    pub privacy_mode: bool,
    pub enable_subagents: bool,
    pub enable_mtls: bool,
    pub is_git_repo: bool,
    pub study_mode: bool,
    pub plan_mode: bool,
    pub system_prompt: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub auto_approve: Option<Vec<String>>,
    pub enabled_tools: EnabledToolsConfig,
    pub model: Model,
    /// When true, send init_prompt_content as first user message on session start (stakpak init)
    pub send_init_prompt_on_start: bool,
    /// Theme override: None = auto-detect, Some(theme) = use specified theme
    pub theme: Option<stakpak_tui::services::detect_term::Theme>,
}

#[allow(unused_assignments)] // plan_mode_active: written in PlanModeActivated, read in later phases
pub async fn run_interactive(
    mut ctx: AppConfig,
    mut config: RunInteractiveConfig,
) -> Result<(), String> {
    // Initialize theme detection before starting TUI
    stakpak_tui::services::detect_term::init_theme(config.theme);

    // Outer loop for profile switching
    'profile_switch_loop: loop {
        let mut model = config.model.clone();
        let mut messages: Vec<ChatMessage> = Vec::new();
        let mut tools_queue: Vec<ToolCall> = Vec::new();
        // Plan mode tracking — written in PlanModeActivated, read in later phases
        #[allow(unused_variables, unused_assignments)]
        let mut plan_mode_active = false;
        let mut plan_instructions_injected = false;
        let mut should_refresh_skills_on_next_message = false;
        let mut total_session_usage = LLMTokenUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            prompt_tokens_details: None,
        };

        // Clone config values for this iteration
        let api_key = ctx.get_stakpak_api_key();
        let api_endpoint = ctx.api_endpoint.clone();
        let has_stakpak_key = api_key.is_some();
        let config_path = ctx.config_path.clone();
        let profile_name = ctx.profile_name.clone();
        let _mcp_server_host = ctx.mcp_server_host.clone();
        let mut agent_context = config.agent_context.clone();
        let mut all_available_remote_skills: Option<Vec<Skill>> = None;
        let system_prompt = config.system_prompt.clone();
        let enable_subagents = config.enable_subagents;
        let checkpoint_id = config.checkpoint_id.clone();
        let session_id = config.session_id.clone();
        let allowed_tools = config.allowed_tools.clone();
        let auto_approve = config.auto_approve.clone();
        let enabled_tools = config.enabled_tools.clone();
        let redact_secrets = config.redact_secrets;
        let privacy_mode = config.privacy_mode;
        let enable_mtls = config.enable_mtls;
        let is_git_repo = config.is_git_repo;
        let study_mode = config.study_mode;

        let (input_tx, input_rx) = tokio::sync::mpsc::channel::<InputEvent>(100);
        let (output_tx, mut output_rx) = tokio::sync::mpsc::channel::<OutputEvent>(100);
        let (mcp_progress_tx, mut mcp_progress_rx) = tokio::sync::mpsc::channel(100);
        let (shutdown_tx, _shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);
        let (cancel_tx, cancel_rx) = tokio::sync::broadcast::channel::<()>(1);

        // Spawn TUI task
        let shutdown_tx_for_tui = shutdown_tx.clone();
        let current_profile_for_tui = ctx.profile_name.clone();
        let allowed_tools_for_tui = allowed_tools.clone(); // Clone for client task before move
        let rulebook_config_for_tui = ctx.rulebooks.clone().map(|rb| stakpak_tui::RulebookConfig {
            include: rb.include,
            exclude: rb.exclude,
            include_tags: rb.include_tags,
            exclude_tags: rb.exclude_tags,
        });
        let editor_command = ctx.editor.clone();

        let auth_display_info_for_tui = ctx.get_auth_display_info();
        let model_for_tui = model.clone();
        let recent_models_for_tui = ctx.recent_models.clone();

        // Use init prompt (loaded at module level as const).
        // Always run discovery probes so both `stakpak init` and `/init` get pre-calculated analysis results.
        let init_prompt_content_for_tui = {
            let discovery_output = crate::utils::discovery::run_all().await;
            if discovery_output.is_empty() {
                Some(INIT_PROMPT.to_string())
            } else {
                Some(format!(
                    "{}\n\n<discovery_results>\n{}</discovery_results>",
                    INIT_PROMPT,
                    discovery_output.trim()
                ))
            }
        };

        let send_init_prompt_on_start = config.send_init_prompt_on_start;
        let tui_handle = tokio::spawn(async move {
            let latest_version = get_latest_cli_version().await;
            stakpak_tui::run_tui(
                input_rx,
                output_tx,
                Some(cancel_tx.clone()),
                shutdown_tx_for_tui,
                latest_version.ok(),
                redact_secrets,
                privacy_mode,
                is_git_repo,
                auto_approve.as_ref(),
                allowed_tools.as_ref(),
                current_profile_for_tui,
                rulebook_config_for_tui,
                model_for_tui,
                editor_command,
                auth_display_info_for_tui,
                init_prompt_content_for_tui,
                send_init_prompt_on_start,
                recent_models_for_tui,
            )
            .await
            .map_err(|e| e.to_string())
        });

        let input_tx_clone = input_tx.clone();
        let mut shutdown_rx_for_progress = shutdown_tx.subscribe();
        let mcp_progress_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    maybe_progress = mcp_progress_rx.recv() => {
                        let Some(progress) = maybe_progress else {
                            break;
                        };
                        let _ = send_input_event(
                            &input_tx_clone,
                            InputEvent::StreamToolResult(progress),
                        )
                        .await;
                    }
                    _ = shutdown_rx_for_progress.recv() => {
                        break;
                    }
                }
            }
        });

        let api_key_for_client = api_key.clone();
        let api_endpoint_for_client = api_endpoint.clone();
        let shutdown_tx_for_client = shutdown_tx.clone();
        let ctx_clone = ctx.clone(); // Clone ctx for use in client task
        let client_handle: tokio::task::JoinHandle<ClientTaskResult> = tokio::spawn(async move {
            let mut current_session_id: Option<Uuid> = None;
            let mut current_metadata: Option<serde_json::Value> = None;

            // Build unified AgentClient config
            let providers = ctx_clone.get_llm_provider_config();
            let mut client_config = AgentClientConfig::new().with_providers(providers);

            if let Some(ref key) = api_key_for_client {
                client_config = client_config.with_stakpak(
                    stakpak_api::StakpakConfig::new(key.clone())
                        .with_endpoint(api_endpoint_for_client.clone()),
                );
            }
            // Pass unified model as smart_model for AgentClient compatibility
            if let Some(model) = &ctx_clone.model {
                client_config = client_config.with_smart_model(model.clone());
            }

            let client: Arc<dyn AgentProvider> = Arc::new(
                AgentClient::new(client_config)
                    .await
                    .map_err(|e| format!("Failed to create client: {}", e))?,
            );

            model = super::helpers::resolve_model_from_provider(model, client.as_ref()).await;

            let mcp_init_config = mcp_init::McpInitConfig {
                redact_secrets,
                privacy_mode,
                enabled_tools: enabled_tools.clone(),
                enable_mtls,
                enable_subagents,
                allowed_tools: allowed_tools_for_tui.clone(),
                subagent_config: stakpak_mcp_server::SubagentConfig {
                    profile_name: Some(ctx_clone.profile_name.clone()),
                    config_path: Some(ctx_clone.config_path.clone()),
                },
            };
            // Tools are already filtered by initialize_mcp_server_and_tools (same as async mode)
            let (mcp_client, mcp_tools, tools, _server_shutdown_tx, _proxy_shutdown_tx) =
                match mcp_init::initialize_mcp_server_and_tools(
                    &ctx_clone,
                    mcp_init_config,
                    Some(mcp_progress_tx.clone()),
                )
                .await
                {
                    Ok(result) => (
                        Some(result.client),
                        result.mcp_tools,
                        result.tools,
                        Some(result.server_shutdown_tx),
                        Some(result.proxy_shutdown_tx),
                    ),
                    Err(e) => {
                        log::warn!(
                            "Failed to initialize MCP client: {}, continuing without tools",
                            e
                        );
                        (None, Vec::new(), Vec::new(), None, None)
                    }
                };

            let data = client.get_my_account().await?;
            send_input_event(&input_tx, InputEvent::GetStatus(data.to_text())).await?;

            // Fetch billing info (only when Stakpak API key is present)
            if has_stakpak_key {
                refresh_billing_info(client.as_ref(), &input_tx).await;
            }
            // Load available profiles and send to TUI
            let profiles_config_path = ctx_clone.config_path.clone();
            let current_profile_name = ctx_clone.profile_name.clone();
            if let Ok(profiles) = AppConfig::list_available_profiles(Some(&profiles_config_path)) {
                let _ = send_input_event(
                    &input_tx,
                    InputEvent::ProfilesLoaded(profiles, current_profile_name),
                )
                .await;
            }

            // Load remote rulebook-backed skills for context injection and TUI selection.
            if let Ok(all_rulebooks) = client.list_rulebooks().await {
                all_available_remote_skills = Some(
                    all_rulebooks
                        .iter()
                        .cloned()
                        .map(Skill::from)
                        .collect::<Vec<_>>(),
                );
                let _ =
                    send_input_event(&input_tx, InputEvent::RulebooksLoaded(all_rulebooks)).await;
            }

            if let Some(session_id_str) = session_id {
                let (chat_messages, tool_calls, session_id_uuid, checkpoint_metadata) =
                    resume_session_from_checkpoint(client.as_ref(), &session_id_str, &input_tx)
                        .await?;

                set_session_id(&mut current_session_id, session_id_uuid, &input_tx).await?;
                current_metadata = checkpoint_metadata;
                should_refresh_skills_on_next_message = true;
                tools_queue.extend(tool_calls.clone());

                if !tools_queue.is_empty() {
                    send_input_event(&input_tx, InputEvent::MessageToolCalls(tools_queue.clone()))
                        .await?;
                    let initial_tool_call = tools_queue.remove(0);
                    send_next_tool_from_queue(&input_tx, &initial_tool_call).await?;
                }

                messages.extend(chat_messages);
            } else if let Some(checkpoint_id_str) = checkpoint_id {
                // Try to get session ID from checkpoint
                let checkpoint_uuid = Uuid::parse_str(&checkpoint_id_str).map_err(|_| {
                    format!(
                        "Invalid checkpoint ID '{}' - must be a valid UUID",
                        checkpoint_id_str
                    )
                })?;

                // Try to get the checkpoint with session info
                if let Ok(checkpoint) = client.get_checkpoint(checkpoint_uuid).await {
                    set_session_id(&mut current_session_id, checkpoint.session_id, &input_tx)
                        .await?;
                }

                let (checkpoint_messages, checkpoint_metadata) =
                    get_checkpoint_messages(client.as_ref(), &checkpoint_id_str).await?;
                current_metadata = checkpoint_metadata;

                let (chat_messages, tool_calls) = extract_checkpoint_messages_and_tool_calls(
                    &checkpoint_id_str,
                    &input_tx,
                    checkpoint_messages,
                )
                .await?;

                tools_queue.extend(tool_calls.clone());

                if !tools_queue.is_empty() {
                    send_input_event(&input_tx, InputEvent::MessageToolCalls(tools_queue.clone()))
                        .await?;
                    let initial_tool_call = tools_queue.remove(0);
                    send_next_tool_from_queue(&input_tx, &initial_tool_call).await?;
                }

                messages.extend(chat_messages);
            }

            if let Some(system_prompt_text) = system_prompt {
                messages.insert(0, system_message(system_prompt_text));
            }

            // Handle --plan CLI flag: activate plan mode at startup
            if config.plan_mode {
                let session_dir = std::path::Path::new(".stakpak/session");
                if stakpak_tui::services::plan::plan_file_exists(session_dir) {
                    // Existing plan found — let the TUI show the modal
                    let meta =
                        stakpak_tui::services::plan::read_plan_file(session_dir).map(|(m, _)| m);
                    send_input_event(
                        &input_tx,
                        InputEvent::ExistingPlanFound(stakpak_tui::ExistingPlanPrompt {
                            inline_prompt: None,
                            metadata: meta,
                        }),
                    )
                    .await?;
                } else {
                    plan_mode_active = true;
                    send_input_event(&input_tx, InputEvent::PlanModeChanged(true)).await?;
                }
            }

            let mut retry_attempts = 0;
            const MAX_RETRY_ATTEMPTS: u32 = 2;

            while let Some(output_event) = output_rx.recv().await {
                match output_event {
                    OutputEvent::SwitchToModel(new_model) => {
                        // Transform model for Stakpak routing if using Stakpak API,
                        // but only for known cloud providers that don't have a direct
                        // API key configured. If the user has a direct provider key,
                        // use it instead of routing through Stakpak.
                        let known_cloud_providers =
                            ["anthropic", "openai", "google", "gemini", "amazon-bedrock"];
                        let has_direct_provider_key = ctx_clone
                            .resolve_provider_auth(&new_model.provider)
                            .is_some();
                        let should_transform = has_stakpak_key
                            && !has_direct_provider_key
                            && new_model.provider != "stakpak"
                            && known_cloud_providers.contains(&new_model.provider.as_str());

                        model = if should_transform {
                            stakpak_api::transform_for_stakpak(new_model.clone())
                        } else {
                            new_model.clone()
                        };

                        // Save to recent models in config and update TUI state
                        if let Ok(mut config_file) = AppConfig::load_config_file(&config_path)
                            && let Some(profile) = config_file.profiles.get_mut(&profile_name)
                        {
                            // Store in normalized "provider/short_name" format
                            let recent_id = crate::config::format_recent_model_id(
                                &new_model.provider,
                                &new_model.id,
                            );
                            profile.add_recent_model(&recent_id);
                            // Clone recent models before saving (to avoid borrow conflict)
                            let updated_recent_models = profile.recent_models.clone();
                            // Best-effort save - don't fail the switch if save fails
                            let _ = config_file.save_to(&config_path);

                            // Update TUI's recent models state for instant feedback
                            let _ = send_input_event(
                                &input_tx,
                                InputEvent::RecentModelsUpdated(updated_recent_models),
                            )
                            .await;
                        }

                        continue;
                    }
                    OutputEvent::UserMessage(
                        user_input,
                        tool_calls_results,
                        image_parts,
                        revert_index,
                    ) => {
                        // Handle revert if provided - truncate messages to the specified user message index
                        if let Some(target_user_idx) = revert_index {
                            // Find the ChatMessage index for the nth user message
                            let truncate_at =
                                find_nth_user_message_index(&messages, target_user_idx);

                            if let Some(idx) = truncate_at {
                                // Truncate: remove target message and everything after
                                messages.truncate(idx);
                                // Clear the tools queue since we're reverting
                                tools_queue.clear();
                                log::info!(
                                    "Reverted messages to user message index {} (truncated to {} messages)",
                                    target_user_idx,
                                    messages.len()
                                );
                            }
                        }

                        let mut user_input = user_input.clone();

                        // Add user shell history to the user input
                        if let Some(tool_call_results) = &tool_calls_results
                            && let Some(history_str) = tool_call_history_string(tool_call_results)
                        {
                            user_input = format!("{}\n\n{}", history_str, user_input);
                        }

                        // Enrich user input with unified agent context for new sessions
                        // or when remote skill selections change.
                        let user_input = if let Some(ref agent_ctx) = agent_context {
                            let is_first = is_first_non_system_message(&messages);
                            let force = should_refresh_skills_on_next_message;
                            if is_first || force {
                                should_refresh_skills_on_next_message = false;
                                agent_ctx.enrich_prompt(&user_input, is_first, force)
                            } else {
                                user_input.to_string()
                            }
                        } else {
                            user_input.to_string()
                        };

                        // Inject plan mode instructions on the first user message
                        // after plan mode is activated (via /plan or --plan)
                        let user_input = if plan_mode_active && !plan_instructions_injected {
                            plan_instructions_injected = true;
                            let plan_prompt = build_plan_mode_instructions();
                            format!("{} {}", plan_prompt, user_input)
                        } else {
                            user_input
                        };

                        // Create message with ContentParts from TUI
                        let user_msg = if image_parts.is_empty() {
                            user_message(user_input)
                        } else {
                            let mut parts = Vec::new();
                            if !user_input.trim().is_empty() {
                                parts.push(
                                    stakpak_shared::models::integrations::openai::ContentPart {
                                        r#type: "text".to_string(),
                                        text: Some(user_input),
                                        image_url: None,
                                    },
                                );
                            }
                            parts.extend(image_parts);
                            ChatMessage {
                                role: Role::User,
                                content: Some(MessageContent::Array(parts)),
                                name: None,
                                tool_calls: None,
                                tool_call_id: None,
                                usage: None,
                                ..Default::default()
                            }
                        };

                        send_input_event(&input_tx, InputEvent::HasUserMessage).await?;
                        // Add tool_result for any remaining queued tool calls before clearing.
                        // Without this, assistant messages containing tool_use blocks for these
                        // calls would be orphaned (no matching tool_result), causing Anthropic
                        // API 400 errors on the next request.
                        for abandoned_tool in tools_queue.drain(..) {
                            messages.push(tool_result(
                                abandoned_tool.id,
                                "TOOL_CALL_CANCELLED".to_string(),
                            ));
                        }
                        // Also add cancelled results for any tool_calls that are currently being
                        // executed (already removed from queue but not yet resolved).
                        // This prevents user messages from being inserted between tool_use and tool_result.
                        for unresolved_id in get_unresolved_tool_call_ids(&messages) {
                            messages.push(tool_result(
                                unresolved_id,
                                "TOOL_CALL_CANCELLED".to_string(),
                            ));
                        }
                        messages.push(user_msg);

                        // Capture telemetry when not using Stakpak API (local mode)
                        if !has_stakpak_key
                            && let Some(ref anonymous_id) = ctx_clone.anonymous_id
                            && ctx_clone.collect_telemetry.unwrap_or(true)
                        {
                            capture_event(
                                anonymous_id,
                                ctx_clone.machine_name.as_deref(),
                                true,
                                TelemetryEvent::UserPrompted,
                            );
                        }
                    }
                    OutputEvent::AcceptTool(tool_call) => {
                        // Check if this is the ask_user tool - handle it specially
                        let tool_name = tool_call
                            .function
                            .name
                            .strip_prefix("stakpak__")
                            .unwrap_or(&tool_call.function.name);
                        if tool_name == "ask_user" {
                            // Parse the questions from the tool call arguments
                            match serde_json::from_str::<
                                stakpak_shared::models::integrations::openai::AskUserRequest,
                            >(&tool_call.function.arguments)
                            {
                                Ok(request) => {
                                    // Send the popup event to TUI
                                    send_input_event(
                                        &input_tx,
                                        InputEvent::ShowAskUserPopup(
                                            tool_call.clone(),
                                            request.questions,
                                        ),
                                    )
                                    .await?;
                                    // Don't continue - wait for AskUserResponse
                                    continue;
                                }
                                Err(e) => {
                                    // Failed to parse arguments - return error result
                                    messages.push(tool_result(
                                        tool_call.id.clone(),
                                        format!("Failed to parse ask_user arguments: {}", e),
                                    ));
                                }
                            }
                            // If we get here, there was an error - continue to next iteration
                            continue;
                        }

                        send_input_event(
                            &input_tx,
                            InputEvent::StartLoadingOperation(LoadingOperation::ToolExecution),
                        )
                        .await?;
                        let result = if let Some(ref client) = mcp_client {
                            run_tool_call(
                                client.as_ref(),
                                &mcp_tools,
                                &tool_call,
                                Some(cancel_rx.resubscribe()),
                                current_session_id,
                                Some(model.id.clone()),
                            )
                            .await?
                        } else {
                            None
                        };

                        let mut should_stop = false;
                        let has_result = result.is_some();

                        if let Some(result) = result {
                            let is_cancelled =
                                result.get_status() == ToolCallResultStatus::Cancelled;

                            // Don't push a tool_result for cancelled tool calls
                            // when there are no more tools queued — the retry/shell
                            // flow will send a SendToolResult event with the final
                            // result later.  However, if there ARE queued tools we
                            // must record a CANCELLED placeholder so the tool_use
                            // block is not left orphaned when the next tool completes
                            // and triggers an API call.
                            if is_cancelled && !tools_queue.is_empty() {
                                messages.push(tool_result(
                                    tool_call.clone().id,
                                    "TOOL_CALL_CANCELLED".to_string(),
                                ));
                            }
                            if !is_cancelled {
                                // If a CANCELLED result was already inserted for this tool_call
                                // (e.g., user sent a message while the tool was in-flight),
                                // skip adding the real result to avoid duplicate tool_call_ids.
                                let already_resolved = messages.iter().any(|m| {
                                    m.role == Role::Tool
                                        && m.tool_call_id.as_deref() == Some(&tool_call.id)
                                });
                                if already_resolved {
                                    // Skip — a CANCELLED placeholder was already inserted
                                } else {
                                    let content_parts: Vec<String> = result
                                        .content
                                        .iter()
                                        .map(|c| match c.raw.as_text() {
                                            Some(text) => text.text.clone(),
                                            None => String::new(),
                                        })
                                        .filter(|s| !s.is_empty())
                                        .collect();

                                    let status = result.get_status();
                                    let result_content = if status == ToolCallResultStatus::Error
                                        && content_parts.len() >= 2
                                    {
                                        // For error cases, preserve the original formatting
                                        let error_message = content_parts[1..].join(": ");
                                        format!("[{}] {}", content_parts[0], error_message)
                                    } else {
                                        content_parts.join("\n")
                                    };

                                    messages.push(tool_result(
                                        tool_call.clone().id,
                                        result_content.clone(),
                                    ));

                                    send_input_event(
                                        &input_tx,
                                        InputEvent::ToolResult(
                                            stakpak_shared::models::integrations::openai::ToolCallResult {
                                                call: tool_call.clone(),
                                                result: result_content,
                                                status,
                                            },
                                        ),
                                    )
                                    .await?;
                                }
                            }
                            send_input_event(
                                &input_tx,
                                InputEvent::EndLoadingOperation(LoadingOperation::ToolExecution),
                            )
                            .await?;

                            should_stop = is_cancelled;
                        }
                        end_tool_execution_loading_if_none(has_result, &input_tx).await?;

                        // Process next tool in queue if available
                        if !tools_queue.is_empty() {
                            let next_tool_call = tools_queue.remove(0);
                            send_next_tool_from_queue(&input_tx, &next_tool_call).await?;
                            continue;
                        }

                        // If there was an cancellation, stop the loop
                        if should_stop {
                            continue;
                        }
                    }
                    OutputEvent::RejectTool(tool_call, should_stop) => {
                        messages.push(tool_result(
                            tool_call.id.clone(),
                            "TOOL_CALL_REJECTED".to_string(),
                        ));
                        if !tools_queue.is_empty() {
                            let tool_call = tools_queue.remove(0);
                            send_next_tool_from_queue(&input_tx, &tool_call).await?;
                            continue;
                        }
                        if should_stop {
                            continue;
                        }
                    }
                    OutputEvent::ListSessions => {
                        send_input_event(
                            &input_tx,
                            InputEvent::StartLoadingOperation(LoadingOperation::SessionsList),
                        )
                        .await?;
                        match list_sessions(client.as_ref()).await {
                            Ok(sessions) => {
                                send_input_event(&input_tx, InputEvent::SetSessions(sessions))
                                    .await?;
                                send_input_event(
                                    &input_tx,
                                    InputEvent::EndLoadingOperation(LoadingOperation::SessionsList),
                                )
                                .await?;
                            }
                            Err(e) => {
                                send_input_event(&input_tx, InputEvent::Error(e)).await?;
                                send_input_event(
                                    &input_tx,
                                    InputEvent::EndLoadingOperation(LoadingOperation::SessionsList),
                                )
                                .await?;
                            }
                        }
                        continue;
                    }
                    OutputEvent::NewSession => {
                        // Clear the current session and start fresh
                        current_session_id = None;
                        messages.clear();
                        total_session_usage = LLMTokenUsage {
                            prompt_tokens: 0,
                            completion_tokens: 0,
                            total_tokens: 0,
                            prompt_tokens_details: None,
                        };
                        continue;
                    }

                    OutputEvent::ResumeSession => {
                        let session_id = if let Some(session_id) = &current_session_id {
                            Some(session_id.to_string())
                        } else {
                            list_sessions(client.as_ref())
                                .await
                                .ok()
                                .and_then(|sessions| {
                                    sessions.first().map(|session| session.id.clone())
                                })
                        };

                        if let Some(session_id) = &session_id {
                            send_input_event(
                                &input_tx,
                                InputEvent::StartLoadingOperation(
                                    LoadingOperation::CheckpointResume,
                                ),
                            )
                            .await?;
                            match resume_session_from_checkpoint(
                                client.as_ref(),
                                session_id,
                                &input_tx,
                            )
                            .await
                            {
                                Ok((
                                    chat_messages,
                                    tool_calls,
                                    session_id_uuid,
                                    checkpoint_metadata,
                                )) => {
                                    // Track the current session ID
                                    set_session_id(
                                        &mut current_session_id,
                                        session_id_uuid,
                                        &input_tx,
                                    )
                                    .await?;
                                    current_metadata = checkpoint_metadata;

                                    // Mark that we need to refresh skills on the next user message
                                    should_refresh_skills_on_next_message = true;

                                    // Reset usage for the resumed session
                                    total_session_usage = LLMTokenUsage {
                                        prompt_tokens: 0,
                                        completion_tokens: 0,
                                        total_tokens: 0,
                                        prompt_tokens_details: None,
                                    };

                                    messages.extend(chat_messages);
                                    tools_queue.extend(tool_calls.clone());

                                    if !tools_queue.is_empty() {
                                        send_input_event(
                                            &input_tx,
                                            InputEvent::MessageToolCalls(tools_queue.clone()),
                                        )
                                        .await?;
                                        let initial_tool_call = tools_queue.remove(0);
                                        send_next_tool_from_queue(&input_tx, &initial_tool_call)
                                            .await?;
                                    }
                                    send_input_event(
                                        &input_tx,
                                        InputEvent::EndLoadingOperation(
                                            LoadingOperation::CheckpointResume,
                                        ),
                                    )
                                    .await?;
                                }
                                Err(_) => {
                                    // Error already handled in the function
                                    send_input_event(
                                        &input_tx,
                                        InputEvent::EndLoadingOperation(
                                            LoadingOperation::CheckpointResume,
                                        ),
                                    )
                                    .await?;
                                    continue;
                                }
                            }
                        } else {
                            send_input_event(
                                &input_tx,
                                InputEvent::Error("No active session to resume".to_string()),
                            )
                            .await?;
                        }
                        continue;
                    }
                    OutputEvent::SwitchToSession(session_id) => {
                        send_input_event(
                            &input_tx,
                            InputEvent::StartLoadingOperation(LoadingOperation::CheckpointResume),
                        )
                        .await?;
                        match resume_session_from_checkpoint(
                            client.as_ref(),
                            &session_id,
                            &input_tx,
                        )
                        .await
                        {
                            Ok((
                                chat_messages,
                                tool_calls,
                                session_id_uuid,
                                checkpoint_metadata,
                            )) => {
                                // Track the current session ID
                                set_session_id(&mut current_session_id, session_id_uuid, &input_tx)
                                    .await?;
                                current_metadata = checkpoint_metadata;

                                // Mark that we need to refresh skills on the next user message
                                should_refresh_skills_on_next_message = true;

                                // Reset usage for the switched session
                                total_session_usage = LLMTokenUsage {
                                    prompt_tokens: 0,
                                    completion_tokens: 0,
                                    total_tokens: 0,
                                    prompt_tokens_details: None,
                                };

                                messages.extend(chat_messages);
                                tools_queue.extend(tool_calls.clone());

                                if !tools_queue.is_empty() {
                                    send_input_event(
                                        &input_tx,
                                        InputEvent::MessageToolCalls(tools_queue.clone()),
                                    )
                                    .await?;
                                    let initial_tool_call = tools_queue.remove(0);
                                    send_next_tool_from_queue(&input_tx, &initial_tool_call)
                                        .await?;
                                }
                                send_input_event(
                                    &input_tx,
                                    InputEvent::EndLoadingOperation(
                                        LoadingOperation::CheckpointResume,
                                    ),
                                )
                                .await?;
                            }
                            Err(_) => {
                                send_input_event(
                                    &input_tx,
                                    InputEvent::EndLoadingOperation(
                                        LoadingOperation::CheckpointResume,
                                    ),
                                )
                                .await?;
                                continue;
                            }
                        }
                        continue;
                    }
                    OutputEvent::SendToolResult(
                        tool_call_result,
                        should_stop,
                        pending_tool_calls,
                    ) => {
                        send_input_event(
                            &input_tx,
                            InputEvent::StartLoadingOperation(LoadingOperation::ToolExecution),
                        )
                        .await?;
                        messages.push(tool_result(
                            tool_call_result.call.clone().id,
                            tool_call_result.result.clone(),
                        ));

                        send_input_event(
                            &input_tx,
                            InputEvent::EndLoadingOperation(LoadingOperation::ToolExecution),
                        )
                        .await?;

                        if should_stop && !pending_tool_calls.is_empty() {
                            tools_queue.extend(pending_tool_calls.clone());
                        }

                        if !tools_queue.is_empty() {
                            let tool_call = tools_queue.remove(0);
                            send_next_tool_from_queue(&input_tx, &tool_call).await?;
                            continue;
                        }
                    }
                    OutputEvent::Memorize => {
                        let checkpoint_id = extract_checkpoint_id_from_messages(&messages);
                        if let Some(checkpoint_id) = checkpoint_id {
                            let client_clone = client.clone();
                            tokio::spawn(async move {
                                if let Ok(checkpoint_id) = Uuid::parse_str(&checkpoint_id) {
                                    let _ = client_clone.memorize_session(checkpoint_id).await;
                                }
                            });
                        }
                        continue;
                    }
                    OutputEvent::RequestProfileSwitch(new_profile) => {
                        // Send progress event
                        send_input_event(
                            &input_tx,
                            InputEvent::ProfileSwitchRequested(new_profile.clone()),
                        )
                        .await?;

                        send_input_event(
                            &input_tx,
                            InputEvent::ProfileSwitchProgress("Validating API key...".to_string()),
                        )
                        .await?;

                        // Validate new profile with API key inheritance
                        let default_api_key = api_key_for_client.clone();
                        let new_config = match super::profile_switch::validate_profile_switch(
                            &new_profile,
                            Some(&config_path),
                            default_api_key,
                        )
                        .await
                        {
                            Ok(config) => config,
                            Err(e) => {
                                send_input_event(&input_tx, InputEvent::ProfileSwitchFailed(e))
                                    .await?;
                                continue; // Stay in current profile
                            }
                        };

                        send_input_event(
                            &input_tx,
                            InputEvent::ProfileSwitchProgress("✓ API key validated".to_string()),
                        )
                        .await?;

                        send_input_event(
                            &input_tx,
                            InputEvent::ProfileSwitchProgress(
                                "Shutting down current session...".to_string(),
                            ),
                        )
                        .await?;

                        // Signal completion
                        send_input_event(
                            &input_tx,
                            InputEvent::ProfileSwitchComplete(new_profile.clone()),
                        )
                        .await?;

                        // Minimal delay to display completion message
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                        // Send shutdown to exit tasks quickly
                        let _ = shutdown_tx_for_client.send(());

                        // Return new config to trigger outer loop restart
                        return Ok((
                            messages,
                            current_session_id,
                            Some(new_config),
                            total_session_usage,
                            format!("{}/{}", model.provider, model.name),
                        ));
                    }
                    OutputEvent::RequestRulebookUpdate(selected_uris) => {
                        // Update selected remote skills (backed by rulebook URIs), then rebuild skill view.
                        if let Some(all_remote_skills) = &all_available_remote_skills {
                            let mut merged_skills: Vec<Skill> = all_remote_skills
                                .iter()
                                .filter(|skill| selected_uris.contains(&skill.uri))
                                .cloned()
                                .collect();

                            let skill_dirs = default_skill_directories();
                            let local_skills = discover_skills(&skill_dirs);
                            merged_skills.extend(local_skills);

                            if let Some(ref mut ctx) = agent_context {
                                ctx.update_skills(Some(merged_skills));
                            }

                            // Set flag to refresh injected context on next message
                            should_refresh_skills_on_next_message = true;
                        }
                        continue;
                    }
                    OutputEvent::RequestCurrentRulebooks => {
                        // Send currently selected remote rulebook URIs to TUI.
                        // Agent context now stores skills, so keep only remote stakpak:// entries.
                        if let Some(ref ctx) = agent_context
                            && let Some(current_skills) = &ctx.skills
                        {
                            let current_uris: Vec<String> = current_skills
                                .iter()
                                .filter(|skill| skill.uri.starts_with("stakpak://"))
                                .map(|skill| skill.uri.clone())
                                .collect();

                            let _ = send_input_event(
                                &input_tx,
                                InputEvent::CurrentRulebooksLoaded(current_uris),
                            )
                            .await;
                        }
                        continue;
                    }
                    OutputEvent::RequestTotalUsage => {
                        // Send total accumulated usage to TUI
                        send_input_event(
                            &input_tx,
                            InputEvent::TotalUsage(total_session_usage.clone()),
                        )
                        .await?;
                        continue;
                    }
                    OutputEvent::RequestAvailableModels => {
                        // Load available models from the provider registry
                        let available_models = client.list_models().await;
                        send_input_event(
                            &input_tx,
                            InputEvent::AvailableModelsLoaded(available_models),
                        )
                        .await?;
                        continue;
                    }
                    OutputEvent::SaveRecentModels(recent_models) => {
                        // Save recent models list to config
                        if let Ok(mut config_file) = AppConfig::load_config_file(&config_path)
                            && let Some(profile) = config_file.profiles.get_mut(&profile_name)
                        {
                            profile.recent_models = recent_models;
                            // Best-effort save
                            let _ = config_file.save_to(&config_path);
                        }
                        continue;
                    }
                    OutputEvent::PlanModeActivated(inline_prompt) => {
                        // Transition to plan mode
                        plan_mode_active = true;
                        send_input_event(&input_tx, InputEvent::PlanModeChanged(true)).await?;

                        // If there's an inline prompt, inject plan instructions + prompt
                        // as a user message so the agent starts planning immediately.
                        if let Some(prompt) = inline_prompt {
                            let instructions = build_plan_mode_instructions();
                            let plan_prompt = format!("{instructions} {prompt}");
                            let user_msg = user_message(plan_prompt);
                            plan_instructions_injected = true;
                            send_input_event(&input_tx, InputEvent::HasUserMessage).await?;
                            messages.push(user_msg);
                        } else {
                            // No inline prompt — wait for the user to type their message.
                            // Don't fall through to the API call with empty messages.
                            continue;
                        }
                    }
                    OutputEvent::PlanFeedback(feedback_text) => {
                        // User submitted feedback from plan review.
                        // Inject as direct user message — the feedback already contains
                        // anchor references so the agent knows what to revise.
                        let user_msg = user_message(feedback_text.clone());
                        messages.push(user_msg);
                        send_input_event(&input_tx, InputEvent::HasUserMessage).await?;
                        send_input_event(&input_tx, InputEvent::AddUserMessage(feedback_text))
                            .await?;
                    }
                    OutputEvent::PlanApproved => {
                        // User approved the plan — plan_mode stays active, PlanStatus drives behavior.
                        // The agent is responsible for updating plan.md front matter to status: approved.
                        let approval_msg = "Plan approved. Update the plan front matter status to `approved` and proceed with creating a new task board breaking down the plan.".to_string();
                        let user_msg = user_message(approval_msg.clone());
                        messages.push(user_msg);
                        send_input_event(&input_tx, InputEvent::HasUserMessage).await?;
                        send_input_event(&input_tx, InputEvent::AddUserMessage(approval_msg))
                            .await?;
                    }
                    OutputEvent::AskUserResponse(tool_call_result) => {
                        // User responded to ask_user popup - add the result to messages
                        messages.push(tool_result(
                            tool_call_result.call.id.clone(),
                            tool_call_result.result.clone(),
                        ));

                        // Display the result in the TUI
                        send_input_event(&input_tx, InputEvent::ToolResult(tool_call_result))
                            .await?;

                        // Process next tool in queue if available
                        if !tools_queue.is_empty() {
                            let tool_call = tools_queue.remove(0);
                            send_next_tool_from_queue(&input_tx, &tool_call).await?;
                            continue;
                        }

                        // No more queued tools — fall through to send to API
                    }
                }

                // Skip sending to API if there are pending tool calls without tool_results
                // This prevents Anthropic API 400 errors about orphaned tool_use blocks
                if has_pending_tool_calls(&messages, &tools_queue) {
                    continue;
                }

                // Start loading before we begin the LLM request/stream handshake
                start_stream_processing_loading(&input_tx).await?;

                let headers = if study_mode {
                    let mut headers = HeaderMap::new();
                    #[allow(clippy::unwrap_used)]
                    headers.insert("x-system-prompt-key", "agent_study_mode".parse().unwrap());
                    Some(headers)
                } else {
                    None
                };
                let response_result = loop {
                    let stream_result = client
                        .chat_completion_stream(
                            model.clone(),
                            messages.clone(),
                            Some(tools.clone()),
                            headers.clone(),
                            current_session_id,
                            current_metadata.clone(),
                        )
                        .await;

                    let (mut stream, current_request_id) = match stream_result {
                        Ok(result) => result,
                        Err(e) => {
                            // Extract a user-friendly error message
                            let error_msg = if e.contains("Server returned non-stream response") {
                                // Extract the actual error from the server response
                                if let Some(start) = e.find(": ") {
                                    e[start + 2..].to_string()
                                } else {
                                    e.clone()
                                }
                            } else {
                                e.clone()
                            };
                            // End loading operation before sending error
                            send_input_event(
                                &input_tx,
                                InputEvent::EndLoadingOperation(LoadingOperation::StreamProcessing),
                            )
                            .await?;
                            send_input_event(&input_tx, InputEvent::Error(error_msg.clone()))
                                .await?;
                            break Err(ApiStreamError::Unknown(error_msg));
                        }
                    };

                    // Create a cancellation receiver for this iteration
                    let mut cancel_rx_iter = cancel_rx.resubscribe();

                    // Race between stream processing and cancellation
                    match tokio::select! {
                        result = process_responses_stream(&mut stream, &input_tx) => result,
                        _ = cancel_rx_iter.recv() => {
                            // Stream was cancelled
                            if let Some(request_id) = &current_request_id {
                                client.cancel_stream(request_id.clone()).await?;
                            }
                            // End any ongoing loading operation
                            send_input_event(&input_tx, InputEvent::EndLoadingOperation(LoadingOperation::StreamProcessing)).await?;
                            send_input_event(&input_tx, InputEvent::Error("STREAM_CANCELLED".to_string())).await?;
                            break Err(ApiStreamError::Unknown("Stream cancelled by user".to_string()));
                        }
                    } {
                        Ok(response) => {
                            retry_attempts = 0;
                            break Ok(response);
                        }
                        Err(e) => {
                            if matches!(e, ApiStreamError::AgentInvalidResponseStream) {
                                if retry_attempts < MAX_RETRY_ATTEMPTS {
                                    retry_attempts += 1;
                                    send_input_event(
                                        &input_tx,
                                        InputEvent::Error(format!(
                                            "RETRY_ATTEMPT_{}",
                                            retry_attempts
                                        )),
                                    )
                                    .await?;

                                    // Loading will be managed by stream processing on retry
                                    continue;
                                } else {
                                    // End loading operation before sending error
                                    send_input_event(
                                        &input_tx,
                                        InputEvent::EndLoadingOperation(
                                            LoadingOperation::StreamProcessing,
                                        ),
                                    )
                                    .await?;
                                    send_input_event(
                                        &input_tx,
                                        InputEvent::Error("MAX_RETRY_REACHED".to_string()),
                                    )
                                    .await?;
                                    break Err(e);
                                }
                            } else {
                                // End loading operation before sending error
                                send_input_event(
                                    &input_tx,
                                    InputEvent::EndLoadingOperation(
                                        LoadingOperation::StreamProcessing,
                                    ),
                                )
                                .await?;
                                send_input_event(&input_tx, InputEvent::Error(format!("{:?}", e)))
                                    .await?;
                                break Err(e);
                            }
                        }
                    }
                };

                match response_result {
                    Ok(response) => {
                        messages.push(response.choices[0].message.clone());

                        if let Some(session_id) = response
                            .metadata
                            .as_ref()
                            .and_then(|meta| meta.get("session_id"))
                            .and_then(|value| value.as_str())
                            .and_then(|value| Uuid::parse_str(value).ok())
                        {
                            set_session_id(&mut current_session_id, session_id, &input_tx).await?;
                        }

                        // Update metadata from checkpoint state so the next
                        // turn sees the latest trimming state.
                        if let Some(state_metadata) = response
                            .metadata
                            .as_ref()
                            .and_then(|meta| meta.get("state_metadata"))
                        {
                            current_metadata = Some(state_metadata.clone());
                        }

                        // Accumulate usage from response
                        total_session_usage.prompt_tokens += response.usage.prompt_tokens;
                        total_session_usage.completion_tokens += response.usage.completion_tokens;
                        total_session_usage.total_tokens += response.usage.total_tokens;

                        // Accumulate prompt token details if available
                        if let Some(response_details) = &response.usage.prompt_tokens_details {
                            if total_session_usage.prompt_tokens_details.is_none() {
                                total_session_usage.prompt_tokens_details =
                                    Some(PromptTokensDetails {
                                        input_tokens: response_details.input_tokens,
                                        output_tokens: response_details.output_tokens,
                                        cache_read_input_tokens: response_details
                                            .cache_read_input_tokens,
                                        cache_write_input_tokens: response_details
                                            .cache_write_input_tokens,
                                    });
                            } else if let Some(details) =
                                total_session_usage.prompt_tokens_details.as_mut()
                            {
                                if let Some(input) = response_details.input_tokens {
                                    details.input_tokens =
                                        Some(details.input_tokens.unwrap_or(0) + input);
                                }
                                if let Some(output) = response_details.output_tokens {
                                    details.output_tokens =
                                        Some(details.output_tokens.unwrap_or(0) + output);
                                }
                                if let Some(cache_read) = response_details.cache_read_input_tokens {
                                    details.cache_read_input_tokens = Some(
                                        details.cache_read_input_tokens.unwrap_or(0) + cache_read,
                                    );
                                }
                                if let Some(cache_write) = response_details.cache_write_input_tokens
                                {
                                    details.cache_write_input_tokens = Some(
                                        details.cache_write_input_tokens.unwrap_or(0) + cache_write,
                                    );
                                }
                            }
                        }

                        // Send updated total usage to TUI for display
                        send_input_event(
                            &input_tx,
                            InputEvent::TotalUsage(total_session_usage.clone()),
                        )
                        .await?;

                        // Refresh billing info after each assistant message (only when using Stakpak API)
                        if has_stakpak_key {
                            refresh_billing_info(client.as_ref(), &input_tx).await;
                        }

                        if current_session_id.is_none()
                            && let Some(checkpoint_id) =
                                extract_checkpoint_id_from_messages(&messages)
                            && let Ok(checkpoint_uuid) = Uuid::parse_str(&checkpoint_id)
                            && let Ok(checkpoint) = client.get_checkpoint(checkpoint_uuid).await
                        {
                            set_session_id(
                                &mut current_session_id,
                                checkpoint.session_id,
                                &input_tx,
                            )
                            .await?;
                        }

                        // Send tool calls to TUI if present
                        if let Some(tool_calls) = &response.choices[0].message.tool_calls {
                            // Send MessageToolCalls only once with all new tools from AI
                            send_input_event(
                                &input_tx,
                                InputEvent::MessageToolCalls(tool_calls.clone()),
                            )
                            .await?;

                            // Add to queue for sequential processing
                            tools_queue.extend(tool_calls.clone());

                            // Send the first tool call to show in UI
                            // Auto-approve ask_user tool (bypass approval bar)
                            if !tools_queue.is_empty() {
                                let tool_call = tools_queue.remove(0);
                                send_next_tool_from_queue(&input_tx, &tool_call).await?;
                                continue;
                            }
                        }
                    }
                    Err(_) => {
                        continue;
                    }
                }
            }

            Ok((
                messages,
                current_session_id,
                None,
                total_session_usage.clone(),
                format!("{}/{}", model.provider, model.name),
            ))
        });

        // Wait for all tasks to finish
        let (client_res, _, _) = tokio::try_join!(client_handle, tui_handle, mcp_progress_handle)
            .map_err(|e| e.to_string())?;

        let (
            final_messages,
            final_session_id,
            profile_switch_config,
            final_usage,
            final_model_name,
        ) = client_res?;

        // Check if profile switch was requested
        if let Some(new_config) = profile_switch_config {
            // Profile switch requested - update config and restart

            // All tasks have already exited from try_join
            // Give a moment for cleanup
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

            // Fetch and filter rulebooks for the new profile
            let providers = new_config.get_llm_provider_config();
            let mut new_client_config = AgentClientConfig::new().with_providers(providers);

            if let Some(api_key) = new_config.get_stakpak_api_key() {
                new_client_config = new_client_config.with_stakpak(
                    stakpak_api::StakpakConfig::new(api_key)
                        .with_endpoint(new_config.api_endpoint.clone()),
                );
            }
            // Pass unified model as smart_model for AgentClient compatibility
            if let Some(model) = &new_config.model {
                new_client_config = new_client_config.with_smart_model(model.clone());
            }

            let client: Box<dyn AgentProvider> = Box::new(
                AgentClient::new(new_client_config)
                    .await
                    .map_err(|e| format!("Failed to create client: {}", e))?,
            );

            let new_rulebooks = client.list_rulebooks().await.ok().map(|rulebooks| {
                if let Some(rulebook_config) = &new_config.rulebooks {
                    rulebook_config.filter_rulebooks(rulebooks)
                } else {
                    rulebooks
                }
            });

            // Update context skills for the new profile (remote rulebooks + local skills).
            if let Some(ref mut ctx) = config.agent_context {
                let mut merged_skills: Vec<Skill> = new_rulebooks
                    .unwrap_or_default()
                    .into_iter()
                    .map(Skill::from)
                    .collect();
                let skill_dirs = default_skill_directories();
                let local_skills = discover_skills(&skill_dirs);
                merged_skills.extend(local_skills);
                ctx.update_skills(if merged_skills.is_empty() {
                    None
                } else {
                    Some(merged_skills)
                });
            }
            config.allowed_tools = new_config.allowed_tools.clone();
            config.auto_approve = new_config.auto_approve.clone();
            config.model = new_config.get_default_model(None);

            // Update ctx
            ctx = new_config;

            // Check if warden is enabled in the new profile and we're not already inside warden
            let should_use_warden = ctx.warden.as_ref().map(|w| w.enabled).unwrap_or(false)
                && std::env::var("STAKPAK_SKIP_WARDEN").is_err();

            if should_use_warden {
                // Re-execute stakpak inside warden container
                if let Err(e) =
                    warden::run_stakpak_in_warden(ctx, &std::env::args().collect::<Vec<_>>()).await
                {
                    return Err(format!("Failed to run stakpak in warden: {}", e));
                }
                // Exit after warden execution completes (warden will handle the restart)
                return Ok(());
            }

            // Continue the loop with the new profile
            continue 'profile_switch_loop;
        }

        // Normal exit - no profile switch requested
        // Display final stats and session info
        let providers = ctx.get_llm_provider_config();
        let mut final_client_config = AgentClientConfig::new().with_providers(providers);

        if let Some(api_key) = ctx.get_stakpak_api_key() {
            final_client_config = final_client_config.with_stakpak(
                stakpak_api::StakpakConfig::new(api_key).with_endpoint(ctx.api_endpoint.clone()),
            );
        }
        // Pass unified model as smart_model for AgentClient compatibility
        if let Some(model) = &ctx.model {
            final_client_config = final_client_config.with_smart_model(model.clone());
        }

        let client: Box<dyn AgentProvider> = Box::new(
            AgentClient::new(final_client_config)
                .await
                .map_err(|e| format!("Failed to create client: {}", e))?,
        );

        // Display session stats
        if let Some(session_id) = final_session_id {
            match client.get_session_stats(session_id).await {
                Ok(stats) => {
                    let renderer = OutputRenderer::new(OutputFormat::Text, false);
                    print!("{}", renderer.render_session_stats(&stats));
                }
                Err(_) => {
                    // Don't fail the whole operation if stats fetch fails
                }
            }
        }

        // Display token usage stats
        if final_usage.total_tokens > 0 {
            let renderer = OutputRenderer::new(OutputFormat::Text, false);
            println!(
                "{}",
                renderer.render_token_usage_stats(&final_usage, Some(&final_model_name))
            );
        }

        let username = client
            .get_my_account()
            .await
            .map(|account| account.username)?;

        let resume_command = build_resume_command(
            final_session_id,
            extract_last_checkpoint_id(&final_messages),
        );

        if let Some(resume_command) = resume_command {
            println!(
                r#"To resume, run:
{}
"#,
                resume_command
            );
        }

        if let Some(session_id) = final_session_id {
            println!(
                "To view full session in browser:
https://stakpak.dev/{}/agent-sessions/{}",
                username, session_id
            );
        }

        println!();
        println!(
            "{}Feedback or bug report?{} {}Join our Discord:{} {}https://discord.gg/c4HUkDD45d{}",
            CliColors::magenta(),
            CliColors::reset(),
            CliColors::orange(),
            CliColors::reset(),
            CliColors::orange(),
            CliColors::reset()
        );
        println!();

        break; // Exit the loop after displaying stats
    } // End of 'profile_switch_loop

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use tokio::time::{Duration, timeout};

    #[tokio::test]
    async fn start_stream_processing_emits_loading_start() {
        let (tx, mut rx) = mpsc::channel(1);
        start_stream_processing_loading(&tx).await.unwrap();

        match rx.recv().await {
            Some(InputEvent::StartLoadingOperation(LoadingOperation::StreamProcessing)) => {}
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[tokio::test]
    async fn end_tool_execution_loading_if_none_emits_end() {
        let (tx, mut rx) = mpsc::channel(1);
        end_tool_execution_loading_if_none(false, &tx)
            .await
            .unwrap();

        match rx.recv().await {
            Some(InputEvent::EndLoadingOperation(LoadingOperation::ToolExecution)) => {}
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[tokio::test]
    async fn end_tool_execution_loading_if_none_skips_when_result_present() {
        let (tx, mut rx) = mpsc::channel(1);
        end_tool_execution_loading_if_none(true, &tx).await.unwrap();

        let recv = timeout(Duration::from_millis(50), rx.recv()).await;
        match recv {
            Err(_) => {} // timeout == no event, expected
            Ok(other) => panic!("unexpected event: {:?}", other),
        }
    }

    fn test_tool_call(id: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            r#type: "function".to_string(),
            function: stakpak_shared::models::integrations::openai::FunctionCall {
                name: format!("{}_fn", id),
                arguments: "{}".to_string(),
            },
            metadata: None,
        }
    }

    fn assistant_with_tool_calls(ids: &[&str]) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("assistant".to_string())),
            tool_calls: Some(ids.iter().map(|id| test_tool_call(id)).collect()),
            ..Default::default()
        }
    }

    fn tool_message(id: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: Role::Tool,
            content: Some(MessageContent::String(content.to_string())),
            tool_call_id: Some(id.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn get_unresolved_tool_call_ids_returns_empty_when_no_messages() {
        let messages: Vec<ChatMessage> = vec![];
        assert!(get_unresolved_tool_call_ids(&messages).is_empty());
    }

    #[test]
    fn get_unresolved_tool_call_ids_returns_empty_when_no_assistant_message() {
        let messages = vec![ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String("hello".to_string())),
            ..Default::default()
        }];
        assert!(get_unresolved_tool_call_ids(&messages).is_empty());
    }

    #[test]
    fn get_unresolved_tool_call_ids_returns_ids_for_unresolved_calls() {
        let messages = vec![assistant_with_tool_calls(&["tool_1"])];

        let unresolved = get_unresolved_tool_call_ids(&messages);
        assert_eq!(unresolved, vec!["tool_1".to_string()]);
    }

    #[test]
    fn get_unresolved_tool_call_ids_returns_empty_when_all_resolved() {
        let messages = vec![
            assistant_with_tool_calls(&["tool_1"]),
            tool_message("tool_1", "result"),
        ];

        assert!(get_unresolved_tool_call_ids(&messages).is_empty());
    }

    #[test]
    fn get_unresolved_tool_call_ids_returns_only_unresolved() {
        let messages = vec![
            assistant_with_tool_calls(&["tool_1", "tool_2"]),
            tool_message("tool_1", "result"),
        ];

        let unresolved = get_unresolved_tool_call_ids(&messages);
        assert_eq!(unresolved, vec!["tool_2".to_string()]);
    }

    #[test]
    fn has_pending_tool_calls_returns_true_when_queue_not_empty() {
        let messages: Vec<ChatMessage> = vec![];
        let tools_queue = vec![test_tool_call("tool_1")];

        assert!(has_pending_tool_calls(&messages, &tools_queue));
    }

    #[test]
    fn has_pending_tool_calls_returns_false_when_empty_queue_and_no_messages() {
        let messages: Vec<ChatMessage> = vec![];
        let tools_queue: Vec<ToolCall> = vec![];

        assert!(!has_pending_tool_calls(&messages, &tools_queue));
    }

    #[test]
    fn has_pending_tool_calls_returns_true_when_assistant_has_unresolved_tool_calls() {
        let messages = vec![assistant_with_tool_calls(&["tool_1"])];
        let tools_queue: Vec<ToolCall> = vec![];

        assert!(has_pending_tool_calls(&messages, &tools_queue));
    }

    #[test]
    fn has_pending_tool_calls_returns_false_when_all_tool_calls_have_results() {
        let messages = vec![
            assistant_with_tool_calls(&["tool_1"]),
            tool_message("tool_1", "result"),
        ];
        let tools_queue: Vec<ToolCall> = vec![];

        assert!(!has_pending_tool_calls(&messages, &tools_queue));
    }

    #[test]
    fn has_pending_tool_calls_returns_true_when_some_tool_calls_missing_results() {
        let messages = vec![
            assistant_with_tool_calls(&["tool_1", "tool_2"]),
            tool_message("tool_1", "result"),
        ];
        let tools_queue: Vec<ToolCall> = vec![];

        assert!(has_pending_tool_calls(&messages, &tools_queue));
    }

    #[test]
    fn has_pending_tool_calls_returns_false_when_assistant_has_empty_tool_calls() {
        let messages = vec![ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("test".to_string())),
            tool_calls: Some(vec![]),
            ..Default::default()
        }];
        let tools_queue: Vec<ToolCall> = vec![];

        assert!(!has_pending_tool_calls(&messages, &tools_queue));
    }

    #[test]
    fn has_pending_tool_calls_returns_false_when_assistant_has_no_tool_calls() {
        let messages = vec![ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("test".to_string())),
            tool_calls: None,
            ..Default::default()
        }];
        let tools_queue: Vec<ToolCall> = vec![];

        assert!(!has_pending_tool_calls(&messages, &tools_queue));
    }

    #[test]
    fn has_pending_tool_calls_checks_last_assistant_message_only() {
        let messages = vec![
            assistant_with_tool_calls(&["tool_old"]),
            tool_message("tool_old", "old result"),
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("continue".to_string())),
                ..Default::default()
            },
            assistant_with_tool_calls(&["tool_new"]),
            tool_message("tool_new", "new result"),
        ];
        let tools_queue: Vec<ToolCall> = vec![];

        // Should return false because the LAST assistant message's tool calls are resolved
        assert!(!has_pending_tool_calls(&messages, &tools_queue));
    }

    #[test]
    fn extract_last_checkpoint_id_picks_newest_assistant() {
        let older = Uuid::from_u128(0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa);
        let newer = Uuid::from_u128(0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb);
        let messages = vec![
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "<checkpoint_id>{}</checkpoint_id>",
                    older
                ))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Tool,
                content: Some(MessageContent::String("tool output".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "<checkpoint_id>{}</checkpoint_id>",
                    newer
                ))),
                ..Default::default()
            },
        ];

        assert_eq!(extract_last_checkpoint_id(&messages), Some(newer));
    }

    #[test]
    fn extract_last_checkpoint_id_returns_none_without_tag() {
        let messages = vec![ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("no checkpoint".to_string())),
            ..Default::default()
        }];

        assert_eq!(extract_last_checkpoint_id(&messages), None);
    }
}
