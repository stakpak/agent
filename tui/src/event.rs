use crate::app::InputEvent;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEventKind};

pub fn map_crossterm_event_to_input_event(event: Event) -> Option<InputEvent> {
    match event {
        Event::Key(KeyEvent {
            code, modifiers, ..
        }) => match code {
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputEvent::Quit)
            }
            KeyCode::Char('j') if modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputEvent::InputChangedNewline)
            }
            KeyCode::Char(c) => Some(InputEvent::InputChanged(c)),
            KeyCode::Backspace => Some(InputEvent::InputBackspace),
            KeyCode::Enter => Some(InputEvent::InputSubmitted),
            KeyCode::Esc => Some(InputEvent::HandleEsc),
            KeyCode::Up => Some(InputEvent::Up),
            KeyCode::Down => Some(InputEvent::Down),
            KeyCode::Left => Some(InputEvent::CursorLeft),
            KeyCode::Right => Some(InputEvent::CursorRight),
            KeyCode::PageUp => Some(InputEvent::PageUp),
            KeyCode::PageDown => Some(InputEvent::PageDown),
            KeyCode::Tab => Some(InputEvent::Tab),
            _ => None,
        },
        Event::Mouse(me) => match me.kind {
            MouseEventKind::ScrollUp => Some(InputEvent::ScrollUp),
            MouseEventKind::ScrollDown => Some(InputEvent::ScrollDown),
            _ => None,
        },
        Event::Resize(w, h) => Some(InputEvent::Resized(w, h)),
        _ => None,
    }
}
