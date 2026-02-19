// TUI crate performs heavy string slicing for text rendering, markdown parsing,
// cursor positioning, and layout. All indices come from find()/rfind()/char_indices()
// on the same strings. Allowing at crate level to avoid 120+ individual annotations.
#![allow(clippy::string_slice)]

mod app;
mod constants;
mod event;
mod event_loop;
mod terminal;
pub mod utils;
mod view;
pub use app::{
    AppState, ExistingPlanPrompt, InputEvent, LoadingOperation, OutputEvent, SessionInfo,
};
pub use event_loop::{RulebookConfig, run_tui};
pub use ratatui::style::Color;

pub mod services;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
pub use event::map_crossterm_event_to_input_event;
use ratatui::style::Style;
use std::io;
pub use terminal::TerminalGuard;
pub use view::view;

use crate::services::message::{Message, invalidate_message_lines_cache};

pub fn toggle_mouse_capture(state: &mut AppState) -> io::Result<()> {
    state.mouse_capture_enabled = !state.mouse_capture_enabled;

    if state.mouse_capture_enabled {
        execute!(std::io::stdout(), EnableMouseCapture)?;
    } else {
        execute!(std::io::stdout(), DisableMouseCapture)?;
    }

    let status = if state.mouse_capture_enabled {
        "enabled"
    } else {
        "disabled . Ctrl+L to enable"
    };

    let color = if state.mouse_capture_enabled {
        Color::LightGreen
    } else {
        Color::LightRed
    };
    state.messages.push(Message::info("SPACING_MARKER", None));
    state.messages.push(Message::info(
        format!("Mouse capture {}", status),
        Some(Style::default().fg(color)),
    ));
    state.messages.push(Message::info("SPACING_MARKER", None));

    // Invalidate cache so the new messages are rendered
    invalidate_message_lines_cache(state);

    Ok(())
}
