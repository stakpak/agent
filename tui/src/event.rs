use crate::app::Msg;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEventKind};

pub fn map_event_to_msg(event: Event) -> Option<Msg> {
    match event {
        Event::Key(KeyEvent {
            code, modifiers, ..
        }) => {
            match code {
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => Some(Msg::Quit),
                KeyCode::Char('j') if modifiers.contains(KeyModifiers::CONTROL) => Some(Msg::InputChangedNewline),                
                KeyCode::Char(c) => Some(Msg::InputChanged(c)),
                KeyCode::Backspace => Some(Msg::InputBackspace),
                KeyCode::Enter => Some(Msg::InputSubmitted),
                KeyCode::Esc => Some(Msg::Quit),
                KeyCode::Up => Some(Msg::Up),
                KeyCode::Down => Some(Msg::Down),
                KeyCode::Left => Some(Msg::CursorLeft),
                KeyCode::Right => Some(Msg::CursorRight),
                KeyCode::PageUp => Some(Msg::PageUp),
                KeyCode::PageDown => Some(Msg::PageDown),
                    _ => None,
            }
        }
        Event::Mouse(me) => match me.kind {
            MouseEventKind::ScrollUp => Some(Msg::ScrollUp),
            MouseEventKind::ScrollDown => Some(Msg::ScrollDown),
            _ => None,
        },
        Event::Resize(w, h) => Some(Msg::Resized(w, h)),
        _ => None,
    }
}
