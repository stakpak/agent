use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
};

#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub shortcut: String,
    pub action: CommandAction,
}

#[derive(Debug, Clone)]
pub enum CommandAction {
    OpenProfileSwitcher,
    OpenRulebookSwitcher,
    OpenSessions,
    OpenShortcuts,
    ResumeSession,
    ShowStatus,
    MemorizeConversation,
    SubmitIssue,
    GetSupport,
    NewSession,
    ShowUsage,
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
            "Shortcuts",
            "Show all keyboard shortcuts",
            "Ctrl+S",
            CommandAction::OpenShortcuts,
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
    const SCROLL_BUFFER_LINES: usize = 2;
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
        Span::styled(" ↑/↓", Style::default().fg(Color::Yellow)),
        Span::raw(": Navigate  "),
        Span::styled("Enter", Style::default().fg(Color::Green)),
        Span::raw(": Select  "),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::raw(": Close"),
    ]));

    f.render_widget(help, chunks[4]);

    // Render the border with title last (so it's on top)
    f.render_widget(block, area);
}

/// Helper function to create a centered rectangle
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
