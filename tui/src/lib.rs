mod app;
mod event;
mod terminal;
mod view;

pub use app::{AppState, InputEvent, OutputEvent, SessionInfo};
pub use ratatui::style::Color;

mod services;

use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::{execute, terminal::EnterAlternateScreen};
pub use event::map_crossterm_event_to_input_event;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
pub use terminal::TerminalGuard;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{Duration, interval};
pub use view::view;

pub async fn run_tui(
    mut input_rx: Receiver<InputEvent>,
    output_tx: Sender<OutputEvent>,
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    latest_version: Option<String>,
    redact_secrets: bool,
    privacy_mode: bool,
) -> io::Result<()> {
    let _guard = TerminalGuard;
    crossterm::terminal::enable_raw_mode()?;
    execute!(
        std::io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

    let mut state = AppState::new(latest_version, redact_secrets, privacy_mode);

    // Internal channel for event handling
    let (internal_tx, mut internal_rx) = tokio::sync::mpsc::channel::<InputEvent>(100);
    let internal_tx_thread = internal_tx.clone();
    std::thread::spawn(move || {
        loop {
            if let Ok(event) = crossterm::event::read() {
                if let Some(event) = crate::event::map_crossterm_event_to_input_event(event) {
                    if internal_tx_thread.blocking_send(event).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let shell_event_tx = internal_tx.clone();

    let mut spinner_interval = interval(Duration::from_millis(100));
    // get terminal width
    let terminal_size = terminal.size()?;
    // Main async update/view loop
    terminal.draw(|f| view::view(f, &state))?;
    let mut should_quit = false;
    loop {
        // Check if double Ctrl+C timer expired
        if state.ctrl_c_pressed_once {
            if let Some(timer) = state.ctrl_c_timer {
                if std::time::Instant::now() > timer {
                    state.ctrl_c_pressed_once = false;
                    state.ctrl_c_timer = None;
                }
            }
        }
        tokio::select! {

               Some(event) = input_rx.recv() => {
                   if matches!(event, InputEvent::ShellOutput(_) | InputEvent::ShellError(_) |
                   InputEvent::ShellWaitingForInput | InputEvent::ShellCompleted(_) | InputEvent::ShellClear) {
            // These are shell events, forward them to the shell channel
            let _ = shell_event_tx.send(event).await;
            continue;
        }
                   if let InputEvent::EmergencyClearTerminal = event {
                    emergency_clear_and_redraw(&mut terminal, &state)?;
                    continue;
                   }
                   if let InputEvent::RunToolCall(tool_call) = &event {
                       services::update::update(&mut state, InputEvent::ShowConfirmationDialog(tool_call.clone()), 10, 40, &internal_tx, &output_tx, cancel_tx.clone(), terminal_size, &shell_event_tx);
                       state.poll_autocomplete_results();
                       terminal.draw(|f| view::view(f, &state))?;
                       continue;
                   }
                   if let InputEvent::ToolResult(ref tool_call_result) = event {
                       services::update::clear_streaming_tool_results(&mut state);
                       services::bash_block::render_result_block(tool_call_result, &mut state, terminal_size);
                   }

                   if let InputEvent::Quit = event { should_quit = true; }
                   else {
                       let term_size = terminal.size()?;
                       let term_rect = ratatui::layout::Rect::new(0, 0, term_size.width, term_size.height);
                       let input_height = 3;
                       let margin_height = 2;
                       let dropdown_showing = state.show_helper_dropdown
                           && !state.filtered_helpers.is_empty()
                           && state.input.starts_with('/');
                       let dropdown_height = if dropdown_showing {
                           state.filtered_helpers.len() as u16
                       } else {
                           0
                       };
                       let hint_height = if dropdown_showing { 0 } else { margin_height };
                       let outer_chunks = ratatui::layout::Layout::default()
                           .direction(ratatui::layout::Direction::Vertical)
                           .constraints([
                               ratatui::layout::Constraint::Min(1),
                               ratatui::layout::Constraint::Length(input_height as u16),
                               ratatui::layout::Constraint::Length(dropdown_height),
                               ratatui::layout::Constraint::Length(hint_height),
                           ])
                           .split(term_rect);
                       let message_area_width = outer_chunks[0].width as usize;
                       let message_area_height = outer_chunks[0].height as usize;
                       services::update::update(&mut state, event, message_area_height, message_area_width, &internal_tx, &output_tx, cancel_tx.clone(), terminal_size, &shell_event_tx);
                       state.poll_autocomplete_results();
                   }
               }
               Some(event) = internal_rx.recv() => {
                if let InputEvent::Quit = event { should_quit = true; }
                   else {
                       let term_size = terminal.size()?;
                       let term_rect = ratatui::layout::Rect::new(0, 0, term_size.width, term_size.height);
                       let input_height = 3;
                       let margin_height = 2;
                       let dropdown_showing = state.show_helper_dropdown
                           && !state.filtered_helpers.is_empty()
                           && state.input.starts_with('/');
                       let dropdown_height = if dropdown_showing {
                           state.filtered_helpers.len() as u16
                       } else {
                           0
                       };
                       let hint_height = if dropdown_showing { 0 } else { margin_height };
                       let outer_chunks = ratatui::layout::Layout::default()
                           .direction(ratatui::layout::Direction::Vertical)
                           .constraints([
                               ratatui::layout::Constraint::Min(1),
                               ratatui::layout::Constraint::Length(input_height as u16),
                               ratatui::layout::Constraint::Length(dropdown_height),
                               ratatui::layout::Constraint::Length(hint_height),
                           ])
                           .split(term_rect);
                       let message_area_width = outer_chunks[0].width as usize;
                       let message_area_height = outer_chunks[0].height as usize;
                    //    if let InputEvent::InputSubmitted = event {
                    //     if (state.show_helper_dropdown && state.autocomplete.is_active()) || (state.show_shell_mode && !state.waiting_for_shell_input) {
                    //         // Do nothing for these cases
                    //     } else if !state.show_shell_mode && !state.input.trim().is_empty() && !state.input.trim().starts_with('/') && state.input.trim() != "clear" {
                    //         let _ = output_tx.try_send(OutputEvent::UserMessage(state.input.clone(), state.shell_tool_calls.clone()));
                    //     }
                    //    }
                    if let InputEvent::EmergencyClearTerminal = event {
                    emergency_clear_and_redraw(&mut terminal, &state)?;
                    continue;
                   }
                       services::update::update(&mut state, event, message_area_height, message_area_width, &internal_tx, &output_tx, cancel_tx.clone(), terminal_size, &shell_event_tx);
                       state.poll_autocomplete_results();
                   }
               }
               _ = spinner_interval.tick() => {
                   // Also check double Ctrl+C timer expiry on every tick
                   if state.ctrl_c_pressed_once {
                       if let Some(timer) = state.ctrl_c_timer {
                           if std::time::Instant::now() > timer {
                               state.ctrl_c_pressed_once = false;
                               state.ctrl_c_timer = None;
                           }
                       }
                   }
                   state.spinner_frame = state.spinner_frame.wrapping_add(1);
                   state.poll_autocomplete_results();
                   terminal.draw(|f| view::view(f, &state))?;
               }
           }
        if should_quit {
            break;
        }
        state.poll_autocomplete_results();
        terminal.draw(|f| view::view(f, &state))?;
    }

    println!("Quitting...");
    let _ = shutdown_tx.send(());
    crossterm::terminal::disable_raw_mode()?;
    execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        DisableBracketedPaste
    )?;
    Ok(())
}

pub fn emergency_clear_and_redraw<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: &AppState,
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
    terminal.draw(|f| view::view(f, state))?;

    Ok(())
}
