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
use stakpak_shared::models::integrations::openai::ChatMessage;

pub struct RunNonInteractiveConfig {
    pub prompt: String,
    pub approve: bool,
    pub verbose: bool,
    pub checkpoint_id: Option<String>,
    pub local_context: Option<LocalContext>,
    pub redact_secrets: bool,
}

pub async fn run_non_interactive(
    ctx: AppConfig,
    config: RunNonInteractiveConfig,
) -> Result<(), String> {
    let mut chat_messages: Vec<ChatMessage> = Vec::new();

    let ctx_clone = ctx.clone();
    let bind_address = network::find_available_bind_address_descending().await?;
    let local_mcp_server_host = format!("http://{}", bind_address);

    tokio::spawn(async move {
        let _ = stakpak_mcp_server::start_server(
            MCPServerConfig {
                api: ClientConfig {
                    api_key: ctx_clone.api_key.clone(),
                    api_endpoint: ctx_clone.api_endpoint.clone(),
                },
                redact_secrets: config.redact_secrets,
                bind_address,
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

    if let Some(message) = chat_messages.last() {
        if config.approve && message.tool_calls.is_some() {
            // Clone the tool_calls to avoid borrowing message while mutating chat_messages
            let tool_calls = message.tool_calls.as_ref().unwrap_or(&vec![]).clone();
            for tool_call in tool_calls.iter() {
                let result = run_tool_call(&clients, &tools_map, tool_call).await?;
                if let Some(result) = result {
                    if !config.verbose {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&result).unwrap_or_default()
                        );
                    }

                    let result_content = result
                        .content
                        .iter()
                        .map(|c| match c.raw.as_text() {
                            Some(text) => text.text.clone(),
                            None => String::new(),
                        })
                        .collect::<Vec<String>>()
                        .join("\n");

                    chat_messages.push(tool_result(tool_call.id.clone(), result_content.clone()));
                }
            }
        }
    }

    if !config.prompt.is_empty() {
        let (user_input, _local_context) =
            add_local_context(&chat_messages, &config.prompt, &config.local_context);
        chat_messages.push(user_message(user_input));
    }

    let response = client
        .chat_completion(chat_messages.clone(), Some(tools))
        .await
        .map_err(|e| e.to_string())?;

    chat_messages.push(response.choices[0].message.clone());

    match config.verbose {
        true => {
            println!(
                "{}",
                serde_json::to_string_pretty(&chat_messages).unwrap_or_default()
            );
        }
        false => {
            println!(
                "{}",
                serde_json::to_string_pretty(&response.choices[0].message).unwrap_or_default()
            );
        }
    }

    Ok(())
}
