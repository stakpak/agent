use crate::commands::agent::run::checkpoint::get_checkpoint_messages;
use crate::commands::agent::run::helpers::{
    add_local_context, convert_tools_map, tool_result, user_message,
};
use crate::commands::agent::run::tooling::run_tool_call;
use crate::config::AppConfig;
use crate::utils::local_context::LocalContext;
use crate::utils::network;
use stakpak_api::{Client, ClientConfig};
use stakpak_mcp_client::ClientManager;
use stakpak_mcp_server::MCPServerConfig;
use stakpak_shared::local_store::LocalStore;
use stakpak_shared::models::integrations::openai::ChatMessage;

pub struct RunAsyncConfig {
    pub prompt: String,
    pub checkpoint_id: Option<String>,
    pub local_context: Option<LocalContext>,
    pub verbose: bool,
    pub redact_secrets: bool,
}

pub async fn run_async(ctx: AppConfig, config: RunAsyncConfig) -> Result<(), String> {
    let mut chat_messages: Vec<ChatMessage> = Vec::new();

    let ctx_clone = ctx.clone();
    let bind_address = network::find_available_bind_address_descending().await?;
    let local_mcp_server_host = format!("http://{}", bind_address);
    let redact_secrets = config.redact_secrets;
    tokio::spawn(async move {
        let _ = stakpak_mcp_server::start_server(
            MCPServerConfig {
                api: ClientConfig {
                    api_key: ctx_clone.api_key.clone(),
                    api_endpoint: ctx_clone.api_endpoint.clone(),
                },
                bind_address,
                redact_secrets,
            },
            None,
        )
        .await;
    });

    let clients = ClientManager::new(ctx.mcp_server_host.unwrap_or(local_mcp_server_host), None)
        .await
        .map_err(|e| e.to_string())?;
    let tools_map = clients.get_tools().await.map_err(|e| e.to_string())?;
    let tools = convert_tools_map(&tools_map);

    let client = Client::new(&ClientConfig {
        api_key: ctx.api_key.clone(),
        api_endpoint: ctx.api_endpoint.clone(),
    })
    .map_err(|e| e.to_string())?;

    // Load checkpoint messages if provided
    if let Some(checkpoint_id) = config.checkpoint_id {
        let mut checkpoint_messages = get_checkpoint_messages(&client, &checkpoint_id).await?;

        // Append checkpoint_id to the last assistant message if present
        if let Some(last_message) = checkpoint_messages.iter_mut().rev().find(|message| {
            message.role != stakpak_shared::models::integrations::openai::Role::User
                && message.role != stakpak_shared::models::integrations::openai::Role::Tool
        }) {
            if last_message.role == stakpak_shared::models::integrations::openai::Role::Assistant {
                last_message.content = Some(
                    stakpak_shared::models::integrations::openai::MessageContent::String(format!(
                        "{}\n<checkpoint_id>{}</checkpoint_id>",
                        last_message.content.as_ref().unwrap_or(
                            &stakpak_shared::models::integrations::openai::MessageContent::String(
                                String::new()
                            )
                        ),
                        checkpoint_id
                    )),
                );
            }
        }
        chat_messages.extend(checkpoint_messages);
    }

    // Add user prompt if provided
    if !config.prompt.is_empty() {
        let (user_input, _local_context) =
            add_local_context(&chat_messages, &config.prompt, &config.local_context);
        chat_messages.push(user_message(user_input));
    }

    let mut step = 0;
    let max_steps = 50; // Safety limit to prevent infinite loops

    loop {
        step += 1;
        if step > max_steps {
            println!(
                "[Reached maximum steps limit ({}), stopping execution]",
                max_steps
            );
            break;
        }

        // Make chat completion request
        let response = client
            .chat_completion(chat_messages.clone(), Some(tools.clone()))
            .await
            .map_err(|e| e.to_string())?;

        chat_messages.push(response.choices[0].message.clone());
        println!(
            "--[Step {}]---------------------------------------\n{}Running {} tools\n-------------------------------------------------\n",
            step,
            if config.verbose {
                format!(
                    "{}\n\n",
                    response.choices[0].message.content.as_ref().unwrap_or(
                        &stakpak_shared::models::integrations::openai::MessageContent::String(
                            String::new()
                        )
                    )
                )
            } else {
                String::new()
            },
            response.choices[0]
                .message
                .tool_calls
                .as_ref()
                .unwrap_or(&vec![])
                .len()
        );

        // Check if there are tool calls to execute
        if let Some(tool_calls) = &response.choices[0].message.tool_calls {
            if tool_calls.is_empty() {
                break;
            }

            // Execute all tool calls
            for (i, tool_call) in tool_calls.iter().enumerate() {
                let result = run_tool_call(&clients, &tools_map, tool_call).await?;
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

                    if config.verbose {
                        println!(
                            "[{} {} result]--------------------------------\n{}\n--------------------------------",
                            i + 1,
                            tool_call.function.name,
                            result_content
                        );
                    }

                    chat_messages.push(tool_result(tool_call.id.clone(), result_content.clone()));
                }
            }

            // Save conversation to file
            let conversation_json =
                serde_json::to_string_pretty(&chat_messages).unwrap_or_default();

            match LocalStore::write_session_data("messages.json", &conversation_json) {
                Ok(path) => {
                    println!("{} messages saved to {}", chat_messages.len(), path);
                }
                Err(e) => {
                    eprintln!("Failed to write messages to file: {}", e);
                }
            }
        } else {
            break;
        }
    }

    // Extract final checkpoint if available
    let latest_checkpoint = chat_messages
        .iter()
        .rev()
        .find(|m| m.role == stakpak_shared::models::integrations::openai::Role::Assistant)
        .and_then(|m| m.content.as_ref().and_then(|c| c.extract_checkpoint_id()));

    println!("Async execution completed after {} steps", step - 1);

    // Save checkpoint to file if available
    if let Some(checkpoint_id) = &latest_checkpoint {
        match LocalStore::write_session_data("checkpoint", checkpoint_id.to_string().as_str()) {
            Ok(path) => {
                println!("Checkpoint {} saved to {}", checkpoint_id, path);
            }
            Err(e) => {
                eprintln!("Failed to write checkpoint to file: {}", e);
            }
        }
    }

    Ok(())
}
