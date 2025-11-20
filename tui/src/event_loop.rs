//! Event Loop Module
//!
//! Contains the main TUI event loop and related helper functions.

use crate::app::{AppState, AppStateOptions, InputEvent, OutputEvent};
use crate::services::bash_block::render_collapsed_result_block;
use crate::services::detect_term::is_unsupported_terminal;
use crate::services::handlers::tool::{
    clear_streaming_tool_results, update_session_tool_calls_queue,
};
use crate::services::message::Message;
use crate::view::view;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::{execute, terminal::EnterAlternateScreen};
use ratatui::{Terminal, backend::CrosstermBackend};
use stakpak_shared::models::integrations::openai::{AgentModel, ToolCallResultStatus};
use std::io;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{Duration, interval};

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
    model: AgentModel,
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

    let mut state = AppState::new(AppStateOptions {
        latest_version,
        redact_secrets,
        privacy_mode,
        is_git_repo,
        auto_approve_tools,
        allowed_tools,
        input_tx: Some(internal_tx.clone()),
        model,
    });

    // Set the current profile name and rulebook config
    state.current_profile_name = current_profile_name;
    state.rulebook_config = rulebook_config;

    // Add welcome messages after state is created
    let welcome_msg =
        crate::services::helper_block::welcome_messages(state.latest_version.clone(), &state);
    state.messages.extend(welcome_msg);
    let internal_tx_thread = internal_tx.clone();
    std::thread::spawn(move || {
        loop {
            if let Ok(event) = crossterm::event::read()
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
                       state.session_tool_calls_queue.insert(tool_call_result.call.id.clone(), ToolCallStatus::Executed);
                       update_session_tool_calls_queue(&mut state, tool_call_result);
                       if tool_call_result.status == ToolCallResultStatus::Cancelled && tool_call_result.call.function.name == "run_command" {

                            state.latest_tool_call = Some(tool_call_result.call.clone());

                       }
                       render_collapsed_result_block(tool_call_result, &mut state);

                       state.messages.push(Message::render_result_border_block(tool_call_result.clone()));
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
                       let term_rect = ratatui::layout::Rect::new(0, 0, term_size.width, term_size.height);
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
                       let message_area_width = outer_chunks[0].width as usize;
                       let message_area_height = outer_chunks[0].height as usize;
                       crate::services::update::update(&mut state, event, message_area_height, message_area_width, &internal_tx, &output_tx, cancel_tx.clone(), &shell_event_tx, term_size);
                       state.poll_file_search_results();
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
                       let term_rect = ratatui::layout::Rect::new(0, 0, term_size.width, term_size.height);
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
                       let message_area_width = outer_chunks[0].width as usize;
                       let message_area_height = outer_chunks[0].height as usize;
                    if let InputEvent::EmergencyClearTerminal = event {
                    emergency_clear_and_redraw(&mut terminal, &mut state)?;
                    continue;
                   }
                       crate::services::update::update(&mut state, event, message_area_height, message_area_width, &internal_tx, &output_tx, cancel_tx.clone(), &shell_event_tx, term_size);
                       state.poll_file_search_results();
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
                   state.poll_file_search_results();
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
