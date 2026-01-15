//! Disclaimer Popup
//!
//! Renders a blocking disclaimer popup with Yes/No buttons.
//! User navigates with left/right arrows and confirms with Enter.
//! This popup is shown when the --dangerously-skip-permissions flag is used.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::AppState;

const POPUP_WIDTH_PERCENT: u16 = 60;
const POPUP_HEIGHT: u16 = 14;

/// Calculate centered popup area
fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let popup_width = (area.width * percent_x) / 100;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, popup_width, height)
}

/// Render the disclaimer popup
pub fn render_disclaimer_popup(f: &mut Frame, state: &AppState) {
    if !state.show_disclaimer_popup {
        return;
    }

    let area = f.area();
    let popup_area = centered_rect(POPUP_WIDTH_PERCENT, POPUP_HEIGHT, area);

    // Clear background
    f.render_widget(Clear, popup_area);

    // Create popup block with warning border
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" ⚠ WARNING ")
        .title_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    // Layout for content and buttons
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // Message content
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Buttons
            Constraint::Length(1), // Spacer
        ])
        .split(inner);

    // Disclaimer message
    let message = vec![
        Line::from(""),
        Line::from(Span::styled(
            "DANGEROUS MODE ENABLED",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("You are about to run in skip-permissions mode."),
        Line::from("ALL tool calls will be automatically approved"),
        Line::from("without any confirmation prompts."),
        Line::from(""),
        Line::from(Span::styled(
            "This can be dangerous. Do you want to continue?",
            Style::default().fg(Color::Yellow),
        )),
    ];

    let message_widget = Paragraph::new(message).alignment(Alignment::Center);

    f.render_widget(message_widget, chunks[0]);

    // Render buttons
    let button_area = chunks[2];
    let button_width = 8u16;
    let total_buttons_width = button_width * 2 + 4; // 2 buttons + gap
    let start_x = button_area.x + (button_area.width.saturating_sub(total_buttons_width)) / 2;

    // Yes button
    let yes_style = if state.disclaimer_selected == 0 {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let yes_button = Paragraph::new("  Yes  ").style(yes_style);
    let yes_area = Rect::new(start_x, button_area.y, button_width, 1);
    f.render_widget(yes_button, yes_area);

    // No button
    let no_style = if state.disclaimer_selected == 1 {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red)
    };
    let no_button = Paragraph::new("   No  ").style(no_style);
    let no_area = Rect::new(start_x + button_width + 4, button_area.y, button_width, 1);
    f.render_widget(no_button, no_area);
}
