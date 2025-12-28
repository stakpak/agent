//! Shell Mode Event Handlers
//!
//! Handles all shell mode-related events including shell output, errors, completion, and shell mode toggling.

use super::navigation::adjust_scroll;
use crate::app::{AppState, OutputEvent, ToolCallStatus};
use crate::services::bash_block::preprocess_terminal_output;
use crate::services::helper_block::push_error_message;
use crate::services::message::Message;
use crate::services::shell_mode::SHELL_PROMPT_PREFIX;
use stakpak_shared::helper::truncate_output;
use stakpak_shared::models::integrations::openai::{
    FunctionCall, ToolCall, ToolCallResult, ToolCallResultStatus,
};
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

/// Extract command from tool call
pub fn extract_command_from_tool_call(tool_call: &ToolCall) -> Result<String, String> {
    // Parse as JSON and extract the command field
    let json = serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments)
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;

    if let Some(command_value) = json.get("command") {
        if let Some(command_str) = command_value.as_str() {
            return Ok(command_str.to_string());
        } else {
            return Ok(command_value.to_string());
        }
    }

    Err("No 'command' field found in JSON arguments".to_string())
}

/// Handle shell mode toggle
pub fn handle_shell_mode(state: &mut AppState) {
    state.show_shell_mode = !state.show_shell_mode;
    // Update textarea shell mode
    state.text_area.set_shell_mode(state.show_shell_mode);

    if state.show_shell_mode {
        state.is_dialog_open = false;
        if let Some(dialog_command) = &state.dialog_command {
            let command = match extract_command_from_tool_call(dialog_command) {
                Ok(command) => command,
                Err(_) => {
                    return;
                }
            };
            state.text_area.set_text(&command);
        }
        state.ondemand_shell_mode = state.dialog_command.is_none();
        if state.ondemand_shell_mode {
            if state.shell_tool_calls.is_none() {
                state.shell_tool_calls = Some(Vec::new());
            }
            state.text_area.set_text("");
        }
    } else {
        state.text_area.set_text("");
    }
    if !state.show_shell_mode && state.dialog_command.is_some() {
        // only show dialog if id of latest tool call is not the same as dialog_command id
        if let Some(latest_tool_call) = &state.latest_tool_call
            && let Some(dialog_command) = &state.dialog_command
            && latest_tool_call.id != dialog_command.id
        {
            state.is_dialog_open = true;
        }
        state.ondemand_shell_mode = false;
    }
}

/// Handle shell output event
pub fn handle_shell_output(state: &mut AppState, line: String) {
    let mut line = line.replace("\r\n", "\n").replace('\r', "\n");

    if let Some(output) = state.active_shell_command_output.as_mut() {
        let text = format!("{}\n", line);
        output.push_str(&text);
        *output = truncate_output(output);
    }

    line = truncate_output(&line);
    state
        .messages
        .push(Message::render_escaped_text_block(line));
}

/// Handle shell error event
pub fn handle_shell_error(state: &mut AppState, line: String) {
    let line = preprocess_terminal_output(&line);
    let line = line.replace("\r\n", "\n").replace('\r', "\n");
    push_error_message(state, &line, None);
}

/// Handle shell waiting for input event
pub fn handle_shell_waiting_for_input(
    state: &mut AppState,
    message_area_height: usize,
    message_area_width: usize,
) {
    state.waiting_for_shell_input = true;
    // Set textarea to shell mode when waiting for input
    state.text_area.set_shell_mode(true);
    // Allow user input when command is waiting
    adjust_scroll(state, message_area_height, message_area_width);
}

/// Handle shell completed event
pub fn handle_shell_completed(
    state: &mut AppState,
    output_tx: &Sender<OutputEvent>,
    message_area_height: usize,
    message_area_width: usize,
) {
    // Command completed, reset active command state
    state.waiting_for_shell_input = false;
    if let Some(dialog_command) = &state.dialog_command {
        let dialog_command_id = dialog_command.id.clone();
        let result = shell_command_to_tool_call_result(state);

        // check the index of dialog_command in tool_calls_execution_order
        let index = state
            .last_message_tool_calls
            .iter()
            .position(|tool_call| tool_call.id == dialog_command_id);

        let should_stop = if let Some(index) = index {
            index != state.last_message_tool_calls.len() - 1
        } else {
            false
        };

        // get the ids of the tool calls after that id
        let tool_calls_after_index = if let Some(index) = index {
            state
                .last_message_tool_calls
                .iter()
                .skip(index + 1)
                .cloned()
                .collect::<Vec<ToolCall>>()
        } else {
            Vec::new()
        };

        // move those rejected tool calls to message_tool_calls and remove them from session_tool_calls_queue and rejected_tool_calls and tool_call_execution_order
        if !tool_calls_after_index.is_empty() {
            for tool_call in tool_calls_after_index.iter() {
                state
                    .session_tool_calls_queue
                    .insert(tool_call.id.clone(), ToolCallStatus::Pending);
            }
        }

        let _ = output_tx.try_send(OutputEvent::SendToolResult(
            result,
            should_stop,
            tool_calls_after_index.clone(),
        ));

        if let Some(dialog_command) = &state.dialog_command
            && let Some(latest_tool_call) = &state.latest_tool_call
            && dialog_command.id == latest_tool_call.id
        {
            state.latest_tool_call = None;
        }
        state.show_shell_mode = false;
        state.dialog_command = None;
        state.toggle_approved_message = true;
        state.text_area.set_shell_mode(false);
    }
    if state.ondemand_shell_mode {
        let new_tool_call_result = shell_command_to_tool_call_result(state);
        if let Some(ref mut tool_calls) = state.shell_tool_calls {
            tool_calls.push(new_tool_call_result);
        }
    }

    state.active_shell_command = None;
    state.active_shell_command_output = None;
    state.text_area.set_text("");
    state.messages.push(Message::plain_text(""));
    state.is_tool_call_shell_command = false;
    adjust_scroll(state, message_area_height, message_area_width);
}

/// Handle shell clear event
pub fn handle_shell_clear(
    state: &mut AppState,
    message_area_height: usize,
    message_area_width: usize,
) {
    // Clear the shell output buffer
    if let Some(output) = state.active_shell_command_output.as_mut() {
        output.clear();
    }

    // Find the last non-shell message to determine where current shell session started
    let mut last_non_shell_index = None;
    for (i, message) in state.messages.iter().enumerate().rev() {
        let is_shell_message = match &message.content {
            crate::services::message::MessageContent::Styled(line) => line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .starts_with(SHELL_PROMPT_PREFIX),
            crate::services::message::MessageContent::Plain(text, _) => {
                text.starts_with(SHELL_PROMPT_PREFIX)
            }
            crate::services::message::MessageContent::PlainText(_) => true,
            _ => false,
        };

        if !is_shell_message {
            last_non_shell_index = Some(i);
            break;
        }
    }

    // If we found a non-shell message, clear everything after it (the current shell session)
    if let Some(index) = last_non_shell_index {
        // Keep messages up to and including the last non-shell message
        state.messages.truncate(index + 1);
    } else {
        // If no non-shell messages found, clear all messages (entire session is shell)
        state.messages.clear();
    }

    // Scroll to the bottom to show the cleared state
    adjust_scroll(state, message_area_height, message_area_width);
}

/// Handle shell kill event
pub fn handle_shell_kill(state: &mut AppState) {
    // Kill the running command if there is one
    if let Some(cmd) = &state.active_shell_command
        && let Err(_e) = cmd.kill()
    {}
    // Reset shell state
    state.active_shell_command = None;
    state.active_shell_command_output = None;
    state.waiting_for_shell_input = false;
    // Reset textarea shell mode
    state.text_area.set_shell_mode(false);
}

/// Convert shell command to tool call result
pub fn shell_command_to_tool_call_result(state: &mut AppState) -> ToolCallResult {
    let (id, name) = if let Some(cmd) = &state.dialog_command {
        (cmd.id.clone(), cmd.function.name.clone())
    } else {
        (
            format!("tool_{}", Uuid::new_v4()),
            "run_command".to_string(),
        )
    };

    let command = state
        .active_shell_command
        .as_ref()
        .map(|cmd| cmd.command.clone())
        .unwrap_or_default();

    let args = format!("{{\"command\": \"{}\"}}", command);

    let call = ToolCall {
        id,
        r#type: "function".to_string(),
        function: FunctionCall {
            name,
            arguments: args,
        },
    };
    ToolCallResult {
        call,
        result: state
            .active_shell_command_output
            .as_ref()
            .cloned()
            .unwrap_or_default(),
        status: ToolCallResultStatus::Success,
    }
}
