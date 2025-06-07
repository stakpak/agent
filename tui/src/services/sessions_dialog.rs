use crate::app::AppState;
use crate::services::message::get_wrapped_message_lines;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

pub fn render_sessions_dialog(f: &mut Frame, state: &AppState) {
    let screen = f.area();
    let dialog_height = 12;

    let message_lines = get_wrapped_message_lines(&state.messages, screen.width as usize);
    let mut last_message_y = message_lines.len() as u16 + 1; // +1 for a gap
    if last_message_y + dialog_height > screen.height {
        last_message_y = screen.height.saturating_sub(dialog_height + 1);
    }

    let area = Rect {
        x: 1,
        y: last_message_y,
        width: screen.width - 2,
        height: dialog_height,
    };

    // Outer block with title
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightYellow))
        .title(Span::styled(
            "View session",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    f.render_widget(block, area);
    // Session list area
    let list_area = Rect {
        x: area.x + 2,
        y: area.y + 1, // Start just below the block title
        width: area.width - 4,
        height: area.height.saturating_sub(3), // Leave space for help at the bottom
    };
    let items: Vec<ListItem> = state
        .sessions
        .iter()
        .map(|s| {
            // Parse the ISO datetime string properly
            let formatted_datetime = if let Ok(dt) =
                chrono::DateTime::parse_from_rfc3339(&s.updated_at.replace(" UTC", "+00:00"))
            {
                dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
            } else {
                // Fallback to string manipulation if parsing fails
                let parts = s.updated_at.split('T').collect::<Vec<_>>();
                let date = parts.first().unwrap_or(&"");
                let time = parts.get(1).and_then(|t| t.split('.').next()).unwrap_or("");
                format!("{} {} UTC", date, time)
            };

            let text = format!("{} . {}", formatted_datetime, s.title);
            ListItem::new(Line::from(vec![Span::raw(text)]))
        })
        .collect();
    let mut list_state = ListState::default();
    list_state.select(Some(state.session_selected));
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().fg(Color::Gray))
        .block(Block::default());
    f.render_stateful_widget(list, list_area, &mut list_state);

    // Help text at the bottom
    let help = "press enter to choose · esc to cancel";
    let help_area = Rect {
        x: area.x + 2,
        y: area.y + area.height - 2, // Second to last line of the dialog
        width: area.width - 4,
        height: 1,
    };
    let help_widget = Paragraph::new(help)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Left);
    f.render_widget(help_widget, help_area);
}
