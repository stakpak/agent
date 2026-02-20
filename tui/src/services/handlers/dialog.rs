//! Dialog Event Handlers
//!
//! Handles all dialog-related events including confirmation dialogs, ESC handling, and dialog navigation.

use crate::app::{AppState, InputEvent, OutputEvent, ToolCallStatus};
use crate::services::bash_block::render_bash_block_rejected;
use crate::services::helper_block::push_styled_message;
use crate::services::message::extract_truncated_command_arguments;
use crate::services::message::{
    Message, MessageContent, get_command_type_name, invalidate_message_lines_cache,
};
use crate::services::text_selection::SelectionState;
use ratatui::layout::Size;
use ratatui::style::Color;
use stakpak_shared::models::integrations::openai::ToolCall;
use stakpak_shared::utils::strip_tool_name;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

use super::EventChannels;

/// Update a run_command block from Pending to Running state
/// This should be called when a run_command tool is approved and starts executing
pub fn update_run_command_to_running(state: &mut AppState, tool_call: &ToolCall) {
    let tool_name = strip_tool_name(&tool_call.function.name);
    if tool_name != "run_command" {
        return;
    }

    // Find the pending message by pending_bash_message_id and update it to Running
    if let Some(pending_id) = state.pending_bash_message_id {
        for msg in &mut state.messages {
            if msg.id == pending_id {
                if let MessageContent::RenderRunCommandBlock(command, _result, _run_state) =
                    &msg.content
                {
                    // Update to Running state - keep the same command, no result yet
                    let cmd = command.clone();
                    msg.content = MessageContent::RenderRunCommandBlock(
                        cmd,
                        None,
                        crate::services::bash_block::RunCommandState::Running,
                    );
                }
                break;
            }
        }
        invalidate_message_lines_cache(state);
    }
}

/// Update the pending tool display to show the first tool (which is being executed)
/// This ensures the UI shows the correct tool when execution starts, not the currently selected one
pub fn update_pending_tool_to_first(
    state: &mut AppState,
    first_tool: &ToolCall,
    is_approved: bool,
) {
    // Remove any existing pending tool block
    if let Some(pending_id) = state.pending_bash_message_id {
        state.messages.retain(|m| m.id != pending_id);
    }

    let tool_name = strip_tool_name(&first_tool.function.name);

    // Create the appropriate pending block based on tool type
    if tool_name == "run_command" {
        let command = super::shell::extract_command_from_tool_call(first_tool)
            .unwrap_or_else(|_| "unknown command".to_string());

        let run_state = if is_approved {
            crate::services::bash_block::RunCommandState::Running
        } else {
            crate::services::bash_block::RunCommandState::Rejected
        };

        let msg = Message::render_run_command_block(command, None, run_state, None);
        state.pending_bash_message_id = Some(msg.id);
        state.messages.push(msg);
    } else {
        // For other tools (str_replace, create, etc.), use the standard pending block
        let msg = Message::render_pending_border_block(first_tool.clone(), is_approved, None);
        state.pending_bash_message_id = Some(msg.id);
        state.messages.push(msg);
    }

    invalidate_message_lines_cache(state);
}

/// Handle ESC event (routes to appropriate handler)
pub fn handle_esc_event(
    state: &mut AppState,
    input_tx: &Sender<InputEvent>,
    output_tx: &Sender<OutputEvent>,
    _shell_tx: &Sender<InputEvent>,
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
) {
    // Always clear text selection on Escape (prevents stuck selections)
    if state.selection.active {
        state.selection = SelectionState::default();
    }

    if state.show_rulebook_switcher {
        state.show_rulebook_switcher = false;
        return;
    }
    if state.show_shortcuts_popup {
        state.show_shortcuts_popup = false;
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

    // Common handling for rejection
    state.message_tool_calls = None;
    state.tool_call_execution_order.clear();
    // Store the latest tool call for potential retry (only for run_command)
    if let Some(tool_call) = &state.dialog_command
        && strip_tool_name(&tool_call.function.name) == "run_command"
    {
        state.latest_tool_call = Some(tool_call.clone());
    }

    let channels = EventChannels {
        output_tx,
        input_tx,
    };
    // Provide default rejection message when user presses ESC
    handle_esc(
        state,
        &channels,
        cancel_tx,
        Some("Tool calls rejected".to_string()),
        true,
        None,
    );
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

    let was_streaming = state.is_streaming;
    let was_dialog_open = state.is_dialog_open;
    let was_shell_mode = state.show_shell_mode;
    state.is_streaming = false;
    if state.show_collapsed_messages {
        state.show_collapsed_messages = false;
    } else if state.show_helper_dropdown {
        state.show_helper_dropdown = false;
    } else if state.is_dialog_open {
        let tool_call_opt = state.dialog_command.clone();
        if let Some(tool_call) = &tool_call_opt {
            let _ = channels
                .output_tx
                .try_send(OutputEvent::RejectTool(tool_call.clone(), should_stop));

            let tool_name = strip_tool_name(&tool_call.function.name);
            if tool_name == "run_command" {
                // For run_command, remove the pending unified block and add rejected unified block
                // Remove pending message by tool_call_id
                if let Ok(tool_call_uuid) = Uuid::parse_str(&tool_call.id) {
                    state.messages.retain(|m| m.id != tool_call_uuid);
                }
                // Also remove by pending_bash_message_id
                if let Some(pending_id) = state.pending_bash_message_id {
                    state.messages.retain(|m| m.id != pending_id);
                }

                let command =
                    crate::services::handlers::shell::extract_command_from_tool_call(tool_call)
                        .unwrap_or_else(|_| "unknown command".to_string());

                // Determine state: Skipped (yellow) or Rejected (red)
                let run_state = if color == Some(Color::Yellow) {
                    crate::services::bash_block::RunCommandState::Skipped
                } else {
                    crate::services::bash_block::RunCommandState::Rejected
                };

                state.messages.push(Message::render_run_command_block(
                    command,
                    message.clone(), // Use rejection message as result
                    run_state,
                    None,
                ));
            } else {
                // For other tools, remove the pending block first
                if let Ok(tool_call_uuid) = Uuid::parse_str(&tool_call.id) {
                    state.messages.retain(|m| m.id != tool_call_uuid);
                }
                if let Some(pending_id) = state.pending_bash_message_id {
                    state.messages.retain(|m| m.id != pending_id);
                }

                // Then add the rejected block
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
        }
        state.is_dialog_open = false;
        state.dialog_command = None;
        state.dialog_focused = false; // Reset focus when dialog closes
        state.text_area.set_text("");
    } else if state.show_shell_mode {
        if state.dialog_command.is_some() {
            // Interactive stall shell: resolve it correctly with captured history
            // instead of just rejecting it.
            if let Some(_tool_call) = &state.dialog_command {
                // Capture history for context
                let history_lines =
                    super::shell::trim_shell_lines(state.shell_history_lines.clone());
                let history_text = history_lines
                    .iter()
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");

                let result = super::shell::shell_command_to_tool_call_result(
                    state,
                    state.shell_pending_command_value.clone(),
                    Some(history_text),
                );

                // Send as a successful result so LLM gets the context
                let _ = channels.output_tx.try_send(OutputEvent::SendToolResult(
                    result,
                    false,
                    Vec::new(),
                ));
            }

            if state.active_shell_command.is_some() {
                super::shell::terminate_active_shell_session(state);
            }
            state.is_tool_call_shell_command = false;

            state.show_shell_mode = false;
            state.shell_popup_visible = false;
            state.shell_popup_expanded = false;
            state.text_area.set_shell_mode(false);
            state.text_area.set_text("");
            state.dialog_command = None;

            // Reset interactive stall tracking state
            state.shell_pending_command_executed = false;
            state.shell_pending_command_value = None;
            state.shell_pending_command_output = None;
            state.shell_pending_command_output_count = 0;

            // Invalidate cache to update the display
            crate::services::message::invalidate_message_lines_cache(state);
        } else {
            // On-demand shell: just tab out/background (don't remove the box)
            super::shell::background_shell_session(state);
        }
    } else {
        // No dialog, no shell — if streaming was active, this is a cancellation.
        // Mark cancel_requested so late streaming events that are already queued
        // in the channel get dropped instead of re-creating content.
        if was_streaming {
            state.cancel_requested = true;
        }
        state.text_area.set_text("");
    }

    state.messages.retain(|m| {
        m.id != state.streaming_tool_result_id.unwrap_or_default()
            && m.id != state.pending_bash_message_id.unwrap_or_default()
    });

    // Invalidate cache and scroll to bottom when something was actually
    // cancelled/rejected (dialog open, shell resolved, or streaming interrupted).
    // Skip for idle ESC (just clearing text or closing a popup/dropdown).
    if was_streaming || was_dialog_open || was_shell_mode {
        crate::services::message::invalidate_message_lines_cache(state);
        state.stay_at_bottom = true;
    }
}

/// Handle show confirmation dialog event
pub fn handle_show_confirmation_dialog(
    state: &mut AppState,
    tool_call: stakpak_shared::models::integrations::openai::ToolCall,
    input_tx: &Sender<InputEvent>,
    output_tx: &Sender<OutputEvent>,
    _terminal_size: Size,
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
        let tool_name = strip_tool_name(&tool_call.function.name);
        if tool_name == "run_command" {
            // Use unified block for run_command
            let command =
                crate::services::handlers::shell::extract_command_from_tool_call(&tool_call)
                    .unwrap_or_else(|_| "unknown command".to_string());
            state.messages.push(Message::render_run_command_block(
                command,
                Some("Tool call already executed".to_string()),
                crate::services::bash_block::RunCommandState::Error,
                None,
            ));
        } else {
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
        }
        state.is_dialog_open = false;
        state.dialog_command = None;
        return;
    }

    state.dialog_command = Some(tool_call.clone());
    let tool_name = strip_tool_name(&tool_call.function.name);
    if tool_name == "run_command" {
        state.latest_tool_call = Some(tool_call.clone());
    }
    let is_auto_approved = state.auto_approve_manager.should_auto_approve(&tool_call);

    // Tool call is pending - create pending border block and check if we should show popup
    // For run_command, try to use tool_call.id as UUID so removal logic in event_loop works
    let message_id = if tool_name == "run_command" {
        Uuid::parse_str(&tool_call.id).unwrap_or_else(|_| Uuid::new_v4())
    } else {
        Uuid::new_v4()
    };

    // Save the previous pending block ID before we overwrite it.
    // This is needed so we can clean up the first tool's pending block when
    // subsequent tools are added to the approval bar (see !was_empty branch below).
    let previous_pending_bash_message_id = state.pending_bash_message_id;

    // Use unified run command block for run_command tool calls
    if tool_name == "run_command" {
        // Extract command from tool call arguments
        let command = crate::services::handlers::shell::extract_command_from_tool_call(&tool_call)
            .unwrap_or_else(|_| "unknown command".to_string());
        state.messages.push(Message::render_run_command_block(
            command,
            None, // No result yet
            crate::services::bash_block::RunCommandState::Pending,
            Some(message_id),
        ));
    } else if tool_name == "resume_subagent_task" {
        // For resume_subagent_task, use the special subagent pending block
        // Try to get pause info from cached subagent state
        let pause_info = serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments)
            .ok()
            .and_then(|args| {
                args.get("task_id")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .and_then(|task_id| state.subagent_pause_info.get(&task_id).cloned());

        state
            .messages
            .push(Message::render_subagent_resume_pending_block(
                tool_call.clone(),
                is_auto_approved,
                pause_info,
                Some(message_id),
            ));
    } else {
        state.messages.push(Message::render_pending_border_block(
            tool_call.clone(),
            is_auto_approved,
            Some(message_id),
        ));
    }
    state.pending_bash_message_id = Some(message_id);

    // Invalidate cache so the new message gets rendered
    invalidate_message_lines_cache(state);

    state.dialog_command = Some(tool_call.clone());
    // Only set is_dialog_open if NOT using the new approval bar flow
    // When toggle_approved_message is true, we use the approval bar instead
    if !state.toggle_approved_message {
        state.is_dialog_open = true;
    }
    state.loading = false;
    state.is_streaming = false;
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

        // Set is_dialog_open so handle_esc can process the rejection
        state.is_dialog_open = true;

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

        // Update run_command block to Running state before execution starts
        update_run_command_to_running(state, &tool_call);

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

    // Tool call is pending - add to approval bar (inline approval)
    if !tool_calls.is_empty() && state.toggle_approved_message {
        let was_empty = state.approval_bar.actions().is_empty();

        // Add tools to the bar (add_action handles duplicate prevention internally)
        for tc in tool_calls {
            state.approval_bar.add_action(tc);
        }

        // If this is the first time showing the approval bar, scroll to show the tool call
        if was_empty && !state.approval_bar.actions().is_empty() {
            state.scroll_to_last_message_start = true;
            state.stay_at_bottom = false;
        }

        // If we just added tools to an empty bar, the first one's pending block
        // is already displayed above. For subsequent tools added, we don't create
        // new pending blocks - the bar navigation will handle switching between them.
        if !was_empty {
            // Remove the pending block we just created since it's not the selected one
            if let Some(pending_id) = state.pending_bash_message_id {
                state.messages.retain(|m| m.id != pending_id);
            }
            // Also remove the previous pending block (from the first tool call that
            // was kept when the bar was initially empty). Without this, the first
            // tool's preview block becomes orphaned and stays stuck in the messages
            // area while the user cycles through tool calls with arrow keys.
            if let Some(prev_id) = previous_pending_bash_message_id {
                state.messages.retain(|m| m.id != prev_id);
            }
            state.pending_bash_message_id = None;

            // Re-create the pending block for the currently selected tool in the bar
            // so the user always sees a preview for the active tab.
            super::tool::create_pending_block_for_selected_tool(state);

            // Force-invalidate cache — bypass the streaming guard because the user
            // needs to see the correct preview even if is_streaming is still true.
            state.assembled_lines_cache = None;
            state.visible_lines_cache = None;
            state.message_lines_cache = None;
            state.collapsed_message_lines_cache = None;
        }
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
