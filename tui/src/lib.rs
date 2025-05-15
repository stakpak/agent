use std::io;

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use tokio::sync::mpsc::{self, Receiver, Sender};

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
            style: style.unwrap_or(Style::default().fg(Color::Rgb(180, 180, 180))),
        }
    }
    fn assistant(text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            text: text.into(),
            style: style.unwrap_or_default(),
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
#[derive(Debug)]
pub enum Msg {
    InputChanged(char),
    InputBackspace,
    InputSubmitted,
    InputSubmittedWith(String),
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    Quit,
}

fn update(state: &mut AppState, msg: Msg, term_height: usize) {
    let input_height = 3;
    state.scroll = state.scroll.max(0);
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
                state
                    .messages
                    .push(Message::user(format!("> {}", selected), None));
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
                    .push(Message::user(format!("> {}", state.input), None));
                state.input.clear();
                let total_lines = state.messages.len() * 2;
                let max_scroll = total_lines.saturating_sub(max_visible_lines);
                if was_at_bottom {
                    state.scroll = max_scroll;
                }
            }
        }
        Msg::InputSubmittedWith(s) => {
            let total_lines = state.messages.len() * 2;
            let max_visible_lines = std::cmp::max(1, term_height.saturating_sub(input_height));
            let max_scroll = total_lines.saturating_sub(max_visible_lines);
            let was_at_bottom = state.scroll == max_scroll;
            state.messages.push(Message::assistant(s.clone(), None));
            state.input.clear();
            let total_lines = state.messages.len() * 2;
            let max_scroll = total_lines.saturating_sub(max_visible_lines);
            if was_at_bottom {
                state.scroll = max_scroll;
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
                state.scroll += 1;
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
            }
        }
        Msg::PageDown => {
            if state.show_helper_dropdown && !state.filtered_helpers.is_empty() {
                state.helper_selected = state.filtered_helpers.len().saturating_sub(1);
            } else {
                let page = std::cmp::max(1, term_height.saturating_sub(input_height));
                state.scroll += page;
            }
        }
        Msg::Quit => {}
    }
    let input_height = 3;
    let message_area_height = term_height.saturating_sub(input_height);
    let mut all_lines: Vec<String> = Vec::new();
    for msg in &state.messages {
        for line in msg.text.lines() {
            let mut current = line;
            while !current.is_empty() {
                let take = current
                    .char_indices()
                    .scan(0, |acc, (i, c)| {
                        *acc += unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                        Some((i, *acc))
                    })
                    .take_while(|&(_i, w)| w <= message_area_height)
                    .last()
                    .map(|(i, _w)| i + 1)
                    .unwrap_or(current.len());
                let (part, rest) = current.split_at(take);
                all_lines.push(part.to_string());
                current = rest;
            }
        }
        all_lines.push(String::new());
    }
    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(message_area_height);
    state.scroll = state.scroll.min(max_scroll);
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
    let message_area = outer_chunks[0];
    let message_area_height = message_area.height as usize;
    let message_area_width = message_area.width as usize;

    // --- NEW: Calculate wrapped lines for each message ---
    let mut all_lines: Vec<(Line, Style)> = Vec::new();
    for msg in &state.messages {
        for line in msg.text.lines() {
            let mut current = line;
            while !current.is_empty() {
                let take = current
                    .char_indices()
                    .scan(0, |acc, (i, c)| {
                        *acc += unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                        Some((i, *acc))
                    })
                    .take_while(|&(_i, w)| w <= message_area_width)
                    .last()
                    .map(|(i, _w)| i + 1)
                    .unwrap_or(current.len());
                let (part, rest) = current.split_at(take);
                all_lines.push((Line::from(vec![Span::styled(part, msg.style)]), msg.style));
                current = rest;
            }
        }
        // Add an empty line for spacing
        all_lines.push((Line::from(""), msg.style));
    }
    // --- END NEW ---

    // --- NEW: Scrolling logic based on wrapped lines ---
    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(message_area_height);
    let scroll = state.scroll.min(max_scroll);
    let mut visible_lines = Vec::new();
    for i in 0..message_area_height {
        if let Some((line, _)) = all_lines.get(scroll + i) {
            visible_lines.push(line.clone());
        } else {
            visible_lines.push(Line::from(""));
        }
    }
    // --- END NEW ---

    let message_widget = Paragraph::new(visible_lines).wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(message_widget, message_area);
    let input_widget = Paragraph::new(vec![Line::from(vec![
        Span::raw("> "),
        Span::raw(&state.input),
    ])])
    .style(Style::default())
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    )
    .wrap(ratatui::widgets::Wrap { trim: false });
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

// Add a guard to always restore terminal state
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
    }
}

pub async fn run_tui(mut input_rx: Receiver<Msg>, output_tx: Sender<String>) -> io::Result<()> {
    let _guard = TerminalGuard;
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
                format!(
                    "cwd: {}",
                    std::env::current_dir().unwrap_or_default().display()
                ),
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
    // Draw the UI once before entering the loop
    terminal.draw(|f| view(f, &state))?;
    let mut should_quit = false;
    while !should_quit {
        tokio::select! {
            Some(msg) = input_rx.recv() => {
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
                    // On InputSubmitted, send input to output_tx
                    if let Msg::InputSubmitted = msg {
                        if !state.input.trim().is_empty() {
                            let _ = output_tx.try_send(state.input.clone());
                        }
                    }
                    update(&mut state, msg, term_height);
                }
            }
        }
        terminal.draw(|f| view(f, &state))?;
    }

    println!("Quitting...");

    Ok(())
}
