mod app;
mod event;
mod services;
mod terminal;
mod view;

pub use app::{AppState, InputEvent, OutputEvent, SessionInfo};
pub use event::map_crossterm_event_to_input_event;
pub use ratatui::style::Color;
pub use terminal::TerminalGuard;
pub use view::view;

use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::{execute, terminal::EnterAlternateScreen};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{Duration, interval};

use crate::services::inline_mode::push_inline_history_messages;

#[allow(clippy::too_many_arguments)]
pub async fn run_tui(
    mut input_rx: Receiver<InputEvent>,
    output_tx: Sender<OutputEvent>,
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    latest_version: Option<String>,
    redact_secrets: bool,
    privacy_mode: bool,
    inline_mode: bool,
) -> io::Result<()> {
    let _guard = TerminalGuard;
    crossterm::terminal::enable_raw_mode()?;

    let mut state = AppState::new(latest_version.clone(), redact_secrets, privacy_mode);
    state.inline_mode = inline_mode;

    // Initialize terminal based on mode
    let mut terminal = if inline_mode {
        ratatui::init_with_options(ratatui::TerminalOptions {
            viewport: ratatui::Viewport::Inline(12), // Dynamic viewport height
        })
    } else {
        // For full screen mode, use alternate screen
        execute!(
            std::io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste
        )?;
        Terminal::new(CrosstermBackend::new(std::io::stdout()))?
    };

    // get terminal width
    let terminal_size: ratatui::prelude::Size = terminal.size()?;

    if state.inline_mode {
        push_inline_history_messages(&state.messages, &mut terminal)?;
    }
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

    // Main async update/view loop
    let mut should_quit = false;
    let mut redraw = true;
    loop {
        if redraw {
            terminal.draw(|f| view::view(f, &state))?;
            redraw = false;
        }

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
                       services::update::update(&mut state, InputEvent::ShowConfirmationDialog(tool_call.clone()), 10, 40, &internal_tx, &output_tx, cancel_tx.clone(), &shell_event_tx, &mut terminal);
                       state.poll_autocomplete_results();
                       redraw = true;


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
                       services::update::update(&mut state, event, message_area_height, message_area_width, &internal_tx, &output_tx, cancel_tx.clone(), &shell_event_tx, &mut terminal);
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

                    if let InputEvent::EmergencyClearTerminal = event {
                    emergency_clear_and_redraw(&mut terminal, &state)?;
                    continue;
                   }
                       services::update::update(&mut state, event, message_area_height, message_area_width, &internal_tx, &output_tx, cancel_tx.clone(), &shell_event_tx, &mut terminal);
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
                   redraw = true;


               }
           }
        if should_quit {
            break;
        }
        state.poll_autocomplete_results();
    }

   if !state.inline_mode {
    println!("Quitting...");
   }
   
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
