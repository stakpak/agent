mod app;
mod event;
mod terminal;
mod view;
pub use app::{AppState, InputEvent, OutputEvent};

mod services;

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
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) -> io::Result<()> {
    let _guard = TerminalGuard;
    crossterm::terminal::enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

    let all_helpers = vec!["/help", "/status", "/sessions", "/quit"];
    let mut state = AppState::new(all_helpers.clone());

    // Internal channel for event handling
    let (internal_tx, mut internal_rx) = tokio::sync::mpsc::channel::<InputEvent>(100);
    std::thread::spawn(move || {
        loop {
            if let Ok(event) = crossterm::event::read() {
                if let Some(event) = crate::event::map_crossterm_event_to_input_event(event) {
                    if internal_tx.blocking_send(event).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut spinner_interval = interval(Duration::from_millis(100));
    // get terminal width
    let terminal_size = terminal.size()?;
    // Main async update/view loop
    terminal.draw(|f| view::view(f, &state))?;
    let mut should_quit = false;
    loop {
        tokio::select! {
            Some(event) = input_rx.recv() => {
                if let InputEvent::RunToolCall(tool_call) = &event {
                    services::update::update(&mut state, InputEvent::ShowConfirmationDialog(tool_call.clone()), 10, 40, &output_tx, terminal_size);
                    terminal.draw(|f| view::view(f, &state))?;
                    continue;
                }
                if let InputEvent::ToolResult(ref tool_call_result) = event {
                    let tool_call = tool_call_result.call.clone();
                    let result = tool_call_result.result.clone();
                    // Use the new render_bash_result_block function for ToolResults
                    services::bash_block::render_result_block(&tool_call, &result, &mut state);
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
                    services::update::update(&mut state, event, message_area_height, message_area_width, &output_tx, terminal_size);
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
                    if let InputEvent::InputSubmitted = event {
                        // if input starts with / don't submit output event
                        if !state.input.trim().is_empty() && !state.input.trim().starts_with('/') {
                            let _ = output_tx.try_send(OutputEvent::UserMessage(state.input.clone()));
                        }
                    }
                    services::update::update(&mut state, event, message_area_height, message_area_width, &output_tx, terminal_size);
                }
            }
            _ = spinner_interval.tick(), if state.loading => {
                state.spinner_frame = state.spinner_frame.wrapping_add(1);
                terminal.draw(|f| view::view(f, &state))?;
            }
        }
        if should_quit {
            break;
        }
        terminal.draw(|f| view::view(f, &state))?;
    }

    println!("Quitting...");
    let _ = shutdown_tx.send(());
    crossterm::terminal::disable_raw_mode()?;
    execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    Ok(())
}
