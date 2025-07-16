use crate::app::{AppState, InputEvent, LoadingType, OutputEvent};
use crate::services::auto_complete::{handle_file_selection, handle_tab_trigger};
use crate::services::bash_block::{
    render_bash_block, render_bash_block_rejected, render_styled_block,
};
use crate::services::helper_block::{
    push_clear_message, push_error_message, push_help_message, push_memorize_message,
    push_status_message, push_styled_message, render_system_message,
};
use crate::services::message::{Message, MessageContent, get_wrapped_message_lines};
use crate::services::shell_mode::SHELL_PROMPT_PREFIX;
use ratatui::layout::Size;
use ratatui::style::{Color, Style};
use stakpak_shared::helper::truncate_output;
use stakpak_shared::models::integrations::openai::{
    FunctionCall, ToolCall, ToolCallResult, ToolCallResultProgress,
};
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

use super::message::{extract_full_command_arguments, extract_truncated_command_arguments};
use console::strip_ansi_codes;

#[allow(clippy::too_many_arguments)]
pub fn update(
    state: &mut AppState,
    event: InputEvent,
    message_area_height: usize,
    message_area_width: usize,
    input_tx: &Sender<InputEvent>,
    output_tx: &Sender<OutputEvent>,
    terminal_size: Size,
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
            } else {
                handle_scroll_down(state, message_area_height, message_area_width);
            }
        }
        InputEvent::DropdownUp => handle_dropdown_up(state),
        InputEvent::DropdownDown => handle_dropdown_down(state),
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
        InputEvent::StreamToolResult(progress) => {
            handle_stream_tool_result(state, progress, terminal_size)
        }
        InputEvent::ScrollUp => handle_scroll_up(state),
        InputEvent::ScrollDown => {
            handle_scroll_down(state, message_area_height, message_area_width)
        }
        InputEvent::PageUp => handle_page_up(state, message_area_height),
        InputEvent::PageDown => handle_page_down(state, message_area_height, message_area_width),
        InputEvent::CursorLeft => {
            if state.cursor_position > 0 {
                let prev = state.input[..state.cursor_position]
                    .chars()
                    .next_back()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
                state.cursor_position -= prev;
            }
        }
        InputEvent::CursorRight => {
            if state.cursor_position < state.input.len() {
                let next = state.input[state.cursor_position..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
                state.cursor_position += next;
            }
        }
        InputEvent::ToggleCursorVisible => state.cursor_visible = !state.cursor_visible,
        InputEvent::ShowConfirmationDialog(tool_call) => {
            state.dialog_command = Some(tool_call.clone());
            let full_command = extract_full_command_arguments(&tool_call);
            let message_id =
                render_bash_block(&tool_call, &full_command, false, state, terminal_size);
            state.pending_bash_message_id = Some(message_id);
            state.is_dialog_open = true;
        }
        InputEvent::Loading(is_loading) => {
            state.loading = is_loading;
        }
        InputEvent::HandleEsc => handle_esc(state, output_tx),

        InputEvent::GetStatus(account_info) => {
            state.account_info = account_info;
        }
        InputEvent::Tab => handle_tab(state),
        InputEvent::SetSessions(sessions) => {
            state.sessions = sessions;
            state.loading = false;
            state.spinner_frame = 0;
            state.loading_type = LoadingType::Llm;
            state.show_sessions_dialog = true;
        }
        InputEvent::ShellOutput(line) => {
            let mut redacted_line = state.secret_manager.redact_and_store_secrets(&line, None);

            if let Some(output) = state.active_shell_command_output.as_mut() {
                let text = format!("{}\n", redacted_line);
                output.push_str(&text);
                *output = truncate_output(output);
            }

            redacted_line = truncate_output(&redacted_line);

            state.messages.push(Message::plain_text(redacted_line));

            adjust_scroll(state, message_area_height, message_area_width);
        }

        InputEvent::ShellError(line) => {
            push_error_message(state, &line);
            adjust_scroll(state, message_area_height, message_area_width);
        }

        InputEvent::ShellInputRequest(prompt) => {
            push_styled_message(
                state,
                &prompt,
                Color::Rgb(180, 180, 180),
                "?! ",
                Color::Yellow,
            );
            state.waiting_for_shell_input = true;
            adjust_scroll(state, message_area_height, message_area_width);
        }

        InputEvent::ShellCompleted(_code) => {
            if state.dialog_command.is_some() {
                let result = shell_command_to_tool_call_result(state);
                let _ = output_tx.try_send(OutputEvent::SendToolResult(result));
                state.show_shell_mode = false;
                state.dialog_command = None;
            }
            if state.ondemand_shell_mode {
                let new_tool_call_result = shell_command_to_tool_call_result(state);
                if let Some(ref mut tool_calls) = state.shell_tool_calls {
                    tool_calls.push(new_tool_call_result);
                }
            }

            if !state.ondemand_shell_mode {
                state.active_shell_command = None;
                state.active_shell_command_output = None;
            }

            state.input.clear();
            state.cursor_position = 0;
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
        InputEvent::HandlePaste(text) => {
            let text = strip_ansi_codes(&text);
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            let line_count = text.lines().count();
            if line_count > 10 {
                state.pasted_long_text = Some(text.clone());
                let placeholder = format!("[Pasted text of {} lines]", line_count);
                state.pasted_placeholder = Some(placeholder.clone());
                // Insert the placeholder at the current cursor position
                let pos = state.cursor_position.min(state.input.len());
                state.input.insert_str(pos, &placeholder);
                state.cursor_position = pos + placeholder.len();
            } else {
                // Normal paste
                state.input.insert_str(state.cursor_position, &text);
                state.cursor_position += text.len();
                state.pasted_long_text = None;
                state.pasted_placeholder = None;
            }
        }
        InputEvent::InputCursorStart => {
            state.cursor_position = 0;
        }
        InputEvent::InputCursorEnd => {
            state.cursor_position = state.input.len();
        }
        InputEvent::InputDelete => {
            state.input.clear();
            state.cursor_position = 0;
        }
        InputEvent::InputDeleteWord => {
            if state.cursor_position > 0 {
                let start = state.input[..state.cursor_position]
                    .trim_end()
                    .rfind(char::is_whitespace)
                    .map_or(0, |i| i + 1);
                state.input.drain(start..state.cursor_position);
                state.cursor_position = start;
            }
        }
        InputEvent::InputCursorPrevWord => {
            let mut pos = state.cursor_position;
            // Skip any whitespace before the cursor
            while pos > 0 {
                let ch = state.input[..pos].chars().next_back();
                if let Some(c) = ch {
                    if c.is_whitespace() {
                        pos -= c.len_utf8();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            // Skip the previous word
            while pos > 0 {
                let ch = state.input[..pos].chars().next_back();
                if let Some(c) = ch {
                    if !c.is_whitespace() {
                        pos -= c.len_utf8();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            state.cursor_position = pos;
        }
        InputEvent::InputCursorNextWord => {
            let mut pos = state.cursor_position;
            // Skip current word forwards (if we're in the middle of a word)
            while pos < state.input.len() && !state.input.as_bytes()[pos].is_ascii_whitespace() {
                pos += 1;
            }
            // Skip whitespace forwards
            while pos < state.input.len() && state.input.as_bytes()[pos].is_ascii_whitespace() {
                pos += 1;
            }
            state.cursor_position = pos;
        }
        InputEvent::AttemptQuit => {
            use std::time::Instant;
            let now = Instant::now();
            if !state.ctrl_c_pressed_once
                || state.ctrl_c_timer.is_none()
                || state.ctrl_c_timer.map(|t| now > t).unwrap_or(true)
            {
                // First press or timer expired: clear input, move cursor, set timer
                state.input.clear();
                state.cursor_position = 0;
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
    state.poll_autocomplete_results();
}

fn handle_shell_mode(state: &mut AppState) {
    state.show_shell_mode = !state.show_shell_mode;
    if state.show_shell_mode {
        state.is_dialog_open = false;
        state.ondemand_shell_mode = state.dialog_command.is_none();
        if state.ondemand_shell_mode && state.shell_tool_calls.is_none() {
            state.shell_tool_calls = Some(Vec::new());
        }
    }
    if !state.show_shell_mode && state.dialog_command.is_some() {
        state.is_dialog_open = true;
        state.ondemand_shell_mode = false;
    }
    state.input.clear();
    state.cursor_position = 0;
}

fn handle_tab(state: &mut AppState) {
    // Check if we're already in helper dropdown mode
    if state.show_helper_dropdown {
        // If in file autocomplete mode, handle file selection
        if state.autocomplete.is_active() {
            let selected_file = state
                .autocomplete
                .get_file_at_index(state.helper_selected)
                .map(|s| s.to_string());
            if let Some(selected_file) = selected_file {
                handle_file_selection(state, &selected_file);
            }
            return;
        }
        // Handle regular helper selection (existing behavior)
        return;
    }
    // Trigger file autocomplete with Tab
    handle_tab_trigger(state);
}

fn handle_dropdown_up(state: &mut AppState) {
    if state.show_helper_dropdown && state.helper_selected > 0 {
        if state.autocomplete.is_active() {
            // File autocomplete mode
            state.helper_selected -= 1;
        } else {
            // Regular helper mode
            if !state.filtered_helpers.is_empty() && state.input.starts_with('/') {
                state.helper_selected -= 1;
            }
        }
    }
}

fn handle_dropdown_down(state: &mut AppState) {
    if state.show_helper_dropdown {
        if state.autocomplete.is_active() {
            // File autocomplete mode
            if state.helper_selected + 1 < state.autocomplete.filtered_count() {
                state.helper_selected += 1;
            }
        } else {
            // Regular helper mode
            if !state.filtered_helpers.is_empty()
                && state.input.starts_with('/')
                && state.helper_selected + 1 < state.filtered_helpers.len()
            {
                state.helper_selected += 1;
            }
        }
    }
}

fn handle_input_changed(state: &mut AppState, c: char) {
    if c == '?' && state.input.is_empty() {
        state.show_shortcuts = !state.show_shortcuts;
        return;
    }
    if c == '$' && (state.input.is_empty() || state.is_dialog_open) {
        state.input.clear();
        handle_shell_mode(state);
        return;
    }

    let pos = state.cursor_position.min(state.input.len());
    state.input.insert(pos, c);
    state.cursor_position = pos + c.len_utf8();

    // If a large paste placeholder is present and input is edited, only clear pasted state if placeholder is completely removed
    if let Some(placeholder) = &state.pasted_placeholder {
        if !state.input.contains(placeholder) {
            state.pasted_long_text = None;
            state.pasted_placeholder = None;
        }
    }

    if state.input.starts_with('/') {
        if state.autocomplete.is_active() {
            state.autocomplete.reset();
        }

        state.show_helper_dropdown = true;
        state.filtered_helpers = state
            .helpers
            .iter()
            .filter(|h| h.starts_with(&state.input))
            .cloned()
            .collect();
        if state.filtered_helpers.is_empty()
            || state.helper_selected >= state.filtered_helpers.len()
        {
            state.helper_selected = 0;
        }
    }
    // Send input to autocomplete worker (async, non-blocking)
    if let Some(tx) = &state.autocomplete_tx {
        let _ = tx.try_send((state.input.clone(), state.cursor_position));
    }

    if state.input.is_empty() {
        state.show_helper_dropdown = false;
        state.filtered_helpers.clear();
        state.filtered_files.clear();
        state.helper_selected = 0;
        state.autocomplete.reset();
    }
}

fn handle_input_backspace(state: &mut AppState) {
    if state.cursor_position > 0 && !state.input.is_empty() {
        let pos = state.cursor_position;
        let prev = state.input[..pos]
            .chars()
            .next_back()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        let remove_at = pos - prev;
        state.input.drain(remove_at..pos);
        state.cursor_position = remove_at;
    }

    // If a large paste placeholder is present and input is edited, only clear pasted state if placeholder is completely removed
    if let Some(placeholder) = &state.pasted_placeholder {
        if !state.input.contains(placeholder) {
            state.pasted_long_text = None;
            state.pasted_placeholder = None;
        }
    }

    // Send input to autocomplete worker (async, non-blocking)
    if let Some(tx) = &state.autocomplete_tx {
        let _ = tx.try_send((state.input.clone(), state.cursor_position));
    }
    // Hide dropdown if input is empty
    if state.input.is_empty() {
        state.show_helper_dropdown = false;
        state.filtered_helpers.clear();
        state.filtered_files.clear();
        state.helper_selected = 0;
        state.autocomplete.reset();
    }
}

fn handle_esc(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    if state.show_sessions_dialog {
        state.show_sessions_dialog = false;
    } else if state.show_helper_dropdown {
        state.show_helper_dropdown = false;
    } else if state.is_dialog_open {
        let tool_call_opt = state.dialog_command.clone();
        if let Some(tool_call) = &tool_call_opt {
            let _ = output_tx.try_send(OutputEvent::RejectTool(tool_call.clone()));
            let truncated_command = extract_truncated_command_arguments(tool_call);
            render_bash_block_rejected(&truncated_command, state);
        }
        state.is_dialog_open = false;
        state.dialog_command = None;
        state.input.clear();
        state.cursor_position = 0;
    } else if state.show_shell_mode {
        state.show_shell_mode = false;
        state.input.clear();
        state.cursor_position = 0;
        if state.dialog_command.is_some() {
            state.is_dialog_open = true;
        }
    } else {
        state.input.clear();
        state.cursor_position = 0;
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
        // Check if we're waiting for shell input (like password)
        if state.waiting_for_shell_input {
            let input = state.input.clone();
            state.input.clear();
            state.cursor_position = 0;
            state.waiting_for_shell_input = false;

            // Send the password to the shell command
            if let Some(cmd) = &state.active_shell_command {
                let stdin_tx = cmd.stdin_tx.clone();
                tokio::spawn(async move {
                    let _ = stdin_tx.send(input).await;
                });
            }
            return;
        }

        // Otherwise, it's a new shell command
        if !state.input.trim().is_empty() {
            let command = state.input.clone();
            state.input.clear();
            state.cursor_position = 0;
            state.show_helper_dropdown = false;

            // Run the shell command with the shell event channel
            state.run_shell_command(command, shell_tx);
        }
        return;
    }

    if state.input.trim() == "clear" {
        push_clear_message(state);
        return;
    }

    if state.show_sessions_dialog {
        let selected = &state.sessions[state.session_selected];
        let _ = output_tx.try_send(OutputEvent::SwitchToSession(selected.id.to_string()));
        state.messages.clear();
        render_system_message(state, &format!("Switching to session . {}", selected.title));
        state.show_sessions_dialog = false;
    } else if state.is_dialog_open {
        state.is_dialog_open = false;
        state.input.clear();
        state.cursor_position = 0;
        if state.dialog_selected == 0 {
            if let Some(tool_call) = &state.dialog_command {
                let _ = output_tx.try_send(OutputEvent::AcceptTool(tool_call.clone()));
            }
        } else {
            // Clone dialog_command before mutating state
            let tool_call_opt = state.dialog_command.clone();
            if let Some(tool_call) = &tool_call_opt {
                let truncated_command = extract_truncated_command_arguments(tool_call);
                render_bash_block_rejected(&truncated_command, state);
            }
        }

        state.dialog_command = None;
    } else if state.show_helper_dropdown {
        if state.autocomplete.is_active() {
            let selected_file = state
                .autocomplete
                .get_file_at_index(state.helper_selected)
                .map(|s| s.to_string());
            if let Some(selected_file) = selected_file {
                handle_file_selection(state, &selected_file);
            }
            return;
        }
        if !state.filtered_helpers.is_empty() {
            let selected = state.filtered_helpers[state.helper_selected];

            match selected {
                "/sessions" => {
                    state.loading_type = LoadingType::Sessions;
                    state.loading = true;
                    let _ = output_tx.try_send(OutputEvent::ListSessions);
                    state.input.clear();
                    state.cursor_position = 0;
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
                    state.input.clear();
                    state.cursor_position = 0;
                    state.show_helper_dropdown = false;
                    return;
                }
                "/help" => {
                    push_help_message(state);
                    state.input.clear();
                    state.cursor_position = 0;
                    state.show_helper_dropdown = false;
                    return;
                }
                "/status" => {
                    push_status_message(state);
                    state.input.clear();
                    state.cursor_position = 0;
                    state.show_helper_dropdown = false;
                    return;
                }
                "/quit" => {
                    state.show_helper_dropdown = false;
                    state.input.clear();
                    state.cursor_position = 0;
                    let _ = input_tx.try_send(InputEvent::Quit);
                }
                _ => {}
            }
        }
    } else if !state.input.trim().is_empty() {
        // PERFORMANCE FIX: Simplified condition for submission
        // Allow submission of any non-empty input that's not a recognized helper command
        let input_height = 3;
        let total_lines = state.messages.len() * 2;
        let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        let was_at_bottom = state.scroll == max_scroll;

        if let (Some(placeholder), Some(long_text)) =
            (&state.pasted_placeholder, &state.pasted_long_text)
        {
            if state.input.contains(placeholder) {
                let replaced = state.input.replace(placeholder, long_text);
                state.input = replaced;
            }
        }
        state.pasted_long_text = None;
        state.pasted_placeholder = None;
        let _ = output_tx.try_send(OutputEvent::UserMessage(
            state.input.clone(),
            state.shell_tool_calls.clone(),
        ));
        state
            .messages
            .push(Message::user(format!("> {}", state.input), None));
        state.input.clear();
        state.cursor_position = 0;
        let total_lines = state.messages.len() * 2;
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        if was_at_bottom {
            state.scroll = max_scroll;
            state.scroll_to_bottom = true;
            state.stay_at_bottom = true;
        }
        state.loading = true;
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
    state.messages.push(Message::assistant(
        None,
        s.clone(),
        color.map(|c| Style::default().fg(c)),
    ));
    state.input.clear();
    state.cursor_position = 0;
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
        if let MessageContent::Plain(text, _) = &mut message.content {
            text.push_str(&s);
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
        state.input.clear();
        state.cursor_position = 0;
        let total_lines = state.messages.len() * 2;
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        if was_at_bottom {
            state.scroll = max_scroll;
            state.scroll_to_bottom = true;
            state.stay_at_bottom = true;
        }
    }
}

fn handle_stream_tool_result(
    state: &mut AppState,
    progress: ToolCallResultProgress,
    terminal_size: Size,
) {
    let tool_call_id = progress.id;
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

    // 4. Re-render the styled block with the full buffer
    render_styled_block(
        &buffer_content,
        "Tool Streaming",
        "Result",
        None,
        state,
        terminal_size,
        "Streaming",
        Some(tool_call_id),
    );
}

fn handle_scroll_up(state: &mut AppState) {
    let lines = 15;
    if state.scroll >= lines {
        state.scroll -= lines;
        state.stay_at_bottom = false;
    } else {
        state.scroll = 0;
        state.stay_at_bottom = false;
    }
}

fn handle_scroll_down(state: &mut AppState, message_area_height: usize, message_area_width: usize) {
    let lines = 15;
    let all_lines = get_wrapped_message_lines(&state.messages, message_area_width);
    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(message_area_height);
    if state.scroll + lines < max_scroll {
        state.scroll += lines;
        state.stay_at_bottom = false;
    } else {
        state.scroll = max_scroll;
        state.stay_at_bottom = true;
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
    let all_lines = get_wrapped_message_lines(&state.messages, message_area_width);
    let total_lines = all_lines.len();
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
    let all_lines = get_wrapped_message_lines(&state.messages, message_area_width);
    let total_lines = all_lines.len();
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

pub fn clear_streaming_tool_results(state: &mut AppState) {
    state.streaming_tool_results.clear();
    state
        .messages
        .retain(|m| m.id != state.streaming_tool_result_id.unwrap_or_default());
    state.streaming_tool_result_id = None;
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
    }
}
