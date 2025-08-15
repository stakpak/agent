use crate::AppState;
use crate::services::shell_mode::SHELL_PROMPT_PREFIX;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn get_multiline_input_lines(state: &AppState, _area_width: usize) -> (Vec<Line>, bool) {
    let input = if state.show_shell_mode && state.waiting_for_shell_input {
        "*".repeat(state.input.chars().count())
    } else {
        state.input.clone()
    };
    let cursor_pos = state.cursor_position.min(input.len());
    let line_segments: Vec<&str> = input.split('\n').collect();
    let mut lines = Vec::new();
    let mut cursor_rendered = false;
    let mut current_pos = 0;
    for (segment_idx, segment) in line_segments.iter().enumerate() {
        let mut line_spans = Vec::new();
        if segment_idx == 0 {
            if state.show_shell_mode {
                line_spans.push(Span::styled(
                    " ".to_string() + SHELL_PROMPT_PREFIX,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                line_spans.push(Span::raw("→ "));
            }
        }
        let line_start_pos = current_pos;
        let line_end_pos = current_pos + segment.len();
        if cursor_pos >= line_start_pos && cursor_pos <= line_end_pos && !cursor_rendered {
            let cursor_offset = cursor_pos - line_start_pos;
            if cursor_offset < segment.len() {
                let before_cursor = &segment[..cursor_offset];
                let at_cursor_char = segment.chars().nth(cursor_offset).unwrap_or(' ');
                let at_cursor = if at_cursor_char == ' ' {
                    "█"
                } else {
                    &at_cursor_char.to_string()
                };
                let after_cursor_start = cursor_offset + at_cursor_char.len_utf8();
                let after_cursor = if after_cursor_start < segment.len() {
                    &segment[after_cursor_start..]
                } else {
                    ""
                };
                if !before_cursor.is_empty() {
                    line_spans.push(Span::raw(before_cursor.to_string()));
                }
                line_spans.push(Span::styled(
                    at_cursor.to_string(),
                    Style::default()
                        .bg(Color::Cyan)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                ));
                if !after_cursor.is_empty() {
                    line_spans.push(Span::raw(after_cursor.to_string()));
                }
                cursor_rendered = true;
            } else {
                line_spans.push(Span::raw(segment.to_string()));
                line_spans.push(Span::styled(
                    "█",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
                cursor_rendered = true;
            }
        } else {
            line_spans.push(Span::raw(segment.to_string()));
        }
        lines.push(Line::from(line_spans));
        current_pos += segment.len() + 1;
    }
    if cursor_pos == input.len() && !cursor_rendered {
        if let Some(last_line) = lines.last_mut() {
            last_line.spans.push(Span::styled(
                "█",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            let mut cursor_line = Vec::new();
            if state.show_shell_mode {
                cursor_line.push(Span::styled(
                    " ".to_string() + SHELL_PROMPT_PREFIX,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                cursor_line.push(Span::raw("> "));
            }
            cursor_line.push(Span::styled(
                "█",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::from(cursor_line));
        }
    }
    if lines.is_empty() {
        let mut default_line = Vec::new();
        if state.show_shell_mode {
            default_line.push(Span::styled(
                " ".to_string() + SHELL_PROMPT_PREFIX,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            default_line.push(Span::raw("> "));
        }
        default_line.push(Span::styled(
            "█",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::from(default_line));
    }
    (lines, cursor_rendered)
}
