use crate::app::InputEvent;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};

pub fn map_crossterm_event_to_input_event(event: Event) -> Option<InputEvent> {
    match event {
        Event::Key(key) => {
            if key.kind != KeyEventKind::Press {
                return None;
            }
            match key.code {
                KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::HandleClipboardImagePaste)
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::AttemptQuit)
                }
                KeyCode::Char('k') => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        Some(InputEvent::ShowRulebookSwitcher)
                    } else if key.modifiers.contains(KeyModifiers::ALT) {
                        Some(InputEvent::ScrollUp)
                    } else {
                        Some(InputEvent::InputChanged('k'))
                    }
                }
                KeyCode::Char('j') => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        Some(InputEvent::InputChangedNewline)
                    } else if key.modifiers.contains(KeyModifiers::ALT) {
                        Some(InputEvent::ScrollDown)
                    } else {
                        Some(InputEvent::InputChanged('j'))
                    }
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
                    // Ctrl+P: toggle plan review when in plan mode, otherwise command palette
                    Some(InputEvent::TogglePlanReview)
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::HandleCtrlS)
                }
                KeyCode::Char('g') if key.modifiers == (KeyModifiers::CONTROL) => {
                    Some(InputEvent::ShowFileChangesPopup)
                }
                KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::InputCursorEnd)
                }
                KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::FileChangesRevertFile)
                }
                KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::FileChangesRevertAll)
                }
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::FileChangesOpenEditor)
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::RulebookSwitcherDeselectAll)
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
                    Some(InputEvent::ToggleSidePanel)
                }
                KeyCode::Char('m') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(InputEvent::AutoApproveCurrentTool)
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
                KeyCode::Up => {
                    if key
                        .modifiers
                        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL)
                    {
                        Some(InputEvent::ScrollUp)
                    } else {
                        Some(InputEvent::Up)
                    }
                }
                KeyCode::Down => {
                    if key
                        .modifiers
                        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL)
                    {
                        Some(InputEvent::ScrollDown)
                    } else {
                        Some(InputEvent::Down)
                    }
                }
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
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                Some(InputEvent::MouseDragStart(me.column, me.row))
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                Some(InputEvent::MouseDrag(me.column, me.row))
            }
            MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                Some(InputEvent::MouseDragEnd(me.column, me.row))
            }
            MouseEventKind::Moved => Some(InputEvent::MouseMove(me.column, me.row)),
            _ => None,
        },
        Event::Resize(w, h) => Some(InputEvent::Resized(w, h)),
        Event::Paste(p) => Some(InputEvent::HandlePaste(p)),
        _ => None,
    }
}
