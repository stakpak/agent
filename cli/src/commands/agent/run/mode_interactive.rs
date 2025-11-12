use crate::agent::run::helpers::system_message;
use crate::commands::agent::run::checkpoint::{
    extract_checkpoint_id_from_messages, extract_checkpoint_messages_and_tool_calls,
    get_checkpoint_messages, prepare_checkpoint_messages_and_tool_calls,
    resume_session_from_checkpoint,
};
use crate::commands::agent::run::helpers::{
    add_local_context, add_rulebooks_with_force, add_subagents, convert_tools_map_with_filter,
    tool_call_history_string, tool_result, user_message,
};
use crate::commands::agent::run::renderer::{OutputFormat, OutputRenderer};
use crate::commands::agent::run::stream::{StreamProcessingResult, process_responses_stream};
use crate::commands::agent::run::tooling::{list_sessions, run_tool_call};
use crate::commands::agent::run::tui::{send_input_event, send_tool_call};
use crate::config::AppConfig;
use crate::utils::check_update::get_latest_cli_version;
use crate::utils::local_context::LocalContext;
use crate::utils::network;
use reqwest::header::HeaderMap;
use stakpak_api::models::ApiStreamError;
use stakpak_api::models::{RecoveryActionRequest, RecoveryMode};
use stakpak_api::{Client, ClientConfig, ListRuleBook};
use stakpak_mcp_client::ClientManager;
use stakpak_mcp_server::{EnabledToolsConfig, MCPServerConfig, ToolMode, start_server};
use stakpak_shared::cert_utils::CertificateChain;
use stakpak_shared::models::integrations::mcp::CallToolResultExt;
use stakpak_shared::models::integrations::openai::{ChatMessage, ToolCall, ToolCallResultStatus};
use stakpak_shared::models::subagent::SubagentConfigs;
use stakpak_tui::{InputEvent, LoadingOperation, OutputEvent};
use std::sync::Arc;
use uuid::Uuid;

type ClientTaskResult = Result<
    (
        Vec<ChatMessage>,
        Option<Uuid>,
        Option<AppConfig>,
        stakpak_shared::models::integrations::openai::Usage,
    ),
    String,
>;

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
}

pub async fn run_interactive(
    mut ctx: AppConfig,
    mut config: RunInteractiveConfig,
) -> Result<(), String> {
    // Outer loop for profile switching
    'profile_switch_loop: loop {
        let mut messages: Vec<ChatMessage> = Vec::new();
        let mut tools_queue: Vec<ToolCall> = Vec::new();
        let mut should_update_rulebooks_on_next_message = false;
        let mut total_session_usage = stakpak_shared::models::integrations::openai::Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            prompt_tokens_details: None,
        };

        // Clone config values for this iteration
        let api_key = ctx.api_key.clone();
        let api_endpoint = ctx.api_endpoint.clone();
        let config_path = ctx.config_path.clone();
        let mcp_server_host = ctx.mcp_server_host.clone();
        let local_context = config.local_context.clone();
        let mut rulebooks = config.rulebooks.clone();
        let mut all_available_rulebooks: Option<Vec<ListRuleBook>> = None;
        let system_prompt = config.system_prompt.clone();
        let subagent_configs = config.subagent_configs.clone();
        let checkpoint_id = config.checkpoint_id.clone();
        let allowed_tools = config.allowed_tools.clone();
        let auto_approve = config.auto_approve.clone();
        let enabled_tools = config.enabled_tools.clone();
        let redact_secrets = config.redact_secrets;
        let privacy_mode = config.privacy_mode;
        let enable_mtls = config.enable_mtls;
        let is_git_repo = config.is_git_repo;
        let study_mode = config.study_mode;

        // Create fresh channels for this iteration
        let (input_tx, input_rx) = tokio::sync::mpsc::channel::<InputEvent>(100);
        let (output_tx, mut output_rx) = tokio::sync::mpsc::channel::<OutputEvent>(100);
        let output_tx_for_tui = output_tx.clone();
        let output_tx_for_client = output_tx.clone();
        let (mcp_progress_tx, mut mcp_progress_rx) = tokio::sync::mpsc::channel(100);
        let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);
        let (cancel_tx, cancel_rx) = tokio::sync::broadcast::channel::<()>(1);
        let ctx_clone = ctx.clone();
        let (bind_address, listener) = network::find_available_bind_address_with_listener().await?;

        // Generate certificates if mTLS is enabled
        let certificate_chain = Arc::new(if enable_mtls {
            Some(CertificateChain::generate().map_err(|e| e.to_string())?)
        } else {
            None
        });

        let protocol = if enable_mtls { "https" } else { "http" };
        let local_mcp_server_host = format!("{}://{}", protocol, bind_address);

        let certificate_chain_for_server = certificate_chain.clone();
        let subagent_configs_for_server = subagent_configs.clone();
        let mcp_handle = tokio::spawn(async move {
            let _ = start_server(
                MCPServerConfig {
                    api: ClientConfig {
                        api_key: ctx_clone.api_key.clone(),
                        api_endpoint: ctx_clone.api_endpoint.clone(),
                    },
                    redact_secrets,
                    privacy_mode,
                    enabled_tools,
                    tool_mode: ToolMode::Combined,
                    subagent_configs: subagent_configs_for_server,
                    bind_address,
                    certificate_chain: certificate_chain_for_server,
                },
                Some(listener),
                Some(shutdown_rx),
            )
            .await;
        });

        // Initialize clients and tools
        let clients = ClientManager::new(
            mcp_server_host.unwrap_or(local_mcp_server_host.clone()),
            Some(mcp_progress_tx),
            certificate_chain,
        )
        .await
        .map_err(|e| e.to_string())?;
        let tools_map = clients.get_tools().await.map_err(|e| e.to_string())?;
        let tools = convert_tools_map_with_filter(&tools_map, allowed_tools.as_ref());

        // Spawn TUI task
        let shutdown_tx_for_tui = shutdown_tx.clone();
        let current_profile_for_tui = ctx.profile_name.clone();
        let rulebook_config_for_tui = ctx.rulebooks.clone().map(|rb| stakpak_tui::RulebookConfig {
            include: rb.include,
            exclude: rb.exclude,
            include_tags: rb.include_tags,
            exclude_tags: rb.exclude_tags,
        });
        let tui_handle = tokio::spawn(async move {
            let latest_version = get_latest_cli_version().await;
            stakpak_tui::run_tui(
                input_rx,
                output_tx_for_tui,
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
            )
            .await
            .map_err(|e| e.to_string())
        });

        let input_tx_clone = input_tx.clone();
        let mcp_progress_handle = tokio::spawn(async move {
            while let Some(progress) = mcp_progress_rx.recv().await {
                let _ =
                    send_input_event(&input_tx_clone, InputEvent::StreamToolResult(progress)).await;
            }
        });

        // Spawn client task
        let api_key_for_client = api_key.clone();
        let api_endpoint_for_client = api_endpoint.clone();
        let shutdown_tx_for_client = shutdown_tx.clone();
        let client_handle: tokio::task::JoinHandle<ClientTaskResult> = tokio::spawn(async move {
            let mut current_session_id: Option<Uuid> = None;
            let client = Client::new(&ClientConfig {
                api_key: api_key_for_client.clone(),
                api_endpoint: api_endpoint_for_client.clone(),
            })
            .map_err(|e| e.to_string())?;

            let data = client.get_my_account().await?;
            send_input_event(&input_tx, InputEvent::GetStatus(data.to_text())).await?;
            // Load available profiles and send to TUI
            let profiles_config_path = ctx.config_path.clone();
            let current_profile_name = ctx.profile_name.clone();
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
                if let Ok(checkpoint_with_session) =
                    client.get_agent_checkpoint(checkpoint_uuid).await
                {
                    current_session_id = Some(checkpoint_with_session.session.id);
                }

                let checkpoint_messages =
                    get_checkpoint_messages(&client, &checkpoint_id_str).await?;

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
                    OutputEvent::UserMessage(user_input, tool_calls_results) => {
                        // Loading will be managed by stream processing
                        let mut user_input = user_input.clone();

                        // Add user shell history to the user input
                        if let Some(tool_call_results) = &tool_calls_results
                            && let Some(history_str) = tool_call_history_string(tool_call_results)
                        {
                            user_input = format!("{}\n\n{}", history_str, user_input);
                        }

                        // Add local context to the user input
                        // Add local context and rulebooks only in specific cases:
                        // 1. First message of new session (messages.is_empty())
                        // 2. Session resume or rulebook update (should_update_rulebooks_on_next_message)
                        let (user_input, _) =
                            if messages.is_empty() || should_update_rulebooks_on_next_message {
                                // Add local context first
                                let (user_input_with_context, _) =
                                    add_local_context(&messages, &user_input, &local_context, true)
                                        .await
                                        .map_err(|e| {
                                            format!("Failed to format local context: {}", e)
                                        })?;

                                // Then add rulebooks
                                let (user_input_with_rulebooks, _) = add_rulebooks_with_force(
                                    &user_input_with_context,
                                    &rulebooks,
                                    true,
                                );
                                should_update_rulebooks_on_next_message = false; // Reset the flag
                                (user_input_with_rulebooks, None::<String>)
                            } else {
                                // Don't add local context or rulebooks for regular messages
                                (user_input.to_string(), None::<String>)
                            };

                        let (user_input, _) =
                            add_subagents(&messages, &user_input, &subagent_configs);

                        send_input_event(&input_tx, InputEvent::HasUserMessage).await?;
                        send_input_event(&input_tx, InputEvent::ResetAutoApproveMessage).await?;
                        tools_queue.clear();
                        messages.push(user_message(user_input));
                    }
                    OutputEvent::AcceptTool(tool_call) => {
                        send_input_event(
                            &input_tx,
                            InputEvent::StartLoadingOperation(LoadingOperation::ToolExecution),
                        )
                        .await?;
                        let result = run_tool_call(
                            &clients,
                            &tools_map,
                            &tool_call,
                            Some(cancel_rx.resubscribe()),
                            current_session_id,
                        )
                        .await?;

                        let mut should_stop = false;

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
                        match list_sessions(&client).await {
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
                        continue;
                    }

                    OutputEvent::ResumeSession => {
                        let session_id = if let Some(session_id) = &current_session_id {
                            Some(session_id.to_string())
                        } else {
                            list_sessions(&client).await.ok().and_then(|sessions| {
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
                            match resume_session_from_checkpoint(&client, session_id, &input_tx)
                                .await
                            {
                                Ok((chat_messages, tool_calls, session_id_uuid)) => {
                                    // Track the current session ID
                                    current_session_id = Some(session_id_uuid);

                                    // Mark that we need to update rulebooks on the next user message
                                    should_update_rulebooks_on_next_message = true;

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
                        match resume_session_from_checkpoint(&client, &session_id, &input_tx).await
                        {
                            Ok((chat_messages, tool_calls, session_id_uuid)) => {
                                // Track the current session ID
                                current_session_id = Some(session_id_uuid);

                                // Mark that we need to update rulebooks on the next user message
                                should_update_rulebooks_on_next_message = true;

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
                    OutputEvent::RecoveryAction {
                        action,
                        recovery_request_id,
                        selected_option_id,
                        mode,
                    } => {
                        let Some(session_id) = current_session_id else {
                            send_input_event(
                                &input_tx,
                                InputEvent::Error(
                                    "Cannot process recovery action without active session".into(),
                                ),
                            )
                            .await?;
                            continue;
                        };

                        let Some(recovery_request_id) = recovery_request_id else {
                            send_input_event(
                                &input_tx,
                                InputEvent::Error(
                                    "Recovery request id unavailable for selected option".into(),
                                ),
                            )
                            .await?;
                            continue;
                        };

                        let request = RecoveryActionRequest {
                            action,
                            selected_option_id: Some(selected_option_id),
                        };

                        match client
                            .submit_recovery_action(
                                session_id,
                                &recovery_request_id,
                                request.action,
                                Some(selected_option_id),
                            )
                            .await
                        {
                            Ok(()) => {
                                eprintln!(
                                    "Submitted recovery action {:?} for session {}; recovery_request_id={}; option={}",
                                    request.action,
                                    session_id,
                                    recovery_request_id,
                                    selected_option_id,
                                );
                                let mode_label = match mode {
                                    RecoveryMode::Redirection => "REDIRECTION",
                                    RecoveryMode::Revert => "REVERT",
                                    RecoveryMode::ModelChange => "MODELCHANGE",
                                };

                                let _ = output_tx_for_client.try_send(OutputEvent::UserMessage(
                                    format!("Proceeding with recovery option [{}]", mode_label),
                                    None,
                                ));
                            }
                            Err(err) => {
                                eprintln!(
                                    "Failed to submit recovery action {:?} for session {}; recovery_request_id={}; option={} -> {}",
                                    request.action,
                                    session_id,
                                    recovery_request_id,
                                    selected_option_id,
                                    err,
                                );
                                send_input_event(
                                    &input_tx,
                                    InputEvent::Error(format!(
                                        "Failed to submit recovery action: {}",
                                        err
                                    )),
                                )
                                .await?;
                            }
                        }

                        continue;
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
                            messages.clone(),
                            Some(tools.clone()),
                            headers.clone(),
                        )
                        .await;

                    let (mut stream, current_request_id) = match stream_result {
                        Ok(result) => result,
                        Err(e) => {
                            send_input_event(&input_tx, InputEvent::Error(e.clone())).await?;
                            break Err(ApiStreamError::Unknown(e));
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
                        Ok(StreamProcessingResult { response, metadata }) => {
                            retry_attempts = 0;
                            break Ok(StreamProcessingResult { response, metadata });
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
                                    send_input_event(
                                        &input_tx,
                                        InputEvent::Error("MAX_RETRY_REACHED".to_string()),
                                    )
                                    .await?;
                                    break Err(e);
                                }
                            } else {
                                send_input_event(&input_tx, InputEvent::Error(format!("{:?}", e)))
                                    .await?;
                                break Err(e);
                            }
                        }
                    }
                };

                match response_result {
                    Ok(processed) => {
                        let StreamProcessingResult { response, metadata } = processed;
                        let mut history_synced = false;

                        if let Some(metadata) = metadata
                            && metadata
                                .get("history_updated")
                                .and_then(|value| value.as_bool())
                                .unwrap_or(false)
                            && let Some(checkpoint_id) = metadata
                                .get("checkpoint_id")
                                .and_then(|value| value.as_str())
                                .filter(|checkpoint_id| !checkpoint_id.is_empty())
                        {
                            match get_checkpoint_messages(&client, &checkpoint_id.to_string()).await
                            {
                                Ok(checkpoint_messages) => {
                                    let (chat_messages, _tool_calls) =
                                        prepare_checkpoint_messages_and_tool_calls(
                                            &checkpoint_id.to_string(),
                                            checkpoint_messages,
                                        );
                                    messages = chat_messages;
                                    history_synced = true;
                                }
                                Err(err) => {
                                    let _ =
                                        send_input_event(&input_tx, InputEvent::Error(err)).await;
                                }
                            }
                        }

                        if !history_synced {
                            messages.push(response.choices[0].message.clone());
                        }

                        // Accumulate usage from response
                        total_session_usage.prompt_tokens += response.usage.prompt_tokens;
                        total_session_usage.completion_tokens += response.usage.completion_tokens;
                        total_session_usage.total_tokens += response.usage.total_tokens;

                        // Accumulate prompt token details if available
                        if let Some(response_details) = &response.usage.prompt_tokens_details {
                            if total_session_usage.prompt_tokens_details.is_none() {
                                total_session_usage.prompt_tokens_details = Some(
                                    stakpak_shared::models::integrations::openai::PromptTokensDetails {
                                        input_tokens: response_details.input_tokens,
                                        output_tokens: response_details.output_tokens,
                                        cache_read_input_tokens: response_details.cache_read_input_tokens,
                                        cache_write_input_tokens: response_details.cache_write_input_tokens,
                                    },
                                );
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

                        if current_session_id.is_none()
                            && let Some(checkpoint_id) =
                                extract_checkpoint_id_from_messages(&messages)
                            && let Ok(checkpoint_uuid) = Uuid::parse_str(&checkpoint_id)
                            && let Ok(checkpoint_with_session) =
                                client.get_agent_checkpoint(checkpoint_uuid).await
                        {
                            current_session_id = Some(checkpoint_with_session.session.id);
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

                        if let Some(session_id) = current_session_id {
                            eprintln!("session_id: {:?}", session_id);
                            match client
                                .get_recovery_options(session_id, Some("pending"))
                                .await
                            {
                                Ok(recovery_response) => {
                                    eprintln!("Recovery response: {:?}", recovery_response);
                                    send_input_event(
                                        &input_tx,
                                        InputEvent::RecoveryOptions(recovery_response),
                                    )
                                    .await?;
                                }
                                Err(err) => {
                                    let message =
                                        format!("Failed to fetch recovery options: {}", err);
                                    send_input_event(&input_tx, InputEvent::Error(message)).await?;
                                }
                            }
                        }

                        send_input_event(&input_tx, InputEvent::ResetAutoApproveMessage).await?;
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
        let (client_res, _, _, _) =
            tokio::try_join!(client_handle, tui_handle, mcp_handle, mcp_progress_handle)
                .map_err(|e| e.to_string())?;

        let (final_messages, final_session_id, profile_switch_config, final_usage) = client_res?;

        // Check if profile switch was requested
        if let Some(new_config) = profile_switch_config {
            // Profile switch requested - update config and restart

            // All tasks have already exited from try_join
            // Give a moment for cleanup
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

            // Fetch and filter rulebooks for the new profile
            let client = Client::new(&ClientConfig {
                api_key: new_config.api_key.clone(),
                api_endpoint: new_config.api_endpoint.clone(),
            })
            .map_err(|e| e.to_string())?;

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

            // Update ctx and restart
            ctx = new_config;
            continue 'profile_switch_loop;
        }

        // Normal exit - no profile switch requested
        // Display final stats and session info
        let client = Client::new(&ClientConfig {
            api_key: ctx.api_key.clone(),
            api_endpoint: ctx.api_endpoint.clone(),
        })
        .map_err(|e| e.to_string())?;

        // Display session stats
        if let Some(session_id) = final_session_id {
            match client.get_agent_session_stats(session_id).await {
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

To get session data, run:
stakpak agent get {}
"#,
                latest_checkpoint, latest_checkpoint
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
