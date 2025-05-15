use crate::app::Msg;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEventKind};

pub fn map_event_to_msg(event: Event) -> Option<Msg> {
    match event {
        Event::Key(KeyEvent {
            code, modifiers, ..
        }) => match code {
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => Some(Msg::Quit),
            KeyCode::Char(c) => Some(Msg::InputChanged(c)),
            KeyCode::Backspace => Some(Msg::InputBackspace),
            KeyCode::Enter => Some(Msg::InputSubmitted),
            KeyCode::Esc => Some(Msg::Quit),
            KeyCode::Up => Some(Msg::Up),
            KeyCode::Down => Some(Msg::Down),
            KeyCode::PageUp => Some(Msg::PageUp),
            KeyCode::PageDown => Some(Msg::PageDown),
            _ => None,
        },
        Event::Mouse(me) => match me.kind {
            MouseEventKind::ScrollUp => Some(Msg::ScrollUp),
            MouseEventKind::ScrollDown => Some(Msg::ScrollDown),
            _ => None,
        },
        _ => None,
    }
}
