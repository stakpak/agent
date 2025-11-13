use crate::app::InputEvent;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};

pub fn map_crossterm_event_to_input_event(event: Event) -> Option<InputEvent> {
    match event {
        Event::Key(key) => {
            if key.kind != KeyEventKind::Press {
                return None;
            }
            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::AttemptQuit)
                }
                KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::ShowRulebookSwitcher)
                }
                KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::RetryLastToolCall)
                }
                KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::ToggleCollapsedMessages)
                }
                KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::ToggleMouseCapture)
                }
                KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::ShowCommandPalette)
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::HandleCtrlS)
                }
                KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::ExpandNotifications)
                }
                KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::ToggleContextPopup)
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::RulebookSwitcherDeselectAll)
                }
                KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::InputChangedNewline)
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::InputDelete)
                }
                KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::InputDeleteWord)
                }
                KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::ToggleAutoApprove)
                }
                KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::InputCursorStart)
                }
                KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::AutoApproveCurrentTool)
                }
                KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::InputCursorEnd)
                }
                KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::ALT) => {
                    Some(InputEvent::InputCursorNextWord)
                }
                KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::ALT) => {
                    Some(InputEvent::InputCursorPrevWord)
                }
                KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::ShowProfileSwitcher)
                }
                KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::CursorLeft)
                }
                KeyCode::Char('<') if key.modifiers.contains(KeyModifiers::ALT) => {
                    Some(InputEvent::InputCursorPrevWord)
                }
                KeyCode::Char('>') if key.modifiers.contains(KeyModifiers::ALT) => {
                    Some(InputEvent::InputCursorNextWord)
                }
                KeyCode::Char('h') => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        Some(InputEvent::InputDelete)
                    } else if key.modifiers.contains(KeyModifiers::ALT) {
                        Some(InputEvent::InputDeleteWord)
                    } else {
                        Some(InputEvent::InputChanged('h'))
                    }
                }
                KeyCode::Char(c) => Some(InputEvent::InputChanged(c)),
                KeyCode::Backspace => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        Some(InputEvent::InputDelete)
                    } else if key.modifiers.contains(KeyModifiers::ALT) {
                        Some(InputEvent::InputDeleteWord)
                    } else {
                        Some(InputEvent::InputBackspace)
                    }
                }
                KeyCode::Enter => Some(InputEvent::InputSubmitted),
                KeyCode::Esc => Some(InputEvent::HandleEsc),
                KeyCode::Up => Some(InputEvent::Up),
                KeyCode::Down => Some(InputEvent::Down),
                KeyCode::Left => {
                    if key.modifiers.contains(KeyModifiers::ALT)
                        || key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        Some(InputEvent::InputCursorPrevWord)
                    } else {
                        Some(InputEvent::CursorLeft)
                    }
                }
                KeyCode::Right => {
                    if key.modifiers.contains(KeyModifiers::ALT)
                        || key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        Some(InputEvent::InputCursorNextWord)
                    } else {
                        Some(InputEvent::CursorRight)
                    }
                }
                KeyCode::Home => Some(InputEvent::InputCursorStart),
                KeyCode::End => Some(InputEvent::InputCursorEnd),
                KeyCode::PageUp => Some(InputEvent::PageUp),
                KeyCode::PageDown => Some(InputEvent::PageDown),
                KeyCode::Tab => Some(InputEvent::Tab),
                _ => None,
            }
        }
        Event::Mouse(me) => match me.kind {
            MouseEventKind::ScrollUp => Some(InputEvent::ScrollUp),
            MouseEventKind::ScrollDown => Some(InputEvent::ScrollDown),
            _ => None,
        },
        Event::Resize(w, h) => Some(InputEvent::Resized(w, h)),
        Event::Paste(p) => Some(InputEvent::HandlePaste(p)),
        _ => None,
    }
}
