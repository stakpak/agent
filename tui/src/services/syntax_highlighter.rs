use std::sync::LazyLock;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SyntectColor, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::services::detect_term::{is_light_mode, should_use_rgb_colors, ThemeColors};

/// Cached syntax definitions — deserialized once from embedded binary data.
static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

/// Cached syntax-highlighting themes — deserialized once from embedded binary data.
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

fn syntect_color_to_ratatui_color(syntect_color: SyntectColor) -> Color {
    if should_use_rgb_colors() {
        Color::Rgb(syntect_color.r, syntect_color.g, syntect_color.b)
    } else {
        // For non-RGB terminals, use a theme-aware cyan color
        ThemeColors::cyan()
    }
}

//  apply_syntax_highlighting -> takes string and optional extension and returns highlighted ratatui lines
pub fn apply_syntax_highlighting(text: &str, extension: Option<&str>) -> Vec<Line<'static>> {
    let syntax_set = &*SYNTAX_SET;
    let theme_set = &*THEME_SET;

    // Select theme based on terminal background color
    // base16-ocean.light has darker colors suitable for light backgrounds
    // base16-ocean.dark has lighter colors suitable for dark backgrounds
    let theme_name = if is_light_mode() {
        "base16-ocean.light"
    } else {
        "base16-ocean.dark"
    };
    let theme = &theme_set.themes[theme_name];

    // add default extensions if none
    let extension = extension.or(Some("js"));
    let syntax = extension
        .and_then(|ext| syntax_set.find_syntax_by_extension(ext))
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();

    for line in LinesWithEndings::from(text) {
        let ranges = highlighter
            .highlight_line(line, syntax_set)
            .unwrap_or_else(|_| vec![(syntect::highlighting::Style::default(), line)]);
        let mut spans = Vec::new();

        for (style, text) in ranges {
            let color = syntect_color_to_ratatui_color(style.foreground);
            // Use only foreground color for better compatibility
            let styled_span = Span::styled(
                text.to_string(),
                Style::default().fg(color), // Only foreground color, no background
            );
            spans.push(styled_span);
        }

        lines.push(Line::from(spans));
    }

    lines
}
