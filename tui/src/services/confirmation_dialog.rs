use crate::{
    app::AppState,
    services::{detect_term::should_use_rgb_colors, message::get_wrapped_message_lines_cached},
};
use ratatui::{
    Frame,
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

fn term_color(color: Color) -> Color {
    if should_use_rgb_colors() {
        color
    } else {
        Color::Reset
    }
}

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

    let mut message =
        "Press Enter to continue. '$' to run the command yourself or Esc to cancel and reprompt";
    if state.tool_call_count > 1 {
        message = "Press Enter to approve once. Ctrl+k to approve all. '$' to run the command yourself or Esc to cancel and reprompt.";
    }

    let line = Line::from(vec![Span::styled(
        message,
        Style::default()
            .fg(term_color(Color::White))
            .add_modifier(Modifier::BOLD),
    )]);

    let border_color = if should_use_rgb_colors() {
        Color::LightYellow
    } else {
        Color::Cyan
    };
    let dialog = Paragraph::new(vec![line])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title("Confirmation"),
        )
        .alignment(Alignment::Center);
    f.render_widget(dialog, area);
}
