//! Tool Call Event Handlers
//!
//! Handles all tool call-related events including streaming tool results, retry logic, and approval popup events.

use crate::app::{AppState, InputEvent, OutputEvent, ToolCallStatus};
use crate::services::commands::{CommandAction, CommandContext, execute_command, filter_commands};
use crate::services::helper_block::push_error_message;
use crate::services::message::{Message, invalidate_message_lines_cache};
use stakpak_shared::models::integrations::openai::{
    ToolCall, ToolCallResult, ToolCallResultProgress,
};
use tokio::sync::mpsc::Sender;

use super::shell::extract_command_from_tool_call;

/// Handle stream tool result event
pub fn handle_stream_tool_result(state: &mut AppState, progress: ToolCallResultProgress) {
    let tool_call_id = progress.id;
    // Check if this tool call is already completed - if so, ignore streaming updates
    if state.completed_tool_calls.contains(&tool_call_id) {
        return;
    }

    // Ensure loading state is true during streaming tool results
    // Only set it if it's not already true to avoid unnecessary state changes
    if !state.loading {
        state.loading = true;
    }
    state.is_streaming = true;
    state.streaming_tool_result_id = Some(tool_call_id);
    // 1. Update the buffer for this tool_call_id
    state
        .streaming_tool_results
        .entry(tool_call_id)
        .or_default()
        .push_str(&format!("{}\n", progress.message));

    // 2. Remove the old message with this id (if any)
    state.messages.retain(|m| m.id != tool_call_id);

    // 3. Get the buffer content for rendering (clone to String)
    let buffer_content = state
        .streaming_tool_results
        .get(&tool_call_id)
        .cloned()
        .unwrap_or_default();

    state.messages.push(Message::render_streaming_border_block(
        &buffer_content,
        "Tool Streaming",
        "Result",
        None,
        "Streaming",
        Some(tool_call_id),
    ));
    invalidate_message_lines_cache(state);

    // If content changed while user is scrolled up, mark it
    if !state.stay_at_bottom {
        state.content_changed_while_scrolled_up = true;
    }
}

/// Handle message tool calls event
pub fn handle_message_tool_calls(state: &mut AppState, tool_calls: Vec<ToolCall>) {
    // exclude any tool call that is already executed
    let rest_tool_calls = tool_calls
        .into_iter()
        .filter(|tool_call| {
            !state.session_tool_calls_queue.contains_key(&tool_call.id)
                || state
                    .session_tool_calls_queue
                    .get(&tool_call.id)
                    .map(|status| status != &ToolCallStatus::Executed)
                    .unwrap_or(false)
        })
        .collect::<Vec<ToolCall>>();

    let prompt_tool_calls = state
        .auto_approve_manager
        .get_prompt_tool_calls(&rest_tool_calls);

    state.message_tool_calls = Some(prompt_tool_calls.clone());

    // Only update last_message_tool_calls if we're not in a retry scenario
    // During retry, we want to preserve the original sequence for ShellCompleted
    if !state.show_shell_mode || state.dialog_command.is_none() {
        state.last_message_tool_calls = prompt_tool_calls.clone();
    }
}

/// Handle retry tool call event
pub fn handle_retry_tool_call(
    state: &mut AppState,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
) {
    if state.latest_tool_call.is_none() {
        return;
    }
    let _ = input_tx.try_send(InputEvent::EmergencyClearTerminal);

    if let Some(cancel_tx) = cancel_tx {
        let _ = cancel_tx.send(());
    }

    if let Some(tool_call) = &state.latest_tool_call {
        // Extract the command from the tool call
        let command = match extract_command_from_tool_call(tool_call) {
            Ok(command) => command,
            Err(_) => {
                return;
            }
        };
        // Enable shell mode
        state.show_shell_mode = true;
        state.is_dialog_open = false;
        state.ondemand_shell_mode = false;
        state.dialog_command = Some(tool_call.clone());
        if state.shell_tool_calls.is_none() {
            state.shell_tool_calls = Some(Vec::new());
        }

        // Set the command in the input but don't execute it yet
        state.text_area.set_text(&command);
        state.text_area.set_cursor(command.len());

        // Clear any existing shell state
        state.active_shell_command = None;
        state.active_shell_command_output = None;
        state.waiting_for_shell_input = false;
        // Set textarea shell mode to match app state
        state.text_area.set_shell_mode(true);
    }
}

/// Handle retry mechanism
pub fn handle_retry_mechanism(state: &mut AppState) {
    if state.messages.len() >= 2 {
        state.messages.pop();
    }
}

/// Handle toggle approval status event
pub fn handle_toggle_approval_status(state: &mut AppState) {
    state.approval_popup.toggle_approval_status();
}

/// Handle approval popup next tab event
pub fn handle_approval_popup_next_tab(state: &mut AppState) {
    state.approval_popup.next_tab();
}

/// Handle approval popup prev tab event
pub fn handle_approval_popup_prev_tab(state: &mut AppState) {
    state.approval_popup.prev_tab();
}

/// Handle approval popup toggle approval event
pub fn handle_approval_popup_toggle_approval(state: &mut AppState) {
    state.approval_popup.toggle_approval_status();
}

/// Handle approval popup escape event
pub fn handle_approval_popup_escape(state: &mut AppState) {
    state.approval_popup.escape();
}

/// Clear streaming tool results
pub fn clear_streaming_tool_results(state: &mut AppState) {
    state.is_streaming = false;

    // Mark the current streaming tool call as completed
    if let Some(tool_call_id) = state.streaming_tool_result_id {
        state.completed_tool_calls.insert(tool_call_id);
    }

    // Clear the streaming data and remove the streaming message and pending bash message id
    state.streaming_tool_results.clear();
    state.messages.retain(|m| {
        m.id != state.streaming_tool_result_id.unwrap_or_default()
            && m.id != state.pending_bash_message_id.unwrap_or_default()
    });
    state.latest_tool_call = None;
    state.pending_bash_message_id = None;
}

/// Update session tool calls queue
pub fn update_session_tool_calls_queue(state: &mut AppState, tool_call_result: &ToolCallResult) {
    if tool_call_result.status
        == stakpak_shared::models::integrations::openai::ToolCallResultStatus::Error
        && let Some(failed_idx) = state
            .tool_call_execution_order
            .iter()
            .position(|id| id == &tool_call_result.call.id)
    {
        for id in state.tool_call_execution_order.iter().skip(failed_idx + 1) {
            state
                .session_tool_calls_queue
                .insert(id.clone(), ToolCallStatus::Skipped);
        }
    }
}

/// Execute command palette selection
pub fn execute_command_palette_selection(
    state: &mut AppState,
    input_tx: &Sender<InputEvent>,
    output_tx: &Sender<OutputEvent>,
) {
    let filtered_commands = filter_commands(&state.command_palette_search);
    if filtered_commands.is_empty() || state.command_palette_selected >= filtered_commands.len() {
        return;
    }

    let selected_command = &filtered_commands[state.command_palette_selected];

    // Close command palette
    state.show_command_palette = false;
    state.command_palette_search.clear();

    // Execute the command - use unified executor for slash commands
    if let Some(command_id) = selected_command.action.to_command_id() {
        let ctx = CommandContext {
            state,
            input_tx,
            output_tx,
        };
        if let Err(e) = execute_command(command_id, ctx) {
            push_error_message(state, &e, None);
        }
    } else {
        // Handle non-slash commands (keyboard shortcuts)
        match selected_command.action {
            CommandAction::OpenProfileSwitcher => {
                let _ = input_tx.try_send(InputEvent::ShowProfileSwitcher);
            }
            CommandAction::OpenRulebookSwitcher => {
                let _ = input_tx.try_send(InputEvent::ShowRulebookSwitcher);
            }
            CommandAction::OpenShortcuts => {
                let _ = input_tx.try_send(InputEvent::ShowShortcuts);
            }
            _ => {
                // Should not happen - all slash commands should be handled above
            }
        }
        state.text_area.set_text("");
        state.show_helper_dropdown = false;
    }
}
