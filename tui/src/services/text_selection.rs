//! Text selection module for mouse-based text selection in the TUI.
//!
//! This module provides:
//! - SelectionState: tracks active selection bounds
//! - Text extraction: converts selection to plain text, excluding borders
//! - Clipboard operations: copy selected text to system clipboard
//! - Highlight rendering: applies selection highlighting to visible lines

use crate::app::AppState;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// Characters that are considered borders/decorations and should be excluded from selection
/// NOTE: We only exclude Unicode box-drawing characters, NOT ASCII '|', '-', '+'
/// because those are commonly used in content (markdown tables, code, etc.)
const BORDER_CHARS: &[char] = &[
    // Light box drawing
    '│', '─', '╭', '╮', '╰', '╯', '├', '┤', '┬', '┴', '┼', '┌', '┐', '└', '┘',
    // Heavy/thick box drawing (used for message prefixes)
    '┃', '━', '┏', '┓', '┗', '┛', '┣', '┫', '┳', '┻', '╋', // Double box drawing
    '║', '═', '╔', '╗', '╚', '╝', '╟', '╢', '╤', '╧', '╠', '╣', '╦', '╩', '╬',
];

/// State for tracking text selection
#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    /// Whether selection is currently active
    pub active: bool,
    /// Starting line index (absolute, not screen-relative)
    pub start_line: Option<usize>,
    /// Starting column
    pub start_col: Option<u16>,
    /// Ending line index (absolute, not screen-relative)
    pub end_line: Option<usize>,
    /// Ending column
    pub end_col: Option<u16>,
}

impl SelectionState {
    /// Get normalized selection bounds (start always before end)
    pub fn normalized_bounds(&self) -> Option<(usize, u16, usize, u16)> {
        match (self.start_line, self.start_col, self.end_line, self.end_col) {
            (Some(sl), Some(sc), Some(el), Some(ec)) => {
                if sl < el || (sl == el && sc <= ec) {
                    Some((sl, sc, el, ec))
                } else {
                    Some((el, ec, sl, sc))
                }
            }
            _ => None,
        }
    }

    /// Check if a given line is within the selection
    pub fn line_in_selection(&self, line_idx: usize) -> bool {
        if let Some((start_line, _, end_line, _)) = self.normalized_bounds() {
            line_idx >= start_line && line_idx <= end_line
        } else {
            false
        }
    }

    /// Get column range for a specific line within selection
    pub fn column_range_for_line(&self, line_idx: usize, line_width: u16) -> Option<(u16, u16)> {
        let (start_line, start_col, end_line, end_col) = self.normalized_bounds()?;

        if line_idx < start_line || line_idx > end_line {
            return None;
        }

        let col_start = if line_idx == start_line { start_col } else { 0 };
        let col_end = if line_idx == end_line {
            end_col
        } else {
            line_width
        };

        Some((col_start, col_end))
    }
}

/// Check if a character is a border/decoration character
fn is_border_char(c: char) -> bool {
    BORDER_CHARS.contains(&c)
}

/// Calculate the display width of the decorative prefix at the start of a line.
///
/// Decorative prefixes are border characters (e.g. `┃`, `│`) optionally followed by a space,
/// used for user message bars, quote bars, and shell bubble borders. This function returns
/// how many display columns the prefix occupies so we can strip only the decoration residue
/// (the trailing space after the border char) without stripping meaningful content indentation.
///
/// For example:
/// - `"┃ some content"` → returns 2 (border char width 1 + space 1)
/// - `"│ quoted text"` → returns 2
/// - `"    indented code"` → returns 0 (no border char, so all whitespace is content)
fn decorative_prefix_width(line: &Line) -> u16 {
    let mut width: u16 = 0;
    let mut found_border = false;

    for span in &line.spans {
        for c in span.content.chars() {
            if is_border_char(c) {
                found_border = true;
                width += unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) as u16;
            } else if c == ' ' && found_border {
                // Count one trailing space after border chars as decorative
                width += 1;
                return width;
            } else {
                // Non-border, non-space char (or space without preceding border): content starts
                return if found_border { width } else { 0 };
            }
        }
    }
    if found_border { width } else { 0 }
}

/// Extract plain text from a Line, excluding border characters
fn extract_text_from_line(line: &Line, start_col: u16, end_col: u16) -> String {
    let mut result = String::new();
    let mut current_col: u16 = 0;

    for span in &line.spans {
        for c in span.content.chars() {
            let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) as u16;

            // Check if this character is within selection range
            if current_col >= start_col && current_col < end_col {
                // Skip border characters
                if !is_border_char(c) {
                    result.push(c);
                }
            }

            current_col += char_width;

            // Stop if we're past the end
            if current_col > end_col {
                break;
            }
        }
    }

    result
}

/// Strip only the decorative prefix residue from extracted line text.
///
/// After `extract_text_from_line` removes border characters, lines that had a decorative prefix
/// (e.g. "┃ content") are left with residual whitespace (e.g. " content"). This function strips
/// only that residual whitespace — the number of spaces equal to `(prefix_width - border_chars_width)`.
/// For lines without a decorative prefix (e.g. code blocks), all leading whitespace is preserved
/// since it represents meaningful indentation.
fn strip_decorative_prefix_residue(line_text: &str, prefix_width: u16, line: &Line) -> String {
    if prefix_width == 0 {
        // No decorative prefix — preserve all leading whitespace (code blocks, plain text)
        return line_text.trim_end().to_string();
    }

    // Count how many display columns of border chars are in the prefix.
    // The residual whitespace = prefix_width - border_char_columns.
    let mut border_char_cols: u16 = 0;
    'outer: for span in &line.spans {
        for c in span.content.chars() {
            if is_border_char(c) {
                border_char_cols += unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) as u16;
            } else {
                break 'outer;
            }
        }
    }

    let residual_spaces = prefix_width.saturating_sub(border_char_cols) as usize;

    // Strip exactly the residual number of leading spaces
    let mut chars = line_text.chars();
    let mut stripped = 0;
    while stripped < residual_spaces {
        match chars.next() {
            Some(' ') => stripped += 1,
            _ => break,
        }
    }

    chars.as_str().trim_end().to_string()
}

/// Extract selected text from a slice of cached lines using the current selection bounds
fn extract_selected_text_from_lines(selection: &SelectionState, cached_lines: &[Line]) -> String {
    let Some((start_line, start_col, end_line, end_col)) = selection.normalized_bounds() else {
        return String::new();
    };

    let mut result = String::new();

    for line_idx in start_line..=end_line {
        if line_idx >= cached_lines.len() {
            break;
        }

        let line = &cached_lines[line_idx];
        let line_width = line_display_width(line);

        // Determine column range for this line
        let col_start = if line_idx == start_line { start_col } else { 0 };
        let col_end = if line_idx == end_line {
            end_col
        } else {
            line_width
        };

        let line_text = extract_text_from_line(line, col_start, col_end);

        // Skip SPACING_MARKER lines (internal rendering markers that should be treated as empty)
        let line_text = if line_text.trim() == "SPACING_MARKER" {
            String::new()
        } else {
            // Strip only decorative prefix residue, preserving meaningful indentation
            let prefix_width = decorative_prefix_width(line);
            strip_decorative_prefix_residue(&line_text, prefix_width, line)
        };

        if !line_text.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&line_text);
        } else if line_idx > start_line && line_idx < end_line {
            // Preserve empty lines within selection (but not at boundaries)
            result.push('\n');
        }
    }

    result
}

/// Extract selected text from the assembled lines cache (main message area)
pub fn extract_selected_text(state: &AppState) -> String {
    let Some((_, cached_lines, _)) = &state.messages_scrolling_state.assembled_lines_cache else {
        return String::new();
    };

    extract_selected_text_from_lines(&state.message_interaction_state.selection, cached_lines)
}

/// Extract selected text from the collapsed message lines cache (fullscreen popup)
pub fn extract_selected_text_from_collapsed(state: &AppState) -> String {
    let Some((_, _, cached_lines)) = &state.messages_scrolling_state.collapsed_message_lines_cache else {
        return String::new();
    };

    extract_selected_text_from_lines(&state.message_interaction_state.selection, cached_lines)
}

/// Calculate display width of a line
fn line_display_width(line: &Line) -> u16 {
    line.spans
        .iter()
        .map(|span| unicode_width::UnicodeWidthStr::width(span.content.as_ref()) as u16)
        .sum()
}

/// Copy text to system clipboard using arboard
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;

    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("Failed to copy to clipboard: {}", e))?;

    Ok(())
}

/// Apply selection highlighting to visible lines
pub fn apply_selection_highlight<'a>(
    lines: Vec<Line<'a>>,
    selection: &SelectionState,
    scroll: usize,
) -> Vec<Line<'a>> {
    if !selection.active {
        return lines;
    }

    let Some((start_line, start_col, end_line, end_col)) = selection.normalized_bounds() else {
        return lines;
    };

    lines
        .into_iter()
        .enumerate()
        .map(|(screen_row, line)| {
            let absolute_line = scroll + screen_row;

            // Check if this line is in selection
            if absolute_line < start_line || absolute_line > end_line {
                return line;
            }

            // Determine column range for this line
            let line_width = line_display_width(&line);
            let col_start = if absolute_line == start_line {
                start_col
            } else {
                0
            };
            let col_end = if absolute_line == end_line {
                end_col
            } else {
                line_width
            };

            highlight_line_range(line, col_start, col_end)
        })
        .collect()
}

/// Highlight a range within a line by inverting colors
fn highlight_line_range(line: Line<'_>, start_col: u16, end_col: u16) -> Line<'_> {
    let mut new_spans: Vec<Span> = Vec::new();
    let mut current_col: u16 = 0;

    for span in line.spans {
        let span_start = current_col;
        let span_width = unicode_width::UnicodeWidthStr::width(span.content.as_ref()) as u16;
        let span_end = span_start + span_width;

        // Check overlap with selection
        if span_end <= start_col || span_start >= end_col {
            // No overlap - keep original
            new_spans.push(span);
        } else if span_start >= start_col && span_end <= end_col {
            // Fully within selection - highlight entire span
            new_spans.push(Span::styled(span.content, get_highlight_style(span.style)));
        } else {
            // Partial overlap - need to split span
            let content = span.content.to_string();
            let chars: Vec<char> = content.chars().collect();
            let mut char_col = span_start;
            let mut segment_start = 0;
            let mut in_selection = char_col >= start_col && char_col < end_col;

            for (i, c) in chars.iter().enumerate() {
                let char_width = unicode_width::UnicodeWidthChar::width(*c).unwrap_or(1) as u16;
                let next_col = char_col + char_width;
                let next_in_selection = next_col > start_col && char_col < end_col;

                // Check if we're transitioning selection state
                if next_in_selection != in_selection || i == chars.len() - 1 {
                    let segment_end = if i == chars.len() - 1 { i + 1 } else { i };
                    if segment_end > segment_start {
                        let segment: String = chars[segment_start..segment_end].iter().collect();
                        let style = if in_selection {
                            get_highlight_style(span.style)
                        } else {
                            span.style
                        };
                        new_spans.push(Span::styled(segment, style));
                    }
                    segment_start = segment_end;
                    in_selection = next_in_selection;
                }

                char_col = next_col;
            }

            // Handle remaining segment
            if segment_start < chars.len() {
                let segment: String = chars[segment_start..].iter().collect();
                let style = if in_selection {
                    get_highlight_style(span.style)
                } else {
                    span.style
                };
                new_spans.push(Span::styled(segment, style));
            }
        }

        current_col = span_end;
    }

    Line::from(new_spans).style(line.style)
}

/// Get highlight style by using text color as background
fn get_highlight_style(original: Style) -> Style {
    // Use the foreground color as background
    let bg = original.fg.unwrap_or(Color::White);

    // Calculate contrasting foreground
    let fg = if is_light_color(bg) {
        Color::Black
    } else {
        Color::White
    };

    Style::default().fg(fg).bg(bg)
}

/// Check if a color is considered "light" for contrast calculation
fn is_light_color(color: Color) -> bool {
    match color {
        Color::Rgb(r, g, b) => {
            // Luminance formula
            (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) > 128.0
        }
        Color::White
        | Color::LightYellow
        | Color::LightCyan
        | Color::LightGreen
        | Color::LightBlue
        | Color::LightMagenta
        | Color::LightRed
        | Color::Gray => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;
    use ratatui::text::{Line, Span};

    /// Helper: create a selection covering all given lines fully
    fn full_selection(start_line: usize, end_line: usize, end_col: u16) -> SelectionState {
        SelectionState {
            active: true,
            start_line: Some(start_line),
            start_col: Some(0),
            end_line: Some(end_line),
            end_col: Some(end_col),
        }
    }

    // -- decorative_prefix_width tests --

    #[test]
    fn test_decorative_prefix_width_user_message() {
        // User message line: "┃ Hello world"
        let line = Line::from(vec![
            Span::styled("┃ ", Style::default()),
            Span::raw("Hello world"),
        ]);
        assert_eq!(decorative_prefix_width(&line), 2);
    }

    #[test]
    fn test_decorative_prefix_width_quote() {
        // Quote line: "│ quoted text"
        let line = Line::from(vec![
            Span::styled("│ ", Style::default()),
            Span::raw("quoted text"),
        ]);
        assert_eq!(decorative_prefix_width(&line), 2);
    }

    #[test]
    fn test_decorative_prefix_width_code_block_no_indent() {
        // Code block line with no indentation: "fn main() {"
        let line = Line::from(vec![Span::raw("fn main() {")]);
        assert_eq!(decorative_prefix_width(&line), 0);
    }

    #[test]
    fn test_decorative_prefix_width_code_block_with_indent() {
        // Code block line with indentation: "    let x = 1;"
        let line = Line::from(vec![Span::raw("    let x = 1;")]);
        assert_eq!(decorative_prefix_width(&line), 0);
    }

    #[test]
    fn test_decorative_prefix_width_empty_line() {
        let line = Line::from(vec![Span::raw("")]);
        assert_eq!(decorative_prefix_width(&line), 0);
    }

    #[test]
    fn test_decorative_prefix_width_border_only() {
        // Line that is just a border char (e.g. part of a box top)
        let line = Line::from(vec![Span::raw("┃")]);
        assert_eq!(decorative_prefix_width(&line), 1);
    }

    // -- strip_decorative_prefix_residue tests --

    #[test]
    fn test_strip_residue_no_prefix() {
        // Code block: no decorative prefix, preserve indentation
        let line = Line::from(vec![Span::raw("    name: Stakpak Dev")]);
        let extracted = " name: Stakpak Dev"; // after border filter (no borders here, but simulating)
        let result = strip_decorative_prefix_residue(extracted, 0, &line);
        assert_eq!(result, " name: Stakpak Dev");
    }

    #[test]
    fn test_strip_residue_user_message() {
        // User message: "┃ Hello" → after border removal → " Hello"
        let line = Line::from(vec![
            Span::styled("┃ ", Style::default()),
            Span::raw("Hello"),
        ]);
        let extracted = " Hello"; // border char removed, space remains
        let result = strip_decorative_prefix_residue(extracted, 2, &line);
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_strip_residue_preserves_content_indent_after_prefix() {
        // User message with indented content: "┃   indented" → " indented" after border removal
        // prefix_width=2 (border+space), border_char_cols=1, residual=1
        // So strip 1 space → "  indented" (2 spaces of content indent remain)
        let line = Line::from(vec![
            Span::styled("┃ ", Style::default()),
            Span::raw("  indented"),
        ]);
        let extracted = "   indented"; // 3 spaces: 1 from prefix + 2 content
        let result = strip_decorative_prefix_residue(extracted, 2, &line);
        assert_eq!(result, "  indented");
    }

    // -- extract_selected_text_from_lines integration tests --

    #[test]
    fn test_extract_code_block_preserves_indentation() {
        // Simulate a YAML code block with indentation (no decorative prefix)
        let lines = vec![
            Line::from(vec![Span::raw("name: Stakpak Dev")]),
            Line::from(vec![Span::raw("  description: AI agent")]),
            Line::from(vec![Span::raw("  features:")]),
            Line::from(vec![Span::raw("    - infrastructure")]),
            Line::from(vec![Span::raw("    - kubernetes")]),
        ];
        let selection = full_selection(0, 4, 50);
        let result = extract_selected_text_from_lines(&selection, &lines);
        assert_eq!(
            result,
            "name: Stakpak Dev\n  description: AI agent\n  features:\n    - infrastructure\n    - kubernetes"
        );
    }

    #[test]
    fn test_extract_user_message_strips_prefix() {
        // User message with "┃ " prefix
        let lines = vec![
            Line::from(vec![
                Span::styled("┃ ", Style::default()),
                Span::raw("Hello world"),
            ]),
            Line::from(vec![
                Span::styled("┃ ", Style::default()),
                Span::raw("How are you?"),
            ]),
        ];
        let selection = full_selection(0, 1, 50);
        let result = extract_selected_text_from_lines(&selection, &lines);
        assert_eq!(result, "Hello world\nHow are you?");
    }

    #[test]
    fn test_extract_mixed_code_and_messages() {
        // Simulates a mix: user message line followed by code block lines
        let lines = vec![
            Line::from(vec![
                Span::styled("┃ ", Style::default()),
                Span::raw("Here is my config:"),
            ]),
            Line::from(vec![Span::raw("server:")]),
            Line::from(vec![Span::raw("  port: 8080")]),
            Line::from(vec![Span::raw("  host: localhost")]),
        ];
        let selection = full_selection(0, 3, 50);
        let result = extract_selected_text_from_lines(&selection, &lines);
        assert_eq!(
            result,
            "Here is my config:\nserver:\n  port: 8080\n  host: localhost"
        );
    }

    #[test]
    fn test_extract_quote_strips_prefix() {
        // Quote line with "│ " prefix
        let lines = vec![Line::from(vec![
            Span::styled("│ ", Style::default()),
            Span::raw("This is a quote"),
        ])];
        let selection = full_selection(0, 0, 50);
        let result = extract_selected_text_from_lines(&selection, &lines);
        assert_eq!(result, "This is a quote");
    }

    #[test]
    fn test_extract_preserves_empty_lines_in_code() {
        let lines = vec![
            Line::from(vec![Span::raw("fn main() {")]),
            Line::from(vec![Span::raw("")]),
            Line::from(vec![Span::raw("    println!(\"hello\");")]),
            Line::from(vec![Span::raw("}")]),
        ];
        let selection = full_selection(0, 3, 50);
        let result = extract_selected_text_from_lines(&selection, &lines);
        assert_eq!(result, "fn main() {\n\n    println!(\"hello\");\n}");
    }

    #[test]
    fn test_extract_spacing_marker_treated_as_empty() {
        let lines = vec![
            Line::from(vec![Span::raw("some text")]),
            Line::from(vec![Span::raw("SPACING_MARKER")]),
            Line::from(vec![Span::raw("more text")]),
        ];
        let selection = full_selection(0, 2, 50);
        let result = extract_selected_text_from_lines(&selection, &lines);
        assert_eq!(result, "some text\n\nmore text");
    }

    #[test]
    fn test_extract_python_indentation_preserved() {
        // Python code block — indentation is critical
        let lines = vec![
            Line::from(vec![Span::raw("def foo():")]),
            Line::from(vec![Span::raw("    if True:")]),
            Line::from(vec![Span::raw("        return 1")]),
            Line::from(vec![Span::raw("    return 0")]),
        ];
        let selection = full_selection(0, 3, 50);
        let result = extract_selected_text_from_lines(&selection, &lines);
        assert_eq!(
            result,
            "def foo():\n    if True:\n        return 1\n    return 0"
        );
    }
}
