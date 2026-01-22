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

/// Extract selected text from the assembled lines cache
pub fn extract_selected_text(state: &AppState) -> String {
    let Some((start_line, start_col, end_line, end_col)) = state.selection.normalized_bounds()
    else {
        return String::new();
    };

    // Get cached lines
    let Some((_, cached_lines, _)) = &state.assembled_lines_cache else {
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

    // Trim leading whitespace (from border prefixes like "┃ ") and trailing whitespace
    // but preserve structure
    result
        .lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join("\n")
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
