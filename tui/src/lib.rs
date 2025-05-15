mod app;
mod event;
mod terminal;
mod view;

pub use app::{AppState, Message, Msg, update};
pub use event::map_event_to_msg;
pub use terminal::TerminalGuard;
pub use view::view;

use crossterm::{execute, terminal::EnterAlternateScreen};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use tokio::sync::mpsc::{Receiver, Sender};

pub async fn run_tui(mut input_rx: Receiver<Msg>, output_tx: Sender<String>) -> io::Result<()> {
    let _guard = TerminalGuard;
    crossterm::terminal::enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

    let all_helpers = vec!["/help", "/status", "/clear", "/about", "/quit"];
    let mut state = AppState::new(all_helpers.clone());

    // Internal channel for event handling
    let (internal_tx, mut internal_rx) = tokio::sync::mpsc::channel::<Msg>(100);
    std::thread::spawn(move || {
        loop {
            if let Ok(event) = crossterm::event::read() {
                if let Some(msg) = crate::event::map_event_to_msg(event) {
                    if internal_tx.blocking_send(msg).is_err() {
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
            Some(msg) = input_rx.recv() => {
                if let Msg::Quit = msg { should_quit = true; }
                else {
                    let term_height = terminal.size()?.height as usize;
                    app::update(&mut state, msg, term_height);
                }
            }
            Some(msg) = internal_rx.recv() => {
                if let Msg::Quit = msg { should_quit = true; }
                else {
                    let term_height = terminal.size()?.height as usize;
                    if let Msg::InputSubmitted = msg {
                        if !state.input.trim().is_empty() {
                            let _ = output_tx.try_send(state.input.clone());
                        }
                    }
                    app::update(&mut state, msg, term_height);
                }
            }
        }
        terminal.draw(|f| view::view(f, &state))?;
    }

    println!("Quitting...");
    Ok(())
}
