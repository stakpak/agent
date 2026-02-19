//! Event Loop Module
//!
//! Contains the main TUI event loop and related helper functions.

use crate::app::{AppState, AppStateOptions, InputEvent, OutputEvent};
use crate::services::detect_term::is_unsupported_terminal;
use crate::services::handlers::tool::{
    clear_streaming_tool_results, handle_tool_result, update_session_tool_calls_queue,
};
use crate::services::message::Message;
use crate::view::view;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::{execute, terminal::EnterAlternateScreen};
use ratatui::{Terminal, backend::CrosstermBackend};
use stakai::Model;
use stakpak_shared::models::integrations::openai::ToolCallResultStatus;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::interval;

use crate::app::ToolCallStatus;
use crate::terminal::TerminalGuard;

// Rulebook config struct (re-defined here to avoid circular dependency)
#[derive(Clone, Debug)]
pub struct RulebookConfig {
    pub include: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
    pub include_tags: Option<Vec<String>>,
    pub exclude_tags: Option<Vec<String>>,
}

#[allow(clippy::too_many_arguments)]
pub async fn run_tui(
    mut input_rx: Receiver<InputEvent>,
    output_tx: Sender<OutputEvent>,
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    latest_version: Option<String>,
    redact_secrets: bool,
    privacy_mode: bool,
    is_git_repo: bool,
    auto_approve_tools: Option<&Vec<String>>,
    allowed_tools: Option<&Vec<String>>,
    current_profile_name: String,
    rulebook_config: Option<RulebookConfig>,
    model: Model,
    editor_command: Option<String>,
    auth_display_info: (Option<String>, Option<String>, Option<String>),
    init_prompt_content: Option<String>,
    send_init_prompt_on_start: bool,
) -> io::Result<()> {
    let _guard = TerminalGuard;

    crossterm::terminal::enable_raw_mode()?;

    // Detect terminal support for mouse capture
    #[cfg(unix)]
    let terminal_info = crate::services::detect_term::detect_terminal();
    #[cfg(unix)]
    let enable_mouse_capture = is_unsupported_terminal(&terminal_info.emulator);

    execute!(
        std::io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste
    )?;

    #[cfg(unix)]
    if enable_mouse_capture {
        execute!(std::io::stdout(), EnableMouseCapture)?;
    } else {
        execute!(std::io::stdout(), DisableMouseCapture)?;
    }

    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

    let term_size = terminal.size()?;

    // Create internal channel for event handling (needed for error reporting during initialization)
    let (internal_tx, mut internal_rx) = tokio::sync::mpsc::channel::<InputEvent>(100);

    // Get board_agent_id from environment variable
    let board_agent_id = std::env::var("AGENT_BOARD_AGENT_ID").ok();

    let mut state = AppState::new(AppStateOptions {
        latest_version,
        redact_secrets,
        privacy_mode,
        is_git_repo,
        auto_approve_tools,
        allowed_tools,
        input_tx: Some(internal_tx.clone()),
        model,
        editor_command,
        auth_display_info,
        board_agent_id,
        init_prompt_content,
    });

    // Set mouse_capture_enabled based on terminal detection (matches the execute logic above)
    #[cfg(unix)]
    {
        state.mouse_capture_enabled = enable_mouse_capture;
    }
    #[cfg(not(unix))]
    {
        state.mouse_capture_enabled = false;
    }

    // Set the current profile name and rulebook config
    state.current_profile_name = current_profile_name;
    state.rulebook_config = rulebook_config;

    // Add welcome messages after state is created
    let welcome_msg =
        crate::services::helper_block::welcome_messages(state.latest_version.clone(), &state);
    state.messages.extend(welcome_msg);

    // Trigger initial board tasks refresh if agent ID is configured
    if state.board_agent_id.is_some() {
        let _ = internal_tx.try_send(InputEvent::RefreshBoardTasks);
    }

    // When started via `stakpak init`, add init prompt as user message and send to backend
    if send_init_prompt_on_start
        && let Some(prompt) = state.init_prompt_content.clone()
        && !prompt.trim().is_empty()
    {
        state.messages.push(Message::user(prompt.clone(), None));
        crate::services::message::invalidate_message_lines_cache(&mut state);
        let _ = output_tx.try_send(OutputEvent::UserMessage(prompt, None, Vec::new()));
    }

    let internal_tx_thread = internal_tx.clone();
    // Create atomic pause flag for input thread
    let input_paused = Arc::new(AtomicBool::new(false));
    let input_paused_thread = input_paused.clone();

    // Spawn input handling thread
    // This thread reads from crossterm and converts to internal events
    // It must be pausable when we yield terminal control to external programs (like nano/vim)
    std::thread::spawn(move || {
        loop {
            // Check if we should pause input reading
            if input_paused_thread.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }

            // Use poll with timeout instead of blocking read to allow checking pause flag
            if let Ok(true) = crossterm::event::poll(Duration::from_millis(50))
                && let Ok(event) = crossterm::event::read()
                && let Some(event) = crate::event::map_crossterm_event_to_input_event(event)
                && internal_tx_thread.blocking_send(event).is_err()
            {
                break;
            }
        }
    });

    let shell_event_tx = internal_tx.clone();

    let mut spinner_interval = interval(Duration::from_millis(100));

    // Main async update/view loop
    terminal.draw(|f| view(f, &mut state))?;
    let mut should_quit = false;

    // Scroll batching: count consecutive scroll events to process in one frame
    // These are reset at the start of each scroll batch
    #[allow(unused_assignments)]
    let mut pending_scroll_up: i32 = 0;
    #[allow(unused_assignments)]
    let mut pending_scroll_down: i32 = 0;

    loop {
        // Check if double Ctrl+C timer expired
        if state.ctrl_c_pressed_once
            && let Some(timer) = state.ctrl_c_timer
            && std::time::Instant::now() > timer
        {
            state.ctrl_c_pressed_once = false;
            state.ctrl_c_timer = None;
        }
        tokio::select! {
               event = input_rx.recv() => {
                let Some(event) = event else {
                    should_quit = true;
                    continue;
                };
                   if matches!(event, InputEvent::ShellOutput(_) | InputEvent::ShellError(_) |
                   InputEvent::ShellWaitingForInput | InputEvent::ShellCompleted(_) | InputEvent::ShellClear) {
            // These are shell events, forward them to the shell channel
            let _ = shell_event_tx.send(event).await;
            continue;
        }
                   if let InputEvent::EmergencyClearTerminal = event {
                    emergency_clear_and_redraw(&mut terminal, &mut state)?;
                    continue;
                   }
                   if let InputEvent::RunToolCall(tool_call) = &event {
                       crate::services::update::update(&mut state, InputEvent::ShowConfirmationDialog(tool_call.clone()), 10, 40, &internal_tx, &output_tx, cancel_tx.clone(), &shell_event_tx, term_size);
                       state.poll_file_search_results();
                       terminal.draw(|f| view(f, &mut state))?;
                       continue;
                   }
                   if let InputEvent::ToolResult(ref tool_call_result) = event {
                       clear_streaming_tool_results(&mut state);

                       // For run_command, also remove any message that matches the tool call ID
                       // (handles case where streaming message uses tool_call_id directly)
                       // The tool call ID is a String, but message IDs are Uuid
                       if let Ok(tool_call_uuid) = uuid::Uuid::parse_str(&tool_call_result.call.id) {
                           state.messages.retain(|m| m.id != tool_call_uuid);
                       }

                       state.session_tool_calls_queue.insert(tool_call_result.call.id.clone(), ToolCallStatus::Executed);
                       update_session_tool_calls_queue(&mut state, tool_call_result);
                       let tool_name = crate::utils::strip_tool_name(&tool_call_result.call.function.name);

                       if tool_call_result.status == ToolCallResultStatus::Cancelled && tool_name == "run_command" {
                           state.latest_tool_call = Some(tool_call_result.call.clone());
                       }
                       // Determine the state for run_command tools
                       let is_cancelled = tool_call_result.status == ToolCallResultStatus::Cancelled;
                       let is_error = tool_call_result.status == ToolCallResultStatus::Error;

                       if (is_cancelled || is_error) && tool_name != "run_command" {
                           // For non-run_command tools with cancelled/error, use old renderer
                           state.messages.push(Message::render_result_border_block(tool_call_result.clone()));
                           state.messages.push(Message::render_full_content_message(tool_call_result.clone()));
                       } else {
                           match tool_name {
                               "str_replace" | "create" => {
                                   // TUI: Show diff result block with yellow border (is_collapsed: None)
                                   state.messages.push(Message::render_result_border_block(tool_call_result.clone()));
                                   // Full screen popup: Show diff-only view without border (is_collapsed: Some(true))
                                   state.messages.push(Message::render_collapsed_message(tool_call_result.call.clone()));
                               }
                               "run_command_task" => {
                                   // TUI: bordered result block (is_collapsed: None)
                                   state.messages.push(Message::render_result_border_block(tool_call_result.clone()));
                                   // Full screen popup: full content without border (is_collapsed: Some(true))
                                   state.messages.push(Message::render_full_content_message(tool_call_result.clone()));
                               }
                                "run_command" => {
                                    // Use unified run command block with appropriate state
                                    let command = crate::services::handlers::shell::extract_command_from_tool_call(&tool_call_result.call)
                                        .unwrap_or_else(|_| "command".to_string());
                                    let run_state = if is_error {
                                        crate::services::bash_block::RunCommandState::Error
                                    } else if is_cancelled {
                                        // Cancelled could be user rejection or actual cancellation
                                        // Use Cancelled for now (user pressed ESC during execution)
                                        crate::services::bash_block::RunCommandState::Cancelled
                                    } else {
                                        crate::services::bash_block::RunCommandState::Completed
                                    };

                                    let run_cmd_msg = Message::render_run_command_block(
                                        command,
                                        Some(tool_call_result.result.clone()),
                                        run_state,
                                        None,
                                    );
                                    let popup_msg = Message::render_full_content_message(tool_call_result.clone());

                                    // If shell is visible/running, insert cancelled block BEFORE the shell message
                                    // so the order is: cancelled command -> shell box
                                    if is_cancelled && state.shell_popup_visible {
                                        if let Some(shell_msg_id) = state.interactive_shell_message_id {
                                            // Find the position of the shell message
                                            if let Some(pos) = state.messages.iter().position(|m| m.id == shell_msg_id) {
                                                // Insert cancelled block and popup before shell message
                                                state.messages.insert(pos, popup_msg);
                                                state.messages.insert(pos, run_cmd_msg);
                                            } else {
                                                // Shell message not found, just push normally
                                                state.messages.push(run_cmd_msg);
                                                state.messages.push(popup_msg);
                                            }
                                        } else {
                                            // No shell message ID, just push normally
                                            state.messages.push(run_cmd_msg);
                                            state.messages.push(popup_msg);
                                        }
                                    } else {
                                        // Normal case: just push to the end
                                        state.messages.push(run_cmd_msg);
                                        state.messages.push(popup_msg);
                                    }
                                }
                                "read" | "view" | "read_file" => {
                                    // View file tool - show compact view with file icon and line count
                                    // Extract file path and optional grep/glob from tool call arguments
                                    let (file_path, grep, glob) = crate::services::handlers::tool::extract_view_params_from_tool_call(&tool_call_result.call);
                                    let file_path = file_path.unwrap_or_else(|| "file".to_string());
                                    let total_lines = tool_call_result.result.lines().count();
                                    state.messages.push(Message::render_view_file_block(file_path.clone(), total_lines, grep.clone(), glob.clone()));
                                    // Full screen popup: same compact view without borders
                                    state.messages.push(Message::render_view_file_block_popup(file_path, total_lines, grep, glob));
                                }
                               _ => {
                                   // TUI: collapsed command message - last 3 lines (is_collapsed: None)
                                   state.messages.push(Message::render_collapsed_command_message(tool_call_result.clone()));
                                   // Full screen popup: full content (is_collapsed: Some(true))
                                   state.messages.push(Message::render_full_content_message(tool_call_result.clone()));
                               }
                           }

                           // Handle file changes for the Changeset (only for non-cancelled/error)
                           if !is_cancelled && !is_error {
                               handle_tool_result(&mut state, tool_call_result.clone());
                           }
                       }
                       // Invalidate cache and scroll to bottom to show the result
                       crate::services::message::invalidate_message_lines_cache(&mut state);
                       state.stay_at_bottom = true;

                       // Refresh board tasks after tool execution (agent may have updated tasks)
                       // Always trigger refresh - the handler will extract agent_id from messages if needed
                       let _ = internal_tx.try_send(InputEvent::RefreshBoardTasks);
                   }
                   if let InputEvent::ToggleMouseCapture = event {
                       #[cfg(unix)]
                       toggle_mouse_capture_with_redraw(&mut terminal, &mut state)?;
                       continue;
                   }

                   if let InputEvent::Quit = event {
                       should_quit = true;
                   }
                   else {
                       // Calculate main area width accounting for side panel
                       let main_area_width = if state.show_side_panel {
                           term_size.width.saturating_sub(32 + 1) // side panel width + margin
                       } else {
                           term_size.width
                       };
                       let term_rect = ratatui::layout::Rect::new(0, 0, main_area_width, term_size.height);
                       let input_height = 3;
                       let margin_height = 2;
                       let dropdown_showing = state.show_helper_dropdown
                           && ((!state.filtered_helpers.is_empty() && state.input().starts_with('/'))
                               || !state.filtered_files.is_empty());
                       let dropdown_height = if dropdown_showing {
                           state.filtered_helpers.len() as u16
                       } else {
                           0
                       };
                       let hint_height = if dropdown_showing { 0 } else { margin_height };
                       let outer_chunks = ratatui::layout::Layout::default()
                           .direction(ratatui::layout::Direction::Vertical)
                           .constraints([
                               ratatui::layout::Constraint::Min(1), // messages
                               ratatui::layout::Constraint::Length(1), // loading indicator
                               ratatui::layout::Constraint::Length(input_height as u16),
                               ratatui::layout::Constraint::Length(dropdown_height),
                               ratatui::layout::Constraint::Length(hint_height),
                           ])
                           .split(term_rect);
                       // Subtract 2 for padding (matches view.rs padded_message_area)
                       let message_area_width = outer_chunks[0].width.saturating_sub(2) as usize;
                       let message_area_height = outer_chunks[0].height as usize;
                        crate::services::update::update(&mut state, event, message_area_height, message_area_width, &internal_tx, &output_tx, cancel_tx.clone(), &shell_event_tx, term_size);
                        state.poll_file_search_results();
                       // Handle pending editor open request
                       if let Some(file_path) = state.pending_editor_open.take() {
                           // Disable mouse capture before opening editor to prevent weird input
                           let was_mouse_capture_enabled = state.mouse_capture_enabled;
                           if was_mouse_capture_enabled {
                               let _ = execute!(std::io::stdout(), DisableMouseCapture);
                               state.mouse_capture_enabled = false;
                           }

                           match crate::services::editor::open_in_editor(
                               &mut terminal,
                               &state.editor_command,
                               &file_path,
                               None,
                           ) {
                               Ok(()) => {
                                   // Editor closed successfully
                               }
                               Err(error) => {
                                   // Show error message
                                   state.messages.push(Message::info(
                                       format!("Failed to open editor: {}", error),
                                       Some(ratatui::style::Style::default().fg(ratatui::style::Color::Red)),
                                   ));
                               }
                           }

                           // Restore mouse capture if it was enabled before
                           if was_mouse_capture_enabled {
                               let _ = execute!(std::io::stdout(), EnableMouseCapture);
                               state.mouse_capture_enabled = true;
                           }
                       }
                   }
               }
               event = internal_rx.recv() => {

                let Some(event) = event else {
                    should_quit = true;
                    continue;
                };

                if let InputEvent::ToggleMouseCapture = event {
                    #[cfg(unix)]
                    toggle_mouse_capture_with_redraw(&mut terminal, &mut state)?;
                    continue;
                }
                if let InputEvent::Quit = event {
                    should_quit = true;
                }
                   else {
                       let term_size = terminal.size()?;
                       // Calculate main area width accounting for side panel
                       let main_area_width = if state.show_side_panel {
                           term_size.width.saturating_sub(32 + 1) // side panel width + margin
                       } else {
                           term_size.width
                       };
                       let term_rect = ratatui::layout::Rect::new(0, 0, main_area_width, term_size.height);
                       let input_height = 3;
                       let margin_height = 2;
                       let dropdown_showing = state.show_helper_dropdown
                           && ((!state.filtered_helpers.is_empty() && state.input().starts_with('/'))
                               || !state.filtered_files.is_empty());
                       let dropdown_height = if dropdown_showing {
                           state.filtered_helpers.len() as u16
                       } else {
                           0
                       };
                       let hint_height = if dropdown_showing { 0 } else { margin_height };
                       let outer_chunks = ratatui::layout::Layout::default()
                           .direction(ratatui::layout::Direction::Vertical)
                           .constraints([
                               ratatui::layout::Constraint::Min(1), // messages
                               ratatui::layout::Constraint::Length(1), // loading indicator
                               ratatui::layout::Constraint::Length(input_height as u16),
                               ratatui::layout::Constraint::Length(dropdown_height),
                               ratatui::layout::Constraint::Length(hint_height),
                           ])
                           .split(term_rect);
                       // Subtract 2 for padding (matches view.rs padded_message_area)
                       let message_area_width = outer_chunks[0].width.saturating_sub(2) as usize;
                       let message_area_height = outer_chunks[0].height as usize;
                    if let InputEvent::EmergencyClearTerminal = event {
                    emergency_clear_and_redraw(&mut terminal, &mut state)?;
                    continue;
                   }

                   // Batch scroll events: if this is a scroll event, drain any pending scroll events
                   // and combine them into a single scroll operation for better performance
                   if matches!(event, InputEvent::ScrollUp | InputEvent::ScrollDown) {
                       pending_scroll_up = 0;
                       pending_scroll_down = 0;

                       // Count the initial event
                       match event {
                           InputEvent::ScrollUp => pending_scroll_up += 1,
                           InputEvent::ScrollDown => pending_scroll_down += 1,
                           _ => {}
                       }

                       // Drain any additional scroll events from the channel (non-blocking)
                       let mut other_event: Option<InputEvent> = None;
                       while let Ok(next_event) = internal_rx.try_recv() {
                           match next_event {
                               InputEvent::ScrollUp => pending_scroll_up += 1,
                               InputEvent::ScrollDown => pending_scroll_down += 1,
                               // Non-scroll event - save it for later
                               other => {
                                   other_event = Some(other);
                                   break;
                               }
                           }
                       }

                       // Process net scroll (combine up and down into single direction)
                       let net_scroll = pending_scroll_down - pending_scroll_up;
                       if net_scroll > 0 {
                           // More downs than ups - scroll down by accumulated amount
                           for _ in 0..net_scroll {
                               crate::services::update::update(&mut state, InputEvent::ScrollDown, message_area_height, message_area_width, &internal_tx, &output_tx, cancel_tx.clone(), &shell_event_tx, term_size);
                           }
                       } else if net_scroll < 0 {
                           // More ups than downs - scroll up by accumulated amount
                           for _ in 0..(-net_scroll) {
                               crate::services::update::update(&mut state, InputEvent::ScrollUp, message_area_height, message_area_width, &internal_tx, &output_tx, cancel_tx.clone(), &shell_event_tx, term_size);
                           }
                       }

                       // If we encountered a non-scroll event, process it too
                       if let Some(other) = other_event {
                           crate::services::update::update(&mut state, other, message_area_height, message_area_width, &internal_tx, &output_tx, cancel_tx.clone(), &shell_event_tx, term_size);
                       }
                   } else {
                       crate::services::update::update(&mut state, event, message_area_height, message_area_width, &internal_tx, &output_tx, cancel_tx.clone(), &shell_event_tx, term_size);
                   }
                   state.poll_file_search_results();

                        // Handle pending editor open request
                         if let Some(file_path) = state.pending_editor_open.take() {
                             // Pause input thread to avoid stealing input from editor
                             input_paused.store(true, Ordering::Relaxed);
                             // Small delay to ensure input thread cycle completes
                             std::thread::sleep(Duration::from_millis(10));

                             // Disable mouse capture before opening editor to prevent weird input
                             let was_mouse_capture_enabled = state.mouse_capture_enabled;
                             if was_mouse_capture_enabled {
                                 let _ = execute!(std::io::stdout(), DisableMouseCapture);
                                 state.mouse_capture_enabled = false;
                             }

                             match crate::services::editor::open_in_editor(
                                 &mut terminal,
                                 &state.editor_command,
                                 &file_path,
                                 None,
                             ) {
                                 Ok(()) => {
                                     // Editor closed successfully
                                 }
                                 Err(error) => {
                                     // Show error message
                                     state.messages.push(Message::info(
                                         format!("Failed to open editor: {}", error),
                                         Some(ratatui::style::Style::default().fg(ratatui::style::Color::Red)),
                                     ));
                                 }
                             }

                             // Restore mouse capture if it was enabled before
                             if was_mouse_capture_enabled {
                                 let _ = execute!(std::io::stdout(), EnableMouseCapture);
                                 state.mouse_capture_enabled = true;
                             }

                             // Resume input thread
                             input_paused.store(false, Ordering::Relaxed);
                         }

                        state.update_session_empty_status();
                    }
                }
               _ = spinner_interval.tick() => {
                   // Also check double Ctrl+C timer expiry on every tick
                   if state.ctrl_c_pressed_once
                       && let Some(timer) = state.ctrl_c_timer
                           && std::time::Instant::now() > timer {
                               state.ctrl_c_pressed_once = false;
                               state.ctrl_c_timer = None;
                           }
                   state.spinner_frame = state.spinner_frame.wrapping_add(1);
                   // Update shell cursor blink (toggles every ~5 ticks = 500ms)
                   crate::services::shell_popup::update_cursor_blink(&mut state);
                   state.poll_file_search_results();

                   // Poll plan file and handle status transitions
                   if let Some((old_status, new_status)) = state.poll_plan_file() {
                       use crate::services::plan::PlanStatus;
                       match new_status {
                           PlanStatus::PendingReview => {
                               // Auto-open plan review when agent sets status to pending_review
                               if !state.plan_review_auto_opened {
                                   state.plan_review_auto_opened = true;
                                   crate::services::plan_review::open_plan_review(&mut state);
                                   // Show system message
                                   crate::services::helper_block::push_styled_message(
                                       &mut state,
                                       " Plan ready for review. Opening reviewer... (ctrl+p to toggle)",
                                       ratatui::style::Color::Cyan,
                                       ">> ",
                                       ratatui::style::Color::Cyan,
                                   );
                               }
                           }
                           PlanStatus::Approved => {
                               // External approval (e.g. agent set it) — no extra state to update
                               let _ = old_status; // suppress unused warning
                           }
                           PlanStatus::Drafting => {
                               // New revision — reset auto-open flag so next pending_review triggers it
                               state.plan_review_auto_opened = false;
                           }
                       }
                   }

                   terminal.draw(|f| view(f, &mut state))?;
               }
           }
        if should_quit {
            break;
        }
        state.poll_file_search_results();
        state.update_session_empty_status();
        terminal.draw(|f| view(f, &mut state))?;
    }

    let _ = shutdown_tx.send(());
    crossterm::terminal::disable_raw_mode()?;
    execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        DisableBracketedPaste,
        DisableMouseCapture
    )?;
    Ok(())
}

pub fn emergency_clear_and_redraw<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: &mut AppState,
) -> io::Result<()> {
    use crossterm::{
        cursor::MoveTo,
        execute,
        terminal::{Clear, ClearType},
    };

    // Nuclear option - clear everything including scrollback
    execute!(
        std::io::stdout(),
        Clear(ClearType::All),
        Clear(ClearType::Purge),
        MoveTo(0, 0)
    )?;

    // Force a complete redraw of the TUI
    terminal.clear()?;
    terminal.draw(|f| view(f, state))?;

    Ok(())
}

fn toggle_mouse_capture_with_redraw<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: &mut AppState,
) -> io::Result<()> {
    crate::toggle_mouse_capture(state)?;
    emergency_clear_and_redraw(terminal, state)?;
    Ok(())
}
