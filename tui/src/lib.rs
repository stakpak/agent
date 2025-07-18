mod app;
mod event;
mod terminal;
mod view;
pub use app::{AppState, InputEvent, OutputEvent, SessionInfo};
pub use ratatui::style::Color;

mod services;

use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::{execute, terminal::EnterAlternateScreen};
pub use event::map_crossterm_event_to_input_event;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
pub use terminal::TerminalGuard;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{Duration, interval};
pub use view::view;

// Thread-local storage for editor state
use std::cell::RefCell;
use std::thread_local;

thread_local! {
    static EDITOR_STATE: RefCell<Option<edtui::EditorState>> = RefCell::new(None);
    static EDITOR_EVENT_HANDLER: RefCell<Option<edtui::EditorEventHandler>> = RefCell::new(None);
}

pub async fn run_tui(
    mut input_rx: Receiver<InputEvent>,
    output_tx: Sender<OutputEvent>,
    cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    latest_version: Option<String>,
    redact_secrets: bool,
    privacy_mode: bool,
) -> io::Result<()> {
    let _guard = TerminalGuard;
    crossterm::terminal::enable_raw_mode()?;
    execute!(
        std::io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

    let all_helpers = vec!["/help", "/status", "/sessions", "/memorize", "/quit"];
    let mut state = AppState::new(
        all_helpers.clone(),
        latest_version,
        redact_secrets,
        privacy_mode,
    );

    // Internal channel for event handling
    let (internal_tx, mut internal_rx) = tokio::sync::mpsc::channel::<InputEvent>(100);
    let internal_tx_thread = internal_tx.clone();
    std::thread::spawn(move || {
        loop {
            if let Ok(event) = crossterm::event::read() {
                if let Some(event) = crate::event::map_crossterm_event_to_input_event(event) {
                    if internal_tx_thread.blocking_send(event).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let shell_event_tx = internal_tx.clone();

    let mut spinner_interval = interval(Duration::from_millis(100));
    // get terminal width
    let terminal_size = terminal.size()?;
    // Main async update/view loop
    terminal.draw(|f| view::view(f, &mut state))?;
    let mut should_quit = false;
    loop {
        tokio::select! {

               Some(event) = input_rx.recv() => {
                   if matches!(event, InputEvent::ShellOutput(_) | InputEvent::ShellError(_) |
                   InputEvent::ShellInputRequest(_) | InputEvent::ShellCompleted(_) | InputEvent::ShellClear) {
            // These are shell events, forward them to the shell channel
            let _ = shell_event_tx.send(event).await;
            continue;
        }
                   if let InputEvent::RunToolCall(tool_call) = &event {
                       services::update::update(&mut state, InputEvent::ShowConfirmationDialog(tool_call.clone()), 10, 40, &output_tx, cancel_tx.clone(), terminal_size, &shell_event_tx);
                       terminal.draw(|f| view::view(f, &mut state))?;
                       continue;
                   }
                   if let InputEvent::ToolResult(ref tool_call_result) = event {
                       services::update::clear_streaming_tool_results(&mut state);
                       services::bash_block::render_result_block(tool_call_result, &mut state, terminal_size);
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
                       services::update::update(&mut state, event, message_area_height, message_area_width, &output_tx, cancel_tx.clone(), terminal_size, &shell_event_tx);
                   }
               }
               Some(event) = internal_rx.recv() => {
                   if let InputEvent::Quit = event { should_quit = true; }
                   else if state.show_editor {
                       // Handle editor events
                       if let InputEvent::ToggleEditor = event {
                           state.show_editor = false;
                           
                           // Only save editor content back to input if we're not editing a file
                           if state.editor_file_path.is_none() {
                               EDITOR_STATE.with(|editor_state| {
                                   if let Some(state_ref) = editor_state.borrow().as_ref() {
                                       state.input = state_ref.lines.iter_row()
                                           .map(|row| row.iter().collect::<String>())
                                           .collect::<Vec<String>>()
                                           .join("\n");
                                   }
                               });
                           }
                           
                           // Clear file path when exiting editor
                           state.editor_file_path = None;
                           
                           // Clear thread-local storage
                           EDITOR_STATE.with(|editor_state| {
                               *editor_state.borrow_mut() = None;
                           });
                           EDITOR_EVENT_HANDLER.with(|editor_event_handler| {
                               *editor_event_handler.borrow_mut() = None;
                           });
                       } else {
                            // Handle editor events using edtui
                            handle_editor_event(&mut state, event);
                        }
                   } else {
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
                        if (state.show_helper_dropdown && state.autocomplete.is_active()) || (state.show_shell_mode && !state.waiting_for_shell_input) {
                            // Do nothing for these cases
                        } else if !state.show_shell_mode && !state.input.trim().is_empty() && !state.input.trim().starts_with('/') {
                            let _ = output_tx.try_send(OutputEvent::UserMessage(state.input.clone(), state.shell_tool_calls.clone()));
                        }
                       }
                       services::update::update(&mut state, event, message_area_height, message_area_width, &output_tx, cancel_tx.clone(), terminal_size, &shell_event_tx);
                   }
               }
               _ = spinner_interval.tick(), if state.loading => {
                   state.spinner_frame = state.spinner_frame.wrapping_add(1);
                   terminal.draw(|f| view::view(f, &mut state))?;
               }
           }
        if should_quit {
            break;
        }
        terminal.draw(|f| view::view(f, &mut state))?;
    }

    println!("Quitting...");
    let _ = shutdown_tx.send(());
    crossterm::terminal::disable_raw_mode()?;
    execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        DisableBracketedPaste
    )?;
    Ok(())
}

fn handle_editor_event(state: &mut AppState, event: InputEvent) {
    use edtui::events::KeyEvent;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    
    // Initialize editor state if not already done
    EDITOR_STATE.with(|editor_state| {
        if editor_state.borrow().is_none() {
            *editor_state.borrow_mut() = Some(edtui::EditorState::new(edtui::Lines::from(&state.editor_content)));
        }
    });
    
    EDITOR_EVENT_HANDLER.with(|editor_event_handler| {
        if editor_event_handler.borrow().is_none() {
            *editor_event_handler.borrow_mut() = Some(edtui::EditorEventHandler::default());
        }
    });
    
    match event {
        InputEvent::Up => {
            // Create a key event for up arrow
            let key_event = ratatui::crossterm::event::KeyEvent {
                code: KeyCode::Up,
                modifiers: KeyModifiers::empty(),
                kind: ratatui::crossterm::event::KeyEventKind::Press,
                state: ratatui::crossterm::event::KeyEventState::empty(),
            };
            let edtui_key_event = KeyEvent::from(key_event);
            EDITOR_EVENT_HANDLER.with(|handler| {
                EDITOR_STATE.with(|state| {
                    if let (Some(handler), Some(state)) = (handler.borrow_mut().as_mut(), state.borrow_mut().as_mut()) {
                        handler.on_key_event(edtui_key_event, state);
                    }
                });
            });
        }
        InputEvent::Down => {
            let key_event = ratatui::crossterm::event::KeyEvent {
                code: KeyCode::Down,
                modifiers: KeyModifiers::empty(),
                kind: ratatui::crossterm::event::KeyEventKind::Press,
                state: ratatui::crossterm::event::KeyEventState::empty(),
            };
            let edtui_key_event = KeyEvent::from(key_event);
            EDITOR_EVENT_HANDLER.with(|handler| {
                EDITOR_STATE.with(|state| {
                    if let (Some(handler), Some(state)) = (handler.borrow_mut().as_mut(), state.borrow_mut().as_mut()) {
                        handler.on_key_event(edtui_key_event, state);
                    }
                });
            });
        }
        InputEvent::CursorLeft => {
            let key_event = ratatui::crossterm::event::KeyEvent {
                code: KeyCode::Left,
                modifiers: KeyModifiers::empty(),
                kind: ratatui::crossterm::event::KeyEventKind::Press,
                state: ratatui::crossterm::event::KeyEventState::empty(),
            };
            let edtui_key_event = KeyEvent::from(key_event);
            EDITOR_EVENT_HANDLER.with(|handler| {
                EDITOR_STATE.with(|state| {
                    if let (Some(handler), Some(state)) = (handler.borrow_mut().as_mut(), state.borrow_mut().as_mut()) {
                        handler.on_key_event(edtui_key_event, state);
                    }
                });
            });
        }
        InputEvent::CursorRight => {
            let key_event = ratatui::crossterm::event::KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::empty(),
                kind: ratatui::crossterm::event::KeyEventKind::Press,
                state: ratatui::crossterm::event::KeyEventState::empty(),
            };
            let edtui_key_event = KeyEvent::from(key_event);
            EDITOR_EVENT_HANDLER.with(|handler| {
                EDITOR_STATE.with(|state| {
                    if let (Some(handler), Some(state)) = (handler.borrow_mut().as_mut(), state.borrow_mut().as_mut()) {
                        handler.on_key_event(edtui_key_event, state);
                    }
                });
            });
        }
        InputEvent::InputChanged(c) => {
            let key_event = ratatui::crossterm::event::KeyEvent {
                code: KeyCode::Char(c),
                modifiers: KeyModifiers::empty(),
                kind: ratatui::crossterm::event::KeyEventKind::Press,
                state: ratatui::crossterm::event::KeyEventState::empty(),
            };
            let edtui_key_event = KeyEvent::from(key_event);
            EDITOR_EVENT_HANDLER.with(|handler| {
                EDITOR_STATE.with(|state| {
                    if let (Some(handler), Some(state)) = (handler.borrow_mut().as_mut(), state.borrow_mut().as_mut()) {
                        handler.on_key_event(edtui_key_event, state);
                    }
                });
            });
        }
        InputEvent::InputBackspace => {
            let key_event = ratatui::crossterm::event::KeyEvent {
                code: KeyCode::Backspace,
                modifiers: KeyModifiers::empty(),
                kind: ratatui::crossterm::event::KeyEventKind::Press,
                state: ratatui::crossterm::event::KeyEventState::empty(),
            };
            let edtui_key_event = KeyEvent::from(key_event);
            EDITOR_EVENT_HANDLER.with(|handler| {
                EDITOR_STATE.with(|state| {
                    if let (Some(handler), Some(state)) = (handler.borrow_mut().as_mut(), state.borrow_mut().as_mut()) {
                        handler.on_key_event(edtui_key_event, state);
                    }
                });
            });
        }
        InputEvent::InputSubmitted => {
            let key_event = ratatui::crossterm::event::KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::empty(),
                kind: ratatui::crossterm::event::KeyEventKind::Press,
                state: ratatui::crossterm::event::KeyEventState::empty(),
            };
            let edtui_key_event = KeyEvent::from(key_event);
            EDITOR_EVENT_HANDLER.with(|handler| {
                EDITOR_STATE.with(|state| {
                    if let (Some(handler), Some(state)) = (handler.borrow_mut().as_mut(), state.borrow_mut().as_mut()) {
                        handler.on_key_event(edtui_key_event, state);
                    }
                });
            });
        }
        InputEvent::SaveFile => {
            // Handle save file event in editor mode
            if let Some(file_path) = &state.editor_file_path {
                // Get the current content from the editor state
                let content = EDITOR_STATE.with(|editor_state| {
                    if let Some(state_ref) = editor_state.borrow().as_ref() {
                        state_ref.lines.iter_row()
                            .map(|row| row.iter().collect::<String>())
                            .collect::<Vec<String>>()
                            .join("\n")
                    } else {
                        state.editor_content.clone()
                    }
                });
                
                if let Err(e) = std::fs::write(file_path, content) {
                    // Add error message to the UI
                    state.messages.push(crate::services::message::Message::info(
                        format!("Failed to save file: {}", e),
                        Some(ratatui::style::Style::default().fg(ratatui::style::Color::Red))
                    ));
                } else {
                    state.messages.push(crate::services::message::Message::info(
                        format!("File saved: {}", file_path), 
                        None
                    ));
                }
            }
        }
        _ => {
            // Ignore other events in editor mode
        }
    }
}
