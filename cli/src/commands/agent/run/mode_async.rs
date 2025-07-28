use crate::commands::agent::run::checkpoint::get_checkpoint_messages;
use crate::commands::agent::run::helpers::{
    add_local_context, add_rulebooks, convert_tools_map, tool_result, user_message,
};
use crate::commands::agent::run::renderer::{OutputFormat, OutputRenderer};
use crate::commands::agent::run::tooling::run_tool_call;
use crate::config::AppConfig;
use crate::utils::local_context::LocalContext;
use crate::utils::network;

use stakpak_api::{Client, ClientConfig, ListRuleBook};
use stakpak_mcp_client::ClientManager;
use stakpak_mcp_server::{MCPServerConfig, ToolMode, start_server};
use stakpak_shared::cert_utils::CertificateChain;
use stakpak_shared::local_store::LocalStore;
use stakpak_shared::models::integrations::openai::ChatMessage;
use std::sync::Arc;
use std::time::Instant;

pub struct RunAsyncConfig {
    pub prompt: String,
    pub checkpoint_id: Option<String>,
    pub local_context: Option<LocalContext>,
    pub verbose: bool,
    pub redact_secrets: bool,
    pub privacy_mode: bool,
    pub rulebooks: Option<Vec<ListRuleBook>>,
    pub max_steps: Option<usize>,
    pub output_format: OutputFormat,
    pub enable_mtls: bool,
}

// All print functions have been moved to the renderer module and are no longer needed here

pub async fn run_async(ctx: AppConfig, config: RunAsyncConfig) -> Result<(), String> {
    let start_time = Instant::now();
    let mut llm_response_time = std::time::Duration::new(0, 0);
    let mut chat_messages: Vec<ChatMessage> = Vec::new();
    let renderer = OutputRenderer::new(config.output_format.clone(), config.verbose);

    print!("{}", renderer.render_title("Stakpak Agent - Async Mode"));
    print!(
        "{}",
        renderer.render_info("Initializing MCP server and client connections...")
    );
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
    tokio::spawn(async move {
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
            None,
        )
        .await;
    });

    // Initialize clients and tools
    let clients = ClientManager::new(
        ctx.mcp_server_host.unwrap_or(local_mcp_server_host),
        None,
        certificate_chain,
    )
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
        let checkpoint_start = Instant::now();
        let mut checkpoint_messages = get_checkpoint_messages(&client, &checkpoint_id).await?;
        llm_response_time += checkpoint_start.elapsed();

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
        print!(
            "{}",
            renderer.render_info(&format!("Resuming from checkpoint ({})", checkpoint_id))
        );
    }

    // Add user prompt if provided
    if !config.prompt.is_empty() {
        let (user_input, _local_context) =
            add_local_context(&chat_messages, &config.prompt, &config.local_context)
                .await
                .map_err(|e| e.to_string())?;
        let (user_input, _rulebooks_text) =
            add_rulebooks(&chat_messages, &user_input, &config.rulebooks);
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
            .chat_completion(chat_messages.clone(), Some(tools.clone()))
            .await
            .map_err(|e| e.to_string())?;
        llm_response_time += llm_start.elapsed();

        chat_messages.push(response.choices[0].message.clone());

        let tool_calls = response.choices[0].message.tool_calls.as_ref();
        let tool_count = tool_calls.map(|t| t.len()).unwrap_or(0);

        print!("{}", renderer.render_step_header(step, tool_count));

        // Show assistant response
        if let Some(content) = &response.choices[0].message.content {
            let content_str = match content {
                stakpak_shared::models::integrations::openai::MessageContent::String(s) => {
                    s.clone()
                }
                stakpak_shared::models::integrations::openai::MessageContent::Array(parts) => parts
                    .iter()
                    .filter_map(|part| part.text.as_ref())
                    .map(|text| text.as_str())
                    .filter(|text| !text.starts_with("<checkpoint_id>"))
                    .collect::<Vec<&str>>()
                    .join("\n"),
            };
            if !content_str.trim().is_empty() {
                print!("{}", renderer.render_assistant_message(&content_str, false));
            }
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

            // Execute all tool calls
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
                let tool_execution =
                    async { run_tool_call(&clients, &tools_map, tool_call, None).await };

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

    // Extract final checkpoint if available
    let latest_checkpoint = chat_messages
        .iter()
        .rev()
        .find(|m| m.role == stakpak_shared::models::integrations::openai::Role::Assistant)
        .and_then(|m| m.content.as_ref().and_then(|c| c.extract_checkpoint_id()));

    let elapsed = start_time.elapsed();
    let tool_execution_time = elapsed.saturating_sub(llm_response_time);

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

    print!("{}", renderer.render_final_completion(&chat_messages));

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
    if let Some(checkpoint_id) = &latest_checkpoint {
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

    Ok(())
}
