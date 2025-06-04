use crate::commands::agent::run::checkpoint::{
    extract_checkpoint_messages_and_tool_calls, get_checkpoint_messages,
    get_messages_from_checkpoint_output,
};
use crate::commands::agent::run::helpers::{
    add_local_context, convert_tools_map, tool_result, user_message,
};
use crate::commands::agent::run::stream::process_responses_stream;
use crate::commands::agent::run::tooling::{list_sessions, run_tool_call};
use crate::commands::agent::run::tui::{send_input_event, send_tool_call};
use crate::config::AppConfig;
use crate::utils::check_update::get_latest_cli_version;
use crate::utils::local_context::LocalContext;
use crate::utils::network;
use stakpak_api::{Client, ClientConfig};
use stakpak_mcp_client::ClientManager;
use stakpak_mcp_server::MCPServerConfig;
use stakpak_shared::models::integrations::openai::{ChatMessage, ToolCall};
use stakpak_tui::{InputEvent, OutputEvent};
use uuid::Uuid;

pub struct RunInteractiveConfig {
    pub checkpoint_id: Option<String>,
    pub local_context: Option<LocalContext>,
    pub redact_secrets: bool,
}

pub async fn run_interactive(ctx: AppConfig, config: RunInteractiveConfig) -> Result<(), String> {
    let mut messages: Vec<ChatMessage> = Vec::new();
    let mut tools_queue: Vec<ToolCall> = Vec::new();
    let (input_tx, input_rx) = tokio::sync::mpsc::channel::<InputEvent>(100);
    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel::<OutputEvent>(100);
    let (mcp_progress_tx, mut mcp_progress_rx) = tokio::sync::mpsc::channel(100);
    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);

    let ctx_clone = ctx.clone();
    let bind_address = network::find_available_bind_address_descending().await?;
    let local_mcp_server_host = format!("http://{}", bind_address);

    // Spawn MCP server task
    let mcp_handle = tokio::spawn(async move {
        let _ = stakpak_mcp_server::start_server(
            MCPServerConfig {
                api: ClientConfig {
                    api_key: ctx_clone.api_key.clone(),
                    api_endpoint: ctx_clone.api_endpoint.clone(),
                },
                redact_secrets: config.redact_secrets,
                bind_address,
            },
            Some(shutdown_rx),
        )
        .await;
    });

    // Initialize clients and tools
    let clients = ClientManager::new(
        ctx.mcp_server_host.unwrap_or(local_mcp_server_host),
        Some(mcp_progress_tx),
    )
    .await
    .map_err(|e| e.to_string())?;
    let tools_map = clients.get_tools().await.map_err(|e| e.to_string())?;
    let tools = convert_tools_map(&tools_map);

    // Spawn TUI task
    let tui_handle = tokio::spawn(async move {
        let latest_version = get_latest_cli_version().await;
        let _ = stakpak_tui::run_tui(input_rx, output_tx, shutdown_tx, latest_version.ok())
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
    let client_handle: tokio::task::JoinHandle<Result<Vec<ChatMessage>, String>> =
        tokio::spawn(async move {
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

            while let Some(output_event) = output_rx.recv().await {
                match output_event {
                    OutputEvent::UserMessage(user_input) => {
                        let (user_input, local_context) =
                            add_local_context(&messages, &user_input, &config.local_context);
                        if let Some(local_context) = local_context {
                            send_input_event(
                                &input_tx,
                                InputEvent::InputSubmittedWith(local_context.to_string()),
                            )
                            .await?;
                        }
                        messages.push(user_message(user_input));
                    }
                    OutputEvent::AcceptTool(tool_call) => {
                        send_input_event(&input_tx, InputEvent::Loading(true)).await?;
                        let result = run_tool_call(&clients, &tools_map, &tool_call).await?;
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
                                    },
                                ),
                            )
                            .await?;
                            send_input_event(&input_tx, InputEvent::Loading(false)).await?;
                        }

                        if !tools_queue.is_empty() {
                            let tool_call = tools_queue.remove(0);
                            send_tool_call(&input_tx, &tool_call).await?;
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
                }
                send_input_event(&input_tx, InputEvent::Loading(true)).await?;

                let mut stream = client
                    .chat_completion_stream(messages.clone(), Some(tools.clone()))
                    .await?;

                let response = match process_responses_stream(&mut stream, &input_tx).await {
                    Ok(response) => response,
                    Err(e) => {
                        send_input_event(&input_tx, InputEvent::Loading(false)).await?;
                        input_tx
                            .send(InputEvent::Quit)
                            .await
                            .map_err(|e| e.to_string())?;
                        return Err(e.to_string());
                    }
                };
                send_input_event(&input_tx, InputEvent::Loading(false)).await?;

                messages.push(response.choices[0].message.clone());

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
        });

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
