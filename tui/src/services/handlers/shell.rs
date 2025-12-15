//! Shell Mode Event Handlers
//!
//! Handles all shell mode-related events including shell output, errors, completion, and shell mode toggling.

use super::navigation::adjust_scroll;
use crate::app::InputEvent;
use crate::app::{AppState, OutputEvent, ToolCallStatus};
use crate::services::bash_block::preprocess_terminal_output;
use crate::services::detect_term::AdaptiveColors;
use crate::services::helper_block::push_error_message;
use crate::services::message::{BubbleColors, Message, MessageContent};
#[cfg(not(unix))]
use crate::services::shell_mode::run_background_shell_command;
#[cfg(unix)]
use crate::services::shell_mode::run_pty_command;
use crate::services::shell_mode::{SHELL_PROMPT_PREFIX, ShellEvent};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use stakpak_shared::helper::truncate_output;
use stakpak_shared::models::integrations::openai::{
    FunctionCall, ToolCall, ToolCallResult, ToolCallResultStatus,
};
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

// Helper to convert vt100 color to ratatui color
fn convert_vt100_color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Capture styled screen content at scroll position 0 (which is safe).
/// Returns styled Lines for the current visible screen.
pub fn capture_styled_screen(parser: &mut vt100::Parser) -> Vec<Line<'static>> {
    // Always capture at scroll position 0 (safe)
    parser.set_scrollback(0);

    let (rows, cols) = parser.screen().size();
    let mut lines = Vec::new();

    for row in 0..rows {
        lines.push(row_to_line(parser.screen(), row, cols));
    }

    // Trim trailing empty lines
    while let Some(last_line) = lines.last() {
        let is_empty = last_line.spans.iter().all(|s| s.content.trim().is_empty());
        if is_empty {
            lines.pop();
        } else {
            break;
        }
    }

    lines
}

/// Helper to convert a single row to a Line
fn row_to_line(screen: &vt100::Screen, row: u16, cols: u16) -> Line<'static> {
    let mut current_line = Vec::new();
    let mut current_text = String::new();
    let mut current_style = Style::default();

    for col in 0..cols {
        if let Some(cell) = screen.cell(row, col) {
            let fg = convert_vt100_color(cell.fgcolor());
            let bg = convert_vt100_color(cell.bgcolor());
            let mut style = Style::default();
            if fg != Color::Reset {
                style = style.fg(fg);
            }
            if bg != Color::Reset {
                style = style.bg(bg);
            }
            if cell.bold() {
                style = style.add_modifier(ratatui::style::Modifier::BOLD);
            }
            if cell.italic() {
                style = style.add_modifier(ratatui::style::Modifier::ITALIC);
            }
            if cell.inverse() {
                style = style.add_modifier(ratatui::style::Modifier::REVERSED);
            }
            if cell.underline() {
                style = style.add_modifier(ratatui::style::Modifier::UNDERLINED);
            }

            if style != current_style {
                if !current_text.is_empty() {
                    current_line.push(Span::styled(current_text.clone(), current_style));
                    current_text.clear();
                }
                current_style = style;
            }

            current_text.push_str(&cell.contents());
        } else {
            if !current_text.is_empty() {
                current_line.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }
            current_style = Style::default();
            current_text.push(' ');
        }
    }
    if !current_text.is_empty() {
        current_line.push(Span::styled(current_text, current_style));
    }
    Line::from(current_line)
}

pub fn send_shell_input(state: &mut AppState, data: &str) {
    if let Some(cmd) = &state.active_shell_command {
        // Mark that user has interacted with the shell
        if !data.is_empty() {
            state.shell_interaction_occurred = true;
        }

        let tx = cmd.stdin_tx.clone();
        let data = data.to_string();
        tokio::spawn(async move {
            let _ = tx.send(data).await;
        });
    }
}

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

/// Handle run shell command event
pub fn handle_run_shell_command(
    state: &mut AppState,
    command: String,
    input_tx: &Sender<InputEvent>,
) {
    let (shell_tx, mut shell_rx) = mpsc::channel::<ShellEvent>(100);

    // Query terminal size directly to ensure we have the correct dimensions
    let (term_cols, term_rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let rows = term_rows.saturating_sub(2).max(1);
    let cols = term_cols.saturating_sub(4).max(1);

    #[cfg(unix)]
    let shell_cmd = match run_pty_command(command.clone(), shell_tx, rows, cols) {
        Ok(cmd) => cmd,
        Err(e) => {
            push_error_message(state, &format!("Failed to run command: {}", e), None);
            return;
        }
    };

    #[cfg(not(unix))]
    let shell_cmd = run_background_shell_command(command.clone(), shell_tx);

    state.active_shell_command = Some(shell_cmd.clone());
    state.active_shell_command_output = Some(String::new());

    // Create a new vt100 parser for the session with 1000 lines of scrollback
    state.shell_screen = vt100::Parser::new(rows, cols, 1000);
    // Reset interaction flag for new command
    state.shell_interaction_occurred = false;

    let input_tx = input_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = shell_rx.recv().await {
            match event {
                ShellEvent::Output(line) => {
                    let _ = input_tx.send(InputEvent::ShellOutput(line)).await;
                }
                ShellEvent::Error(line) => {
                    let _ = input_tx.send(InputEvent::ShellError(line)).await;
                }

                ShellEvent::Completed(code) => {
                    let _ = input_tx.send(InputEvent::ShellCompleted(code)).await;
                    break;
                }
                ShellEvent::Clear => {
                    let _ = input_tx.send(InputEvent::ShellClear).await;
                }
            }
        }
    });

    state.show_shell_mode = true;
    state.text_area.set_shell_mode(true);
}

// In handle_shell_mode (shell.rs):

pub fn handle_shell_mode(state: &mut AppState, input_tx: &Sender<InputEvent>) {
    // If we are currently showing shell mode, we are toggling it OFF.
    // Requirement: Background the session when leaving, unless terminated.
    if state.show_shell_mode {
        state.show_shell_mode = false;
        // Update textarea shell mode
        state.text_area.set_shell_mode(false);

        let command_name = state
            .active_shell_command
            .as_ref()
            .map(|c| c.command.clone())
            .unwrap_or_else(|| "sh".to_string());

        // Update the message in chat to reflect background status
        // Use the accumulated history for the background view
        let fresh_lines = state.shell_history_lines.clone();
        if let Some(id) = state.interactive_shell_message_id {
            for msg in &mut state.messages {
                if msg.id == id {
                    let new_colors = BubbleColors {
                        border_color: Color::DarkGray,
                        title_color: Color::DarkGray,
                        content_color: AdaptiveColors::text(),
                        tool_type: "Interactive Bash".to_string(),
                    };
                    msg.content = MessageContent::RenderRefreshedTerminal(
                        format!(
                            "Shell Command {} (Background - Ctrl+Y to resume)",
                            command_name
                        ),
                        fresh_lines.clone(),
                        Some(new_colors),
                        state.terminal_size.width as usize,
                    );
                    break;
                }
            }
        }

        // Clear the text area but DO NOT kill the shell (Persistence)
        state.text_area.set_text("");

        // Handle dialog logic if needed (restore dialog state)
        if state.dialog_command.is_some() {
            if let Some(latest_tool_call) = &state.latest_tool_call
                && let Some(dialog_command) = &state.dialog_command
                && latest_tool_call.id != dialog_command.id
            {
                state.is_dialog_open = true;
            }
            state.ondemand_shell_mode = false;
        }
        return;
    }

    // If we are currently NOT showing shell mode, we are toggling it ON.
    // Check if we have an active session to resume
    if state.active_shell_command.is_some() {
        state.show_shell_mode = true;
        state.text_area.set_shell_mode(true);
        // Message title update will happen in next handle_shell_output or we can force it here
        if state.interactive_shell_message_id.is_some() {
            // Find message and update title to Focused
            // (Optional optimization: let handle_shell_output do it on next frame)
        }
        return;
    }

    // Start a new shell if none exists
    let shell = std::env::var("SHELL").unwrap_or("sh".to_string());
    let _ = input_tx.try_send(InputEvent::RunShellCommand(shell));
}

// Helper to fully terminate the session (called when user sends message)
pub fn terminate_active_shell_session(state: &mut AppState) {
    if state.active_shell_command.is_some() {
        let command_name = state
            .active_shell_command
            .as_ref()
            .map(|c| c.command.clone())
            .unwrap_or_else(|| "sh".to_string());

        // Update the message in chat to reflect termination
        if let Some(id) = state.interactive_shell_message_id {
            for msg in &mut state.messages {
                if msg.id == id {
                    if let MessageContent::RenderRefreshedTerminal(_, lines, _, width) =
                        &msg.content
                    {
                        let new_colors = BubbleColors {
                            border_color: Color::DarkGray,
                            title_color: Color::DarkGray,
                            content_color: AdaptiveColors::text(),
                            tool_type: "Interactive Bash (Terminated)".to_string(),
                        };
                        msg.content = MessageContent::RenderRefreshedTerminal(
                            format!("Shell Command {} (Terminated)", command_name),
                            lines.clone(),
                            Some(new_colors),
                            *width,
                        );
                    }
                    break;
                }
            }
        }

        // Now kill it
        handle_shell_kill(state);
    }
}

/// Handle shell output event
pub fn handle_shell_output(state: &mut AppState, raw_data: String) {
    // Guard: If shell was terminated, ignore any pending output
    if state.active_shell_command.is_none() {
        return;
    }

    // 1. Append to raw output log (truncated)
    if let Some(output) = state.active_shell_command_output.as_mut() {
        output.push_str(&raw_data);
        *output = truncate_output(output);
    }

    // Process raw output into Virtual Terminal Screen
    state.shell_screen.process(raw_data.as_bytes());

    // 3. Determine Styling based on Focus
    let command_name = state
        .active_shell_command
        .as_ref()
        .map(|c| c.command.clone())
        .unwrap_or_else(|| "sh".to_string());

    let (colors, title) = if state.show_shell_mode {
        (
            BubbleColors {
                border_color: Color::Cyan, // Using Cyan for "Cool" look
                title_color: Color::Cyan,
                content_color: AdaptiveColors::text(),
                tool_type: "Interactive Bash".to_string(),
            },
            format!("Shell Command {} [Focused]", command_name),
        )
    } else {
        (
            BubbleColors {
                border_color: Color::DarkGray,
                title_color: Color::DarkGray,
                content_color: AdaptiveColors::text(),
                tool_type: "Interactive Bash".to_string(),
            },
            format!(
                "Shell Command {} (Background - Ctrl+Y to focus)",
                command_name
            ),
        )
    };

    // 4. Capture styled screen content at scroll=0 (safe)
    let screen_lines = capture_styled_screen(&mut state.shell_screen);

    // Smart history accumulation:
    // vt100 tracks scrollback count - that tells us how many lines have scrolled off
    // We can use this to know when to preserve older content
    let (term_rows, _) = state.shell_screen.screen().size();

    // Probe actual scrollback size
    state.shell_screen.set_scrollback(usize::MAX);
    let scrollback_count = state.shell_screen.screen().scrollback();
    state.shell_screen.set_scrollback(0); // Reset to normal view

    // Calculate expected total lines (scrollback + visible)
    let expected_total = scrollback_count + term_rows as usize;

    // If we have more expected lines than current history, we need to grow
    // But we can only safely capture the visible screen, so we track growth
    if state.shell_history_lines.is_empty() {
        // First capture - just set it
        state.shell_history_lines = screen_lines.clone();
    } else if !screen_lines.is_empty() {
        // Check if content has grown beyond what we have
        let current_history_len = state.shell_history_lines.len();

        if expected_total > current_history_len {
            // Content has grown - we need to shift and add new lines
            // The new lines are the difference between expected and current
            let lines_to_add = expected_total - current_history_len;

            // Keep history but limit it to prevent memory bloat
            const MAX_HISTORY: usize = 5000;

            // Append the current visible lines minus overlap
            // The last (term_rows - lines_to_add) lines of history should overlap with
            // the first (term_rows - lines_to_add) lines of screen
            if lines_to_add < screen_lines.len() {
                // Take only the NEW lines from screen (the bottom portion)
                let new_lines_start = screen_lines.len() - lines_to_add;
                for line in screen_lines[new_lines_start..].iter() {
                    state.shell_history_lines.push(line.clone());
                }
            } else {
                // All lines are new (e.g. huge output dump)
                state
                    .shell_history_lines
                    .extend(screen_lines.iter().cloned());
            }

            // Trim history if too large
            if state.shell_history_lines.len() > MAX_HISTORY {
                let trim_amount = state.shell_history_lines.len() - MAX_HISTORY;
                state.shell_history_lines.drain(0..trim_amount);
            }
        } else {
            // No scrolling happened, just update the visible portion
            // Replace the last term_rows lines with current screen
            let history_len = state.shell_history_lines.len();
            let replace_start = history_len.saturating_sub(term_rows as usize);
            state.shell_history_lines.truncate(replace_start);
            state
                .shell_history_lines
                .extend(screen_lines.iter().cloned());
        }
    }

    // 5. Update UI
    // Ensure we have a target message ID for the interactive shell
    let target_id = if let Some(id) = state.interactive_shell_message_id {
        Some(id)
    } else {
        // Create new message if none exists
        let new_id = Uuid::new_v4();
        state.interactive_shell_message_id = Some(new_id);

        let new_message = Message {
            id: new_id,
            content: MessageContent::RenderRefreshedTerminal(
                title.clone(),
                screen_lines.clone(),
                Some(colors.clone()),
                state.terminal_size.width as usize,
            ),
            is_collapsed: None,
        };
        state.messages.push(new_message);
        None // Already pushed
    };

    if let Some(id) = target_id
        && let Some(msg) = state.messages.iter_mut().find(|m| m.id == id)
    {
        msg.content = MessageContent::RenderRefreshedTerminal(
            title,
            screen_lines,
            Some(colors),
            state.terminal_size.width as usize,
        );
    }
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
    state.interactive_shell_message_id = None;
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
    state.interactive_shell_message_id = None;
    state.waiting_for_shell_input = false;
    // Reset textarea shell mode
    state.text_area.set_shell_mode(false);
}

/// Convert shell command to tool call result
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
        result: String::from("Interactive shell exited"),
        status: ToolCallResultStatus::Success,
    }
}
