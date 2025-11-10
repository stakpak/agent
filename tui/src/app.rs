use crate::services::approval_popup::PopupService;
use crate::services::auto_approve::AutoApproveManager;
use crate::services::detect_term::AdaptiveColors;
use crate::services::file_search::{FileSearch, file_search_worker, find_at_trigger};
use crate::services::helper_block::push_error_message;
use crate::services::helper_block::push_styled_message;
use crate::services::message::Message;
#[cfg(not(unix))]
use crate::services::shell_mode::run_background_shell_command;
#[cfg(unix)]
use crate::services::shell_mode::run_pty_command;
use crate::services::shell_mode::{SHELL_PROMPT_PREFIX, ShellCommand, ShellEvent};
use crate::services::textarea::{TextArea, TextAreaState};
use ratatui::layout::Size;
use ratatui::style::Color;
use ratatui::text::Line;
use stakpak_api::ListRuleBook;
use stakpak_api::models::{
    RecoveryOption as ApiRecoveryOption, RecoveryOptionsResponse as ApiRecoveryOptionsResponse,
};
use stakpak_shared::models::integrations::openai::{
    ToolCall, ToolCallResult, ToolCallResultProgress,
};
use stakpak_shared::secret_manager::SecretManager;
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

// Type alias to reduce complexity - now stores processed lines for better performance
type MessageLinesCache = (Vec<Message>, usize, Vec<Line<'static>>);

const INTERACTIVE_COMMANDS: [&str; 2] = ["ssh", "sudo"];

// --- NEW: Async file_search result struct ---
pub struct FileSearchResult {
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LoadingOperation {
    LlmRequest,
    ToolExecution,
    SessionsList,
    StreamProcessing,
    LocalContext,
    Rulebooks,
    CheckpointResume,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolCallStatus {
    Approved,
    Rejected,
    Executed,
    Skipped,
    Pending,
}

#[derive(Debug)]
pub struct LoadingStateManager {
    active_operations: std::collections::HashSet<LoadingOperation>,
}

impl Default for LoadingStateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadingStateManager {
    pub fn new() -> Self {
        Self {
            active_operations: std::collections::HashSet::new(),
        }
    }

    pub fn start_operation(&mut self, operation: LoadingOperation) {
        self.active_operations.insert(operation);
    }

    pub fn end_operation(&mut self, operation: LoadingOperation) {
        self.active_operations.remove(&operation);
    }

    pub fn is_loading(&self) -> bool {
        !self.active_operations.is_empty()
    }

    pub fn get_loading_type(&self) -> LoadingType {
        if self
            .active_operations
            .contains(&LoadingOperation::SessionsList)
        {
            LoadingType::Sessions
        } else {
            LoadingType::Llm
        }
    }

    pub fn clear_all(&mut self) {
        self.active_operations.clear();
    }
}

pub struct AppState {
    pub text_area: TextArea,
    pub text_area_state: TextAreaState,
    pub cursor_visible: bool,
    pub messages: Vec<Message>,
    pub scroll: usize,
    pub scroll_to_bottom: bool,
    pub stay_at_bottom: bool,
    pub content_changed_while_scrolled_up: bool,
    pub helpers: Vec<HelperCommand>,
    pub show_helper_dropdown: bool,
    pub helper_selected: usize,
    pub helper_scroll: usize,
    pub filtered_helpers: Vec<HelperCommand>,
    pub filtered_files: Vec<String>, // NEW: for file file_search
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
    pub file_search: FileSearch,
    pub secret_manager: SecretManager,
    pub latest_version: Option<String>,
    pub ctrl_c_pressed_once: bool,
    pub ctrl_c_timer: Option<std::time::Instant>,
    pub pasted_long_text: Option<String>,
    pub pasted_placeholder: Option<String>,
    // --- NEW: FileSearch channels ---
    pub file_search_tx: Option<mpsc::Sender<(String, usize)>>,
    pub file_search_rx: Option<mpsc::Receiver<FileSearchResult>>,
    pub is_streaming: bool,
    pub interactive_commands: Vec<String>,
    pub auto_approve_manager: AutoApproveManager,
    pub allowed_tools: Option<Vec<String>>,
    pub dialog_focused: bool,
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
    pub message_lines_cache: Option<MessageLinesCache>,
    pub collapsed_message_lines_cache: Option<MessageLinesCache>,
    pub processed_lines_cache: Option<(Vec<Message>, usize, Vec<Line<'static>>)>,

    pub pending_pastes: Vec<(String, String)>,
    pub mouse_capture_enabled: bool,
    pub loading_manager: LoadingStateManager,
    pub has_user_messages: bool,

    pub message_tool_calls: Option<Vec<ToolCall>>,
    pub approval_popup: PopupService,

    pub message_approved_tools: Vec<ToolCall>,
    pub message_rejected_tools: Vec<ToolCall>,

    pub toggle_approved_message: bool,
    pub terminal_size: Size,

    // Session tool calls queue to track tool call status
    pub session_tool_calls_queue: std::collections::HashMap<String, ToolCallStatus>,
    pub tool_call_execution_order: Vec<String>,
    pub last_message_tool_calls: Vec<ToolCall>,

    // Profile switcher state
    pub show_profile_switcher: bool,
    pub available_profiles: Vec<String>,
    pub profile_switcher_selected: usize,
    pub current_profile_name: String,
    pub profile_switching_in_progress: bool,
    pub profile_switch_status_message: Option<String>,
    pub rulebook_config: Option<crate::RulebookConfig>,

    // Shortcuts popup state
    pub show_shortcuts_popup: bool,
    pub shortcuts_scroll: usize,
    // Rulebook switcher state
    pub show_rulebook_switcher: bool,
    pub available_rulebooks: Vec<ListRuleBook>,
    pub selected_rulebooks: std::collections::HashSet<String>, // URIs of selected rulebooks
    pub rulebook_switcher_selected: usize,
    pub rulebook_search_input: String,
    pub filtered_rulebooks: Vec<ListRuleBook>,

    // Command palette state
    pub show_command_palette: bool,
    pub command_palette_selected: usize,
    pub command_palette_scroll: usize,
    pub command_palette_search: String,
    // Usage tracking
    pub current_message_usage: Option<stakpak_shared::models::integrations::openai::Usage>,
    pub total_session_usage: stakpak_shared::models::integrations::openai::Usage,
    pub recovery_options: Vec<ApiRecoveryOption>,
    pub show_recovery_options_popup: bool,
    pub recovery_popup_selected: usize,
    pub recovery_response: Option<ApiRecoveryOptionsResponse>,
}

#[derive(Debug)]
pub enum InputEvent {
    AssistantMessage(String),
    AddUserMessage(String),
    StreamAssistantMessage(Uuid, String),
    RunToolCall(ToolCall),
    ToolResult(ToolCallResult),
    StreamToolResult(ToolCallResultProgress),
    StartLoadingOperation(LoadingOperation),
    EndLoadingOperation(LoadingOperation),
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
    MessageToolCalls(Vec<ToolCall>),
    RecoveryOptions(ApiRecoveryOptionsResponse),
    BulkAutoApproveMessage,
    ResetAutoApproveMessage,
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
    HandleReject(Option<String>, bool, Option<Color>),
    CursorLeft,
    CursorRight,
    ToggleCursorVisible,
    Resized(u16, u16),
    ShowConfirmationDialog(ToolCall),
    DialogConfirm,
    DialogCancel,
    HasUserMessage,
    Tab,
    ToggleApprovalStatus,
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
    ToggleMouseCapture, // Toggle mouse capture on/off
    // Approval popup events
    ApprovalPopupNextTab,
    ApprovalPopupPrevTab,
    ApprovalPopupToggleApproval,
    ApprovalPopupSubmit,
    ApprovalPopupEscape,
    // Profile switcher events
    ShowProfileSwitcher,
    ProfilesLoaded(Vec<String>, String), // (available_profiles, current_profile_name)
    ProfileSwitchRequested(String),
    ProfileSwitchProgress(String),
    ProfileSwitchComplete(String),
    ProfileSwitchFailed(String),
    // Command palette events
    ShowCommandPalette,
    CommandPaletteSearchInputChanged(char),
    CommandPaletteSearchBackspace,
    ProfileSwitcherSelect,
    ProfileSwitcherCancel,
    // Shortcuts popup events
    ShowShortcuts,
    ShortcutsCancel,

    // Rulebook switcher events
    ShowRulebookSwitcher,
    RulebooksLoaded(Vec<ListRuleBook>),
    CurrentRulebooksLoaded(Vec<String>), // Currently active rulebook URIs
    RulebookSwitcherSelect,
    RulebookSwitcherToggle,
    RulebookSwitcherCancel,
    RulebookSwitcherConfirm,
    RulebookSwitcherSelectAll,   // Ctrl+D to select all rulebooks
    RulebookSwitcherDeselectAll, // Ctrl+S to deselect all rulebooks
    RulebookSearchInputChanged(char),
    RulebookSearchBackspace,
    HandleCtrlS,
    ExpandNotifications,
    // Usage tracking events
    StreamUsage(stakpak_shared::models::integrations::openai::Usage),
    RequestTotalUsage,
    TotalUsage(stakpak_shared::models::integrations::openai::Usage),
}

#[derive(Debug)]
pub enum OutputEvent {
    UserMessage(String, Option<Vec<ToolCallResult>>),
    AcceptTool(ToolCall),
    RejectTool(ToolCall, bool),
    ListSessions,
    SwitchToSession(String),
    NewSession,
    Memorize,
    SendToolResult(ToolCallResult, bool, Vec<ToolCall>),
    ResumeSession,
    RequestProfileSwitch(String),
    RequestRulebookUpdate(Vec<String>), // Selected rulebook URIs
    RequestCurrentRulebooks,            // Request currently active rulebooks
    RequestTotalUsage,                  // Request total accumulated token usage
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
                command: "/new",
                description: "Start a new session",
            },
            HelperCommand {
                command: "/memorize",
                description: "Memorize the current conversation history",
            },
            HelperCommand {
                command: "/usage",
                description: "Show token usage for this session",
            },
            HelperCommand {
                command: "/issue",
                description: "Submit issue on GitHub repo",
            },
            HelperCommand {
                command: "/support",
                description: "Go to Discord support channel",
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
                command: "/mouse_capture",
                description: "Toggle mouse capture on/off",
            },
            HelperCommand {
                command: "/profiles",
                description: "Switch to a different profile",
            },
            HelperCommand {
                command: "/quit",
                description: "Quit the application",
            },
            HelperCommand {
                command: "/shortcuts",
                description: "Show keyboard shortcuts",
            },
        ]
    }

    pub fn new(
        latest_version: Option<String>,
        redact_secrets: bool,
        privacy_mode: bool,
        is_git_repo: bool,
        auto_approve_tools: Option<&Vec<String>>,
        allowed_tools: Option<&Vec<String>>,
        input_tx: Option<mpsc::Sender<InputEvent>>,
    ) -> Self {
        let helpers = Self::get_helper_commands();
        let (file_search_tx, file_search_rx) = mpsc::channel::<(String, usize)>(10);
        let (result_tx, result_rx) = mpsc::channel::<FileSearchResult>(10);
        let helpers_clone = helpers.clone();
        let file_search_instance = FileSearch::default();
        // Spawn file_search worker from file_search.rs
        tokio::spawn(file_search_worker(
            file_search_rx,
            result_tx,
            helpers_clone,
            file_search_instance,
        ));

        // TODO(TEST): remove hardcoded recovery response when backend provides real data
        let test_recovery_options: Vec<ApiRecoveryOption> = vec![
            ApiRecoveryOption {
                id: Uuid::nil(),
                mode: stakpak_api::models::RecoveryMode::Redirection,
                state_edits: serde_json::json!([
                    {
                        "content": null,
                        "failed_tool_call_ids_to_remove": null,
                        "message_index": 14,
                        "recovery_operation": "Truncate",
                        "role": null
                    },
                    {
                        "content": null,
                        "failed_tool_call_ids_to_remove": ["tool_id_1", "tool_id_2"],
                        "message_index": 0,
                        "recovery_operation": "RemoveTools",
                        "role": null
                    },
                    {
                        "content": "Guidance text",
                        "failed_tool_call_ids_to_remove": null,
                        "message_index": 0,
                        "recovery_operation": "Append",
                        "role": "user"
                    }
                ]),
                reasoning: "Brief explanation of why this recovery is needed".to_string(),
                redirection_message: Some(
                    "Guidance message with sections like [WHAT WENT WRONG], [WHAT TO AVOID], etc."
                        .to_string(),
                ),
                revert_to_checkpoint: None,
                model: None,
                system_prompt_key: None,
            },
            ApiRecoveryOption {
                id: Uuid::from_u128(1),
                mode: stakpak_api::models::RecoveryMode::Revert,
                state_edits: serde_json::json!([
                    {
                        "content": "Guidance text",
                        "failed_tool_call_ids_to_remove": null,
                        "message_index": 0,
                        "recovery_operation": "Append",
                        "role": "user"
                    }
                ]),
                reasoning: "Revert to checkpoint and provide guidance".to_string(),
                redirection_message: Some("Guidance message".to_string()),
                revert_to_checkpoint: Some(Uuid::from_u128(0x10)),
                model: None,
                system_prompt_key: None,
            },
            ApiRecoveryOption {
                id: Uuid::from_u128(2),
                mode: stakpak_api::models::RecoveryMode::ModelChange,
                state_edits: serde_json::json!([
                    {
                        "content": "Guidance text",
                        "failed_tool_call_ids_to_remove": null,
                        "message_index": 0,
                        "recovery_operation": "Append",
                        "role": "user"
                    }
                ]),
                reasoning: "Switch to more capable model".to_string(),
                redirection_message: Some("Guidance message".to_string()),
                revert_to_checkpoint: Some(Uuid::from_u128(0x20)),
                model: None,
                system_prompt_key: None,
            },
        ];
        let test_recovery_response = ApiRecoveryOptionsResponse {
            id: Some("test-recovery-response".to_string()),
            recovery_options: test_recovery_options.clone(),
        };

        AppState {
            text_area: TextArea::new(),
            text_area_state: TextAreaState::default(),
            cursor_visible: true,
            messages: Vec::new(), // Will be populated after state is created
            scroll: 0,
            scroll_to_bottom: false,
            stay_at_bottom: true,
            content_changed_while_scrolled_up: false,
            helpers: helpers.clone(),
            show_helper_dropdown: false,
            helper_selected: 0,
            helper_scroll: 0,
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
            file_search: FileSearch::default(),
            secret_manager: SecretManager::new(redact_secrets, privacy_mode),
            latest_version: latest_version.clone(),
            ctrl_c_pressed_once: false,
            ctrl_c_timer: None,
            pasted_long_text: None,
            pasted_placeholder: None,
            file_search_tx: Some(file_search_tx),
            file_search_rx: Some(result_rx),
            is_streaming: false,
            interactive_commands: INTERACTIVE_COMMANDS.iter().map(|s| s.to_string()).collect(),
            auto_approve_manager: AutoApproveManager::new(auto_approve_tools, input_tx),
            allowed_tools: allowed_tools.cloned(),
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
            message_lines_cache: None,
            collapsed_message_lines_cache: None,
            processed_lines_cache: None,
            pending_pastes: Vec::new(),
            mouse_capture_enabled: crate::services::detect_term::is_unsupported_terminal(
                &crate::services::detect_term::detect_terminal().emulator,
            ), // Start with mouse capture enabled only for supported terminals
            loading_manager: LoadingStateManager::new(),
            has_user_messages: false,
            message_tool_calls: None,
            approval_popup: PopupService::new(),
            message_approved_tools: Vec::new(),
            message_rejected_tools: Vec::new(),
            toggle_approved_message: true,
            terminal_size: Size {
                width: 0,
                height: 0,
            },

            // Session tool calls queue to track tool call status
            session_tool_calls_queue: std::collections::HashMap::new(),
            tool_call_execution_order: Vec::new(),
            last_message_tool_calls: Vec::new(),

            // Profile switcher initialization
            show_profile_switcher: false,
            available_profiles: Vec::new(),
            profile_switcher_selected: 0,
            current_profile_name: "default".to_string(),
            profile_switching_in_progress: false,
            profile_switch_status_message: None,
            rulebook_config: None,

            // Shortcuts popup initialization
            show_shortcuts_popup: false,
            shortcuts_scroll: 0,
            // Rulebook switcher initialization
            show_rulebook_switcher: false,
            available_rulebooks: Vec::new(),
            selected_rulebooks: std::collections::HashSet::new(),
            rulebook_switcher_selected: 0,
            rulebook_search_input: String::new(),
            filtered_rulebooks: Vec::new(),

            // Command palette initialization
            show_command_palette: false,
            command_palette_selected: 0,
            command_palette_scroll: 0,
            command_palette_search: String::new(),
            // Usage tracking
            current_message_usage: None,
            total_session_usage: stakpak_shared::models::integrations::openai::Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                prompt_tokens_details: None,
            },
            show_recovery_options_popup: false,
            recovery_popup_selected: 0,
            recovery_options: test_recovery_options,
            recovery_response: Some(test_recovery_response),
        }
    }

    pub fn update_session_empty_status(&mut self) {
        // Check if there are any user messages (not just any messages)
        let session_empty = !self.has_user_messages && self.text_area.text().is_empty();
        self.text_area.set_session_empty(session_empty);
    }

    // Convenience methods for accessing input and cursor
    pub fn input(&self) -> &str {
        self.text_area.text()
    }

    pub fn cursor_position(&self) -> usize {
        self.text_area.cursor()
    }

    pub fn set_input(&mut self, input: &str) {
        self.text_area.set_text(input);
    }

    pub fn set_cursor_position(&mut self, pos: usize) {
        self.text_area.set_cursor(pos);
    }

    pub fn insert_char(&mut self, c: char) {
        self.text_area.insert_str(&c.to_string());
    }

    pub fn insert_str(&mut self, s: &str) {
        self.text_area.insert_str(s);
    }

    pub fn clear_input(&mut self) {
        self.text_area.set_text("");
    }

    /// Check if user input should be blocked (during profile switch)
    pub fn is_input_blocked(&self) -> bool {
        self.profile_switching_in_progress
    }

    pub fn run_shell_command(&mut self, command: String, input_tx: &mpsc::Sender<InputEvent>) {
        let (shell_tx, mut shell_rx) = mpsc::channel::<ShellEvent>(100);
        self.messages.push(Message::plain_text("SPACING_MARKER"));
        push_styled_message(
            self,
            &command,
            AdaptiveColors::text(),
            SHELL_PROMPT_PREFIX,
            AdaptiveColors::dark_magenta(),
        );
        self.messages.push(Message::plain_text("SPACING_MARKER"));
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

    // --- NEW: Poll file_search results and update state ---
    pub fn poll_file_search_results(&mut self) {
        if let Some(rx) = &mut self.file_search_rx {
            while let Ok(result) = rx.try_recv() {
                // Get input text before any mutable operations
                let input_text = self.text_area.text().to_string();

                let filtered_files = result.filtered_files.clone();
                self.filtered_files = filtered_files;
                self.file_search.filtered_files = self.filtered_files.clone();
                self.file_search.is_file_mode = !self.filtered_files.is_empty();
                self.file_search.trigger_char = if !self.filtered_files.is_empty() {
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
                self.show_helper_dropdown = (input_text.trim().starts_with('/'))
                    || (!self.filtered_helpers.is_empty() && input_text.starts_with('/'))
                    || (has_at_trigger && !self.waiting_for_shell_input);
            }
        }
    }
}
