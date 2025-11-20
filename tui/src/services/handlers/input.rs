//! Input Event Handlers
//!
//! Handles all input-related events including text input, cursor movement, and paste operations.

use crate::app::{AppState, InputEvent, OutputEvent};
use crate::constants::MAX_PASTE_CHAR_COUNT;
use crate::services::auto_approve::AutoApprovePolicy;
use crate::services::commands::{CommandContext, execute_command};
use crate::services::file_search::handle_file_selection;
use crate::services::helper_block::render_system_message;
use crate::services::helper_block::{push_clear_message, push_error_message, push_styled_message};
use crate::services::message::Message;
use ratatui::style::{Color, Style};
use stakpak_shared::models::integrations::openai::AgentModel;
use tokio::sync::mpsc::Sender;

use crate::constants::{CONTEXT_MAX_UTIL_TOKENS, CONTEXT_MAX_UTIL_TOKENS_ECO};

/// Handle InputChanged event - routes to appropriate handler based on popup state
pub fn handle_input_changed_event(state: &mut AppState, c: char, input_tx: &Sender<InputEvent>) {
    if state.approval_popup.is_visible() {
        if c == ' ' {
            state.approval_popup.toggle_approval_status();
            return;
        }
        return; // Consume all input when popup is visible
    }
    if state.show_command_palette {
        // Handle search input for command palette
        let _ = input_tx.try_send(InputEvent::CommandPaletteSearchInputChanged(c));
        return;
    }
    if state.show_rulebook_switcher {
        if c == ' ' {
            let _ = input_tx.try_send(InputEvent::RulebookSwitcherToggle);
            return;
        }
        // Handle search input
        let _ = input_tx.try_send(InputEvent::RulebookSearchInputChanged(c));
        return;
    }
    handle_input_changed(state, c);
}

/// Handle InputBackspace event - routes to appropriate handler based on popup state
pub fn handle_input_backspace_event(state: &mut AppState, input_tx: &Sender<InputEvent>) {
    if state.show_command_palette {
        let _ = input_tx.try_send(InputEvent::CommandPaletteSearchBackspace);
        return;
    }
    if state.show_rulebook_switcher {
        let _ = input_tx.try_send(InputEvent::RulebookSearchBackspace);
        return;
    }
    handle_input_backspace(state);
}

/// Handle InputSubmitted event - routes to appropriate handler based on state
pub fn handle_input_submitted_event(
    state: &mut AppState,
    message_area_height: usize,
    output_tx: &Sender<OutputEvent>,
    input_tx: &Sender<InputEvent>,
    shell_tx: &Sender<InputEvent>,
) {
    if state.show_profile_switcher {
        let _ = input_tx.try_send(InputEvent::ProfileSwitcherSelect);
        return;
    }
    if state.show_command_palette {
        // Execute the selected command
        use super::tool::execute_command_palette_selection;
        execute_command_palette_selection(state, input_tx, output_tx);
        return;
    }
    if state.show_rulebook_switcher {
        let _ = input_tx.try_send(InputEvent::RulebookSwitcherConfirm);
        return;
    }
    if state.show_recovery_options_popup {
        if let Some(selected) = state.recovery_options.get(state.recovery_popup_selected)
            && let Some(response) = &state.recovery_response
        {
            let recovery_request_id = response.id.clone().unwrap_or_default();
            let selected_option_id = selected.id;
            let mode = selected.mode.clone();

            // Send recovery action
            let _ = output_tx.try_send(OutputEvent::RecoveryAction {
                action: stakpak_api::models::RecoveryActionType::Approve,
                recovery_request_id,
                selected_option_id,
                mode,
            });
        }

        state.recovery_options.clear();
        state.recovery_response = None;
        state.recovery_popup_selected = 0;
        state.show_recovery_options_popup = false;
        return;
    }
    if state.approval_popup.is_visible() {
        // Update approved and rejected tool calls from popup
        state.message_approved_tools = state
            .approval_popup
            .get_approved_tool_calls()
            .into_iter()
            .cloned()
            .collect();
        state.message_rejected_tools = state
            .approval_popup
            .get_rejected_tool_calls()
            .into_iter()
            .cloned()
            .collect();

        // Create tools_status maintaining the original order from message_tool_calls
        use crate::app::ToolCallStatus;
        use stakpak_shared::models::integrations::openai::ToolCall;
        if let Some(tool_calls) = &state.message_tool_calls {
            let tools_status: Vec<(ToolCall, bool)> = tool_calls
                .iter()
                .map(|tool_call| {
                    let is_approved = state.message_approved_tools.contains(tool_call);
                    let is_rejected = state.message_rejected_tools.contains(tool_call);
                    let status = if is_approved {
                        ToolCallStatus::Approved
                    } else {
                        ToolCallStatus::Rejected
                    };
                    state.tool_call_execution_order.push(tool_call.id.clone());
                    state
                        .session_tool_calls_queue
                        .insert(tool_call.id.clone(), status);
                    (tool_call.clone(), is_approved && !is_rejected)
                })
                .collect();

            // Get the first tool from the ordered list
            if let Some((first_tool, is_approved)) = tools_status.first() {
                // Compare with dialog_command to determine action
                if let Some(dialog_command) = &state.dialog_command
                    && first_tool == dialog_command
                {
                    state
                        .session_tool_calls_queue
                        .insert(dialog_command.id.clone(), ToolCallStatus::Executed);
                    if *is_approved {
                        // Fire accept tool
                        let _ = output_tx.try_send(OutputEvent::AcceptTool(dialog_command.clone()));
                    } else {
                        // Fire handle reject
                        let _ = input_tx.try_send(InputEvent::HandleReject(None, true, None));
                    }
                }
            }
        }

        // Clear message_tool_calls to prevent further ShowConfirmationDialog calls
        // This prevents the race condition where individual tool calls try to show dialogs
        state.message_tool_calls = None;
        state.is_dialog_open = false;

        state.approval_popup.escape();
        return;
    }
    if !state.is_pasting {
        handle_input_submitted(state, message_area_height, output_tx, input_tx, shell_tx);
    }
}

/// Handle character input change
pub fn handle_input_changed(state: &mut AppState, c: char) {
    state.show_shortcuts = false;

    if c == '$' && (state.input().is_empty() || state.is_dialog_open) && !state.show_sessions_dialog
    {
        state.text_area.set_text("");
        // Shell mode toggle will be handled by shell module
        use super::shell;
        shell::handle_shell_mode(state);
        return;
    }

    state.text_area.insert_str(&c.to_string());

    // If a large paste placeholder is present and input is edited, only clear pasted state if placeholder is completely removed
    if let Some(placeholder) = &state.pasted_placeholder
        && !state.input().contains(placeholder)
    {
        state.pasted_long_text = None;
        state.pasted_placeholder = None;
    }

    if state.input().starts_with('/') {
        if state.file_search.is_active() {
            state.file_search.reset();
        }
        state.show_helper_dropdown = true;
        state.helper_scroll = 0;
    }

    if let Some(tx) = &state.file_search_tx {
        let _ = tx.try_send((state.input().to_string(), state.cursor_position()));
    }

    if state.input().is_empty() {
        state.show_helper_dropdown = false;
        state.filtered_helpers.clear();
        state.filtered_files.clear();
        state.helper_selected = 0;
        state.helper_scroll = 0;
        state.file_search.reset();
    }
}

/// Handle backspace input
pub fn handle_input_backspace(state: &mut AppState) {
    state.text_area.delete_backward(1);

    // If a large paste placeholder is present and input is edited, only clear pasted state if placeholder is completely removed
    if let Some(placeholder) = &state.pasted_placeholder
        && !state.input().contains(placeholder)
    {
        state.pasted_long_text = None;
        state.pasted_placeholder = None;
    }

    // Send input to file_search worker (async, non-blocking)
    if let Some(tx) = &state.file_search_tx {
        let _ = tx.try_send((state.input().to_string(), state.cursor_position()));
    }

    // Handle helper filtering after backspace
    if state.input().starts_with('/') {
        if state.file_search.is_active() {
            state.file_search.reset();
        }
        state.show_helper_dropdown = true;
        state.helper_scroll = 0;
    }

    // Hide dropdown if input is empty
    if state.input().is_empty() {
        state.show_helper_dropdown = false;
        state.filtered_helpers.clear();
        state.filtered_files.clear();
        state.helper_selected = 0;
        state.helper_scroll = 0;
        state.file_search.reset();
    }
}

/// Handle input submission
fn handle_input_submitted(
    state: &mut AppState,
    message_area_height: usize,
    output_tx: &Sender<OutputEvent>,
    input_tx: &Sender<InputEvent>,
    shell_tx: &Sender<InputEvent>,
) {
    if state.show_recovery_options_popup {
        state.show_recovery_options_popup = false;
        return;
    }
    if state.show_shell_mode {
        if state.active_shell_command.is_some() {
            let input = state.input().to_string();
            state.text_area.set_text("");

            // Send the input to the shell command
            if let Some(cmd) = &state.active_shell_command {
                let stdin_tx = cmd.stdin_tx.clone();
                tokio::spawn(async move {
                    let _ = stdin_tx.send(input).await;
                });
            }
            state.waiting_for_shell_input = false;
            return;
        }

        // Otherwise, it's a new shell command
        if !state.input().trim().is_empty() {
            let command = state.input().to_string();
            state.text_area.set_text("");
            state.show_helper_dropdown = false;

            // Run the shell command with the shell event channel
            state.run_shell_command(command.clone(), shell_tx);
        }
        return;
    }

    if state.input().trim() == "clear" {
        push_clear_message(state);
        return;
    }

    // Handle toggle auto-approve command
    let input_text = state.input().to_string();
    if input_text.trim().starts_with("/toggle_auto_approve") {
        let input_parts: Vec<&str> = input_text.split_whitespace().collect();
        if input_parts.len() >= 2 {
            let tool_name = input_parts[1];

            // Get current policy for the tool
            let current_policy = state
                .auto_approve_manager
                .get_policy_for_tool_name(tool_name);
            let new_policy = if current_policy == AutoApprovePolicy::Auto {
                AutoApprovePolicy::Prompt
            } else {
                AutoApprovePolicy::Auto
            };

            if let Err(e) = state
                .auto_approve_manager
                .update_tool_policy(tool_name, new_policy.clone())
            {
                push_error_message(
                    state,
                    &format!("Failed to toggle auto-approve for {}: {}", tool_name, e),
                    None,
                );
            } else {
                let status = if new_policy == AutoApprovePolicy::Auto {
                    "enabled"
                } else {
                    "disabled"
                };
                push_styled_message(
                    state,
                    &format!("Auto-approve {} for {} tool", status, tool_name),
                    Color::Yellow,
                    "",
                    Color::Yellow,
                );
            }
        } else {
            push_error_message(state, "Usage: /toggle_auto_approve <tool_name>", None);
        }
        state.text_area.set_text("");
        state.show_helper_dropdown = false;
        return;
    }

    if state.show_sessions_dialog {
        let selected = &state.sessions[state.session_selected];
        let _ = output_tx.try_send(OutputEvent::SwitchToSession(selected.id.to_string()));
        state.message_tool_calls = None;
        state.message_approved_tools.clear();
        state.message_rejected_tools.clear();
        state.tool_call_execution_order.clear();
        state.session_tool_calls_queue.clear();
        state.toggle_approved_message = true;
        state.messages.clear();

        // Reset usage for the switched session
        state.total_session_usage = stakpak_shared::models::integrations::openai::Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            prompt_tokens_details: None,
        };

        render_system_message(state, &format!("Switching to session . {}", selected.title));
        state.show_sessions_dialog = false;
    } else if state.is_dialog_open {
        state.toggle_approved_message = true;
        state.approval_popup.toggle();
        state.is_dialog_open = true;
        state.dialog_selected = 0;
        state.dialog_focused = false;
        state.text_area.set_text("");
    // Reset focus when dialog closes
    } else if state.show_helper_dropdown {
        if state.file_search.is_active() {
            let selected_file = state
                .file_search
                .get_file_at_index(state.helper_selected)
                .map(|s| s.to_string());
            if let Some(selected_file) = selected_file {
                handle_file_selection(state, &selected_file);
            }
            return;
        }
        if !state.filtered_helpers.is_empty() {
            let command_id = state.filtered_helpers[state.helper_selected].command;

            // Use unified command executor
            let ctx = CommandContext {
                state,
                input_tx,
                output_tx,
            };
            if let Err(e) = execute_command(command_id, ctx) {
                push_error_message(state, &e, None);
            }
        }
    } else if !state.input().trim().is_empty() {
        // PERFORMANCE FIX: Simplified condition for submission
        // Allow submission of any non-empty input that's not a recognized helper command
        let input_height = 3;
        let total_lines = state.messages.len() * 2;
        let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        let was_at_bottom = state.scroll == max_scroll;

        let mut final_input = state.input().to_string();

        // Process any pending pastes first
        for (placeholder, long_text) in state.pending_pastes.drain(..) {
            if final_input.contains(&placeholder) {
                final_input = final_input.replace(&placeholder, &long_text);
                state.text_area.set_text(&final_input);
                break; // Only process the first matching paste
            }
        }

        // Also handle the existing pasted_placeholder system
        if let (Some(placeholder), Some(long_text)) =
            (&state.pasted_placeholder, &state.pasted_long_text)
            && final_input.contains(placeholder)
        {
            final_input = final_input.replace(placeholder, long_text);
            state.text_area.set_text(&final_input);
        }
        state.pasted_long_text = None;
        state.pasted_placeholder = None;

        // Use eco limit if eco model is selected
        let max_tokens = match state.model {
            AgentModel::Eco => CONTEXT_MAX_UTIL_TOKENS_ECO,
            AgentModel::Smart => CONTEXT_MAX_UTIL_TOKENS,
        };

        let capped_tokens = state.total_session_usage.total_tokens.min(max_tokens);
        let utilization_ratio = (capped_tokens as f64 / max_tokens as f64).clamp(0.0, 1.0);
        let utilization_pct = (utilization_ratio * 100.0).round() as u64;

        let user_message_text = final_input.clone();
        if utilization_pct < 92 {
            let _ = output_tx.try_send(OutputEvent::UserMessage(
                final_input.clone(),
                state.shell_tool_calls.clone(),
            ));
            let _ = input_tx.try_send(InputEvent::AddUserMessage(user_message_text));
        }

        if utilization_pct >= 92 {
            if !state.messages.is_empty() {
                state.messages.push(Message::plain_text(""));
            }

            state.messages.push(Message::user(final_input, None));
            // Add spacing after user message
            state.messages.push(Message::plain_text(""));
            state.messages.push(Message::info("Approaching max context limit this will overload the model and might not work as expected. ctrl+g for more".to_string(), Some(Style::default().fg(Color::Yellow))));
            state.messages.push(Message::plain_text(""));
            state.messages.push(Message::info(
                "Start a new session or /summarize to export compressed summary to be resued"
                    .to_string(),
                Some(Style::default().fg(Color::Green)),
            ));
            state.messages.push(Message::plain_text(""));
        }
        state.shell_tool_calls = None;
        state.text_area.set_text("");
        let total_lines = state.messages.len() * 2;
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        if was_at_bottom {
            state.scroll = max_scroll;
            state.scroll_to_bottom = true;
            state.stay_at_bottom = true;
        }
        // Loading will be managed by stream processing
        state.spinner_frame = 0;
    }
}

/// Handle input submitted with specific text and color
pub fn handle_input_submitted_with(
    state: &mut AppState,
    s: String,
    color: Option<Color>,
    message_area_height: usize,
) {
    state.shell_tool_calls = None;
    let input_height = 3;
    let total_lines = state.messages.len() * 2;
    let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
    let max_scroll = total_lines.saturating_sub(max_visible_lines);
    let was_at_bottom = state.scroll == max_scroll;
    state.messages.push(Message::submitted_with(
        None,
        s.clone(),
        color.map(|c| Style::default().fg(c)),
    ));
    // Loading will be managed by stream processing
    state.text_area.set_text("");

    // If content changed while user is scrolled up, mark it
    if !was_at_bottom {
        state.content_changed_while_scrolled_up = true;
    }

    let total_lines = state.messages.len() * 2;
    let max_scroll = total_lines.saturating_sub(max_visible_lines);
    if was_at_bottom {
        state.scroll = max_scroll;
        state.scroll_to_bottom = true;
        state.stay_at_bottom = true;
    }
}

/// Handle paste operation
pub fn handle_paste(state: &mut AppState, pasted: String) -> bool {
    // Normalize line endings: many terminals convert newlines to \r when pasting,
    // but textarea expects \n. This is the same fix used in Codex.
    let normalized_pasted = pasted.replace("\r\n", "\n").replace('\r', "\n");

    let char_count = normalized_pasted.chars().count();
    if char_count > MAX_PASTE_CHAR_COUNT {
        let placeholder = format!("[Pasted Content {char_count} chars]");
        state.text_area.insert_element(&placeholder);
        state.pending_pastes.push((placeholder, normalized_pasted));
    } else {
        state.text_area.insert_str(&normalized_pasted);
    }

    true
}

/// Handle input delete (clear input)
pub fn handle_input_delete(state: &mut AppState) {
    state.text_area.set_text("");
    state.show_helper_dropdown = false;
}

/// Handle input delete word
pub fn handle_input_delete_word(state: &mut AppState) {
    state.text_area.delete_backward_word();
    state.show_helper_dropdown = false;
}

/// Handle cursor move to start of line
pub fn handle_input_cursor_start(state: &mut AppState) {
    state.text_area.move_cursor_to_beginning_of_line(false);
}

/// Handle cursor move to end of line
pub fn handle_input_cursor_end(state: &mut AppState) {
    state.text_area.move_cursor_to_end_of_line(false);
}

/// Handle cursor move to previous word
pub fn handle_input_cursor_prev_word(state: &mut AppState) {
    state
        .text_area
        .set_cursor(state.text_area.beginning_of_previous_word());
}

/// Handle cursor move to next word
pub fn handle_input_cursor_next_word(state: &mut AppState) {
    state
        .text_area
        .set_cursor(state.text_area.end_of_next_word());
}

/// Handle cursor left movement (with approval popup check)
pub fn handle_cursor_left(state: &mut AppState) {
    if state.approval_popup.is_visible() {
        state.approval_popup.prev_tab();
        return; // Event was consumed by popup
    }
    state.text_area.move_cursor_left();
}

/// Handle cursor right movement (with approval popup check)
pub fn handle_cursor_right(state: &mut AppState) {
    if state.approval_popup.is_visible() {
        state.approval_popup.next_tab();
        return; // Event was consumed by popup
    }
    state.text_area.move_cursor_right();
}
