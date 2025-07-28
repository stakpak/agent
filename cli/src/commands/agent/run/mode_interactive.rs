use crate::commands::agent::run::checkpoint::{
    extract_checkpoint_id_from_messages, extract_checkpoint_messages_and_tool_calls,
    get_checkpoint_messages, get_messages_from_checkpoint_output,
};
use crate::commands::agent::run::helpers::{
    add_local_context, add_rulebooks, convert_tools_map, tool_call_history_string, tool_result,
    user_message,
};
use crate::commands::agent::run::stream::process_responses_stream;
use crate::commands::agent::run::tooling::{list_sessions, run_tool_call};
use crate::commands::agent::run::tui::{send_input_event, send_tool_call};
use crate::config::AppConfig;
use crate::utils::check_update::get_latest_cli_version;
use crate::utils::local_context::LocalContext;
use crate::utils::network;
use stakpak_api::{Client, ClientConfig, ListRuleBook};
use stakpak_mcp_client::ClientManager;
use stakpak_mcp_server::{MCPServerConfig, ToolMode, start_server};
use stakpak_shared::cert_utils::CertificateChain;
use stakpak_shared::models::integrations::mcp::CallToolResultExt;
use stakpak_shared::models::integrations::openai::{ChatMessage, ToolCall, ToolCallResultStatus};
use stakpak_tui::{Color, InputEvent, OutputEvent};
use std::sync::Arc;
use uuid::Uuid;

pub struct RunInteractiveConfig {
    pub checkpoint_id: Option<String>,
    pub local_context: Option<LocalContext>,
    pub redact_secrets: bool,
    pub privacy_mode: bool,
    pub rulebooks: Option<Vec<ListRuleBook>>,
    pub enable_mtls: bool,
}

pub async fn run_interactive(ctx: AppConfig, config: RunInteractiveConfig) -> Result<(), String> {
    let mut messages: Vec<ChatMessage> = Vec::new();
    let mut tools_queue: Vec<ToolCall> = Vec::new();
    let (input_tx, input_rx) = tokio::sync::mpsc::channel::<InputEvent>(100);
    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel::<OutputEvent>(100);
    let (mcp_progress_tx, mut mcp_progress_rx) = tokio::sync::mpsc::channel(100);
    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);
    let (cancel_tx, cancel_rx) = tokio::sync::broadcast::channel::<()>(1);
    let ctx_clone = ctx.clone();
    let (bind_address, listener) = network::find_available_bind_address_with_listener().await?;

    // Generate certificates if mTLS is enabled
    let certificate_chain = Arc::new(if config.enable_mtls {
        Some(CertificateChain::generate().map_err(|e| e.to_string())?)
    } else {
        None
    });

    let protocol = if config.enable_mtls { "https" } else { "http" };
    let local_mcp_server_host = format!("{}://{}", protocol, bind_address);

    let certificate_chain_for_server = certificate_chain.clone();
    let mcp_handle = tokio::spawn(async move {
        let _ = start_server(
            MCPServerConfig {
                api: ClientConfig {
                    api_key: ctx_clone.api_key.clone(),
                    api_endpoint: ctx_clone.api_endpoint.clone(),
                },
                redact_secrets: config.redact_secrets,
                privacy_mode: config.privacy_mode,
                tool_mode: ToolMode::Combined,
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
        ctx.mcp_server_host.unwrap_or(local_mcp_server_host),
        Some(mcp_progress_tx),
        certificate_chain,
    )
    .await
    .map_err(|e| e.to_string())?;
    let tools_map = clients.get_tools().await.map_err(|e| e.to_string())?;
    let tools = convert_tools_map(&tools_map);

    // Spawn TUI task
    let tui_handle = tokio::spawn(async move {
        let latest_version = get_latest_cli_version().await;
        let _ = stakpak_tui::run_tui(
            input_rx,
            output_tx,
            Some(cancel_tx.clone()),
            shutdown_tx,
            latest_version.ok(),
            config.redact_secrets,
            config.privacy_mode,
        )
        .await
        .map_err(|e| e.to_string());
    });

    let input_tx_clone = input_tx.clone();
    let mcp_progress_handle = tokio::spawn(async move {
        while let Some(progress) = mcp_progress_rx.recv().await {
            let _ = send_input_event(&input_tx_clone, InputEvent::StreamToolResult(progress)).await;
        }
    });

    // Spawn client task
    let client_handle: tokio::task::JoinHandle<Result<Vec<ChatMessage>, String>> = tokio::spawn(
        async move {
            let client = Client::new(&ClientConfig {
                api_key: ctx.api_key.clone(),
                api_endpoint: ctx.api_endpoint.clone(),
            })
            .map_err(|e| e.to_string())?;

            let data = client.get_my_account().await?;
            send_input_event(&input_tx, InputEvent::GetStatus(data.to_text())).await?;

            if let Some(checkpoint_id) = config.checkpoint_id {
                let checkpoint_messages = get_checkpoint_messages(&client, &checkpoint_id).await?;

                let (chat_messages, tool_calls) = extract_checkpoint_messages_and_tool_calls(
                    &checkpoint_id,
                    &input_tx,
                    checkpoint_messages,
                )
                .await?;

                tools_queue.extend(tool_calls.clone());

                if !tools_queue.is_empty() {
                    let initial_tool_call = tools_queue.remove(0);
                    send_tool_call(&input_tx, &initial_tool_call).await?;
                }

                messages.extend(chat_messages);
            }

            let mut retry_attempts = 0;
            const MAX_RETRY_ATTEMPTS: u32 = 3;

            while let Some(output_event) = output_rx.recv().await {
                match output_event {
                    OutputEvent::UserMessage(user_input, tool_calls_results) => {
                        send_input_event(&input_tx, InputEvent::Loading(true)).await?;
                        let mut user_input = user_input.clone();

                        // Add user shell history to the user input
                        if let Some(tool_call_results) = &tool_calls_results {
                            if let Some(history_str) = tool_call_history_string(tool_call_results) {
                                user_input = format!("{}\n\n{}", history_str, user_input);
                            }
                        }

                        // Add local context to the user input
                        let (user_input, local_context) =
                            add_local_context(&messages, &user_input, &config.local_context);
                        if let Some(local_context) = local_context {
                            send_input_event(
                                &input_tx,
                                InputEvent::InputSubmittedWithColor(
                                    local_context.to_string(),
                                    Color::DarkGray,
                                ),
                            )
                            .await?;
                        }

                        // Add rulebooks to the user input
                        let (user_input, rulebooks_text) =
                            add_rulebooks(&messages, &user_input, &config.rulebooks);
                        if let Some(rulebooks_text) = rulebooks_text {
                            send_input_event(
                                &input_tx,
                                InputEvent::InputSubmittedWithColor(
                                    rulebooks_text,
                                    Color::DarkGray,
                                ),
                            )
                            .await?;
                        }

                        messages.push(user_message(user_input));
                    }
                    OutputEvent::AcceptTool(tool_call) => {
                        send_input_event(&input_tx, InputEvent::Loading(true)).await?;
                        let result = run_tool_call(
                            &clients,
                            &tools_map,
                            &tool_call,
                            Some(cancel_rx.resubscribe()),
                        )
                        .await?;

                        let mut should_stop = false;

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
                            send_input_event(&input_tx, InputEvent::Loading(false)).await?;

                            // Continue to next tool or main loop if error
                            should_stop = match result.get_status() {
                                ToolCallResultStatus::Cancelled => true,
                                ToolCallResultStatus::Error => false,
                                ToolCallResultStatus::Success => false,
                            };
                        }

                        // Process next tool in queue if available
                        if !tools_queue.is_empty() {
                            let next_tool_call = tools_queue.remove(0);
                            send_tool_call(&input_tx, &next_tool_call).await?;
                            continue;
                        }

                        // If there was an cancellation, stop the loop
                        if should_stop {
                            continue;
                        }
                    }
                    OutputEvent::RejectTool(_tool_call) => {
                        if !tools_queue.is_empty() {
                            let tool_call = tools_queue.remove(0);
                            send_tool_call(&input_tx, &tool_call).await?;
                        }
                        continue;
                    }

                    OutputEvent::ListSessions => {
                        match list_sessions(&client).await {
                            Ok(sessions) => {
                                send_input_event(&input_tx, InputEvent::SetSessions(sessions))
                                    .await?;
                            }
                            Err(e) => {
                                send_input_event(&input_tx, InputEvent::Error(e)).await?;
                            }
                        }
                        continue;
                    }
                    OutputEvent::SwitchToSession(session_id) => {
                        send_input_event(&input_tx, InputEvent::Loading(true)).await?;
                        let session_id = Uuid::parse_str(&session_id).map_err(|e| e.to_string())?;
                        match client.get_agent_session_latest_checkpoint(session_id).await {
                            Ok(checkpoint) => {
                                let (chat_messages, tool_calls) =
                                    extract_checkpoint_messages_and_tool_calls(
                                        &checkpoint.checkpoint.id.to_string(),
                                        &input_tx,
                                        get_messages_from_checkpoint_output(&checkpoint.output),
                                    )
                                    .await?;
                                messages.extend(chat_messages);

                                tools_queue.extend(tool_calls.clone());
                                if !tools_queue.is_empty() {
                                    let initial_tool_call = tools_queue.remove(0);
                                    send_tool_call(&input_tx, &initial_tool_call).await?;
                                }
                                send_input_event(&input_tx, InputEvent::Loading(false)).await?;
                            }
                            Err(e) => {
                                send_input_event(&input_tx, InputEvent::Loading(false)).await?;
                                send_input_event(&input_tx, InputEvent::Error(e)).await?;
                            }
                        }
                        continue;
                    }
                    OutputEvent::SendToolResult(tool_call_result) => {
                        send_input_event(&input_tx, InputEvent::Loading(true)).await?;
                        messages.push(tool_result(
                            tool_call_result.call.clone().id,
                            tool_call_result.result.clone(),
                        ));

                        send_input_event(&input_tx, InputEvent::Loading(false)).await?;

                        if !tools_queue.is_empty() {
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
                }

                let response = loop {
                    let mut stream = client
                        .chat_completion_stream(messages.clone(), Some(tools.clone()))
                        .await?;

                    match process_responses_stream(&mut stream, &input_tx).await {
                        Ok(response) => break response,
                        Err(e) => {
                            send_input_event(&input_tx, InputEvent::Loading(false)).await?;

                            // Handle retry logic for AgentInvalidResponseStream errors
                            if e.contains("AgentInvalidResponseStream")
                                && retry_attempts < MAX_RETRY_ATTEMPTS
                            {
                                retry_attempts += 1;

                                // Find the failed user message before removing it
                                let mut failed_user_message = None;
                                for msg in messages.iter().rev() {
                                    if msg.role
                                        == stakpak_shared::models::integrations::openai::Role::User
                                    {
                                        failed_user_message = Some(msg.clone());
                                        break;
                                    }
                                }

                                // Remove the failed conversation turn (user message and ANY assistant messages after it)
                                let mut user_index = None;
                                for (i, msg) in messages.iter().enumerate().rev() {
                                    if msg.role
                                        == stakpak_shared::models::integrations::openai::Role::User
                                    {
                                        user_index = Some(i);
                                        break;
                                    }
                                }

                                if let Some(index) = user_index {
                                    // Remove everything from the user message onwards
                                    // This includes the user message + any partial assistant responses
                                    messages.truncate(index);
                                }

                                // Show retry message in TUI
                                send_input_event(
                                    &input_tx,
                                    InputEvent::Error(format!("There was an issue sending your request, retrying attempt {}...", retry_attempts))
                                ).await?;

                                // Wait before retry (except first attempt)
                                if retry_attempts > 1 {
                                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                                }

                                // RETRY: Add the same user message back and try again
                                if let Some(user_msg) = failed_user_message {
                                    messages.push(user_msg);
                                }

                                send_input_event(&input_tx, InputEvent::Loading(true)).await?;
                                continue; // This continues the loop to retry
                            }

                            // Max retries reached - clean up everything including the user message
                            if retry_attempts >= MAX_RETRY_ATTEMPTS {
                                // Remove the entire failed conversation turn including the user message
                                let mut user_index = None;
                                for (i, msg) in messages.iter().enumerate().rev() {
                                    if msg.role
                                        == stakpak_shared::models::integrations::openai::Role::User
                                    {
                                        user_index = Some(i);
                                        break;
                                    }
                                }

                                if let Some(index) = user_index {
                                    messages.truncate(index); // Remove user message and everything after it
                                }

                                send_input_event(
                                    &input_tx,
                                    InputEvent::Error(
                                        "Maximum retry attempts reached. Please try again later."
                                            .to_string(),
                                    ),
                                )
                                .await?;
                            } else {
                                send_input_event(&input_tx, InputEvent::Error(e.clone())).await?;
                            }

                            return Err(e);
                        }
                    }
                };

                messages.push(response.choices[0].message.clone());

                send_input_event(&input_tx, InputEvent::Loading(false)).await?;

                // Send tool calls to TUI if present
                if let Some(tool_calls) = &response.choices[0].message.tool_calls {
                    tools_queue.extend(tool_calls.clone());
                    if !tools_queue.is_empty() {
                        let tool_call = tools_queue.remove(0);
                        send_tool_call(&input_tx, &tool_call).await?;
                        continue;
                    }
                }
            }

            Ok(messages)
        },
    );

    // Wait for all tasks to finish
    let (client_res, _, _, _) =
        tokio::try_join!(client_handle, tui_handle, mcp_handle, mcp_progress_handle)
            .map_err(|e| e.to_string())?;

    // Get latest checkpoint
    let latest_checkpoint = client_res?
        .iter()
        .rev()
        .find(|m| m.role == stakpak_shared::models::integrations::openai::Role::Assistant)
        .and_then(|m| m.content.as_ref().and_then(|c| c.extract_checkpoint_id()));

    if let Some(latest_checkpoint) = latest_checkpoint {
        println!(
            r#"
Terminating session at checkpoint {}

To resume, run:
stakpak -c {}

To get session data, run:
stakpak agent get {}
"#,
            latest_checkpoint, latest_checkpoint, latest_checkpoint
        );
    }

    Ok(())
}
