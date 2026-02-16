//! Tool Call Event Handlers
//!
//! Handles all tool call-related events including streaming tool results, retry logic, and approval popup events.

use crate::app::{AppState, InputEvent, OutputEvent, ToolCallStatus};
use crate::services::commands::{CommandAction, CommandContext, execute_command, filter_commands};
use crate::services::helper_block::push_error_message;
use crate::services::message::{Message, invalidate_message_lines_cache};
use stakpak_shared::models::integrations::openai::{
    ProgressType, ToolCall, ToolCallResult, ToolCallResultProgress, ToolCallResultStatus,
    ToolCallStreamInfo,
};
use tokio::sync::mpsc::Sender;

use super::shell::extract_command_from_tool_call;
use vt100;

/// Handle stream tool result event
/// Returns Some(command) if an interactive stall was detected and shell mode should be triggered
pub fn handle_stream_tool_result(
    state: &mut AppState,
    progress: ToolCallResultProgress,
) -> Option<String> {
    let tool_call_id = progress.id;
    // Check if this tool call is already completed - if so, ignore streaming updates
    if state.completed_tool_calls.contains(&tool_call_id) {
        return None;
    }

    // Check for interactive stall notification
    const INTERACTIVE_STALL_MARKER: &str = "__INTERACTIVE_STALL__";
    if progress.message.contains(INTERACTIVE_STALL_MARKER) {
        // Extract the message content (everything after the marker)
        let stall_message = progress
            .message
            .replace(INTERACTIVE_STALL_MARKER, "")
            .trim_start_matches(':')
            .trim()
            .to_string();

        // Update the pending/running bash message to show stall warning
        if let Some(pending_id) = state.pending_bash_message_id {
            for msg in &mut state.messages {
                if msg.id == pending_id {
                    // Update to the stall warning variant - handle both old and new block types
                    match &msg.content {
                        crate::services::message::MessageContent::RenderPendingBorderBlock(
                            tc,
                            auto,
                        ) => {
                            msg.content = crate::services::message::MessageContent::RenderPendingBorderBlockWithStallWarning(tc.clone(), *auto, format!(" {}", stall_message));
                        }
                        crate::services::message::MessageContent::RenderRunCommandBlock(
                            command,
                            result,
                            _run_state,
                        ) => {
                            // Update to RunningWithStallWarning state
                            msg.content = crate::services::message::MessageContent::RenderRunCommandBlock(
                                command.clone(),
                                result.clone(),
                                crate::services::bash_block::RunCommandState::RunningWithStallWarning(stall_message.clone()),
                            );
                        }
                        _ => {}
                    }
                    break;
                }
            }

            invalidate_message_lines_cache(state);
            return None;
        }

        invalidate_message_lines_cache(state);
        return None; // Don't add this marker to the streaming buffer
    }

    // Handle TaskWait progress type specially - use replace mode instead of append
    if matches!(progress.progress_type, Some(ProgressType::TaskWait)) {
        return handle_task_wait_progress(state, progress);
    }

    // Ensure loading state is true during streaming tool results
    // Only set it if it's not already true to avoid unnecessary state changes
    if !state.loading {
        state.loading = true;
    }
    state.is_streaming = true;
    state.streaming_tool_result_id = Some(tool_call_id);
    // 1. Update the buffer for this tool_call_id (append mode for command output)
    state
        .streaming_tool_results
        .entry(tool_call_id)
        .or_default()
        .push_str(&format!("{}\n", progress.message));

    // 2. Check if this is a run_command - get command from the pending message or dialog_command
    let is_run_command = state
        .dialog_command
        .as_ref()
        .map(|tc| crate::utils::strip_tool_name(&tc.function.name) == "run_command")
        .unwrap_or(false);

    let command_str = if is_run_command {
        state
            .dialog_command
            .as_ref()
            .and_then(|tc| extract_command_from_tool_call(tc).ok())
    } else {
        None
    };

    // 3. Remove the pending message with pending_bash_message_id (not the streaming message id)
    if let Some(pending_id) = state.pending_bash_message_id {
        state.messages.retain(|m| m.id != pending_id);
    }
    // Also remove any old streaming message with this id
    state.messages.retain(|m| m.id != tool_call_id);

    // 4. Get the buffer content for rendering (clone to String)
    let buffer_content = state
        .streaming_tool_results
        .get(&tool_call_id)
        .cloned()
        .unwrap_or_default();

    // 5. Use unified run command block for run_command, otherwise use the default streaming block
    if is_run_command {
        let cmd = command_str.unwrap_or_else(|| "command".to_string());
        state.messages.push(Message::render_run_command_block(
            cmd,
            Some(buffer_content),
            crate::services::bash_block::RunCommandState::Running,
            Some(tool_call_id),
        ));
    } else {
        state.messages.push(Message::render_streaming_border_block(
            &buffer_content,
            "Tool Streaming",
            "Result",
            None,
            "Streaming",
            Some(tool_call_id),
        ));
    }
    invalidate_message_lines_cache(state);

    // If content changed while user is scrolled up, mark it
    if !state.stay_at_bottom {
        state.content_changed_while_scrolled_up = true;
    }

    None
}

/// Handle TaskWait progress type with replace mode and dedicated UI
fn handle_task_wait_progress(
    state: &mut AppState,
    progress: ToolCallResultProgress,
) -> Option<String> {
    let tool_call_id = progress.id;

    // Ensure loading state is true
    if !state.loading {
        state.loading = true;
    }
    state.is_streaming = true;
    state.streaming_tool_result_id = Some(tool_call_id);

    // Remove the pending message if exists
    if let Some(pending_id) = state.pending_bash_message_id {
        state.messages.retain(|m| m.id != pending_id);
    }
    // Remove any old message with this id (replace mode)
    state.messages.retain(|m| m.id != tool_call_id);

    // Use structured task updates if available, otherwise fall back to message
    if let Some(task_updates) = progress.task_updates {
        // Extract target task IDs from task updates
        let target_task_ids: Vec<String> = task_updates
            .iter()
            .filter(|t| t.is_target)
            .map(|t| t.task_id.clone())
            .collect();

        // Cache pause info for paused subagent tasks (for approval bar display)
        for task in &task_updates {
            if task.status == "Paused"
                && let Some(pause_info) = &task.pause_info
            {
                state
                    .subagent_pause_info
                    .insert(task.task_id.clone(), pause_info.clone());
            }
        }

        let overall_progress = progress.progress.unwrap_or(0.0);

        // Use dedicated task wait block
        state.messages.push(Message::render_task_wait_block(
            task_updates,
            overall_progress,
            target_task_ids,
            Some(tool_call_id),
        ));
    } else {
        // Fallback: use generic streaming block with replace mode
        // Store message directly (not appending)
        state
            .streaming_tool_results
            .insert(tool_call_id, progress.message.clone());

        state.messages.push(Message::render_streaming_border_block(
            &progress.message,
            "Wait for Tasks",
            "Progress",
            None,
            "TaskWait",
            Some(tool_call_id),
        ));
    }

    invalidate_message_lines_cache(state);

    // If content changed while user is scrolled up, mark it
    if !state.stay_at_bottom {
        state.content_changed_while_scrolled_up = true;
    }

    None
}

/// Handle message tool calls event
pub fn handle_message_tool_calls(state: &mut AppState, tool_calls: Vec<ToolCall>) {
    // Clear the streaming preview block now that tool calls are finalized
    if let Some(preview_id) = state.tool_call_stream_preview_id.take() {
        state.messages.retain(|m| m.id != preview_id);
        invalidate_message_lines_cache(state);
    }

    // exclude any tool call that is already executed
    let rest_tool_calls = tool_calls
        .into_iter()
        .filter(|tool_call| {
            !state.session_tool_calls_queue.contains_key(&tool_call.id)
                || state
                    .session_tool_calls_queue
                    .get(&tool_call.id)
                    .map(|status| status != &ToolCallStatus::Executed)
                    .unwrap_or(false)
        })
        .collect::<Vec<ToolCall>>();

    let prompt_tool_calls = state
        .auto_approve_manager
        .get_prompt_tool_calls(&rest_tool_calls);

    state.message_tool_calls = Some(prompt_tool_calls.clone());

    // Only update last_message_tool_calls if we're not in a retry scenario
    // During retry, we want to preserve the original sequence for ShellCompleted
    if !state.show_shell_mode || state.dialog_command.is_none() {
        state.last_message_tool_calls = prompt_tool_calls.clone();
    }
}

/// Handle streaming tool call progress (tool calls being generated by the LLM)
/// Uses replace-mode: removes old preview and inserts updated one each time.
pub fn handle_stream_tool_call_progress(state: &mut AppState, infos: Vec<ToolCallStreamInfo>) {
    let preview_id = *state
        .tool_call_stream_preview_id
        .get_or_insert_with(uuid::Uuid::new_v4);

    // Ensure loading state
    if !state.loading {
        state.loading = true;
    }
    state.is_streaming = true;

    // Remove old preview message (replace mode)
    state.messages.retain(|m| m.id != preview_id);

    // Add updated preview
    state.messages.push(Message::render_tool_call_stream_block(
        infos,
        Some(preview_id),
    ));

    invalidate_message_lines_cache(state);

    if !state.stay_at_bottom {
        state.content_changed_while_scrolled_up = true;
    }
}

/// Handle retry tool call event
pub fn handle_retry_tool_call(
    state: &mut AppState,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
) {
    if state.latest_tool_call.is_none() {
        return;
    }
    let _ = input_tx.try_send(InputEvent::EmergencyClearTerminal);

    if let Some(cancel_tx) = cancel_tx {
        let _ = cancel_tx.send(());
    }

    if let Some(tool_call) = &state.latest_tool_call {
        // Extract the command from the tool call
        let command = match extract_command_from_tool_call(tool_call) {
            Ok(command) => command,
            Err(_) => {
                return;
            }
        };
        // Enable shell mode and popup
        state.show_shell_mode = true;
        state.shell_popup_visible = true;
        state.shell_popup_expanded = true;
        state.is_dialog_open = false;
        state.ondemand_shell_mode = false;
        state.dialog_command = Some(tool_call.clone());
        if state.shell_tool_calls.is_none() {
            state.shell_tool_calls = Some(Vec::new());
        }

        // Clear any existing shell state
        state.active_shell_command = None;
        state.active_shell_command_output = None;
        state.shell_history_lines.clear(); // Clear history for fresh retry

        // Reset the screen parser with safe dimensions matching PTY (shell.rs)
        let rows = state.terminal_size.height.saturating_sub(2).max(1);
        let cols = state.terminal_size.width.saturating_sub(4).max(1);
        state.shell_screen = vt100::Parser::new(rows, cols, 0);

        // Set textarea shell mode to match app state
        state.text_area.set_shell_mode(true);

        // Automatically execute the command
        let _ = input_tx.try_send(InputEvent::RunShellWithCommand(command));
    }
}

/// Handle retry mechanism
pub fn handle_retry_mechanism(state: &mut AppState) {
    if state.messages.len() >= 2 {
        state.messages.pop();
    }
}

/// Handle interactive stall detection - automatically switch to shell mode and run the command
pub fn handle_interactive_stall_detected(
    state: &mut AppState,
    command: String,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
) {
    // Close any confirmation dialog
    state.is_dialog_open = false;

    // Set up shell mode state
    if let Some(tool_call) = &state.latest_tool_call {
        state.dialog_command = Some(tool_call.clone());
    }
    state.ondemand_shell_mode = false;

    if state.shell_tool_calls.is_none() {
        state.shell_tool_calls = Some(Vec::new());
    }

    // Trigger running the shell with the command - this spawns the user's shell and then executes the command
    let _ = input_tx.try_send(InputEvent::RunShellWithCommand(command));
}

/// Handle toggle approval status event
pub fn handle_toggle_approval_status(state: &mut AppState) {
    state.approval_bar.toggle_selected();
}

/// Handle approval bar next tab event
pub fn handle_approval_popup_next_tab(state: &mut AppState) {
    state.approval_bar.select_next();
}

/// Handle approval bar prev tab event
pub fn handle_approval_popup_prev_tab(state: &mut AppState) {
    state.approval_bar.select_prev();
}

/// Handle approval bar toggle approval event
pub fn handle_approval_popup_toggle_approval(state: &mut AppState) {
    state.approval_bar.toggle_selected();
}

/// Handle approval bar escape event (reject all)
pub fn handle_approval_popup_escape(state: &mut AppState) {
    state.approval_bar.reject_all();
    state.approval_bar.clear();
}

/// Clear streaming tool results
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

/// Update session tool calls queue
pub fn update_session_tool_calls_queue(state: &mut AppState, tool_call_result: &ToolCallResult) {
    if tool_call_result.status == ToolCallResultStatus::Error
        && let Some(failed_idx) = state
            .tool_call_execution_order
            .iter()
            .position(|id| id == &tool_call_result.call.id)
    {
        for id in state.tool_call_execution_order.iter().skip(failed_idx + 1) {
            state
                .session_tool_calls_queue
                .insert(id.clone(), ToolCallStatus::Skipped);
        }
    }
}

/// Execute command palette selection
pub fn execute_command_palette_selection(
    state: &mut AppState,
    input_tx: &Sender<InputEvent>,
    output_tx: &Sender<OutputEvent>,
) {
    let filtered_commands = filter_commands(&state.command_palette_search);
    if filtered_commands.is_empty() || state.command_palette_selected >= filtered_commands.len() {
        return;
    }

    let selected_command = &filtered_commands[state.command_palette_selected];

    // Close command palette
    state.show_command_palette = false;
    state.command_palette_search.clear();

    // Execute the command - use unified executor for slash commands
    if let Some(command_id) = selected_command.action.to_command_id() {
        let ctx = CommandContext {
            state,
            input_tx,
            output_tx,
        };
        if let Err(e) = execute_command(command_id, ctx) {
            push_error_message(state, &e, None);
        }
    } else {
        // Handle non-slash commands (keyboard shortcuts)
        match selected_command.action {
            CommandAction::OpenProfileSwitcher => {
                let _ = input_tx.try_send(InputEvent::ShowProfileSwitcher);
            }
            CommandAction::OpenRulebookSwitcher => {
                let _ = input_tx.try_send(InputEvent::ShowRulebookSwitcher);
            }
            CommandAction::OpenShortcuts => {
                let _ = input_tx.try_send(InputEvent::ShowShortcuts);
            }
            CommandAction::OpenShellMode => {
                let _ = input_tx.try_send(InputEvent::ShellMode);
            }
            _ => {
                // Should not happen - all slash commands should be handled above
            }
        }
        state.text_area.set_text("");
        state.show_helper_dropdown = false;
    }
}

/// Handle completed tool result event
pub fn handle_tool_result(state: &mut AppState, result: ToolCallResult) {
    use crate::services::changeset::FileEdit;

    // Only process successful tool calls
    if !matches!(result.status, ToolCallResultStatus::Success)
        || result.result.contains("TOOL_CALL_REJECTED")
    {
        return;
    }

    let function_name = result.call.function.name.as_str();
    let args_str = &result.call.function.arguments;

    // Parse arguments
    let args: serde_json::Value = match serde_json::from_str(args_str) {
        Ok(v) => v,
        Err(_) => return, // Should not happen if tool call was successful
    };

    // Normalize/Strip tool name for checking
    let tool_name_stripped = crate::utils::strip_tool_name(function_name);

    match tool_name_stripped {
        "write_to_file" | "create" | "create_file" => {
            if let Some(path) = args
                .get("TargetFile")
                .or(args.get("path"))
                .and_then(|v| v.as_str())
            {
                let code_content = args
                    .get("CodeContent")
                    .or(args.get("content"))
                    .or(args.get("file_content"))
                    .or(args.get("body"))
                    .or(args.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let is_overwrite = args
                    .get("Overwrite")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                // If no content in args but file exists, read from disk to count lines
                let line_count = if code_content.is_empty() {
                    std::fs::read_to_string(path)
                        .map(|content| content.lines().count().max(1)) // At least 1 line for non-empty files
                        .unwrap_or(0)
                } else {
                    code_content.lines().count()
                };

                let summary = if is_overwrite {
                    "Overwrote file"
                } else {
                    "Created file"
                };

                let edit = FileEdit::new(summary.to_string())
                    .with_stats(line_count, 0)
                    .with_tool_call(result.call.clone());

                state.changeset.track_file(path, edit);

                // If file does not exist, mark it as Deleted immediately
                if !std::path::Path::new(path).exists() {
                    state.changeset.mark_removed(path, None);
                }
            }
        }
        "replace_file_content" | "multi_replace_file_content" | "str_replace" | "edit_file" => {
            if let Some(path) = args
                .get("TargetFile")
                .or(args.get("path"))
                .and_then(|v| v.as_str())
            {
                // For str_replace, check if the changes are still present in the file
                // This prevents tracking reverted or manually edited files
                if tool_name_stripped == "str_replace"
                    && let Some(new_str) = args.get("new_str").and_then(|v| v.as_str())
                    && let Ok(current_content) = std::fs::read_to_string(path)
                    && !current_content.contains(new_str)
                {
                    // File was reverted or manually edited, don't track it
                    return;
                }

                // Parse diff from the result message
                let (added, removed) = parse_diff_stats(&result.result);

                let summary = if tool_name_stripped == "replace_file_content"
                    || tool_name_stripped == "str_replace"
                {
                    "Edited file"
                } else {
                    "Multi-edit file"
                };

                // Extract diff preview - first few lines of the diff block
                let diff_preview = extract_diff_preview(&result.result);

                let mut edit = FileEdit::new(summary.to_string())
                    .with_stats(added, removed)
                    .with_tool_call(result.call.clone());

                if let Some(preview) = diff_preview {
                    edit = edit.with_diff_preview(preview);
                }

                state.changeset.track_file(path, edit);

                // If file does not exist, mark it as Deleted immediately
                if !std::path::Path::new(path).exists() {
                    state.changeset.mark_removed(path, None);
                }
            }
        }
        "remove_file" | "delete_file" | "stakpak__remove" | "remove" => {
            // Assuming remove_file takes "path" or "TargetFile"
            if let Some(path) = args
                .get("path")
                .or(args.get("TargetFile"))
                .and_then(|v| v.as_str())
            {
                // Extract backup path from result.result if available
                let backup_path = extract_backup_path(&result.result);

                state.changeset.mark_removed(path, backup_path);
            }
        }
        _ => {}
    }
}

/// Extract backup path from the XML output
fn extract_backup_path(result: &str) -> Option<String> {
    // Look for backup_path="..." in the result string
    // Format: backup_path="/path/to/backup/file"
    if let Some(start_idx) = result.find("backup_path=\"") {
        let after_start = &result[start_idx + "backup_path=\"".len()..];
        if let Some(end_idx) = after_start.find('"') {
            return Some(after_start[..end_idx].to_string());
        }
    }
    None
}

/// Parse added/removed lines from a diff string
fn parse_diff_stats(message: &str) -> (usize, usize) {
    let mut added = 0;
    let mut removed = 0;
    let mut in_diff_block = false;

    for line in message.lines() {
        if line.trim().starts_with("```diff") {
            in_diff_block = true;
            continue;
        }
        if line.trim().starts_with("```") && in_diff_block {
            in_diff_block = false;
            continue;
        }

        if in_diff_block {
            // Skip diff headers
            if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@") {
                continue;
            }

            if line.starts_with('+') {
                added += 1;
            } else if line.starts_with('-') {
                removed += 1;
            }
        }
    }

    (added, removed)
}

/// Extract the first few lines of the diff for preview
fn extract_diff_preview(message: &str) -> Option<String> {
    let mut lines = Vec::new();
    let mut in_diff_block = false;

    for line in message.lines() {
        if line.trim().starts_with("```diff") {
            in_diff_block = true;
            continue;
        }
        if line.trim().starts_with("```") && in_diff_block {
            break;
        }

        if in_diff_block {
            lines.push(line);
            if lines.len() >= 5 {
                // Keep only first 5 lines
                break;
            }
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

/// Extract view tool parameters (path, grep, glob) from a tool call
pub fn extract_view_params_from_tool_call(
    tool_call: &ToolCall,
) -> (Option<String>, Option<String>, Option<String>) {
    // Try to parse arguments as JSON
    if let Ok(args) = serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments) {
        let path = args
            .get("path")
            .or(args.get("filePath"))
            .or(args.get("file_path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let grep = args
            .get("grep")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let glob = args
            .get("glob")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        return (path, grep, glob);
    }

    (None, None, None)
}

// ========== Approval Bar Handlers ==========

/// Handle approve all in approval bar
pub fn handle_approval_bar_approve_all(state: &mut AppState) {
    state.approval_bar.approve_all();
}

/// Handle reject all in approval bar
pub fn handle_approval_bar_reject_all(state: &mut AppState) {
    state.approval_bar.reject_all();
}

/// Handle select action by index (1-based)
pub fn handle_approval_bar_select_action(state: &mut AppState, index: usize) {
    let old_index = state.approval_bar.selected_index();
    state.approval_bar.select_action(index);
    if old_index != state.approval_bar.selected_index() {
        update_pending_tool_display(state);
    }
}

/// Handle approve selected action
pub fn handle_approval_bar_approve_selected(state: &mut AppState) {
    state.approval_bar.approve_selected();
}

/// Handle reject selected action
pub fn handle_approval_bar_reject_selected(state: &mut AppState) {
    state.approval_bar.reject_selected();
}

/// Handle toggle selected action (space key)
pub fn handle_approval_bar_toggle_selected(state: &mut AppState, _input_tx: &Sender<InputEvent>) {
    state.approval_bar.toggle_selected();
    // Update the display to reflect the new status
    update_pending_tool_display(state);
}

/// Handle next action navigation (right arrow)
pub fn handle_approval_bar_next_action(state: &mut AppState, _input_tx: &Sender<InputEvent>) {
    state.approval_bar.select_next();
    update_pending_tool_display(state);
}

/// Handle prev action navigation (left arrow)
pub fn handle_approval_bar_prev_action(state: &mut AppState, _input_tx: &Sender<InputEvent>) {
    state.approval_bar.select_prev();
    update_pending_tool_display(state);
}

/// Handle collapse/escape
pub fn handle_approval_bar_collapse(state: &mut AppState) {
    // Reject all pending tools and clear
    state.approval_bar.reject_all();
    state.approval_bar.clear();
}

/// Update the pending tool display in messages area based on selected tab
fn update_pending_tool_display(state: &mut AppState) {
    // Remove any existing pending tool block
    if let Some(pending_id) = state.pending_bash_message_id {
        state.messages.retain(|m| m.id != pending_id);
    }

    // Create a new pending block for the currently selected tool
    create_pending_block_for_selected_tool(state);

    // Force-invalidate cache — bypass the streaming guard in invalidate_message_lines_cache
    // because the user is actively cycling through tool call tabs and needs to see the
    // updated preview immediately, even if is_streaming is still true from the LLM stream.
    state.assembled_lines_cache = None;
    state.visible_lines_cache = None;
    state.message_lines_cache = None;
    state.collapsed_message_lines_cache = None;

    // Don't auto-scroll - let user control scroll position
    state.stay_at_bottom = false;
}

/// Create a pending block in messages for the currently selected tool in the approval bar.
/// Sets `pending_bash_message_id` and `dialog_command` to match the selected tool.
/// Does NOT remove any existing pending block — caller is responsible for cleanup.
pub fn create_pending_block_for_selected_tool(state: &mut AppState) {
    // Get the currently selected tool call
    if let Some(action) = state.approval_bar.selected_action() {
        let tool_call = &action.tool_call;
        let tool_name = crate::utils::strip_tool_name(&tool_call.function.name);

        // Determine the approval state for display
        let auto_approve = action.status == crate::services::approval_bar::ApprovalStatus::Approved;

        // Create the appropriate pending block based on tool type
        if tool_name == "run_command" {
            let command = super::shell::extract_command_from_tool_call(tool_call)
                .unwrap_or_else(|_| "unknown command".to_string());

            let run_state = match action.status {
                crate::services::approval_bar::ApprovalStatus::Approved => {
                    crate::services::bash_block::RunCommandState::Pending // Still pending execution
                }
                crate::services::approval_bar::ApprovalStatus::Rejected => {
                    crate::services::bash_block::RunCommandState::Rejected
                }
            };

            let msg = Message::render_run_command_block(command, None, run_state, None);
            state.pending_bash_message_id = Some(msg.id);
            state.messages.push(msg);
        } else if tool_name == "resume_subagent_task" {
            // For resume_subagent_task, use the special subagent pending block
            let pause_info =
                serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments)
                    .ok()
                    .and_then(|args| {
                        args.get("task_id")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    })
                    .and_then(|task_id| state.subagent_pause_info.get(&task_id).cloned());

            let msg = Message::render_subagent_resume_pending_block(
                tool_call.clone(),
                auto_approve,
                pause_info,
                None,
            );
            state.pending_bash_message_id = Some(msg.id);
            state.messages.push(msg);
        } else {
            // For other tools, use the standard pending block
            let msg = Message::render_pending_border_block(tool_call.clone(), auto_approve, None);
            state.pending_bash_message_id = Some(msg.id);
            state.messages.push(msg);
        }

        // Update dialog_command to the selected tool
        state.dialog_command = Some(tool_call.clone());
    }
}
