use ratatui::style::Style;

#[derive(Clone)]
pub struct Message {
    pub text: String,
    pub style: Style,
}

impl Message {
    pub fn info(text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            text: text.into(),
            style: style.unwrap_or(Style::default().fg(ratatui::style::Color::DarkGray)),
        }
    }
    pub fn user(text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            text: text.into(),
            style: style.unwrap_or(Style::default().fg(ratatui::style::Color::Rgb(180, 180, 180))),
        }
    }
    pub fn assistant(text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            text: text.into(),
            style: style.unwrap_or_default(),
        }
    }
}

pub struct AppState {
    pub input: String,
    pub messages: Vec<Message>,
    pub scroll: usize,
    pub helpers: Vec<&'static str>,
    pub show_helper_dropdown: bool,
    pub helper_selected: usize,
    pub filtered_helpers: Vec<&'static str>,
    pub show_shortcuts: bool,
}

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
    DropdownUp,
    DropdownDown,
    Up,
    Down,
    Quit,
}

impl AppState {
    pub fn new(helpers: Vec<&'static str>) -> Self {
        AppState {
            input: String::new(),
            messages: vec![
                Message::info(
                    "* Welcome to Stakpak!",
                    Some(Style::default().fg(ratatui::style::Color::Cyan)),
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
            helpers: helpers.clone(),
            show_helper_dropdown: false,
            helper_selected: 0,
            filtered_helpers: helpers,
            show_shortcuts: false,
        }
    }
}

pub fn update(state: &mut AppState, msg: Msg, term_height: usize) {
    state.scroll = state.scroll.max(0);
    match msg {
        Msg::Up => {
            if state.show_helper_dropdown
                && !state.filtered_helpers.is_empty()
                && state.input.starts_with('/')
            {
                handle_dropdown_up(state);
            } else {
                handle_scroll_up(state);
            }
        }
        Msg::Down => {
            if state.show_helper_dropdown
                && !state.filtered_helpers.is_empty()
                && state.input.starts_with('/')
            {
                handle_dropdown_down(state);
            } else {
                handle_scroll_down(state);
            }
        }
        Msg::DropdownUp => handle_dropdown_up(state),
        Msg::DropdownDown => handle_dropdown_down(state),
        Msg::InputChanged(c) => handle_input_changed(state, c),
        Msg::InputBackspace => handle_input_backspace(state),
        Msg::InputSubmitted => handle_input_submitted(state, term_height),
        Msg::InputSubmittedWith(s) => handle_input_submitted_with(state, s, term_height),
        Msg::ScrollUp => handle_scroll_up(state),
        Msg::ScrollDown => handle_scroll_down(state),
        Msg::PageUp => handle_page_up(state, term_height),
        Msg::PageDown => handle_page_down(state, term_height),
        Msg::Quit => {}
    }
    adjust_scroll(state, term_height);
}

fn handle_dropdown_up(state: &mut AppState) {
    if state.show_helper_dropdown
        && !state.filtered_helpers.is_empty()
        && state.input.starts_with('/')
        && state.helper_selected > 0
    {
        state.helper_selected -= 1;
    }
}

fn handle_dropdown_down(state: &mut AppState) {
    if state.show_helper_dropdown
        && !state.filtered_helpers.is_empty()
        && state.input.starts_with('/')
        && state.helper_selected + 1 < state.filtered_helpers.len()
    {
        state.helper_selected += 1;
    }
}

fn handle_input_changed(state: &mut AppState, c: char) {
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

fn handle_input_backspace(state: &mut AppState) {
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

fn handle_input_submitted(state: &mut AppState, term_height: usize) {
    let input_height = 3;
    if state.show_helper_dropdown && !state.filtered_helpers.is_empty() {
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

fn handle_input_submitted_with(state: &mut AppState, s: String, term_height: usize) {
    let input_height = 3;
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

fn handle_scroll_up(state: &mut AppState) {
    if state.scroll > 0 {
        state.scroll -= 1;
    }
}

fn handle_scroll_down(state: &mut AppState) {
    state.scroll += 1;
}

fn handle_page_up(state: &mut AppState, term_height: usize) {
    let input_height = 3;
    let page = std::cmp::max(1, term_height.saturating_sub(input_height));
    if state.scroll >= page {
        state.scroll -= page;
    } else {
        state.scroll = 0;
    }
}

fn handle_page_down(state: &mut AppState, term_height: usize) {
    let input_height = 3;
    let page = std::cmp::max(1, term_height.saturating_sub(input_height));
    state.scroll += page;
}

fn adjust_scroll(state: &mut AppState, term_height: usize) {
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
                if take == 0 {
                    // fallback: push the first char and advance
                    let ch_len = current.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                    let (part, rest) = current.split_at(ch_len);
                    all_lines.push(part.to_string());
                    current = rest;
                } else {
                    let (part, rest) = current.split_at(take);
                    all_lines.push(part.to_string());
                    current = rest;
                }
            }
        }
        all_lines.push(String::new());
    }
    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(message_area_height);
    state.scroll = state.scroll.min(max_scroll);
}
