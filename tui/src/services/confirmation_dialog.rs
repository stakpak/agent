use crate::app::AppState;
use crate::services::message::get_wrapped_message_lines;
use ratatui::{
    Frame,
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn render_confirmation_dialog(f: &mut Frame, state: &AppState) {
    let screen = f.area();
    let message_lines = get_wrapped_message_lines(&state.messages, screen.width as usize);
    let mut last_message_y = message_lines.len() as u16 + 1; // +1 for a gap

    // Fixed dialog height: just 3 lines (border, message, border)
    let dialog_height = 3;

    // Clamp so dialog fits on screen
    if last_message_y + dialog_height > screen.height {
        last_message_y = screen.height.saturating_sub(dialog_height + 1);
    }

    let area = ratatui::layout::Rect {
        x: 1,
        y: last_message_y,
        width: screen.width - 2,
        height: dialog_height,
    };

    let line = Line::from(vec![Span::styled(
        "Press Enter to continue or Esc to cancel",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )]);
    let dialog = Paragraph::new(vec![line])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightYellow))
                .title("Confirmation"),
        )
        .alignment(Alignment::Center);
    f.render_widget(dialog, area);
}
