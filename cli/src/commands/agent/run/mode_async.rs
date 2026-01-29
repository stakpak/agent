use crate::agent::run::helpers::system_message;
use crate::commands::agent::run::helpers::{
    add_agents_md, add_local_context, add_rulebooks, add_subagents, tool_result, user_message,
};
use crate::commands::agent::run::mcp_init::{McpInitConfig, initialize_mcp_server_and_tools};
use crate::commands::agent::run::renderer::{OutputFormat, OutputRenderer};
use crate::commands::agent::run::tooling::run_tool_call;
use crate::config::AppConfig;
use crate::utils::agents_md::AgentsMdInfo;
use crate::utils::local_context::LocalContext;
use stakpak_api::{
    AgentClient, AgentClientConfig, AgentProvider, SessionStorage, models::ListRuleBook,
};
use stakpak_mcp_server::EnabledToolsConfig;
use stakpak_shared::local_store::LocalStore;
use stakpak_shared::models::integrations::openai::{AgentModel, ChatMessage};
use stakpak_shared::models::llm::LLMTokenUsage;
use stakpak_shared::models::subagent::SubagentConfigs;
use std::time::Instant;
use uuid::Uuid;

pub struct RunAsyncConfig {
    pub prompt: String,
    pub checkpoint_id: Option<String>,
    pub local_context: Option<LocalContext>,
    pub verbose: bool,
    pub redact_secrets: bool,
    pub privacy_mode: bool,
    pub rulebooks: Option<Vec<ListRuleBook>>,
    pub subagent_configs: Option<SubagentConfigs>,
    pub max_steps: Option<usize>,
    pub output_format: OutputFormat,
    pub allowed_tools: Option<Vec<String>>,
    pub enable_mtls: bool,
    pub system_prompt: Option<String>,
    pub enabled_tools: EnabledToolsConfig,
    pub model: AgentModel,
    pub agents_md: Option<AgentsMdInfo>,
}

// All print functions have been moved to the renderer module and are no longer needed here

pub async fn run_async(ctx: AppConfig, config: RunAsyncConfig) -> Result<(), String> {
    let start_time = Instant::now();
    let mut llm_response_time = std::time::Duration::new(0, 0);
    let mut chat_messages: Vec<ChatMessage> = Vec::new();
    let mut total_usage = LLMTokenUsage::default();
    let renderer = OutputRenderer::new(config.output_format.clone(), config.verbose);

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
    };
    let mcp_init_result = initialize_mcp_server_and_tools(&ctx, mcp_init_config, None).await?;
    let mcp_client = mcp_init_result.client;
    let mcp_tools = mcp_init_result.mcp_tools;
    let server_shutdown_tx = mcp_init_result.server_shutdown_tx;
    let proxy_shutdown_tx = mcp_init_result.proxy_shutdown_tx;

    // Filter tools if allowed_tools is specified
    let tools = if let Some(allowed) = &config.allowed_tools {
        mcp_init_result
            .tools
            .into_iter()
            .filter(|t| allowed.contains(&t.function.name))
            .collect()
    } else {
        mcp_init_result.tools
    };

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

    // Load checkpoint messages if provided
    if let Some(checkpoint_id_str) = config.checkpoint_id {
        let checkpoint_start = Instant::now();

        // Parse checkpoint UUID
        let checkpoint_uuid = Uuid::parse_str(&checkpoint_id_str)
            .map_err(|_| format!("Invalid checkpoint ID: {}", checkpoint_id_str))?;

        // Get checkpoint with session info
        match client.get_checkpoint(checkpoint_uuid).await {
            Ok(checkpoint) => {
                current_session_id = Some(checkpoint.session_id);
                current_checkpoint_id = Some(checkpoint_uuid);
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

    if let Some(system_prompt) = config.system_prompt {
        chat_messages.insert(0, system_message(system_prompt));
        print!("{}", renderer.render_info("System prompt loaded"));
    }

    // Add user prompt if provided
    if !config.prompt.is_empty() {
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

        let (user_input, _subagents_text) =
            add_subagents(&chat_messages, &user_input, &config.subagent_configs);

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
        let response_result = client
            .chat_completion(
                config.model.clone(),
                chat_messages.clone(),
                Some(tools.clone()),
                current_session_id,
            )
            .await;

        let response = match response_result {
            Ok(response) => response,
            Err(e) => {
                print!(
                    "{}",
                    renderer.render_error(&format!("Error during execution: {}", e))
                );
                break;
            }
        };
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
                let tool_execution = async {
                    run_tool_call(&mcp_client, &mcp_tools, tool_call, None, current_session_id)
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
    println!();

    // Print token usage at the end
    print!("{}", renderer.render_token_usage_stats(&total_usage));

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

        // Print resume command
        println!("\nTo resume, run:\nstakpak -c {}\n", checkpoint_id);
    } else {
        print!(
            "{}",
            renderer.render_info("No checkpoint available to save")
        );
    }

    // Attempt to print billing info (cost)
    if let Ok(account_data) = client.get_my_account().await {
        let billing_username = account_data
            .scope
            .as_ref()
            .map(|s| s.name.as_str())
            .unwrap_or(&account_data.username);

        if let Ok(billing_info) = client.get_billing_info(billing_username).await {
            let mut info_str = String::new();
            for (name, feature) in billing_info.features {
                if let Some(balance) = feature.balance {
                    info_str.push_str(&format!("  - {}: {:.2}\n", name, balance));
                }
                // Check for included usage as well which might represent "credits" in some contexts
                if let Some(usage) = feature.usage {
                    info_str.push_str(&format!("  - {} Usage: {:.2}\n", name, usage));
                }
            }

            if !info_str.is_empty() {
                print!("{}", renderer.render_info("Billing Status:"));
                print!("{}", renderer.render_info(&info_str));
            }
        }
    }

    // Print session ID if available
    if let Some(session_id) = current_session_id {
        println!("Session ID: {}", session_id);
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

    Ok(())
}
