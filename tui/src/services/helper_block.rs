use crate::app::{AppState, LoadingType};
use crate::services::message::{Message, MessageContent};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use uuid::Uuid;

pub fn get_stakpak_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
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
                .fg(Color::White)
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
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )]));

    let usage_modes = vec![
        ("REPL", "stakpak (interactive session)", Color::White),
        (
            "Non-interactive",
            "stakpak -p  \"prompt\" -c <checkpoint_id>",
            Color::White,
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
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("to see all commands", Style::default().fg(Color::Gray)),
    ]));
    lines.push(Line::from(""));
    // Section header
    lines.push(Line::from(vec![Span::styled(
        "Available commands",
        Style::default()
            .fg(Color::White)
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
    let commands = vec![
        ("/help", "show this help overlay"),
        ("/status", "show account status"),
        ("/sessions", "show list of sessions"),
        ("/quit", "quit the app"),
    ];
    for (cmd, desc) in commands {
        lines.push(Line::from(vec![
            Span::styled(cmd, Style::default().fg(Color::Cyan)),
            Span::raw(" – "),
            Span::raw(desc),
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
        ("Up/Down", "scroll prompt history", Color::Yellow),
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
    });
}

pub fn push_error_message(state: &mut AppState, error: &str) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "[Error] ",
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
    });
}

pub fn render_loading_spinner<'a>(state: &'a AppState) -> Line<'a> {
    let spinner_chars = ["▄▀", "▐▌", "▀▄", "▐▌"];
    let spinner = spinner_chars[state.spinner_frame % spinner_chars.len()];
    let spinner_text = if state.loading_type == LoadingType::Sessions {
        "Loading sessions..."
    } else {
        "Stakpaking..."
    };
    let loading_line = Line::from(vec![Span::styled(
        format!("{} {}", spinner, spinner_text),
        Style::default()
            .fg(Color::LightRed)
            .add_modifier(Modifier::BOLD),
    )]);
    loading_line
}
