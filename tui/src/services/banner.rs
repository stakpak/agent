use crate::app::AppState;
use crate::services::detect_term::ThemeColors;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::time::{Duration, Instant};

const BANNER_MESSAGE_DURATION: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct BannerMessage {
    pub text: String,
    pub created_at: Instant,
}

impl BannerMessage {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            created_at: Instant::now(),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > BANNER_MESSAGE_DURATION
    }
}

/// Returns 1 if there is an active (non-expired) banner message, 0 otherwise.
pub fn banner_height(state: &AppState) -> u16 {
    match &state.banner_message {
        Some(msg) if !msg.is_expired() => 1,
        _ => 0,
    }
}

pub fn render_banner(f: &mut Frame, area: Rect, state: &mut AppState) {
    // Clear expired message
    if let Some(msg) = &state.banner_message
        && msg.is_expired()
    {
        state.banner_message = None;
    }

    // No message — nothing to render
    let Some(msg) = &state.banner_message else {
        return;
    };

    let text = msg.text.clone();
    let banner = Paragraph::new(Line::from(vec![Span::styled(
        text,
        Style::default()
            .fg(ratatui::style::Color::White)
            .bg(ThemeColors::accent())
            .add_modifier(Modifier::BOLD),
    )]))
    .alignment(Alignment::Center);
    f.render_widget(banner, area);
}
