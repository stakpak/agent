//! Dialog Event Handlers
//!
//! Handles all dialog-related events including confirmation dialogs, ESC handling, and dialog navigation.

use crate::app::{AppState, InputEvent, OutputEvent, ToolCallStatus};
use crate::services::bash_block::render_bash_block_rejected;
use crate::services::helper_block::push_styled_message;
use crate::services::message::extract_truncated_command_arguments;
use crate::services::message::{Message, MessageContent, get_command_type_name};
use ratatui::layout::Size;
use ratatui::style::Color;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

use super::EventChannels;

/// Handle ESC event (routes to appropriate handler)
pub fn handle_esc_event(
    state: &mut AppState,
    input_tx: &Sender<InputEvent>,
    output_tx: &Sender<OutputEvent>,
    shell_tx: &Sender<InputEvent>,
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
) {
    if state.show_recovery_options_popup {
        state.show_recovery_options_popup = false;
        return;
    }
    if state.show_context_popup {
        state.show_context_popup = false;
        return;
    }
    if state.show_rulebook_switcher {
        state.show_rulebook_switcher = false;
        return;
    }
    if state.show_command_palette {
        state.show_command_palette = false;
        state.command_palette_search.clear();
        return;
    }
    if state.show_profile_switcher {
        state.show_profile_switcher = false;
        return;
    }
    if state.show_shortcuts_popup {
        state.show_shortcuts_popup = false;
        return;
    }
    if state.show_collapsed_messages {
        state.show_collapsed_messages = false;
        return;
    }
    if state.approval_popup.is_visible() {
        state.approval_popup.escape();
        state.toggle_approved_message = false;
    } else {
        state.message_rejected_tools = state
            .approval_popup
            .get_approved_tool_calls()
            .into_iter()
            .cloned()
            .collect();
        state.message_approved_tools.clear();
        state.message_tool_calls = None;
        state.tool_call_execution_order.clear();
        // Store the latest tool call for potential retry (only for run_command)
        if let Some(tool_call) = &state.dialog_command
            && tool_call.function.name == "run_command"
        {
            state.latest_tool_call = Some(tool_call.clone());
        }

        let channels = EventChannels {
            output_tx,
            input_tx,
            shell_tx,
        };
        handle_esc(state, &channels, cancel_tx, None, true, None);
    }
}

/// Handle ESC key press
pub fn handle_esc(
    state: &mut AppState,
    channels: &EventChannels,
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
    message: Option<String>,
    should_stop: bool,
    color: Option<Color>,
) {
    let _ = channels
        .input_tx
        .try_send(InputEvent::EmergencyClearTerminal);

    if let Some(cancel_tx) = cancel_tx {
        let _ = cancel_tx.send(());
    }

    state.is_streaming = false;
    if state.show_sessions_dialog {
        state.show_sessions_dialog = false;
    } else if state.show_collapsed_messages {
        state.show_collapsed_messages = false;
    } else if state.show_helper_dropdown {
        state.show_helper_dropdown = false;
    } else if state.is_dialog_open {
        let tool_call_opt = state.dialog_command.clone();
        if let Some(tool_call) = &tool_call_opt {
            let _ = channels
                .output_tx
                .try_send(OutputEvent::RejectTool(tool_call.clone(), should_stop));
            let truncated_command = extract_truncated_command_arguments(tool_call, None);
            let title = get_command_type_name(tool_call);
            let rendered_lines =
                render_bash_block_rejected(&truncated_command, &title, message.clone(), color);
            state.messages.push(Message {
                id: Uuid::new_v4(),
                content: MessageContent::StyledBlock(rendered_lines),
                is_collapsed: None,
            });
        }
        state.is_dialog_open = false;
        state.dialog_command = None;
        state.dialog_focused = false; // Reset focus when dialog closes
        state.text_area.set_text("");
    } else if state.show_shell_mode {
        if state.active_shell_command.is_some() {
            let _ = channels.shell_tx.try_send(InputEvent::ShellKill);
        }
        state.show_shell_mode = false;
        state.text_area.set_shell_mode(false);
        state.text_area.set_text("");
        if state.dialog_command.is_some() {
            state.dialog_command = None;
        }
    } else {
        state.text_area.set_text("");
    }

    state.messages.retain(|m| {
        m.id != state.streaming_tool_result_id.unwrap_or_default()
            && m.id != state.pending_bash_message_id.unwrap_or_default()
    });
}

/// Handle show confirmation dialog event
pub fn handle_show_confirmation_dialog(
    state: &mut AppState,
    tool_call: stakpak_shared::models::integrations::openai::ToolCall,
    input_tx: &Sender<InputEvent>,
    output_tx: &Sender<OutputEvent>,
    terminal_size: Size,
) {
    if state.latest_tool_call.is_some() && state.show_shell_mode {
        return;
    }
    if state
        .session_tool_calls_queue
        .get(&tool_call.id)
        .map(|status| status == &ToolCallStatus::Executed)
        .unwrap_or(false)
    {
        let truncated_command = extract_truncated_command_arguments(&tool_call, None);
        let title = get_command_type_name(&tool_call);
        let rendered_lines = render_bash_block_rejected(
            &truncated_command,
            &title,
            Some("Tool call already executed".to_string()),
            None,
        );
        state.messages.push(Message {
            id: Uuid::new_v4(),
            content: MessageContent::StyledBlock(rendered_lines),
            is_collapsed: None,
        });
        state.is_dialog_open = false;
        state.dialog_command = None;
        return;
    }

    state.dialog_command = Some(tool_call.clone());
    if tool_call.function.name == "run_command" {
        state.latest_tool_call = Some(tool_call.clone());
    }
    let is_auto_approved = state.auto_approve_manager.should_auto_approve(&tool_call);

    if tool_call.function.name == "str_replace" || tool_call.function.name == "create" {
        state
            .messages
            .push(Message::render_collapsed_message(tool_call.clone()));
    }

    // Tool call is pending - create pending border block and check if we should show popup
    let message_id = Uuid::new_v4();
    state.messages.push(Message::render_pending_border_block(
        tool_call.clone(),
        is_auto_approved,
        Some(message_id),
    ));
    state.pending_bash_message_id = Some(message_id);

    state.dialog_command = Some(tool_call.clone());
    state.is_dialog_open = true;
    state.loading = false;
    state.dialog_focused = false;

    // check if its skipped
    let is_skipped =
        state.session_tool_calls_queue.get(&tool_call.id) == Some(&ToolCallStatus::Skipped);

    // Check if this tool call is already rejected (after popup interaction) or skipped
    if state
        .message_rejected_tools
        .iter()
        .any(|tool| tool.id == tool_call.id)
        || is_skipped
    {
        if !is_skipped {
            // Remove from rejected list to avoid processing it again
            state
                .message_rejected_tools
                .retain(|tool| tool.id != tool_call.id);
        }

        let input_tx_clone = input_tx.clone();
        let message = if is_skipped {
            "Tool call skipped due to sequential execution failure"
        } else {
            "Tool call rejected"
        };

        let color = if is_skipped {
            Some(Color::Yellow)
        } else {
            None
        };

        let _ = input_tx_clone.try_send(InputEvent::HandleReject(
            Some(message.to_string()),
            !is_skipped,
            color,
        ));

        state
            .session_tool_calls_queue
            .insert(tool_call.id.clone(), ToolCallStatus::Executed);
        return;
    }

    // Check if this tool call is already approved (after popup interaction or auto-approved)
    if is_auto_approved
        || state
            .message_approved_tools
            .iter()
            .any(|tool| tool.id == tool_call.id)
    {
        // Remove from approved list to avoid processing it again
        state
            .message_approved_tools
            .retain(|tool| tool.id != tool_call.id);

        // Send tool call with delay
        let tool_call_clone = tool_call.clone();
        let output_tx_clone = output_tx.clone();

        let _ = output_tx_clone.try_send(OutputEvent::AcceptTool(tool_call_clone));
        state
            .session_tool_calls_queue
            .insert(tool_call.id.clone(), ToolCallStatus::Executed);
        state.is_dialog_open = false;
        state.dialog_selected = 0;
        state.dialog_command = None;
        state.dialog_focused = false;
        return;
    }

    let tool_calls = if let Some(tool_calls) = state.message_tool_calls.clone() {
        tool_calls.clone()
    } else {
        vec![tool_call.clone()]
    };

    // Tool call is pending - check if we should show popup first
    use crate::services::approval_popup::PopupService;
    if !tool_calls.is_empty() && state.toggle_approved_message {
        state.approval_popup = PopupService::new_with_tool_calls(tool_calls.clone(), terminal_size);
    }
}

/// Handle toggle dialog focus event
pub fn handle_toggle_dialog_focus(state: &mut AppState) {
    if state.is_dialog_open {
        state.dialog_focused = !state.dialog_focused;
        let focus_message = if state.dialog_focused {
            "Dialog focused"
        } else {
            "Chat view focused"
        };
        push_styled_message(
            state,
            &format!("🎯 {}", focus_message),
            Color::DarkGray,
            "",
            Color::Cyan,
        );
    }
}
