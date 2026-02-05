use crate::agent::run::helpers::system_message;
use crate::commands::agent::run::checkpoint::{
    extract_checkpoint_id_from_messages, extract_checkpoint_messages_and_tool_calls,
    get_checkpoint_messages, resume_session_from_checkpoint,
};
use crate::commands::agent::run::helpers::{
    add_agents_md, add_local_context, add_rulebooks, add_subagents, convert_tools_with_filter,
    refresh_billing_info, tool_call_history_string, tool_result, user_message,
};
use crate::commands::agent::run::mcp_init;
use crate::commands::agent::run::renderer::{OutputFormat, OutputRenderer};
use crate::commands::agent::run::stream::process_responses_stream;
use crate::commands::agent::run::tooling::{list_sessions, run_tool_call};
use crate::commands::agent::run::tui::{send_input_event, send_tool_call};
use crate::commands::warden;
use crate::config::AppConfig;
use crate::utils::agents_md::AgentsMdInfo;
use crate::utils::check_update::get_latest_cli_version;
use crate::utils::local_context::LocalContext;
use reqwest::header::HeaderMap;
use stakpak_api::models::ApiStreamError;
use stakpak_api::{AgentClient, AgentClientConfig, AgentProvider, models::ListRuleBook};

use stakpak_mcp_server::EnabledToolsConfig;
use stakpak_shared::models::integrations::mcp::CallToolResultExt;
use stakpak_shared::models::integrations::openai::{
    AgentModel, ChatMessage, MessageContent, Role, ToolCall, ToolCallResultStatus,
};
use stakpak_shared::models::llm::{LLMTokenUsage, PromptTokensDetails};
use stakpak_shared::models::subagent::SubagentConfigs;
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

pub struct RunInteractiveConfig {
    pub checkpoint_id: Option<String>,
    pub local_context: Option<LocalContext>,
    pub redact_secrets: bool,
    pub privacy_mode: bool,
    pub rulebooks: Option<Vec<ListRuleBook>>,
    pub subagent_configs: Option<SubagentConfigs>,
    pub enable_mtls: bool,
    pub is_git_repo: bool,
    pub study_mode: bool,
    pub system_prompt: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub auto_approve: Option<Vec<String>>,
    pub enabled_tools: EnabledToolsConfig,
    pub model: AgentModel,
    pub agents_md: Option<AgentsMdInfo>,
}

pub async fn run_interactive(
    mut ctx: AppConfig,
    mut config: RunInteractiveConfig,
) -> Result<(), String> {
    // Outer loop for profile switching
    'profile_switch_loop: loop {
        let mut model = config.model.clone();
        let mut messages: Vec<ChatMessage> = Vec::new();
        let mut tools_queue: Vec<ToolCall> = Vec::new();
        let mut should_update_rulebooks_on_next_message = false;
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
        let _mcp_server_host = ctx.mcp_server_host.clone();
        let local_context = config.local_context.clone();
        let mut rulebooks = config.rulebooks.clone();
        let mut all_available_rulebooks: Option<Vec<ListRuleBook>> = None;
        let system_prompt = config.system_prompt.clone();
        let subagent_configs = config.subagent_configs.clone();
        let agents_md = config.agents_md.clone();
        let checkpoint_id = config.checkpoint_id.clone();
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

        let model_clone = model.clone();
        let auth_display_info_for_tui = ctx.get_auth_display_info();
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
                model_clone,
                editor_command,
                auth_display_info_for_tui,
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

            // Build unified AgentClient config
            let providers = ctx_clone.get_llm_provider_config();
            let mut client_config = AgentClientConfig::new().with_providers(providers);

            if let Some(ref key) = api_key_for_client {
                client_config = client_config.with_stakpak(
                    stakpak_api::StakpakConfig::new(key.clone())
                        .with_endpoint(api_endpoint_for_client.clone()),
                );
            }
            if let Some(smart_model) = &ctx_clone.smart_model {
                client_config = client_config.with_smart_model(smart_model.clone());
            }
            if let Some(eco_model) = &ctx_clone.eco_model {
                client_config = client_config.with_eco_model(eco_model.clone());
            }
            if let Some(recovery_model) = &ctx_clone.recovery_model {
                client_config = client_config.with_recovery_model(recovery_model.clone());
            }

            let client: Arc<dyn AgentProvider> = Arc::new(
                AgentClient::new(client_config)
                    .await
                    .map_err(|e| format!("Failed to create client: {}", e))?,
            );

            let mcp_init_config = mcp_init::McpInitConfig {
                redact_secrets,
                privacy_mode,
                enabled_tools: enabled_tools.clone(),
                enable_mtls,
            };
            let (mcp_client, mcp_tools, _tools, _server_shutdown_tx, _proxy_shutdown_tx) =
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

            let tools = convert_tools_with_filter(&mcp_tools, allowed_tools_for_tui.as_ref());

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

            // Load available rulebooks and send to TUI
            if let Ok(all_rulebooks) = client.list_rulebooks().await {
                all_available_rulebooks = Some(all_rulebooks.clone());
                let _ =
                    send_input_event(&input_tx, InputEvent::RulebooksLoaded(all_rulebooks)).await;
            }

            if let Some(checkpoint_id_str) = checkpoint_id {
                // Try to get session ID from checkpoint
                let checkpoint_uuid = Uuid::parse_str(&checkpoint_id_str).map_err(|_| {
                    format!(
                        "Invalid checkpoint ID '{}' - must be a valid UUID",
                        checkpoint_id_str
                    )
                })?;

                // Try to get the checkpoint with session info
                if let Ok(checkpoint) = client.get_checkpoint(checkpoint_uuid).await {
                    current_session_id = Some(checkpoint.session_id);
                }

                let checkpoint_messages =
                    get_checkpoint_messages(client.as_ref(), &checkpoint_id_str).await?;

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
                    send_tool_call(&input_tx, &initial_tool_call).await?;
                }

                messages.extend(chat_messages);
            }

            if let Some(system_prompt_text) = system_prompt {
                messages.insert(0, system_message(system_prompt_text));
            }

            let mut retry_attempts = 0;
            const MAX_RETRY_ATTEMPTS: u32 = 2;

            while let Some(output_event) = output_rx.recv().await {
                match output_event {
                    OutputEvent::SwitchModel(new_model) => {
                        model = new_model;
                        continue;
                    }
                    OutputEvent::UserMessage(user_input, tool_calls_results, image_parts) => {
                        let mut user_input = user_input.clone();

                        // Add user shell history to the user input
                        if let Some(tool_call_results) = &tool_calls_results
                            && let Some(history_str) = tool_call_history_string(tool_call_results)
                        {
                            user_input = format!("{}\n\n{}", history_str, user_input);
                        }

                        // Add local context to user input for new sessions
                        // Add rulebooks to user input for new sessions or when rulebook settings change
                        let (user_input, _) =
                            if messages.is_empty() || should_update_rulebooks_on_next_message {
                                let (user_input_with_context, _) =
                                    add_local_context(&messages, &user_input, &local_context, true)
                                        .await
                                        .map_err(|e| {
                                            format!("Failed to format local context: {}", e)
                                        })?;

                                let (user_input_with_rulebooks, _) =
                                    if let Some(rulebooks) = &rulebooks {
                                        add_rulebooks(&user_input_with_context, rulebooks)
                                    } else {
                                        (user_input_with_context, None)
                                    };

                                should_update_rulebooks_on_next_message = false; // Reset the flag
                                (user_input_with_rulebooks, None::<String>)
                            } else {
                                (user_input.to_string(), None::<String>)
                            };

                        let (user_input, _) =
                            add_subagents(&messages, &user_input, &subagent_configs);

                        let user_input = if messages.is_empty()
                            && let Some(agents_md_info) = &agents_md
                        {
                            let (user_input, _) = add_agents_md(&user_input, agents_md_info);
                            user_input
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
                            )
                            .await?
                        } else {
                            None
                        };

                        let mut should_stop = false;
                        let has_result = result.is_some();

                        if let Some(result) = result {
                            let content_parts: Vec<String> = result
                                .content
                                .iter()
                                .map(|c| match c.raw.as_text() {
                                    Some(text) => text.text.clone(),
                                    None => String::new(),
                                })
                                .filter(|s| !s.is_empty())
                                .collect();

                            let result_content = if result.get_status()
                                == ToolCallResultStatus::Error
                                && content_parts.len() >= 2
                            {
                                // For error cases, preserve the original formatting
                                let error_message = content_parts[1..].join(": ");
                                format!("[{}] {}", content_parts[0], error_message)
                            } else {
                                content_parts.join("\n")
                            };

                            messages
                                .push(tool_result(tool_call.clone().id, result_content.clone()));

                            send_input_event(
                                &input_tx,
                                InputEvent::ToolResult(
                                    stakpak_shared::models::integrations::openai::ToolCallResult {
                                        call: tool_call.clone(),
                                        result: result_content,
                                        status: result.get_status(),
                                    },
                                ),
                            )
                            .await?;
                            send_input_event(
                                &input_tx,
                                InputEvent::EndLoadingOperation(LoadingOperation::ToolExecution),
                            )
                            .await?;

                            // Continue to next tool or main loop if error
                            should_stop = match result.get_status() {
                                ToolCallResultStatus::Cancelled => true,
                                ToolCallResultStatus::Error => false,
                                ToolCallResultStatus::Success => false,
                            };
                        }
                        end_tool_execution_loading_if_none(has_result, &input_tx).await?;

                        // Process next tool in queue if available
                        if !tools_queue.is_empty() {
                            // Don't re-send MessageToolCalls - tools were already sent when AI returned them
                            // Just send the next individual tool call to process
                            let next_tool_call = tools_queue.remove(0);
                            send_tool_call(&input_tx, &next_tool_call).await?;
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
                            send_tool_call(&input_tx, &tool_call).await?;
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
                                Ok((chat_messages, tool_calls, session_id_uuid)) => {
                                    // Track the current session ID
                                    current_session_id = Some(session_id_uuid);

                                    // Mark that we need to update rulebooks on the next user message
                                    should_update_rulebooks_on_next_message = true;

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
                                        send_tool_call(&input_tx, &initial_tool_call).await?;
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
                            Ok((chat_messages, tool_calls, session_id_uuid)) => {
                                // Track the current session ID
                                current_session_id = Some(session_id_uuid);

                                // Mark that we need to update rulebooks on the next user message
                                should_update_rulebooks_on_next_message = true;

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
                                    send_tool_call(&input_tx, &initial_tool_call).await?;
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
                            // Don't re-send MessageToolCalls - just process next tool
                            let tool_call = tools_queue.remove(0);
                            send_tool_call(&input_tx, &tool_call).await?;
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
                        ));
                    }
                    OutputEvent::RequestRulebookUpdate(selected_uris) => {
                        // Update the rulebooks list based on selected URIs
                        if let Some(all_rulebooks) = &all_available_rulebooks {
                            let filtered_rulebooks: Vec<_> = all_rulebooks
                                .iter()
                                .filter(|rb| selected_uris.contains(&rb.uri))
                                .cloned()
                                .collect();

                            // Update the rulebooks with the filtered list
                            rulebooks = Some(filtered_rulebooks);

                            // Set flag to update rulebooks on next message
                            should_update_rulebooks_on_next_message = true;
                        }
                        continue;
                    }
                    OutputEvent::RequestCurrentRulebooks => {
                        // Send currently active rulebook URIs to TUI
                        if let Some(current_rulebooks) = &rulebooks {
                            let current_uris: Vec<String> =
                                current_rulebooks.iter().map(|rb| rb.uri.clone()).collect();

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
                            current_session_id = Some(session_id);
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
                            current_session_id = Some(checkpoint.session_id);
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
                            if !tools_queue.is_empty() {
                                let tool_call = tools_queue.remove(0);
                                send_tool_call(&input_tx, &tool_call).await?;
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
            ))
        });

        // Wait for all tasks to finish
        let (client_res, _, _) = tokio::try_join!(client_handle, tui_handle, mcp_progress_handle)
            .map_err(|e| e.to_string())?;

        let (final_messages, final_session_id, profile_switch_config, final_usage) = client_res?;

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
            if let Some(smart_model) = &new_config.smart_model {
                new_client_config = new_client_config.with_smart_model(smart_model.clone());
            }
            if let Some(eco_model) = &new_config.eco_model {
                new_client_config = new_client_config.with_eco_model(eco_model.clone());
            }
            if let Some(recovery_model) = &new_config.recovery_model {
                new_client_config = new_client_config.with_recovery_model(recovery_model.clone());
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

            // Update config with new rulebooks
            config.rulebooks = new_rulebooks;
            config.allowed_tools = new_config.allowed_tools.clone();
            config.auto_approve = new_config.auto_approve.clone();

            // Update ctx
            ctx = new_config;

            // Check if warden is enabled in the new profile and we're not already inside warden
            let should_use_warden = ctx.warden.as_ref().map(|w| w.enabled).unwrap_or(false)
                && std::env::var("STAKPAK_SKIP_WARDEN").is_err();

            if should_use_warden {
                // Set the profile environment variable so warden knows which profile to use
                // This is safe because we're setting it for the current process before re-execution
                unsafe {
                    std::env::set_var("STAKPAK_PROFILE", &ctx.profile_name);
                }

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
        if let Some(smart_model) = &ctx.smart_model {
            final_client_config = final_client_config.with_smart_model(smart_model.clone());
        }
        if let Some(eco_model) = &ctx.eco_model {
            final_client_config = final_client_config.with_eco_model(eco_model.clone());
        }
        if let Some(recovery_model) = &ctx.recovery_model {
            final_client_config = final_client_config.with_recovery_model(recovery_model.clone());
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
            println!("{}", renderer.render_token_usage_stats(&final_usage));
        }

        let username = client
            .get_my_account()
            .await
            .map(|account| account.username)?;

        let latest_checkpoint = final_messages
            .iter()
            .rev()
            .find(|m| m.role == stakpak_shared::models::integrations::openai::Role::Assistant)
            .and_then(|m| m.content.as_ref().and_then(|c| c.extract_checkpoint_id()));

        if let Some(latest_checkpoint) = latest_checkpoint {
            println!(
                r#"To resume, run:
stakpak -c {}
"#,
                latest_checkpoint
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
            "\x1b[35mFeedback or bug report?\x1b[0m \x1b[38;5;214mJoin our Discord:\x1b[0m \x1b[38;5;214mhttps://discord.gg/c4HUkDD45d\x1b[0m"
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
        let tool_call = ToolCall {
            id: "tool_1".to_string(),
            r#type: "function".to_string(),
            function: stakpak_shared::models::integrations::openai::FunctionCall {
                name: "test_tool".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let messages = vec![ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("test".to_string())),
            tool_calls: Some(vec![tool_call]),
            ..Default::default()
        }];

        let unresolved = get_unresolved_tool_call_ids(&messages);
        assert_eq!(unresolved, vec!["tool_1".to_string()]);
    }

    #[test]
    fn get_unresolved_tool_call_ids_returns_empty_when_all_resolved() {
        let tool_call = ToolCall {
            id: "tool_1".to_string(),
            r#type: "function".to_string(),
            function: stakpak_shared::models::integrations::openai::FunctionCall {
                name: "test_tool".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let messages = vec![
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("test".to_string())),
                tool_calls: Some(vec![tool_call]),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Tool,
                content: Some(MessageContent::String("result".to_string())),
                tool_call_id: Some("tool_1".to_string()),
                ..Default::default()
            },
        ];

        assert!(get_unresolved_tool_call_ids(&messages).is_empty());
    }

    #[test]
    fn get_unresolved_tool_call_ids_returns_only_unresolved() {
        let tool_call_1 = ToolCall {
            id: "tool_1".to_string(),
            r#type: "function".to_string(),
            function: stakpak_shared::models::integrations::openai::FunctionCall {
                name: "test_tool".to_string(),
                arguments: "{}".to_string(),
            },
        };
        let tool_call_2 = ToolCall {
            id: "tool_2".to_string(),
            r#type: "function".to_string(),
            function: stakpak_shared::models::integrations::openai::FunctionCall {
                name: "test_tool_2".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let messages = vec![
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("test".to_string())),
                tool_calls: Some(vec![tool_call_1, tool_call_2]),
                ..Default::default()
            },
            // Only tool_1 has a result
            ChatMessage {
                role: Role::Tool,
                content: Some(MessageContent::String("result".to_string())),
                tool_call_id: Some("tool_1".to_string()),
                ..Default::default()
            },
        ];

        let unresolved = get_unresolved_tool_call_ids(&messages);
        assert_eq!(unresolved, vec!["tool_2".to_string()]);
    }

    #[test]
    fn has_pending_tool_calls_returns_true_when_queue_not_empty() {
        let messages: Vec<ChatMessage> = vec![];
        let tools_queue = vec![ToolCall {
            id: "tool_1".to_string(),
            r#type: "function".to_string(),
            function: stakpak_shared::models::integrations::openai::FunctionCall {
                name: "test_tool".to_string(),
                arguments: "{}".to_string(),
            },
        }];

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
        let tool_call = ToolCall {
            id: "tool_1".to_string(),
            r#type: "function".to_string(),
            function: stakpak_shared::models::integrations::openai::FunctionCall {
                name: "test_tool".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let messages = vec![ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("test".to_string())),
            tool_calls: Some(vec![tool_call]),
            ..Default::default()
        }];
        let tools_queue: Vec<ToolCall> = vec![];

        assert!(has_pending_tool_calls(&messages, &tools_queue));
    }

    #[test]
    fn has_pending_tool_calls_returns_false_when_all_tool_calls_have_results() {
        let tool_call = ToolCall {
            id: "tool_1".to_string(),
            r#type: "function".to_string(),
            function: stakpak_shared::models::integrations::openai::FunctionCall {
                name: "test_tool".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let messages = vec![
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("test".to_string())),
                tool_calls: Some(vec![tool_call]),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Tool,
                content: Some(MessageContent::String("result".to_string())),
                tool_call_id: Some("tool_1".to_string()),
                ..Default::default()
            },
        ];
        let tools_queue: Vec<ToolCall> = vec![];

        assert!(!has_pending_tool_calls(&messages, &tools_queue));
    }

    #[test]
    fn has_pending_tool_calls_returns_true_when_some_tool_calls_missing_results() {
        let tool_call_1 = ToolCall {
            id: "tool_1".to_string(),
            r#type: "function".to_string(),
            function: stakpak_shared::models::integrations::openai::FunctionCall {
                name: "test_tool".to_string(),
                arguments: "{}".to_string(),
            },
        };
        let tool_call_2 = ToolCall {
            id: "tool_2".to_string(),
            r#type: "function".to_string(),
            function: stakpak_shared::models::integrations::openai::FunctionCall {
                name: "test_tool_2".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let messages = vec![
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("test".to_string())),
                tool_calls: Some(vec![tool_call_1, tool_call_2]),
                ..Default::default()
            },
            // Only tool_1 has a result, tool_2 is missing
            ChatMessage {
                role: Role::Tool,
                content: Some(MessageContent::String("result".to_string())),
                tool_call_id: Some("tool_1".to_string()),
                ..Default::default()
            },
        ];
        let tools_queue: Vec<ToolCall> = vec![];

        assert!(has_pending_tool_calls(&messages, &tools_queue));
    }

    #[test]
    fn has_pending_tool_calls_returns_false_when_assistant_has_empty_tool_calls() {
        let messages = vec![ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("test".to_string())),
            tool_calls: Some(vec![]), // Empty tool_calls
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
        let tool_call_old = ToolCall {
            id: "tool_old".to_string(),
            r#type: "function".to_string(),
            function: stakpak_shared::models::integrations::openai::FunctionCall {
                name: "old_tool".to_string(),
                arguments: "{}".to_string(),
            },
        };
        let tool_call_new = ToolCall {
            id: "tool_new".to_string(),
            r#type: "function".to_string(),
            function: stakpak_shared::models::integrations::openai::FunctionCall {
                name: "new_tool".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let messages = vec![
            // First assistant message with unresolved tool call
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("first".to_string())),
                tool_calls: Some(vec![tool_call_old]),
                ..Default::default()
            },
            // Result for the old tool
            ChatMessage {
                role: Role::Tool,
                content: Some(MessageContent::String("old result".to_string())),
                tool_call_id: Some("tool_old".to_string()),
                ..Default::default()
            },
            // User message
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("continue".to_string())),
                ..Default::default()
            },
            // Second (last) assistant message with tool call
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("second".to_string())),
                tool_calls: Some(vec![tool_call_new]),
                ..Default::default()
            },
            // Result for the new tool
            ChatMessage {
                role: Role::Tool,
                content: Some(MessageContent::String("new result".to_string())),
                tool_call_id: Some("tool_new".to_string()),
                ..Default::default()
            },
        ];
        let tools_queue: Vec<ToolCall> = vec![];

        // Should return false because the LAST assistant message's tool calls are resolved
        assert!(!has_pending_tool_calls(&messages, &tools_queue));
    }
}
