use crate::app::AppState;
use crate::services::detect_term::ThemeColors;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
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

fn find_slash_commands(text: &str) -> Vec<(usize, String)> {
    let mut commands = Vec::new();
    let mut chars = text.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        if c == '/' {
            let is_word_start = i == 0
                || text[..i]
                    .chars()
                    .last()
                    .is_some_and(|prev| prev.is_whitespace());

            if is_word_start {
                let start = i;
                let mut end = i + 1;
                while let Some(&(j, ch)) = chars.peek() {
                    if ch.is_whitespace() {
                        break;
                    }
                    end = j + ch.len_utf8();
                    chars.next();
                }
                let cmd = text[start..end].to_string();

                if cmd.len() > 1 {
                    commands.push((start, cmd));
                }
            }
        }
    }
    commands
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

    let border_style = Style::default().fg(msg.style.color());
    let accent_color = ThemeColors::accent();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style);

    // Build styled spans with clickable commands
    let commands = find_slash_commands(&msg.text);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut click_regions: Vec<(String, Rect)> = Vec::new();

    // Track x offset: +1 for left border
    let mut char_x: u16 = 1;
    let mut byte_offset: usize = 0;

    if commands.is_empty() {
        // No commands — plain text
        spans.push(Span::raw(msg.text.clone()));
    } else {
        for (cmd_start, cmd) in &commands {
            // Text before the command
            if *cmd_start > byte_offset {
                let plain = &msg.text[byte_offset..*cmd_start];
                char_x += plain.chars().count() as u16;
                spans.push(Span::raw(plain.to_string()));
            }

            // The command itself — styled as clickable
            let cmd_width = cmd.chars().count() as u16;
            let cmd_rect = Rect::new(
                area.x.saturating_add(char_x),
                area.y.saturating_add(1), // +1 for top border
                cmd_width,
                1,
            );
            click_regions.push((cmd.clone(), cmd_rect));

            let styled = Span::styled(
                cmd.clone(),
                Style::default()
                    .fg(accent_color)
                    .add_modifier(Modifier::UNDERLINED),
            );
            spans.push(styled);

            char_x += cmd_width;
            byte_offset = *cmd_start + cmd.len();
        }

        // Remaining text after last command
        if byte_offset < msg.text.len() {
            spans.push(Span::raw(msg.text[byte_offset..].to_string()));
        }
    }

    let paragraph = Paragraph::new(Line::from(spans))
        .block(block)
        .alignment(Alignment::Left);

    f.render_widget(paragraph, area);
    state.banner_click_regions = click_regions;
}
