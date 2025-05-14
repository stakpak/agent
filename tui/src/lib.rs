use std::io;

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use tokio::sync::mpsc;

// Message struct for user and info messages
#[derive(Clone)]
struct Message {
    text: String,
    style: Style,
}

impl Message {
    fn info(text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            text: text.into(),
            style: style.unwrap_or(Style::default().fg(Color::DarkGray)),
        }
    }
    fn user(text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            text: text.into(),
            style: style.unwrap_or_default(),
        }
    }
    fn _assistant(text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            text: text.into(),
            style: style.unwrap_or(Style::default().fg(Color::White)),
        }
    }
}

// App state
struct AppState {
    input: String,
    messages: Vec<Message>,
    scroll: usize,
    helpers: Vec<&'static str>,
    show_helper_dropdown: bool,
    helper_selected: usize,
    filtered_helpers: Vec<&'static str>,
    show_shortcuts: bool,
}

// Messages/events
enum Msg {
    InputChanged(char),
    InputBackspace,
    InputSubmitted,
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    Quit,
}

fn update(state: &mut AppState, msg: Msg, term_height: usize) {
    let input_height = 3;
    let lines_per_message = 2;
    let max_visible_messages = std::cmp::max(
        1,
        term_height.saturating_sub(input_height) / lines_per_message,
    );
    let total_lines = state.messages.len() * 2;
    let max_scroll = total_lines.saturating_sub(max_visible_messages);
    state.scroll = state.scroll.min(max_scroll);
    match msg {
        Msg::InputChanged(c) => {
            if c == '?' && state.input.is_empty() {
                state.show_shortcuts = !state.show_shortcuts;
            } else {
                state.input.push(c);
                if state.input.starts_with('/') {
                    state.show_helper_dropdown = true;
                    state.filtered_helpers = state
                        .helpers
                        .iter()
                        .filter(|h| h.starts_with(&state.input))
                        .cloned()
                        .collect();
                    if state.filtered_helpers.is_empty()
                        || state.helper_selected >= state.filtered_helpers.len()
                    {
                        state.helper_selected = 0;
                    }
                } else {
                    state.show_helper_dropdown = false;
                    state.filtered_helpers.clear();
                    state.helper_selected = 0;
                }
            }
        }
        Msg::InputBackspace => {
            state.input.pop();
            if state.input.starts_with('/') {
                state.show_helper_dropdown = true;
                state.filtered_helpers = state
                    .helpers
                    .iter()
                    .filter(|h| h.starts_with(&state.input))
                    .cloned()
                    .collect();
                if state.filtered_helpers.is_empty()
                    || state.helper_selected >= state.filtered_helpers.len()
                {
                    state.helper_selected = 0;
                }
            } else {
                state.show_helper_dropdown = false;
                state.filtered_helpers.clear();
                state.helper_selected = 0;
            }
        }
        Msg::InputSubmitted => {
            if state.show_helper_dropdown && !state.filtered_helpers.is_empty() {
                // Send the selected helper as a message
                let total_lines = state.messages.len() * 2;
                let max_visible_lines = std::cmp::max(1, term_height.saturating_sub(input_height));
                let max_scroll = total_lines.saturating_sub(max_visible_lines);
                let was_at_bottom = state.scroll == max_scroll;
                let selected = state.filtered_helpers[state.helper_selected];
                state.messages.push(Message::user(selected, None));
                state.input.clear();
                state.show_helper_dropdown = false;
                state.helper_selected = 0;
                state.filtered_helpers = state.helpers.clone();
                let total_lines = state.messages.len() * 2;
                let max_scroll = total_lines.saturating_sub(max_visible_lines);
                if was_at_bottom {
                    state.scroll = max_scroll;
                }
            } else if !state.input.trim().is_empty() {
                let total_lines = state.messages.len() * 2;
                let max_visible_lines = std::cmp::max(1, term_height.saturating_sub(input_height));
                let max_scroll = total_lines.saturating_sub(max_visible_lines);
                let was_at_bottom = state.scroll == max_scroll;
                state
                    .messages
                    .push(Message::user(state.input.clone(), None));
                state.input.clear();
                let total_lines = state.messages.len() * 2;
                let max_scroll = total_lines.saturating_sub(max_visible_lines);
                if was_at_bottom {
                    state.scroll = max_scroll;
                }
            }
        }
        Msg::ScrollUp => {
            if state.show_helper_dropdown && !state.filtered_helpers.is_empty() {
                if state.helper_selected > 0 {
                    state.helper_selected -= 1;
                }
            } else if state.scroll > 0 {
                state.scroll -= 1;
            }
        }
        Msg::ScrollDown => {
            if state.show_helper_dropdown && !state.filtered_helpers.is_empty() {
                if state.helper_selected + 1 < state.filtered_helpers.len() {
                    state.helper_selected += 1;
                }
            } else {
                if state.scroll < max_scroll {
                    state.scroll += 1;
                }
                state.scroll = state.scroll.min(max_scroll);
            }
        }
        Msg::PageUp => {
            if state.show_helper_dropdown && !state.filtered_helpers.is_empty() {
                state.helper_selected = 0;
            } else {
                let page = std::cmp::max(1, term_height.saturating_sub(input_height));
                if state.scroll >= page {
                    state.scroll -= page;
                } else {
                    state.scroll = 0;
                }
                state.scroll = state.scroll.min(max_scroll);
            }
        }
        Msg::PageDown => {
            if state.show_helper_dropdown && !state.filtered_helpers.is_empty() {
                state.helper_selected = state.filtered_helpers.len().saturating_sub(1);
            } else {
                let page = std::cmp::max(1, term_height.saturating_sub(input_height));
                if state.scroll + page < max_scroll {
                    state.scroll += page;
                } else {
                    state.scroll = max_scroll;
                }
                state.scroll = state.scroll.min(max_scroll);
            }
        }
        Msg::Quit => {}
    }
}

fn view(f: &mut Frame, state: &AppState) {
    let input_height = 3;
    let margin_height = 2;
    // Layout: message area, input area, and bottom margin (no dropdown chunk)
    let outer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(input_height as u16),
            Constraint::Length(margin_height),
        ])
        .split(f.size());
    let message_area_height = outer_chunks[0].height as usize;
    let lines_per_message = 2;
    let max_visible_messages = std::cmp::max(1, message_area_height / lines_per_message);
    let max_scroll = state.messages.len().saturating_sub(max_visible_messages);
    let scroll = state.scroll.min(max_scroll);
    let visible_messages = state
        .messages
        .iter()
        .skip(scroll)
        .take(max_visible_messages);
    let mut message_lines = Vec::new();
    for msg in visible_messages {
        message_lines.push(Line::from(vec![Span::styled(&msg.text, msg.style)]));
        message_lines.push(Line::from("")); // Add empty line for spacing
    }
    let message_widget = Paragraph::new(message_lines);
    f.render_widget(message_widget, outer_chunks[0]);
    let input_widget = Paragraph::new(vec![Line::from(vec![
        Span::raw("> "),
        Span::raw(&state.input),
    ])])
    .style(Style::default())
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(input_widget, outer_chunks[1]);
    // Render helper dropdown if needed (overlay, does not move input)
    if state.show_helper_dropdown && !state.filtered_helpers.is_empty() {
        use ratatui::widgets::{List, ListItem, ListState};
        // If input is exactly '/', show all helpers
        let items: Vec<ListItem> = if state.input == "/" {
            state
                .helpers
                .iter()
                .map(|h| ListItem::new(Line::from(vec![Span::raw(format!("  {}  ", h))])))
                .collect()
        } else {
            state
                .filtered_helpers
                .iter()
                .map(|h| ListItem::new(Line::from(vec![Span::raw(format!("  {}  ", h))])))
                .collect()
        };
        let mut list_state = ListState::default();
        list_state.select(Some(
            state.helper_selected.min(items.len().saturating_sub(1)),
        ));
        let input_area = outer_chunks[1];
        let dropdown_height = items.len() as u16;
        let dropdown_area = Rect {
            x: input_area.x,
            y: input_area.y.saturating_sub(dropdown_height),
            width: input_area.width,
            height: dropdown_height,
        };
        let dropdown_widget = List::new(items)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .block(Block::default()); // No border_style
        f.render_stateful_widget(dropdown_widget, dropdown_area, &mut list_state);
    }
    // Render the hint or shortcuts as a separate line in the margin chunk
    let hint_area = outer_chunks[2];
    if state.show_shortcuts {
        let shortcuts = vec![
            Line::from(
                "! for bash mode    double tap esc to undo    / for commands     # to memorize",
            ),
            Line::from(
                "↵ for newline      @ for file paths          shift + tab to auto-accept edits",
            ),
        ];
        let shortcuts_widget = Paragraph::new(shortcuts).style(Style::default().fg(Color::Cyan));
        f.render_widget(shortcuts_widget, hint_area);
    } else {
        let hint = Paragraph::new(Span::styled(
            "? for shortcuts",
            Style::default().fg(Color::Cyan),
        ));
        f.render_widget(hint, hint_area);
    }
}

pub async fn run_tui(mut external_rx: mpsc::Receiver<Msg>) -> io::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let all_helpers = vec!["/help", "/status", "/clear", "/about", "/quit"];
    let mut state = AppState {
        input: String::new(),
        messages: vec![
            Message::info(
                "* Welcome to Stakpak!",
                Some(Style::default().fg(Color::Cyan)),
            ),
            Message::info("/help for help, /status for your current setup", None),
            Message::info(
                format!("cwd: {}", std::env::current_dir().unwrap().display()),
                None,
            ),
        ],
        scroll: 0,
        helpers: all_helpers.clone(),
        show_helper_dropdown: false,
        helper_selected: 0,
        filtered_helpers: all_helpers.clone(),
        show_shortcuts: false,
    };

    // Internal channel for event handling
    let (internal_tx, mut internal_rx) = mpsc::channel::<Msg>(100);
    std::thread::spawn(move || {
        loop {
            if let Ok(Event::Key(key)) = event::read() {
                let msg = match key.code {
                    KeyCode::Char('c')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        Msg::Quit
                    }
                    KeyCode::Char(c) => Msg::InputChanged(c),
                    KeyCode::Backspace => Msg::InputBackspace,
                    KeyCode::Enter => Msg::InputSubmitted,
                    KeyCode::Esc => Msg::Quit,
                    KeyCode::Up => Msg::ScrollUp,
                    KeyCode::Down => Msg::ScrollDown,
                    KeyCode::PageUp => Msg::PageUp,
                    KeyCode::PageDown => Msg::PageDown,
                    _ => continue,
                };
                if internal_tx.blocking_send(msg).is_err() {
                    break;
                }
            }
        }
    });

    // Main async update/view loop
    let mut should_quit = false;
    while !should_quit {
        tokio::select! {
            Some(msg) = external_rx.recv() => {
                if let Msg::Quit = msg { should_quit = true; }
                else {
                    let term_height = terminal.size()?.height as usize;
                    update(&mut state, msg, term_height);
                }
            }
            Some(msg) = internal_rx.recv() => {
                if let Msg::Quit = msg { should_quit = true; }
                else {
                    let term_height = terminal.size()?.height as usize;
                    update(&mut state, msg, term_height);
                }
            }
        }
        terminal.draw(|f| view(f, &state))?;
    }

    // Tear down raw mode + alt screen
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}
