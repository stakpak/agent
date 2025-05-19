use ratatui::style::Style;
use ratatui::text::{Line, Span};
use stakpak_shared::models::integrations::openai::ToolCall;
use tokio::sync::mpsc::Sender;

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
    pub cursor_position: usize,
    pub cursor_visible: bool,
    pub messages: Vec<Message>,
    pub scroll: usize,
    pub scroll_to_bottom: bool,
    pub stay_at_bottom: bool,
    pub helpers: Vec<&'static str>,
    pub show_helper_dropdown: bool,
    pub helper_selected: usize,
    pub filtered_helpers: Vec<&'static str>,
    pub show_shortcuts: bool,
    pub is_dialog_open: bool,
    pub dialog_command: Option<ToolCall>,
    pub dialog_selected: usize,
    pub loading: bool,
    pub spinner_frame: usize,
}

#[derive(Debug)]
pub enum InputEvent {
    AssistantMessage(String),
    RunCommand(ToolCall),
    ToolResult(String),
    InputChanged(char),
    InputBackspace,
    InputChangedNewline,
    InputSubmitted,
    InputSubmittedWith(String),
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    DropdownUp,
    DropdownDown,
    DialogUp,
    DialogDown,
    Up,
    Down,
    Quit,
    CursorLeft,
    CursorRight,
    ToggleCursorVisible,
    Resized(u16, u16),
    ShowConfirmationDialog(ToolCall),
    DialogConfirm,
    DialogCancel,
    Tick,
}

#[derive(Debug)]
pub enum OutputEvent {
    UserMessage(String),
    AcceptTool(ToolCall),
}

impl AppState {
    pub fn new(helpers: Vec<&'static str>) -> Self {
        AppState {
            input: String::new(),
            cursor_position: 0,
            cursor_visible: true,
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
            scroll_to_bottom: false,
            stay_at_bottom: true,
            helpers: helpers.clone(),
            show_helper_dropdown: false,
            helper_selected: 0,
            filtered_helpers: helpers,
            show_shortcuts: false,
            is_dialog_open: false,
            dialog_command: None,
            dialog_selected: 0,
            loading: false,
            spinner_frame: 0,
        }
    }
}

pub fn update(
    state: &mut AppState,
    event: InputEvent,
    message_area_height: usize,
    message_area_width: usize,
    output_tx: &Sender<OutputEvent>,
) {
    state.scroll = state.scroll.max(0);
    match event {
        InputEvent::Up => {
            if state.show_helper_dropdown
                && !state.filtered_helpers.is_empty()
                && state.input.starts_with('/')
            {
                handle_dropdown_up(state);
            } else if state.is_dialog_open {
                handle_dialog_up(state);
            } else {
                handle_scroll_up(state);
            }
        }
        InputEvent::Down => {
            if state.show_helper_dropdown
                && !state.filtered_helpers.is_empty()
                && state.input.starts_with('/')
            {
                handle_dropdown_down(state);
            } else if state.is_dialog_open {
                handle_dialog_down(state);
            } else {
                handle_scroll_down(state, message_area_height, message_area_width);
            }
        }
        InputEvent::DropdownUp => handle_dropdown_up(state),
        InputEvent::DropdownDown => handle_dropdown_down(state),
        InputEvent::InputChanged(c) => handle_input_changed(state, c),
        InputEvent::InputBackspace => handle_input_backspace(state),
        InputEvent::InputSubmitted => handle_input_submitted(state, message_area_height, output_tx),
        InputEvent::InputChangedNewline => handle_input_changed(state, '\n'),
        InputEvent::InputSubmittedWith(s) => {
            handle_input_submitted_with(state, s, message_area_height)
        }
        InputEvent::ScrollUp => handle_scroll_up(state),
        InputEvent::ScrollDown => {
            handle_scroll_down(state, message_area_height, message_area_width)
        }
        InputEvent::PageUp => handle_page_up(state, message_area_height),
        InputEvent::PageDown => handle_page_down(state, message_area_height, message_area_width),
        InputEvent::Quit => {}
        InputEvent::CursorLeft => {
            if state.cursor_position > 0 {
                let prev = state.input[..state.cursor_position]
                    .chars()
                    .next_back()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
                state.cursor_position -= prev;
            }
        }
        InputEvent::CursorRight => {
            if state.cursor_position < state.input.len() {
                let next = state.input[state.cursor_position..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
                state.cursor_position += next;
            }
        }
        InputEvent::ToggleCursorVisible => state.cursor_visible = !state.cursor_visible,
        InputEvent::ShowConfirmationDialog(tool_call) => {
            state.is_dialog_open = true;
            state.dialog_command = Some(tool_call);
            state.dialog_selected = 0;
        }
        InputEvent::Tick => {
            if state.loading {
                state.spinner_frame = state.spinner_frame.wrapping_add(1);
            }
        }
        _ => {}
    }
    adjust_scroll(state, message_area_height, message_area_width);
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

fn handle_dialog_up(state: &mut AppState) {
    if state.is_dialog_open && state.dialog_selected > 0 {
        state.dialog_selected -= 1;
    }
}

fn handle_dialog_down(state: &mut AppState) {
    if state.is_dialog_open && state.dialog_selected < 1 {
        state.dialog_selected += 1;
    }
}

fn handle_input_changed(state: &mut AppState, c: char) {
    if c == '?' && state.input.is_empty() {
        state.show_shortcuts = !state.show_shortcuts;
        return;
    }

    let pos = state.cursor_position.min(state.input.len());
    state.input.insert(pos, c);
    state.cursor_position = pos + c.len_utf8();

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

fn handle_input_backspace(state: &mut AppState) {
    if state.cursor_position > 0 && !state.input.is_empty() {
        let pos = state.cursor_position;
        let prev = state.input[..pos]
            .chars()
            .next_back()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        let remove_at = pos - prev;
        state.input.drain(remove_at..pos);
        state.cursor_position = remove_at;
    }
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

fn handle_input_submitted(
    state: &mut AppState,
    message_area_height: usize,
    output_tx: &Sender<OutputEvent>,
) {
    let input_height = 3;
    if state.is_dialog_open {
        state.is_dialog_open = false;
        state.input.clear();
        state.cursor_position = 0;

        if state.dialog_selected == 0 {
            if let Some(tool_call) = &state.dialog_command {
                let _ = output_tx.try_send(OutputEvent::AcceptTool(tool_call.clone()));
            }
        }
    } else if state.show_helper_dropdown && !state.filtered_helpers.is_empty() {
        let total_lines = state.messages.len() * 2;
        let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        let selected = state.filtered_helpers[state.helper_selected];
        let was_at_bottom = state.scroll == max_scroll;
        state
            .messages
            .push(Message::user(format!("> {}", selected), None));
        state.input.clear();
        state.cursor_position = 0;
        state.show_helper_dropdown = false;
        state.helper_selected = 0;
        state.filtered_helpers = state.helpers.clone();
        let total_lines = state.messages.len() * 2;
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        if was_at_bottom {
            state.scroll = max_scroll;
            state.scroll_to_bottom = true;
            state.stay_at_bottom = true;
        }
        state.loading = true;
        state.spinner_frame = 0;
    } else if !state.input.trim().is_empty() {
        let total_lines = state.messages.len() * 2;
        let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        let was_at_bottom = state.scroll == max_scroll;
        state
            .messages
            .push(Message::user(format!("> {}", state.input), None));
        state.input.clear();
        state.cursor_position = 0;
        let total_lines = state.messages.len() * 2;
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        if was_at_bottom {
            state.scroll = max_scroll;
            state.scroll_to_bottom = true;
            state.stay_at_bottom = true;
        }
        state.loading = true;
        state.spinner_frame = 0;
    }
}

fn handle_input_submitted_with(state: &mut AppState, s: String, message_area_height: usize) {
    let input_height = 3;
    let total_lines = state.messages.len() * 2;
    let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
    let max_scroll = total_lines.saturating_sub(max_visible_lines);
    let was_at_bottom = state.scroll == max_scroll;
    state.messages.push(Message::assistant(s.clone(), None));
    state.input.clear();
    state.cursor_position = 0;
    let total_lines = state.messages.len() * 2;
    let max_scroll = total_lines.saturating_sub(max_visible_lines);
    if was_at_bottom {
        state.scroll = max_scroll;
        state.scroll_to_bottom = true;
        state.stay_at_bottom = true;
    }
    state.loading = false;
}

fn handle_scroll_up(state: &mut AppState) {
    if state.scroll > 0 {
        state.scroll -= 1;
        state.stay_at_bottom = false;
    }
}

fn handle_scroll_down(state: &mut AppState, message_area_height: usize, message_area_width: usize) {
    let all_lines = get_wrapped_message_lines(&state.messages, message_area_width);
    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(message_area_height);
    if state.scroll < max_scroll {
        state.scroll += 1;
        if state.scroll == max_scroll {
            state.stay_at_bottom = true;
        }
    } else {
        state.stay_at_bottom = true;
    }
}

fn handle_page_up(state: &mut AppState, message_area_height: usize) {
    let input_height = 3;
    let page = std::cmp::max(1, message_area_height.saturating_sub(input_height));
    if state.scroll >= page {
        state.scroll -= page;
    } else {
        state.scroll = 0;
    }
}

fn handle_page_down(state: &mut AppState, message_area_height: usize, message_area_width: usize) {
    let all_lines = get_wrapped_message_lines(&state.messages, message_area_width);
    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(message_area_height);
    let page = std::cmp::max(1, message_area_height);
    if state.scroll < max_scroll {
        state.scroll = (state.scroll + page).min(max_scroll);
        if state.scroll == max_scroll {
            state.stay_at_bottom = true;
        }
    } else {
        state.stay_at_bottom = true;
    }
}

fn adjust_scroll(state: &mut AppState, message_area_height: usize, message_area_width: usize) {
    let all_lines = get_wrapped_message_lines(&state.messages, message_area_width);
    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(message_area_height);
    if state.stay_at_bottom {
        state.scroll = max_scroll;
    } else if state.scroll_to_bottom {
        state.scroll = max_scroll;
        state.scroll_to_bottom = false;
    } else if state.scroll > max_scroll {
        state.scroll = max_scroll;
    }
}

/// Returns the wrapped lines for all messages, matching the logic in render_messages
pub fn get_wrapped_message_lines(messages: &[Message], width: usize) -> Vec<(Line, Style)> {
    let mut all_lines: Vec<(Line, Style)> = Vec::new();
    for msg in messages {
        for line in msg.text.lines() {
            let mut current = line;
            while !current.is_empty() {
                let take = current
                    .char_indices()
                    .scan(0, |acc, (i, c)| {
                        *acc += unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                        Some((i, *acc))
                    })
                    .take_while(|&(_i, w)| w <= width)
                    .last()
                    .map(|(i, _w)| i + 1)
                    .unwrap_or(current.len());
                if take == 0 {
                    let ch_len = current.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                    let (part, rest) = current.split_at(ch_len);
                    all_lines.push((Line::from(vec![Span::styled(part, msg.style)]), msg.style));
                    current = rest;
                } else {
                    let (part, rest) = current.split_at(take);
                    all_lines.push((Line::from(vec![Span::styled(part, msg.style)]), msg.style));
                    current = rest;
                }
            }
        }
        all_lines.push((Line::from(""), msg.style));
    }
    all_lines
}
