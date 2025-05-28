use crate::app::AppState;
use crate::services::message::get_wrapped_message_lines;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

pub fn render_sessions_dialog(f: &mut Frame, state: &AppState, message_area: Rect) {
    let screen = f.area();
    let max_height = message_area.height.saturating_sub(2).min(20);
    let session_count = state.sessions.len() as u16;
    let dialog_height = (session_count + 3).min(max_height);

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
    // Help text
    let help = "press enter to choose · esc to cancel";
    let help_area = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width - 4,
        height: 1,
    };
    let help_widget = Paragraph::new(help)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Left);
    f.render_widget(help_widget, help_area);
    // Session list area
    let list_area = Rect {
        x: area.x + 2,
        y: area.y + 3,
        width: area.width - 4,
        height: area.height.saturating_sub(4),
    };
    let items: Vec<ListItem> = state
        .sessions
        .iter()
        .map(|s| {
            let text = format!("{} . {}", s.updated_at, s.title);
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
}
