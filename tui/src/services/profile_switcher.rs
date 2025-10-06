use crate::app::AppState;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

pub fn render_profile_switcher_popup(f: &mut Frame, state: &AppState) {
    // Calculate popup size (60% width, fit height to content)
    let area = centered_rect(60, 50, f.area());

    // Clear background
    f.render_widget(ratatui::widgets::Clear, area);

    // Create list items
    let items: Vec<ListItem> = state
        .available_profiles
        .iter()
        .enumerate()
        .map(|(idx, profile_name)| {
            let is_selected = idx == state.profile_switcher_selected;
            let is_current = profile_name == &state.current_profile_name;

            // Build the display line
            let mut spans = vec![];

            // Selection indicator
            spans.push(Span::raw(if is_selected { "→ " } else { "  " }));

            // Profile name
            let name_style = if is_current {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            spans.push(Span::styled(profile_name.clone(), name_style));

            // Current indicator
            if is_current {
                spans.push(Span::styled(" (current)", Style::default().fg(Color::Cyan)));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Switch Profile ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    // Split area for list and help text
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(area);

    f.render_widget(list, chunks[0]);

    // Help text
    let help = Paragraph::new(Line::from(vec![
        Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
        Span::raw(": Navigate  "),
        Span::styled("↵", Style::default().fg(Color::Yellow)),
        Span::raw(": Switch  "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(": Cancel"),
    ]))
    .block(Block::default().borders(Borders::ALL))
    .alignment(Alignment::Center);

    f.render_widget(help, chunks[1]);
}

pub fn render_profile_switch_overlay(f: &mut Frame, state: &AppState) {
    let area = centered_rect(50, 20, f.area());

    f.render_widget(ratatui::widgets::Clear, area);

    let status_text = state
        .profile_switch_status_message
        .as_deref()
        .unwrap_or("Switching profile...");

    let block = Block::default()
        .title(" Profile Switch ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(vec![
        Line::from(Span::styled(
            status_text,
            Style::default().fg(Color::Yellow),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Please wait... (Ctrl+C to cancel)",
            Style::default().fg(Color::Gray),
        )),
    ])
    .block(block)
    .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
}

/// Helper function to create a centered rect
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
