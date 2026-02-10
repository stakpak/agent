//! Create Custom Command Popup
//!
//! A popup UI for creating new custom slash commands via /create_custom_command.

use crate::app::{AppState, CreateCustomCommandState};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

/// Render the create custom command popup
pub fn render_create_command_popup(f: &mut Frame, state: &AppState) {
    let area = centered_rect(85, 70, f.area());

    f.render_widget(ratatui::widgets::Clear, area);

    let (step_label, prompt, path_hint, input_box_title, placeholder) =
        match &state.create_custom_command {
            Some(CreateCustomCommandState::AskingName) => (
                "Step 1 of 2: Name",
                "Enter the command name (letters, numbers, hyphens only).".to_string(),
                Some("File will be saved to: .stakpak/commands/{name}.md".to_string()),
                " Name ",
                " Enter name (e.g. my-command) ",
            ),
            Some(CreateCustomCommandState::AskingBody { name }) => (
                "Step 2 of 2: Body",
                format!(
                    "Enter the prompt content for /{}. This will be sent when you run the command.",
                    name
                ),
                Some(format!("File saved to: .stakpak/commands/{}.md", name)),
                " Prompt content ",
                " Enter prompt content (press Enter when done) ",
            ),
            None => return,
        };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Create Custom Command ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));

    // Render the outer block FIRST to prevent overlapping
    f.render_widget(block, area);

    let inner = Rect {
        x: area.x + 2,
        y: area.y + 2,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(4),
    };

    // Build constraints dynamically with proper spacing
    let mut constraints = vec![
        Constraint::Length(1), // Step label
        Constraint::Length(2), // Prompt
    ];
    if path_hint.is_some() {
        constraints.push(Constraint::Length(1)); // Path hint
    }
    if state.create_custom_command_popup_error.is_some() {
        constraints.push(Constraint::Length(2)); // Error
    }
    constraints.push(Constraint::Min(12)); // Input area - large multiline box
    constraints.push(Constraint::Length(1)); // Help

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let mut chunk_idx = 0;

    // Step label
    let step_para = Paragraph::new(Line::from(vec![
        Span::styled(step_label, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ]));
    f.render_widget(step_para, chunks[chunk_idx]);
    chunk_idx += 1;

    // Prompt / instruction
    let prompt_lines_vec: Vec<Line> = prompt
        .split('\n')
        .map(|s| Line::from(Span::styled(s, Style::default().fg(Color::Gray))))
        .collect();
    let prompt_para = Paragraph::new(prompt_lines_vec);
    f.render_widget(prompt_para, chunks[chunk_idx]);
    chunk_idx += 1;

    // Path hint: where the file will be saved
    if let Some(path) = &path_hint {
        let path_para = Paragraph::new(
            Span::styled(path, Style::default().fg(Color::Yellow))
        );
        f.render_widget(path_para, chunks[chunk_idx]);
        chunk_idx += 1;
    }

    // Error message (inside popup)
    if let Some(err) = &state.create_custom_command_popup_error {
        let err_para = Paragraph::new(Line::from(vec![
            Span::styled("✗ ", Style::default().fg(Color::Red)),
            Span::styled(err.as_str(), Style::default().fg(Color::Red)),
        ]));
        f.render_widget(err_para, chunks[chunk_idx]);
        chunk_idx += 1;
    }

    // Input field with highlighted box (wraps for multi-line body)
    let input_area = chunks[chunk_idx];
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(Span::styled(
            input_box_title,
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        ));
    let input_inner = Rect {
        x: input_area.x + 1,
        y: input_area.y + 1,
        width: input_area.width.saturating_sub(2),
        height: input_area.height.saturating_sub(2),
    };

    let input = &state.create_custom_command_popup_input;
    let input_spans = if input.is_empty() {
        vec![
            Span::styled("> ", Style::default().fg(Color::Magenta)),
            Span::styled("|", Style::default().fg(Color::Cyan)),
            Span::styled(placeholder, Style::default().fg(Color::DarkGray)),
        ]
    } else {
        vec![
            Span::styled("> ", Style::default().fg(Color::Magenta)),
            Span::styled(
                input.as_str(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("|", Style::default().fg(Color::Cyan)),
        ]
    };
    let input_para = Paragraph::new(Line::from(input_spans)).wrap(Wrap { trim: false });
    f.render_widget(input_block, input_area);
    f.render_widget(input_para, input_inner);

    // Help
    let help = Paragraph::new(vec![Line::from(vec![
        Span::styled("Enter", Style::default().fg(Color::DarkGray)),
        Span::styled(" confirm  ", Style::default().fg(Color::Cyan)),
        Span::styled("Esc", Style::default().fg(Color::DarkGray)),
        Span::styled(" cancel", Style::default().fg(Color::Cyan)),
    ])]);
    f.render_widget(help, chunks[chunk_idx]);
}

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
