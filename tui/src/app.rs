use crate::services::message::Message;
use ratatui::style::Style;
use stakpak_shared::models::integrations::openai::{
    ToolCall, ToolCallResult, ToolCallResultProgress,
};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug)]
pub struct SessionInfo {
    pub title: String,
    pub id: String,
    pub updated_at: String,
    pub checkpoints: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub enum LoadingType {
    LLM,
    Sessions,
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
    pub loading_type: LoadingType,
    pub spinner_frame: usize,
    pub sessions: Vec<SessionInfo>,
    pub show_sessions_dialog: bool,
    pub session_selected: usize,
    pub account_info: String,
    pub pending_bash_message_id: Option<Uuid>, // New field to track pending bash message
    pub streaming_tool_results: HashMap<Uuid, String>,
    pub streaming_tool_result_id: Option<Uuid>,
}

#[derive(Debug)]
pub enum InputEvent {
    AssistantMessage(String),
    StreamAssistantMessage(Uuid, String),
    RunToolCall(ToolCall),
    ToolResult(ToolCallResult),
    StreamToolResult(ToolCallResultProgress),
    Loading(bool),
    InputChanged(char),
    GetStatus(String),
    Error(String),
    SetSessions(Vec<SessionInfo>),
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
    RejectTool(ToolCall),
    ListSessions,
    SwitchToSession(String),
}

impl AppState {
    pub fn new(helpers: Vec<&'static str>, latest_version: Option<String>) -> Self {
        let version_message = match latest_version {
            Some(version) => {
                if version != env!("CARGO_PKG_VERSION") {
                    Message::info(
                        format!(
                            "🚀 Update available!  Current: {}  →  New: {} ✨   ",
                            env!("CARGO_PKG_VERSION"),
                            version
                        ),
                        Some(Style::default().fg(ratatui::style::Color::Yellow)),
                    )
                } else {
                    Message::info(
                        format!("Current Version: {}", env!("CARGO_PKG_VERSION")),
                        None,
                    )
                }
            }
            None => Message::info(
                format!("Current Version: {}", env!("CARGO_PKG_VERSION")),
                None,
            ),
        };
        AppState {
            input: String::new(),
            cursor_position: 0,
            cursor_visible: true,
            messages: vec![
                Message::info(
                    r"
 ▗▄▄▖▗▄▄▄▖▗▄▖ ▗▖ ▗▖▗▄▄▖  ▗▄▖ ▗▖ ▗▖     ▗▄▖  ▗▄▄▖▗▄▄▄▖▗▖  ▗▖▗▄▄▄▖
▐▌     █ ▐▌ ▐▌▐▌▗▞▘▐▌ ▐▌▐▌ ▐▌▐▌▗▞▘    ▐▌ ▐▌▐▌   ▐▌   ▐▛▚▖▐▌  █  
 ▝▀▚▖  █ ▐▛▀▜▌▐▛▚▖ ▐▛▀▘ ▐▛▀▜▌▐▛▚▖     ▐▛▀▜▌▐▌▝▜▌▐▛▀▀▘▐▌ ▝▜▌  █  
▗▄▄▞▘  █ ▐▌ ▐▌▐▌ ▐▌▐▌   ▐▌ ▐▌▐▌ ▐▌    ▐▌ ▐▌▝▚▄▞▘▐▙▄▄▖▐▌  ▐▌  █  ",
                    Some(Style::default().fg(ratatui::style::Color::Cyan)),
                ),
                version_message,
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
            loading_type: LoadingType::LLM,
            spinner_frame: 0,
            sessions: Vec::new(),
            show_sessions_dialog: false,
            session_selected: 0,
            account_info: String::new(),
            pending_bash_message_id: None, // Initialize new field
            streaming_tool_results: HashMap::new(),
            streaming_tool_result_id: None,
        }
    }
}
