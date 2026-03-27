mod events;
mod types;

pub use events::{InputEvent, OutputEvent};
use stakai::Model;
pub use types::*;

use crate::services::auto_approve::AutoApproveManager;
use crate::services::banner::BannerMessage;
use crate::services::board_tasks::TaskProgress;
use crate::services::changeset::{Changeset, SidePanelSection, TodoItem};
use crate::services::detect_term::ThemeColors;
use crate::services::file_search::{FileSearch, file_search_worker, find_at_trigger};
#[cfg(unix)]
use crate::services::helper_block::push_error_message;
use crate::services::helper_block::push_styled_message;
use crate::services::message::Message;
#[cfg(not(unix))]
use crate::services::shell_mode::run_background_shell_command;
#[cfg(unix)]
use crate::services::shell_mode::run_pty_command;
use crate::services::shell_mode::{SHELL_PROMPT_PREFIX, ShellEvent};
use crate::services::text_selection::SelectionState;
use crate::services::textarea::{TextArea, TextAreaState};
use crate::services::toast::Toast;
use ratatui::layout::{Rect, Size};
use ratatui::text::Line;
use stakpak_shared::secret_manager::SecretManager;
use std::collections::{HashMap, VecDeque};
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct AppState {
    // ========== Input State (TextArea + helpers + file search) ==========
    pub input_state: InputState,

    // ========== Messages & Scrolling State ==========
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

    // ========== Loading State ==========
    pub loading_state: LoadingState,

    // ========== Shell Popup State ==========
    pub shell_popup_state: ShellPopupState,

    // ========== Tool Call State ==========
    pub tool_call_state: ToolCallState,

    // ========== Dialog & Approval State ==========
    pub dialog_approval_state: DialogApprovalState,

    // ========== Sessions State ==========
    pub sessions_state: SessionsState,

    // ========== Session Tool Calls Queue ==========
    pub session_tool_calls_state: SessionToolCallsState,

    // ========== Profile Switcher State ==========
    pub profile_switcher_state: ProfileSwitcherState,

    // ========== Rulebook Switcher State ==========
    pub rulebook_switcher_state: RulebookSwitcherState,

    // ========== Model Switcher State ==========
    pub model_switcher_state: ModelSwitcherState,

    // ========== Command Palette State ==========
    pub command_palette_state: CommandPaletteState,

    // ========== Shortcuts Popup State ==========
    pub shortcuts_panel_state: ShortcutsPanelState,

    // ========== File Changes Popup State ==========
    pub file_changes_popup_state: FileChangesPopupState,

    // ========== Usage Tracking State ==========
    pub usage_tracking_state: UsageTrackingState,

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

    // ========== Text Selection State ==========
    pub selection: SelectionState,
    pub toast: Option<Toast>,
    pub banner_message: Option<BannerMessage>,
    /// Stores the banner area rect for mouse click detection
    pub banner_area: Option<Rect>,
    /// Clickable command regions within the banner: (command_text, bounding_rect)
    pub banner_click_regions: Vec<(String, Rect)>,
    /// Auto-scroll direction during drag selection: -1 (up), 0 (none), 1 (down)
    pub selection_auto_scroll: i32,

    // ========== Message Action Popup State ==========
    pub show_message_action_popup: bool,
    pub message_action_popup_selected: usize,
    pub message_action_popup_position: Option<(u16, u16)>, // (x, y) position for popup
    pub message_action_target_message_id: Option<Uuid>,    // The user message being acted on
    pub message_action_target_text: Option<String>,        // The text of the target message
    pub message_area_y: u16, // Y offset of message area for click detection
    pub message_area_x: u16, // X offset of padded message area for column mapping
    pub message_area_height: u16, // Height of message area (set during render for accurate event handling)
    pub hover_row: Option<u16>,   // Current mouse hover row for debugging

    // ========== Collapsed Popup Geometry (for text selection in fullscreen popup) ==========
    pub collapsed_popup_area_y: u16, // Y offset of popup content area
    pub collapsed_popup_area_x: u16, // X offset of popup content area
    pub collapsed_popup_area_height: u16, // Height of popup content area

    // ========== Input Area State ==========
    /// Stores the input area content rect for mouse click positioning
    pub input_content_area: Option<ratatui::layout::Rect>,

    // ========== Side Panel State ==========
    pub show_side_panel: bool,
    pub side_panel_focus: SidePanelSection,
    pub side_panel_section_collapsed: std::collections::HashMap<SidePanelSection, bool>,
    /// Stores the screen area for each side panel section to handle mouse clicks
    pub side_panel_areas: HashMap<SidePanelSection, ratatui::layout::Rect>,
    /// Current session ID for backup paths
    pub session_id: String,
    /// Timestamp when session ID was last copied (for "Copied!" feedback)
    pub session_id_copied_at: Option<std::time::Instant>,
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

    // ========== Message Revert State ==========
    /// Counter for user messages (1-indexed, incremented when user sends a message)
    /// Used to track which user message triggered file edits for selective revert
    pub user_message_count: usize,
    /// Pending revert: truncate backend messages to this user message index when next message is sent
    /// Set when user selects "Revert" action, consumed when sending the next user message
    pub pending_revert_index: Option<usize>,

    // ========== Plan Mode State ==========
    pub plan_mode_state: PlanModeState,

    // ========== Plan Review State ==========
    pub plan_review_state: PlanReviewState,

    // ========== Ask User Inline Block State ==========
    pub ask_user_state: AskUserState,
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
    /// Recently used model IDs (most recent first)
    pub recent_models: Vec<String>,
}

impl AppState {
    pub fn get_helper_commands() -> Vec<HelperCommand> {
        // Built-in commands from the unified command system
        let mut helpers = crate::services::commands::commands_to_helper_commands();

        // Predefined commands shipped with the binary (from libs/api/src/commands/*.md)
        // Skip any that clash with built-in command names
        let builtin_names: std::collections::HashSet<String> =
            helpers.iter().map(|h| h.command.clone()).collect();
        for (name, description, prompt_content) in stakpak_api::commands::load_predefined_commands()
        {
            let command = format!("/{name}");
            if builtin_names.contains(&command) {
                continue;
            }
            helpers.push(HelperCommand {
                command,
                description,
                source: CommandSource::BuiltInWithPrompt { prompt_content },
            });
        }

        // Load custom commands from ~/.stakpak/commands/ and .stakpak/commands/
        let custom = crate::services::custom_commands::load_custom_commands();

        // Merge: skip custom commands whose names clash with built-in or predefined commands
        let builtin_names: std::collections::HashSet<String> =
            helpers.iter().map(|h| h.command.clone()).collect();
        helpers.extend(
            custom
                .into_iter()
                .filter(|c| !builtin_names.contains(&c.command)),
        );

        helpers
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
            recent_models,
        } = options;

        let helpers = Self::get_helper_commands();
        let (file_search_tx, result_rx) = Self::init_file_search_channels(&helpers);

        AppState {
            input_state: InputState {
                text_area: TextArea::new(),
                text_area_state: TextAreaState::default(),
                cursor_visible: true,
                helpers,
                show_helper_dropdown: false,
                helper_selected: 0,
                helper_scroll: 0,
                filtered_helpers: Vec::new(),
                filtered_files: Vec::new(),
                file_search: FileSearch::default(),
                file_search_tx: Some(file_search_tx),
                file_search_rx: Some(result_rx),
                is_pasting: false,
                pasted_long_text: None,
                pasted_placeholder: None,
                pending_pastes: Vec::new(),
                attached_images: Vec::new(),
                pending_path_start: None,
                interactive_commands: crate::constants::INTERACTIVE_COMMANDS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            },
            loading_state: LoadingState::default(),
            messages: Vec::new(), // Will be populated after state is created
            scroll: 0,
            scroll_to_bottom: false,
            scroll_to_last_message_start: false,
            stay_at_bottom: true,
            block_stay_at_bottom_frames: 0,
            scroll_lines_from_end: None,
            content_changed_while_scrolled_up: false,
            dialog_approval_state: DialogApprovalState::default(),
            sessions_state: SessionsState::default(),
            tool_call_state: ToolCallState {
                max_retry_attempts: 3,
                ..Default::default()
            },
            session_tool_calls_state: SessionToolCallsState::default(),
            shell_popup_state: ShellPopupState {
                shell_cursor_visible: true,
                shell_mode_input: String::new(),
                ..Default::default()
            },
            secret_manager: SecretManager::new(redact_secrets, privacy_mode),
            latest_version: latest_version.clone(),
            ctrl_c_pressed_once: false,
            ctrl_c_timer: None,
            auto_approve_manager: AutoApproveManager::new(auto_approve_tools, input_tx),
            allowed_tools: allowed_tools.cloned(),
            // Default to messages view focused
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
            line_to_message_map: Vec::new(),
            mouse_capture_enabled: false,
            has_user_messages: false,
            terminal_size: Size {
                width: 0,
                height: 0,
            },
            shell_screen: vt100::Parser::new(24, 80, 1000),
            shell_scroll: 0,
            shell_history_lines: Vec::new(),
            interactive_shell_message_id: None,
            shell_interaction_occurred: false,

            // Text selection initialization
            selection: SelectionState::default(),
            toast: None,
            banner_message: None,
            banner_area: None,
            banner_click_regions: Vec::new(),
            selection_auto_scroll: 0,
            input_content_area: None,

            // Message action popup initialization
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

            // Profile switcher initialization
            profile_switcher_state: ProfileSwitcherState {
                current_profile_name: "default".to_string(),
                ..Default::default()
            },

            // Shortcuts popup initialization
            shortcuts_panel_state: ShortcutsPanelState::default(),
            // Rulebook switcher initialization
            rulebook_switcher_state: RulebookSwitcherState::default(),

            // Model switcher initialization
            model_switcher_state: ModelSwitcherState {
                recent_models,
                ..Default::default()
            },
            // Command palette initialization
            command_palette_state: CommandPaletteState::default(),

            // File changes popup initialization
            file_changes_popup_state: FileChangesPopupState::default(),

            // Usage tracking
            usage_tracking_state: UsageTrackingState::default(),
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
            session_id_copied_at: None,
            board_agent_id,
            editor_command: crate::services::editor::detect_editor(editor_command)
                .unwrap_or_else(|| "nano".to_string()),
            pending_editor_open: None,
            pending_user_messages: VecDeque::new(),
            billing_info: None,
            auth_display_info,

            // Plan mode/review initialization
            plan_mode_state: PlanModeState::default(),
            plan_review_state: PlanReviewState::default(),
            subagent_pause_info: HashMap::new(),
            init_prompt_content,

            // Message revert state initialization
            user_message_count: 0,
            pending_revert_index: None,

            // Ask User inline block initialization
            ask_user_state: AskUserState {
                ask_user_focused: true,
                ..Default::default()
            },
        }
    }

    pub fn update_session_empty_status(&mut self) {
        let session_empty = !self.has_user_messages && self.input_state.text_area.text().is_empty();
        self.input_state.text_area.set_session_empty(session_empty);
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
        if !self.plan_mode_state.plan_mode_active {
            return None;
        }

        let session_dir = std::path::Path::new(".stakpak/session");
        let path = plan::plan_file_path(session_dir);

        let Ok(content) = std::fs::read_to_string(&path) else {
            // File doesn't exist (yet) — clear stale cache
            if self.plan_mode_state.plan_metadata.is_some() {
                self.plan_mode_state.plan_metadata = None;
                self.plan_mode_state.plan_content_hash = None;
            }
            return None;
        };

        let new_hash = plan::compute_plan_hash(&content);

        // Skip re-parse if content unchanged
        if self.plan_mode_state.plan_content_hash.as_deref() == Some(&new_hash) {
            return None;
        }

        self.plan_mode_state.plan_content_hash = Some(new_hash);
        let new_meta = plan::parse_plan_front_matter(&content);
        self.plan_mode_state.plan_metadata = new_meta.clone();

        // Detect status transitions
        if let Some(ref meta) = new_meta {
            let new_status = meta.status;
            let old_status = self.plan_mode_state.plan_previous_status;

            if old_status != Some(new_status) {
                self.plan_mode_state.plan_previous_status = Some(new_status);
                return Some((old_status, new_status));
            }
        }

        None
    }

// Convenience methods for accessing input and cursor (using input_state)
    pub fn input(&self) -> &str {
        self.input_state.text_area.text()
    }

    pub fn cursor_position(&self) -> usize {
        self.input_state.text_area.cursor()
    }

    pub fn set_input(&mut self, input: &str) {
        self.input_state.text_area.set_text(input);
    }

    pub fn set_cursor_position(&mut self, pos: usize) {
        self.input_state.text_area.set_cursor(pos);
    }

    pub fn insert_char(&mut self, c: char) {
        self.input_state.text_area.insert_str(&c.to_string());
    }

    pub fn insert_str(&mut self, s: &str) {
        self.input_state.text_area.insert_str(s);
    }

    pub fn clear_input(&mut self) {
        self.input_state.text_area.set_text("");
    }

    /// Check if user input should be blocked (during profile switch)
    pub fn is_input_blocked(&self) -> bool {
        self.profile_switcher_state.profile_switching_in_progress
    }

    pub fn run_shell_command(&mut self, command: String, input_tx: &mpsc::Sender<InputEvent>) {
        let (shell_tx, mut shell_rx) = mpsc::channel::<ShellEvent>(100);
        self.messages.push(Message::plain_text("SPACING_MARKER"));
        push_styled_message(
            self,
            &command,
            ThemeColors::text(),
            SHELL_PROMPT_PREFIX,
            ThemeColors::magenta(),
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

        self.shell_popup_state.active_shell_command = Some(shell_cmd.clone());
        self.shell_popup_state.active_shell_command_output = Some(String::new());
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

    // --- Poll file_search results and update state (for @ file completion only) ---
    pub fn poll_file_search_results(&mut self) {
        if let Some(rx) = &mut self.input_state.file_search_rx {
            while let Ok(result) = rx.try_recv() {
                // Get input text before any mutable operations
                let input_text = self.input_state.text_area.text().to_string();

                let filtered_files = result.filtered_files.clone();
                self.input_state.filtered_files = filtered_files;
                self.input_state.file_search.filtered_files = self.input_state.filtered_files.clone();
                self.input_state.file_search.is_file_mode = !self.input_state.filtered_files.is_empty();
                self.input_state.file_search.trigger_char = if !self.input_state.filtered_files.is_empty() {
                    Some('@')
                } else {
                    None
                };

                // NOTE: Slash command filtering (filtered_helpers) is now done synchronously
                // in handle_input_changed / handle_input_backspace to avoid race conditions
                // that caused buggy behavior in external terminals (iTerm2, Warp, etc.).
                // The async worker still computes filtered_helpers but we ignore it here.

                // Show dropdown for @ file triggers (slash command dropdown is managed synchronously)
                let has_at_trigger =
                    find_at_trigger(&result.input, result.cursor_position).is_some();
                if has_at_trigger && !self.shell_popup_state.waiting_for_shell_input {
                    self.input_state.show_helper_dropdown = true;
                }
                // If we have file results, reset selection if out of bounds
                if !self.input_state.filtered_files.is_empty()
                    && self.input_state.helper_selected >= self.input_state.filtered_files.len()
                {
                    self.input_state.helper_selected = 0;
                }

                // Don't overwrite show_helper_dropdown for slash commands —
                // that state is already set synchronously by the input handlers.
                // Only hide if input is completely empty (safety net).
                if input_text.is_empty() {
                    self.input_state.show_helper_dropdown = false;
                }
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
