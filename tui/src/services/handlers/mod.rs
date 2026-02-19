//! Event Handlers Module
//!
//! This module contains all event handlers organized by functionality.
//! The main `update()` function routes InputEvents to the appropriate handler modules.

pub mod ask_user;
mod dialog;
mod input;
mod message;
mod misc;
mod navigation;
mod popup;
pub mod shell;
pub mod tool;

// Re-export find_image_file_by_name for use in clipboard_paste
pub use input::find_image_file_by_name;

use crate::app::{AppState, InputEvent, OutputEvent, PendingUserMessage};
use ratatui::layout::Size;
use tokio::sync::mpsc::Sender;

/// Groups related event channel senders together to reduce function parameter counts
pub struct EventChannels<'a> {
    pub output_tx: &'a Sender<OutputEvent>,
    pub input_tx: &'a Sender<InputEvent>,
}

fn take_merged_pending_user_message(state: &mut AppState) -> Option<PendingUserMessage> {
    let mut merged = state.pending_user_messages.pop_front()?;
    while let Some(next) = state.pending_user_messages.pop_front() {
        merged.merge_from(next);
    }
    Some(merged)
}

fn flush_pending_user_messages_if_idle(
    state: &mut AppState,
    input_tx: &Sender<InputEvent>,
    output_tx: &Sender<OutputEvent>,
) {
    if state.loading_manager.is_loading() {
        return;
    }

    let Some(pending_message) = take_merged_pending_user_message(state) else {
        return;
    };

    let PendingUserMessage {
        final_input,
        shell_tool_calls,
        image_parts,
        user_message_text,
    } = pending_message;

    match output_tx.try_send(OutputEvent::UserMessage(
        final_input,
        shell_tool_calls,
        image_parts,
    )) {
        Ok(()) => {
            if let Err(e) = input_tx.try_send(InputEvent::AddUserMessage(user_message_text.clone()))
            {
                log::warn!("Failed to send AddUserMessage event: {}", e);
                message::handle_add_user_message(state, user_message_text);
            }
        }
        Err(
            tokio::sync::mpsc::error::TrySendError::Full(OutputEvent::UserMessage(
                final_input,
                shell_tool_calls,
                image_parts,
            ))
            | tokio::sync::mpsc::error::TrySendError::Closed(OutputEvent::UserMessage(
                final_input,
                shell_tool_calls,
                image_parts,
            )),
        ) => {
            log::warn!("Failed to flush buffered UserMessage event: output channel unavailable");
            state
                .pending_user_messages
                .push_front(PendingUserMessage::new(
                    final_input,
                    shell_tool_calls,
                    image_parts,
                    user_message_text,
                ));
        }
        Err(_) => {
            // OutputEvent::UserMessage is always used here.
        }
    }
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
            | InputEvent::AvailableModelsLoaded(_)
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

    state.scroll = state.scroll.max(0);

    // Intercept keys for "existing plan found" modal
    if state.existing_plan_prompt.is_some() {
        match event {
            InputEvent::InputChanged('u') => {
                // Use existing plan — proceed with plan mode, keep plan.md
                if let Some(prompt) = state.existing_plan_prompt.take() {
                    let _ =
                        output_tx.try_send(OutputEvent::PlanModeActivated(prompt.inline_prompt));
                }
                return;
            }
            InputEvent::InputChanged('n') => {
                // Start new — archive existing plan, then activate
                let session_dir = std::path::Path::new(".stakpak/session");
                crate::services::plan::archive_plan_file(session_dir);
                if let Some(prompt) = state.existing_plan_prompt.take() {
                    let _ =
                        output_tx.try_send(OutputEvent::PlanModeActivated(prompt.inline_prompt));
                }
                return;
            }
            InputEvent::HandleEsc => {
                // Cancel — dismiss modal, don't enter plan mode
                state.existing_plan_prompt = None;
                return;
            }
            InputEvent::AttemptQuit | InputEvent::Quit => {
                // Allow quit through
            }
            _ => {
                return; // Consume everything else
            }
        }
    }

    // Intercept keys for Plan Review overlay
    if state.show_plan_review {
        // Sub-intercept: comment modal is open
        if state.plan_review_show_comment_modal {
            match event {
                InputEvent::HandleEsc => {
                    crate::services::plan_review::close_comment_modal(state);
                    return;
                }
                InputEvent::InputChanged(c) => {
                    crate::services::plan_review::modal_input_char(state, c);
                    return;
                }
                InputEvent::InputBackspace => {
                    crate::services::plan_review::modal_input_backspace(state);
                    return;
                }
                InputEvent::InputChangedNewline => {
                    // Enter adds newline in modal
                    crate::services::plan_review::modal_input_newline(state);
                    return;
                }
                InputEvent::InputSubmitted => {
                    // Ctrl+Enter submits
                    crate::services::plan_review::submit_comment(state);
                    return;
                }
                InputEvent::AttemptQuit | InputEvent::Quit => {
                    // Allow quit through
                }
                _ => {
                    return; // Consume everything else
                }
            }
        } else if state.plan_review_confirm.is_some() {
            // Confirmation dialog is open — y/Enter confirms, n/Esc cancels
            match event {
                InputEvent::HandleEsc | InputEvent::InputChanged('n') => {
                    state.plan_review_confirm = None;
                    return;
                }
                InputEvent::InputSubmitted
                | InputEvent::InputChangedNewline
                | InputEvent::InputChanged('y') => {
                    crate::services::plan_review::execute_confirm(state, output_tx);
                    return;
                }
                InputEvent::AttemptQuit | InputEvent::Quit => {
                    // Allow quit through
                }
                _ => {
                    return; // Consume everything else
                }
            }
        } else {
            match event {
                InputEvent::HandleEsc
                | InputEvent::PlanReviewClose
                | InputEvent::TogglePlanReview => {
                    crate::services::plan_review::close_plan_review(state);
                    return;
                }
                InputEvent::Up | InputEvent::PlanReviewCursorUp => {
                    crate::services::plan_review::cursor_up(state);
                    return;
                }
                InputEvent::Down | InputEvent::PlanReviewCursorDown => {
                    crate::services::plan_review::cursor_down(state);
                    return;
                }
                InputEvent::InputChanged('k') => {
                    crate::services::plan_review::cursor_up(state);
                    return;
                }
                InputEvent::InputChanged('j') => {
                    crate::services::plan_review::cursor_down(state);
                    return;
                }
                InputEvent::InputChanged('c') => {
                    crate::services::plan_review::open_comment_modal(state);
                    return;
                }
                InputEvent::InputChanged('r') => {
                    // 'r' key is no longer bound (replies removed)
                    return;
                }
                InputEvent::InputChanged('x') => {
                    // 'x' key is no longer bound (resolve removed)
                    return;
                }
                InputEvent::InputChanged('d') => {
                    crate::services::plan_review::open_delete_confirm(state);
                    return;
                }
                InputEvent::Tab | InputEvent::PlanReviewNextComment => {
                    crate::services::plan_review::next_comment(state);
                    return;
                }
                InputEvent::PlanReviewPrevComment => {
                    crate::services::plan_review::prev_comment(state);
                    return;
                }
                InputEvent::PageUp | InputEvent::PlanReviewPageUp => {
                    crate::services::plan_review::page_up(state, message_area_height);
                    return;
                }
                InputEvent::PageDown | InputEvent::PlanReviewPageDown => {
                    crate::services::plan_review::page_down(state, message_area_height);
                    return;
                }
                InputEvent::PlanReviewComment => {
                    crate::services::plan_review::open_comment_modal(state);
                    return;
                }
                InputEvent::PlanReviewResolve => {
                    // Resolve removed — no-op
                    return;
                }
                InputEvent::InputSubmitted
                | InputEvent::InputChangedNewline
                | InputEvent::PlanReviewApprove
                | InputEvent::PlanReviewFeedback => {
                    crate::services::plan_review::open_submit_confirm(state);
                    return;
                }
                InputEvent::InputChanged('a') | InputEvent::InputChanged('f') => {
                    // Legacy bindings — route to unified submit
                    crate::services::plan_review::open_submit_confirm(state);
                    return;
                }
                InputEvent::AttemptQuit | InputEvent::Quit => {
                    // Allow quit events through
                }
                _ => {
                    // Consume all other events while plan review is open
                    return;
                }
            }
        }
    }

    // Intercept keys for File Changes Popup
    if state.show_file_changes_popup {
        match event {
            InputEvent::HandleEsc => {
                popup::handle_file_changes_popup_cancel(state);
                return;
            }
            InputEvent::FileChangesRevertAll => {
                // Ctrl+Z to Revert All
                popup::handle_file_changes_popup_revert_all(state);
                return;
            }
            InputEvent::FileChangesRevertFile => {
                // Ctrl+X to Revert single file
                popup::handle_file_changes_popup_revert(state);
                return;
            }
            InputEvent::FileChangesOpenEditor => {
                // Ctrl+N to open in editor
                popup::handle_file_changes_popup_open_editor(state);
                return;
            }
            InputEvent::Up | InputEvent::ScrollUp => {
                popup::handle_file_changes_popup_navigate(state, -1);
                return;
            }
            InputEvent::Down | InputEvent::ScrollDown => {
                popup::handle_file_changes_popup_navigate(state, 1);
                return;
            }
            InputEvent::InputChanged(c) => {
                popup::handle_file_changes_popup_search_input(state, c);
                return;
            }
            InputEvent::InputBackspace => {
                popup::handle_file_changes_popup_backspace(state);
                return;
            }
            InputEvent::MouseClick(col, row) => {
                popup::handle_file_changes_popup_mouse_click(state, col, row);
                return;
            }
            _ => {
                // Consume other events to prevent side effects
                return;
            }
        }
    }

    // Intercept keys for Ask User inline block
    // Tab toggles focus: focused = navigate inside block, unfocused = scroll freely
    if state.show_ask_user_popup {
        match event {
            InputEvent::HandleEsc | InputEvent::AskUserCancel => {
                if state.ask_user_focused {
                    // First Esc unfocuses, second Esc cancels
                    state.ask_user_focused = false;
                    ask_user::refresh_ask_user_block_pub(state);
                } else {
                    ask_user::handle_ask_user_cancel(state, output_tx);
                }
                return;
            }
            InputEvent::Tab => {
                // Toggle focus on the ask_user block
                state.ask_user_focused = !state.ask_user_focused;
                if state.ask_user_focused {
                    // Scroll to bottom to show the block
                    state.stay_at_bottom = true;
                }
                ask_user::refresh_ask_user_block_pub(state);
                return;
            }
            InputEvent::ShowAskUserPopup(tool_call, questions) => {
                ask_user::handle_show_ask_user_popup(state, tool_call, questions);
                return;
            }
            _ => {}
        }

        // When focused, intercept navigation keys for block interaction
        if state.ask_user_focused {
            match event {
                InputEvent::AskUserNextTab | InputEvent::CursorRight => {
                    ask_user::handle_ask_user_next_tab(state);
                    return;
                }
                InputEvent::AskUserPrevTab | InputEvent::CursorLeft => {
                    ask_user::handle_ask_user_prev_tab(state);
                    return;
                }
                InputEvent::AskUserNextOption | InputEvent::Down | InputEvent::ScrollDown => {
                    ask_user::handle_ask_user_next_option(state);
                    return;
                }
                InputEvent::AskUserPrevOption | InputEvent::Up | InputEvent::ScrollUp => {
                    ask_user::handle_ask_user_prev_option(state);
                    return;
                }
                InputEvent::AskUserSelectOption | InputEvent::InputSubmitted => {
                    ask_user::handle_ask_user_select_option(state, output_tx);
                    return;
                }
                InputEvent::AskUserSubmit => {
                    ask_user::handle_ask_user_submit(state, output_tx);
                    return;
                }
                InputEvent::AskUserCustomInputChanged(c) => {
                    ask_user::handle_ask_user_custom_input_changed(state, c);
                    return;
                }
                InputEvent::AskUserCustomInputBackspace => {
                    ask_user::handle_ask_user_custom_input_backspace(state);
                    return;
                }
                InputEvent::AskUserCustomInputDelete => {
                    ask_user::handle_ask_user_custom_input_delete(state);
                    return;
                }
                InputEvent::InputChanged(c) => {
                    if ask_user::is_custom_input_selected(state) {
                        ask_user::handle_ask_user_custom_input_changed(state, c);
                    }
                    return;
                }
                InputEvent::InputBackspace => {
                    if ask_user::is_custom_input_selected(state) {
                        ask_user::handle_ask_user_custom_input_backspace(state);
                    }
                    return;
                }
                InputEvent::InputDelete => {
                    if ask_user::is_custom_input_selected(state) {
                        ask_user::handle_ask_user_custom_input_delete(state);
                    }
                    return;
                }
                _ => {
                    // Consume other events while focused
                    return;
                }
            }
        }
        // When unfocused, all events pass through to normal handlers (scrolling works)
    }

    // Handle ShowAskUserPopup event even when popup is not visible
    if let InputEvent::ShowAskUserPopup(tool_call, questions) = event {
        ask_user::handle_show_ask_user_popup(state, tool_call, questions);
        return;
    }

    // Intercept keys for Model Switcher Popup
    if state.show_model_switcher {
        match event {
            InputEvent::HandleEsc => {
                popup::handle_model_switcher_cancel(state);
                return;
            }
            InputEvent::Tab => {
                // Tabs are hidden for now, consume the event to prevent side effects
                return;
            }
            InputEvent::Up | InputEvent::ScrollUp => {
                // Navigate up in filtered model list
                let filtered = crate::services::model_switcher::filter_models(
                    &state.available_models,
                    state.model_switcher_mode,
                    &state.model_switcher_search,
                );
                if !filtered.is_empty() {
                    // Find current position in filtered list
                    let current_pos = filtered
                        .iter()
                        .position(|&idx| idx == state.model_switcher_selected)
                        .unwrap_or(0);
                    // Move up (with wrap)
                    let new_pos = if current_pos > 0 {
                        current_pos - 1
                    } else {
                        filtered.len() - 1
                    };
                    state.model_switcher_selected = filtered[new_pos];
                }
                return;
            }
            InputEvent::Down | InputEvent::ScrollDown => {
                // Navigate down in filtered model list
                let filtered = crate::services::model_switcher::filter_models(
                    &state.available_models,
                    state.model_switcher_mode,
                    &state.model_switcher_search,
                );
                if !filtered.is_empty() {
                    // Find current position in filtered list
                    let current_pos = filtered
                        .iter()
                        .position(|&idx| idx == state.model_switcher_selected)
                        .unwrap_or(0);
                    // Move down (with wrap)
                    let new_pos = if current_pos < filtered.len() - 1 {
                        current_pos + 1
                    } else {
                        0
                    };
                    state.model_switcher_selected = filtered[new_pos];
                }
                return;
            }
            InputEvent::InputSubmitted => {
                popup::handle_model_switcher_select(state, output_tx);
                return;
            }
            InputEvent::InputChanged(c) | InputEvent::ModelSwitcherSearchInputChanged(c) => {
                // Add character to search
                state.model_switcher_search.push(c);
                // Reset selection to first filtered result
                let filtered = crate::services::model_switcher::filter_models(
                    &state.available_models,
                    state.model_switcher_mode,
                    &state.model_switcher_search,
                );
                state.model_switcher_selected = filtered.first().copied().unwrap_or(0);
                return;
            }
            InputEvent::InputBackspace | InputEvent::ModelSwitcherSearchBackspace => {
                // Remove character from search
                state.model_switcher_search.pop();
                // Reset selection to first filtered result
                let filtered = crate::services::model_switcher::filter_models(
                    &state.available_models,
                    state.model_switcher_mode,
                    &state.model_switcher_search,
                );
                state.model_switcher_selected = filtered.first().copied().unwrap_or(0);
                return;
            }
            InputEvent::AvailableModelsLoaded(_) => {
                // Let this fall through to the main handler
            }
            _ => {
                // Consume other events to prevent side effects
                return;
            }
        }
    }

    // Intercept keys for Approval Bar (inline approval)
    // Controls: ←→ navigate, Space toggle, Enter confirm all, Esc reject all
    // Don't intercept if collapsed messages popup is showing
    if state.approval_bar.is_visible() && !state.show_collapsed_messages {
        match event {
            InputEvent::HandleEsc => {
                if !state.approval_bar.is_esc_pending() {
                    // First ESC: show hint and set is_dialog_open
                    state.approval_bar.set_esc_pending(true);
                    state.is_dialog_open = true;
                    return;
                }

                // Second ESC: reject all tools
                state.approval_bar.reject_all();

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

                // Process tools in order
                if let Some(tool_calls) = &state.message_tool_calls.clone() {
                    for tool_call in tool_calls {
                        let is_approved = state.message_approved_tools.contains(tool_call);
                        let status = if is_approved {
                            crate::app::ToolCallStatus::Approved
                        } else {
                            crate::app::ToolCallStatus::Rejected
                        };
                        state.tool_call_execution_order.push(tool_call.id.clone());
                        state
                            .session_tool_calls_queue
                            .insert(tool_call.id.clone(), status);
                    }

                    // Always execute the FIRST tool, regardless of which tab is selected
                    if let Some(first_tool) = tool_calls.first() {
                        // Set dialog_command to the first tool for proper processing
                        state.dialog_command = Some(first_tool.clone());
                        state
                            .session_tool_calls_queue
                            .insert(first_tool.id.clone(), crate::app::ToolCallStatus::Executed);

                        let is_approved = state.message_approved_tools.contains(first_tool);

                        // Update the pending display to show the first tool (which is being executed)
                        dialog::update_pending_tool_to_first(state, first_tool, is_approved);

                        if is_approved {
                            // Update run_command block to Running state
                            dialog::update_run_command_to_running(state, first_tool);
                            let _ = output_tx.try_send(OutputEvent::AcceptTool(first_tool.clone()));
                        } else {
                            // Fire handle reject - keep is_dialog_open true so it renders properly
                            state.is_dialog_open = true;
                            let _ = input_tx.try_send(InputEvent::HandleReject(
                                Some("Tool call rejected".to_string()),
                                true,
                                None,
                            ));
                        }
                    }
                }

                // Clear message_tool_calls but DON'T clear is_dialog_open yet
                // HandleReject will clear it after rendering
                state.message_tool_calls = None;

                // Clear the approval bar
                state.approval_bar.clear();

                return;
            }
            InputEvent::InputChanged(' ') => {
                // Space: toggle approve/reject for selected
                tool::handle_approval_bar_toggle_selected(state, input_tx);
                return;
            }
            InputEvent::CursorLeft => {
                // Left arrow: select previous tab and update message display
                tool::handle_approval_bar_prev_action(state, input_tx);
                return;
            }
            InputEvent::CursorRight => {
                // Right arrow: select next tab and update message display
                tool::handle_approval_bar_next_action(state, input_tx);
                return;
            }
            InputEvent::InputSubmitted => {
                // If ESC was pending, Enter cancels it and goes back to showing the bar
                if state.approval_bar.is_esc_pending() {
                    state.approval_bar.set_esc_pending(false);
                    state.is_dialog_open = false;
                    return;
                }
                // Otherwise, confirm all and execute (handled in input.rs)
                // Let it pass through to handle_input_submitted_event
            }
            _ => {
                // Let other events pass through to normal handling
            }
        }
    }

    // Intercept keys for Shell Mode (only when not loading)
    if state.show_shell_mode
        && state.active_shell_command.is_some()
        && !state.is_dialog_open
        && !state.approval_bar.is_visible()
        && !state.shell_loading
    {
        match event {
            InputEvent::InputChanged(c) => {
                state.shell_scroll = 0;
                shell::send_shell_input(state, &c.to_string());
                return;
            }
            InputEvent::InputBackspace => {
                state.shell_scroll = 0;
                shell::send_shell_input(state, "\x7f");
                return;
            }
            InputEvent::InputSubmitted => {
                state.shell_scroll = 0;
                // Windows ConPTY expects carriage return, Unix expects newline
                #[cfg(windows)]
                shell::send_shell_input(state, "\r");
                #[cfg(not(windows))]
                shell::send_shell_input(state, "\n");
                return;
            }
            InputEvent::CursorLeft => {
                state.shell_scroll = 0;
                shell::send_shell_input(state, "\x1b[D");
                return;
            }
            InputEvent::CursorRight => {
                state.shell_scroll = 0;
                shell::send_shell_input(state, "\x1b[C");
                return;
            }
            InputEvent::Up => {
                state.shell_scroll = 0;
                shell::send_shell_input(state, "\x1b[A");
                return;
            }
            InputEvent::Down => {
                state.shell_scroll = 0;
                shell::send_shell_input(state, "\x1b[B");
                return;
            }

            InputEvent::ScrollUp => {
                // Scroll popup up (show older content)
                if state.shell_popup_visible && state.shell_popup_expanded {
                    state.shell_popup_scroll = state.shell_popup_scroll.saturating_add(1);
                } else {
                    let visible_height = state.terminal_size.height.saturating_sub(2) as usize;
                    let total_lines = state.shell_history_lines.len();
                    let max_scroll = total_lines.saturating_sub(visible_height) as u16;
                    state.shell_scroll = state.shell_scroll.saturating_add(1).min(max_scroll);
                }
                return;
            }
            InputEvent::ScrollDown => {
                // Scroll popup down (show newer content)
                if state.shell_popup_visible && state.shell_popup_expanded {
                    state.shell_popup_scroll = state.shell_popup_scroll.saturating_sub(1);
                } else {
                    state.shell_scroll = state.shell_scroll.saturating_sub(1);
                }
                return;
            }
            InputEvent::PageUp => {
                if state.shell_popup_visible && state.shell_popup_expanded {
                    let page_size = state.terminal_size.height / 4;
                    state.shell_popup_scroll =
                        state.shell_popup_scroll.saturating_add(page_size as usize);
                } else {
                    let visible_height = state.terminal_size.height.saturating_sub(2) as usize;
                    let total_lines = state.shell_history_lines.len();
                    let max_scroll = total_lines.saturating_sub(visible_height) as u16;
                    let page_size = state.terminal_size.height / 2;
                    state.shell_scroll =
                        state.shell_scroll.saturating_add(page_size).min(max_scroll);
                }
                return;
            }
            InputEvent::PageDown => {
                if state.shell_popup_visible && state.shell_popup_expanded {
                    let page_size = state.terminal_size.height / 4;
                    state.shell_popup_scroll =
                        state.shell_popup_scroll.saturating_sub(page_size as usize);
                } else {
                    let page_size = state.terminal_size.height / 2;
                    state.shell_scroll = state.shell_scroll.saturating_sub(page_size);
                }
                return;
            }
            InputEvent::HandleEsc => {
                // Don't send ESC to shell - let it fall through to handle_esc_event
                // which will terminate the shell and cancel the tool call
            }
            InputEvent::Tab => {
                shell::send_shell_input(state, "\t");
                return;
            }
            InputEvent::AttemptQuit => {
                // Ctrl+C sends SIGINT to cancel running commands in shell
                shell::send_shell_input(state, "\x03");
                return;
            }
            InputEvent::InputDelete => {
                state.shell_scroll = 0;
                shell::send_shell_input(state, "\x15");
                return;
            }
            InputEvent::InputDeleteWord => {
                state.shell_scroll = 0;
                shell::send_shell_input(state, "\x17");
                return;
            }
            _ => {}
        }
    }

    // Route events to appropriate handlers
    match event {
        // Input handlers
        InputEvent::InputChanged(c) => {
            input::handle_input_changed_event(state, c, input_tx);
        }
        InputEvent::InputBackspace => {
            input::handle_input_backspace_event(state, input_tx);
        }
        InputEvent::InputChangedNewline => {
            input::handle_input_changed(state, '\n', input_tx);
        }
        InputEvent::InputSubmitted => {
            input::handle_input_submitted_event(
                state,
                message_area_height,
                output_tx,
                input_tx,
                shell_tx,
                cancel_tx,
            );
        }
        InputEvent::InputSubmittedWith(s) => {
            input::handle_input_submitted_with(state, s, None, message_area_height);
        }
        InputEvent::InputSubmittedWithColor(s, color) => {
            input::handle_input_submitted_with(state, s, Some(color), message_area_height);
        }
        InputEvent::HandlePaste(text) => {
            input::handle_paste(state, text);
        }
        InputEvent::HandleClipboardImagePaste => {
            input::handle_clipboard_image_paste(state);
        }
        InputEvent::InputDelete => {
            input::handle_input_delete(state);
        }
        InputEvent::InputDeleteWord => {
            input::handle_input_delete_word(state);
        }
        InputEvent::InputCursorStart => {
            input::handle_input_cursor_start(state);
        }
        InputEvent::InputCursorEnd => {
            input::handle_input_cursor_end(state);
        }
        InputEvent::InputCursorPrevWord => {
            input::handle_input_cursor_prev_word(state);
        }
        InputEvent::InputCursorNextWord => {
            input::handle_input_cursor_next_word(state);
        }
        InputEvent::CursorLeft => {
            input::handle_cursor_left(state);
        }
        InputEvent::CursorRight => {
            input::handle_cursor_right(state);
        }

        // Navigation handlers
        InputEvent::Up => {
            navigation::handle_up_navigation(state);
        }
        InputEvent::Down => {
            navigation::handle_down_navigation(state, message_area_height, message_area_width);
        }
        InputEvent::ScrollUp => {
            navigation::handle_up_navigation(state);
        }
        InputEvent::ScrollDown => {
            navigation::handle_down_navigation(state, message_area_height, message_area_width);
        }
        InputEvent::PageUp => {
            navigation::handle_page_up(state, message_area_height, message_area_width);
        }
        InputEvent::PageDown => {
            navigation::handle_page_down(state, message_area_height, message_area_width);
        }
        InputEvent::DropdownUp => {
            navigation::handle_dropdown_up(state);
        }
        InputEvent::DropdownDown => {
            navigation::handle_dropdown_down(state);
        }
        InputEvent::HandleEsc => {
            dialog::handle_esc_event(state, input_tx, output_tx, shell_tx, cancel_tx);
        }
        InputEvent::HandleReject(message, should_stop, color) => {
            let channels = EventChannels {
                output_tx,
                input_tx,
            };
            dialog::handle_esc(state, &channels, cancel_tx, message, should_stop, color);
        }
        InputEvent::ShowConfirmationDialog(tool_call) => {
            dialog::handle_show_confirmation_dialog(
                state,
                tool_call,
                input_tx,
                output_tx,
                terminal_size,
            );
        }
        InputEvent::ToggleDialogFocus => {
            dialog::handle_toggle_dialog_focus(state);
        }

        // Tool handlers
        InputEvent::StreamToolResult(progress) => {
            if let Some(command) = tool::handle_stream_tool_result(state, progress) {
                // Interactive stall detected - trigger shell mode with the command
                tool::handle_interactive_stall_detected(state, command, input_tx);
            }
        }
        InputEvent::MessageToolCalls(tool_calls) => {
            tool::handle_message_tool_calls(state, tool_calls);
        }
        InputEvent::StreamToolCallProgress(infos) => {
            tool::handle_stream_tool_call_progress(state, infos);
        }
        InputEvent::RetryLastToolCall => {
            tool::handle_retry_tool_call(state, input_tx, cancel_tx);
        }
        InputEvent::InteractiveStallDetected(command) => {
            tool::handle_interactive_stall_detected(state, command, input_tx);
        }
        InputEvent::ToggleApprovalStatus => {
            tool::handle_toggle_approval_status(state);
        }
        InputEvent::ApprovalPopupNextTab => {
            tool::handle_approval_popup_next_tab(state);
        }
        InputEvent::ApprovalPopupPrevTab => {
            tool::handle_approval_popup_prev_tab(state);
        }
        InputEvent::ApprovalPopupToggleApproval => {
            tool::handle_approval_popup_toggle_approval(state);
        }
        InputEvent::ApprovalPopupEscape => {
            tool::handle_approval_popup_escape(state);
        }
        // Approval bar handlers
        InputEvent::ApprovalBarApproveAll => {
            tool::handle_approval_bar_approve_all(state);
        }
        InputEvent::ApprovalBarRejectAll => {
            tool::handle_approval_bar_reject_all(state);
        }
        InputEvent::ApprovalBarSelectAction(index) => {
            tool::handle_approval_bar_select_action(state, index);
        }
        InputEvent::ApprovalBarApproveSelected => {
            tool::handle_approval_bar_approve_selected(state);
        }
        InputEvent::ApprovalBarRejectSelected => {
            tool::handle_approval_bar_reject_selected(state);
        }
        InputEvent::ApprovalBarNextAction => {
            tool::handle_approval_bar_next_action(state, input_tx);
        }
        InputEvent::ApprovalBarPrevAction => {
            tool::handle_approval_bar_prev_action(state, input_tx);
        }
        InputEvent::ApprovalBarCollapse => {
            tool::handle_approval_bar_collapse(state);
        }
        // Shell handlers
        InputEvent::RunShellCommand(command) => {
            shell::handle_run_shell_command(state, command, input_tx);
        }
        InputEvent::RunShellWithCommand(command) => {
            shell::handle_run_shell_with_command(state, command, input_tx);
        }
        InputEvent::ShellMode => {
            shell::handle_shell_mode(state, input_tx);
        }
        InputEvent::ShellOutput(line) => {
            let should_auto_complete = shell::handle_shell_output(state, line);
            if should_auto_complete {
                let _ = input_tx.try_send(InputEvent::ShellCompleted(0));
            }
        }
        InputEvent::ShellError(line) => {
            shell::handle_shell_error(state, line);
        }
        InputEvent::ShellWaitingForInput => {
            shell::handle_shell_waiting_for_input(state, message_area_height, message_area_width);
        }
        InputEvent::ShellCompleted(_) => {
            shell::handle_shell_completed(
                state,
                output_tx,
                message_area_height,
                message_area_width,
            );
        }
        InputEvent::ShellClear => {
            shell::handle_shell_clear(state, message_area_height, message_area_width);
        }
        InputEvent::ShellKill => {
            shell::handle_shell_kill(state);
        }

        // Popup handlers
        InputEvent::ShowProfileSwitcher => {
            popup::handle_show_profile_switcher(state);
        }
        InputEvent::ProfileSwitcherSelect => {
            popup::handle_profile_switcher_select(state, output_tx);
        }
        InputEvent::ProfileSwitcherCancel => {
            popup::handle_profile_switcher_cancel(state);
        }
        InputEvent::ProfilesLoaded(profiles, current_profile) => {
            popup::handle_profiles_loaded(state, profiles, current_profile);
        }
        InputEvent::ProfileSwitchRequested(profile) => {
            popup::handle_profile_switch_requested(state, profile);
        }
        InputEvent::ProfileSwitchProgress(message) => {
            popup::handle_profile_switch_progress(state, message);
        }
        InputEvent::ProfileSwitchComplete(profile) => {
            popup::handle_profile_switch_complete(state, profile);
        }
        InputEvent::ProfileSwitchFailed(error) => {
            popup::handle_profile_switch_failed(state, error);
        }
        InputEvent::ShowRulebookSwitcher => {
            popup::handle_show_rulebook_switcher(state, output_tx);
        }
        InputEvent::RulebookSwitcherSelect => {
            popup::handle_rulebook_switcher_select(state);
        }
        InputEvent::RulebookSwitcherToggle => {
            popup::handle_rulebook_switcher_toggle(state);
        }
        InputEvent::RulebookSwitcherCancel => {
            popup::handle_rulebook_switcher_cancel(state);
        }
        InputEvent::RulebookSwitcherConfirm => {
            popup::handle_rulebook_switcher_confirm(state, output_tx);
        }
        InputEvent::RulebookSwitcherSelectAll => {
            popup::handle_rulebook_switcher_select_all(state);
        }
        InputEvent::RulebookSwitcherDeselectAll => {
            popup::handle_rulebook_switcher_deselect_all(state);
        }
        InputEvent::RulebookSearchInputChanged(c) => {
            popup::handle_rulebook_search_input_changed(state, c);
        }
        InputEvent::RulebookSearchBackspace => {
            popup::handle_rulebook_search_backspace(state);
        }
        InputEvent::RulebooksLoaded(rulebooks) => {
            popup::handle_rulebooks_loaded(state, rulebooks);
        }
        InputEvent::CurrentRulebooksLoaded(current_uris) => {
            popup::handle_current_rulebooks_loaded(state, current_uris);
        }
        InputEvent::ShowCommandPalette => {
            popup::handle_show_command_palette(state);
        }
        InputEvent::CommandPaletteSearchInputChanged(c) => {
            popup::handle_command_palette_search_input_changed(state, c);
        }
        InputEvent::CommandPaletteSearchBackspace => {
            popup::handle_command_palette_search_backspace(state);
        }
        InputEvent::ShowShortcuts => {
            popup::handle_show_shortcuts(state);
        }
        InputEvent::ShortcutsCancel => {
            popup::handle_shortcuts_cancel(state);
        }
        InputEvent::ToggleCollapsedMessages => {
            popup::handle_toggle_collapsed_messages(state, message_area_height, message_area_width);
        }
        InputEvent::ShowFileChangesPopup => {
            popup::handle_show_file_changes_popup(state);
        }
        InputEvent::ToggleMoreShortcuts => {
            popup::handle_toggle_more_shortcuts(state);
        }

        // Model switcher handlers
        InputEvent::ShowModelSwitcher => {
            popup::handle_show_model_switcher(state, output_tx);
        }
        InputEvent::AvailableModelsLoaded(models) => {
            popup::handle_available_models_loaded(state, models);
        }
        InputEvent::ModelSwitcherSelect => {
            popup::handle_model_switcher_select(state, output_tx);
        }
        InputEvent::ModelSwitcherCancel => {
            popup::handle_model_switcher_cancel(state);
        }
        InputEvent::ModelSwitcherSearchInputChanged(_)
        | InputEvent::ModelSwitcherSearchBackspace => {
            // These are handled in the model switcher intercept block above
            // If we reach here, the model switcher is not visible, so ignore
        }

        // Side panel handlers
        InputEvent::ToggleSidePanel => {
            popup::handle_toggle_side_panel(state, input_tx);
        }
        InputEvent::SidePanelNextSection => {
            popup::handle_side_panel_next_section(state);
        }
        InputEvent::SidePanelToggleSection => {
            popup::handle_side_panel_toggle_section(state);
        }

        // Message handlers
        InputEvent::StreamAssistantMessage(id, s) => {
            message::handle_stream_message(state, id, s, message_area_height);
        }
        InputEvent::AddUserMessage(s) => {
            message::handle_add_user_message(state, s);
        }

        InputEvent::HasUserMessage => {
            message::handle_has_user_message(state);
        }
        InputEvent::StreamUsage(usage) => {
            message::handle_stream_usage(state, usage);
        }
        InputEvent::RequestTotalUsage => {
            message::handle_request_total_usage(output_tx);
        }
        InputEvent::TotalUsage(usage) => {
            message::handle_total_usage(state, usage);
        }

        // Misc handlers
        InputEvent::Error(err) => {
            misc::handle_error(state, err);
        }
        InputEvent::Resized(width, height) => {
            misc::handle_resized(state, width, height);
        }
        InputEvent::ToggleCursorVisible => {
            misc::handle_toggle_cursor_visible(state);
        }
        InputEvent::ToggleAutoApprove => {
            misc::handle_toggle_auto_approve(state);
        }
        InputEvent::AutoApproveCurrentTool => {
            misc::handle_auto_approve_current_tool(state);
        }
        InputEvent::Tab => {
            misc::handle_tab(state, message_area_height, message_area_width);
        }
        InputEvent::HandleCtrlS => {
            misc::handle_ctrl_s(state, input_tx);
        }
        InputEvent::Quit => {
            // Quit is handled in event loop
        }
        InputEvent::AttemptQuit => {
            misc::handle_attempt_quit(state, input_tx);
        }
        InputEvent::ToggleMouseCapture => {
            misc::handle_toggle_mouse_capture(state);
        }
        InputEvent::OpenFileInEditor => {
            // Handled in file changes popup context above
            // This match arm exists to satisfy exhaustive pattern matching
        }
        InputEvent::FileChangesRevertFile => {
            // Handled in file changes popup context above
        }
        InputEvent::FileChangesRevertAll => {
            // Handled in file changes popup context above
        }
        InputEvent::FileChangesOpenEditor => {
            // Handled in file changes popup context above
        }
        InputEvent::EmergencyClearTerminal => {
            // EmergencyClearTerminal is handled in event loop
        }
        InputEvent::SetSessions(sessions) => {
            misc::handle_set_sessions(state, sessions);
        }
        InputEvent::StartLoadingOperation(operation) => {
            misc::handle_start_loading_operation(state, operation);
        }
        InputEvent::EndLoadingOperation(operation) => {
            misc::handle_end_loading_operation(state, operation);
        }
        InputEvent::AssistantMessage(msg) => {
            misc::handle_assistant_message(state, msg);
        }
        InputEvent::GetStatus(account_info) => {
            misc::handle_get_status(state, account_info);
        }
        InputEvent::StreamModel(model) => {
            misc::handle_stream_model(state, model);
        }
        InputEvent::BillingInfoLoaded(billing_info) => {
            misc::handle_billing_info_loaded(state, billing_info);
        }
        InputEvent::RunToolCall(_) => {}
        InputEvent::ToolResult(_) => {
            // NOTE: handle_tool_result is called in event_loop.rs before routing here,
            // so we don't need to call it again to avoid double-counting file changes.
        }
        InputEvent::ApprovalPopupSubmit => {}
        InputEvent::MouseClick(col, row) => {
            // Check if click is on file changes popup first
            if state.show_file_changes_popup {
                popup::handle_file_changes_popup_mouse_click(state, col, row);
            } else {
                popup::handle_side_panel_mouse_click(state, col, row);
            }
        }

        // Board tasks events
        InputEvent::RefreshBoardTasks => {
            misc::handle_refresh_board_tasks(state, input_tx);
        }
        InputEvent::BoardTasksLoaded(tasks) => {
            misc::handle_board_tasks_loaded(state, tasks);
        }
        InputEvent::BoardTasksError(err) => {
            misc::handle_board_tasks_error(state, err);
        }

        // Plan mode events
        InputEvent::PlanModeChanged(active) => {
            use crate::services::helper_block::push_styled_message;

            let was_active = state.plan_mode_active;
            state.plan_mode_active = active;

            // Show system message when entering plan mode
            if active && !was_active {
                push_styled_message(
                    state,
                    " Plan mode activated - what are we working on today?",
                    ratatui::style::Color::Cyan,
                    "⚙ ",
                    ratatui::style::Color::Cyan,
                );
            }
        }
        InputEvent::ExistingPlanFound(prompt) => {
            // Backend detected an existing plan at --plan startup.
            // Show the modal so the user can choose to resume or start fresh.
            state.existing_plan_prompt = Some(prompt);
        }

        // Plan review events
        InputEvent::TogglePlanReview => {
            if state.show_plan_review {
                crate::services::plan_review::close_plan_review(state);
            } else if state.plan_mode_active {
                crate::services::plan_review::open_plan_review(state);
            } else {
                // Fall through to command palette when not in plan mode
                popup::handle_show_command_palette(state);
            }
        }
        InputEvent::PlanReviewClose => {
            crate::services::plan_review::close_plan_review(state);
        }
        InputEvent::PlanReviewCursorUp => {
            crate::services::plan_review::cursor_up(state);
        }
        InputEvent::PlanReviewCursorDown => {
            crate::services::plan_review::cursor_down(state);
        }
        InputEvent::PlanReviewNextComment => {
            crate::services::plan_review::next_comment(state);
        }
        InputEvent::PlanReviewPrevComment => {
            crate::services::plan_review::prev_comment(state);
        }
        InputEvent::PlanReviewPageUp => {
            crate::services::plan_review::page_up(state, message_area_height);
        }
        InputEvent::PlanReviewPageDown => {
            crate::services::plan_review::page_down(state, message_area_height);
        }
        InputEvent::PlanReviewComment => {
            // Handled by plan review interceptor above
        }
        InputEvent::PlanReviewApprove => {
            // Handled by plan review interceptor
        }
        InputEvent::PlanReviewFeedback => {
            // Handled by plan review interceptor
        }
        InputEvent::PlanReviewResolve => {
            // Handled by plan review interceptor above
        }

        // Ask User popup events (handled in intercept block above, but need match arms)
        InputEvent::ShowAskUserPopup(_, _)
        | InputEvent::AskUserNextTab
        | InputEvent::AskUserPrevTab
        | InputEvent::AskUserNextOption
        | InputEvent::AskUserPrevOption
        | InputEvent::AskUserSelectOption
        | InputEvent::AskUserCustomInputChanged(_)
        | InputEvent::AskUserCustomInputBackspace
        | InputEvent::AskUserCustomInputDelete
        | InputEvent::AskUserSubmit
        | InputEvent::AskUserCancel => {
            // These are handled in the intercept block above when popup is visible
            // If we reach here, the popup is not visible, so ignore
        }
    }

    flush_pending_user_messages_if_idle(state, input_tx, output_tx);
    navigation::adjust_scroll(state, message_area_height, message_area_width);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppStateOptions, LoadingOperation};
    use crate::services::message::MessageContent;
    use ratatui::layout::Size;
    use stakai::Model;
    use stakpak_shared::models::integrations::openai::{
        ContentPart, FunctionCall, ToolCall, ToolCallResult, ToolCallResultStatus,
    };
    use tokio::sync::mpsc;

    fn build_state() -> AppState {
        AppState::new(AppStateOptions {
            latest_version: None,
            redact_secrets: false,
            privacy_mode: false,
            is_git_repo: false,
            auto_approve_tools: None,
            allowed_tools: None,
            input_tx: None,
            model: Model::default(),
            editor_command: None,
            auth_display_info: (None, None, None),
            board_agent_id: None,
            init_prompt_content: None,
        })
    }

    fn make_tool_result(id: &str) -> ToolCallResult {
        ToolCallResult {
            call: ToolCall {
                id: id.to_string(),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: "run_command".to_string(),
                    arguments: "{}".to_string(),
                },
                metadata: None,
            },
            result: format!("result-{id}"),
            status: ToolCallResultStatus::Success,
        }
    }

    fn make_image_part(label: &str) -> ContentPart {
        ContentPart {
            r#type: "text".to_string(),
            text: Some(label.to_string()),
            image_url: None,
        }
    }

    #[tokio::test]
    async fn flush_pending_messages_merges_queue_into_single_user_message() {
        let mut state = build_state();
        state
            .pending_user_messages
            .push_back(PendingUserMessage::new(
                "first".to_string(),
                Some(vec![make_tool_result("t1")]),
                vec![make_image_part("img-1")],
                "first".to_string(),
            ));
        state
            .pending_user_messages
            .push_back(PendingUserMessage::new(
                "second".to_string(),
                Some(vec![make_tool_result("t2")]),
                vec![make_image_part("img-2")],
                "second".to_string(),
            ));

        let (input_tx, mut input_rx) = mpsc::channel(8);
        let (output_tx, mut output_rx) = mpsc::channel(8);

        flush_pending_user_messages_if_idle(&mut state, &input_tx, &output_tx);

        match output_rx.recv().await {
            Some(OutputEvent::UserMessage(text, Some(tool_calls), image_parts)) => {
                assert_eq!(text, "first\n\nsecond");
                assert_eq!(tool_calls.len(), 2);
                assert_eq!(image_parts.len(), 2);
            }
            other => panic!("unexpected output event: {:?}", other),
        }

        match input_rx.recv().await {
            Some(InputEvent::AddUserMessage(text)) => {
                assert_eq!(text, "first\n\nsecond");
            }
            other => panic!("unexpected input event: {:?}", other),
        }

        assert!(state.pending_user_messages.is_empty());
    }

    #[tokio::test]
    async fn flush_pending_messages_does_not_run_when_busy() {
        let mut state = build_state();
        state
            .loading_manager
            .start_operation(LoadingOperation::StreamProcessing);
        state.loading = true;

        state
            .pending_user_messages
            .push_back(PendingUserMessage::new(
                "queued".to_string(),
                None,
                Vec::new(),
                "queued".to_string(),
            ));

        let (input_tx, mut input_rx) = mpsc::channel(1);
        let (output_tx, mut output_rx) = mpsc::channel(1);

        flush_pending_user_messages_if_idle(&mut state, &input_tx, &output_tx);

        assert!(output_rx.try_recv().is_err());
        assert!(input_rx.try_recv().is_err());
        assert_eq!(state.pending_user_messages.len(), 1);
    }

    #[tokio::test]
    async fn flush_pending_messages_requeues_when_output_channel_is_full() {
        let mut state = build_state();
        state
            .pending_user_messages
            .push_back(PendingUserMessage::new(
                "queued".to_string(),
                Some(vec![make_tool_result("t1")]),
                vec![make_image_part("img-1")],
                "queued".to_string(),
            ));

        let (input_tx, mut input_rx) = mpsc::channel(1);
        let (output_tx, mut output_rx) = mpsc::channel(1);

        let send_res = output_tx.try_send(OutputEvent::RequestTotalUsage);
        assert!(send_res.is_ok());

        flush_pending_user_messages_if_idle(&mut state, &input_tx, &output_tx);

        assert_eq!(state.pending_user_messages.len(), 1);
        match state.pending_user_messages.front() {
            Some(message) => {
                assert_eq!(message.final_input, "queued");
                assert_eq!(message.user_message_text, "queued");
            }
            None => panic!("expected queued pending message"),
        }

        match output_rx.recv().await {
            Some(OutputEvent::RequestTotalUsage) => {}
            other => panic!("unexpected output event: {:?}", other),
        }
        assert!(input_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn flush_pending_messages_falls_back_to_local_user_message_when_input_channel_is_full() {
        let mut state = build_state();
        state
            .pending_user_messages
            .push_back(PendingUserMessage::new(
                "queued".to_string(),
                None,
                Vec::new(),
                "queued".to_string(),
            ));

        let (input_tx, mut input_rx) = mpsc::channel(1);
        let (output_tx, mut output_rx) = mpsc::channel(1);

        let send_res = input_tx.try_send(InputEvent::ToggleCursorVisible);
        assert!(send_res.is_ok());

        flush_pending_user_messages_if_idle(&mut state, &input_tx, &output_tx);

        match output_rx.recv().await {
            Some(OutputEvent::UserMessage(text, _, _)) => {
                assert_eq!(text, "queued");
            }
            other => panic!("unexpected output event: {:?}", other),
        }

        match input_rx.recv().await {
            Some(InputEvent::ToggleCursorVisible) => {}
            other => panic!("unexpected input event: {:?}", other),
        }
        assert!(input_rx.try_recv().is_err());

        assert!(
            state
                .messages
                .iter()
                .any(|message| matches!(&message.content, MessageContent::UserMessage(text) if text == "queued"))
        );
    }

    #[tokio::test]
    async fn update_invokes_flush_when_idle() {
        let mut state = build_state();
        state
            .pending_user_messages
            .push_back(PendingUserMessage::new(
                "from-update".to_string(),
                None,
                Vec::new(),
                "from-update".to_string(),
            ));

        let (input_tx, mut input_rx) = mpsc::channel(8);
        let (output_tx, mut output_rx) = mpsc::channel(8);
        let (shell_tx, _shell_rx) = mpsc::channel(8);

        update(
            &mut state,
            InputEvent::ToggleCursorVisible,
            10,
            80,
            &input_tx,
            &output_tx,
            None,
            &shell_tx,
            Size::new(80, 24),
        );

        match output_rx.recv().await {
            Some(OutputEvent::UserMessage(text, _, _)) => assert_eq!(text, "from-update"),
            other => panic!("unexpected output event: {:?}", other),
        }

        match input_rx.recv().await {
            Some(InputEvent::AddUserMessage(text)) => assert_eq!(text, "from-update"),
            other => panic!("unexpected input event: {:?}", other),
        }
        assert!(state.pending_user_messages.is_empty());
    }
}
