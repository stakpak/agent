use crate::app::AppState;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

pub fn render_profile_switcher_popup(f: &mut Frame, state: &AppState) {
    // Calculate popup size (35% width, fit height to content)
    let area = centered_rect(35, 50, f.area());

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

            // Profile name (no selection indicator)
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
            spans.push(Span::styled(
                format!(" {}", profile_name.clone()),
                name_style,
            ));

            // Current indicator
            if is_current {
                spans.push(Span::styled(" (current)", Style::default().fg(Color::Cyan)));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    // Create the main block with border (no title)
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Split area for title, list and help text inside the block
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
            Constraint::Min(3),    // List
            Constraint::Length(1), // Help text
        ])
        .split(inner_area);

    // Render title inside the popup
    let title = " Switch Profile";
    let title_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let title_line = Line::from(Span::styled(title, title_style));
    let title_paragraph = Paragraph::new(title_line);

    f.render_widget(title_paragraph, chunks[0]);

    // Create list with proper block and padding
    let list = List::new(items)
        .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black))
        .block(Block::default().borders(Borders::NONE));

    // Create list state for highlighting
    let mut list_state = ListState::default();
    list_state.select(Some(state.profile_switcher_selected));

    // Render list with proper padding
    let list_area = Rect {
        x: chunks[1].x,
        y: chunks[1].y + 1,
        width: chunks[1].width,
        height: chunks[1].height,
    };

    f.render_stateful_widget(list, list_area, &mut list_state);

    // Help text
    let help = Paragraph::new(Line::from(vec![
        Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
        Span::raw(": Navigate  "),
        Span::styled("↵", Style::default().fg(Color::Cyan)),
        Span::raw(": Switch  "),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::raw(": Cancel"),
    ]));

    let help_area = Rect {
        x: chunks[2].x + 1,
        y: chunks[2].y,
        width: chunks[2].width.saturating_sub(2),
        height: chunks[2].height,
    };

    f.render_widget(help, help_area);

    // Render the border with title last (so it's on top)
    f.render_widget(block, area);
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
