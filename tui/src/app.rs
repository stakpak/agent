mod events;
mod types;

pub use events::{InputEvent, OutputEvent};
pub use types::*;

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
use ratatui::text::Line;
use stakpak_api::ListRuleBook;
use stakpak_api::models::{
    RecoveryOption as ApiRecoveryOption, RecoveryOptionsResponse as ApiRecoveryOptionsResponse,
};
use stakpak_shared::models::integrations::openai::{AgentModel, ToolCall, ToolCallResult};
use stakpak_shared::secret_manager::SecretManager;
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct AppState {
    // ========== Input & TextArea State ==========
    pub text_area: TextArea,
    pub text_area_state: TextAreaState,
    pub cursor_visible: bool,
    pub helpers: Vec<HelperCommand>,
    pub show_helper_dropdown: bool,
    pub helper_selected: usize,
    pub helper_scroll: usize,
    pub filtered_helpers: Vec<HelperCommand>,
    pub filtered_files: Vec<String>,
    pub file_search: FileSearch,
    pub file_search_tx: Option<mpsc::Sender<(String, usize)>>,
    pub file_search_rx: Option<mpsc::Receiver<FileSearchResult>>,
    pub is_pasting: bool,
    pub pasted_long_text: Option<String>,
    pub pasted_placeholder: Option<String>,
    pub pending_pastes: Vec<(String, String)>,
    pub interactive_commands: Vec<String>,

    // ========== Messages & Scrolling State ==========
    pub messages: Vec<Message>,
    pub scroll: usize,
    pub scroll_to_bottom: bool,
    pub stay_at_bottom: bool,
    pub content_changed_while_scrolled_up: bool,
    pub message_lines_cache: Option<MessageLinesCache>,
    pub collapsed_message_lines_cache: Option<MessageLinesCache>,
    pub processed_lines_cache: Option<(Vec<Message>, usize, Vec<Line<'static>>)>,
    pub show_collapsed_messages: bool,
    pub collapsed_messages_scroll: usize,
    pub collapsed_messages_selected: usize,
    pub has_user_messages: bool,

    // ========== Loading State ==========
    pub loading: bool,
    pub loading_type: LoadingType,
    pub spinner_frame: usize,
    pub loading_manager: LoadingStateManager,

    // ========== Shell Mode State ==========
    pub show_shell_mode: bool,
    pub active_shell_command: Option<ShellCommand>,
    pub active_shell_command_output: Option<String>,
    pub shell_mode_input: String,
    pub waiting_for_shell_input: bool,
    pub is_tool_call_shell_command: bool,
    pub ondemand_shell_mode: bool,
    pub shell_tool_calls: Option<Vec<ToolCallResult>>,

    // ========== Tool Call State ==========
    pub pending_bash_message_id: Option<Uuid>,
    pub streaming_tool_results: HashMap<Uuid, String>,
    pub streaming_tool_result_id: Option<Uuid>,
    pub completed_tool_calls: std::collections::HashSet<Uuid>,
    pub is_streaming: bool,
    pub latest_tool_call: Option<ToolCall>,
    pub retry_attempts: usize,
    pub max_retry_attempts: usize,
    pub last_user_message_for_retry: Option<String>,
    pub is_retrying: bool,

    // ========== Dialog & Approval State ==========
    pub is_dialog_open: bool,
    pub dialog_command: Option<ToolCall>,
    pub dialog_selected: usize,
    pub dialog_message_id: Option<Uuid>,
    pub dialog_focused: bool,
    pub approval_popup: PopupService,
    pub message_tool_calls: Option<Vec<ToolCall>>,
    pub message_approved_tools: Vec<ToolCall>,
    pub message_rejected_tools: Vec<ToolCall>,
    pub toggle_approved_message: bool,
    pub show_shortcuts: bool,

    // ========== Sessions Dialog State ==========
    pub sessions: Vec<SessionInfo>,
    pub show_sessions_dialog: bool,
    pub session_selected: usize,
    pub account_info: String,

    // ========== Session Tool Calls Queue ==========
    pub session_tool_calls_queue: std::collections::HashMap<String, ToolCallStatus>,
    pub tool_call_execution_order: Vec<String>,
    pub last_message_tool_calls: Vec<ToolCall>,

    // ========== Profile Switcher State ==========
    pub show_profile_switcher: bool,
    pub available_profiles: Vec<String>,
    pub profile_switcher_selected: usize,
    pub current_profile_name: String,
    pub profile_switching_in_progress: bool,
    pub profile_switch_status_message: Option<String>,

    // ========== Rulebook Switcher State ==========
    pub show_rulebook_switcher: bool,
    pub available_rulebooks: Vec<ListRuleBook>,
    pub selected_rulebooks: std::collections::HashSet<String>,
    pub rulebook_switcher_selected: usize,
    pub rulebook_search_input: String,
    pub filtered_rulebooks: Vec<ListRuleBook>,
    pub rulebook_config: Option<crate::RulebookConfig>,

    // ========== Command Palette State ==========
    pub show_command_palette: bool,
    pub command_palette_selected: usize,
    pub command_palette_scroll: usize,
    pub command_palette_search: String,

    // ========== Shortcuts Popup State ==========
    pub show_shortcuts_popup: bool,
    pub shortcuts_scroll: usize,

    // ========== Context Popup State ==========
    pub show_context_popup: bool,

    // ========== Usage Tracking State ==========
    pub current_message_usage: stakpak_shared::models::integrations::openai::Usage,
    pub total_session_usage: stakpak_shared::models::integrations::openai::Usage,
    pub context_usage_percent: u64,

    // ========== Recovery Options State ==========
    pub recovery_options: Vec<ApiRecoveryOption>,
    pub show_recovery_options_popup: bool,
    pub recovery_popup_selected: usize,
    pub recovery_response: Option<ApiRecoveryOptionsResponse>,

    // ========== Configuration State ==========
    pub secret_manager: SecretManager,
    pub latest_version: Option<String>,
    pub is_git_repo: bool,
    pub auto_approve_manager: AutoApproveManager,
    pub allowed_tools: Option<Vec<String>>,
    pub model: AgentModel,

    // ========== Misc State ==========
    pub ctrl_c_pressed_once: bool,
    pub ctrl_c_timer: Option<std::time::Instant>,
    pub mouse_capture_enabled: bool,
    pub terminal_size: Size,
}

impl AppState {
    pub fn get_helper_commands() -> Vec<HelperCommand> {
        // Use unified command system
        crate::services::commands::commands_to_helper_commands()
    }

    /// Initialize file search channels and spawn worker
    fn init_file_search_channels(
        helpers: &[HelperCommand],
    ) -> (
        mpsc::Sender<(String, usize)>,
        mpsc::Receiver<FileSearchResult>,
    ) {
        let (file_search_tx, file_search_rx) = mpsc::channel::<(String, usize)>(10);
        let (result_tx, result_rx) = mpsc::channel::<FileSearchResult>(10);
        let helpers_clone = helpers.to_vec();
        let file_search_instance = FileSearch::default();
        // Spawn file_search worker from file_search.rs
        tokio::spawn(file_search_worker(
            file_search_rx,
            result_tx,
            helpers_clone,
            file_search_instance,
        ));
        (file_search_tx, result_rx)
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
        let (file_search_tx, result_rx) = Self::init_file_search_channels(&helpers);

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
            interactive_commands: crate::constants::INTERACTIVE_COMMANDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
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
            show_context_popup: false,
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
            current_message_usage: stakpak_shared::models::integrations::openai::Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                prompt_tokens_details: None,
            },
            total_session_usage: stakpak_shared::models::integrations::openai::Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                prompt_tokens_details: None,
            },
            context_usage_percent: 0,
            // ========== Recovery Options State ==========
            recovery_options: Vec::new(),
            show_recovery_options_popup: false,
            recovery_popup_selected: 0,
            recovery_response: None,
            model: AgentModel::Smart,
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
