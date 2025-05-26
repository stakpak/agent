mod app;
mod event;
mod terminal;
mod view;

pub use app::{AppState, InputEvent, Message, OutputEvent, render_bash_block, render_bash_result_block, update};
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
    // Main async update/view loop
    terminal.draw(|f| view::view(f, &state))?;
    let mut should_quit = false;
    loop {
        tokio::select! {
            Some(event) = input_rx.recv() => {
                if let InputEvent::RunCommand(tool_call) = &event {
                    app::update(&mut state, InputEvent::ShowConfirmationDialog(tool_call.clone()), 10, 40, &output_tx);
                    terminal.draw(|f| view::view(f, &state))?;
                    continue;
                }
                if let InputEvent::ToolResult(ref tool_call_result) = event {
                    let tool_call = tool_call_result.call.clone();
                    let result = tool_call_result.result.clone();
                    // Use the new render_bash_result_block function for ToolResults
                    render_bash_result_block(&tool_call, &result, &mut state);
                }
                if let InputEvent::Quit = event { should_quit = true; }
                else {
                    let term_size = terminal.size()?;
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
                        .split(term_size);
                    let message_area_width = outer_chunks[0].width as usize;
                    let message_area_height = outer_chunks[0].height as usize;
                    app::update(&mut state, event, message_area_height, message_area_width, &output_tx);
                }
            }
            Some(event) = internal_rx.recv() => {
                if let InputEvent::Quit = event { should_quit = true; }
                else {
                    let term_size = terminal.size()?;
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
                        .split(term_size);
                    let message_area_width = outer_chunks[0].width as usize;
                    let message_area_height = outer_chunks[0].height as usize;
                    if let InputEvent::InputSubmitted = event {
                        // if input starts with / don't submit output event
                        if !state.input.trim().is_empty() && !state.input.trim().starts_with('/') {
                            let _ = output_tx.try_send(OutputEvent::UserMessage(state.input.clone()));
                        }
                    }
                    app::update(&mut state, event, message_area_height, message_area_width, &output_tx);
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
    Ok(())
}