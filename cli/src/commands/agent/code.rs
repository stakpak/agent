use stakpak_mcp_client::ClientManager;
use stakpak_shared::models::integrations::openai::{
    ChatMessage, FunctionDefinition, MessageContent, Role, Tool,
};
use stakpak_tui::Msg;

use crate::{client::Client, config::AppConfig};

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

// Helper to send a message to the TUI
async fn send_input_msg(
    input_tx: &tokio::sync::mpsc::Sender<Msg>,
    content: String,
) -> Result<(), String> {
    input_tx
        .send(Msg::InputSubmittedWith(content))
        .await
        .map_err(|e| e.to_string())
}

// Helper to send tool call messages to the TUI
async fn send_tool_calls(
    input_tx: &tokio::sync::mpsc::Sender<Msg>,
    tool_calls: &[stakpak_shared::models::integrations::openai::ToolCall],
) -> Result<(), String> {
    let msg = tool_calls
        .iter()
        .map(|tool_call| {
            format!(
                "{}: {}",
                tool_call.function.name, tool_call.function.arguments
            )
        })
        .collect::<Vec<String>>()
        .join("\n");
    send_input_msg(input_tx, msg).await
}

pub async fn run(config: AppConfig) -> Result<(), String> {
    let mut messages: Vec<ChatMessage> = Vec::new();
    let (input_tx, input_rx) = tokio::sync::mpsc::channel::<Msg>(100);
    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel::<String>(100);

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
        while let Some(user_input) = output_rx.recv().await {
            messages.push(user_message(user_input));

            let response = match client
                .chat_completion(messages.clone(), Some(tools.clone()))
                .await
            {
                Ok(response) => response,
                Err(e) => {
                    input_tx.send(Msg::Quit).await.map_err(|e| e.to_string())?;
                    return Err(e.to_string());
                }
            };

            messages.push(response.choices[0].message.clone());

            // Send main response content to TUI
            let content = response.choices[0]
                .message
                .content
                .clone()
                .unwrap_or(MessageContent::String("".to_string()))
                .to_string();
            send_input_msg(&input_tx, content).await?;

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
