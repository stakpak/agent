use stakpak_shared::models::integrations::openai::ToolCall;
use stakpak_tui::InputEvent;

pub async fn send_input_event(
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
    event: InputEvent,
) -> Result<(), String> {
    input_tx.send(event).await.map_err(|e| e.to_string())
}

pub async fn send_tool_call(
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
    tool_call: &ToolCall,
) -> Result<(), String> {
    send_input_event(input_tx, InputEvent::RunToolCall(tool_call.clone())).await?;
    Ok(())
}
