use crate::view::render_system_message;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;
use stakpak_shared::models::integrations::openai::ToolCall;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;
pub enum MessageContent {
    Plain(String, Style),
    Styled(Line<'static>),
    StyledBlock(Vec<Line<'static>>),
}

pub struct SessionInfo {
    pub title: String,
    pub id: String,
    pub updated_at: String,
}

// TODO: add user list sessions
pub fn list_sessions() -> Vec<SessionInfo> {
    vec![]
}

pub struct Message {
    pub id: Uuid,
    pub content: MessageContent,
}

impl Message {
    pub fn info(text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::Plain(
                text.into(),
                style.unwrap_or(Style::default().fg(ratatui::style::Color::DarkGray)),
            ),
        }
    }
    pub fn user(text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::Plain(
                text.into(),
                style.unwrap_or(Style::default().fg(ratatui::style::Color::Rgb(180, 180, 180))),
            ),
        }
    }
    pub fn assistant(id: Option<Uuid>, text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            id: id.unwrap_or(Uuid::new_v4()),
            content: MessageContent::Plain(text.into(), style.unwrap_or_default()),
        }
    }
    pub fn styled(line: Line<'static>) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::Styled(line),
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
    pub sessions: Vec<SessionInfo>,
    pub show_sessions_dialog: bool,
    pub session_selected: usize,
    pub account_info: String,
}

#[derive(Debug)]
pub enum InputEvent {
    AssistantMessage(String),
    StreamAssistantMessage(Uuid, String),
    RunCommand(ToolCall),
    ToolResult(String),
    Loading(bool),
    InputChanged(char),
    GetStatus(String),
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
    HandleEsc,
    CursorLeft,
    CursorRight,
    ToggleCursorVisible,
    Resized(u16, u16),
    ShowConfirmationDialog(ToolCall),
    DialogConfirm,
    DialogCancel,
    Tab,
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
            sessions: list_sessions(),
            show_sessions_dialog: false,
            session_selected: 0,
            account_info: String::new(),
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
            if state.show_sessions_dialog {
                if state.session_selected > 0 {
                    state.session_selected -= 1;
                }
            } else if state.show_helper_dropdown
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
            if state.show_sessions_dialog {
                if state.session_selected + 1 < state.sessions.len() {
                    state.session_selected += 1;
                }
            } else if state.show_helper_dropdown
                && !state.filtered_helpers.is_empty()
                && state.input.starts_with('/')
            {
                handle_dropdown_down(state);
            } else if state.is_dialog_open {
                handle_dialog_down(state, message_area_height, message_area_width);
            } else {
                handle_scroll_down(state, message_area_height, message_area_width);
            }
        }
        InputEvent::DropdownUp => handle_dropdown_up(state),
        InputEvent::DropdownDown => handle_dropdown_down(state),
        InputEvent::InputChanged(c) => handle_input_changed(state, c),
        InputEvent::InputBackspace => handle_input_backspace(state),
        InputEvent::InputSubmitted => {
            if state.show_sessions_dialog {
                let selected = &state.sessions[state.session_selected];
                render_system_message(state, &format!("Switching to session . {}", selected.title));
                state.show_sessions_dialog = false;
                // input box and helper will show again automatically
            } else {
                handle_input_submitted(state, message_area_height, output_tx);
            }
        }
        InputEvent::InputChangedNewline => handle_input_changed(state, '\n'),
        InputEvent::InputSubmittedWith(s) => {
            handle_input_submitted_with(state, s, message_area_height)
        }
        InputEvent::StreamAssistantMessage(id, s) => {
            handle_stream_message(state, id, s, message_area_height)
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

        InputEvent::Loading(is_loading) => {
            state.loading = is_loading;
        }
        InputEvent::HandleEsc => handle_esc(state),

        InputEvent::GetStatus(account_info) => {
            state.account_info = account_info;
        }
        InputEvent::Tab => handle_tab(state),
        _ => {}
    }
    adjust_scroll(state, message_area_height, message_area_width);
}


fn handle_tab(state: &mut AppState) {
    // state.show_helper_dropdown = true;
    // state.filtered_helpers = state
    //     .helpers
    //     .iter()
    //     .filter(|h| h.starts_with(&state.input))
    //     .cloned()
    //     .collect();
    // if state.filtered_helpers.is_empty()
    //     || state.helper_selected >= state.filtered_helpers.len()
    // {
    //     state.helper_selected = 0;
    // }


    if state.is_dialog_open {
        state.dialog_selected = (state.dialog_selected + 1) % 2;
    }
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

fn can_scroll_up(state: &AppState) -> bool {
    state.scroll > 0
}

fn can_scroll_down(state: &AppState, message_area_height: usize, message_area_width: usize) -> bool {
    let all_lines = get_wrapped_message_lines(&state.messages, message_area_width);
    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(message_area_height);
    state.scroll < max_scroll
}

fn handle_dialog_up(state: &mut AppState) {
    if state.is_dialog_open {
        if state.dialog_selected == 0 {
            if can_scroll_up(state) {
                handle_scroll_up(state);
            }
        } else if state.dialog_selected > 0 {
            state.dialog_selected -= 1;
        }
    }
}

fn handle_dialog_down(state: &mut AppState, message_area_height: usize, message_area_width: usize) {
    if state.is_dialog_open {
        if can_scroll_down(state, message_area_height, message_area_width) {
            handle_scroll_down(state, message_area_height, message_area_width);
        } else if state.dialog_selected < 1 {
            state.dialog_selected += 1;
        }
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

fn handle_esc(state: &mut AppState) {
    state.input.clear();
    state.cursor_position = 0;
    if state.show_sessions_dialog {
        state.show_sessions_dialog = false;
    } else if state.show_helper_dropdown {
        state.show_helper_dropdown = false;
    } else if state.is_dialog_open {
        state.is_dialog_open = false;
        if let Some(tool_call) = state.dialog_command.take() {
            let input = state.input.clone();
            render_bash_block(&tool_call, &input, false, state);
        }
    } else {
        return;
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
        } else {
            let tool_call = state.dialog_command.clone();
            let input = state.input.clone();
            if let Some(tool_call) = tool_call {
                render_bash_block(&tool_call, &input, false, state);
            }
        }
    } else if state.show_helper_dropdown && !state.filtered_helpers.is_empty() {
        let selected = state.filtered_helpers[state.helper_selected];

        match selected {
            "/sessions" => {
                // state.show_sessions_dialog = true;
                // state.session_selected = 0;
                state.input.clear();
                state.cursor_position = 0;
                state.show_helper_dropdown = false;
                return;
            }
            "/help" => {
                push_help_message(state);
                state.input.clear();
                state.cursor_position = 0;
                state.show_helper_dropdown = false;
                return;
            }
            "/status" => {
                push_status_message(state);
                state.input.clear();
                state.cursor_position = 0;
                state.show_helper_dropdown = false;
                return;
            }
            "/quit" => {
                state.show_helper_dropdown = false;
                state.input.clear();
                state.cursor_position = 0;
                std::process::exit(0);
            }
            _ => {}
        }

        let total_lines = state.messages.len() * 2;
        let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
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
    } else if !state.input.trim().is_empty() && !state.input.trim().starts_with('/') {
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
    state
        .messages
        .push(Message::assistant(None, s.clone(), None));
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

fn handle_stream_message(state: &mut AppState, id: Uuid, s: String, message_area_height: usize) {
    if let Some(message) = state.messages.iter_mut().find(|m| m.id == id) {
        if let MessageContent::Plain(text, _) = &mut message.content {
            text.push_str(&s);
        }
    } else {
        let input_height = 3;
        let total_lines = state.messages.len() * 2;
        let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        let was_at_bottom = state.scroll == max_scroll;
        state
            .messages
            .push(Message::assistant(Some(id), s.clone(), None));
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
        match &msg.content {
            MessageContent::Plain(text, style) => {
                for line in text.lines() {
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
                            break;
                        }
                        let mut safe_take = take;
                        while safe_take > 0 && !current.is_char_boundary(safe_take) {
                            safe_take -= 1;
                        }
                        if safe_take == 0 {
                            break;
                        }
                        let (part, rest) = current.split_at(safe_take);
                        all_lines.push((Line::from(vec![Span::styled(part, *style)]), *style));
                        current = rest;
                    }
                }
                all_lines.push((Line::from(""), *style));
            }
            MessageContent::Styled(line) => {
                all_lines.push((line.clone(), Style::default()));
                all_lines.push((Line::from(""), Style::default()));
            }
            MessageContent::StyledBlock(lines) => {
                for line in lines {
                    all_lines.push((line.clone(), Style::default()));
                }
            }
        }
    }
    all_lines
}

pub fn render_bash_block<'a>(
    tool_call: &'a ToolCall,
    output: &'a str,
    accepted: bool,
    state: &mut AppState,
) {
    // Extract command name from arguments JSON
    let command_name = serde_json::from_str::<Value>(&tool_call.function.arguments)
        .ok()
        .and_then(|v| {
            v.get("command")
                .and_then(|c| c.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "?".to_string());

    let mut lines = Vec::new();
    // Header
    lines.push(Line::from(vec![
        Span::styled(
            "● ",
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Bash",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({})", command_name),
            Style::default().fg(Color::Gray),
        ),
        Span::styled("...", Style::default().fg(Color::Gray)),
    ]));
    if !accepted {
        lines.push(Line::from(vec![Span::styled(
            "  L No (tell Stakpak what to do differently)",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )]));
    }
    // Output lines
    let output_pad = "    "; // 4 spaces, adjust as needed
    for (i, line) in output.lines().enumerate() {
        let prefix = if i == 0 { "└ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{output_pad}{prefix}"),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(line, Style::default().fg(Color::Gray)),
        ]));
    }
    let mut owned_lines: Vec<Line<'static>> = lines
        .into_iter()
        .map(|line| {
            let owned_spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|span| Span::styled(span.content.into_owned(), span.style))
                .collect();
            Line::from(owned_spans)
        })
        .collect();
    owned_lines.push(Line::from(vec![Span::styled(
        "  ",
        Style::default().fg(Color::Gray),
    )]));
    state.messages.push(Message {
        id: Uuid::new_v4(),
        content: MessageContent::StyledBlock(owned_lines),
    });
}

pub fn get_stakpak_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

pub fn push_status_message(state: &mut AppState) {
    let status_text = state.account_info.clone();
    let version = get_stakpak_version();
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "?".to_string());

    // Default values
    let mut id = "unknown".to_string();
    let mut username = "unknown".to_string();
    let mut name = "unknown".to_string();

    for line in status_text.lines() {
        if let Some(rest) = line.strip_prefix("ID: ") {
            id = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Username: ") {
            username = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Name: ") {
            name = rest.trim().to_string();
        }
    }

    let lines = vec![
        Line::from(vec![Span::styled(
            format!("Stakpak Code Status v{}", version),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Working Directory",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(format!("  L {}", cwd)),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Account",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(format!("  L Username: {}", username)),
        Line::from(format!("  L ID: {}", id)),
        Line::from(format!("  L Name: {}", name)),
        Line::from(""),
    ];
    state.messages.push(Message {
        id: uuid::Uuid::new_v4(),
        content: MessageContent::StyledBlock(lines),
    });
}

pub fn push_help_message(state: &mut AppState) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    let mut lines = Vec::new();
    // usage mode
    lines.push(Line::from(vec![Span::styled(
        "Usage Mode",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )]));

    let usage_modes = vec![
        ("REPL", "stakpak (interactive session)", Color::White),
        (
            "Non-interactive",
            "stakpak -p  \"prompt\" -c <checkpoint_id>",
            Color::White,
        ),
    ];
    for (mode, desc, color) in usage_modes {
        lines.push(Line::from(vec![
            Span::styled(
                "● ",
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(mode),
            Span::raw(" – "),
            Span::styled(
                desc,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("Run"),
        Span::styled(
            " stakpak --help ",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("to see all commands", Style::default().fg(Color::Gray)),
    ]));
    lines.push(Line::from(""));
    // Section header
    lines.push(Line::from(vec![Span::styled(
        "Available commands",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(""));
    // Slash-commands header
    lines.push(Line::from(vec![Span::styled(
        "Slash-commands",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )]));

    // Slash-commands list
    let commands = vec![
        ("/help", "show this help overlay"),
        ("/status", "show account status"),
        ("/sessions", "show list of sessions"),
        ("/quit", "quit the app"),
    ];
    for (cmd, desc) in commands {
        lines.push(Line::from(vec![
            Span::styled(cmd, Style::default().fg(Color::Cyan)),
            Span::raw(" – "),
            Span::raw(desc),
        ]));
    }
    lines.push(Line::from(""));

    // Keyboard shortcuts header
    lines.push(Line::from(vec![Span::styled(
        "Keyboard shortcuts",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )]));
    // Shortcuts list
    let shortcuts = vec![
        ("Enter", "send message", Color::Yellow),
        ("Ctrl+J or Shift+Enter", "insert newline", Color::Yellow),
        ("Up/Down", "scroll prompt history", Color::Yellow),
        ("Esc", "Closes any open dialog", Color::Yellow),
        ("Ctrl+C", "quit Codex", Color::Yellow),
    ];
    for (key, desc, color) in shortcuts {
        lines.push(Line::from(vec![
            Span::styled(key, Style::default().fg(color)),
            Span::raw(" – "),
            Span::raw(desc),
        ]));
    }
    lines.push(Line::from(""));
    state.messages.push(Message {
        id: uuid::Uuid::new_v4(),
        content: MessageContent::StyledBlock(lines),
    });
}
