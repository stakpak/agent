use rmcp::model::{CallToolRequestParam, CallToolResult};
use stakpak_mcp_client::ClientManager;
use stakpak_shared::models::integrations::openai::{
    ChatMessage, FunctionDefinition, MessageContent, Role, Tool,
};
use stakpak_tui::{InputEvent, OutputEvent};
use uuid::Uuid;

use crate::{client::Client, config::AppConfig};

use super::truncate_output;

// Helper to convert tools_map to Vec<Tool>
fn convert_tools_map(
    tools_map: &std::collections::HashMap<String, Vec<rmcp::model::Tool>>,
) -> Vec<Tool> {
    tools_map
        .iter()
        .flat_map(|(_name, tools)| {
            tools.iter().map(|tool| Tool {
                r#type: "function".to_string(),
                function: FunctionDefinition {
                    name: tool.name.clone().into_owned(),
                    description: tool.description.clone().map(|d| d.to_string()),
                    parameters: serde_json::Value::Object((*tool.input_schema).clone()),
                },
            })
        })
        .collect()
}

// Helper to create a ChatMessage from user input
fn user_message(user_input: String) -> ChatMessage {
    ChatMessage {
        role: Role::User,
        content: Some(MessageContent::String(user_input)),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    }
}

fn tool_result(tool_call_id: String, result: String) -> ChatMessage {
    ChatMessage {
        role: Role::Tool,
        content: Some(MessageContent::String(result)),
        name: None,
        tool_calls: None,
        tool_call_id: Some(tool_call_id),
    }
}

// Helper to send a message to the TUI
async fn send_input_event(
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
    event: InputEvent,
) -> Result<(), String> {
    input_tx.send(event).await.map_err(|e| e.to_string())
}

// Helper to send tool call messages to the TUI
async fn send_tool_calls(
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
    tool_calls: &[stakpak_shared::models::integrations::openai::ToolCall],
) -> Result<(), String> {
    for tool_call in tool_calls {
        if tool_call.function.name == "run_command" {
            send_input_event(input_tx, InputEvent::RunCommand(tool_call.clone())).await?;
        }
    }
    Ok(())
}

async fn run_tool_call(
    client_manager: &ClientManager,
    tools_map: &std::collections::HashMap<String, Vec<rmcp::model::Tool>>,
    tool_call: &stakpak_shared::models::integrations::openai::ToolCall,
) -> Result<Option<CallToolResult>, String> {
    let tool_name = &tool_call.function.name;
    let client_name = tools_map
        .iter()
        .find(|(_, tools)| tools.iter().any(|tool| tool.name == *tool_name))
        .map(|(name, _)| name.clone());

    if let Some(client_name) = client_name {
        let client = client_manager
            .get_client(&client_name)
            .await
            .map_err(|e| e.to_string())?;
        let result = client
            .call_tool(CallToolRequestParam {
                name: tool_name.clone().into(),
                arguments: Some(
                    serde_json::from_str(&tool_call.function.arguments)
                        .map_err(|e| e.to_string())?,
                ),
            })
            .await
            .map_err(|e| e.to_string())?;

        return Ok(Some(result));
    }

    Ok(None)
}

pub async fn get_checkpoint_messages(
    client: &Client,
    checkpoint_id: &String,
) -> Result<Vec<ChatMessage>, String> {
    let checkpoint_uuid = Uuid::parse_str(checkpoint_id).map_err(|_| {
        format!(
            "Invalid checkpoint ID '{}' - must be a valid UUID",
            checkpoint_id
        )
    })?;

    let checkpoint = client
        .get_agent_checkpoint(checkpoint_uuid)
        .await
        .map_err(|e| e.to_string())?;
    let checkpoint_output: crate::client::models::AgentOutput = checkpoint.output;

    if let crate::client::models::AgentOutput::PabloV1 { messages, .. } = checkpoint_output {
        return Ok(messages.clone());
    }

    Ok(vec![])
}

pub async fn run(config: AppConfig) -> Result<(), String> {
    let mut messages: Vec<ChatMessage> = Vec::new();
    let (input_tx, input_rx) = tokio::sync::mpsc::channel::<InputEvent>(100);
    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel::<OutputEvent>(100);

    // Initialize clients and tools
    let clients = ClientManager::new().await.map_err(|e| e.to_string())?;
    let tools_map = clients.get_tools().await.map_err(|e| e.to_string())?;
    let tools = convert_tools_map(&tools_map);

    // Spawn TUI task
    let tui_handle = tokio::spawn(async move {
        let _ = stakpak_tui::run_tui(input_rx, output_tx)
            .await
            .map_err(|e| e.to_string());
    });

    // Spawn client task
    let client_handle: tokio::task::JoinHandle<Result<(), String>> = tokio::spawn(async move {
        let client = Client::new(&config).map_err(|e| e.to_string())?;
        while let Some(output_event) = output_rx.recv().await {
            match output_event {
                OutputEvent::UserMessage(user_input) => {
                    messages.push(user_message(user_input));
                }
                OutputEvent::AcceptTool(tool_call) => {
                    send_input_event(&input_tx, InputEvent::Loading(true)).await?;
                    let result = run_tool_call(&clients, &tools_map, &tool_call).await?;
                    send_input_event(&input_tx, InputEvent::Loading(false)).await?;
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

                        messages.push(tool_result(tool_call.id, result_content.clone()));

                        send_input_event(
                            &input_tx,
                            InputEvent::ToolResult(truncate_output(&result_content)),
                        )
                        .await?;
                    }
                }
            }
            send_input_event(&input_tx, InputEvent::Loading(true)).await?;
            let response = match client
                .chat_completion(messages.clone(), Some(tools.clone()))
                .await
            {
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

            // Send main response content to TUI
            let content = response.choices[0]
                .message
                .content
                .clone()
                .unwrap_or(MessageContent::String("".to_string()))
                .to_string();

            send_input_event(&input_tx, InputEvent::InputSubmittedWith(content)).await?;

            // Send tool calls to TUI if present
            if let Some(tool_calls) = &response.choices[0].message.tool_calls {
                send_tool_calls(&input_tx, tool_calls).await?;
            }
        }
        Ok(())
    });

    // Wait for both tasks to finish
    let (_, client_res) = tokio::try_join!(tui_handle, client_handle).map_err(|e| e.to_string())?;
    client_res?;
    Ok(())
}

pub struct RunNonInteractiveConfig {
    pub prompt: String,
    pub approve: bool,
    pub verbose: bool,
    pub checkpoint_id: Option<String>,
}

pub async fn run_non_interactive(
    ctx: AppConfig,
    config: RunNonInteractiveConfig,
) -> Result<(), String> {
    let mut chat_messages: Vec<ChatMessage> = Vec::new();

    let clients = ClientManager::new().await.map_err(|e| e.to_string())?;
    let tools_map = clients.get_tools().await.map_err(|e| e.to_string())?;
    let tools = convert_tools_map(&tools_map);

    let client = Client::new(&ctx).map_err(|e| e.to_string())?;

    if let Some(checkpoint_id) = config.checkpoint_id {
        let checkpoint_messages = get_checkpoint_messages(&client, &checkpoint_id).await?;
        chat_messages.extend(checkpoint_messages);
    }

    if let Some(message) = chat_messages.last() {
        if config.approve && message.tool_calls.is_some() {
            // Clone the tool_calls to avoid borrowing message while mutating chat_messages
            let tool_calls = message.tool_calls.as_ref().unwrap().clone();
            for tool_call in tool_calls.iter() {
                let result = run_tool_call(&clients, &tools_map, tool_call).await?;
                if let Some(result) = result {
                    if !config.verbose {
                        println!("{}", serde_json::to_string_pretty(&result).unwrap());
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
        chat_messages.push(user_message(config.prompt));
    }

    let response = client
        .chat_completion(chat_messages.clone(), Some(tools))
        .await
        .map_err(|e| e.to_string())?;

    chat_messages.push(response.choices[0].message.clone());

    match config.verbose {
        true => {
            println!("{}", serde_json::to_string_pretty(&chat_messages).unwrap());
        }
        false => {
            println!(
                "{}",
                serde_json::to_string_pretty(&response.choices[0].message).unwrap()
            );
        }
    }

    Ok(())
}
