mod events;
mod types;

pub use events::{InputEvent, OutputEvent};
use stakai::Model;
pub use types::*;

use crate::services::auto_approve::AutoApproveManager;
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
use crate::services::textarea::{TextArea, TextAreaState};
use crate::services::toast::Toast;
use stakpak_shared::secret_manager::SecretManager;
use stakpak_shared::task_manager::TaskManagerHandle;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct AppState {
    pub input_state: InputState,
    pub messages_scrolling_state: MessagesScrollingState,
    pub loading_state: LoadingState,
    pub shell_popup_state: ShellPopupState,
    pub tool_call_state: ToolCallState,
    pub dialog_approval_state: DialogApprovalState,
    pub sessions_state: SessionsState,
    pub session_tool_calls_state: SessionToolCallsState,
    pub profile_switcher_state: ProfileSwitcherState,
    pub rulebook_switcher_state: RulebookSwitcherState,
    pub model_switcher_state: ModelSwitcherState,
    pub command_palette_state: CommandPaletteState,
    pub shortcuts_panel_state: ShortcutsPanelState,
    pub file_changes_popup_state: FileChangesPopupState,
    pub usage_tracking_state: UsageTrackingState,
    pub configuration_state: ConfigurationState,
    pub quit_intent_state: QuitIntentState,
    pub terminal_ui_state: TerminalUiState,
    pub shell_runtime_state: ShellRuntimeState,
    pub shell_session_state: ShellSessionState,
    pub banner_state: BannerState,
    pub toast: Option<Toast>,
    pub message_interaction_state: MessageInteractionState,
    pub side_panel_state: SidePanelState,
    pub user_message_queue_state: UserMessageQueueState,
    pub message_revert_state: MessageRevertState,
    pub plan_mode_state: PlanModeState,
    pub plan_review_state: PlanReviewState,
    pub ask_user_state: AskUserState,
    pub tool_approval_popup_state: AutoApprovePopupState,
    pub approval_settings_persistence_state: ApprovalSettingsPersistenceModal,
    /// Handle to the TaskManager for querying background task status (e.g., running subagents)
    pub task_manager_handle: Option<Arc<TaskManagerHandle>>,
    /// Cached count of running/pending background tasks (refreshed on Ctrl+C press)
    pub running_background_tasks: usize,
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
    /// Handle to the TaskManager for querying background task status
    pub task_manager_handle: Option<Arc<TaskManagerHandle>>,
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
            task_manager_handle,
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
            messages_scrolling_state: MessagesScrollingState::default(),
            dialog_approval_state: DialogApprovalState::default(),
            sessions_state: SessionsState::default(),
            tool_call_state: ToolCallState {
                max_retry_attempts: 3,
                ..Default::default()
            },
            session_tool_calls_state: SessionToolCallsState::default(),
            shell_popup_state: ShellPopupState {
                cursor_visible: true,
                ..Default::default()
            },
            quit_intent_state: QuitIntentState::default(),
            terminal_ui_state: TerminalUiState::default(),
            shell_runtime_state: ShellRuntimeState::default(),
            shell_session_state: ShellSessionState::default(),

            toast: None,
            banner_state: BannerState::default(),

            // Message interaction initialization
            message_interaction_state: MessageInteractionState::default(),

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

            // tool approval popup initialization
            tool_approval_popup_state: AutoApprovePopupState::default(),

            // Policy persistence modal initialization
            approval_settings_persistence_state: ApprovalSettingsPersistenceModal::default(),

            // Usage tracking
            usage_tracking_state: UsageTrackingState::default(),

            // Configuration state
            configuration_state: ConfigurationState {
                secret_manager: SecretManager::new(redact_secrets, privacy_mode),
                latest_version: latest_version.clone(),
                is_git_repo,
                auto_approve_manager: AutoApproveManager::new(auto_approve_tools, input_tx),
                allowed_tools: allowed_tools.cloned(),
                model,
                auth_display_info,
                init_prompt_content,
            },

            // Side panel initialization
            side_panel_state: SidePanelState {
                board_agent_id,
                editor_command: crate::services::editor::detect_editor(editor_command)
                    .unwrap_or_else(|| "nano".to_string()),
                ..Default::default()
            },
            user_message_queue_state: UserMessageQueueState::default(),
            message_revert_state: MessageRevertState::default(),
            task_manager_handle,
            running_background_tasks: 0,

            // Plan mode/review initialization
            plan_mode_state: PlanModeState::default(),
            plan_review_state: PlanReviewState::default(),
            // Ask User inline block initialization
            ask_user_state: AskUserState {
                is_focused: true,
                ..Default::default()
            },
        }
    }

    pub fn update_session_empty_status(&mut self) {
        let session_empty = !self.messages_scrolling_state.has_user_messages
            && self.input_state.text_area.text().is_empty();
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
        if !self.plan_mode_state.is_active {
            return None;
        }

        let session_dir = std::path::Path::new(".stakpak/session");
        let path = plan::plan_file_path(session_dir);

        let Ok(content) = std::fs::read_to_string(&path) else {
            // File doesn't exist (yet) — clear stale cache
            if self.plan_mode_state.metadata.is_some() {
                self.plan_mode_state.metadata = None;
                self.plan_mode_state.content_hash = None;
            }
            return None;
        };

        let new_hash = plan::compute_plan_hash(&content);

        // Skip re-parse if content unchanged
        if self.plan_mode_state.content_hash.as_deref() == Some(&new_hash) {
            return None;
        }

        self.plan_mode_state.content_hash = Some(new_hash);
        let new_meta = plan::parse_plan_front_matter(&content);
        self.plan_mode_state.metadata = new_meta.clone();

        // Detect status transitions
        if let Some(ref meta) = new_meta {
            let new_status = meta.status;
            let old_status = self.plan_mode_state.previous_status;

            if old_status != Some(new_status) {
                self.plan_mode_state.previous_status = Some(new_status);
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
        self.profile_switcher_state.switching_in_progress
    }

    pub fn run_shell_command(&mut self, command: String, input_tx: &mpsc::Sender<InputEvent>) {
        let (shell_tx, mut shell_rx) = mpsc::channel::<ShellEvent>(100);
        self.messages_scrolling_state
            .messages
            .push(Message::plain_text("SPACING_MARKER"));
        push_styled_message(
            self,
            &command,
            ThemeColors::text(),
            SHELL_PROMPT_PREFIX,
            ThemeColors::magenta(),
        );
        self.messages_scrolling_state
            .messages
            .push(Message::plain_text("SPACING_MARKER"));
        let rows = if self.terminal_ui_state.terminal_size.height > 0 {
            self.terminal_ui_state.terminal_size.height
        } else {
            24
        };
        let cols = if self.terminal_ui_state.terminal_size.width > 0 {
            self.terminal_ui_state.terminal_size.width
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
        self.shell_runtime_state.screen = vt100::Parser::new(rows, cols, 0);
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
                self.input_state.file_search.filtered_files =
                    self.input_state.filtered_files.clone();
                self.input_state.file_search.is_file_mode =
                    !self.input_state.filtered_files.is_empty();
                self.input_state.file_search.trigger_char =
                    if !self.input_state.filtered_files.is_empty() {
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
        if !self.side_panel_state.auto_shown && !self.side_panel_state.is_shown {
            self.side_panel_state.is_shown = true;
            self.side_panel_state.auto_shown = true;
        }
    }
}
