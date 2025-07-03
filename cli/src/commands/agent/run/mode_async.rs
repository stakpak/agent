use crate::commands::agent::run::checkpoint::get_checkpoint_messages;
use crate::commands::agent::run::helpers::{
    add_local_context, add_rulebooks, convert_tools_map, tool_result, user_message,
};
use crate::commands::agent::run::tooling::run_tool_call;
use crate::config::AppConfig;
use crate::utils::local_context::LocalContext;
use crate::utils::network;
use serde_json::Value;
use stakpak_api::{Client, ClientConfig, ListRuleBook};
use stakpak_mcp_client::ClientManager;
use stakpak_mcp_server::{MCPServerConfig, ToolMode};
use stakpak_shared::local_store::LocalStore;
use stakpak_shared::models::integrations::openai::ChatMessage;
use std::time::Instant;

pub struct RunAsyncConfig {
    pub prompt: String,
    pub checkpoint_id: Option<String>,
    pub local_context: Option<LocalContext>,
    pub verbose: bool,
    pub redact_secrets: bool,
    pub rulebooks: Option<Vec<ListRuleBook>>,
    pub max_steps: Option<usize>,
}

fn print_header(title: &str) {
    println!("╭─────────────────────────────────────────────────────────────────────────────────╮");
    println!("│ {:<79} │", title);
    println!("╰─────────────────────────────────────────────────────────────────────────────────╯");
}

fn print_step_header(step: usize, total_tools: usize) {
    println!();
    let header_text = if total_tools > 0 {
        format!(
            "Step {} - Executing {} tool{}",
            step,
            total_tools,
            if total_tools == 1 { "" } else { "s" }
        )
    } else {
        format!("Step {} - Agent response", step)
    };

    println!("{}", header_text);
    println!("{}", "─".repeat(header_text.chars().count()));
}

fn truncate_yaml_value(value: &Value, max_length: usize) -> Value {
    match value {
        Value::String(s) => {
            if s.chars().count() > max_length {
                let truncated: String = s.chars().take(max_length).collect();
                Value::String(format!(
                    "{}... [truncated - {} chars total]",
                    truncated,
                    s.chars().count()
                ))
            } else {
                value.clone()
            }
        }
        Value::Array(arr) => {
            if arr.len() > 5 {
                let mut truncated = arr.iter().take(5).cloned().collect::<Vec<_>>();
                truncated.push(Value::String(format!(
                    "... [truncated - {} items total]",
                    arr.len()
                )));
                Value::Array(truncated)
            } else {
                Value::Array(
                    arr.iter()
                        .map(|v| truncate_yaml_value(v, max_length))
                        .collect(),
                )
            }
        }
        Value::Object(obj) => {
            let mut truncated_obj = serde_json::Map::new();
            for (k, v) in obj {
                truncated_obj.insert(k.clone(), truncate_yaml_value(v, max_length));
            }
            Value::Object(truncated_obj)
        }
        _ => value.clone(),
    }
}

fn print_tool_start(tool_name: &str, tool_params: &str, tool_index: usize, total_tools: usize) {
    println!("Tool {}/{}: {}", tool_index + 1, total_tools, tool_name);

    if !tool_params.trim().is_empty() {
        if let Ok(params_json) = serde_json::from_str::<Value>(tool_params) {
            let truncated_params = truncate_yaml_value(&params_json, 200);
            if let Ok(pretty_json) = serde_json::to_string_pretty(&truncated_params) {
                println!("  Arguments:");
                for line in pretty_json.lines() {
                    println!("    {}", line);
                }
            } else {
                println!("  Arguments: {}", tool_params);
            }
        } else {
            println!("  Arguments: {}", tool_params);
        }
    }
}

fn print_tool_result(result: &str, verbose: bool) {
    println!("  Result:");
    if verbose {
        for line in result.lines() {
            println!("    {}", line);
        }
        println!(); // Add blank line after verbose tool output
    } else {
        // Show truncated result
        let first_line = result.lines().next().unwrap_or("").trim();
        if !first_line.is_empty() {
            let truncated = if first_line.chars().count() > 80 {
                let truncated_chars: String = first_line.chars().take(80).collect();
                format!("{}...", truncated_chars)
            } else {
                first_line.to_string()
            };
            println!("    {}", truncated);
        }
    }
}

fn print_info(message: &str) {
    println!("[info] {}", message);
}

fn print_success(message: &str) {
    println!("[success] {}", message);
}

fn print_warning(message: &str) {
    println!("[warning] {}", message);
}

fn print_error(message: &str) {
    println!("[error] {}", message);
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut wrapped_lines = Vec::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            wrapped_lines.push(String::new());
            continue;
        }

        if line.chars().count() <= max_width {
            wrapped_lines.push(line.to_string());
        } else {
            let mut current_line = String::new();
            for word in line.split_whitespace() {
                if current_line.is_empty() {
                    current_line = word.to_string();
                } else if current_line.chars().count() + 1 + word.chars().count() <= max_width {
                    current_line.push(' ');
                    current_line.push_str(word);
                } else {
                    wrapped_lines.push(current_line);
                    current_line = word.to_string();
                }
            }
            if !current_line.is_empty() {
                wrapped_lines.push(current_line);
            }
        }
    }

    wrapped_lines
}

fn format_xml_tags_as_boxes(content: &str) -> String {
    let mut result = content.to_string();

    // Handle common XML tags manually to avoid regex dependency
    let tags_to_handle = ["reasoning", "report", "todo"];

    for tag in &tags_to_handle {
        let start_tag = format!("<{}>", tag);
        let end_tag = format!("</{}>", tag);

        while let Some(start_pos) = result.find(&start_tag) {
            if let Some(relative_end_pos) = result[start_pos..].find(&end_tag) {
                let actual_end_pos = start_pos + relative_end_pos;
                let content_start = start_pos + start_tag.len();
                let tag_content = result[content_start..actual_end_pos].trim();
                let full_end_pos = actual_end_pos + end_tag.len();

                if !tag_content.is_empty() {
                    // Wrap content to 76 characters (leaving room for "│ " prefix)
                    let wrapped_lines = wrap_text(tag_content, 76);

                    // Calculate the actual box width needed
                    let max_content_width = wrapped_lines
                        .iter()
                        .map(|line| line.chars().count())
                        .max()
                        .unwrap_or(0);

                    // Calculate minimum width needed for header: "┌─ tag ─┐"
                    let tag_name = tag;
                    let min_header_width = tag_name.chars().count() + 7; // "┌─ " + tag + " ─┐"
                    let content_width = max_content_width + 4; // "│ " + content + " │"
                    let box_width = std::cmp::max(min_header_width, content_width).max(20);

                    // Create header: ┌─ TAG ─────────────┐
                    let tag_part = format!("─ {} ", tag_name);
                    let remaining_width = box_width - 2; // Subtract "┌" and "┐"
                    let padding_width = remaining_width.saturating_sub(tag_part.chars().count());
                    let header = format!("┌{}{}┐", tag_part, "─".repeat(padding_width));
                    let footer = format!("└{}┘", "─".repeat(box_width - 2));

                    // Format content lines with proper padding
                    let content_lines = wrapped_lines
                        .iter()
                        .map(|line| {
                            let padding =
                                " ".repeat(box_width.saturating_sub(line.chars().count() + 4));
                            format!("│ {}{} │", line, padding)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    let box_content = format!("{}\n{}\n{}", header, content_lines, footer);

                    // Replace by reconstructing the string with before + replacement + after
                    let before = &result[..start_pos];
                    let after = &result[full_end_pos..];
                    result = format!("{}{}{}", before, box_content, after);
                } else {
                    // If empty content, just remove the tags by reconstructing without them
                    let before = &result[..start_pos];
                    let after = &result[full_end_pos..];
                    result = format!("{}{}", before, after);
                }
            } else {
                break; // No matching end tag found
            }
        }
    }

    result
}

fn print_assistant_message(content: &str, verbose: bool, is_final: bool) {
    let formatted_content = format_xml_tags_as_boxes(content);

    if is_final {
        println!(
            "┌─ Final Agent Response ──────────────────────────────────────────────────────────"
        );
        for line in formatted_content.lines() {
            println!("│ {}", line);
        }
        println!(
            "└─────────────────────────────────────────────────────────────────────────────────"
        );
    } else {
        // Always show agent response, but truncate in regular mode
        println!("Agent Response:");
        if verbose {
            // Show full response
            for line in formatted_content.lines() {
                println!("  {}", line);
            }
        } else {
            // Show truncated response - first 3 lines max
            let lines: Vec<&str> = formatted_content.lines().collect();
            let display_lines = if lines.len() > 3 { 3 } else { lines.len() };

            for line in lines.iter().take(display_lines) {
                let truncated_line = if line.chars().count() > 80 {
                    let truncated: String = line.chars().take(80).collect();
                    format!("{}...", truncated)
                } else {
                    line.to_string()
                };
                println!("  {}", truncated_line);
            }

            if lines.len() > 3 {
                println!("  ... ({} more lines)", lines.len() - 3);
            }
        }
    }
}

pub async fn run_async(ctx: AppConfig, config: RunAsyncConfig) -> Result<(), String> {
    let start_time = Instant::now();
    let mut llm_response_time = std::time::Duration::new(0, 0);
    let mut chat_messages: Vec<ChatMessage> = Vec::new();

    print_header("Stakpak Agent - Async Mode");

    // Initialize MCP server
    print_info("Initializing MCP server and client connections...");
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
                tool_mode: ToolMode::Combined,
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
        print_info(&format!("Resuming from checkpoint ({})", checkpoint_id));
    }

    // Add user prompt if provided
    if !config.prompt.is_empty() {
        let (user_input, _local_context) =
            add_local_context(&chat_messages, &config.prompt, &config.local_context);
        let (user_input, _rulebooks_text) =
            add_rulebooks(&chat_messages, &user_input, &config.rulebooks);
        chat_messages.push(user_message(user_input));
    }

    let mut step = 0;
    let max_steps = config.max_steps.unwrap_or(50); // Safety limit to prevent infinite loops

    print_info("Starting execution...");
    println!();

    loop {
        step += 1;
        if step > max_steps {
            print_warning(&format!(
                "Reached maximum steps limit ({}), stopping execution",
                max_steps
            ));
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

        print_step_header(step, tool_count);

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
                print_assistant_message(&content_str, config.verbose, false);
            }
        }

        // Check if there are tool calls to execute
        if let Some(tool_calls) = tool_calls {
            if tool_calls.is_empty() {
                print_success("No more tools to execute - agent completed successfully");
                break;
            }

            // Execute all tool calls
            for (i, tool_call) in tool_calls.iter().enumerate() {
                // Print tool start with arguments
                print_tool_start(
                    &tool_call.function.name,
                    &tool_call.function.arguments,
                    i,
                    tool_calls.len(),
                );

                // Add timeout for tool execution
                let tool_execution = async { run_tool_call(&clients, &tools_map, tool_call).await };

                let result = match tokio::time::timeout(
                    std::time::Duration::from_secs(60), // 60 second timeout
                    tool_execution,
                )
                .await
                {
                    Ok(result) => result?,
                    Err(_) => {
                        print_error(&format!(
                            "Tool '{}' timed out after 60 seconds",
                            tool_call.function.name
                        ));
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
                    print_tool_result(&result_content, config.verbose);

                    chat_messages.push(tool_result(tool_call.id.clone(), result_content.clone()));
                } else {
                    print_warning(&format!(
                        "Tool '{}' returned no result",
                        tool_call.function.name
                    ));
                }
            }
        } else {
            print_success("No more tools to execute - agent completed successfully");
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

    println!();
    print_header("Execution Summary");
    print_success(&format!(
        "Completed after {} steps in {:.2}s",
        step - 1,
        elapsed.as_secs_f64()
    ));
    print_info(&format!(
        "Tool execution time: {:.2}s",
        tool_execution_time.as_secs_f64()
    ));
    print_info(&format!(
        "API call time: {:.2}s",
        llm_response_time.as_secs_f64()
    ));
    print_info(&format!(
        "Total messages in conversation: {}",
        chat_messages.len()
    ));

    // Show final assistant message
    if let Some(final_message) = chat_messages
        .iter()
        .rev()
        .find(|m| m.role == stakpak_shared::models::integrations::openai::Role::Assistant)
    {
        if let Some(content) = &final_message.content {
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
                println!();
                print_assistant_message(&content_str, true, true);
            }
        }
    }

    // Save conversation to file
    let conversation_json = serde_json::to_string_pretty(&chat_messages).unwrap_or_default();
    match LocalStore::write_session_data("messages.json", &conversation_json) {
        Ok(path) => {
            print_success(&format!(
                "Saved {} history messages to {}",
                chat_messages.len(),
                path
            ));
        }
        Err(e) => {
            print_error(&format!("Failed to save messages: {}", e));
        }
    }

    // Save checkpoint to file if available
    if let Some(checkpoint_id) = &latest_checkpoint {
        match LocalStore::write_session_data("checkpoint", checkpoint_id.to_string().as_str()) {
            Ok(path) => {
                print_success(&format!("Checkpoint {} saved to {}", checkpoint_id, path));
            }
            Err(e) => {
                print_error(&format!("Failed to save checkpoint: {}", e));
            }
        }
    } else {
        print_info("No checkpoint available to save");
    }

    Ok(())
}
