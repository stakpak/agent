//! Miscellaneous Event Handlers
//!
//! Handles miscellaneous events that don't fit into other categories.

use crate::app::{AppState, InputEvent};
use crate::services::bash_block::render_bash_block_rejected;
use crate::services::commands::list_auto_approved_tools;
use crate::services::file_search::{handle_file_selection, handle_tab_trigger};
use crate::services::helper_block::{handle_errors, push_error_message, push_styled_message};
use crate::services::message::Message;
use crate::services::message::get_wrapped_collapsed_message_lines_cached;
use ratatui::layout::Size;
use ratatui::style::Color;
use stakpak_shared::models::llm::LLMModel;
use uuid::Uuid;

/// Handle error event
pub fn handle_error(state: &mut AppState, err: String) {
    if err.contains("FREE_PLAN") {
        push_error_message(state, "Free plan limit reached.", None);
        push_error_message(
            state,
            "Please top up your account at https://stakpak.dev/settings/billing to keep Stakpaking.",
            Some(true),
        );
        return;
    }
    if err == "STREAM_CANCELLED" {
        let rendered_lines =
            render_bash_block_rejected("Interrupted by user", "System", None, None);
        state.messages.push(Message {
            id: Uuid::new_v4(),
            content: crate::services::message::MessageContent::StyledBlock(rendered_lines),
            is_collapsed: None,
        });
        return;
    }
    let mut error_message = handle_errors(err);
    if error_message.contains("RETRY_ATTEMPT") || error_message.contains("MAX_RETRY_REACHED") {
        if error_message.contains("RETRY_ATTEMPT") {
            let retry_attempt = error_message.split("RETRY_ATTEMPT_").last().unwrap_or("1");
            error_message = format!(
                "There was an issue sending your request, retrying attempt {}...",
                retry_attempt
            );
        } else if error_message.contains("MAX_RETRY_REACHED") {
            error_message = "Maximum retry attempts reached. Please try again later.".to_string();
        }
        use super::tool::handle_retry_mechanism;
        handle_retry_mechanism(state);
    }

    push_error_message(state, &error_message, None);
}

/// Handle resized event
pub fn handle_resized(state: &mut AppState, width: u16, height: u16) {
    let old_terminal_size = state.terminal_size;
    state.terminal_size = Size { width, height };

    // Recreate the approval popup if it's visible and terminal size changed
    if state.approval_popup.is_visible() && old_terminal_size != state.terminal_size {
        state
            .approval_popup
            .recreate_with_terminal_size(state.terminal_size);
    }

    // Resize shell parser
    // We reserve space for borders (4 columns for side borders/padding, 2 rows for top/bottom borders)
    let shell_rows = height.saturating_sub(2).max(1);
    let shell_cols = width.saturating_sub(4).max(1);
    state.shell_screen.set_size(shell_rows, shell_cols);
}

/// Handle toggle cursor visible event
pub fn handle_toggle_cursor_visible(state: &mut AppState) {
    state.cursor_visible = !state.cursor_visible;
}

/// Handle toggle auto approve event
pub fn handle_toggle_auto_approve(state: &mut AppState) {
    if let Err(e) = state.auto_approve_manager.toggle_enabled() {
        push_error_message(
            state,
            &format!("Failed to toggle auto-approve: {}", e),
            None,
        );
    } else {
        let status = if state.auto_approve_manager.is_enabled() {
            "enabled"
        } else {
            "disabled"
        };

        let status_color = if state.auto_approve_manager.is_enabled() {
            Color::Green
        } else {
            Color::LightRed
        };

        push_styled_message(
            state,
            &format!("Auto-approve {}", status),
            status_color,
            "",
            Color::Green,
        );
    }
}

/// Handle auto approve current tool event
pub fn handle_auto_approve_current_tool(state: &mut AppState) {
    list_auto_approved_tools(state);
}

/// Handle tab event
pub fn handle_tab(state: &mut AppState, message_area_height: usize, message_area_width: usize) {
    if state.show_collapsed_messages {
        handle_collapsed_messages_tab(state, message_area_height, message_area_width);
    } else {
        handle_tab_normal(state);
    }
}

/// Handle tab in normal mode
fn handle_tab_normal(state: &mut AppState) {
    // If side panel is visible and input is empty, cycle sections
    if state.show_side_panel && state.text_area.text().is_empty() {
        state.side_panel_focus = state.side_panel_focus.next();
        return;
    }

    // Check if we're already in helper dropdown mode
    if state.show_helper_dropdown {
        // If in file file_search mode, handle file selection
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
        // Handle helper selection - auto-complete the selected helper
        if !state.filtered_helpers.is_empty() && state.input().starts_with('/') {
            let selected_helper = &state.filtered_helpers[state.helper_selected];
            // Commands that take arguments should have a trailing space
            let needs_space = matches!(selected_helper.command, "/editor" | "/toggle_auto_approve");
            let new_text = if needs_space {
                format!("{} ", selected_helper.command)
            } else {
                selected_helper.command.to_string()
            };
            state.text_area.set_text(&new_text);
            // Position cursor at the end of the text
            state.text_area.set_cursor(new_text.len());
            state.show_helper_dropdown = false;
            state.filtered_helpers.clear();
            state.helper_selected = 0;
            state.helper_scroll = 0;
            return;
        }
        return;
    }
    // Trigger file file_search with Tab
    handle_tab_trigger(state);
}

/// Handle collapsed messages tab
fn handle_collapsed_messages_tab(
    state: &mut AppState,
    message_area_height: usize,
    message_area_width: usize,
) {
    let collapsed_messages: Vec<Message> = state
        .messages
        .iter()
        .filter(|m| m.is_collapsed == Some(true))
        .cloned()
        .collect();

    if collapsed_messages.is_empty() {
        return;
    }

    // Move to next message
    state.collapsed_messages_selected =
        (state.collapsed_messages_selected + 1) % collapsed_messages.len();

    // Calculate scroll position to show the top of the selected message
    let mut line_count = 0;

    for (i, _message) in collapsed_messages.iter().enumerate() {
        if i == state.collapsed_messages_selected {
            // This is our target message, set scroll to show its top
            state.collapsed_messages_scroll = line_count;
            break;
        }

        // Count lines for this message
        let message_lines = get_wrapped_collapsed_message_lines_cached(state, message_area_width);
        line_count += message_lines.len();
    }

    // Ensure scroll doesn't exceed bounds
    let all_lines = get_wrapped_collapsed_message_lines_cached(state, message_area_width);
    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(message_area_height);
    state.collapsed_messages_scroll = state.collapsed_messages_scroll.min(max_scroll);
}

/// Handle Ctrl+S event
pub fn handle_ctrl_s(state: &mut AppState, input_tx: &tokio::sync::mpsc::Sender<InputEvent>) {
    if state.show_rulebook_switcher {
        let _ = input_tx.try_send(InputEvent::RulebookSwitcherSelectAll);
        return;
    }
    let _ = input_tx.try_send(InputEvent::ShowShortcuts);
}

/// Handle attempt quit event
pub fn handle_attempt_quit(state: &mut AppState, input_tx: &tokio::sync::mpsc::Sender<InputEvent>) {
    use std::time::Instant;
    let now = Instant::now();
    if !state.ctrl_c_pressed_once
        || state.ctrl_c_timer.is_none()
        || state.ctrl_c_timer.map(|t| now > t).unwrap_or(true)
    {
        // First press or timer expired: clear input, move cursor, set timer
        state.text_area.set_text("");
        state.ctrl_c_pressed_once = true;
        state.ctrl_c_timer = Some(now + std::time::Duration::from_secs(2));
    } else {
        // Second press within 2s: trigger quit
        state.ctrl_c_pressed_once = false;
        state.ctrl_c_timer = None;
        let _ = input_tx.try_send(InputEvent::Quit);
    }
}

/// Handle toggle mouse capture event
pub fn handle_toggle_mouse_capture(state: &mut AppState) {
    #[cfg(unix)]
    let _ = crate::toggle_mouse_capture(state);
}

/// Handle set sessions event
pub fn handle_set_sessions(state: &mut AppState, sessions: Vec<crate::app::SessionInfo>) {
    // Terminate any active shell before showing sessions dialog
    if let Some(cmd) = &state.active_shell_command {
        let _ = cmd.kill();
    }
    if let Some(shell_msg_id) = state.interactive_shell_message_id {
        state.messages.retain(|m| m.id != shell_msg_id);
    }
    state.active_shell_command = None;
    state.active_shell_command_output = None;
    state.interactive_shell_message_id = None;
    state.show_shell_mode = false;
    state.shell_popup_visible = false;
    state.shell_popup_expanded = false;
    state.waiting_for_shell_input = false;
    state.text_area.set_shell_mode(false);

    state.sessions = sessions;
    state.show_sessions_dialog = true;
}

/// Handle start loading operation event
pub fn handle_start_loading_operation(
    state: &mut AppState,
    operation: crate::app::LoadingOperation,
) {
    state.loading_manager.start_operation(operation.clone());
    state.loading = state.loading_manager.is_loading();
    state.loading_type = state.loading_manager.get_loading_type();
}

/// Handle end loading operation event
pub fn handle_end_loading_operation(state: &mut AppState, operation: crate::app::LoadingOperation) {
    state.loading_manager.end_operation(operation);
    state.loading = state.loading_manager.is_loading();
    state.loading_type = state.loading_manager.get_loading_type();
}

/// Handle assistant message event
pub fn handle_assistant_message(state: &mut AppState, msg: String) {
    state.messages.push(Message::assistant(None, msg, None));

    // Auto-show side panel on first message (assistant)
    state.auto_show_side_panel();
}

/// Handle get status event
pub fn handle_get_status(state: &mut AppState, account_info: String) {
    state.account_info = account_info;
}

/// Handle stream model event
pub fn handle_stream_model(state: &mut AppState, model: LLMModel) {
    state.llm_model = Some(model);
}
