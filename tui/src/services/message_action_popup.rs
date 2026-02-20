//! Message Action Popup
//!
//! A popup that appears when left-clicking on a user message.
//! Provides actions like copying the message text or reverting to that point.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use uuid::Uuid;

use crate::app::AppState;

/// The menu items in the message action popup
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageAction {
    CopyMessage,
    RevertToMessage,
}

impl MessageAction {
    pub fn all() -> Vec<Self> {
        vec![Self::CopyMessage, Self::RevertToMessage]
    }
}

/// Render the message action popup - centered on screen like file_changes_popup
pub fn render_message_action_popup(f: &mut Frame, state: &AppState) {
    if !state.show_message_action_popup {
        return;
    }

    // Calculate popup size - centered, max width 50, height for 2 items + title + padding
    let popup_width: u16 = 50;
    let popup_height: u16 = 7; // Title + 2 items + borders + padding

    let terminal_area = f.area();
    let x = (terminal_area.width.saturating_sub(popup_width)) / 2;
    let y = (terminal_area.height.saturating_sub(popup_height)) / 2;

    let area = Rect::new(x, y, popup_width, popup_height);

    // Clear the area behind the popup
    f.render_widget(Clear, area);

    // Create the main block with border (Cyan like file_changes_popup)
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    f.render_widget(block, area);

    // Inner area (inside borders)
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
            Constraint::Length(1), // Spacing
            Constraint::Min(2),    // Items
        ])
        .split(inner_area);

    // Render title - Yellow Bold like file_changes_popup
    let title = Paragraph::new(Line::from(vec![Span::styled(
        " Message Action",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )]));
    f.render_widget(title, chunks[0]);

    // Render menu items
    let actions = MessageAction::all();
    let mut item_lines: Vec<Line> = Vec::new();

    for (idx, action) in actions.iter().enumerate() {
        let is_selected = idx == state.message_action_popup_selected;

        let (highlight_word, rest_text) = match action {
            MessageAction::CopyMessage => ("Copy", " message text to clipboard"),
            MessageAction::RevertToMessage => ("Revert", " undo messages and file changes"),
        };

        // Full width background for selected item
        let available_width = (inner_area.width as usize).saturating_sub(2);
        let text_len = 2 + highlight_word.len() + rest_text.len(); // "  " prefix + text
        let padding = available_width.saturating_sub(text_len);

        let line = if is_selected {
            Line::from(vec![
                Span::styled("  ", Style::default().bg(Color::Cyan).fg(Color::Black)),
                Span::styled(
                    highlight_word,
                    Style::default()
                        .bg(Color::Cyan)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(rest_text, Style::default().bg(Color::Cyan).fg(Color::Black)),
                Span::styled(
                    " ".repeat(padding),
                    Style::default().bg(Color::Cyan).fg(Color::Black),
                ),
            ])
        } else {
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    highlight_word,
                    Style::default().fg(Color::Reset), // Reset color for highlight word
                ),
                Span::styled(rest_text, Style::default().fg(Color::DarkGray)),
            ])
        };

        item_lines.push(line);
    }

    let items = Paragraph::new(item_lines);
    f.render_widget(items, chunks[2]);
}

/// Get the currently selected action
pub fn get_selected_action(state: &AppState) -> Option<MessageAction> {
    let actions = MessageAction::all();
    actions.get(state.message_action_popup_selected).copied()
}

/// Find the user message at a given absolute line index
/// Returns (message_id, message_text) if found
/// Uses the line_to_message_map that was built during rendering
pub fn find_user_message_at_line(state: &AppState, absolute_line: usize) -> Option<(Uuid, String)> {
    // Search through the line-to-message map to find which user message contains this line
    for (start_line, end_line, msg_id, is_user, text, _user_idx) in &state.line_to_message_map {
        if *is_user && absolute_line >= *start_line && absolute_line < *end_line {
            return Some((*msg_id, text.clone()));
        }
    }

    None
}
