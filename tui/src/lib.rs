mod app;
mod event;
mod terminal;
mod view;

pub use app::{AppState, InputEvent, Message, OutputEvent, update};
pub use event::map_crossterm_event_to_input_event;
pub use terminal::TerminalGuard;
pub use view::view;

use crossterm::{execute, terminal::EnterAlternateScreen};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use tokio::sync::mpsc::{Receiver, Sender};
use regex::Regex;

pub async fn run_tui(
    mut input_rx: Receiver<InputEvent>,
    output_tx: Sender<OutputEvent>,
) -> io::Result<()> {
    let _guard = TerminalGuard;
    crossterm::terminal::enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

    let all_helpers = vec!["/help", "/status", "/clear", "/about", "/quit"];
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

    // Main async update/view loop
    terminal.draw(|f| view::view(f, &state))?;
    let mut should_quit = false;
    while !should_quit {
        tokio::select! {
            Some(event) = input_rx.recv() => {
                eprintln!("event: {:?}", event);
                if let InputEvent::RunCommand(tool_call) = &event {
                    let command = format!("{:?}", tool_call);
                    eprintln!("command: {}", command);
                    app::update(&mut state, InputEvent::ShowConfirmationDialog(command), 10, 40, &output_tx);
                    terminal.draw(|f| view::view(f, &state))?;
                    continue;
                } else  if let  InputEvent::InputSubmittedWith(ref s) = event {
                    if s.starts_with("run_command:") {
                        // Remove the run_command message from chat and show dialog instead
                        let re = Regex::new(r#"command"\s*:\s*"([^"]+)""#).unwrap();
                        let command = re.captures(s).and_then(|cap| cap.get(1)).map(|m| m.as_str().to_string()).unwrap_or_else(|| "unknown".to_string());
                        // Remove last message if it is the run_command message
                        if let Some(last) = state.messages.last() {
                            if last.text.trim().starts_with("run_command:") {
                                state.messages.pop();
                            }
                        }
                        app::update(&mut state, InputEvent::ShowConfirmationDialog(command), 10, 40, &output_tx);
                        terminal.draw(|f| view::view(f, &state))?;
                        continue;
                    }
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
                        if !state.input.trim().is_empty() {
                            let _ = output_tx.try_send(OutputEvent::UserMessage(state.input.clone()));
                        }
                    }
                    app::update(&mut state, event, message_area_height, message_area_width, &output_tx);
                }
            }
        }
        terminal.draw(|f| view::view(f, &state))?;
    }

    println!("Quitting...");
    Ok(())
}
