use super::message::extract_truncated_command_arguments;
use crate::app::{AppState, InputEvent, OutputEvent};
use crate::services::auto_approve::AutoApprovePolicy;
use crate::services::bash_block::{preprocess_terminal_output, render_bash_block_rejected};
use crate::services::file_search::{handle_file_selection, handle_tab_trigger};
use crate::services::helper_block::{
    handle_errors, push_clear_message, push_error_message, push_help_message,
    push_memorize_message, push_status_message, push_styled_message, render_system_message,
    welcome_messages,
};
use crate::services::message::{
    Message, MessageContent, get_command_type_name, get_wrapped_collapsed_message_lines_cached,
    get_wrapped_message_lines_cached,
};
use crate::services::shell_mode::SHELL_PROMPT_PREFIX;
use ratatui::style::{Color, Style};
use serde_json;
use stakpak_shared::helper::truncate_output;
use stakpak_shared::models::integrations::openai::{
    FunctionCall, ToolCall, ToolCallResult, ToolCallResultProgress, ToolCallResultStatus,
};
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

const SCROLL_LINES: usize = 2;
const MAX_PASTE_CHAR_COUNT: usize = 1000;

#[allow(clippy::too_many_arguments)]
pub fn update(
    state: &mut AppState,
    event: InputEvent,
    message_area_height: usize,
    message_area_width: usize,
    input_tx: &Sender<InputEvent>,
    output_tx: &Sender<OutputEvent>,
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
    shell_tx: &Sender<InputEvent>,
) {
    state.scroll = state.scroll.max(0);
    match event {
        InputEvent::Up => {
            if state.show_sessions_dialog {
                if state.session_selected > 0 {
                    state.session_selected -= 1;
                }
            } else if state.show_helper_dropdown {
                handle_dropdown_up(state);
            } else if state.is_dialog_open && state.dialog_focused {
                // Handle dialog navigation only when dialog is focused
                if state.dialog_selected > 0 {
                    state.dialog_selected -= 1;
                } else {
                    // Wrap to the last option
                    state.dialog_selected = 2;
                }
            } else {
                handle_scroll_up(state);
            }
        }
        InputEvent::Down => {
            if state.show_sessions_dialog {
                if state.session_selected + 1 < state.sessions.len() {
                    state.session_selected += 1;
                }
            } else if state.show_helper_dropdown {
                handle_dropdown_down(state);
            } else if state.is_dialog_open && state.dialog_focused {
                // Handle dialog navigation only when dialog is focused
                if state.dialog_selected < 2 {
                    state.dialog_selected += 1;
                } else {
                    // Wrap to the first option
                    state.dialog_selected = 0;
                }
            } else {
                handle_scroll_down(state, message_area_height, message_area_width);
            }
        }
        InputEvent::DropdownUp => handle_dropdown_up(state),
        InputEvent::DropdownDown => handle_dropdown_down(state),
        InputEvent::DialogUp => {
            if state.is_dialog_open {
                if state.dialog_selected > 0 {
                    state.dialog_selected -= 1;
                } else {
                    // Wrap to the last option
                    state.dialog_selected = 2;
                }
            }
        }
        InputEvent::DialogDown => {
            if state.is_dialog_open {
                if state.dialog_selected < 2 {
                    state.dialog_selected += 1;
                } else {
                    // Wrap to the first option
                    state.dialog_selected = 0;
                }
            }
        }
        InputEvent::InputChanged(c) => handle_input_changed(state, c),
        InputEvent::InputBackspace => handle_input_backspace(state),
        InputEvent::InputSubmitted => {
            if !state.is_pasting {
                handle_input_submitted(state, message_area_height, output_tx, input_tx, shell_tx);
            }
        }
        InputEvent::InputChangedNewline => handle_input_changed(state, '\n'),
        InputEvent::InputSubmittedWith(s) => {
            handle_input_submitted_with(state, s, None, message_area_height)
        }
        InputEvent::InputSubmittedWithColor(s, color) => {
            handle_input_submitted_with(state, s, Some(color), message_area_height)
        }
        InputEvent::StreamAssistantMessage(id, s) => {
            handle_stream_message(state, id, s, message_area_height)
        }
        InputEvent::StreamToolResult(progress) => handle_stream_tool_result(state, progress),
        InputEvent::AddUserMessage(s) => {
            // Add spacing before user message if not the first message
            if !state.messages.is_empty() {
                state.messages.push(Message::plain_text(""));
            }
            state.messages.push(Message::user(s, None));
            // Add spacing after user message
            state.messages.push(Message::plain_text(""));

            // Invalidate cache since messages changed
            crate::services::message::invalidate_message_lines_cache(state);
        }
        InputEvent::Error(err) => {
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
                render_bash_block_rejected("Interrupted by user", "System", None);
                return;
            }
            let mut error_message = handle_errors(err);
            if error_message.contains("RETRY_ATTEMPT")
                || error_message.contains("MAX_RETRY_REACHED")
            {
                if error_message.contains("RETRY_ATTEMPT") {
                    let retry_attempt = error_message.split("RETRY_ATTEMPT_").last().unwrap_or("1");
                    error_message = format!(
                        "There was an issue sending your request, retrying attempt {}...",
                        retry_attempt
                    );
                } else if error_message.contains("MAX_RETRY_REACHED") {
                    error_message =
                        "Maximum retry attempts reached. Please try again later.".to_string();
                }
                handle_retry_mechanism(state);
            }

            push_error_message(state, &error_message, None);
        }
        InputEvent::ScrollUp => handle_scroll_up(state),
        InputEvent::ScrollDown => {
            handle_scroll_down(state, message_area_height, message_area_width)
        }
        InputEvent::PageUp => {
            state.stay_at_bottom = false; // unlock from bottom
            handle_page_up(state, message_area_height);
            adjust_scroll(state, message_area_height, message_area_width);
        }
        InputEvent::PageDown => {
            state.stay_at_bottom = false; // unlock from bottom
            handle_page_down(state, message_area_height, message_area_width);
            adjust_scroll(state, message_area_height, message_area_width);
        }
        InputEvent::CursorLeft => {
            state.text_area.move_cursor_left();
        }
        InputEvent::CursorRight => {
            state.text_area.move_cursor_right();
        }
        InputEvent::ToggleCursorVisible => state.cursor_visible = !state.cursor_visible,
        InputEvent::ToggleAutoApprove => {
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
        InputEvent::AutoApproveCurrentTool => {
            list_auto_approved_tools(state);
        }
        InputEvent::ToggleDialogFocus => {
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

        InputEvent::ShowConfirmationDialog(tool_call) => {
            // Store the latest tool call for potential retry (only for run_command)
            if tool_call.function.name == "run_command" {
                state.latest_tool_call = Some(tool_call.clone());
            }
            state.dialog_command = Some(tool_call.clone());
            let is_auto_approved = state.auto_approve_manager.should_auto_approve(&tool_call);

            if tool_call.function.name == "str_replace" {
                state
                    .messages
                    .push(Message::render_collapsed_message(tool_call.clone()));
            }

            let message_id = Uuid::new_v4();
            state.messages.push(Message::render_pending_border_block(
                tool_call.clone(),
                is_auto_approved,
                Some(message_id),
            ));
            state.pending_bash_message_id = Some(message_id);

            // Check if auto-approve should be used
            if state.auto_approve_manager.should_auto_approve(&tool_call) {
                // Auto-approve the tool call
                let _ = output_tx.try_send(OutputEvent::AcceptTool(tool_call.clone()));
            } else {
                // Show confirmation dialog as usual
                state.dialog_command = Some(tool_call.clone());
                state.is_dialog_open = true;
                state.loading = false;
                state.dialog_focused = false; //Should be if we have multiple options, Default to dialog focused when dialog opens
            }
        }
        InputEvent::StartLoadingOperation(operation) => {
            state.loading_manager.start_operation(operation.clone());
            state.loading = state.loading_manager.is_loading();
            state.loading_type = state.loading_manager.get_loading_type();
        }
        InputEvent::EndLoadingOperation(operation) => {
            state.loading_manager.end_operation(operation);
            state.loading = state.loading_manager.is_loading();
            state.loading_type = state.loading_manager.get_loading_type();
        }
        InputEvent::HandleEsc => handle_esc(state, output_tx, cancel_tx, shell_tx, input_tx),

        InputEvent::GetStatus(account_info) => {
            state.account_info = account_info;
        }
        InputEvent::Tab => {
            if state.show_collapsed_messages {
                handle_collapsed_messages_tab(state, message_area_height, message_area_width);
            } else {
                handle_tab(state);
            }
        }
        InputEvent::SetSessions(sessions) => {
            state.sessions = sessions;
            state.show_sessions_dialog = true;
        }
        InputEvent::ShellOutput(line) => {
            // remove ansi codes
            let line = preprocess_terminal_output(&line);
            // normalize line endings
            let mut line = line.replace("\r\n", "\n").replace('\r', "\n");

            if let Some(output) = state.active_shell_command_output.as_mut() {
                let text = format!("{}\n", line);
                output.push_str(&text);
                *output = truncate_output(output);
            }

            line = truncate_output(&line);
            state.messages.push(Message::plain_text(line));
        }

        InputEvent::ShellError(line) => {
            let line = preprocess_terminal_output(&line);
            let line = line.replace("\r\n", "\n").replace('\r', "\n");
            push_error_message(state, &line, None);
        }

        InputEvent::ShellWaitingForInput => {
            state.waiting_for_shell_input = true;
            // Set textarea to shell mode when waiting for input
            state.text_area.set_shell_mode(true);
            // Allow user input when command is waiting
            adjust_scroll(state, message_area_height, message_area_width);
        }

        InputEvent::ShellCompleted(_code) => {
            // Command completed, reset active command state
            state.waiting_for_shell_input = false;

            if state.dialog_command.is_some() {
                let result = shell_command_to_tool_call_result(state);
                let _ = output_tx.try_send(OutputEvent::SendToolResult(result));
                if let Some(dialog_command) = &state.dialog_command {
                    if let Some(latest_tool_call) = &state.latest_tool_call {
                        if dialog_command.id == latest_tool_call.id {
                            state.latest_tool_call = None;
                        }
                    }
                }
                state.show_shell_mode = false;
                state.dialog_command = None;
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
        InputEvent::ShellClear => {
            // Clear the shell output buffer
            if let Some(output) = state.active_shell_command_output.as_mut() {
                output.clear();
            }

            // Find the last non-shell message to determine where current shell session started
            let mut last_non_shell_index = None;
            for (i, message) in state.messages.iter().enumerate().rev() {
                let is_shell_message = match &message.content {
                    MessageContent::Styled(line) => line
                        .spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                        .starts_with(SHELL_PROMPT_PREFIX),
                    MessageContent::Plain(text, _) => text.starts_with(SHELL_PROMPT_PREFIX),
                    MessageContent::PlainText(_) => true,
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
        InputEvent::ShellKill => {
            // Kill the running command if there is one
            if let Some(cmd) = &state.active_shell_command {
                if let Err(_e) = cmd.kill() {}
            }
            // Reset shell state
            state.active_shell_command = None;
            state.active_shell_command_output = None;
            state.waiting_for_shell_input = false;
            // Reset textarea shell mode
            state.text_area.set_shell_mode(false);
        }
        InputEvent::HandlePaste(text) => {
            handle_paste(state, text);
        }
        InputEvent::InputCursorStart => {
            state.text_area.move_cursor_to_beginning_of_line(false);
        }
        InputEvent::InputCursorEnd => {
            state.text_area.move_cursor_to_end_of_line(false);
        }
        InputEvent::InputDelete => {
            state.text_area.set_text("");
            state.show_helper_dropdown = false;
        }
        InputEvent::InputDeleteWord => {
            state.text_area.delete_backward_word();
            state.show_helper_dropdown = false;
        }
        InputEvent::InputCursorPrevWord => {
            state
                .text_area
                .set_cursor(state.text_area.beginning_of_previous_word());
        }
        InputEvent::InputCursorNextWord => {
            state
                .text_area
                .set_cursor(state.text_area.end_of_next_word());
        }
        InputEvent::RetryLastToolCall => {
            handle_retry_tool_call(state, input_tx, cancel_tx);
        }
        InputEvent::ToggleCollapsedMessages => {
            state.show_collapsed_messages = !state.show_collapsed_messages;
            if state.show_collapsed_messages {
                // Calculate scroll position to show the top of the last message
                let collapsed_messages: Vec<Message> = state
                    .messages
                    .iter()
                    .filter(|m| m.is_collapsed == Some(true))
                    .cloned()
                    .collect();

                if !collapsed_messages.is_empty() {
                    // Set selected to the last message
                    state.collapsed_messages_selected = collapsed_messages.len() - 1;

                    // Get all collapsed message lines once
                    let all_lines =
                        get_wrapped_collapsed_message_lines_cached(state, message_area_width);

                    // Calculate scroll to show the top of the last message
                    // For now, just scroll to the bottom to show the last message
                    let total_lines = all_lines.len();
                    let max_scroll = total_lines.saturating_sub(message_area_height);
                    state.collapsed_messages_scroll = max_scroll;
                } else {
                    state.collapsed_messages_scroll = 0;
                    state.collapsed_messages_selected = 0;
                }
            }
        }
        InputEvent::AttemptQuit => {
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

        _ => {}
    }
    adjust_scroll(state, message_area_height, message_area_width);
}

fn extract_command_from_tool_call(tool_call: &ToolCall) -> Result<String, String> {
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

fn handle_shell_mode(state: &mut AppState) {
    state.show_shell_mode = !state.show_shell_mode;
    // Update textarea shell mode
    state.text_area.set_shell_mode(state.show_shell_mode);

    if state.show_shell_mode {
        state.is_dialog_open = false;
        if let Some(dialog_command) = &state.dialog_command {
            let command = match extract_command_from_tool_call(dialog_command) {
                Ok(command) => command,
                Err(e) => {
                    eprintln!("Error extracting command: {}", e);
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
        if let Some(latest_tool_call) = &state.latest_tool_call {
            if let Some(dialog_command) = &state.dialog_command {
                if latest_tool_call.id != dialog_command.id {
                    state.is_dialog_open = true;
                }
            }
        }
        state.ondemand_shell_mode = false;
    }
}

fn handle_tab(state: &mut AppState) {
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
            state.text_area.set_text(selected_helper.command);
            state.show_helper_dropdown = false;
            state.filtered_helpers.clear();
            state.helper_selected = 0;
            return;
        }
        return;
    }
    // Trigger file file_search with Tab
    handle_tab_trigger(state);
}

fn handle_dropdown_up(state: &mut AppState) {
    if state.show_helper_dropdown && state.helper_selected > 0 {
        if state.file_search.is_active() {
            // File file_search mode
            state.helper_selected -= 1;
        } else {
            // Regular helper mode
            if !state.filtered_helpers.is_empty() && state.input().starts_with('/') {
                state.helper_selected -= 1;
            }
        }
    }
}

fn handle_dropdown_down(state: &mut AppState) {
    if state.show_helper_dropdown {
        if state.file_search.is_active() {
            // File file_search mode
            if state.helper_selected + 1 < state.file_search.filtered_count() {
                state.helper_selected += 1;
            }
        } else {
            // Regular helper mode
            if !state.filtered_helpers.is_empty()
                && state.input().starts_with('/')
                && state.helper_selected + 1 < state.filtered_helpers.len()
            {
                state.helper_selected += 1;
            }
        }
    }
}

fn handle_input_changed(state: &mut AppState, c: char) {
    if c == '?' && state.input().is_empty() && !state.is_dialog_open && !state.show_sessions_dialog
    {
        state.show_shortcuts = !state.show_shortcuts;
        return;
    }
    state.show_shortcuts = false;

    if c == '$' && (state.input().is_empty() || state.is_dialog_open) && !state.show_sessions_dialog
    {
        state.text_area.set_text("");
        handle_shell_mode(state);
        return;
    }

    state.text_area.insert_str(&c.to_string());

    // If a large paste placeholder is present and input is edited, only clear pasted state if placeholder is completely removed
    if let Some(placeholder) = &state.pasted_placeholder {
        if !state.input().contains(placeholder) {
            state.pasted_long_text = None;
            state.pasted_placeholder = None;
        }
    }

    if state.input().starts_with('/') {
        if state.file_search.is_active() {
            state.file_search.reset();
        }
        state.show_helper_dropdown = true;
    }

    if let Some(tx) = &state.file_search_tx {
        let _ = tx.try_send((state.input().to_string(), state.cursor_position()));
    }

    if state.input().is_empty() {
        state.show_helper_dropdown = false;
        state.filtered_helpers.clear();
        state.filtered_files.clear();
        state.helper_selected = 0;
        state.file_search.reset();
    }
}

fn handle_input_backspace(state: &mut AppState) {
    state.text_area.delete_backward(1);

    // If a large paste placeholder is present and input is edited, only clear pasted state if placeholder is completely removed
    if let Some(placeholder) = &state.pasted_placeholder {
        if !state.input().contains(placeholder) {
            state.pasted_long_text = None;
            state.pasted_placeholder = None;
        }
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
    }

    // Hide dropdown if input is empty
    if state.input().is_empty() {
        state.show_helper_dropdown = false;
        state.filtered_helpers.clear();
        state.filtered_files.clear();
        state.helper_selected = 0;
        state.file_search.reset();
    }
}

fn handle_esc(
    state: &mut AppState,
    output_tx: &Sender<OutputEvent>,
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
    input_tx: &Sender<InputEvent>,
    shell_tx: &Sender<InputEvent>,
) {
    let _ = input_tx.try_send(InputEvent::EmergencyClearTerminal);

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
            let _ = output_tx.try_send(OutputEvent::RejectTool(tool_call.clone()));
            let truncated_command = extract_truncated_command_arguments(tool_call);
            let title = get_command_type_name(tool_call);
            let rendered_lines = render_bash_block_rejected(&truncated_command, &title, None);
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
            let _ = shell_tx.try_send(InputEvent::ShellKill);
        }
        state.show_shell_mode = false;
        state.text_area.set_text("");
        if state.dialog_command.is_some() {
            state.is_dialog_open = true;
        }
    } else {
        state.text_area.set_text("");
    }
}

fn handle_input_submitted(
    state: &mut AppState,
    message_area_height: usize,
    output_tx: &Sender<OutputEvent>,
    input_tx: &Sender<InputEvent>,
    shell_tx: &Sender<InputEvent>,
) {
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
        state.messages.clear();
        render_system_message(state, &format!("Switching to session . {}", selected.title));
        state.show_sessions_dialog = false;
    } else if state.is_dialog_open {
        if let Some(dialog_command) = &state.dialog_command {
            let _ = output_tx.try_send(OutputEvent::AcceptTool(dialog_command.clone()));
        }
        state.is_dialog_open = false;
        state.dialog_selected = 0;
        state.dialog_command = None;
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
            let selected = &state.filtered_helpers[state.helper_selected];

            match selected.command {
                "/sessions" => {
                    let _ = output_tx.try_send(OutputEvent::ListSessions);
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                    return;
                }
                "/resume" => {
                    state.messages.clear();
                    state
                        .messages
                        .extend(welcome_messages(state.latest_version.clone(), state));
                    render_system_message(state, "Resuming last session.");

                    let _ = output_tx.try_send(OutputEvent::ResumeSession);

                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                    return;
                }

                "/clear" => {
                    push_clear_message(state);
                    return;
                }
                "/memorize" => {
                    push_memorize_message(state);
                    let _ = output_tx.try_send(OutputEvent::Memorize);
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                    return;
                }
                "/help" => {
                    push_help_message(state);
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                    return;
                }
                "/status" => {
                    push_status_message(state);
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                    return;
                }
                "/quit" => {
                    state.show_helper_dropdown = false;
                    state.text_area.set_text("");
                    let _ = input_tx.try_send(InputEvent::Quit);
                }
                "/toggle_auto_approve" => {
                    let input = "/toggle_auto_approve ".to_string();
                    state.text_area.set_text(&input);
                    state.text_area.set_cursor(input.len());
                    state.show_helper_dropdown = false;
                    return;
                }
                "/list_approved_tools" => {
                    list_auto_approved_tools(state);
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                    return;
                }
                "/mouse_capture" => {
                    // Toggle mouse capture using shared function
                    let _ = crate::toggle_mouse_capture(state);

                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                    return;
                }

                _ => {}
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
        {
            if final_input.contains(placeholder) {
                final_input = final_input.replace(placeholder, long_text);
                state.text_area.set_text(&final_input);
            }
        }
        state.pasted_long_text = None;
        state.pasted_placeholder = None;
        let _ = output_tx.try_send(OutputEvent::UserMessage(
            final_input.clone(),
            state.shell_tool_calls.clone(),
        ));

        let _ = input_tx.try_send(InputEvent::AddUserMessage(final_input));
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

fn handle_input_submitted_with(
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
    let total_lines = state.messages.len() * 2;
    let max_scroll = total_lines.saturating_sub(max_visible_lines);
    if was_at_bottom {
        state.scroll = max_scroll;
        state.scroll_to_bottom = true;
        state.stay_at_bottom = true;
    }
}

fn handle_stream_message(state: &mut AppState, id: Uuid, s: String, message_area_height: usize) {
    if let Some(message) = state.messages.iter_mut().find(|m| m.id == id) {
        state.is_streaming = true;
        if !state.loading {
            state.loading = true;
        }
        if let MessageContent::AssistantMD(text, _) = &mut message.content {
            text.push_str(&s);
        }
        crate::services::message::invalidate_message_lines_cache(state);

        // During streaming, only adjust scroll if we're staying at bottom
        if state.stay_at_bottom {
            let input_height = 3;
            let total_lines = state.messages.len() * 2;
            let max_visible_lines =
                std::cmp::max(1, message_area_height.saturating_sub(input_height));
            let max_scroll = total_lines.saturating_sub(max_visible_lines);
            state.scroll = max_scroll;
        }
    } else {
        let input_height = 3;
        let total_lines = state.messages.len() * 2;
        let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        let was_at_bottom = state.scroll == max_scroll;
        state
            .messages
            .push(Message::assistant(Some(id), s.clone(), None));
        state.text_area.set_text("");
        let total_lines = state.messages.len() * 2;
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        if was_at_bottom {
            state.scroll = max_scroll;
            state.scroll_to_bottom = true;
            state.stay_at_bottom = true;
        }
        state.is_streaming = false;
    }
}

fn handle_stream_tool_result(state: &mut AppState, progress: ToolCallResultProgress) {
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
    crate::services::message::invalidate_message_lines_cache(state);
}

fn handle_scroll_up(state: &mut AppState) {
    if state.show_collapsed_messages {
        if state.collapsed_messages_scroll >= SCROLL_LINES {
            state.collapsed_messages_scroll -= SCROLL_LINES;
        } else {
            state.collapsed_messages_scroll = 0;
        }
    } else if state.scroll >= SCROLL_LINES {
        state.scroll -= SCROLL_LINES;
        state.stay_at_bottom = false;
    } else {
        state.scroll = 0;
        state.stay_at_bottom = false;
    }
}

fn handle_scroll_down(state: &mut AppState, message_area_height: usize, message_area_width: usize) {
    if state.show_collapsed_messages {
        // For collapsed messages popup, we need to calculate scroll based on collapsed messages only
        let total_lines = if let Some((_, _, cached_lines)) = &state.collapsed_message_lines_cache {
            cached_lines.len()
        } else {
            // Fallback: calculate once and cache
            let all_lines = get_wrapped_collapsed_message_lines_cached(state, message_area_width);
            all_lines.len()
        };

        let max_scroll = total_lines.saturating_sub(message_area_height);
        if state.collapsed_messages_scroll + SCROLL_LINES < max_scroll {
            state.collapsed_messages_scroll += SCROLL_LINES;
        } else {
            state.collapsed_messages_scroll = max_scroll;
        }
    } else {
        // Use cached line count instead of recalculating every scroll
        let total_lines = if let Some((_, _, cached_lines)) = &state.message_lines_cache {
            cached_lines.len()
        } else {
            // Fallback: calculate once and cache
            let all_lines = get_wrapped_message_lines_cached(state, message_area_width);
            all_lines.len()
        };

        let max_scroll = total_lines.saturating_sub(message_area_height);
        if state.scroll + SCROLL_LINES < max_scroll {
            state.scroll += SCROLL_LINES;
            state.stay_at_bottom = false;
        } else {
            state.scroll = max_scroll;
            state.stay_at_bottom = true;
        }
    }
}

fn handle_page_up(state: &mut AppState, message_area_height: usize) {
    let input_height = 3;
    let page = std::cmp::max(1, message_area_height.saturating_sub(input_height));
    if state.scroll >= page {
        state.scroll -= page;
    } else {
        state.scroll = 0;
    }
}

fn handle_page_down(state: &mut AppState, message_area_height: usize, message_area_width: usize) {
    // Use cached line count instead of recalculating every page operation
    let total_lines = if let Some((_, _, cached_lines)) = &state.message_lines_cache {
        cached_lines.len()
    } else {
        // Fallback: calculate once and cache
        let all_lines = get_wrapped_message_lines_cached(state, message_area_width);
        all_lines.len()
    };

    let max_scroll = total_lines.saturating_sub(message_area_height);
    let page = std::cmp::max(1, message_area_height);
    if state.scroll < max_scroll {
        state.scroll = (state.scroll + page).min(max_scroll);
        if state.scroll == max_scroll {
            state.stay_at_bottom = true;
        }
    } else {
        state.stay_at_bottom = true;
    }
}

fn adjust_scroll(state: &mut AppState, message_area_height: usize, message_area_width: usize) {
    // Use cached line count instead of recalculating every adjustment
    let total_lines = if let Some((_, _, cached_lines)) = &state.message_lines_cache {
        cached_lines.len()
    } else {
        // Fallback: calculate once and cache
        let all_lines = get_wrapped_message_lines_cached(state, message_area_width);
        all_lines.len()
    };

    let max_scroll = total_lines.saturating_sub(message_area_height);
    if state.stay_at_bottom {
        state.scroll = max_scroll;
    } else if state.scroll_to_bottom {
        state.scroll = max_scroll;
        state.scroll_to_bottom = false;
    } else if state.scroll > max_scroll {
        state.scroll = max_scroll;
    }
}

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

pub fn shell_command_to_tool_call_result(state: &mut AppState) -> ToolCallResult {
    let id = if let Some(cmd) = &state.dialog_command {
        cmd.id.clone()
    } else {
        format!("tool_{}", Uuid::new_v4())
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
            name: "run_command".to_string(),
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

fn handle_retry_mechanism(state: &mut AppState) {
    if state.messages.len() >= 2 {
        state.messages.pop();
    }
}

fn handle_retry_tool_call(
    state: &mut AppState,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
) {
    let _ = input_tx.try_send(InputEvent::EmergencyClearTerminal);

    if let Some(cancel_tx) = cancel_tx {
        let _ = cancel_tx.send(());
    }

    if let Some(tool_call) = &state.latest_tool_call {
        // Extract the command from the tool call
        let command = match extract_command_from_tool_call(tool_call) {
            Ok(command) => command,
            Err(e) => {
                eprintln!("Error extracting command: {}", e);
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
        // Reset textarea shell mode
        state.text_area.set_shell_mode(false);
    }
}

// auto approve current tool
#[allow(dead_code)]
fn list_auto_approved_tools(state: &mut AppState) {
    // No dialog open - show current auto-approve settings and allow disabling
    let config = state.auto_approve_manager.get_config();
    let mut auto_approved_tools: Vec<_> = config
        .tools
        .iter()
        .filter(|(_, policy)| **policy == AutoApprovePolicy::Auto)
        .collect();

    // Filter by allowed_tools if configured
    if let Some(allowed_tools) = &state.allowed_tools {
        if !allowed_tools.is_empty() {
            auto_approved_tools.retain(|(tool_name, _)| allowed_tools.contains(tool_name));
        }
    }

    if auto_approved_tools.is_empty() {
        let message = if state
            .allowed_tools
            .as_ref()
            .is_some_and(|tools| !tools.is_empty())
        {
            "💡 No allowed tools are currently set to auto-approve."
        } else {
            "💡 No tools are currently set to auto-approve."
        };
        push_styled_message(state, message, Color::Cyan, "", Color::Cyan);
    } else {
        let tool_list = auto_approved_tools
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        // add a spacing marker
        state.messages.push(Message::plain_text(""));
        push_styled_message(
            state,
            &format!("🔓 Tools currently set to auto-approve: {}", tool_list),
            Color::Yellow,
            "",
            Color::Yellow,
        );
        // push_styled_message(
        //     state,
        //     "💡 To disable auto-approve for a tool, type: /disable_auto_approve <tool_name>",
        //     Color::Cyan,
        //     "",
        //     Color::Cyan,
        // );
    }
}

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
