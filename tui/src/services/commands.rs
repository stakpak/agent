//! Unified Command System
//!
//! This module provides a single source of truth for all commands in the TUI.
//! Commands can be executed from:
//! - Direct slash command input (e.g., typing "/help")
//! - Helper dropdown selection
//! - Command palette
//!
//! All commands are defined here and executed through a unified executor.

use crate::app::{AppState, HelperCommand};
use crate::constants::SUMMARIZE_PROMPT_BASE;
use crate::services::auto_approve::AutoApprovePolicy;
use crate::services::helper_block::{
    push_clear_message, push_error_message, push_help_message, push_issue_message,
    push_memorize_message, push_status_message, push_styled_message, push_support_message,
    push_usage_message, render_system_message, welcome_messages,
};
use crate::services::message::{Message, MessageContent};
use crate::{InputEvent, OutputEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
};

use stakpak_shared::models::llm::LLMTokenUsage;
use tokio::sync::mpsc::Sender;

/// Command identifier - the slash command string (e.g., "/help", "/clear")
pub type CommandId = &'static str;

/// Command metadata for display (used by command palette)
#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub shortcut: String,
    pub action: CommandAction,
}

/// Command action enum for command palette
#[derive(Debug, Clone)]
pub enum CommandAction {
    OpenProfileSwitcher,
    OpenRulebookSwitcher,
    OpenSessions,
    OpenShortcuts,
    OpenShellMode,
    ResumeSession,
    ShowStatus,
    MemorizeConversation,
    SubmitIssue,
    GetSupport,
    NewSession,
    ShowUsage,
    SwitchModel,
    PlanMode,
}

impl CommandAction {
    /// Convert CommandAction to command ID for unified execution
    pub fn to_command_id(&self) -> Option<&'static str> {
        match self {
            CommandAction::OpenSessions => Some("/sessions"),
            CommandAction::ResumeSession => Some("/resume"),
            CommandAction::ShowStatus => Some("/status"),
            CommandAction::MemorizeConversation => Some("/memorize"),
            CommandAction::SubmitIssue => Some("/issue"),
            CommandAction::GetSupport => Some("/support"),
            CommandAction::NewSession => Some("/new"),
            CommandAction::ShowUsage => Some("/usage"),
            CommandAction::SwitchModel => Some("/model"),
            CommandAction::PlanMode => Some("/plan"),
            // These don't have slash commands, handled separately
            CommandAction::OpenProfileSwitcher
            | CommandAction::OpenRulebookSwitcher
            | CommandAction::OpenShortcuts
            | CommandAction::OpenShellMode => None,
        }
    }
}

impl Command {
    pub fn new(name: &str, description: &str, shortcut: &str, action: CommandAction) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            shortcut: shortcut.to_string(),
            action,
        }
    }
}

/// Command execution context
pub struct CommandContext<'a> {
    pub state: &'a mut AppState,
    pub input_tx: &'a Sender<InputEvent>,
    pub output_tx: &'a Sender<OutputEvent>,
}

// ========== Command Registry ==========

/// Get all commands for command palette
pub fn get_all_commands() -> Vec<Command> {
    vec![
        Command::new(
            "Profiles",
            "Change active profile",
            "Ctrl+F",
            CommandAction::OpenProfileSwitcher,
        ),
        Command::new(
            "Rulebooks",
            "Select and switch rulebooks",
            "Ctrl+K",
            CommandAction::OpenRulebookSwitcher,
        ),
        Command::new(
            "Context",
            "Show context utilization popup",
            "Ctrl+G",
            CommandAction::ShowUsage, // reuse for now; actual action handled upstream
        ),
        Command::new(
            "Shortcuts",
            "Show all keyboard shortcuts",
            "Ctrl+S",
            CommandAction::OpenShortcuts,
        ),
        Command::new(
            "Shell Mode",
            "Open interactive shell",
            "$",
            CommandAction::OpenShellMode,
        ),
        Command::new(
            "New Session",
            "Start a new session",
            "/new",
            CommandAction::NewSession,
        ),
        Command::new(
            "Sessions",
            "List and manage sessions",
            "/sessions",
            CommandAction::OpenSessions,
        ),
        Command::new(
            "Resume",
            "Resume last session",
            "/resume",
            CommandAction::ResumeSession,
        ),
        Command::new(
            "Usage",
            "Show token usage for this session",
            "/usage",
            CommandAction::ShowUsage,
        ),
        Command::new(
            "Status",
            "Show account information",
            "/status",
            CommandAction::ShowStatus,
        ),
        Command::new(
            "Memorize",
            "Save conversation to memory",
            "/memorize",
            CommandAction::MemorizeConversation,
        ),
        Command::new(
            "Submit Issue",
            "Submit issue on GitHub repo",
            "/issue",
            CommandAction::SubmitIssue,
        ),
        Command::new(
            "Get Help",
            "Go to Discord channel",
            "/support",
            CommandAction::GetSupport,
        ),
        Command::new(
            "Switch Model",
            "Switch to a different AI model",
            "Ctrl+M",
            CommandAction::SwitchModel,
        ),
        Command::new(
            "Plan Mode",
            "Enter plan mode — research and draft before executing",
            "/plan",
            CommandAction::PlanMode,
        ),
    ]
}

/// Convert Command to HelperCommand for backward compatibility
pub fn commands_to_helper_commands() -> Vec<HelperCommand> {
    vec![
        HelperCommand {
            command: "/help",
            description: "Show help information and available commands",
        },
        HelperCommand {
            command: "/model",
            description: "Open model switcher to change AI model",
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
            command: "/summarize",
            description: "Summarize the session into summary.md for later resume",
        },
        HelperCommand {
            command: "/usage",
            description: "Show token usage for this session",
        },
        HelperCommand {
            command: "/issue",
            description: "Report an issue or bug",
        },
        HelperCommand {
            command: "/editor",
            description: "Open file in external editor: /editor <path>",
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
        HelperCommand {
            command: "/plan",
            description: "Enter plan mode: /plan [optional prompt]",
        },
        HelperCommand {
            command: "/init",
            description: "Analyze your infrastructure setup",
        },
    ]
}

/// Filter commands based on search query
pub fn filter_commands(query: &str) -> Vec<Command> {
    if query.is_empty() {
        return get_all_commands();
    }

    let query_lower = query.to_lowercase();
    get_all_commands()
        .into_iter()
        .filter(|cmd| {
            cmd.name.to_lowercase().contains(&query_lower)
                || cmd.description.to_lowercase().contains(&query_lower)
        })
        .collect()
}

// ========== Command Execution ==========

/// Execute a command by its ID
pub fn execute_command(command_id: CommandId, ctx: CommandContext) -> Result<(), String> {
    match command_id {
        "/help" => {
            push_help_message(ctx.state);
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            Ok(())
        }
        "/model" => {
            // Show model switcher popup
            ctx.state.show_model_switcher = true;
            ctx.state.model_switcher_selected = 0;
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            // Request available models from the output handler
            let _ = ctx.output_tx.try_send(OutputEvent::RequestAvailableModels);
            Ok(())
        }
        "/clear" => {
            push_clear_message(ctx.state);
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            Ok(())
        }
        "/status" => {
            push_status_message(ctx.state);
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            Ok(())
        }
        "/sessions" => {
            let _ = ctx.output_tx.try_send(OutputEvent::ListSessions);
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            Ok(())
        }
        "/resume" => {
            resume_session(ctx.state, ctx.output_tx);
            Ok(())
        }
        "/new" => {
            new_session(ctx.state, ctx.output_tx);
            Ok(())
        }
        "/memorize" => {
            push_memorize_message(ctx.state);
            let _ = ctx.output_tx.try_send(OutputEvent::Memorize);
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            Ok(())
        }
        "/summarize" => {
            let prompt = build_summarize_prompt(ctx.state);
            ctx.state.messages.push(Message::info("".to_string(), None));
            ctx.state.messages.push(Message::info(
                "Requesting session summary (summary.md)...",
                Some(Style::default().fg(Color::Cyan)),
            ));
            let _ = ctx.output_tx.try_send(OutputEvent::UserMessage(
                prompt.clone(),
                ctx.state.shell_tool_calls.clone(),
                Vec::new(), // No image parts for command
                None,       // No revert index
            ));
            ctx.state.shell_tool_calls = None;
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            Ok(())
        }
        "/usage" => {
            push_usage_message(ctx.state);
            let _ = ctx.output_tx.try_send(OutputEvent::RequestTotalUsage);
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            Ok(())
        }
        "/issue" => {
            push_issue_message(ctx.state);
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            Ok(())
        }
        "/support" => {
            push_support_message(ctx.state);
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            Ok(())
        }
        "/quit" => {
            ctx.state.show_helper_dropdown = false;
            ctx.state.text_area.set_text("");
            let _ = ctx.input_tx.try_send(InputEvent::Quit);
            Ok(())
        }
        "/toggle_auto_approve" => {
            // Special case: keep input for user to specify tool name
            let input = "/toggle_auto_approve ".to_string();
            ctx.state.text_area.set_text(&input);
            ctx.state.text_area.set_cursor(input.len());
            ctx.state.show_helper_dropdown = false;
            Ok(())
        }
        "/profiles" => {
            ctx.state.show_profile_switcher = true;
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            let _ = ctx.input_tx.try_send(InputEvent::ShowProfileSwitcher);
            Ok(())
        }
        "/list_approved_tools" => {
            list_auto_approved_tools(ctx.state);
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            Ok(())
        }
        "/mouse_capture" => {
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            let _ = ctx.input_tx.try_send(InputEvent::ToggleMouseCapture);
            Ok(())
        }
        "/editor" => {
            // Parse path argument if provided: /editor <path>
            let input = ctx.state.input().trim().to_string();
            let path_arg = if let Some(stripped) = input.strip_prefix("/editor ") {
                let path = stripped.trim();
                if path.is_empty() {
                    None
                } else {
                    Some(path.to_string())
                }
            } else {
                None
            };

            if let Some(path_str) = path_arg {
                // Resolve the path - handle both absolute and relative paths
                let path = std::path::Path::new(&path_str);
                let resolved_path = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    // Resolve relative path against current working directory
                    std::env::current_dir().unwrap_or_default().join(path)
                };

                if resolved_path.exists() {
                    ctx.state.pending_editor_open =
                        Some(resolved_path.to_string_lossy().to_string());
                    ctx.state.text_area.set_text("");
                    ctx.state.show_helper_dropdown = false;
                } else {
                    push_error_message(
                        ctx.state,
                        &format!("File not found: {}", resolved_path.display()),
                        None,
                    );
                    ctx.state.text_area.set_text("");
                    ctx.state.show_helper_dropdown = false;
                }
            } else {
                // No path provided - keep /editor with space so user can type path
                // This makes /editor a standalone feature, not tied to changeset
                let new_text = "/editor ";
                ctx.state.text_area.set_text(new_text);
                ctx.state.text_area.set_cursor(new_text.len());
                ctx.state.show_helper_dropdown = false;
            }
            Ok(())
        }
        "/shortcuts" => {
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            let _ = ctx.input_tx.try_send(InputEvent::ShowShortcuts);
            Ok(())
        }
        "/plan" => {
            // Already in plan mode? Show a message instead
            if ctx.state.plan_mode_active {
                crate::services::helper_block::push_styled_message(
                    ctx.state,
                    " Already in plan mode. Use ctrl+p to review the plan.",
                    ratatui::style::Color::Yellow,
                    "⚠ ",
                    ratatui::style::Color::Yellow,
                );
                ctx.state.text_area.set_text("");
                ctx.state.show_helper_dropdown = false;
                return Ok(());
            }

            // Parse optional inline prompt: "/plan deploy auth service" → Some("deploy auth service")
            let input = ctx.state.input().trim().to_string();
            let inline_prompt = input
                .strip_prefix("/plan")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());

            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;

            // Check for existing plan — show modal if one exists
            let session_dir = std::path::Path::new(".stakpak/session");
            if crate::services::plan::plan_file_exists(session_dir) {
                let meta = crate::services::plan::read_plan_file(session_dir).map(|(m, _)| m);
                ctx.state.existing_plan_prompt = Some(crate::app::ExistingPlanPrompt {
                    inline_prompt,
                    metadata: meta,
                });
            } else {
                let _ = ctx
                    .output_tx
                    .try_send(OutputEvent::PlanModeActivated(inline_prompt));
            }
            Ok(())
        }

        "/init" => {
            //  init prompt is always available (embedded at compile time)
            let prompt = match ctx.state.init_prompt_content.as_deref() {
                Some(p) if !p.trim().is_empty() => p.to_string(),
                _ => {
                    push_error_message(
                        ctx.state,
                        "Internal error: init prompt not available",
                        None,
                    );
                    ctx.state.text_area.set_text("");
                    ctx.state.show_helper_dropdown = false;
                    return Ok(());
                }
            };

            ctx.state.messages.push(Message::user(prompt.clone(), None));
            let _ = ctx.output_tx.try_send(OutputEvent::UserMessage(
                prompt,
                ctx.state.shell_tool_calls.clone(),
                Vec::new(), // No image parts for command
                None,       // No revert index
            ));
            ctx.state.shell_tool_calls = None;
            ctx.state.text_area.set_text("");
            ctx.state.show_helper_dropdown = false;
            crate::services::message::invalidate_message_lines_cache(ctx.state);
            Ok(())
        }

        _ => Err(format!("Unknown command: {}", command_id)),
    }
}

// ========== Helper Functions ==========

/// Terminate any active shell command before switching sessions
fn terminate_active_shell(state: &mut AppState) {
    if let Some(cmd) = &state.active_shell_command {
        // Kill the running command
        let _ = cmd.kill();
    }

    // Remove the shell message box if it exists
    if let Some(shell_msg_id) = state.interactive_shell_message_id {
        state.messages.retain(|m| m.id != shell_msg_id);
    }

    // Reset all shell-related state
    state.active_shell_command = None;
    state.active_shell_command_output = None;
    state.interactive_shell_message_id = None;
    state.show_shell_mode = false;
    state.shell_popup_visible = false;
    state.shell_popup_expanded = false;
    state.waiting_for_shell_input = false;
    state.shell_pending_command_executed = false;
    state.shell_pending_command_value = None;
    state.shell_pending_command_output = None;
    state.shell_pending_command_output_count = 0;
    state.dialog_command = None;
    state.text_area.set_shell_mode(false);
}

pub fn resume_session(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    // Terminate any active shell before switching sessions
    terminate_active_shell(state);

    state.message_tool_calls = None;
    state.message_approved_tools.clear();
    state.message_rejected_tools.clear();
    state.tool_call_execution_order.clear();
    state.session_tool_calls_queue.clear();
    state.approval_bar.clear();
    state.toggle_approved_message = true;

    state.messages.clear();
    state
        .messages
        .extend(welcome_messages(state.latest_version.clone(), state));
    render_system_message(state, "Resuming last session.");

    // Reset scroll state to show bottom when messages are loaded
    state.scroll = 0;
    state.scroll_to_bottom = true;
    state.stay_at_bottom = true;

    // Clear changeset and todos from previous session
    state.changeset = crate::services::changeset::Changeset::default();
    state.todos.clear();

    // Invalidate caches
    crate::services::message::invalidate_message_lines_cache(state);

    // Reset usage for the resumed session
    state.total_session_usage = LLMTokenUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        prompt_tokens_details: None,
    };
    state.current_message_usage = LLMTokenUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        prompt_tokens_details: None,
    };

    let _ = output_tx.try_send(OutputEvent::ResumeSession);

    state.text_area.set_text("");
    state.show_helper_dropdown = false;

    // Clear plan mode state
    state.plan_mode_active = false;
    state.plan_metadata = None;
    state.plan_content_hash = None;
    state.plan_previous_status = None;
    state.plan_review_auto_opened = false;
    state.show_plan_review = false;
    state.plan_review_content.clear();
    state.plan_review_lines.clear();
    state.plan_review_comments = None;
    state.plan_review_resolved_anchors.clear();
    state.plan_review_show_comment_modal = false;
    state.plan_review_comment_input.clear();
    state.plan_review_selected_comment = None;
    state.plan_review_modal_kind = None;
}

pub fn new_session(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    // Terminate any active shell before starting new session
    terminate_active_shell(state);

    let _ = output_tx.try_send(OutputEvent::NewSession);
    state.text_area.set_text("");
    state.messages.clear();
    state
        .messages
        .extend(welcome_messages(state.latest_version.clone(), state));
    render_system_message(state, "New session started.");

    // Reset scroll state
    state.scroll = 0;
    state.scroll_to_bottom = true;
    state.stay_at_bottom = true;

    // Clear changeset and todos from previous session
    state.changeset = crate::services::changeset::Changeset::default();
    state.todos.clear();

    // Invalidate caches
    crate::services::message::invalidate_message_lines_cache(state);

    // Reset usage for the new session
    state.total_session_usage = LLMTokenUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        prompt_tokens_details: None,
    };
    state.current_message_usage = LLMTokenUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        prompt_tokens_details: None,
    };

    state.show_helper_dropdown = false;
}

pub fn build_summarize_prompt(state: &AppState) -> String {
    let usage = &state.total_session_usage;
    let total_tokens = usage.total_tokens;
    let prompt_tokens = usage.prompt_tokens;
    let completion_tokens = usage.completion_tokens;

    // Use current_model if set (from streaming), otherwise use default model
    let active_model = state.current_model.as_ref().unwrap_or(&state.model);
    let max_tokens = active_model.limit.context as u32;

    let context_usage_pct = if max_tokens > 0 {
        (total_tokens as f64 / max_tokens as f64) * 100.0
    } else {
        0.0
    };

    let recent_inputs = collect_recent_user_inputs(state, 6);

    let mut prompt = String::from(SUMMARIZE_PROMPT_BASE);
    prompt.push('\n');
    prompt.push_str("Session snapshot:\n");
    prompt.push_str(&format!(
        "- Active profile: {}\n",
        state.current_profile_name
    ));
    prompt.push_str(&format!(
        "- Total tokens used: {} (prompt: {}, completion: {})\n",
        total_tokens, prompt_tokens, completion_tokens
    ));
    prompt.push_str(&format!(
        "- Context window usage: {:.1}% of {} tokens\n",
        context_usage_pct.min(100.0),
        max_tokens
    ));
    if !recent_inputs.is_empty() {
        prompt.push('\n');
        prompt.push_str("Recent user inputs to emphasize:\n");
        for input in recent_inputs {
            prompt.push_str("- ");
            prompt.push_str(&input);
            prompt.push('\n');
        }
    }
    prompt.push('\n');
    prompt.push_str(
        "Be precise, note outstanding TODOs or follow-ups, and reflect any cost or context considerations mentioned earlier.\n",
    );
    prompt.push_str(
        "When ready, create or overwrite `summary.md` using the tool call and populate it with the markdown summary.\n",
    );

    prompt
}

fn collect_recent_user_inputs(state: &AppState, limit: usize) -> Vec<String> {
    let mut entries = Vec::new();
    for message in state.messages.iter().rev() {
        match &message.content {
            MessageContent::Plain(text, _) | MessageContent::PlainText(text) => {
                let trimmed = text.trim();
                if let Some(stripped) = trimmed.strip_prefix("→ ") {
                    entries.push(stripped.trim().to_string());
                } else if trimmed.starts_with('/') {
                    entries.push(trimmed.to_string());
                }
            }
            _ => {}
        }
        if entries.len() >= limit {
            break;
        }
    }
    entries.reverse();
    entries
}

pub fn list_auto_approved_tools(state: &mut AppState) {
    let config = state.auto_approve_manager.get_config();
    let mut auto_approved_tools: Vec<_> = config
        .tools
        .iter()
        .filter(|(_, policy)| **policy == AutoApprovePolicy::Auto)
        .collect();

    // Filter by allowed_tools if configured
    if let Some(allowed_tools) = &state.allowed_tools
        && !allowed_tools.is_empty()
    {
        auto_approved_tools.retain(|(tool_name, _)| allowed_tools.contains(tool_name));
    }

    if auto_approved_tools.is_empty() {
        let message = if state
            .allowed_tools
            .as_ref()
            .is_some_and(|tools| !tools.is_empty())
        {
            "No allowed tools are currently set to auto-approve."
        } else {
            "No tools are currently set to auto-approve."
        };
        push_styled_message(state, message, Color::Cyan, "", Color::Cyan);
    } else {
        let tool_list = auto_approved_tools
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        // add a spacing marker
        state.messages.push(Message::plain_text(""));
        push_styled_message(
            state,
            &format!("Tools currently set to auto-approve: {}", tool_list),
            Color::Yellow,
            "",
            Color::Yellow,
        );
    }
}

// ========== Command Palette Rendering ==========
// NOTE: render_command_palette is preserved for reference but no longer used.
// The unified popup in shortcuts_popup.rs now handles command palette rendering.

#[allow(dead_code)]
pub fn render_command_palette(f: &mut Frame, state: &crate::app::AppState) {
    // Calculate popup size (smaller height)
    let area = centered_rect(42, 50, f.area());

    f.render_widget(ratatui::widgets::Clear, area);

    // Create the main block with border and background
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Split area for title, search, content, scroll indicators, and help text
    let inner_area = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width - 2,
        height: area.height - 2,
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title
            Constraint::Length(3), // Search with spacing
            Constraint::Min(3),    // Content
            Constraint::Length(1), // Scroll indicators
            Constraint::Length(1), // Help text
        ])
        .split(inner_area);

    // Render title
    let title = " Command Palette ";
    let title_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let title_line = Line::from(Span::styled(title, title_style));
    let title_paragraph = Paragraph::new(title_line);

    f.render_widget(title_paragraph, chunks[0]);

    // Render search input
    let search_prompt = ">";
    let cursor = "|";
    let placeholder = "Type to filter";

    let search_spans = if state.command_palette_search.is_empty() {
        vec![
            Span::raw(" "), // Small space before
            Span::styled(search_prompt, Style::default().fg(Color::Magenta)),
            Span::raw(" "),
            Span::styled(cursor, Style::default().fg(Color::Cyan)),
            Span::styled(placeholder, Style::default().fg(Color::DarkGray)),
            Span::raw(" "), // Small space after
        ]
    } else {
        vec![
            Span::raw(" "), // Small space before
            Span::styled(search_prompt, Style::default().fg(Color::Magenta)),
            Span::raw(" "),
            Span::styled(
                &state.command_palette_search,
                Style::default()
                    .fg(Color::Reset)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(cursor, Style::default().fg(Color::Cyan)),
            Span::raw(" "), // Small space after
        ]
    };

    let search_text = Text::from(vec![
        Line::from(""), // Empty line above
        Line::from(search_spans),
        Line::from(""), // Empty line below
    ]);
    let search_paragraph = Paragraph::new(search_text);
    f.render_widget(search_paragraph, chunks[1]);

    // Get filtered commands
    let filtered_commands = filter_commands(&state.command_palette_search);
    let total_commands = filtered_commands.len();
    let height = chunks[2].height as usize;

    // Calculate scroll position
    use crate::constants::SCROLL_BUFFER_LINES;
    let max_scroll = total_commands.saturating_sub(height.saturating_sub(SCROLL_BUFFER_LINES));
    let scroll = if state.command_palette_scroll > max_scroll {
        max_scroll
    } else {
        state.command_palette_scroll
    };

    // Add top arrow indicator if there are hidden items above
    let mut visible_lines = Vec::new();
    let has_content_above = scroll > 0;
    if has_content_above {
        visible_lines.push(Line::from(vec![Span::styled(
            " ▲",
            Style::default().fg(Color::Reset),
        )]));
    }

    // Create visible lines
    for i in 0..height {
        let line_index = scroll + i;
        if line_index < total_commands {
            let command = &filtered_commands[line_index];
            let available_width = area.width as usize - 2; // Account for borders
            let is_selected = line_index == state.command_palette_selected;
            let bg_color = if is_selected {
                Color::Cyan
            } else {
                Color::Reset
            };
            let text_color = if is_selected {
                Color::Black
            } else {
                Color::Reset
            };

            // Create a single line with name on left and shortcut on right
            let name_formatted = format!(
                " {:<width$}",
                command.name,
                width = available_width - command.shortcut.len() - 2
            );
            let shortcut_formatted = format!("{} ", command.shortcut);

            let spans = vec![
                Span::styled(name_formatted, Style::default().fg(text_color).bg(bg_color)),
                Span::styled(
                    shortcut_formatted,
                    Style::default()
                        .fg(if is_selected {
                            Color::Black
                        } else {
                            Color::DarkGray
                        })
                        .bg(bg_color),
                ),
            ];

            visible_lines.push(Line::from(spans));
        } else {
            visible_lines.push(Line::from(""));
        }
    }

    // Render content
    let content_paragraph = Paragraph::new(visible_lines)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .style(Style::default().bg(Color::Reset).fg(Color::White));

    f.render_widget(content_paragraph, chunks[2]);

    // Calculate cumulative commands count
    let mut cumulative_commands_count = 0;
    for line_index in 0..=(scroll + height).min(total_commands.saturating_sub(1)) {
        if line_index < total_commands {
            cumulative_commands_count += 1;
        }
    }

    // Scroll indicators
    let has_content_below = scroll < max_scroll;

    if has_content_above || has_content_below {
        let mut indicator_spans = vec![];

        // Show cumulative commands counter and down arrow on the left
        indicator_spans.push(Span::styled(
            format!(" ({}/{})", cumulative_commands_count, total_commands),
            Style::default().fg(Color::Reset),
        ));

        if has_content_below {
            indicator_spans.push(Span::styled(" ▼", Style::default().fg(Color::DarkGray)));
        }

        let indicator_paragraph = Paragraph::new(Line::from(indicator_spans));
        f.render_widget(indicator_paragraph, chunks[3]);
    } else {
        // Empty line when no scroll indicators
        f.render_widget(Paragraph::new(""), chunks[3]);
    }

    // Help text
    let help = Paragraph::new(Line::from(vec![
        Span::styled(" ↑/↓", Style::default().fg(Color::DarkGray)),
        Span::styled(" navigate", Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled("enter", Style::default().fg(Color::DarkGray)),
        Span::styled(" select", Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled("esc", Style::default().fg(Color::DarkGray)),
        Span::styled(" close", Style::default().fg(Color::Cyan)),
    ]));

    f.render_widget(help, chunks[4]);

    // Render the border with title last (so it's on top)
    f.render_widget(block, area);
}

/// Helper function to create a centered rectangle
#[allow(dead_code)]
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
