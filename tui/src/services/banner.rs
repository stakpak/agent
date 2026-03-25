use crate::app::AppState;
use crate::services::detect_term::ThemeColors;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    widgets::{Block, Borders, Paragraph},
};
use std::time::{Duration, Instant};

const BANNER_MESSAGE_DURATION: Duration = Duration::from_secs(60);

/// Height of the banner when visible: 1 line of text + 2 border lines.
const BANNER_VISIBLE_HEIGHT: u16 = 3;

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

/// Returns the banner height: `BANNER_VISIBLE_HEIGHT` when there is an active
/// (non-expired) message, `0` otherwise.
pub fn banner_height(state: &AppState) -> u16 {
    match &state.banner_message {
        Some(msg) if !msg.is_expired() => BANNER_VISIBLE_HEIGHT,
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
    let warning_style = Style::default().fg(ThemeColors::warning());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(warning_style);

    let paragraph = Paragraph::new(text).block(block).alignment(Alignment::Left);

    f.render_widget(paragraph, area);
}
