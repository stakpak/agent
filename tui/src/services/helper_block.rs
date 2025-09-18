use crate::app::{AppState, LoadingType};
use crate::services::message::{Message, MessageContent};
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
}

pub fn push_help_message(state: &mut AppState) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    let mut lines = Vec::new();
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
    let lines = vec![
        Line::from(vec![
            Span::styled(
                flag,
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(error, Style::default().fg(Color::Red)),
        ]),
        Line::from(""),
    ];
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
}

pub fn render_loading_spinner(state: &AppState) -> Line<'_> {
    let spinner_chars = ["▄▀", "▐▌", "▀▄", "▐▌"];
    let spinner = spinner_chars[state.spinner_frame % spinner_chars.len()];
    let spinner_text = if state.loading_type == LoadingType::Sessions {
        "Loading sessions..."
    } else {
        "Stakpaking..."
    };

    if state.loading_type == LoadingType::Sessions {
        Line::from(vec![Span::styled(
            format!("{} {}", spinner, spinner_text),
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        )])
    } else {
        Line::from(vec![
            Span::styled(
                format!("{} {}", spinner, spinner_text),
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" - Esc to cancel", Style::default().fg(Color::DarkGray)),
        ])
    }
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
}

pub fn version_message(latest_version: Option<String>) -> Message {
    match latest_version {
        Some(version) => {
            if version != format!("v{}", env!("CARGO_PKG_VERSION")) {
                Message::info(
                    format!(
                        "🚀 Update available!  v{}  →  {} ✨   ",
                        env!("CARGO_PKG_VERSION"),
                        version
                    ),
                    Some(Style::default().fg(ratatui::style::Color::Yellow)),
                )
            } else {
                Message::info(
                    format!("Current Version: {}", env!("CARGO_PKG_VERSION")),
                    None,
                )
            }
        }
        None => Message::info(
            format!("Current Version: {}", env!("CARGO_PKG_VERSION")),
            None,
        ),
    }
}

// pub fn welcome_messages(latest_version: Option<String>) -> Vec<Message> {
//     vec![
//         Message::info(
//             r"
//  ▗▄▄▖▗▄▄▄▖▗▄▖ ▗▖ ▗▖▗▄▄▖  ▗▄▖ ▗▖ ▗▖     ▗▄▖  ▗▄▄▖▗▄▄▄▖▗▖  ▗▖▗▄▄▄▖
// ▐▌     █ ▐▌ ▐▌▐▌▗▞▘▐▌ ▐▌▐▌ ▐▌▐▌▗▞▘    ▐▌ ▐▌▐▌   ▐▌   ▐▛▚▖▐▌  █
//  ▝▀▚▖  █ ▐▛▀▜▌▐▛▚▖ ▐▛▀▘ ▐▛▀▜▌▐▛▚▖     ▐▛▀▜▌▐▌▝▜▌▐▛▀▀▘▐▌ ▝▜▌  █
// ▗▄▄▞▘  █ ▐▌ ▐▌▐▌ ▐▌▐▌   ▐▌ ▐▌▐▌ ▐▌    ▐▌ ▐▌▝▚▄▞▘▐▙▄▄▖▐▌  ▐▌  █  ",
//             Some(Style::default().fg(ratatui::style::Color::Cyan)),
//         ),
//         version_message(latest_version),
//         Message::info("/help for help, /status for your current setup", None),
//         Message::info(
//             format!(
//                 "cwd: {}",
//                 std::env::current_dir().unwrap_or_default().display()
//             ),
//             None,
//         ),
//     ]
// }

pub fn welcome_messages(
    latest_version: Option<String>,
    state: &crate::app::AppState,
) -> Vec<Message> {
    let mut messages = vec![
        Message::info(
            r"
   ██████╗████████╗ █████╗ ██╗  ██╗██████╗  █████╗ ██╗  ██╗ 
   ██╔═══╝╚══██╔══╝██╔══██╗██║ ██╔╝██╔══██╗██╔══██╗██║ ██╔╝ 
   ███████╗  ██║   ███████║█████╔╝ ██████╔╝███████║█████╔╝  
   ╚════██║  ██║   ██╔══██║██╔═██╗ ██╔═══╝ ██╔══██║██╔═██╗  
   ███████║  ██║   ██║  ██║██║  ██╗██║     ██║  ██║██║  ██╗ 
   ╚══════╝  ╚═╝   ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝ ",
            Some(Style::default().fg(ratatui::style::Color::Cyan)),
        ),
        Message::info("SPACING_MARKER", None),
        version_message(latest_version),
        Message::info("SPACING_MARKER", None),
        Message::info("/help for help, /status for your current setup", None),
        Message::info("SPACING_MARKER", None),
        Message::info(
            format!(
                "cwd: {}",
                std::env::current_dir().unwrap_or_default().display()
            ),
            None,
        ),
        Message::info("SPACING_MARKER", None),
    ];

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
}

const EXCEEDED_API_LIMIT_ERROR: &str = "Exceeded API limit";
const EXCEEDED_API_LIMIT_ERROR_MESSAGE: &str =
    "Please top up your account at https://stakpak.dev/settings/billing to keep Stakpaking.";

pub fn handle_errors(error: String) -> String {
    if format!("{:?}", error).contains(EXCEEDED_API_LIMIT_ERROR) {
        EXCEEDED_API_LIMIT_ERROR_MESSAGE.to_string()
    } else if error.contains("Unknown(\"") && error.ends_with("\")") {
        let start = 9; // length of "Unknown(\""
        let end = error.len() - 2; // remove ")" and "\""
        if start < end {
            error[start..end].to_string()
        } else {
            format!("{:?}", error)
        }
    } else {
        format!("{:?}", error)
    }
}
