use crate::{app::AppState, services::message::get_wrapped_message_lines_cached};
use ratatui::{
    Frame,
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn render_confirmation_dialog(f: &mut Frame, state: &mut AppState) {
    let screen = f.area();
    let message_lines = get_wrapped_message_lines_cached(state, screen.width as usize);
    let mut last_message_y = message_lines.len() as u16 + 1; // +1 for a gap

    // Fixed dialog height: just 3 lines (border, message, border)
    let dialog_height = 3;

    // Clamp so dialog fits on screen
    if last_message_y + dialog_height > screen.height {
        last_message_y = screen.height.saturating_sub(dialog_height + 3);
    }

    let area = ratatui::layout::Rect {
        x: 1,
        y: last_message_y,
        width: screen.width - 2,
        height: dialog_height,
    };

    let message =
        "Press Enter to continue. '$' to run the command yourself or Esc to cancel and reprompt";

    let line = Line::from(vec![Span::styled(
        message,
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
