use crate::app::AppState;
use crate::services::detect_term::ThemeColors;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};
use std::time::{Duration, Instant};

const BANNER_MESSAGE_DURATION: Duration = Duration::from_secs(60);

/// Height of the banner when visible: 1 line of text + 2 border lines.
const BANNER_VISIBLE_HEIGHT: u16 = 3;

/// Visual style variants for banners
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerStyle {
    /// Warning message (yellow border)
    Warning,
    /// Error message (red border)
    Error,
    /// Informational message (cyan border)
    Info,
    /// Success message (green border)
    Success,
}

impl BannerStyle {
    pub fn color(&self) -> Color {
        match self {
            BannerStyle::Warning => ThemeColors::warning(),
            BannerStyle::Error => ThemeColors::danger(),
            BannerStyle::Info => ThemeColors::accent(),
            BannerStyle::Success => ThemeColors::success(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BannerMessage {
    pub text: String,
    pub created_at: Instant,
    pub style: BannerStyle,
}

impl BannerMessage {
    pub fn new(text: impl Into<String>, style: BannerStyle) -> Self {
        Self {
            text: text.into(),
            created_at: Instant::now(),
            style,
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
    let border_style = Style::default().fg(msg.style.color());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style);

    let paragraph = Paragraph::new(text).block(block).alignment(Alignment::Left);

    f.render_widget(paragraph, area);
}
