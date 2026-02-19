use crate::app::AppState;
use crate::constants::{EXCEEDED_API_LIMIT_ERROR, EXCEEDED_API_LIMIT_ERROR_MESSAGE};
use crate::services::message::{Message, MessageContent, invalidate_message_lines_cache};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use uuid::Uuid;

pub fn get_stakpak_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Generate a mouse capture hint message based on the terminal type
pub fn mouse_capture_hint_message(state: &crate::app::AppState) -> Message {
    let status = if state.mouse_capture_enabled {
        "enabled"
    } else {
        "disabled"
    };
    let status_color = if state.mouse_capture_enabled {
        Color::LightGreen
    } else {
        Color::LightRed
    };

    let styled_line = Line::from(vec![
        Span::styled(
            "█",
            Style::default()
                .fg(Color::LightMagenta)
                .bg(Color::LightMagenta),
        ),
        Span::raw("  Mouse capture "),
        Span::styled(status, Style::default().fg(status_color)),
        Span::styled(
            " • Ctrl+L to toggle",
            Style::default().fg(Color::LightMagenta),
        ),
    ]);

    Message::styled(styled_line)
}

pub fn push_status_message(state: &mut AppState) {
    let status_text = state.account_info.clone();
    let version = get_stakpak_version();
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "?".to_string());

    // Default values
    let mut id = "unknown".to_string();
    let mut username = "unknown".to_string();
    let mut name = "unknown".to_string();

    for line in status_text.lines() {
        if let Some(rest) = line.strip_prefix("ID: ") {
            id = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Username: ") {
            username = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Name: ") {
            name = rest.trim().to_string();
        }
    }

    let lines = vec![
        Line::from(vec![Span::styled(
            format!("Stakpak Code Status v{}", version),
            Style::default()
                .fg(Color::Reset)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Working Directory",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(format!("  L {}", cwd)),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Account",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(format!("  L Username: {}", username)),
        Line::from(format!("  L ID: {}", id)),
        Line::from(format!("  L Name: {}", name)),
        Line::from(""),
    ];
    state.messages.push(Message {
        id: uuid::Uuid::new_v4(),
        content: MessageContent::StyledBlock(lines),
        is_collapsed: None,
    });
    invalidate_message_lines_cache(state);
}

pub fn push_usage_message(state: &mut AppState) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};

    let usage = &state.total_session_usage;
    let mut lines = Vec::new();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "Session Usage",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(""));

    if usage.total_tokens == 0 {
        lines.push(Line::from(vec![Span::styled(
            " No tokens used yet in this session.",
            Style::default().fg(Color::DarkGray),
        )]));
    } else {
        // Manually format each line with fixed spacing to align all numbers (no colons)
        let formatted_prompt = format_number_with_separator(usage.prompt_tokens);
        lines.push(Line::from(vec![
            Span::raw(" Prompt tokens"),
            Span::raw("      "), // 6 spaces to align numbers
            Span::styled(
                formatted_prompt,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        // Show prompt token details if available
        if let Some(details) = &usage.prompt_tokens_details {
            // Always show all fields except output_tokens (redundant with Completion tokens), using 0 if None, with fixed spacing
            let input_tokens = format_number_with_separator(details.input_tokens.unwrap_or(0));
            let cache_write =
                format_number_with_separator(details.cache_write_input_tokens.unwrap_or(0));
            let cache_read =
                format_number_with_separator(details.cache_read_input_tokens.unwrap_or(0));

            lines.push(Line::from(vec![
                Span::raw("  ├─ Input tokens"),
                Span::raw("   "), // 3 spaces to align numbers
                Span::styled(input_tokens, Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::from(vec![
                Span::raw("  ├─ Cache write"),
                Span::raw("    "), // 4 spaces to align numbers
                Span::styled(cache_write, Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::from(vec![
                Span::raw("  └─ Cache read"),
                Span::raw("     "), // 5 spaces to align numbers
                Span::styled(cache_read, Style::default().fg(Color::DarkGray)),
            ]));
        }

        let formatted_completion = format_number_with_separator(usage.completion_tokens);
        lines.push(Line::from(vec![
            Span::raw(" Completion tokens"),
            Span::raw("  "), // 2 spaces to align numbers
            Span::styled(
                formatted_completion,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        let formatted_total = format_number_with_separator(usage.total_tokens);
        lines.push(Line::from(vec![
            Span::raw(" Total tokens"),
            Span::raw("       "), // 7 spaces to align numbers
            Span::styled(
                formatted_total,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    state.messages.push(Message {
        id: uuid::Uuid::new_v4(),
        content: MessageContent::StyledBlock(lines),
        is_collapsed: None,
    });
    invalidate_message_lines_cache(state);
}

pub fn format_number_with_separator(n: u32) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (count, c) in s.chars().rev().enumerate() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

pub fn push_memorize_message(state: &mut AppState) {
    let lines = vec![
        Line::from(vec![Span::styled(
            "📝 Memorizing conversation history...",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "We're extracting important information from your conversation in the background.",
            Style::default().fg(Color::Reset),
        )]),
        Line::from(vec![Span::styled(
            "Feel free to continue talking to the agent while this happens!",
            Style::default().fg(Color::Green),
        )]),
        Line::from(""),
    ];
    state.messages.push(Message {
        id: uuid::Uuid::new_v4(),
        content: MessageContent::StyledBlock(lines),
        is_collapsed: None,
    });
    invalidate_message_lines_cache(state);
}

pub fn push_help_message(state: &mut AppState) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    let mut lines = Vec::new();
    lines.push(Line::from(""));
    // usage mode
    lines.push(Line::from(vec![Span::styled(
        "Usage Mode",
        Style::default()
            .fg(Color::Reset)
            .add_modifier(Modifier::BOLD),
    )]));

    let usage_modes = vec![
        ("REPL", "stakpak (interactive session)", Color::Reset),
        (
            "Non-interactive",
            "stakpak -p  \"prompt\" -c <checkpoint_id>",
            Color::Reset,
        ),
    ];
    for (mode, desc, color) in usage_modes {
        lines.push(Line::from(vec![
            Span::styled(
                "● ",
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(mode),
            Span::raw(" – "),
            Span::styled(
                desc,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("Run"),
        Span::styled(
            " stakpak --help ",
            Style::default()
                .fg(Color::Reset)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("to see all commands", Style::default().fg(Color::Gray)),
    ]));
    lines.push(Line::from(""));
    // Section header
    lines.push(Line::from(vec![Span::styled(
        "Available commands",
        Style::default()
            .fg(Color::Reset)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(""));
    // Slash-commands header
    lines.push(Line::from(vec![Span::styled(
        "Slash-commands",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )]));

    // Slash-commands list
    let commands = crate::app::AppState::get_helper_commands();

    for cmd in &commands {
        lines.push(Line::from(vec![
            Span::styled(cmd.command, Style::default().fg(Color::Cyan)),
            Span::raw(" – "),
            Span::raw(cmd.description),
        ]));
    }
    lines.push(Line::from(""));

    // Keyboard shortcuts header
    lines.push(Line::from(vec![Span::styled(
        "Keyboard shortcuts",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )]));

    // Shortcuts list
    let shortcuts = vec![
        ("Enter", "send message", Color::Yellow),
        ("Ctrl+J or Shift+Enter", "insert newline", Color::Yellow),
        // ("Up/Down", "scroll prompt history", Color::Yellow),
        ("Ctrl+C", "quit Stakpak", Color::Yellow),
    ];
    for (key, desc, color) in shortcuts {
        lines.push(Line::from(vec![
            Span::styled(key, Style::default().fg(color)),
            Span::raw(" – "),
            Span::raw(desc),
        ]));
    }
    lines.push(Line::from(""));
    state.messages.push(Message {
        id: uuid::Uuid::new_v4(),
        content: MessageContent::StyledBlock(lines),
        is_collapsed: None,
    });
    invalidate_message_lines_cache(state);
}

pub fn render_system_message(state: &mut AppState, msg: &str) {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("🤖", Style::default()),
        Span::styled(
            " System",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    let message = Line::from(vec![Span::raw(format!(
        "{pad} - {msg}",
        pad = " ".repeat(2)
    ))]);
    lines.push(message);
    lines.push(Line::from(vec![Span::raw(" ")]));

    state.messages.push(Message {
        id: Uuid::new_v4(),
        content: MessageContent::StyledBlock(lines),
        is_collapsed: None,
    });
    invalidate_message_lines_cache(state);
}

pub fn push_error_message(state: &mut AppState, error: &str, remove_flag: Option<bool>) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    let mut flag = "[Error] ".to_string();
    if let Some(remove_flag) = remove_flag
        && remove_flag
    {
        flag = " ".repeat(flag.len());
    }
    // split error by \n
    let error_parts: Vec<&str> = error.split('\n').collect();
    let mut lines = Vec::new();
    for (i, part) in error_parts.iter().enumerate() {
        if i == 0 {
            // First line gets the error flag
            lines.push(Line::from(vec![
                Span::styled(
                    flag.clone(),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(*part, Style::default().fg(Color::Red)),
            ]));
        } else {
            // Subsequent lines are just the error text
            lines.push(Line::from(vec![Span::styled(
                *part,
                Style::default().fg(Color::Red),
            )]));
        }
    }
    lines.push(Line::from(""));
    let owned_lines: Vec<Line<'static>> = lines
        .into_iter()
        .map(|line| {
            let owned_spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|span| Span::styled(span.content.into_owned(), span.style))
                .collect();
            Line::from(owned_spans)
        })
        .collect();
    state.messages.push(Message {
        id: uuid::Uuid::new_v4(),
        content: MessageContent::StyledBlock(owned_lines),
        is_collapsed: None,
    });
    invalidate_message_lines_cache(state);
}

pub fn push_styled_message(
    state: &mut AppState,
    message: &str,
    color: Color,
    icon: &str,
    icon_color: Color,
) {
    let line = Line::from(vec![
        Span::styled(icon.to_string(), Style::default().fg(icon_color)),
        Span::styled(message.to_string(), Style::default().fg(color)),
    ]);
    state.messages.push(Message::styled(line));
    invalidate_message_lines_cache(state);
}

pub fn version_message(latest_version: Option<String>) -> Message {
    match latest_version {
        Some(version) => {
            if version != format!("v{}", env!("CARGO_PKG_VERSION")) {
                Message::info(
                    format!(
                        "🚀 update available!  v{}  →  {} ✨   ",
                        env!("CARGO_PKG_VERSION"),
                        version
                    ),
                    Some(Style::default().fg(ratatui::style::Color::Yellow)),
                )
            } else {
                Message::info(format!("version: {}", env!("CARGO_PKG_VERSION")), None)
            }
        }
        None => Message::info(format!("version: {}", env!("CARGO_PKG_VERSION")), None),
    }
}

#[allow(unused_variables)]
pub fn welcome_messages(
    latest_version: Option<String>,
    state: &crate::app::AppState,
) -> Vec<Message> {
    let mut messages = vec![
        Message::info(
            r" 
   ╔═╗ ╔═╗    ██████╗████████╗ █████╗ ██╗  ██╗██████╗  █████╗ ██╗  ██╗ 
   ║^║ ║^║    ██╔═══╝╚══██╔══╝██╔══██╗██║ ██╔╝██╔══██╗██╔══██╗██║ ██╔╝ 
  ╔╝█████╚╗   ███████╗  ██║   ███████║█████╔╝ ██████╔╝███████║█████╔╝ 
>═║███████║═< ╚════██║  ██║   ██╔══██║██╔═██╗ ██╔═══╝ ██╔══██║██╔═██╗ 
>═║███████║═< ███████║  ██║   ██║  ██║██║  ██╗██║     ██║  ██║██║  ██╗ 
  ╚═══════╝   ╚══════╝  ╚═╝   ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝ ",
            Some(Style::default().fg(ratatui::style::Color::Cyan)),
        ),
        Message::info("SPACING_MARKER", None),
        version_message(latest_version),
        Message::info("SPACING_MARKER", None),
        Message::info(
            format!(
                "cwd: {}",
                std::env::current_dir().unwrap_or_default().display()
            ),
            None,
        ),
    ];

    // Show allowed tools for debugging
    // if let Some(tools) = &state.allowed_tools {
    //     if tools.is_empty() {
    //         messages.push(Message::info(
    //             "allowed_tools: [] (empty - all tools allowed)",
    //             Some(Style::default().fg(ratatui::style::Color::Gray)),
    //         ));
    //     } else {
    //         messages.push(Message::info(
    //             format!("allowed_tools: {} configured", tools.len()),
    //             Some(Style::default().fg(ratatui::style::Color::Gray)),
    //         ));
    //     }
    // } else {
    //     messages.push(Message::info(
    //         "allowed_tools: all tools allowed",
    //         Some(Style::default().fg(ratatui::style::Color::Gray)),
    //     ));
    // }

    // // Show rulebook configuration
    // if let Some(rb_config) = &state.rulebook_config {
    //     if let Some(include) = &rb_config.include
    //         && !include.is_empty()
    //     {
    //         messages.push(Message::info(
    //             format!("rulebooks.include: {:?}", include),
    //             Some(Style::default().fg(ratatui::style::Color::DarkGray)),
    //         ));
    //     }
    //     if let Some(exclude) = &rb_config.exclude
    //         && !exclude.is_empty()
    //     {
    //         messages.push(Message::info(
    //             format!("rulebooks.exclude: {:?}", exclude),
    //             Some(Style::default().fg(ratatui::style::Color::DarkGray)),
    //         ));
    //     }
    //     if let Some(include_tags) = &rb_config.include_tags
    //         && !include_tags.is_empty()
    //     {
    //         messages.push(Message::info(
    //             format!("rulebooks.include_tags: {:?}", include_tags),
    //             Some(Style::default().fg(ratatui::style::Color::DarkGray)),
    //         ));
    //     }
    //     if let Some(exclude_tags) = &rb_config.exclude_tags
    //         && !exclude_tags.is_empty()
    //     {
    //         messages.push(Message::info(
    //             format!("rulebooks.exclude_tags: {:?}", exclude_tags),
    //             Some(Style::default().fg(ratatui::style::Color::DarkGray)),
    //         ));
    //     }
    // }

    messages.push(Message::info("SPACING_MARKER", None));
    #[cfg(unix)]
    if state.mouse_capture_enabled {
        messages.push(mouse_capture_hint_message(state));
        messages.push(Message::info("SPACING_MARKER", None));
    }
    messages
}

pub fn push_clear_message(state: &mut AppState) {
    state.messages.clear();
    state.text_area.set_text("");
    state.show_helper_dropdown = false;
    let welcome_msg = welcome_messages(state.latest_version.clone(), state);
    state.messages.extend(welcome_msg);
    invalidate_message_lines_cache(state);
}

pub fn handle_errors(error: String) -> String {
    if error.contains(EXCEEDED_API_LIMIT_ERROR) {
        EXCEEDED_API_LIMIT_ERROR_MESSAGE.to_string()
    } else if error.contains("Unknown(\"") && error.ends_with("\")") {
        let start = 9; // length of "Unknown(\""
        let end = error.len() - 2; // remove ")" and "\""
        if start < end {
            error[start..end].to_string()
        } else {
            error
        }
    } else {
        error
    }
}

pub fn push_issue_message(state: &mut AppState) {
    let url = "https://github.com/stakpak/cli/issues/new";

    // Try to open the URL in the default browser
    match open::that(url) {
        Ok(_) => {
            let message = Message::styled(Line::from(vec![
                Span::styled(
                    "🔗 Opening GitHub Issues... ",
                    Style::default().fg(Color::Green),
                ),
                Span::raw(url),
            ]));
            state.messages.push(message);
        }
        Err(e) => {
            let message = Message::styled(Line::from(vec![
                Span::styled(
                    "❌ Failed to open GitHub Issues: ",
                    Style::default().fg(Color::Red),
                ),
                Span::raw(e.to_string()),
                Span::raw(" - "),
                Span::raw(url),
            ]));
            state.messages.push(message);
        }
    }
    invalidate_message_lines_cache(state);
}

pub fn push_support_message(state: &mut AppState) {
    let url = "https://discord.gg/c4HUkDD45d";

    // Try to open the URL in the default browser
    match open::that(url) {
        Ok(_) => {
            let message = Message::styled(Line::from(vec![
                Span::styled(
                    "💬 Opening Discord Support... ",
                    Style::default().fg(Color::Green),
                ),
                Span::raw(url),
            ]));
            state.messages.push(message);
        }
        Err(e) => {
            let message = Message::styled(Line::from(vec![
                Span::styled(
                    "❌ Failed to open Discord Support: ",
                    Style::default().fg(Color::Red),
                ),
                Span::raw(e.to_string()),
                Span::raw(" - "),
                Span::raw(url),
            ]));
            state.messages.push(message);
        }
    }
    invalidate_message_lines_cache(state);
}
