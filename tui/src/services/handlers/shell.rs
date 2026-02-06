//! Shell Mode Event Handlers
//!
//! Handles all shell mode-related events including shell output, errors, completion, and shell mode toggling.

use super::navigation::adjust_scroll;
use crate::app::InputEvent;
use crate::app::{AppState, OutputEvent, ToolCallStatus};
use crate::services::bash_block::preprocess_terminal_output;
use crate::services::detect_term::AdaptiveColors;
use crate::services::helper_block::push_error_message;
use crate::services::message::{
    BubbleColors, Message, MessageContent, invalidate_message_lines_cache,
};
use crate::services::shell_mode::run_pty_command;
use crate::services::shell_mode::{SHELL_PROMPT_PREFIX, ShellEvent};
use ratatui::style::{Color, Modifier, Style};
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

/// Check if a line appears to be a shell prompt (ends with common prompt characters)
/// This is used to detect when a command has completed and the shell is ready for more input
fn looks_like_prompt(line: &str) -> bool {
    let trimmed = line.trim_end();
    if trimmed.is_empty() {
        return false;
    }

    // Common prompt ending characters
    let prompt_endings = ['$', '%', '>', '#', '✗', '❯', '➜', '»'];

    // Check if line ends with a prompt character (possibly followed by a space)
    let last_char = trimmed.chars().last();
    if let Some(c) = last_char {
        return prompt_endings.contains(&c);
    }

    false
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

    lines
}

/// Trim trailing empty lines from shell output for display
pub fn trim_shell_lines(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
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
    // Clone the tx first to avoid borrow conflict with cursor reset
    let some_tx = state
        .active_shell_command
        .as_ref()
        .map(|cmd| cmd.stdin_tx.clone());

    if let Some(tx) = some_tx {
        // Mark that user has interacted with the shell
        if !data.is_empty() {
            state.shell_interaction_occurred = true;
            // Reset cursor blink to visible when typing
            crate::services::shell_popup::reset_cursor_blink(state);
        }

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

    // Determine if we should type a command after shell starts
    // For tool call shell mode (is_tool_call_shell_command), we type the pending command
    // For on-demand shell mode, we don't type anything
    let command_to_execute = if state.is_tool_call_shell_command {
        state.shell_pending_command_value.clone()
    } else {
        None
    };

    // Use PTY for cross-platform interactive shell support (Unix + Windows 10 1809+)
    let shell_cmd = match run_pty_command(command.clone(), command_to_execute, shell_tx, rows, cols)
    {
        Ok(cmd) => cmd,
        Err(e) => {
            push_error_message(state, &format!("Failed to run command: {}", e), None);
            return;
        }
    };

    state.active_shell_command = Some(shell_cmd.clone());
    state.active_shell_command_output = Some(String::new());

    // Create a new vt100 parser for the session with 1000 lines of scrollback
    state.shell_screen = vt100::Parser::new(rows, cols, 1000);
    // Reset interaction flag for new command
    state.shell_interaction_occurred = false;
    // Show loading indicator while shell initializes
    state.shell_loading = true;
    // Clear history for new session
    state.shell_history_lines.clear();

    // Create initial shell message with loading indicator
    let loading_colors = BubbleColors {
        border_color: Color::Yellow,
        title_color: Color::Yellow,
        content_color: AdaptiveColors::text(),
        tool_type: "Interactive Bash".to_string(),
    };
    let loading_content = vec![Line::from(vec![Span::styled(
        "  Starting shell...",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )])];
    let new_id = Uuid::new_v4();
    state.interactive_shell_message_id = Some(new_id);
    state.messages.push(Message {
        id: new_id,
        content: MessageContent::RenderRefreshedTerminal(
            format!("Shell Command {} [Initializing]", command),
            loading_content,
            Some(loading_colors),
            state.terminal_size.width as usize,
        ),
        is_collapsed: None,
    });

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

    // Set new popup state fields
    state.shell_popup_visible = true;
    state.shell_popup_expanded = true;
    state.shell_popup_scroll = 0; // Reset scroll to show bottom
    // Invalidate cache so old bordered message is hidden
    invalidate_message_lines_cache(state);
    // Also set legacy alias for backward compatibility
    state.show_shell_mode = true;
    state.text_area.set_shell_mode(true);
}

/// Handle run shell with command event - runs the command in an interactive shell
/// This spawns the user's shell, shows the prompt, then types and executes the command
pub fn handle_run_shell_with_command(
    state: &mut AppState,
    command: String,
    input_tx: &Sender<InputEvent>,
) {
    // Mark this as a tool call shell command for proper UI state
    state.is_tool_call_shell_command = true;
    // Store the command value for later tool call result
    state.shell_pending_command_value = Some(command.clone());
    // Initially false - will become true when command starts executing
    state.shell_pending_command_executed = false;
    state.shell_pending_command_output = Some(String::new());
    state.shell_pending_command_output_count = 0;
    // Reset prompt detection state for auto-completion
    state.shell_initial_prompt_shown = false;
    state.shell_command_typed = false;

    // Run the command via interactive shell - PTY will show prompt then type the command
    handle_run_shell_command(state, command, input_tx);
}

/// Helper to background the active shell session (minimize popup)
pub fn background_shell_session(state: &mut AppState) {
    if !state.show_shell_mode {
        return;
    }

    // Collapse popup (shrink, not hide)
    state.shell_popup_expanded = false;
    state.show_shell_mode = false;
    // Update textarea shell mode
    state.text_area.set_shell_mode(false);

    let command_name = state
        .active_shell_command
        .as_ref()
        .map(|c| c.command.clone())
        .unwrap_or_else(|| "shell".to_string());

    // state.shell_history_lines already contains the full history including active view
    // We trim it for the message bubble preview
    let mut fresh_lines = trim_shell_lines(state.shell_history_lines.clone());

    if let Some(cmd) = state.shell_pending_command_value.clone() {
        let command_line = Line::from(vec![
            Span::styled(
                "$ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(cmd, Style::default().fg(AdaptiveColors::text())),
        ]);
        fresh_lines.insert(0, command_line);
    }

    // Update the message bubble to gray (background)
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
                    format!("Shell Command {} (Background - Esc to exit)", command_name),
                    fresh_lines.clone(),
                    Some(new_colors),
                    state.terminal_size.width as usize,
                );
                break;
            }
        }
    }

    // Clear text area but persist shell
    state.text_area.set_text("");

    // Invalidate cache
    invalidate_message_lines_cache(state);
}

pub fn handle_shell_mode(state: &mut AppState, input_tx: &Sender<InputEvent>) {
    // '$' only EXPANDS the shell popup - it does NOT toggle/shrink
    // Only Esc can exit shell mode (handled elsewhere)

    // If already expanded, '$' IS TYPED INTO SHELL
    if state.show_shell_mode && state.shell_popup_expanded {
        if let Some(shell_cmd) = &state.active_shell_command {
            let stdin_tx = shell_cmd.stdin_tx.clone();
            tokio::spawn(async move {
                let _ = stdin_tx.send("$".to_string()).await;
            });
        }
        return;
    }

    // If popup is visible but shrunk (backgrounded), expand it back
    if state.shell_popup_visible && !state.shell_popup_expanded {
        state.shell_popup_expanded = true;
        state.show_shell_mode = true;
        state.text_area.set_shell_mode(true);

        // Update message to show focused state
        if let Some(id) = state.interactive_shell_message_id {
            let command_name = state
                .active_shell_command
                .as_ref()
                .map(|c| c.command.clone())
                .unwrap_or_else(|| "shell".to_string());

            let current_screen = capture_styled_screen(&mut state.shell_screen);

            let focused_colors = BubbleColors {
                border_color: Color::Cyan,
                title_color: Color::Cyan,
                content_color: AdaptiveColors::text(),
                tool_type: "Interactive Bash".to_string(),
            };

            for msg in &mut state.messages {
                if msg.id == id {
                    msg.content = MessageContent::RenderRefreshedTerminal(
                        format!("Shell Command {} [Focused]", command_name),
                        current_screen,
                        Some(focused_colors),
                        state.terminal_size.width as usize,
                    );
                    break;
                }
            }

            invalidate_message_lines_cache(state);
        }
        return;
    }

    // If we have an existing session, resume it
    if state.active_shell_command.is_some() {
        state.shell_popup_visible = true;
        state.shell_popup_expanded = true;
        state.show_shell_mode = true;
        state.text_area.set_shell_mode(true);
        invalidate_message_lines_cache(state);
        return;
    }

    // Start a new on-demand shell (no command - just interactive)
    state.ondemand_shell_mode = true;
    state.shell_pending_command_value = None; // No command to type
    state.shell_pending_command_executed = false;
    state.is_tool_call_shell_command = false;

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
            .unwrap_or_else(|| "shell".to_string());

        // If this was from an interactive stall, add the result to shell_tool_calls
        // so it gets sent with the user's message
        if state.shell_pending_command_executed {
            let cmd_value = state.shell_pending_command_value.take();
            let shell_output = state.shell_pending_command_output.take();

            // Build the tool call result with captured output
            let result = shell_command_to_tool_call_result(state, cmd_value, shell_output);

            // If this is NOT a tool call (but a TUI interactive stall),
            // add to shell_tool_calls so it gets sent with the NEXT user message.
            // For real tool calls, the result is sent immediately by the handler.
            if !state.is_tool_call_shell_command {
                if state.shell_tool_calls.is_none() {
                    state.shell_tool_calls = Some(Vec::new());
                }
                if let Some(ref mut tool_calls) = state.shell_tool_calls {
                    tool_calls.push(result);
                }
            }

            // Reset the tracking state
            state.shell_pending_command_executed = false;
            state.shell_pending_command_output_count = 0;
            state.dialog_command = None;
        }

        // Update the message in chat to reflect termination
        if let Some(id) = state.interactive_shell_message_id {
            for msg in &mut state.messages {
                if msg.id == id {
                    if let MessageContent::RenderRefreshedTerminal(_, lines, _, width) =
                        &msg.content
                    {
                        let (new_color, status_suffix) = if state.is_tool_call_shell_command {
                            (Color::Green, "Completed")
                        } else {
                            (Color::DarkGray, "Terminated")
                        };

                        let new_colors = BubbleColors {
                            border_color: new_color,
                            title_color: new_color,
                            content_color: AdaptiveColors::text(),
                            tool_type: format!("Interactive Bash ({})", status_suffix),
                        };
                        let trimmed_lines = trim_shell_lines(lines.clone());
                        msg.content = MessageContent::RenderRefreshedTerminal(
                            format!("Shell Command {} ({})", command_name, status_suffix),
                            trimmed_lines,
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
/// Returns true if auto-completion was triggered
pub fn handle_shell_output(state: &mut AppState, raw_data: String) -> bool {
    // Guard: If shell was terminated, ignore any pending output
    if state.active_shell_command.is_none() {
        return false;
    }

    // First output received - hide loading indicator
    state.shell_loading = false;

    // For tool call shell mode: first output means initialization is complete
    if state.is_tool_call_shell_command && !state.shell_pending_command_executed {
        state.shell_pending_command_executed = true;
    }

    // If we're tracking a pending command (from interactive stall), capture all output
    if state.shell_pending_command_executed {
        state.shell_pending_command_output_count += 1;
        if let Some(output) = state.shell_pending_command_output.as_mut() {
            output.push_str(&raw_data);
        }
    }

    // 1. Append to raw output log (truncated)
    if let Some(output) = state.active_shell_command_output.as_mut() {
        output.push_str(&raw_data);
        *output = truncate_output(output);
    }

    // Process raw output into Virtual Terminal Screen
    state.shell_screen.process(raw_data.as_bytes());

    // 3. Determine Styling based on Focus
    // Get the shell command for the title (simple version)
    let shell_name = state
        .active_shell_command
        .as_ref()
        .map(|c| {
            // Extract just the shell name from the path
            c.command
                .split('/')
                .next_back()
                .unwrap_or(&c.command)
                .split_whitespace()
                .next()
                .unwrap_or("shell")
        })
        .unwrap_or("shell")
        .to_string();

    let (colors, title) = if state.show_shell_mode {
        (
            BubbleColors {
                border_color: Color::Cyan, // Using Cyan for "Cool" look
                title_color: Color::Cyan,
                content_color: AdaptiveColors::text(),
                tool_type: "Interactive Bash".to_string(),
            },
            format!("Shell Command {} [Focused]", shell_name),
        )
    } else {
        (
            BubbleColors {
                border_color: Color::DarkGray,
                title_color: Color::DarkGray,
                content_color: AdaptiveColors::text(),
                tool_type: "Interactive Bash".to_string(),
            },
            format!("Shell Command {} (Background - '$' to focus)", shell_name),
        )
    };

    // 4. Capture styled screen content at scroll=0 (safe)
    let screen_lines = capture_styled_screen(&mut state.shell_screen);

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
    // Prepended lines (like command injection) that are NOT part of terminal history
    let mut display_lines = trim_shell_lines(screen_lines.clone());
    if let Some(cmd) = state.shell_pending_command_value.clone() {
        let command_line = Line::from(vec![
            Span::styled(
                "$ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(cmd, Style::default().fg(AdaptiveColors::text())),
        ]);
        display_lines.insert(0, command_line);
    }

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
                display_lines.clone(),
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
            display_lines.clone(),
            Some(colors),
            state.terminal_size.width as usize,
        );
    }

    // Invalidate message cache so the updated content is rendered
    invalidate_message_lines_cache(state);

    // === Auto-completion detection for tool call shell commands ===
    // Only for tool call shells (not on-demand shells)
    if !state.is_tool_call_shell_command {
        return false;
    }

    // Get the last non-empty line from display_lines to check for prompt
    let last_line_text = display_lines
        .iter()
        .rev()
        .find(|line| {
            let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            !text.trim().is_empty()
        })
        .map(|line| {
            line.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .unwrap_or_default();

    let current_is_prompt = looks_like_prompt(&last_line_text);

    // State machine for prompt detection:
    // 1. Initial state: shell_initial_prompt_shown = false, shell_command_typed = false
    // 2. First prompt seen: shell_initial_prompt_shown = true
    // 3. Command typed (output count increases after prompt): shell_command_typed = true
    // 4. New prompt after command + user interaction: AUTO-COMPLETE!

    if !state.shell_initial_prompt_shown {
        // This is the first output - if it's a prompt, mark initial prompt as shown
        if current_is_prompt {
            state.shell_initial_prompt_shown = true;
        }
        return false;
    }

    if !state.shell_command_typed {
        // We saw the initial prompt, now we're waiting for the command to be typed
        // Once output count increases (command starts producing output), command was typed
        if state.shell_pending_command_output_count > 2 {
            state.shell_command_typed = true;
        }
        return false;
    }

    // Command was typed. Now check if we've returned to prompt AND user has interacted
    // shell_interaction_occurred is set when user types something (like password)
    if current_is_prompt
        && state.shell_interaction_occurred
        && state.shell_pending_command_output_count > 3
    {
        // We're back at a prompt after the command ran and user interacted
        // This means the command is complete - trigger auto-completion!
        return true;
    }

    false
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

    // If this was from an interactive stall command OR a tool call shell, capture and log the result
    if state.shell_pending_command_executed || state.is_tool_call_shell_command {
        // CRITICAL: Capture values BEFORE calling terminate_active_shell_session
        // because it clears them!
        let cmd_value = state.shell_pending_command_value.clone();

        // Capture output from either pending command or active shell command
        let shell_output = if let Some(output) = state.shell_pending_command_output.clone() {
            Some(output)
        } else {
            state.active_shell_command_output.clone()
        };

        // remove ansi codes and everhting from shell output
        let processed_shell_output = shell_output.map(|output| {
            let output = preprocess_terminal_output(&output);
            output.replace("\r\n", "\n").replace('\r', "\n")
        });

        let saved_dialog_command = state.dialog_command.clone();

        let processed_terminal_command = cmd_value.map(|s| preprocess_terminal_output(&s));

        let result = shell_command_to_tool_call_result(
            state,
            processed_terminal_command,
            processed_shell_output,
        );

        // Auto-terminate and finalize the shell session
        terminate_active_shell_session(state);

        // Hide shell popup on completion
        state.shell_popup_visible = false;
        state.shell_popup_expanded = false;
        // Invalidate cache to restore normal message display
        invalidate_message_lines_cache(state);
        state.show_shell_mode = false;
        state.text_area.set_shell_mode(false);

        if let Some(dialog_command) = saved_dialog_command {
            let dialog_command_id = dialog_command.id.clone();
            // Check the index of dialog_command in tool_calls_execution_order
            let index = state
                .last_message_tool_calls
                .iter()
                .position(|tool_call| tool_call.id == dialog_command_id);

            let should_stop = if let Some(index) = index {
                index != state.last_message_tool_calls.len() - 1
            } else {
                false
            };

            // Get the ids of the tool calls after that id
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

            // Move those rejected tool calls to message_tool_calls
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

            if let Some(latest_tool_call) = &state.latest_tool_call
                && dialog_command.id == latest_tool_call.id
            {
                state.latest_tool_call = None;
            }
            state.dialog_command = None;
            state.toggle_approved_message = true;
        }

        // Invalidate cache to show the updated message
        invalidate_message_lines_cache(state);
    }

    if state.ondemand_shell_mode && state.shell_interaction_occurred {
        let new_tool_call_result = shell_command_to_tool_call_result(state, None, None);
        if let Some(ref mut tool_calls) = state.shell_tool_calls {
            tool_calls.push(new_tool_call_result);
        }
    }

    state.active_shell_command = None;
    state.active_shell_command_output = None;
    // Remove the RefreshedTerminal message when shell completes
    if let Some(shell_msg_id) = state.interactive_shell_message_id {
        state.messages.retain(|m| m.id != shell_msg_id);
    }
    state.interactive_shell_message_id = None;
    state.text_area.set_text("");
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
/// Includes the actual shell output if provided
pub fn shell_command_to_tool_call_result(
    state: &mut AppState,
    command_value: Option<String>,
    shell_output: Option<String>,
) -> ToolCallResult {
    let (id, name) = if let Some(cmd) = &state.dialog_command {
        (cmd.id.clone(), cmd.function.name.clone())
    } else {
        (
            format!("tool_{}", Uuid::new_v4()),
            "run_command".to_string(),
        )
    };

    // Use the original command value if provided, otherwise fall back to active_shell_command
    let command = command_value.unwrap_or_else(|| {
        state
            .active_shell_command
            .as_ref()
            .map(|cmd| cmd.command.clone())
            .unwrap_or_default()
    });

    let args = format!("{{\"command\": \"{}\"}}", command);

    // Build the result string with actual output
    let result_text = if let Some(output) = shell_output {
        // Clean up the output - split into lines, remove the LAST line (prompt), and join
        let mut lines: Vec<&str> = output.lines().collect();
        if !lines.is_empty() {
            lines.pop(); // Remove the last line (prompt)
        }
        let cleaned_output = lines.join("\n").trim().to_string();

        if cleaned_output.is_empty() {
            "Command completed (no output)".to_string()
        } else {
            truncate_output(&cleaned_output)
        }
    } else {
        "Interactive shell exited".to_string()
    };

    let call = ToolCall {
        id,
        r#type: "function".to_string(),
        function: FunctionCall {
            name,
            arguments: args,
        },
        metadata: None,
    };
    ToolCallResult {
        call,
        result: result_text,
        status: ToolCallResultStatus::Success,
    }
}
