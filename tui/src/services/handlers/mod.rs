//! Event Handlers Module
//!
//! This module contains all event handlers organized by functionality.
//! The main `update()` function routes InputEvents to the appropriate handler modules.

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

use crate::app::{AppState, InputEvent, OutputEvent};
use ratatui::layout::Size;
use tokio::sync::mpsc::Sender;

/// Groups related event channel senders together to reduce function parameter counts
pub struct EventChannels<'a> {
    pub output_tx: &'a Sender<OutputEvent>,
    pub input_tx: &'a Sender<InputEvent>,
    pub shell_tx: &'a Sender<InputEvent>,
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

    // Intercept keys for Shell Mode (only when not loading)
    if state.show_shell_mode
        && state.active_shell_command.is_some()
        && !state.is_dialog_open
        && !state.approval_popup.is_visible()
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
                let visible_height = state.terminal_size.height.saturating_sub(2) as usize;
                let total_lines = state.shell_history_lines.len();
                let max_scroll = total_lines.saturating_sub(visible_height) as u16;
                state.shell_scroll = state.shell_scroll.saturating_add(1).min(max_scroll);
                eprintln!(
                    "ScrollUp: shell_scroll={}, max={}",
                    state.shell_scroll, max_scroll
                );
                return;
            }
            InputEvent::ScrollDown => {
                state.shell_scroll = state.shell_scroll.saturating_sub(1);
                eprintln!("ScrollDown: shell_scroll={}", state.shell_scroll);
                return;
            }
            InputEvent::PageUp => {
                let visible_height = state.terminal_size.height.saturating_sub(2) as usize;
                let total_lines = state.shell_history_lines.len();
                let max_scroll = total_lines.saturating_sub(visible_height) as u16;
                let page_size = state.terminal_size.height / 2;
                state.shell_scroll = state.shell_scroll.saturating_add(page_size).min(max_scroll);
                eprintln!(
                    "PageUp: shell_scroll={}, max={}",
                    state.shell_scroll, max_scroll
                );
                return;
            }
            InputEvent::PageDown => {
                let page_size = state.terminal_size.height / 2;
                state.shell_scroll = state.shell_scroll.saturating_sub(page_size);
                eprintln!("PageDown: shell_scroll={}", state.shell_scroll);
                return;
            }
            InputEvent::HandleEsc => {
                shell::send_shell_input(state, "\x1b");
                return;
            }
            InputEvent::Tab => {
                shell::send_shell_input(state, "\t");
                return;
            }
            InputEvent::AttemptQuit => {
                // Ctrl+C backgrounds the shell instead of sending SIGINT
                shell::handle_shell_mode(state, input_tx);
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
                shell_tx,
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
            tool::handle_stream_tool_result(state, progress);
        }
        InputEvent::MessageToolCalls(tool_calls) => {
            tool::handle_message_tool_calls(state, tool_calls);
        }
        InputEvent::RetryLastToolCall => {
            tool::handle_retry_tool_call(state, input_tx, cancel_tx);
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
        // Shell handlers
        InputEvent::RunShellCommand(command) => {
            shell::handle_run_shell_command(state, command, input_tx);
        }
        InputEvent::ShellMode => {
            shell::handle_shell_mode(state, input_tx);
        }
        InputEvent::ShellOutput(line) => {
            shell::handle_shell_output(state, line);
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
        InputEvent::ToggleContextPopup => {
            popup::handle_toggle_context_popup(state);
        }
        InputEvent::ToggleMoreShortcuts => {
            popup::handle_toggle_more_shortcuts(state);
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
        InputEvent::RunToolCall(_) => {}
        InputEvent::ToolResult(_) => {}
        InputEvent::ApprovalPopupSubmit => {}
    }

    navigation::adjust_scroll(state, message_area_height, message_area_width);
}
