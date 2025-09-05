use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SyntectColor, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::services::detect_term::should_use_rgb_colors;

fn syntect_color_to_ratatui_color(syntect_color: SyntectColor) -> Color {
    if should_use_rgb_colors() {
        Color::Rgb(syntect_color.r, syntect_color.g, syntect_color.b)
    } else {
        // For non-RGB terminals, use a simple cyan color
        Color::Cyan
    }
}

//  apply_syntax_highlighting -> takes string and optional extension and returns highlighted ratatui lines
pub fn apply_syntax_highlighting(text: &str, extension: Option<&str>) -> Vec<Line<'static>> {
    let syntax_set = SyntaxSet::load_defaults_newlines();
    let theme_set = ThemeSet::load_defaults();

    // Use a better default theme for code highlighting
    let theme = &theme_set.themes["base16-ocean.dark"];

    // add default extensions if none
    let extension = extension.or(Some("js"));
    let syntax = extension
        .and_then(|ext| syntax_set.find_syntax_by_extension(ext))
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();

    for line in LinesWithEndings::from(text) {
        let ranges = highlighter
            .highlight_line(line, &syntax_set)
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
