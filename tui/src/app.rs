use crate::services::auto_complete::{AutoComplete, autocomplete_worker, find_at_trigger};
use crate::services::helper_block::{push_styled_message, welcome_messages};
use crate::services::message::Message;
use crate::services::render_input::get_multiline_input_lines;
use ratatui::style::Color;
use ratatui::text::Line;
use stakpak_shared::models::integrations::openai::{
    ToolCall, ToolCallResult, ToolCallResultProgress,
};
use stakpak_shared::secret_manager::SecretManager;
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::services::shell_mode::{
    SHELL_PROMPT_PREFIX, ShellCommand, ShellEvent, run_background_shell_command,
};

use crate::services::helper_block::push_error_message;
#[cfg(unix)]
use crate::services::shell_mode::run_pty_command;

const INTERACTIVE_COMMANDS: [&str; 2] = ["ssh", "sudo"];

// --- NEW: Async autocomplete result struct ---
pub struct AutoCompleteResult {
    pub filtered_helpers: Vec<&'static str>,
    pub filtered_files: Vec<String>,
    pub cursor_position: usize,
    pub input: String,
}

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
    pub filtered_files: Vec<String>, // NEW: for file autocomplete
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
    pub pending_bash_message_id: Option<Uuid>,
    pub streaming_tool_results: HashMap<Uuid, String>,
    pub streaming_tool_result_id: Option<Uuid>,
    pub completed_tool_calls: std::collections::HashSet<Uuid>,
    pub show_shell_mode: bool,
    pub active_shell_command: Option<ShellCommand>,
    pub active_shell_command_output: Option<String>,
    pub shell_mode_input: String,
    pub waiting_for_shell_input: bool,
    pub is_tool_call_shell_command: bool,
    pub is_pasting: bool,
    pub ondemand_shell_mode: bool,
    pub shell_tool_calls: Option<Vec<ToolCallResult>>,
    pub dialog_message_id: Option<Uuid>,
    pub autocomplete: AutoComplete,
    pub secret_manager: SecretManager,
    pub latest_version: Option<String>,
    pub ctrl_c_pressed_once: bool,
    pub ctrl_c_timer: Option<std::time::Instant>,
    pub pasted_long_text: Option<String>,
    pub pasted_placeholder: Option<String>,
    // --- NEW: autocomplete channels ---
    pub autocomplete_tx: Option<mpsc::Sender<(String, usize)>>,
    pub autocomplete_rx: Option<mpsc::Receiver<AutoCompleteResult>>,
    pub is_streaming: bool,
    pub interactive_commands: Vec<String>,
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
    ShellMode,
    GetStatus(String),
    Error(String),
    SetSessions(Vec<SessionInfo>),
    InputBackspace,
    InputChangedNewline,
    InputSubmitted,
    InputSubmittedWith(String),
    InputSubmittedWithColor(String, Color),
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
    ShellWaitingForInput,
    ShellCompleted(i32),
    ShellClear,
    HandlePaste(String),
    InputDelete,
    InputDeleteWord,
    InputCursorStart,
    InputCursorEnd,
    InputCursorPrevWord,
    InputCursorNextWord,
    AttemptQuit, // First Ctrl+C press for quit sequence
}

#[derive(Debug)]
pub enum OutputEvent {
    UserMessage(String, Option<Vec<ToolCallResult>>),
    AcceptTool(ToolCall),
    RejectTool(ToolCall),
    ListSessions,
    SwitchToSession(String),
    Memorize,
    SendToolResult(ToolCallResult),
}

impl AppState {
    pub fn new(
        helpers: Vec<&'static str>,
        latest_version: Option<String>,
        redact_secrets: bool,
        privacy_mode: bool,
    ) -> Self {
        let (autocomplete_tx, autocomplete_rx) = mpsc::channel::<(String, usize)>(10);
        let (result_tx, result_rx) = mpsc::channel::<AutoCompleteResult>(10);
        let helpers_clone = helpers.clone();
        let autocomplete_instance = AutoComplete::default();
        // Spawn autocomplete worker from auto_complete.rs
        tokio::spawn(autocomplete_worker(
            autocomplete_rx,
            result_tx,
            helpers_clone,
            autocomplete_instance,
        ));

        AppState {
            input: String::new(),
            cursor_position: 0,
            cursor_visible: true,
            messages: welcome_messages(latest_version.clone()),
            scroll: 0,
            scroll_to_bottom: false,
            stay_at_bottom: true,
            helpers: helpers.clone(),
            show_helper_dropdown: false,
            helper_selected: 0,
            filtered_helpers: helpers,
            filtered_files: Vec::new(),
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
            pending_bash_message_id: None,
            streaming_tool_results: HashMap::new(),
            streaming_tool_result_id: None,
            completed_tool_calls: std::collections::HashSet::new(),
            show_shell_mode: false,
            active_shell_command: None,
            active_shell_command_output: None,
            shell_mode_input: String::new(),
            waiting_for_shell_input: false,
            is_tool_call_shell_command: false,
            is_pasting: false,
            ondemand_shell_mode: false,
            shell_tool_calls: None,
            dialog_message_id: None,
            autocomplete: AutoComplete::default(),
            secret_manager: SecretManager::new(redact_secrets, privacy_mode),
            latest_version: latest_version.clone(),
            ctrl_c_pressed_once: false,
            ctrl_c_timer: None,
            pasted_long_text: None,
            pasted_placeholder: None,
            autocomplete_tx: Some(autocomplete_tx),
            autocomplete_rx: Some(result_rx),
            is_streaming: false,
            interactive_commands: INTERACTIVE_COMMANDS.iter().map(|s| s.to_string()).collect(),
        }
    }
    pub fn render_input(&self, area_width: usize) -> (Vec<Line>, bool) {
        let (lines, cursor_rendered) = get_multiline_input_lines(self, area_width);
        (lines, cursor_rendered)
    }
    pub fn run_shell_command(&mut self, command: String, input_tx: &mpsc::Sender<InputEvent>) {
        let (shell_tx, mut shell_rx) = mpsc::channel::<ShellEvent>(100);
        push_styled_message(
            self,
            &command,
            Color::Rgb(180, 180, 180),
            SHELL_PROMPT_PREFIX,
            Color::Rgb(160, 92, 158),
        );
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
        self.active_shell_command = Some(shell_cmd.clone());
        self.active_shell_command_output = Some(String::new());
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
                    ShellEvent::WaitingForInput => {
                        let _ = input_tx.send(InputEvent::ShellWaitingForInput).await;
                    }
                    ShellEvent::Completed(code) => {
                        let _ = input_tx.send(InputEvent::ShellCompleted(code)).await;
                        break;
                    }
                    ShellEvent::Clear => {
                        let _ = input_tx.send(InputEvent::ShellClear).await;
                    }
                }
            }
        });
    }

    // --- NEW: Poll autocomplete results and update state ---
    pub fn poll_autocomplete_results(&mut self) {
        if let Some(rx) = &mut self.autocomplete_rx {
            while let Ok(result) = rx.try_recv() {
                let filtered_files = result.filtered_files.clone();
                let is_files_empty = filtered_files.is_empty();
                self.filtered_files = filtered_files;
                self.autocomplete.filtered_files = self.filtered_files.clone();
                self.autocomplete.is_file_mode = !self.filtered_files.is_empty();
                self.autocomplete.trigger_char = if !self.filtered_files.is_empty() {
                    Some('@')
                } else {
                    None
                };
                // Show dropdown if input is exactly '/' or if filtered_helpers is not empty and input starts with '/'
                let has_at_trigger =
                    find_at_trigger(&result.input, result.cursor_position).is_some();
                self.show_helper_dropdown = (self.input.trim() == "/")
                    || (!self.filtered_helpers.is_empty() && self.input.starts_with('/'))
                    || (has_at_trigger && !is_files_empty);
            }
        }
    }
}
