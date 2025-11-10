use crate::app::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

pub fn render_recovery_popup(f: &mut Frame, state: &mut AppState) {
    if state.recovery_options.is_empty() {
        return;
    }

    if state.recovery_popup_selected >= state.recovery_options.len() {
        state.recovery_popup_selected = state.recovery_options.len().saturating_sub(1);
    }

    let screen = f.area();
    let popup_height = (state.recovery_options.len() as u16)
        .saturating_mul(3)
        .saturating_add(6);
    let popup_height = popup_height.min(screen.height).max(7);

    let popup_area = Rect {
        x: screen.x,
        y: screen
            .y
            .saturating_add(screen.height.saturating_sub(popup_height)),
        width: screen.width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner_area = Rect {
        x: popup_area.x + 1,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(2),
        height: popup_area.height.saturating_sub(2),
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner_area);

    let title_line = Line::from(vec![Span::styled(
        " Recovery Options",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]);
    f.render_widget(Paragraph::new(title_line), chunks[0]);

    let mut list_state = ListState::default();
    list_state.select(Some(state.recovery_popup_selected));

    let list_width = chunks[1].width.saturating_sub(2);

    let list_items: Vec<ListItem> = state
        .recovery_options
        .iter()
        .enumerate()
        .map(|(idx, option)| {
            let mut lines: Vec<Line> = Vec::new();

            let label = format!("{}.", idx + 1);
            lines.push(Line::from(vec![format_label(
                &label,
                &option.mode,
                idx == state.recovery_popup_selected,
            )]));

            let summary = summarize_option(option);
            let wrapped = textwrap::wrap(&summary, list_width as usize);
            for line in wrapped {
                lines.push(Line::from(vec![Span::styled(
                    format!("    {}", line),
                    Style::default().fg(Color::Gray),
                )]));
            }

            lines.push(Line::from(""));

            ListItem::new(Text::from(lines))
        })
        .collect();

    let list = List::new(list_items)
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::NONE));

    f.render_stateful_widget(list, chunks[1], &mut list_state);

    let help = Paragraph::new(Line::from(vec![
        Span::styled(
            "↑/↓",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": Navigate  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "Enter",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": Select  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "Esc",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::styled(": Close", Style::default().fg(Color::DarkGray)),
    ]))
    .alignment(ratatui::layout::Alignment::Left);
    f.render_widget(help, chunks[2]);

    f.render_widget(block, popup_area);
}

fn format_label(
    label: &str,
    mode: &stakpak_api::models::RecoveryMode,
    selected: bool,
) -> Span<'static> {
    let text = format!("{} {}", label, format_mode(mode));
    if selected {
        Span::styled(text, Style::default().fg(Color::Black))
    } else {
        Span::styled(text, Style::default().fg(Color::Yellow))
    }
}

fn format_mode(mode: &stakpak_api::models::RecoveryMode) -> &'static str {
    match mode {
        stakpak_api::models::RecoveryMode::Redirection => "REDIRECTION",
        stakpak_api::models::RecoveryMode::Revert => "REVERT",
        stakpak_api::models::RecoveryMode::ModelChange => "MODELCHANGE",
    }
}

fn summarize_option(option: &stakpak_api::models::RecoveryOption) -> String {
    let primary = option
        .redirection_message
        .as_ref()
        .filter(|msg| !msg.trim().is_empty())
        .unwrap_or(&option.reasoning);

    let sanitized = primary.replace('\n', " ").trim().to_string();

    if sanitized.len() > 140 {
        format!("{}...", sanitized.chars().take(140).collect::<String>())
    } else {
        sanitized
    }
}
