use crate::app::{AppState, InputEvent, LoadingType, OutputEvent};
use crate::services::bash_block::{
    render_bash_block, render_bash_block_rejected, render_styled_block,
};
use crate::services::helper_block::{
    push_error_message, push_help_message, push_status_message, render_system_message,
};
use crate::services::message::{Message, MessageContent, get_wrapped_message_lines};
use ratatui::layout::Size;
use stakpak_shared::models::integrations::openai::ToolCallResultProgress;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

use super::message::{extract_full_command_arguments, extract_truncated_command_arguments};

pub fn update(
    state: &mut AppState,
    event: InputEvent,
    message_area_height: usize,
    message_area_width: usize,
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
            } else if state.show_helper_dropdown
                && !state.filtered_helpers.is_empty()
                && state.input.starts_with('/')
            {
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
            } else if state.show_helper_dropdown
                && !state.filtered_helpers.is_empty()
                && state.input.starts_with('/')
            {
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
            handle_input_submitted(state, message_area_height, output_tx, shell_tx);
        }
        InputEvent::InputChangedNewline => handle_input_changed(state, '\n'),
        InputEvent::InputSubmittedWith(s) => {
            handle_input_submitted_with(state, s, message_area_height)
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
        InputEvent::Quit => {}
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
            state.is_dialog_open = true;
            state.dialog_command = Some(tool_call.clone());
            let full_command = extract_full_command_arguments(&tool_call);
            let message_id =
                render_bash_block(&tool_call, &full_command, false, state, terminal_size);
            state.pending_bash_message_id = Some(message_id);
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
            state.messages.push(Message::info(line, None));
            adjust_scroll(state, message_area_height, message_area_width);
        }

        InputEvent::ShellError(line) => {
            push_error_message(state, &line);
            adjust_scroll(state, message_area_height, message_area_width);
        }

        InputEvent::ShellInputRequest(prompt) => {
            render_system_message(state, &format!("{}", prompt));
            state.waiting_for_shell_input = true;
            adjust_scroll(state, message_area_height, message_area_width);
        }

        InputEvent::ShellCompleted(code) => {
            let msg = if code == 0 {
                "✓ Command completed successfully"
            } else {
                &format!("✗ Command failed with exit code: {}", code)
            };
            render_system_message(state, &msg);
            state.active_shell_command = None;
            adjust_scroll(state, message_area_height, message_area_width);
        }
        _ => {}
    }
    adjust_scroll(state, message_area_height, message_area_width);
}

fn handle_tab(_state: &mut AppState) {}

fn handle_dropdown_up(state: &mut AppState) {
    if state.show_helper_dropdown
        && !state.filtered_helpers.is_empty()
        && state.input.starts_with('/')
        && state.helper_selected > 0
    {
        state.helper_selected -= 1;
    }
}

fn handle_dropdown_down(state: &mut AppState) {
    if state.show_helper_dropdown
        && !state.filtered_helpers.is_empty()
        && state.input.starts_with('/')
        && state.helper_selected + 1 < state.filtered_helpers.len()
    {
        state.helper_selected += 1;
    }
}

fn handle_input_changed(state: &mut AppState, c: char) {
    if c == '?' && state.input.is_empty() {
        state.show_shortcuts = !state.show_shortcuts;
        return;
    }

    let pos = state.cursor_position.min(state.input.len());
    state.input.insert(pos, c);
    state.cursor_position = pos + c.len_utf8();

    if state.input.starts_with('!') {
        state.input = "".to_string();
        state.cursor_position = 0;
        state.show_shell_mode = !state.show_shell_mode;
        state.show_helper_dropdown = false;
        return;
    }

    if state.input.starts_with('/') {
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
    } else {
        state.show_helper_dropdown = false;
        state.filtered_helpers.clear();
        state.helper_selected = 0;
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
    if state.input.starts_with('/') {
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
    } else {
        state.show_helper_dropdown = false;
        state.filtered_helpers.clear();
        state.helper_selected = 0;
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
    }

    state.input.clear();
    state.cursor_position = 0;
}

fn handle_input_submitted(
    state: &mut AppState,
    message_area_height: usize,
    output_tx: &Sender<OutputEvent>,
    shell_tx: &Sender<InputEvent>,
) {
    let input_height = 3;
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
    } else if state.show_helper_dropdown && !state.filtered_helpers.is_empty() {
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
                std::process::exit(0);
            }
            _ => {}
        }

        let total_lines = state.messages.len() * 2;
        let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        let was_at_bottom = state.scroll == max_scroll;
        state
            .messages
            .push(Message::user(format!("> {}", selected), None));
        state.input.clear();
        state.cursor_position = 0;
        state.show_helper_dropdown = false;
        state.helper_selected = 0;
        state.filtered_helpers = state.helpers.clone();
        let total_lines = state.messages.len() * 2;
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        if was_at_bottom {
            state.scroll = max_scroll;
            state.scroll_to_bottom = true;
            state.stay_at_bottom = true;
        }
        state.loading = true;
        state.spinner_frame = 0;
    } else if !state.input.trim().is_empty() && !state.input.trim().starts_with('/') {
        let total_lines = state.messages.len() * 2;
        let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        let was_at_bottom = state.scroll == max_scroll;
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

fn handle_input_submitted_with(state: &mut AppState, s: String, message_area_height: usize) {
    let input_height = 3;
    let total_lines = state.messages.len() * 2;
    let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
    let max_scroll = total_lines.saturating_sub(max_visible_lines);
    let was_at_bottom = state.scroll == max_scroll;
    state
        .messages
        .push(Message::assistant(None, s.clone(), None));
    state.input.clear();
    state.cursor_position = 0;
    let total_lines = state.messages.len() * 2;
    let max_scroll = total_lines.saturating_sub(max_visible_lines);
    if was_at_bottom {
        state.scroll = max_scroll;
        state.scroll_to_bottom = true;
        state.stay_at_bottom = true;
    }
    state.loading = false;
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
        state.loading = false;
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
    if state.scroll > 0 {
        state.scroll -= 1;
        state.stay_at_bottom = false;
    }
}

fn handle_scroll_down(state: &mut AppState, message_area_height: usize, message_area_width: usize) {
    let all_lines = get_wrapped_message_lines(&state.messages, message_area_width);
    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(message_area_height);
    if state.scroll < max_scroll {
        state.scroll += 1;
        if state.scroll == max_scroll {
            state.stay_at_bottom = true;
        }
    } else {
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
