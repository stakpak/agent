use crate::services::auto_approve::AutoApproveManager;
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

use crate::services::shell_mode::{SHELL_PROMPT_PREFIX, ShellCommand, ShellEvent};

use crate::services::helper_block::push_error_message;
#[cfg(unix)]
use crate::services::shell_mode::run_pty_command;
#[cfg(not(unix))]
use crate::services::shell_mode::run_background_shell_command;

const INTERACTIVE_COMMANDS: [&str; 2] = ["ssh", "sudo"];

// --- NEW: Async autocomplete result struct ---
pub struct AutoCompleteResult {
    pub filtered_helpers: Vec<HelperCommand>,
    pub filtered_files: Vec<String>,
    pub cursor_position: usize,
    pub input: String,
}

#[derive(Debug, Clone)]
pub struct HelperCommand {
    pub command: &'static str,
    pub description: &'static str,
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
    pub helpers: Vec<HelperCommand>,
    pub show_helper_dropdown: bool,
    pub helper_selected: usize,
    pub filtered_helpers: Vec<HelperCommand>,
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
    pub auto_approve_manager: AutoApproveManager,
    pub dialog_focused: bool, // NEW: tracks which area has focus when dialog is open
    pub latest_tool_call: Option<ToolCall>,
    // Retry mechanism state
    pub retry_attempts: usize,
    pub max_retry_attempts: usize,
    pub last_user_message_for_retry: Option<String>,
    pub is_retrying: bool,
    pub show_collapsed_messages: bool, // NEW: tracks if collapsed messages popup is open
    pub collapsed_messages_scroll: usize, // NEW: scroll position for collapsed messages popup
    pub collapsed_messages_selected: usize, // NEW: selected message index in collapsed messages popup

    pub is_git_repo: bool,
}

#[derive(Debug)]
pub enum InputEvent {
    AssistantMessage(String),
    AddUserMessage(String),
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
    ShellKill,
    HandlePaste(String),
    InputDelete,
    InputDeleteWord,
    InputCursorStart,
    InputCursorEnd,
    InputCursorPrevWord,
    InputCursorNextWord,
    ToggleAutoApprove,
    AutoApproveCurrentTool,
    ToggleDialogFocus,       // NEW: toggle between messages view and dialog focus
    RetryLastToolCall,       // Ctrl+R to retry last tool call in shell mode
    AttemptQuit,             // First Ctrl+C press for quit sequence
    ToggleCollapsedMessages, // Ctrl+T to toggle collapsed messages popup
    EmergencyClearTerminal,
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
    ResumeSession,
}

impl AppState {
    pub fn get_helper_commands() -> Vec<HelperCommand> {
        vec![
            HelperCommand {
                command: "/help",
                description: "Show help information and available commands",
            },
            HelperCommand {
                command: "/clear",
                description: "Clear the screen and show welcome message",
            },
            HelperCommand {
                command: "/status",
                description: "Show account status and current working directory",
            },
            HelperCommand {
                command: "/sessions",
                description: "List available sessions to switch to",
            },
            HelperCommand {
                command: "/resume",
                description: "Resume the last session",
            },
            HelperCommand {
                command: "/memorize",
                description: "Memorize the current conversation history",
            },
            HelperCommand {
                command: "/list_approved_tools",
                description: "List all tools that are auto-approved",
            },
            HelperCommand {
                command: "/toggle_auto_approve",
                description: "Toggle auto-approve for a specific tool e.g. /toggle_auto_approve view",
            },
            HelperCommand {
                command: "/quit",
                description: "Quit the application",
            },
        ]
    }

    pub fn new(
        latest_version: Option<String>,
        redact_secrets: bool,
        privacy_mode: bool,
        is_git_repo: bool,
    ) -> Self {
        let helpers = Self::get_helper_commands();
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
            auto_approve_manager: AutoApproveManager::new(),
            dialog_focused: false, // Default to messages view focused
            latest_tool_call: None,
            retry_attempts: 0,
            max_retry_attempts: 3,
            last_user_message_for_retry: None,
            is_retrying: false,
            show_collapsed_messages: false,
            collapsed_messages_scroll: 0,
            collapsed_messages_selected: 0,
            is_git_repo,
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
        #[cfg(unix)]
        let shell_cmd = match run_pty_command(command.clone(), shell_tx) {
            Ok(cmd) => cmd,
            Err(e) => {
                push_error_message(self, &format!("Failed to run command: {}", e), None);
                return;
            }
        };

        #[cfg(not(unix))]
        let shell_cmd = run_background_shell_command(command.clone(), shell_tx);

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

                // Update filtered_helpers from async worker
                self.filtered_helpers = result.filtered_helpers;

                // Reset selection index if it's out of bounds
                if !self.filtered_helpers.is_empty()
                    && self.helper_selected >= self.filtered_helpers.len()
                {
                    self.helper_selected = 0;
                }

                // Show dropdown if input is exactly '/' or if filtered_helpers is not empty and input starts with '/'
                let has_at_trigger =
                    find_at_trigger(&result.input, result.cursor_position).is_some();
                self.show_helper_dropdown = (self.input.trim().starts_with('/'))
                    || (!self.filtered_helpers.is_empty() && self.input.starts_with('/'))
                    || (has_at_trigger && !is_files_empty && !self.waiting_for_shell_input);
            }
        }
    }
}
