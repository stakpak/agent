//! Type Definitions Module
//!
//! This module contains all type definitions used throughout the TUI application.
//! Types are organized here for better maintainability and code organization.

use crate::services::file_search::FileSearch;
use crate::services::message::Message;
use crate::services::approval_bar::ApprovalBar;
use crate::services::auto_approve::AutoApproveManager;
use crate::services::shell_mode::ShellCommand;
use crate::services::textarea::{TextArea, TextAreaState};
use ratatui::text::Line;
use stakai::Model;
use stakpak_api::models::ListRuleBook;
use stakpak_shared::secret_manager::SecretManager;
use stakpak_shared::models::integrations::openai::{ContentPart, ToolCall, ToolCallResult};
use stakpak_shared::models::llm::LLMTokenUsage;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

// Type alias to reduce complexity - now stores processed lines for better performance
pub type MessageLinesCache = (Vec<Message>, usize, Vec<Line<'static>>);

/// Cached rendered lines for a single message.
/// Uses Arc to avoid expensive cloning when returning cached lines.
#[derive(Clone, Debug)]
pub struct RenderedMessageCache {
    /// Hash of the message content for change detection
    pub content_hash: u64,
    /// The rendered lines for this message (shared via Arc to avoid cloning)
    pub rendered_lines: Arc<Vec<Line<'static>>>,
    /// Width the message was rendered at
    pub width: usize,
}

/// Per-message cache for efficient incremental rendering.
/// Only re-renders messages that have actually changed.
pub type PerMessageCache = HashMap<Uuid, RenderedMessageCache>;

/// Cache for the currently visible lines on screen.
/// This avoids re-slicing and cloning on every frame when only scroll position changes.
#[derive(Clone, Debug)]
pub struct VisibleLinesCache {
    /// The scroll position these lines were computed for
    pub scroll: usize,
    /// The width these lines were computed for
    pub width: usize,
    /// The height (number of lines) requested
    pub height: usize,
    /// The visible lines (Arc to avoid cloning on every frame)
    pub lines: Arc<Vec<Line<'static>>>,
    /// Generation counter from assembled cache (to detect when source changed)
    pub source_generation: u64,
}

/// Performance metrics for render operations (for benchmarking)
#[derive(Debug, Default, Clone)]
pub struct RenderMetrics {
    /// Total time spent rendering in the last render cycle (microseconds)
    pub last_render_time_us: u64,
    /// Number of messages that hit the cache
    pub cache_hits: usize,
    /// Number of messages that missed the cache and required re-rendering
    pub cache_misses: usize,
    /// Total number of lines rendered
    pub total_lines: usize,
    /// Rolling average render time (microseconds)
    pub avg_render_time_us: u64,
    /// Number of render cycles tracked for average
    render_count: u64,
}

impl RenderMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new render cycle's metrics
    pub fn record_render(
        &mut self,
        render_time_us: u64,
        cache_hits: usize,
        cache_misses: usize,
        total_lines: usize,
    ) {
        self.last_render_time_us = render_time_us;
        self.cache_hits = cache_hits;
        self.cache_misses = cache_misses;
        self.total_lines = total_lines;

        // Update rolling average
        self.render_count += 1;
        if self.render_count == 1 {
            self.avg_render_time_us = render_time_us;
        } else {
            // Exponential moving average with alpha = 0.1
            self.avg_render_time_us = (self.avg_render_time_us * 9 + render_time_us) / 10;
        }
    }

    /// Reset metrics (useful for benchmarking specific scenarios)
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Async file_search result struct
pub struct FileSearchResult {
    pub filtered_helpers: Vec<HelperCommand>,
    pub filtered_files: Vec<String>,
    pub cursor_position: usize,
    pub input: String,
}

/// Source of a slash command — built-in or loaded from a custom `.md` file.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandSource {
    /// Hard-coded command handled by `execute_command()`.
    BuiltIn,
    /// Built-in command whose prompt is embedded at compile time (e.g. `/claw`, `/review`).
    /// Handled generically by the `_ =>` fallback in `execute_command()` — no bespoke
    /// match arm required. If the prompt contains `{input}`, it accepts user arguments
    /// and the placeholder is replaced at runtime; otherwise the prompt fires as-is.
    BuiltInWithPrompt { prompt_content: String },
    /// User-defined command loaded from `~/.stakpak/commands/` or `.stakpak/commands/`.
    /// The `prompt_content` is the raw markdown body of the file.
    Custom { prompt_content: String },
}

#[derive(Debug, Clone)]
pub struct HelperCommand {
    pub command: String,
    pub description: String,
    pub source: CommandSource,
}

#[derive(Debug, Clone)]
pub struct AttachedImage {
    pub placeholder: String,
    pub path: PathBuf,
    pub filename: String,
    pub dimensions: (u32, u32),
    pub start_pos: usize,
    pub end_pos: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingUserMessage {
    pub final_input: String,
    pub shell_tool_calls: Option<Vec<ToolCallResult>>,
    pub image_parts: Vec<ContentPart>,
    pub user_message_text: String,
}

impl PendingUserMessage {
    pub fn new(
        final_input: String,
        shell_tool_calls: Option<Vec<ToolCallResult>>,
        image_parts: Vec<ContentPart>,
        user_message_text: String,
    ) -> Self {
        Self {
            final_input,
            shell_tool_calls,
            image_parts,
            user_message_text,
        }
    }

    pub fn merge_from(&mut self, other: PendingUserMessage) {
        fn append_with_separator(target: &mut String, value: &str) {
            if value.is_empty() {
                return;
            }
            if !target.is_empty() {
                target.push_str("\n\n");
            }
            target.push_str(value);
        }

        append_with_separator(&mut self.final_input, &other.final_input);
        append_with_separator(&mut self.user_message_text, &other.user_message_text);

        self.image_parts.extend(other.image_parts);

        match (&mut self.shell_tool_calls, other.shell_tool_calls) {
            (Some(existing), Some(mut incoming)) => existing.append(&mut incoming),
            (None, Some(incoming)) => self.shell_tool_calls = Some(incoming),
            _ => {}
        }
    }
}

/// Stashed state for the "existing plan found" modal.
///
/// When plan mode is requested and `.stakpak/session/plan.md` already exists,
/// the inline prompt is stashed here while the user decides whether to resume
/// or start fresh.
#[derive(Debug, Clone)]
pub struct ExistingPlanPrompt {
    /// The inline prompt from `/plan <prompt>` (or `None` for bare `/plan`).
    pub inline_prompt: Option<String>,
    /// Metadata parsed from the existing plan file (for display in the modal).
    pub metadata: Option<crate::services::plan::PlanMetadata>,
}

/// Input & TextArea state - handles user input, autocomplete dropdowns, and file search
#[derive(Debug)]
pub struct InputState {
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
    pub attached_images: Vec<AttachedImage>,
    pub pending_path_start: Option<usize>,
    pub interactive_commands: Vec<String>,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            text_area: TextArea::new(),
            text_area_state: TextAreaState::default(),
            cursor_visible: true,
            helpers: Vec::new(),
            show_helper_dropdown: false,
            helper_selected: 0,
            helper_scroll: 0,
            filtered_helpers: Vec::new(),
            filtered_files: Vec::new(),
            file_search: FileSearch::default(),
            file_search_tx: None,
            file_search_rx: None,
            is_pasting: false,
            pasted_long_text: None,
            pasted_placeholder: None,
            pending_pastes: Vec::new(),
            attached_images: Vec::new(),
            pending_path_start: None,
            interactive_commands: Vec::new(),
        }
    }
}

pub struct LoadingState {
    pub is_loading: bool,
    pub loading_type: LoadingType,
    pub spinner_frame: usize,
    pub loading_manager: LoadingStateManager
}

pub struct MessagesScrollingState {
    pub messages: Vec<Message>,
    pub scroll: usize,
    pub scroll_to_bottom: bool,
    pub scroll_to_last_message_start: bool,
    pub stay_at_bottom: bool,
    /// Counter to block stay_at_bottom for N frames (used when scroll_to_last_message_start needs to persist)
    pub block_stay_at_bottom_frames: u8,
    /// When scroll is locked, this stores how many lines from the end we want to show at top of viewport
    /// This allows us to maintain relative position even as total_lines changes
    pub scroll_lines_from_end: Option<usize>,
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
    /// Format: (cache_key_hash, lines, generation_counter)
    pub assembled_lines_cache: Option<(u64, Vec<Line<'static>>, u64)>,
    /// Cache for visible lines on screen (avoids cloning on every frame)
    pub visible_lines_cache: Option<VisibleLinesCache>,
    /// Generation counter for assembled cache (increments on each rebuild)
    pub cache_generation: u64,
    /// Performance metrics for render operations
    pub render_metrics: RenderMetrics,
    /// Last width used for rendering (to detect width changes)
    pub last_render_width: usize,
    /// Maps line ranges to message info for click detection
    /// Format: Vec<(start_line, end_line, message_id, is_user_message, message_text, user_message_index)>
    pub line_to_message_map: Vec<(usize, usize, Uuid, bool, String, usize)>,
}

impl Default for MessagesScrollingState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            scroll: 0,
            scroll_to_bottom: false,
            scroll_to_last_message_start: false,
            stay_at_bottom: true,
            block_stay_at_bottom_frames: 0,
            scroll_lines_from_end: None,
            content_changed_while_scrolled_up: false,
            message_lines_cache: None,
            collapsed_message_lines_cache: None,
            processed_lines_cache: None,
            show_collapsed_messages: false,
            collapsed_messages_scroll: 0,
            collapsed_messages_selected: 0,
            has_user_messages: false,
            per_message_cache: HashMap::new(),
            assembled_lines_cache: None,
            visible_lines_cache: None,
            cache_generation: 0,
            render_metrics: RenderMetrics::new(),
            last_render_width: 0,
            line_to_message_map: Vec::new(),
        }
    }
}

pub struct SidePanelState {
    pub show_side_panel: bool,
    pub side_panel_focus: crate::services::changeset::SidePanelSection,
    pub side_panel_section_collapsed:
        HashMap<crate::services::changeset::SidePanelSection, bool>,
    pub side_panel_areas:
        HashMap<crate::services::changeset::SidePanelSection, ratatui::layout::Rect>,
    pub session_id: String,
    pub session_id_copied_at: Option<std::time::Instant>,
    pub changeset: crate::services::changeset::Changeset,
    pub todos: Vec<crate::services::changeset::TodoItem>,
    pub task_progress: Option<crate::services::board_tasks::TaskProgress>,
    pub session_start_time: std::time::Instant,
    pub side_panel_auto_shown: bool,
    pub board_agent_id: Option<String>,
    pub editor_command: String,
    pub pending_editor_open: Option<String>,
    pub billing_info: Option<stakpak_shared::models::billing::BillingResponse>,
}

impl Default for SidePanelState {
    fn default() -> Self {
        let mut collapsed = HashMap::new();
        collapsed.insert(crate::services::changeset::SidePanelSection::Context, false);
        collapsed.insert(crate::services::changeset::SidePanelSection::Billing, false);
        collapsed.insert(crate::services::changeset::SidePanelSection::Tasks, false);
        collapsed.insert(crate::services::changeset::SidePanelSection::Changeset, false);

        Self {
            show_side_panel: false,
            side_panel_focus: crate::services::changeset::SidePanelSection::Context,
            side_panel_section_collapsed: collapsed,
            side_panel_areas: HashMap::new(),
            session_id: String::new(),
            session_id_copied_at: None,
            changeset: crate::services::changeset::Changeset::new(),
            todos: Vec::new(),
            task_progress: None,
            session_start_time: std::time::Instant::now(),
            side_panel_auto_shown: false,
            board_agent_id: None,
            editor_command: "nano".to_string(),
            pending_editor_open: None,
            billing_info: None,
        }
    }
}

pub struct ConfigurationState {
    pub secret_manager: SecretManager,
    pub latest_version: Option<String>,
    pub is_git_repo: bool,
    pub auto_approve_manager: AutoApproveManager,
    pub allowed_tools: Option<Vec<String>>,
    pub model: Model,
    pub auth_display_info: (Option<String>, Option<String>, Option<String>),
    pub init_prompt_content: Option<String>,
}

pub struct MessageInteractionState {
    pub show_message_action_popup: bool,
    pub message_action_popup_selected: usize,
    pub message_action_popup_position: Option<(u16, u16)>,
    pub message_action_target_message_id: Option<Uuid>,
    pub message_action_target_text: Option<String>,
    pub message_area_y: u16,
    pub message_area_x: u16,
    pub message_area_height: u16,
    pub hover_row: Option<u16>,
    pub collapsed_popup_area_y: u16,
    pub collapsed_popup_area_x: u16,
    pub collapsed_popup_area_height: u16,
    pub input_content_area: Option<ratatui::layout::Rect>,
}

impl Default for MessageInteractionState {
    fn default() -> Self {
        Self {
            show_message_action_popup: false,
            message_action_popup_selected: 0,
            message_action_popup_position: None,
            message_action_target_message_id: None,
            message_action_target_text: None,
            message_area_y: 0,
            message_area_x: 0,
            message_area_height: 0,
            hover_row: None,
            collapsed_popup_area_y: 0,
            collapsed_popup_area_x: 0,
            collapsed_popup_area_height: 0,
            input_content_area: None,
        }
    }
}

/// Shell popup and shell-command execution UI state.
#[derive(Default)]
pub struct ShellPopupState {
    pub shell_popup_visible: bool,
    pub shell_popup_expanded: bool,
    pub shell_popup_scroll: usize,
    /// Flag to request a terminal clear and redraw (e.g., after shell popup closes)
    pub needs_terminal_clear: bool,
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
}

/// Tool-call streaming, retry, and cancellation lifecycle state.
#[derive(Default)]
pub struct ToolCallState {
    pub pending_bash_message_id: Option<Uuid>,
    pub streaming_tool_results: HashMap<Uuid, String>,
    pub streaming_tool_result_id: Option<Uuid>,
    pub completed_tool_calls: HashSet<Uuid>,
    pub is_streaming: bool,
    /// When true, cancellation has been requested (ESC pressed) but the final ToolResult
    /// hasn't arrived yet. Late StreamToolResult/StreamAssistantMessage events should be ignored.
    pub cancel_requested: bool,
    pub latest_tool_call: Option<ToolCall>,
    /// Stable message ID for the tool call streaming preview block
    pub tool_call_stream_preview_id: Option<Uuid>,
    pub retry_attempts: usize,
    pub max_retry_attempts: usize,
    pub last_user_message_for_retry: Option<String>,
    pub is_retrying: bool,
}

/// Dialog visibility, approval-bar interaction, and tool-approval selection state.
pub struct DialogApprovalState {
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
}

impl Default for DialogApprovalState {
    fn default() -> Self {
        Self {
            is_dialog_open: false,
            dialog_command: None,
            dialog_selected: 0,
            dialog_message_id: None,
            dialog_focused: false,
            approval_bar: ApprovalBar::new(),
            message_tool_calls: None,
            message_approved_tools: Vec::new(),
            message_rejected_tools: Vec::new(),
            toggle_approved_message: true,
            show_shortcuts: false,
        }
    }
}

#[derive(Default)]
pub struct SessionsState {
    pub sessions: Vec<SessionInfo>,
    pub session_selected: usize,
    pub account_info: String,
}

#[derive(Default)]
pub struct SessionToolCallsState {
    pub session_tool_calls_queue: HashMap<String, ToolCallStatus>,
    pub tool_call_execution_order: Vec<String>,
    pub last_message_tool_calls: Vec<ToolCall>,
}

#[derive(Default)]
pub struct ProfileSwitcherState {
    pub show_profile_switcher: bool,
    pub available_profiles: Vec<String>,
    pub profile_switcher_selected: usize,
    pub current_profile_name: String,
    pub profile_switching_in_progress: bool,
    pub profile_switch_status_message: Option<String>,
}

#[derive(Default)]
pub struct RulebookSwitcherState {
    pub show_rulebook_switcher: bool,
    pub available_rulebooks: Vec<ListRuleBook>,
    pub selected_rulebooks: HashSet<String>,
    pub rulebook_switcher_selected: usize,
    pub rulebook_search_input: String,
    pub filtered_rulebooks: Vec<ListRuleBook>,
    pub rulebook_config: Option<crate::RulebookConfig>,
}

#[derive(Default)]
pub struct ModelSwitcherState {
    pub show_model_switcher: bool,
    pub available_models: Vec<Model>,
    pub model_switcher_selected: usize,
    pub current_model: Option<Model>,
    pub model_switcher_mode: ModelSwitcherMode,
    pub model_switcher_search: String,
    pub recent_models: Vec<String>,
}

#[derive(Default)]
pub struct CommandPaletteState {
    pub show_command_palette: bool,
    pub command_palette_selected: usize,
    pub command_palette_scroll: usize,
    pub command_palette_search: String,
}

#[derive(Default)]
pub struct ShortcutsPanelState {
    pub show_shortcuts_popup: bool,
    pub shortcuts_scroll: usize,
    pub shortcuts_popup_mode: ShortcutsPopupMode,
}

#[derive(Default)]
pub struct FileChangesPopupState {
    pub show_file_changes_popup: bool,
    pub file_changes_selected: usize,
    pub file_changes_scroll: usize,
    pub file_changes_search: String,
}

pub struct UsageTrackingState {
    pub current_message_usage: LLMTokenUsage,
    pub total_session_usage: LLMTokenUsage,
    pub context_usage_percent: u64,
}

#[derive(Default)]
pub struct PlanModeState {
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
}

#[derive(Default)]
pub struct PlanReviewState {
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
    pub plan_review_resolved_anchors:
        Vec<(String, crate::services::plan_comments::ResolvedAnchor)>,
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
}

#[derive(Default)]
pub struct AskUserState {
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
    /// Multi-select toggle state: question label -> list of currently selected option values
    pub ask_user_multi_selections: HashMap<String, Vec<String>>,
}

impl Default for UsageTrackingState {
    fn default() -> Self {
        Self {
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
        }
    }
}

impl Default for LoadingState {
    fn default() -> Self {
        Self {
            is_loading: false,
            loading_type: LoadingType::Llm,
            spinner_frame: 0,
            loading_manager: LoadingStateManager::new(),
        }
    }
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

/// Mode for the unified shortcuts/commands/sessions popup
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ShortcutsPopupMode {
    #[default]
    Commands,
    Shortcuts,
    Sessions,
}

/// Mode for the model switcher popup filter tabs
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ModelSwitcherMode {
    #[default]
    All, // Show all models grouped by provider
    Reasoning, // Show only models with reasoning support
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

#[cfg(test)]
mod tests {
    use super::*;
    use stakpak_shared::models::integrations::openai::{
        FunctionCall, ToolCall, ToolCallResultStatus,
    };

    fn tool_result(id: &str) -> ToolCallResult {
        ToolCallResult {
            call: ToolCall {
                id: id.to_string(),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: "run_command".to_string(),
                    arguments: "{}".to_string(),
                },
                metadata: None,
            },
            result: format!("result-{id}"),
            status: ToolCallResultStatus::Success,
        }
    }

    #[test]
    fn pending_user_message_merge_combines_all_parts() {
        let mut first = PendingUserMessage::new(
            "first".to_string(),
            Some(vec![tool_result("t1")]),
            vec![ContentPart {
                r#type: "text".to_string(),
                text: Some("img-1".to_string()),
                image_url: None,
            }],
            "first".to_string(),
        );

        let second = PendingUserMessage::new(
            "second".to_string(),
            Some(vec![tool_result("t2")]),
            vec![ContentPart {
                r#type: "text".to_string(),
                text: Some("img-2".to_string()),
                image_url: None,
            }],
            "second".to_string(),
        );

        first.merge_from(second);

        assert_eq!(first.final_input, "first\n\nsecond");
        assert_eq!(first.user_message_text, "first\n\nsecond");
        assert_eq!(first.image_parts.len(), 2);
        assert_eq!(
            first
                .shell_tool_calls
                .as_ref()
                .map(std::vec::Vec::len)
                .unwrap_or_default(),
            2
        );
    }

    #[test]
    fn pending_user_message_merge_skips_empty_text_with_no_extra_separator() {
        let mut first = PendingUserMessage::new("".to_string(), None, Vec::new(), "".to_string());

        let second = PendingUserMessage::new(
            "second".to_string(),
            None,
            vec![ContentPart {
                r#type: "text".to_string(),
                text: Some("img-2".to_string()),
                image_url: None,
            }],
            "second".to_string(),
        );

        first.merge_from(second);

        assert_eq!(first.final_input, "second");
        assert_eq!(first.user_message_text, "second");
        assert_eq!(first.image_parts.len(), 1);
    }

    #[test]
    fn pending_user_message_merge_adopts_incoming_tool_calls_when_initially_none() {
        let mut first =
            PendingUserMessage::new("first".to_string(), None, Vec::new(), "first".to_string());

        let second = PendingUserMessage::new(
            "second".to_string(),
            Some(vec![tool_result("t2")]),
            Vec::new(),
            "second".to_string(),
        );

        first.merge_from(second);

        assert_eq!(
            first
                .shell_tool_calls
                .as_ref()
                .map(std::vec::Vec::len)
                .unwrap_or_default(),
            1
        );
    }
}
