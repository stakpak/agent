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
    /// When true, the banner ignores the duration-based timeout and stays
    /// visible until explicitly dismissed (e.g. by user action or first message).
    pub persistent: bool,
    /// When set, clicking anywhere on the banner executes this command
    /// (e.g. "/init"). Makes the entire banner act as a single CTA.
    pub action: Option<String>,
}

impl BannerMessage {
    pub fn new(text: impl Into<String>, style: BannerStyle) -> Self {
        Self {
            text: text.into(),
            created_at: Instant::now(),
            style,
            persistent: false,
            action: None,
        }
    }

    /// Create a banner that stays visible until explicitly dismissed.
    pub fn persistent(text: impl Into<String>, style: BannerStyle) -> Self {
        Self {
            text: text.into(),
            created_at: Instant::now(),
            style,
            persistent: true,
            action: None,
        }
    }

    /// Create a persistent banner where clicking anywhere triggers the given command.
    pub fn persistent_with_action(
        text: impl Into<String>,
        style: BannerStyle,
        action: impl Into<String>,
    ) -> Self {
        Self {
            text: text.into(),
            created_at: Instant::now(),
            style,
            persistent: true,
            action: Some(action.into()),
        }
    }

    pub fn is_expired(&self) -> bool {
        if self.persistent {
            return false;
        }
        self.created_at.elapsed() > BANNER_MESSAGE_DURATION
    }
}

/// Returns the banner height: `BANNER_VISIBLE_HEIGHT` when there is an active
/// (non-expired) message, `0` otherwise.
pub fn banner_height(state: &AppState) -> u16 {
    match &state.banner_state.message {
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
    if let Some(msg) = &state.banner_state.message
        && msg.is_expired()
    {
        state.banner_state.message = None;
    }

    // No message — nothing to render
    let Some(msg) = &state.banner_state.message else {
        return;
    };

    let border_style = Style::default().fg(msg.style.color());
    let accent_color = ThemeColors::accent();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .padding(ratatui::widgets::Padding::horizontal(1));

    // Build styled spans: slash-commands get accent color + underline,
    // everything else is plain text.
    let commands = find_slash_commands(&msg.text);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut per_cmd_regions: Vec<(String, Rect)> = Vec::new();

    // Track x offset: +1 for left border, +1 for horizontal padding
    let mut char_x: u16 = 2;
    let mut byte_offset: usize = 0;

    if commands.is_empty() {
        spans.push(Span::raw(msg.text.clone()));
    } else {
        for (cmd_start, cmd) in &commands {
            if *cmd_start > byte_offset {
                let plain = &msg.text[byte_offset..*cmd_start];
                char_x += plain.chars().count() as u16;
                spans.push(Span::raw(plain.to_string()));
            }

            let cmd_width = cmd.chars().count() as u16;
            let cmd_rect = Rect::new(
                area.x.saturating_add(char_x),
                area.y.saturating_add(1), // +1 for top border
                cmd_width,
                1,
            );
            per_cmd_regions.push((cmd.clone(), cmd_rect));

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

        if byte_offset < msg.text.len() {
            spans.push(Span::raw(msg.text[byte_offset..].to_string()));
        }
    }

    // When the banner has an action, the entire banner area is clickable
    // (even though only the slash-command text is visually underlined).
    let click_regions = if let Some(action) = &msg.action {
        vec![(action.clone(), area)]
    } else {
        per_cmd_regions
    };

    // Append right-aligned dismiss button: pad with spaces then render " x "
    // Content width = area width - 2 (borders) - 2 (horizontal padding)
    let content_width = area.width.saturating_sub(4) as usize;
    let text_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let dismiss_label = " x ";
    let pad = content_width.saturating_sub(text_width + dismiss_label.len());
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
    spans.push(Span::styled(
        dismiss_label.to_string(),
        Style::default()
            .fg(msg.style.color())
            .add_modifier(Modifier::DIM),
    ));

    // Dismiss click target covers the " x " label plus some padding around it
    // for a more forgiving click area (5 chars wide).
    let dismiss_width: u16 = 5;
    let dismiss_x = area.x + area.width.saturating_sub(2 + dismiss_width); // border(1) + padding(1) + target
    let dismiss_y = area.y; // cover the full banner height for easier clicking
    state.banner_state.dismiss_region = Some(Rect::new(
        dismiss_x,
        dismiss_y,
        dismiss_width + 2,
        area.height,
    ));

    let paragraph = Paragraph::new(Line::from(spans))
        .block(block)
        .alignment(Alignment::Left);

    f.render_widget(paragraph, area);
    state.banner_state.click_regions = click_regions;
}
