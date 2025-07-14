use crate::AppState;
use crate::services::shell_mode::SHELL_PROMPT_PREFIX;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn get_multiline_input_lines(state: &AppState, area_width: usize) -> (Vec<Line>, bool) {
    let input = if state.show_shell_mode && state.waiting_for_shell_input {
        "*".repeat(state.input.chars().count())
    } else {
        state.input.clone()
    };
    let available_width = area_width.saturating_sub(4); // -4 for borders and padding
    let cursor_pos = state.cursor_position.min(input.len());
    let line_segments: Vec<&str> = input.split('\n').collect();
    let mut lines = Vec::new();
    let mut cursor_rendered = false;
    let mut current_pos = 0;
    for (segment_idx, segment) in line_segments.iter().enumerate() {
        if segment.is_empty() {
            let mut empty_line = Vec::new();
            if segment_idx == 0 {
                if state.show_shell_mode {
                    empty_line.push(Span::styled(
                        " ".to_string() + SHELL_PROMPT_PREFIX,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    empty_line.push(Span::raw("> "));
                }
            }
            if current_pos == cursor_pos && !cursor_rendered {
                empty_line.push(Span::styled(
                    "█",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
                cursor_rendered = true;
            }
            lines.push(Line::from(empty_line));
            current_pos += 1;
            continue;
        }
        let prompt_text = if state.show_shell_mode {
            format!(" {}", SHELL_PROMPT_PREFIX)
        } else if segment_idx == 0 {
            "> ".to_string()
        } else {
            String::new()
        };
        let prompt_width = prompt_text.chars().count();
        let content_width = available_width.saturating_sub(prompt_width);
        let mut current_line_content = String::new();
        let mut current_line_width = 0;
        let mut line_start_pos = current_pos;
        let mut word_positions = Vec::new();
        let mut word_start = 0;
        let mut in_word = false;
        for (i, ch) in segment.char_indices() {
            if ch.is_whitespace() {
                if in_word {
                    word_positions.push((word_start, i, false));
                    in_word = false;
                }
                if !in_word {
                    word_start = i;
                }
            } else {
                if !in_word {
                    if word_start < i {
                        word_positions.push((word_start, i, true));
                    }
                    word_start = i;
                    in_word = true;
                }
            }
        }
        if in_word && word_start < segment.len() {
            word_positions.push((word_start, segment.len(), false));
        } else if !in_word && word_start < segment.len() {
            word_positions.push((word_start, segment.len(), true));
        }
        if word_positions.is_empty() {
            word_positions.push((0, segment.len(), false));
        }
        for (start, end, _is_whitespace) in word_positions {
            let text = &segment[start..end];
            let text_width = text.chars().count();
            if current_line_width + text_width > content_width && !current_line_content.is_empty() {
                let mut line_spans = Vec::new();
                if segment_idx == 0 && lines.is_empty() {
                    if state.show_shell_mode {
                        line_spans.push(Span::styled(
                            prompt_text.clone(),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ));
                    } else {
                        line_spans.push(Span::raw(prompt_text.clone()));
                    }
                }
                let line_end_pos = line_start_pos + current_line_content.len();
                if cursor_pos >= line_start_pos && cursor_pos <= line_end_pos && !cursor_rendered {
                    let cursor_offset = cursor_pos - line_start_pos;
                    if cursor_offset < current_line_content.len() {
                        let before_cursor = &current_line_content[..cursor_offset];
                        let at_cursor_char = current_line_content
                            .chars()
                            .nth(cursor_offset)
                            .unwrap_or(' ');
                        let at_cursor = if at_cursor_char == ' ' {
                            "█"
                        } else {
                            &at_cursor_char.to_string()
                        };
                        let after_cursor_start = cursor_offset + at_cursor_char.len_utf8();
                        let after_cursor = if after_cursor_start < current_line_content.len() {
                            &current_line_content[after_cursor_start..]
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
                        line_spans.push(Span::raw(current_line_content.clone()));
                        line_spans.push(Span::styled(
                            "█",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ));
                        cursor_rendered = true;
                    }
                } else {
                    line_spans.push(Span::raw(current_line_content.clone()));
                }
                lines.push(Line::from(line_spans));
                current_line_content.clear();
                current_line_width = 0;
                line_start_pos = current_pos + start;
            }
            current_line_content.push_str(text);
            current_line_width += text_width;
        }
        if !current_line_content.is_empty() {
            let mut line_spans = Vec::new();
            if segment_idx == 0 && lines.is_empty() {
                if state.show_shell_mode {
                    line_spans.push(Span::styled(
                        prompt_text.clone(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    line_spans.push(Span::raw(prompt_text.clone()));
                }
            }
            let line_end_pos = line_start_pos + current_line_content.len();
            if cursor_pos >= line_start_pos && cursor_pos <= line_end_pos && !cursor_rendered {
                let cursor_offset = cursor_pos - line_start_pos;
                if cursor_offset < current_line_content.len() {
                    let before_cursor = &current_line_content[..cursor_offset];
                    let at_cursor_char = current_line_content
                        .chars()
                        .nth(cursor_offset)
                        .unwrap_or(' ');
                    let at_cursor = if at_cursor_char == ' ' {
                        "█"
                    } else {
                        &at_cursor_char.to_string()
                    };
                    let after_cursor_start = cursor_offset + at_cursor_char.len_utf8();
                    let after_cursor = if after_cursor_start < current_line_content.len() {
                        &current_line_content[after_cursor_start..]
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
                    line_spans.push(Span::raw(current_line_content.clone()));
                    line_spans.push(Span::styled(
                        "█",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ));
                    cursor_rendered = true;
                }
            } else {
                line_spans.push(Span::raw(current_line_content.clone()));
            }
            lines.push(Line::from(line_spans));
        }
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
