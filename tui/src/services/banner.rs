use crate::services::detect_term::ThemeColors;
use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub fn render_banner(f: &mut Frame, area: Rect) {
    let banner = Paragraph::new(Line::from(vec![Span::styled(
        "Welcome to Stakpak! Enter /init to get your system scanned",
        Style::default()
            .fg(ratatui::style::Color::White)
            .bg(ThemeColors::accent())
            .add_modifier(Modifier::BOLD),
    )]))
    .alignment(Alignment::Center);
    f.render_widget(banner, area);
}
