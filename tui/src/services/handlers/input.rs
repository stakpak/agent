//! Input Event Handlers
//!
//! Handles all input-related events including text input, cursor movement, and paste operations.

use std::path::{Path, PathBuf};

use crate::app::{AppState, AttachedImage, InputEvent, OutputEvent, PendingUserMessage};
use crate::constants::MAX_PASTE_CHAR_COUNT;
use crate::services::auto_approve::AutoApprovePolicy;
use crate::services::clipboard_paste::{normalize_pasted_path, paste_image_to_temp_png};
use crate::services::commands::{CommandContext, execute_command};
use crate::services::file_search::handle_file_selection;
use crate::services::helper_block::{
    push_clear_message, push_error_message, push_styled_message, render_system_message,
};
use crate::services::message::{BubbleColors, Message, MessageContent};
use ratatui::style::{Color, Style};
use stakpak_shared::models::llm::LLMTokenUsage;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

/// Handle InputChanged event - routes to appropriate handler based on popup state
pub fn handle_input_changed_event(state: &mut AppState, c: char, input_tx: &Sender<InputEvent>) {
    if state.approval_bar.is_visible() {
        if c == ' ' {
            state.approval_bar.toggle_selected();
            return;
        }
        // Block all typing when approval bar is visible
        return;
    }
    if state.show_shortcuts_popup {
        // Handle search input for command palette / shortcuts
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
    handle_input_changed(state, c, input_tx);
}

/// Handle InputBackspace event - routes to appropriate handler based on popup state
pub fn handle_input_backspace_event(state: &mut AppState, input_tx: &Sender<InputEvent>) {
    if state.approval_bar.is_visible() {
        // Block backspace when approval bar is visible
        return;
    }
    if state.show_shortcuts_popup {
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
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
) {
    if state.show_profile_switcher {
        let _ = input_tx.try_send(InputEvent::ProfileSwitcherSelect);
        return;
    }
    if state.show_shortcuts_popup {
        match state.shortcuts_popup_mode {
            crate::app::ShortcutsPopupMode::Commands => {
                // Execute the selected command
                use super::tool::execute_command_palette_selection;
                execute_command_palette_selection(state, input_tx, output_tx);
                return;
            }
            crate::app::ShortcutsPopupMode::Sessions => {
                // Select the session and resume it
                if !state.sessions.is_empty() && state.session_selected < state.sessions.len() {
                    let selected = &state.sessions[state.session_selected];
                    let selected_id = selected.id.to_string();
                    let selected_title = selected.title.clone();
                    let _ = output_tx.try_send(OutputEvent::SwitchToSession(selected_id));

                    // Reset state for new session
                    state.message_tool_calls = None;
                    state.message_approved_tools.clear();
                    state.message_rejected_tools.clear();
                    state.tool_call_execution_order.clear();
                    state.session_tool_calls_queue.clear();
                    state.approval_bar.clear();
                    state.toggle_approved_message = true;
                    state.messages.clear();
                    state.scroll = 0;
                    state.scroll_to_bottom = true;
                    state.stay_at_bottom = true;

                    // Clear changeset and todos from previous session
                    state.changeset = crate::services::changeset::Changeset::default();
                    state.todos.clear();

                    crate::services::message::invalidate_message_lines_cache(state);

                    // Reset usage
                    state.total_session_usage = LLMTokenUsage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                        prompt_tokens_details: None,
                    };
                    state.current_message_usage = LLMTokenUsage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                        prompt_tokens_details: None,
                    };

                    render_system_message(
                        state,
                        &format!("Switching to session . {}", selected_title),
                    );
                    state.show_shortcuts_popup = false;
                }
                return;
            }
            crate::app::ShortcutsPopupMode::Shortcuts => {
                // Shortcuts tab doesn't have enter action, just ignore
            }
        }
    }
    if state.show_rulebook_switcher {
        let _ = input_tx.try_send(InputEvent::RulebookSwitcherConfirm);
        return;
    }
    // Handle approval bar submission (inline approval)
    // Enter key: approve all pending tools and execute
    if state.approval_bar.is_visible() {
        use crate::app::ToolCallStatus;

        // Update approved and rejected tool calls from bar
        state.message_approved_tools = state
            .approval_bar
            .get_approved_tool_calls()
            .into_iter()
            .cloned()
            .collect();
        state.message_rejected_tools = state
            .approval_bar
            .get_rejected_tool_calls()
            .into_iter()
            .cloned()
            .collect();

        // Process tools in order using message_tool_calls
        if let Some(tool_calls) = &state.message_tool_calls.clone() {
            for tool_call in tool_calls {
                let is_approved = state.message_approved_tools.contains(tool_call);
                let status = if is_approved {
                    ToolCallStatus::Approved
                } else {
                    ToolCallStatus::Rejected
                };
                state.tool_call_execution_order.push(tool_call.id.clone());
                state
                    .session_tool_calls_queue
                    .insert(tool_call.id.clone(), status);
            }

            // Always execute the FIRST tool, regardless of which tab is selected
            // User pressing Enter means "I'm done reviewing, start execution from the beginning"
            if let Some(first_tool) = tool_calls.first() {
                // Set dialog_command to the first tool for proper processing
                state.dialog_command = Some(first_tool.clone());
                state
                    .session_tool_calls_queue
                    .insert(first_tool.id.clone(), ToolCallStatus::Executed);

                let is_approved = state.message_approved_tools.contains(first_tool);

                // Update the pending display to show the first tool (which is being executed)
                // This ensures the UI shows the correct tool as "running", not the selected one
                super::dialog::update_pending_tool_to_first(state, first_tool, is_approved);

                if is_approved {
                    // Update run_command block to Running state
                    super::dialog::update_run_command_to_running(state, first_tool);
                    let _ = output_tx.try_send(OutputEvent::AcceptTool(first_tool.clone()));
                } else {
                    // Fire handle reject - set is_dialog_open for handle_esc to work
                    state.is_dialog_open = true;
                    let _ = input_tx.try_send(InputEvent::HandleReject(
                        Some("Tool call rejected".to_string()),
                        true,
                        None,
                    ));
                }
            }
        }

        // Clear state
        state.message_tool_calls = None;
        state.is_dialog_open = false;

        // Clear the approval bar
        state.approval_bar.clear();
        return;
    }

    // If side panel is visible and input is empty, Enter toggles the focused section
    // This is safe because empty input has nothing to submit anyway
    if state.show_side_panel && !state.is_dialog_open && state.text_area.text().is_empty() {
        let current = state
            .side_panel_section_collapsed
            .get(&state.side_panel_focus)
            .copied()
            .unwrap_or(false);
        state
            .side_panel_section_collapsed
            .insert(state.side_panel_focus, !current);
        return;
    }

    if !state.is_pasting {
        handle_input_submitted(
            state,
            message_area_height,
            output_tx,
            input_tx,
            shell_tx,
            cancel_tx,
        );
    }
}

/// Handle character input change
pub fn handle_input_changed(state: &mut AppState, c: char, input_tx: &Sender<InputEvent>) {
    state.show_shortcuts = false;

    if c == '$' && state.input().is_empty() {
        state.text_area.set_text("");
        // Shell mode toggle will be handled by shell module
        use super::shell;
        shell::handle_shell_mode(state, input_tx);
        return;
    }

    state.text_area.insert_str(&c.to_string());

    // Check if editing inside a placeholder - reveal original path
    check_placeholder_edit(state);

    // Detect and convert pending paths
    detect_and_convert_paths(state);

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

    // Check if editing inside a placeholder - reveal original path
    check_placeholder_edit(state);

    // If a large paste placeholder is present and input is edited, only clear pasted state if placeholder is completely removed
    if let Some(placeholder) = &state.pasted_placeholder
        && !state.input().contains(placeholder)
    {
        state.pasted_long_text = None;
        state.pasted_placeholder = None;
    }

    // Clean up attached_images when their placeholders are no longer in the input
    // This prevents orphaned image references when user backspaces over placeholders
    let current_input = state.input().to_string();
    state
        .attached_images
        .retain(|img| current_input.contains(&img.placeholder));

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
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
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

            // Run the shell command via event
            let _ = shell_tx.try_send(InputEvent::RunShellCommand(command.clone()));
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

    if state.is_dialog_open {
        state.toggle_approved_message = true;
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
            return; // Only return after executing a valid command
        }

        // IMPORTANT: If no matching helpers and not in file search,
        // fall through to check for commands with arguments below
        state.show_helper_dropdown = false;
    }

    // Check if input starts with a known command that takes arguments
    // This runs regardless of show_helper_dropdown state to handle cases like:
    // - User types "/editor path/to/file" manually
    // - User selects a file via @ dropdown, resulting in "/editor filename"
    {
        let input = state.input().to_string();
        let command_with_args: Option<&str> = if input.starts_with("/editor ") {
            Some("/editor")
        } else if input.starts_with("/toggle_auto_approve ") {
            Some("/toggle_auto_approve")
        } else {
            None
        };

        if let Some(command_id) = command_with_args {
            let ctx = CommandContext {
                state,
                input_tx,
                output_tx,
            };
            if let Err(e) = execute_command(command_id, ctx) {
                push_error_message(state, &e, None);
            }
            return;
        }
    }

    if !state.input().trim().is_empty() || !state.attached_images.is_empty() {
        // Allow submission if there's text input OR attached images

        log::debug!(
            "Submitting message with {} attached images",
            state.attached_images.len()
        );

        // Convert any pending image paths before submission
        convert_all_pending_paths(state);

        let input_height = 3;
        let total_lines = state.messages.len() * 2;
        let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        let was_at_bottom = state.scroll == max_scroll;

        let mut final_input = state.input().to_string();

        // Check for tool-initiated shell resolution
        if state.is_tool_call_shell_command
            && state.active_shell_command.is_some()
            && !state.show_shell_mode
        {
            // 1. Signal cancellation to unblock the MCP task
            if let Some(cancel_tx) = &cancel_tx {
                let _ = cancel_tx.send(());
            }

            // 2. Capture history and resolve as success result
            let history_lines = crate::services::handlers::shell::trim_shell_lines(
                state.shell_history_lines.clone(),
            );
            if !history_lines.is_empty() {
                let history_text = history_lines
                    .iter()
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");

                // Form a successful tool call result with history
                let result = crate::services::handlers::shell::shell_command_to_tool_call_result(
                    state,
                    state.shell_pending_command_value.clone(),
                    Some(history_text),
                );

                // Send the result via OutputEvent so the LLM gets it
                let _ = output_tx.try_send(OutputEvent::SendToolResult(result, false, Vec::new()));
            }

            // 3. Clean up the shell
            crate::services::handlers::shell::terminate_active_shell_session(state);
            state.is_tool_call_shell_command = false;
        }

        // Check for on-demand shell termination and history attachment
        if state.ondemand_shell_mode
            && state.active_shell_command.is_some()
            && !state.show_shell_mode
        {
            let mut history_lines = crate::services::handlers::shell::trim_shell_lines(
                state.shell_history_lines.clone(),
            );

            if !history_lines.is_empty() {
                history_lines.pop();
            }

            if !history_lines.is_empty() {
                // Add history message for UI
                state.messages.push(Message {
                    id: Uuid::new_v4(),
                    content: MessageContent::RenderRefreshedTerminal(
                        "Shell history".to_string(),
                        history_lines.clone(),
                        Some(BubbleColors {
                            border_color: Color::Magenta,
                            title_color: Color::Magenta,
                            content_color: Color::Reset,
                            tool_type: "Shell".to_string(),
                        }),
                        state.terminal_size.width as usize,
                    ),
                    is_collapsed: None,
                });

                // Construct ToolCallResult for LLM context (instead of appending text)
                let history_text = history_lines
                    .iter()
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");

                if state.shell_tool_calls.is_none() {
                    state.shell_tool_calls = Some(Vec::new());
                }

                let result = crate::services::handlers::shell::shell_command_to_tool_call_result(
                    state,
                    Some("/bin/bash (Interactive Session)".to_string()),
                    Some(history_text),
                );

                if let Some(ref mut tool_calls) = state.shell_tool_calls {
                    tool_calls.push(result);
                }
            }

            // Remove the active shell message bubble
            if let Some(shell_msg_id) = state.interactive_shell_message_id {
                state.messages.retain(|m| m.id != shell_msg_id);
            }
            state.interactive_shell_message_id = None;

            // Full clear of shell variables
            state.active_shell_command = None;
            state.active_shell_command_output = None;
            state.shell_history_lines.clear();
            state.show_shell_mode = false;
            state.shell_popup_visible = false;
            state.shell_popup_expanded = false;
            state.ondemand_shell_mode = false; // Reset on-demand mode

            // Note: We don't call terminate_active_shell_session here because we manually cleaned up
            // and we don't want the "Terminated" message update logic from that function.
            // But we DO need to ensure the actual process is killed.
            // terminate_active_shell_session calls handle_shell_kill(state).
            // We should call handle_shell_kill directly or similar.
            crate::services::handlers::shell::handle_shell_kill(state);
        } else {
            // Standard cleanup for other cases (like interactive stall termination if not on-demand flow)
            crate::services::handlers::shell::terminate_active_shell_session(state);
        }

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

        // Keep placeholders in text for LLM context
        let user_message_text = final_input.clone();

        // Use current_model if set (from streaming), otherwise use default model
        let active_model = state.current_model.as_ref().unwrap_or(&state.model);
        let max_tokens = active_model.limit.context as u32;

        // Use prompt_tokens for context window utilization (actual input context size)
        let capped_tokens = state.current_message_usage.prompt_tokens.min(max_tokens);
        let utilization_ratio = (capped_tokens as f64 / max_tokens as f64).clamp(0.0, 1.0);
        let utilization_pct = (utilization_ratio * 100.0).round() as u64;

        if utilization_pct < 92 {
            // Process all images and create ContentParts
            let attached_paths: Vec<_> = state
                .attached_images
                .iter()
                .map(|img| img.path.clone())
                .collect();

            let image_parts =
                crate::services::image_upload::process_all_images(&final_input, &attached_paths);

            if image_parts.is_empty() && !attached_paths.is_empty() {
                log::warn!(
                    "Had {} attached paths but created 0 ContentParts",
                    attached_paths.len()
                );
            }

            // Check for image processing errors
            let mut image_errors = Vec::new();
            for img in &state.attached_images {
                if !img.path.exists() {
                    image_errors.push(format!("Image file not found: {}", img.path.display()));
                } else if !is_supported_format(&img.path) {
                    let ext = img
                        .path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("unknown");
                    image_errors.push(format!(
                        "Unsupported image format: {} ({}). Supported formats: JPEG, PNG, GIF, WebP", 
                        img.filename,
                        ext.to_uppercase()
                    ));
                }
            }

            if !image_errors.is_empty() {
                for error in image_errors {
                    push_error_message(state, &error, None);
                }
            }

            let should_buffer_message =
                state.loading_manager.is_loading() || !state.pending_user_messages.is_empty();

            if should_buffer_message {
                // Buffer while operations are active (or if previous buffered messages are pending)
                state
                    .pending_user_messages
                    .push_back(PendingUserMessage::new(
                        final_input.clone(),
                        state.shell_tool_calls.clone(),
                        image_parts,
                        user_message_text,
                    ));
            } else {
                // Take pending revert index if set (will be None on normal messages)
                let revert_index = state.pending_revert_index.take();

                if let Err(e) = output_tx.try_send(OutputEvent::UserMessage(
                    final_input.clone(),
                    state.shell_tool_calls.clone(),
                    image_parts,
                    revert_index,
                )) {
                    log::warn!("Failed to send UserMessage event: {}", e);
                }

                if let Err(e) = input_tx.try_send(InputEvent::AddUserMessage(user_message_text)) {
                    log::warn!("Failed to send AddUserMessage event: {}", e);
                }
            }
        } else {
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

        // Always clear attached images and reset state after submission
        state.shell_tool_calls = None;
        state.text_area.set_text("");
        state.attached_images.clear();
        state.pending_path_start = None;
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

/// Handle text paste (Event::Paste), including large text and image *paths*.
pub fn handle_paste(state: &mut AppState, pasted: String) -> bool {
    // Normalize line endings: many terminals convert newlines to \r when pasting,
    // but textarea expects \n. This is the same fix used in Codex.
    let normalized_pasted = pasted.replace("\r\n", "\n").replace('\r', "\n");

    // On macOS, Cmd+V might paste text even when an image is on clipboard.
    // First, try to check if there's an image on the clipboard (for Cmd+V on macOS).
    #[cfg(not(target_os = "android"))]
    {
        // Try to get image from clipboard - if successful, use that instead of text
        // This also checks for file paths in clipboard text
        if let Ok((path, info)) = paste_image_to_temp_png() {
            attach_image(
                state,
                path,
                info.width,
                info.height,
                info.encoded_format.label(),
            );
            return true;
        }
    }

    // Detect and redact secrets in pasted content
    // This allows users to paste API keys, passwords, etc. and have them automatically
    // redacted with placeholders that the agent can use
    let redacted_pasted = state
        .secret_manager
        .redact_and_store_secrets(&normalized_pasted, None);

    // Also check if the pasted text itself contains file paths
    // (e.g., user pastes "check this out /path/to/image.png")
    let char_count = redacted_pasted.chars().count();
    if char_count > MAX_PASTE_CHAR_COUNT {
        let placeholder = format!("[Pasted Content {char_count} chars]");
        state.text_area.insert_element(&placeholder);
        // Store the redacted version (with placeholders) for later expansion
        state.pending_pastes.push((placeholder, redacted_pasted));
    } else if char_count > 1 && handle_paste_image_path(state, redacted_pasted.clone()) {
        // Path inserted - conversion will happen when user types or hits Enter
    } else {
        state.text_area.insert_str(&redacted_pasted);
    }

    true
}

/// Handle Ctrl+V clipboard image paste (non-text clipboard images).
pub fn handle_clipboard_image_paste(state: &mut AppState) {
    state.is_pasting = true;
    #[cfg(not(target_os = "android"))]
    {
        match paste_image_to_temp_png() {
            Ok((path, info)) => {
                attach_image(
                    state,
                    path,
                    info.width,
                    info.height,
                    info.encoded_format.label(),
                );
            }
            Err(e) => {
                log::warn!("Failed to paste image from clipboard: {}", e);
                let error_msg = match e.to_string().as_str() {
                    s if s.contains("clipboard unavailable") => {
                        "Clipboard is not available. Please check system permissions.".to_string()
                    }
                    s if s.contains("no image") => {
                        "No image found on clipboard. Please copy an image first.".to_string()
                    }
                    s if s.contains("encode") => {
                        "Failed to process clipboard image. The image format may not be supported."
                            .to_string()
                    }
                    s if s.contains("io error") => {
                        "Failed to save clipboard image. Please try again.".to_string()
                    }
                    _ => format!("Failed to paste image: {}", e),
                };
                push_error_message(state, &error_msg, None);
            }
        }
    }
    #[cfg(target_os = "android")]
    {
        push_error_message(state, "Image paste is not supported on Android.", None);
    }
    state.is_pasting = false;
}

fn handle_paste_image_path(state: &mut AppState, pasted: String) -> bool {
    // First, try to normalize as a direct path
    if let Some(path_buf) = normalize_pasted_path(&pasted)
        && image::image_dimensions(&path_buf).is_ok()
    {
        // Just insert the path as text - let detection logic handle conversion
        state
            .text_area
            .insert_str(path_buf.display().to_string().as_str());
        return true;
    }

    // Try to extract file paths from text that may contain other content
    let extracted_paths = crate::services::clipboard_paste::extract_file_paths_from_text(&pasted);
    for path_buf in extracted_paths {
        // Validate it's a valid image
        if image::image_dimensions(&path_buf).is_ok() {
            // Just insert the path as text - let detection logic handle conversion
            state
                .text_area
                .insert_str(path_buf.display().to_string().as_str());
            return true;
        }
    }

    // Check if it looks like a filename (no slashes, might be a bare filename)
    let trimmed = pasted.trim();
    if !trimmed.is_empty() && !trimmed.contains('/') && !trimmed.contains('\\') {
        // Search common directories for image files matching this name
        if let Some(path_buf) = find_image_file_by_name(trimmed) {
            // Validate it's a valid image
            if image::image_dimensions(&path_buf).is_ok() {
                // Just insert the path as text - let detection logic handle conversion
                state
                    .text_area
                    .insert_str(path_buf.display().to_string().as_str());
                return true;
            }
        }
    }

    false
}

/// Search common directories for an image file matching the given name (with various extensions)
pub fn find_image_file_by_name(name: &str) -> Option<PathBuf> {
    // Common image extensions to try
    const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff", "tif"];

    // Common directories to search (Desktop, Downloads, Documents, Pictures)
    let common_dirs = [
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(&h).join("Desktop")),
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(&h).join("Downloads")),
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(&h).join("Documents")),
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(&h).join("Pictures")),
        // Also try current directory
        std::env::current_dir().ok(),
    ];

    for dir_opt in common_dirs.iter().flatten() {
        for ext in IMAGE_EXTENSIONS {
            let candidate = dir_opt.join(format!("{}.{}", name, ext));
            if candidate.exists() && candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}
fn attach_image(state: &mut AppState, path: PathBuf, width: u32, height: u32, format_label: &str) {
    let filename = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled")
        .to_string();
    let placeholder = format!("[Image {} {}x{} {}]", filename, width, height, format_label);
    let cursor = state.cursor_position();
    state.text_area.insert_element(&placeholder);
    state.attached_images.push(AttachedImage {
        placeholder: placeholder.clone(),
        path: path.clone(),
        filename,
        dimensions: (width, height),
        start_pos: cursor,
        end_pos: cursor + placeholder.len(),
    });
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

/// Handle cursor left movement (with approval bar check)
pub fn handle_cursor_left(state: &mut AppState) {
    if state.approval_bar.is_visible() {
        state.approval_bar.select_prev();
        return; // Event was consumed by approval bar
    }
    state.text_area.move_cursor_left();
}

/// Handle cursor right movement (with approval bar check)
pub fn handle_cursor_right(state: &mut AppState) {
    if state.approval_bar.is_visible() {
        state.approval_bar.select_next();
        return; // Event was consumed by approval bar
    }
    state.text_area.move_cursor_right();
}

/// Check if user is editing inside a placeholder and reveal original path
fn check_placeholder_edit(state: &mut AppState) {
    let input = state.input();
    let cursor = state.cursor_position();

    // Check if any placeholder is modified
    for img in state.attached_images.clone().iter() {
        if cursor >= img.start_pos && cursor <= img.end_pos {
            // Check if placeholder still matches
            if img.end_pos <= input.len() {
                let current_text = &input[img.start_pos..img.end_pos];
                if current_text != img.placeholder {
                    // Placeholder modified - reveal path
                    let path_str = img.path.display().to_string();
                    state
                        .text_area
                        .replace_range(img.start_pos..img.end_pos, &path_str);
                    state.text_area.set_cursor(img.start_pos + path_str.len());
                    state
                        .attached_images
                        .retain(|p| p.start_pos != img.start_pos);
                    return;
                }
            }
        }
    }
}

/// Detect paths in input and convert them to placeholders
fn detect_and_convert_paths(state: &mut AppState) {
    let input = state.input();
    let cursor = state.cursor_position();

    // Find all image paths in the input
    let paths = find_all_image_paths(input);

    // Find which path to convert (if any)
    let mut path_to_convert: Option<(usize, usize, String)> = None;
    let mut pending_start: Option<usize> = None;

    for (start, end, path_str) in paths {
        // Skip if this range already contains a placeholder
        if end <= input.len() {
            let text_at_pos = &input[start..end];
            if text_at_pos.starts_with("[Image ") {
                continue; // Already a placeholder
            }
        }

        // Skip URLs
        if path_str.starts_with("http://") || path_str.starts_with("https://") {
            continue;
        }

        // Convert if:
        // 1. User typed after the path (cursor > end), OR
        // 2. There's text after the path (not at end of input)
        let has_text_after = end < input.len();
        let cursor_after_path = cursor > end;

        if has_text_after || cursor_after_path {
            // Mark this path for conversion
            path_to_convert = Some((start, end, path_str));
            break; // Only convert one at a time
        } else {
            // Path is at the end, waiting for more input
            pending_start = Some(start);
        }
    }

    // Now perform the conversion if needed
    if let Some((start, end, path_str)) = path_to_convert {
        let success = convert_path_to_placeholder(state, start, end, &path_str);
        if success {
            update_placeholder_positions_after_replacement(state);
        }
        state.pending_path_start = None;
    } else if let Some(start) = pending_start {
        state.pending_path_start = Some(start);
    }
}

/// Find all image paths in the input text
fn find_all_image_paths(input: &str) -> Vec<(usize, usize, String)> {
    const IMAGE_EXTS: &[&str] = &[
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".tiff", ".tif",
    ];
    let mut paths = Vec::new();

    // Pattern 1: Quoted paths (single or double quotes) - handles spaces
    // Example: '/path/to/file with spaces.png' or "/path/to/file.png"
    for quote in &['\'', '"'] {
        let mut i = 0;
        while i < input.len() {
            if let Some(start) = input[i..].find(*quote) {
                let start_pos = i + start;
                let after_quote = start_pos + quote.len_utf8();

                if let Some(end_quote_pos) = input[after_quote..].find(*quote) {
                    let path_start = after_quote;
                    let path_end = after_quote + end_quote_pos;
                    let potential_path = &input[path_start..path_end];

                    // Check if it has an image extension
                    if IMAGE_EXTS
                        .iter()
                        .any(|ext| potential_path.to_lowercase().ends_with(ext))
                    {
                        // Store the full quoted text (including quotes) for proper replacement
                        let end_with_quote = path_end + quote.len_utf8();
                        let quoted_text = &input[start_pos..end_with_quote];
                        paths.push((start_pos, end_with_quote, quoted_text.to_string()));
                    }

                    i = path_end + quote.len_utf8();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    // Pattern 2: Unquoted paths (must not contain spaces)
    // Look for image extensions and work backwards/forwards
    // IMPORTANT: Must look like an actual file path, not just text ending with an extension
    for ext in IMAGE_EXTS {
        let ext_lower = ext.to_lowercase();
        let mut search_pos = 0;

        while let Some(ext_pos) = input[search_pos..].to_lowercase().find(&ext_lower) {
            let ext_start = search_pos + ext_pos;
            let ext_end = ext_start + ext.len();

            // Work backwards to find path start
            let mut path_start = ext_start;
            while path_start > 0 {
                let prev_char = input.as_bytes()[path_start - 1];
                // Stop at whitespace or quotes
                if prev_char == b' '
                    || prev_char == b'\n'
                    || prev_char == b'\t'
                    || prev_char == b'\''
                    || prev_char == b'"'
                {
                    break;
                }
                path_start -= 1;
            }

            // Make sure we're on char boundary
            while path_start > 0 && !input.is_char_boundary(path_start) {
                path_start -= 1;
            }

            let potential_path = &input[path_start..ext_end];

            // Skip if this is part of a quoted path (already handled)
            let is_quoted = paths
                .iter()
                .any(|(s, e, _)| *s <= path_start && *e >= ext_end);

            let looks_like_path = potential_path.contains('/')
                || potential_path.contains('\\')
                || potential_path.starts_with('~')
                || potential_path.starts_with(".")
                || potential_path.starts_with("[Image "); // Already a placeholder

            if !is_quoted && !potential_path.is_empty() && looks_like_path {
                paths.push((path_start, ext_end, potential_path.to_string()));
            }

            search_pos = ext_end;
        }
    }

    // Sort by position and deduplicate
    paths.sort_by_key(|(start, _, _)| *start);
    paths.dedup_by_key(|(start, _, _)| *start);

    paths
}

/// Convert all pending image paths in the input (called on submission)
fn convert_all_pending_paths(state: &mut AppState) {
    let mut attempts = 0;
    const MAX_ATTEMPTS: usize = 20; // Safety limit to prevent infinite loops

    loop {
        attempts += 1;
        if attempts > MAX_ATTEMPTS {
            log::warn!("Exceeded max attempts to convert image paths");
            break;
        }

        let input_before = state.input().to_string();
        let paths = find_all_image_paths(&input_before);

        // Find first unconverted path
        let mut found_path = None;
        for (start, end, path_str) in paths {
            // Skip if this range already contains a placeholder
            if end <= input_before.len() {
                let text_at_pos = &input_before[start..end];
                if text_at_pos.starts_with("[Image ") {
                    continue; // Already a placeholder
                }
            }

            // Skip URLs
            if path_str.starts_with("http://") || path_str.starts_with("https://") {
                continue;
            }

            found_path = Some((start, end, path_str));
            break;
        }

        if let Some((start, end, path_str)) = found_path {
            let success = convert_path_to_placeholder(state, start, end, &path_str);

            if success {
                // Update positions of all existing placeholders after replacement
                update_placeholder_positions_after_replacement(state);
            } else {
                // Failed to convert - keep the path in text and skip it
                break; // Stop trying to convert this batch
            }

            // Check if input actually changed to prevent infinite loop
            let input_after = state.input().to_string();
            if input_after == input_before {
                log::warn!("Input unchanged after conversion attempt, breaking loop");
                break;
            }
        } else {
            break; // No more paths to convert
        }
    }

    state.pending_path_start = None;
}

/// Check if image format is supported by the API
fn is_supported_format(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    !matches!(ext.as_str(), "tiff" | "tif" | "bmp")
}

/// Update placeholder positions after a text replacement
fn update_placeholder_positions_after_replacement(state: &mut AppState) {
    let input = state.input().to_string();

    // Recalculate all placeholder positions based on current input
    for img in &mut state.attached_images {
        // Find where this placeholder actually is in the current input
        if let Some(pos) = input.find(&img.placeholder) {
            img.start_pos = pos;
            img.end_pos = pos + img.placeholder.len();
        }
    }
}

/// Convert a path string to an image placeholder
/// Returns true if successful, false if failed
fn convert_path_to_placeholder(
    state: &mut AppState,
    start: usize,
    end: usize,
    path_str: &str,
) -> bool {
    // Strip quotes if present
    let clean_path = path_str.trim_matches('\'').trim_matches('"');
    let path = PathBuf::from(clean_path);

    // Resolve relative paths
    let resolved_path = if path.is_absolute() {
        path.clone()
    } else {
        // Try resolving from current directory
        if let Ok(cwd) = std::env::current_dir() {
            let resolved = cwd.join(&path);
            if resolved.exists() {
                resolved
            } else {
                // Maybe it's a path like "Users/..." that needs leading /
                let with_slash = PathBuf::from(format!("/{}", clean_path));
                if with_slash.exists() {
                    with_slash
                } else {
                    path.clone()
                }
            }
        } else {
            path.clone()
        }
    };

    // Quick validation - just check if file exists
    if !resolved_path.exists() || !resolved_path.is_file() {
        return false;
    }

    // Don't convert unsupported formats to placeholders - keep as path
    if !is_supported_format(&resolved_path) {
        return false;
    }

    // Get dimensions quickly without processing
    let (width, height) = match image::image_dimensions(&resolved_path) {
        Ok(dims) => dims,
        Err(_) => return false,
    };

    let filename = resolved_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled")
        .to_string();

    let placeholder = format!("[Image {} {}x{} JPEG]", filename, width, height);

    // Replace path with placeholder
    state.text_area.replace_range(start..end, &placeholder);
    state
        .text_area
        .register_element(start..start + placeholder.len());

    // Check if this path is already attached (avoid duplicates)
    let already_attached = state
        .attached_images
        .iter()
        .any(|img| img.path == resolved_path);

    if !already_attached {
        // Store original path - processing will happen on submission
        state.attached_images.push(AttachedImage {
            placeholder: placeholder.clone(),
            path: resolved_path,
            filename,
            dimensions: (width, height),
            start_pos: start,
            end_pos: start + placeholder.len(),
        });
    }

    true
}
