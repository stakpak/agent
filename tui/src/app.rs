mod events;
mod types;

pub use events::{InputEvent, OutputEvent};
use stakai::Model;
use stakpak_shared::models::llm::LLMTokenUsage;
pub use types::*;

use crate::services::approval_bar::ApprovalBar;
use crate::services::auto_approve::AutoApproveManager;
use crate::services::board_tasks::TaskProgress;
use crate::services::changeset::{Changeset, SidePanelSection, TodoItem};
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
use stakpak_api::models::ListRuleBook;
use stakpak_shared::models::integrations::openai::{ToolCall, ToolCallResult};
use stakpak_shared::secret_manager::SecretManager;
use std::collections::{HashMap, VecDeque};
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
    /// Images attached via pasted file paths (and future clipboard image support).
    pub attached_images: Vec<AttachedImage>,
    pub pending_path_start: Option<usize>,
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
    /// Per-message rendered line cache for efficient incremental rendering
    pub per_message_cache: PerMessageCache,
    /// Assembled lines cache (the final combined output of all message lines)
    /// Format: (cache_key, lines, generation_counter)
    pub assembled_lines_cache: Option<(usize, Vec<Line<'static>>, u64)>,
    /// Cache for visible lines on screen (avoids cloning on every frame)
    pub visible_lines_cache: Option<VisibleLinesCache>,
    /// Generation counter for assembled cache (increments on each rebuild)
    pub cache_generation: u64,
    /// Performance metrics for render operations
    pub render_metrics: RenderMetrics,
    /// Last width used for rendering (to detect width changes)
    pub last_render_width: usize,

    // ========== Loading State ==========
    pub loading: bool,
    pub loading_type: LoadingType,
    pub spinner_frame: usize,
    pub loading_manager: LoadingStateManager,

    // ========== Shell Popup State ==========
    pub shell_popup_visible: bool,
    pub shell_popup_expanded: bool,
    pub shell_popup_scroll: usize,
    pub shell_cursor_visible: bool,
    pub shell_cursor_blink_timer: u8,
    pub active_shell_command: Option<ShellCommand>,
    pub active_shell_command_output: Option<String>,
    pub waiting_for_shell_input: bool,
    pub shell_tool_calls: Option<Vec<ToolCallResult>>,
    pub shell_loading: bool,
    pub shell_pending_command_value: Option<String>,
    pub shell_pending_command_executed: bool,
    pub shell_pending_command_output: Option<String>,
    // Backward compatibility aliases (to be removed after full migration)
    pub show_shell_mode: bool, // alias for shell_popup_visible && shell_popup_expanded
    pub shell_mode_input: String, // unused, kept for compatibility
    pub is_tool_call_shell_command: bool,
    pub ondemand_shell_mode: bool,
    pub shell_pending_command: Option<String>,
    pub shell_pending_command_output_count: usize,
    /// Tracks if the initial shell prompt has been shown (before command is typed)
    pub shell_initial_prompt_shown: bool,
    /// Tracks if the command has been typed into the shell (after initial prompt)
    pub shell_command_typed: bool,

    // ========== Tool Call State ==========
    pub pending_bash_message_id: Option<Uuid>,
    pub streaming_tool_results: HashMap<Uuid, String>,
    pub streaming_tool_result_id: Option<Uuid>,
    pub completed_tool_calls: std::collections::HashSet<Uuid>,
    pub is_streaming: bool,
    pub latest_tool_call: Option<ToolCall>,
    /// Stable message ID for the tool call streaming preview block
    pub tool_call_stream_preview_id: Option<Uuid>,
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
    pub approval_bar: ApprovalBar,
    pub message_tool_calls: Option<Vec<ToolCall>>,
    pub message_approved_tools: Vec<ToolCall>,
    pub message_rejected_tools: Vec<ToolCall>,
    pub toggle_approved_message: bool,
    pub show_shortcuts: bool,

    // ========== Sessions State ==========
    pub sessions: Vec<SessionInfo>,
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

    // ========== Model Switcher State ==========
    pub show_model_switcher: bool,
    pub available_models: Vec<Model>,
    pub model_switcher_selected: usize,
    pub current_model: Option<Model>,
    pub model_switcher_mode: ModelSwitcherMode,
    pub model_switcher_search: String,

    // ========== Command Palette State ==========
    pub show_command_palette: bool,
    pub command_palette_selected: usize,
    pub command_palette_scroll: usize,
    pub command_palette_search: String,

    // ========== Shortcuts Popup State ==========
    pub show_shortcuts_popup: bool,
    pub shortcuts_scroll: usize,
    pub shortcuts_popup_mode: ShortcutsPopupMode,

    // ========== File Changes Popup State ==========
    pub show_file_changes_popup: bool,
    pub file_changes_selected: usize,
    pub file_changes_scroll: usize,
    pub file_changes_search: String,

    // ========== Usage Tracking State ==========
    pub current_message_usage: LLMTokenUsage,
    pub total_session_usage: LLMTokenUsage,
    pub context_usage_percent: u64,

    // ========== Configuration State ==========
    pub secret_manager: SecretManager,
    pub latest_version: Option<String>,
    pub is_git_repo: bool,
    pub auto_approve_manager: AutoApproveManager,
    pub allowed_tools: Option<Vec<String>>,
    pub model: Model,
    /// Auth display info: (config_provider, auth_provider, subscription_name) for local providers
    pub auth_display_info: (Option<String>, Option<String>, Option<String>),
    /// Content of init prompt for /init
    pub init_prompt_content: Option<String>,

    // ========== Misc State ==========
    pub ctrl_c_pressed_once: bool,
    pub ctrl_c_timer: Option<std::time::Instant>,
    pub mouse_capture_enabled: bool,
    pub terminal_size: Size,
    pub shell_screen: vt100::Parser,
    pub shell_scroll: u16,
    pub shell_history_lines: Vec<ratatui::text::Line<'static>>, // Accumulated styled history
    pub interactive_shell_message_id: Option<Uuid>,
    pub shell_interaction_occurred: bool,

    // ========== Side Panel State ==========
    pub show_side_panel: bool,
    pub side_panel_focus: SidePanelSection,
    pub side_panel_section_collapsed: std::collections::HashMap<SidePanelSection, bool>,
    /// Stores the screen area for each side panel section to handle mouse clicks
    pub side_panel_areas: HashMap<SidePanelSection, ratatui::layout::Rect>,
    /// Current session ID for backup paths
    pub session_id: String,
    pub changeset: Changeset,

    pub todos: Vec<TodoItem>,
    /// Task progress (completed/total checklist items)
    pub task_progress: Option<TaskProgress>,
    pub session_start_time: std::time::Instant,

    // Auto-show side panel tracking
    pub side_panel_auto_shown: bool,

    /// Agent board ID for task tracking (from AGENT_BOARD_AGENT_ID or created)
    pub board_agent_id: Option<String>,

    /// External editor command (vim, nvim, or nano)
    pub editor_command: String,

    /// Pending file to open in editor (set by handler, consumed by event loop)
    pub pending_editor_open: Option<String>,

    /// Billing info for the side panel
    pub billing_info: Option<stakpak_shared::models::billing::BillingResponse>,

    /// Cached pause info for subagent tasks (task_id -> pause_info)
    /// Used to display what subagents want to do in the approval bar
    pub subagent_pause_info:
        HashMap<String, stakpak_shared::models::integrations::openai::TaskPauseInfo>,
    /// Buffered user messages waiting to be sent after streaming completes
    pub pending_user_messages: VecDeque<PendingUserMessage>,

    // ========== Plan Mode State ==========
    /// Whether plan mode is active (set by /plan command, cleared by /new session)
    pub plan_mode_active: bool,
    /// Cached plan metadata from `.stakpak/session/plan.md` front matter
    pub plan_metadata: Option<crate::services::plan::PlanMetadata>,
    /// SHA-256 hash of the last-read plan content (for change detection)
    pub plan_content_hash: Option<String>,
    /// Previous plan status (for detecting transitions)
    pub plan_previous_status: Option<crate::services::plan::PlanStatus>,
    /// Whether plan review was auto-opened for current reviewing transition
    pub plan_review_auto_opened: bool,
    /// When set, the "existing plan found" modal is visible.
    /// Contains the stashed prompt and plan metadata for the modal to display.
    pub existing_plan_prompt: Option<ExistingPlanPrompt>,

    // ========== Plan Review State ==========
    /// Whether the plan review overlay is visible
    pub show_plan_review: bool,
    /// Scroll offset (line index of the top visible line)
    pub plan_review_scroll: usize,
    /// Currently selected line (0-indexed)
    pub plan_review_cursor_line: usize,
    /// Cached plan content (loaded when review opens)
    pub plan_review_content: String,
    /// Cached split lines of plan content
    pub plan_review_lines: Vec<String>,
    /// Cached plan comments (loaded when review opens)
    pub plan_review_comments: Option<crate::services::plan_comments::PlanComments>,
    /// Resolved anchors mapping comment IDs to line numbers
    pub plan_review_resolved_anchors: Vec<(String, crate::services::plan_comments::ResolvedAnchor)>,
    /// Whether the comment input modal is open
    pub plan_review_show_comment_modal: bool,
    /// Text buffer for composing a new comment
    pub plan_review_comment_input: String,
    /// Selected comment ID (for reply targeting)
    pub plan_review_selected_comment: Option<String>,
    /// Kind of comment modal currently open
    pub plan_review_modal_kind: Option<crate::services::plan_review::CommentModalKind>,
    /// Confirmation dialog currently shown (approve, feedback, delete)
    pub plan_review_confirm: Option<crate::services::plan_review::ConfirmAction>,

    // ========== Ask User Inline Block State ==========
    /// Whether the ask user interaction is active
    pub show_ask_user_popup: bool,
    /// Questions to display in the inline block
    pub ask_user_questions: Vec<stakpak_shared::models::integrations::openai::AskUserQuestion>,
    /// User's answers (question label -> answer)
    pub ask_user_answers:
        HashMap<String, stakpak_shared::models::integrations::openai::AskUserAnswer>,
    /// Currently selected tab index (question index, or questions.len() for Submit)
    pub ask_user_current_tab: usize,
    /// Currently selected option index within the current question
    pub ask_user_selected_option: usize,
    /// Custom input text when "Type something..." is selected
    pub ask_user_custom_input: String,
    /// The tool call that triggered this (for sending result back)
    pub ask_user_tool_call: Option<ToolCall>,
    /// Message ID for the inline ask_user block in the messages list
    pub ask_user_message_id: Option<Uuid>,
    /// Whether the ask_user block has keyboard focus (Tab toggles)
    pub ask_user_focused: bool,
}

pub struct AppStateOptions<'a> {
    pub latest_version: Option<String>,
    pub redact_secrets: bool,
    pub privacy_mode: bool,
    pub is_git_repo: bool,
    pub auto_approve_tools: Option<&'a Vec<String>>,
    pub allowed_tools: Option<&'a Vec<String>>,
    pub input_tx: Option<mpsc::Sender<InputEvent>>,
    pub model: Model,
    pub editor_command: Option<String>,
    /// Auth display info: (config_provider, auth_provider, subscription_name) for local providers
    pub auth_display_info: (Option<String>, Option<String>, Option<String>),
    /// Agent board ID for task tracking (from AGENT_BOARD_AGENT_ID env var)
    pub board_agent_id: Option<String>,
    /// Content of init prompt
    pub init_prompt_content: Option<String>,
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

    pub fn new(options: AppStateOptions) -> Self {
        let AppStateOptions {
            latest_version,
            redact_secrets,
            privacy_mode,
            is_git_repo,
            auto_approve_tools,
            allowed_tools,
            input_tx,
            model,
            editor_command,
            auth_display_info,
            board_agent_id,
            init_prompt_content,
        } = options;

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
            session_selected: 0,
            account_info: String::new(),
            pending_bash_message_id: None,
            streaming_tool_results: HashMap::new(),
            streaming_tool_result_id: None,
            completed_tool_calls: std::collections::HashSet::new(),
            shell_popup_visible: false,
            shell_popup_expanded: false,
            shell_popup_scroll: 0,
            shell_cursor_visible: true,
            shell_cursor_blink_timer: 0,
            active_shell_command: None,
            active_shell_command_output: None,
            waiting_for_shell_input: false,
            is_pasting: false,
            shell_tool_calls: None,
            shell_loading: false,
            shell_pending_command_value: None,
            shell_pending_command_executed: false,
            shell_pending_command_output: None,
            // Backward compatibility aliases
            show_shell_mode: false,
            shell_mode_input: String::new(),
            is_tool_call_shell_command: false,
            ondemand_shell_mode: false,
            shell_pending_command: None,
            shell_pending_command_output_count: 0,
            shell_initial_prompt_shown: false,
            shell_command_typed: false,
            attached_images: Vec::new(),
            pending_path_start: None,
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
            tool_call_stream_preview_id: None,
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
            per_message_cache: HashMap::new(),
            assembled_lines_cache: None,
            visible_lines_cache: None,
            cache_generation: 0,
            render_metrics: RenderMetrics::new(),
            last_render_width: 0,
            pending_pastes: Vec::new(),
            mouse_capture_enabled: false, // Will be set based on terminal detection in event_loop
            loading_manager: LoadingStateManager::new(),
            has_user_messages: false,
            message_tool_calls: None,
            approval_bar: ApprovalBar::new(),
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
            shell_screen: vt100::Parser::new(24, 80, 1000),
            shell_scroll: 0,
            shell_history_lines: Vec::new(),
            interactive_shell_message_id: None,
            shell_interaction_occurred: false,

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
            shortcuts_popup_mode: ShortcutsPopupMode::default(),
            // Rulebook switcher initialization
            show_rulebook_switcher: false,
            available_rulebooks: Vec::new(),
            selected_rulebooks: std::collections::HashSet::new(),
            rulebook_switcher_selected: 0,
            rulebook_search_input: String::new(),
            filtered_rulebooks: Vec::new(),

            // Model switcher initialization
            show_model_switcher: false,
            available_models: Vec::new(),
            model_switcher_selected: 0,
            current_model: None,
            model_switcher_mode: ModelSwitcherMode::default(),
            model_switcher_search: String::new(),
            // Command palette initialization
            show_command_palette: false,
            command_palette_selected: 0,
            command_palette_scroll: 0,
            command_palette_search: String::new(),

            // File changes popup initialization
            show_file_changes_popup: false,
            file_changes_selected: 0,
            file_changes_scroll: 0,
            file_changes_search: String::new(),

            // Usage tracking
            current_message_usage: LLMTokenUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                prompt_tokens_details: None,
            },
            total_session_usage: LLMTokenUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                prompt_tokens_details: None,
            },
            context_usage_percent: 0,
            model,

            // Side panel initialization
            show_side_panel: false,
            side_panel_focus: SidePanelSection::Context,
            side_panel_section_collapsed: {
                let mut collapsed = std::collections::HashMap::new();
                collapsed.insert(SidePanelSection::Context, false); // Always expanded
                collapsed.insert(SidePanelSection::Billing, false); // Expanded by default
                collapsed.insert(SidePanelSection::Tasks, false); // Expanded by default
                collapsed.insert(SidePanelSection::Changeset, false); // Expanded by default
                collapsed
            },
            side_panel_areas: HashMap::new(),
            changeset: Changeset::new(),
            todos: Vec::new(),
            task_progress: None,
            session_start_time: std::time::Instant::now(),
            side_panel_auto_shown: false,
            session_id: String::new(), // Will be set when session starts
            board_agent_id,
            editor_command: crate::services::editor::detect_editor(editor_command)
                .unwrap_or_else(|| "nano".to_string()),
            pending_editor_open: None,
            pending_user_messages: VecDeque::new(),
            billing_info: None,
            auth_display_info,

            // Plan mode initialization
            plan_mode_active: false,
            plan_metadata: None,
            plan_content_hash: None,
            plan_previous_status: None,
            plan_review_auto_opened: false,
            existing_plan_prompt: None,

            // Plan review initialization
            show_plan_review: false,
            plan_review_scroll: 0,
            plan_review_cursor_line: 0,
            plan_review_content: String::new(),
            plan_review_lines: Vec::new(),
            plan_review_comments: None,
            plan_review_resolved_anchors: Vec::new(),
            plan_review_show_comment_modal: false,
            plan_review_comment_input: String::new(),
            plan_review_selected_comment: None,
            plan_review_modal_kind: None,
            plan_review_confirm: None,
            subagent_pause_info: HashMap::new(),
            init_prompt_content,

            // Ask User inline block initialization
            show_ask_user_popup: false,
            ask_user_questions: Vec::new(),
            ask_user_answers: HashMap::new(),
            ask_user_current_tab: 0,
            ask_user_selected_option: 0,
            ask_user_custom_input: String::new(),
            ask_user_tool_call: None,
            ask_user_message_id: None,
            ask_user_focused: true,
        }
    }

    pub fn update_session_empty_status(&mut self) {
        // Check if there are any user messages (not just any messages)
        let session_empty = !self.has_user_messages && self.text_area.text().is_empty();
        self.text_area.set_session_empty(session_empty);
    }

    /// Poll `.stakpak/session/plan.md` for changes and update cached metadata.
    ///
    /// Called on each spinner tick (~100 ms) while plan mode is active.
    /// Uses SHA-256 content hashing to avoid unnecessary re-parsing.
    /// Returns `Some((old_status, new_status))` when a status transition is detected.
    pub fn poll_plan_file(
        &mut self,
    ) -> Option<(
        Option<crate::services::plan::PlanStatus>,
        crate::services::plan::PlanStatus,
    )> {
        use crate::services::plan;

        // Only poll when plan mode is active
        if !self.plan_mode_active {
            return None;
        }

        let session_dir = std::path::Path::new(".stakpak/session");
        let path = plan::plan_file_path(session_dir);

        let Ok(content) = std::fs::read_to_string(&path) else {
            // File doesn't exist (yet) — clear stale cache
            if self.plan_metadata.is_some() {
                self.plan_metadata = None;
                self.plan_content_hash = None;
            }
            return None;
        };

        let new_hash = plan::compute_plan_hash(&content);

        // Skip re-parse if content unchanged
        if self.plan_content_hash.as_deref() == Some(&new_hash) {
            return None;
        }

        self.plan_content_hash = Some(new_hash);
        let new_meta = plan::parse_plan_front_matter(&content);
        self.plan_metadata = new_meta.clone();

        // Detect status transitions
        if let Some(ref meta) = new_meta {
            let new_status = meta.status;
            let old_status = self.plan_previous_status;

            if old_status != Some(new_status) {
                self.plan_previous_status = Some(new_status);
                return Some((old_status, new_status));
            }
        }

        None
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
        let rows = if self.terminal_size.height > 0 {
            self.terminal_size.height
        } else {
            24
        };
        let cols = if self.terminal_size.width > 0 {
            self.terminal_size.width
        } else {
            80
        };

        #[cfg(unix)]
        let shell_cmd = match run_pty_command(command.clone(), None, shell_tx, rows, cols) {
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
        self.shell_screen = vt100::Parser::new(rows, cols, 0);
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
    pub fn auto_show_side_panel(&mut self) {
        if !self.side_panel_auto_shown && !self.show_side_panel {
            self.show_side_panel = true;
            self.side_panel_auto_shown = true;
        }
    }
}
