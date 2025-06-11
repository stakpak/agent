use crate::services::helper_block::push_error_message;
use crate::services::message::Message;
use ratatui::style::{Color, Style};
use stakpak_shared::models::integrations::openai::{
    ToolCall, ToolCallResult, ToolCallResultProgress,
};
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::services::shell_mode::{
    ShellCommand, ShellEvent, run_background_shell_command, run_pty_command,
};

#[derive(Debug)]
pub struct SessionInfo {
    pub title: String,
    pub id: String,
    pub updated_at: String,
    pub checkpoints: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub enum LoadingType {
    Llm,
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
    pub show_shell_mode: bool,
    pub active_shell_command: Option<ShellCommand>,
    pub shell_mode_input: String,
    pub waiting_for_shell_input: bool,
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
    ShellOutput(String),
    ShellError(String),
    ShellInputRequest(String),
    ShellCompleted(i32),
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
                if version != format!("v{}", env!("CARGO_PKG_VERSION")) {
                    Message::info(
                        format!(
                            "🚀 Update available!  v{}  →  {} ✨   ",
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
            loading_type: LoadingType::Llm,
            spinner_frame: 0,
            sessions: Vec::new(),
            show_sessions_dialog: false,
            session_selected: 0,
            account_info: String::new(),
            pending_bash_message_id: None, // Initialize new field
            streaming_tool_results: HashMap::new(),
            streaming_tool_result_id: None,
            show_shell_mode: false,
            active_shell_command: None,
            shell_mode_input: String::new(),
            waiting_for_shell_input: false,
        }
    }

    pub fn run_shell_command(&mut self, command: String, input_tx: &mpsc::Sender<InputEvent>) {
        let (shell_tx, mut shell_rx) = mpsc::channel::<ShellEvent>(100);

        // Show the command being run
        self.messages.push(Message::info(
            format!("$ {}", command),
            Some(Style::default().fg(Color::Gray)),
        ));

        // Use PTY for sudo commands
        let shell_cmd = if command.contains("sudo") || command.contains("ssh") {
            #[cfg(unix)]
            {
                match run_pty_command(command.clone(), shell_tx) {
                    Ok(cmd) => cmd,
                    Err(e) => {
                        push_error_message(self, &format!("Failed to run command: {}", e));
                        return;
                    }
                }
            }
            #[cfg(not(unix))]
            {
                run_background_shell_command(command.clone(), shell_tx)
            }
        } else {
            run_background_shell_command(command.clone(), shell_tx)
        };

        // Store the command handle
        self.active_shell_command = Some(shell_cmd.clone());

        // Spawn task to handle shell events and convert to InputEvents
        let input_tx = input_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = shell_rx.recv().await {
                match event {
                    ShellEvent::Output(line) => {
                        let _ = input_tx.send(InputEvent::ShellOutput(line)).await;
                    }
                    ShellEvent::Error(line) => {
                        let _ = input_tx.send(InputEvent::ShellError(line)).await;
                    }
                    ShellEvent::InputRequest(prompt) => {
                        let _ = input_tx.send(InputEvent::ShellInputRequest(prompt)).await;
                    }
                    ShellEvent::Completed(code) => {
                        let _ = input_tx.send(InputEvent::ShellCompleted(code)).await;
                        break;
                    }
                }
            }
        });
    }
}
