use super::message::extract_truncated_command_arguments;
use crate::app::{AppState, InputEvent, OutputEvent, ToolCallStatus};
use crate::constants::{
    CONTEXT_MAX_UTIL_TOKENS, CONTEXT_MAX_UTIL_TOKENS_ECO, SUMMARIZE_PROMPT_BASE,
};
use crate::services::approval_popup::PopupService;
use crate::services::auto_approve::AutoApprovePolicy;
use crate::services::bash_block::{preprocess_terminal_output, render_bash_block_rejected};
use crate::services::detect_term::AdaptiveColors;
use crate::services::file_search::{handle_file_selection, handle_tab_trigger};
use crate::services::helper_block::{
    handle_errors, push_clear_message, push_error_message, push_help_message, push_issue_message,
    push_memorize_message, push_model_message, push_status_message, push_styled_message,
    push_support_message, push_usage_message, render_system_message, welcome_messages,
};
use crate::services::message::{
    Message, MessageContent, get_command_type_name, get_wrapped_collapsed_message_lines_cached,
    get_wrapped_message_lines_cached,
};
use crate::services::shell_mode::SHELL_PROMPT_PREFIX;
use ratatui::layout::Size;
use ratatui::style::{Color, Style};
use serde_json;
use stakpak_shared::helper::truncate_output;
use stakpak_shared::models::integrations::openai::{
    AgentModel, FunctionCall, ToolCall, ToolCallResult, ToolCallResultProgress,
    ToolCallResultStatus,
};
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

const SCROLL_LINES: usize = 1;
const MAX_PASTE_CHAR_COUNT: usize = 1000;

/// Groups related event channel senders together to reduce function parameter counts
struct EventChannels<'a> {
    output_tx: &'a Sender<OutputEvent>,
    input_tx: &'a Sender<InputEvent>,
    shell_tx: &'a Sender<InputEvent>,
}

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
    terminal_size: Size,
) {
    // Block all input during profile switch EXCEPT profile switch events and Quit
    if state.is_input_blocked() {
        match event {
            InputEvent::ProfilesLoaded(_, _)
            | InputEvent::ProfileSwitchRequested(_)
            | InputEvent::ProfileSwitchProgress(_)
            | InputEvent::ProfileSwitchComplete(_)
            | InputEvent::ProfileSwitchFailed(_)
            | InputEvent::RulebooksLoaded(_)
            | InputEvent::CurrentRulebooksLoaded(_)
            | InputEvent::Quit
            | InputEvent::AttemptQuit => {
                // Allow these events through
            }
            _ => {
                // Block everything else
                return;
            }
        }
    }

    state.scroll = state.scroll.max(0);
    match event {
        InputEvent::Up => {
            handle_up_navigation(state);
        }
        InputEvent::Down => {
            handle_down_navigation(state, message_area_height, message_area_width);
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

        InputEvent::InputChanged(c) => {
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
        InputEvent::InputBackspace => {
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

        // Handle rulebook switcher first to avoid race conditions
        InputEvent::ShowRulebookSwitcher => {
            // Don't show rulebook switcher if input is blocked or dialog is open
            if state.profile_switching_in_progress
                || state.is_dialog_open
                || state.approval_popup.is_visible()
            {
                return;
            }

            // Clear any pending input to prevent empty message submission
            state.text_area.set_text("");

            // Request current active rulebooks to pre-select them
            let _ = output_tx.try_send(OutputEvent::RequestCurrentRulebooks);

            state.show_rulebook_switcher = true;
            state.rulebook_switcher_selected = 0;
            state.rulebook_search_input.clear();
            filter_rulebooks(state);
        }

        InputEvent::InputSubmitted => {
            if state.show_profile_switcher {
                let _ = input_tx.try_send(InputEvent::ProfileSwitcherSelect);
                return;
            }
            if state.show_command_palette {
                // Execute the selected command
                execute_command_palette_selection(state, input_tx, output_tx);
                return;
            }
            if state.show_rulebook_switcher {
                let _ = input_tx.try_send(InputEvent::RulebookSwitcherConfirm);
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
                                let _ = output_tx
                                    .try_send(OutputEvent::AcceptTool(dialog_command.clone()));
                            } else {
                                // Fire handle reject
                                let _ =
                                    input_tx.try_send(InputEvent::HandleReject(None, true, None));
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
        InputEvent::StreamUsage(usage) => {
            state.current_message_usage = usage;
        }
        InputEvent::RequestTotalUsage => {
            // Request total usage from CLI
            let _ = output_tx.try_send(OutputEvent::RequestTotalUsage);
        }
        InputEvent::TotalUsage(usage) => {
            // Update total session usage from CLI
            state.total_session_usage = usage;
            // If cost message was just displayed, update it
            let should_update = state
                .messages
                .last()
                .and_then(|msg| {
                    if let MessageContent::StyledBlock(lines) = &msg.content {
                        lines
                            .first()
                            .and_then(|l| l.spans.first())
                            .map(|s| s.content.contains("Token Usage & Costs"))
                    } else {
                        None
                    }
                })
                .unwrap_or(false);

            if should_update {
                state.messages.pop(); // Remove old message
                crate::services::helper_block::push_usage_message(state);
            }
        }
        InputEvent::HasUserMessage => {
            state.has_user_messages = true;
            state.toggle_approved_message = true;
            state.message_approved_tools.clear();
            state.message_rejected_tools.clear();
            state.message_tool_calls = None;
            state.tool_call_execution_order.clear();
            state.is_dialog_open = false;
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
        InputEvent::ScrollUp => handle_up_navigation(state),
        InputEvent::ScrollDown => {
            handle_down_navigation(state, message_area_height, message_area_width)
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
                let rendered_lines =
                    render_bash_block_rejected("Interrupted by user", "System", None, None);
                state.messages.push(Message {
                    id: Uuid::new_v4(),
                    content: MessageContent::StyledBlock(rendered_lines),
                    is_collapsed: None,
                });
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
            if state.approval_popup.is_visible() {
                state.approval_popup.prev_tab();
                return; // Event was consumed by popup
            }
            state.text_area.move_cursor_left();
        }
        InputEvent::CursorRight => {
            if state.approval_popup.is_visible() {
                state.approval_popup.next_tab();
                return; // Event was consumed by popup
            }
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

        InputEvent::HandleReject(message, should_stop, color) => {
            let channels = EventChannels {
                output_tx,
                input_tx,
                shell_tx,
            };
            handle_esc(state, &channels, cancel_tx, message, should_stop, color);
        }

        InputEvent::ShowConfirmationDialog(tool_call) => {
            if state.latest_tool_call.is_some() && state.show_shell_mode {
                return;
            }
            if state
                .session_tool_calls_queue
                .get(&tool_call.id)
                .map(|status| status == &ToolCallStatus::Executed)
                .unwrap_or(false)
            {
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
                state.is_dialog_open = false;
                state.dialog_command = None;
                return;
            }

            state.dialog_command = Some(tool_call.clone());
            if tool_call.function.name == "run_command" {
                state.latest_tool_call = Some(tool_call.clone());
            }
            let is_auto_approved = state.auto_approve_manager.should_auto_approve(&tool_call);

            if tool_call.function.name == "str_replace" || tool_call.function.name == "create" {
                state
                    .messages
                    .push(Message::render_collapsed_message(tool_call.clone()));
            }

            // Tool call is pending - create pending border block and check if we should show popup
            let message_id = Uuid::new_v4();
            state.messages.push(Message::render_pending_border_block(
                tool_call.clone(),
                is_auto_approved,
                Some(message_id),
            ));
            state.pending_bash_message_id = Some(message_id);

            state.dialog_command = Some(tool_call.clone());
            state.is_dialog_open = true;
            state.loading = false;
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

            // Tool call is pending - check if we should show popup first
            if !tool_calls.is_empty() && state.toggle_approved_message {
                state.approval_popup =
                    PopupService::new_with_tool_calls(tool_calls.clone(), terminal_size);
                return;
            }
        }

        InputEvent::MessageToolCalls(tool_calls) => {
            // execlude any tool call that is already executed
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
        InputEvent::ToggleMoreShortcuts => {
            state.show_shortcuts = !state.show_shortcuts;
        }
        InputEvent::HandleCtrlS => {
            if state.show_rulebook_switcher {
                let _ = input_tx.try_send(InputEvent::RulebookSwitcherSelectAll);
                return;
            }
            let _ = input_tx.try_send(InputEvent::ShowShortcuts);
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
        InputEvent::HandleEsc => {
            if state.show_context_popup {
                state.show_context_popup = false;
                return;
            }
            if state.show_rulebook_switcher {
                state.show_rulebook_switcher = false;
                return;
            }
            if state.show_command_palette {
                state.show_command_palette = false;
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
            if state.approval_popup.is_visible() {
                state.approval_popup.escape();
                state.toggle_approved_message = false;
                return; // TODO: either reject all or add a toggle event to toggle back the popup.
            } else {
                state.message_rejected_tools = state
                    .approval_popup
                    .get_approved_tool_calls()
                    .into_iter()
                    .cloned()
                    .collect();
                state.message_approved_tools.clear();
                state.message_tool_calls = None;
                state.tool_call_execution_order.clear();
                // Store the latest tool call for potential retry (only for run_command)
                if let Some(tool_call) = &state.dialog_command
                    && tool_call.function.name == "run_command"
                {
                    state.latest_tool_call = Some(tool_call.clone());
                }

                let channels = EventChannels {
                    output_tx,
                    input_tx,
                    shell_tx,
                };
                handle_esc(state, &channels, cancel_tx, None, true, None);
            }
        }

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
        InputEvent::ToggleApprovalStatus => {
            state.approval_popup.toggle_approval_status();
        }
        InputEvent::SetSessions(sessions) => {
            state.sessions = sessions;
            state.show_sessions_dialog = true;
        }
        InputEvent::ShellOutput(line) => {
            let mut line = line.replace("\r\n", "\n").replace('\r', "\n");

            if let Some(output) = state.active_shell_command_output.as_mut() {
                let text = format!("{}\n", line);
                output.push_str(&text);
                *output = truncate_output(output);
            }

            line = truncate_output(&line);
            state
                .messages
                .push(Message::render_escaped_text_block(line));
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
            if let Some(cmd) = &state.active_shell_command
                && let Err(_e) = cmd.kill()
            {}
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
        InputEvent::Resized(width, height) => {
            let old_terminal_size = state.terminal_size;
            state.terminal_size = Size { width, height };

            // Recreate the approval popup if it's visible and terminal size changed
            if state.approval_popup.is_visible() && old_terminal_size != state.terminal_size {
                state
                    .approval_popup
                    .recreate_with_terminal_size(state.terminal_size);
            }
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
            // If approval popup is visible, toggle its maximize state instead
            if state.approval_popup.is_visible() {
                state.approval_popup.toggle_maximize();
                return;
            }

            // Otherwise, handle collapsed messages popup as usual
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
        InputEvent::ToggleContextPopup => {
            state.show_context_popup = !state.show_context_popup;
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

        // Profile switcher events
        InputEvent::ProfilesLoaded(profiles, _current_profile) => {
            // Only update the available profiles list
            // Do NOT update current_profile_name - it's already set correctly when TUI starts
            state.available_profiles = profiles;
        }

        InputEvent::ShowProfileSwitcher => {
            // Don't show profile switcher if input is blocked or dialog is open
            if state.profile_switching_in_progress
                || state.is_dialog_open
                || state.approval_popup.is_visible()
            {
                return;
            }

            state.show_profile_switcher = true;
            state.profile_switcher_selected = 0;

            // Pre-select current profile
            if let Some(idx) = state
                .available_profiles
                .iter()
                .position(|p| p == &state.current_profile_name)
            {
                state.profile_switcher_selected = idx;
            }
        }

        InputEvent::ShowCommandPalette => {
            // Don't show command palette if input is blocked or dialog is open
            if state.profile_switching_in_progress
                || state.is_dialog_open
                || state.approval_popup.is_visible()
            {
                return;
            }

            state.show_command_palette = true;
            state.command_palette_selected = 0;
            state.command_palette_scroll = 0;
            state.command_palette_search = String::new();
        }

        InputEvent::ShowShortcuts => {
            // Don't show shortcuts popup if input is blocked or dialog is open
            if state.profile_switching_in_progress
                || state.is_dialog_open
                || state.approval_popup.is_visible()
                || state.show_profile_switcher
            {
                return;
            }

            state.show_shortcuts_popup = true;
        }

        InputEvent::ProfileSwitcherSelect => {
            // Don't process if switching is already in progress
            if state.profile_switching_in_progress {
                return;
            }

            if state.show_profile_switcher && !state.available_profiles.is_empty() {
                let selected_profile =
                    state.available_profiles[state.profile_switcher_selected].clone();

                // Don't switch if already on this profile
                if selected_profile == state.current_profile_name {
                    state.show_profile_switcher = false;
                    return;
                }

                // Send request to switch profile
                let _ = output_tx.try_send(OutputEvent::RequestProfileSwitch(selected_profile));
            }
        }

        InputEvent::ProfileSwitcherCancel => {
            state.show_profile_switcher = false;
        }

        InputEvent::ShortcutsCancel => {
            state.show_shortcuts_popup = false;
        }

        InputEvent::ProfileSwitchRequested(ref profile) => {
            state.profile_switching_in_progress = true;
            state.show_profile_switcher = false;

            // Clear profile switcher state immediately to prevent stray selects
            state.profile_switcher_selected = 0;

            state.profile_switch_status_message =
                Some(format!("🔄 Switching to profile: {}", profile));

            state.messages.push(Message::info(
                format!("🔄 Switching to profile: {}", profile),
                None,
            ));
        }

        InputEvent::ProfileSwitchProgress(ref message) => {
            state.profile_switch_status_message = Some(message.clone());
            state.messages.push(Message::info(message.clone(), None));
        }

        InputEvent::ProfileSwitchComplete(ref profile) => {
            // Clear EVERYTHING
            state.messages.clear();
            state.session_tool_calls_queue.clear();
            state.completed_tool_calls.clear();
            state.streaming_tool_results.clear();
            state.active_shell_command = None;
            state.shell_tool_calls = None;
            state.message_tool_calls = None;
            state.message_approved_tools.clear();
            state.message_rejected_tools.clear();
            state.has_user_messages = false;
            state.scroll = 0;
            state.scroll_to_bottom = true;
            state.stay_at_bottom = true;
            state.tool_call_execution_order.clear();
            state.last_message_tool_calls.clear();

            // Clear shell mode state
            state.show_shell_mode = false;
            state.shell_mode_input.clear();
            state.waiting_for_shell_input = false;
            state.active_shell_command_output = None;
            state.is_tool_call_shell_command = false;
            state.ondemand_shell_mode = false;

            // Clear file search
            state.filtered_files.clear();

            // Clear dialog state
            state.is_dialog_open = false;
            state.dialog_command = None;
            state.show_sessions_dialog = false;
            state.show_shortcuts = false;
            state.show_collapsed_messages = false;
            state.approval_popup = PopupService::new();

            // Clear retry state
            state.retry_attempts = 0;
            state.last_user_message_for_retry = None;
            state.is_retrying = false;

            // CRITICAL: Close profile switcher to prevent stray selects
            state.show_profile_switcher = false;
            state.profile_switcher_selected = 0;

            // Update profile info
            state.current_profile_name = profile.clone();
            state.profile_switching_in_progress = false;
            state.profile_switch_status_message = None;

            // Show success and welcome messages
            state.messages.push(Message::info(
                format!("✅ Successfully switched to profile: {}", profile),
                Some(Style::default().fg(AdaptiveColors::green())),
            ));

            let welcome_msg = welcome_messages(state.latest_version.clone(), state);
            state.messages.extend(welcome_msg);

            // Invalidate all caches
            crate::services::message::invalidate_message_lines_cache(state);
        }

        InputEvent::ProfileSwitchFailed(ref error) => {
            state.profile_switching_in_progress = false;
            state.profile_switch_status_message = None;
            state.show_profile_switcher = false;

            state.messages.push(Message::info(
                format!("❌ Profile switch failed: {}", error),
                Some(Style::default().fg(AdaptiveColors::red())),
            ));
            state.messages.push(Message::info(
                "Staying in current profile. Press Ctrl+P to try again.",
                None,
            ));
        }

        // Rulebook switcher events
        InputEvent::RulebooksLoaded(rulebooks) => {
            state.available_rulebooks = rulebooks;
            filter_rulebooks(state);
        }

        InputEvent::CurrentRulebooksLoaded(current_uris) => {
            // Set the currently active rulebooks as selected
            state.selected_rulebooks = current_uris.into_iter().collect();
        }

        InputEvent::RulebookSwitcherSelect => {
            if state.show_rulebook_switcher && !state.filtered_rulebooks.is_empty() {
                let selected_rulebook = &state.filtered_rulebooks[state.rulebook_switcher_selected];

                // Toggle selection
                if state.selected_rulebooks.contains(&selected_rulebook.uri) {
                    state.selected_rulebooks.remove(&selected_rulebook.uri);
                } else {
                    state
                        .selected_rulebooks
                        .insert(selected_rulebook.uri.clone());
                }
            }
        }

        InputEvent::RulebookSwitcherToggle => {
            if state.show_rulebook_switcher && !state.filtered_rulebooks.is_empty() {
                let selected_rulebook = &state.filtered_rulebooks[state.rulebook_switcher_selected];

                // Toggle selection
                if state.selected_rulebooks.contains(&selected_rulebook.uri) {
                    state.selected_rulebooks.remove(&selected_rulebook.uri);
                } else {
                    state
                        .selected_rulebooks
                        .insert(selected_rulebook.uri.clone());
                }
            }
        }

        InputEvent::RulebookSwitcherCancel => {
            state.show_rulebook_switcher = false;
        }

        InputEvent::RulebookSwitcherConfirm => {
            if state.show_rulebook_switcher {
                // Send the selected rulebooks to the CLI
                let selected_uris: Vec<String> = state.selected_rulebooks.iter().cloned().collect();
                let _ = output_tx.try_send(OutputEvent::RequestRulebookUpdate(selected_uris));

                // Close the switcher
                state.show_rulebook_switcher = false;

                // Show confirmation message
                let count = state.selected_rulebooks.len();
                state.messages.push(Message::info(
                    format!(
                        "Selected {} rulebook(s). They will be applied to your next message.",
                        count
                    ),
                    Some(Style::default().fg(AdaptiveColors::green())),
                ));
            }
        }

        InputEvent::RulebookSwitcherSelectAll => {
            if state.show_rulebook_switcher {
                // Select all filtered rulebooks
                state.selected_rulebooks.clear();
                for rulebook in &state.filtered_rulebooks {
                    state.selected_rulebooks.insert(rulebook.uri.clone());
                }
            }
        }

        InputEvent::RulebookSwitcherDeselectAll => {
            if state.show_rulebook_switcher {
                // Deselect all rulebooks
                state.selected_rulebooks.clear();
            }
        }

        InputEvent::RulebookSearchInputChanged(c) => {
            if state.show_rulebook_switcher {
                state.rulebook_search_input.push(c);
                filter_rulebooks(state);
            }
        }

        InputEvent::RulebookSearchBackspace => {
            if state.show_rulebook_switcher && !state.rulebook_search_input.is_empty() {
                state.rulebook_search_input.pop();
                filter_rulebooks(state);
            }
        }

        InputEvent::CommandPaletteSearchInputChanged(c) => {
            if state.show_command_palette {
                state.command_palette_search.push(c);
                state.command_palette_selected = 0;
            }
        }

        InputEvent::CommandPaletteSearchBackspace => {
            if state.show_command_palette && !state.command_palette_search.is_empty() {
                state.command_palette_search.pop();
                state.command_palette_selected = 0;
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
                Err(_) => {
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
        if let Some(latest_tool_call) = &state.latest_tool_call
            && let Some(dialog_command) = &state.dialog_command
            && latest_tool_call.id != dialog_command.id
        {
            state.is_dialog_open = true;
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
            state.helper_scroll = 0;
            return;
        }
        return;
    }
    // Trigger file file_search with Tab
    handle_tab_trigger(state);
}

/// Updates helper dropdown scroll position to keep selected item visible
fn update_helper_dropdown_scroll(state: &mut AppState) {
    let commands_to_show = if state.input() == "/" {
        &state.helpers
    } else {
        &state.filtered_helpers
    };

    let total_commands = commands_to_show.len();
    if total_commands == 0 {
        return;
    }

    const MAX_VISIBLE_ITEMS: usize = 5;
    let visible_height = MAX_VISIBLE_ITEMS.min(total_commands);

    // Calculate the scroll position to keep the selected item visible
    if state.helper_selected < state.helper_scroll {
        // Selected item is above visible area, scroll up
        state.helper_scroll = state.helper_selected;
    } else if state.helper_selected >= state.helper_scroll + visible_height {
        // Selected item is below visible area, scroll down
        state.helper_scroll = state.helper_selected - visible_height + 1;
    }

    // Ensure scroll doesn't go beyond bounds
    let max_scroll = total_commands.saturating_sub(visible_height);
    if state.helper_scroll > max_scroll {
        state.helper_scroll = max_scroll;
    }
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
                update_helper_dropdown_scroll(state);
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
            let commands_to_show = if state.input() == "/" {
                &state.helpers
            } else {
                &state.filtered_helpers
            };

            if !commands_to_show.is_empty()
                && state.input().starts_with('/')
                && state.helper_selected + 1 < commands_to_show.len()
            {
                state.helper_selected += 1;
                update_helper_dropdown_scroll(state);
            }
        }
    }
}

fn handle_input_changed(state: &mut AppState, c: char) {
    state.show_shortcuts = false;

    if c == '$' && (state.input().is_empty() || state.is_dialog_open) && !state.show_sessions_dialog
    {
        state.text_area.set_text("");
        handle_shell_mode(state);
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

fn handle_input_backspace(state: &mut AppState) {
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

fn handle_esc(
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
            let _ = channels
                .output_tx
                .try_send(OutputEvent::RejectTool(tool_call.clone(), should_stop));
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
        state.is_dialog_open = false;
        state.dialog_command = None;
        state.dialog_focused = false; // Reset focus when dialog closes
        state.text_area.set_text("");
    } else if state.show_shell_mode {
        if state.active_shell_command.is_some() {
            let _ = channels.shell_tx.try_send(InputEvent::ShellKill);
        }
        state.show_shell_mode = false;
        state.text_area.set_shell_mode(false);
        state.text_area.set_text("");
        if state.dialog_command.is_some() {
            state.dialog_command = None;
        }
    } else {
        state.text_area.set_text("");
    }

    state.messages.retain(|m| {
        m.id != state.streaming_tool_result_id.unwrap_or_default()
            && m.id != state.pending_bash_message_id.unwrap_or_default()
    });
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
            let selected = &state.filtered_helpers[state.helper_selected];

            match selected.command {
                "/sessions" => {
                    let _ = output_tx.try_send(OutputEvent::ListSessions);
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                }
                "/resume" => {
                    resume_session(state, output_tx);
                }
                "/new" => {
                    new_session(state, output_tx);
                }
                "/clear" => {
                    push_clear_message(state);
                }
                "/memorize" => {
                    push_memorize_message(state);
                    let _ = output_tx.try_send(OutputEvent::Memorize);
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                }
                "/summarize" => {
                    let prompt = build_summarize_prompt(state);
                    state.messages.push(Message::info("".to_string(), None));
                    state.messages.push(Message::info(
                        "Requesting session summary (summary.md)...",
                        Some(Style::default().fg(Color::Cyan)),
                    ));
                    let _ = output_tx.try_send(OutputEvent::UserMessage(
                        prompt.clone(),
                        state.shell_tool_calls.clone(),
                    ));
                    state.shell_tool_calls = None;
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                }
                "/usage" => {
                    push_usage_message(state);
                    let _ = output_tx.try_send(OutputEvent::RequestTotalUsage);
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                }
                "/help" => {
                    push_help_message(state);
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                }
                "/model" => {
                    match switch_model(state) {
                        Ok(()) => {
                            let _ =
                                output_tx.try_send(OutputEvent::SwitchModel(state.model.clone()));
                            push_model_message(state);
                        }
                        Err(e) => {
                            push_error_message(state, &e, None);
                        }
                    }
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                }
                "/status" => {
                    push_status_message(state);
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                }
                "/issue" => {
                    push_issue_message(state);
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                }
                "/support" => {
                    push_support_message(state);
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
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
                }
                "/profiles" => {
                    state.show_profile_switcher = true;
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                }
                "/list_approved_tools" => {
                    list_auto_approved_tools(state);
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                }
                "/mouse_capture" => {
                    // Toggle mouse capture using shared function
                    #[cfg(unix)]
                    let _ = crate::toggle_mouse_capture(state);

                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
                }
                "/shortcuts" => {
                    state.show_shortcuts_popup = true;
                    state.text_area.set_text("");
                    state.show_helper_dropdown = false;
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

fn build_summarize_prompt(state: &AppState) -> String {
    let usage = &state.total_session_usage;
    let total_tokens = usage.total_tokens;
    let prompt_tokens = usage.prompt_tokens;
    let completion_tokens = usage.completion_tokens;
    let context_usage_pct = if CONTEXT_MAX_UTIL_TOKENS > 0 {
        (total_tokens as f64 / CONTEXT_MAX_UTIL_TOKENS as f64) * 100.0
    } else {
        0.0
    };

    let recent_inputs = collect_recent_user_inputs(state, 6);

    let mut prompt = String::from(SUMMARIZE_PROMPT_BASE);
    prompt.push('\n');
    prompt.push_str("Session snapshot:\n");
    prompt.push_str(&format!(
        "- Active profile: {}\n",
        state.current_profile_name
    ));
    prompt.push_str(&format!(
        "- Total tokens used: {} (prompt: {}, completion: {})\n",
        total_tokens, prompt_tokens, completion_tokens
    ));
    prompt.push_str(&format!(
        "- Context window usage: {:.1}% of {} tokens\n",
        context_usage_pct.min(100.0),
        CONTEXT_MAX_UTIL_TOKENS
    ));
    if !recent_inputs.is_empty() {
        prompt.push('\n');
        prompt.push_str("Recent user inputs to emphasize:\n");
        for input in recent_inputs {
            prompt.push_str("- ");
            prompt.push_str(&input);
            prompt.push('\n');
        }
    }
    prompt.push('\n');
    prompt.push_str(
        "Be precise, note outstanding TODOs or follow-ups, and reflect any cost or context considerations mentioned earlier.\n",
    );
    prompt.push_str(
        "When ready, create or overwrite `summary.md` using the tool call and populate it with the markdown summary.\n",
    );

    prompt
}

fn collect_recent_user_inputs(state: &AppState, limit: usize) -> Vec<String> {
    let mut entries = Vec::new();
    for message in state.messages.iter().rev() {
        match &message.content {
            MessageContent::Plain(text, _) | MessageContent::PlainText(text) => {
                let trimmed = text.trim();
                if let Some(stripped) = trimmed.strip_prefix("→ ") {
                    entries.push(stripped.trim().to_string());
                } else if trimmed.starts_with('/') {
                    entries.push(trimmed.to_string());
                }
            }
            _ => {}
        }
        if entries.len() >= limit {
            break;
        }
    }
    entries.reverse();
    entries
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

        // If content changed while user is scrolled up, mark it
        if !state.stay_at_bottom {
            state.content_changed_while_scrolled_up = true;
        }

        // During streaming, only adjust scroll if we're staying at bottom
        if state.stay_at_bottom {
            let input_height = 3;
            let total_lines = state.messages.len() * 2;
            let max_visible_lines =
                std::cmp::max(1, message_area_height.saturating_sub(input_height));
            let max_scroll = total_lines.saturating_sub(max_visible_lines);
            state.scroll = max_scroll;
        }
        state.is_streaming = false;
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

    // If content changed while user is scrolled up, mark it
    if !state.stay_at_bottom {
        state.content_changed_while_scrolled_up = true;
    }
}

/// Updates command palette scroll position to keep selected item visible
fn update_command_palette_scroll(state: &mut AppState) {
    let filtered_commands =
        crate::services::command_palette::filter_commands(&state.command_palette_search);
    let total_commands = filtered_commands.len();

    if total_commands == 0 {
        return;
    }

    // Assume a fixed height for the command list (adjust based on your popup height)
    let visible_height = 6; // Adjust this based on your actual popup height

    // Calculate the scroll position to keep the selected item visible
    if state.command_palette_selected < state.command_palette_scroll {
        // Selected item is above visible area, scroll up
        state.command_palette_scroll = state.command_palette_selected;
    } else if state.command_palette_selected >= state.command_palette_scroll + visible_height {
        // Selected item is below visible area, scroll down
        state.command_palette_scroll = state.command_palette_selected - visible_height + 1;
    }

    // Ensure scroll doesn't go beyond bounds
    let max_scroll = total_commands.saturating_sub(visible_height);
    if state.command_palette_scroll > max_scroll {
        state.command_palette_scroll = max_scroll;
    }
}

/// Handles upward navigation with approval popup check
fn handle_up_navigation(state: &mut AppState) {
    if state.show_command_palette {
        let filtered_commands =
            crate::services::command_palette::filter_commands(&state.command_palette_search);
        if state.command_palette_selected > 0 {
            state.command_palette_selected -= 1;
        } else {
            state.command_palette_selected = filtered_commands.len().saturating_sub(1);
        }
        // Update scroll position to keep selected item visible
        update_command_palette_scroll(state);
        return;
    }
    if state.show_profile_switcher {
        if state.profile_switcher_selected > 0 {
            state.profile_switcher_selected -= 1;
        } else {
            state.profile_switcher_selected = state.available_profiles.len() - 1;
        }
        return;
    }

    if state.show_shortcuts_popup {
        // Handle scrolling up in shortcuts popup (like collapsed messages)
        if state.shortcuts_scroll >= SCROLL_LINES {
            state.shortcuts_scroll -= SCROLL_LINES;
        } else {
            state.shortcuts_scroll = 0;
        }
        return;
    }
    if state.show_rulebook_switcher {
        if state.rulebook_switcher_selected > 0 {
            state.rulebook_switcher_selected -= 1;
        } else {
            state.rulebook_switcher_selected = state.filtered_rulebooks.len().saturating_sub(1);
        }
        return;
    }
    // Check if approval popup is visible and should consume the event
    if state.approval_popup.is_visible() {
        state.approval_popup.scroll_up();
        return; // Event was consumed by popup
    }

    // Handle different UI states
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

/// Handles downward navigation with approval popup check
fn handle_down_navigation(
    state: &mut AppState,
    message_area_height: usize,
    message_area_width: usize,
) {
    if state.show_command_palette {
        let filtered_commands =
            crate::services::command_palette::filter_commands(&state.command_palette_search);
        if state.command_palette_selected < filtered_commands.len().saturating_sub(1) {
            state.command_palette_selected += 1;
        } else {
            state.command_palette_selected = 0;
        }
        // Update scroll position to keep selected item visible
        update_command_palette_scroll(state);
        return;
    }
    if state.show_profile_switcher {
        if state.profile_switcher_selected < state.available_profiles.len() - 1 {
            state.profile_switcher_selected += 1;
        } else {
            state.profile_switcher_selected = 0;
        }
        return;
    }

    if state.show_shortcuts_popup {
        // Handle scrolling down in shortcuts popup (like collapsed messages)
        let all_lines = crate::services::shortcuts_popup::get_cached_shortcuts_content(None);
        let total_lines = all_lines.len();
        let max_scroll = total_lines;

        if state.shortcuts_scroll + SCROLL_LINES < max_scroll {
            state.shortcuts_scroll += SCROLL_LINES;
        } else {
            state.shortcuts_scroll = max_scroll;
        }
        return;
    }
    if state.show_rulebook_switcher {
        if state.rulebook_switcher_selected < state.filtered_rulebooks.len().saturating_sub(1) {
            state.rulebook_switcher_selected += 1;
        } else {
            state.rulebook_switcher_selected = 0;
        }
    }
    // Check if approval popup is visible and should consume the event
    if state.approval_popup.is_visible() {
        state.approval_popup.scroll_down();
        return; // Event was consumed by popup
    }

    // Handle different UI states
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

            // If content changed while we were scrolled up, invalidate cache once
            // to catch up with new content that arrived while scrolled up
            if state.content_changed_while_scrolled_up {
                crate::services::message::invalidate_message_lines_cache(state);
                state.content_changed_while_scrolled_up = false;
            }
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

            // If content changed while we were scrolled up, invalidate cache once
            if state.content_changed_while_scrolled_up {
                crate::services::message::invalidate_message_lines_cache(state);
                state.content_changed_while_scrolled_up = false;
            }
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
        // Set textarea shell mode to match app state
        state.text_area.set_shell_mode(true);
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
    if let Some(allowed_tools) = &state.allowed_tools
        && !allowed_tools.is_empty()
    {
        auto_approved_tools.retain(|(tool_name, _)| allowed_tools.contains(tool_name));
    }

    if auto_approved_tools.is_empty() {
        let message = if state
            .allowed_tools
            .as_ref()
            .is_some_and(|tools| !tools.is_empty())
        {
            "No allowed tools are currently set to auto-approve."
        } else {
            "No tools are currently set to auto-approve."
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
            &format!("Tools currently set to auto-approve: {}", tool_list),
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

fn filter_rulebooks(state: &mut AppState) {
    if state.rulebook_search_input.is_empty() {
        state.filtered_rulebooks = state.available_rulebooks.clone();
    } else {
        let search_term = state.rulebook_search_input.to_lowercase();
        state.filtered_rulebooks = state
            .available_rulebooks
            .iter()
            .filter(|rulebook| {
                rulebook.uri.to_lowercase().contains(&search_term)
                    || rulebook.description.to_lowercase().contains(&search_term)
                    || rulebook
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&search_term))
            })
            .cloned()
            .collect();
    }

    // Reset selection if it's out of bounds
    if state.rulebook_switcher_selected >= state.filtered_rulebooks.len() {
        state.rulebook_switcher_selected = 0;
    }
}

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

fn resume_session(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    state.message_tool_calls = None;
    state.message_approved_tools.clear();
    state.message_rejected_tools.clear();
    state.tool_call_execution_order.clear();
    state.session_tool_calls_queue.clear();
    state.toggle_approved_message = true;

    state.messages.clear();
    state
        .messages
        .extend(welcome_messages(state.latest_version.clone(), state));
    render_system_message(state, "Resuming last session.");

    // Reset usage for the resumed session
    state.total_session_usage = stakpak_shared::models::integrations::openai::Usage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        prompt_tokens_details: None,
    };

    let _ = output_tx.try_send(OutputEvent::ResumeSession);

    state.text_area.set_text("");
    state.show_helper_dropdown = false;
}

fn new_session(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    let _ = output_tx.try_send(OutputEvent::NewSession);
    state.text_area.set_text("");
    state.messages.clear();
    state
        .messages
        .extend(welcome_messages(state.latest_version.clone(), state));
    render_system_message(state, "New session started.");

    // Reset usage for the new session
    state.total_session_usage = stakpak_shared::models::integrations::openai::Usage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        prompt_tokens_details: None,
    };

    state.show_helper_dropdown = false;
}

fn execute_command_palette_selection(
    state: &mut AppState,
    input_tx: &Sender<InputEvent>,
    output_tx: &Sender<OutputEvent>,
) {
    use crate::services::command_palette::{CommandAction, filter_commands};

    let filtered_commands = filter_commands(&state.command_palette_search);
    if filtered_commands.is_empty() || state.command_palette_selected >= filtered_commands.len() {
        return;
    }

    let selected_command = &filtered_commands[state.command_palette_selected];

    // Close command palette
    state.show_command_palette = false;
    state.command_palette_search.clear();

    // Execute the command based on its action
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
        CommandAction::NewSession => {
            new_session(state, output_tx);
        }
        CommandAction::OpenSessions => {
            state.text_area.set_text("/sessions");
            let _ = output_tx.try_send(OutputEvent::ListSessions);
        }
        CommandAction::ResumeSession => {
            resume_session(state, output_tx);
        }
        CommandAction::ShowStatus => {
            push_status_message(state);
        }
        CommandAction::MemorizeConversation => {
            push_memorize_message(state);
            let _ = output_tx.try_send(OutputEvent::Memorize);
        }
        CommandAction::SubmitIssue => {
            push_issue_message(state);
        }
        CommandAction::GetSupport => {
            push_support_message(state);
        }
        CommandAction::ShowUsage => {
            push_usage_message(state);
        }
        CommandAction::SwitchModel => match switch_model(state) {
            Ok(()) => {
                let _ = output_tx.try_send(OutputEvent::SwitchModel(state.model.clone()));
                push_model_message(state);
            }
            Err(e) => {
                push_error_message(state, &e, None);
            }
        },
    }
    state.text_area.set_text("");
    state.show_helper_dropdown = false;
}

fn switch_model(state: &mut AppState) -> Result<(), String> {
    match state.model {
        AgentModel::Smart => {
            if state.total_session_usage.total_tokens < CONTEXT_MAX_UTIL_TOKENS_ECO {
                state.model = AgentModel::Eco;
                Ok(())
            } else {
                Err(
                    "Cannot switch model: context exceeds eco model context window size."
                        .to_string(),
                )
            }
        }
        AgentModel::Eco => {
            state.model = AgentModel::Smart;
            Ok(())
        }
    }
}
