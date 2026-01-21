use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use similar::TextDiff;
use stakpak_shared::models::integrations::openai::ToolCall;

use crate::services::detect_term::AdaptiveColors;

pub fn render_file_diff_block(
    tool_call: &ToolCall,
    terminal_width: usize,
) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    // Use the same diff-only approach as render_file_diff_block_from_args
    // This shows only the actual changes (old_str vs new_str), not the whole file
    render_file_diff_block_from_args(tool_call, terminal_width)
}

/// Generate a diff directly from old_str and new_str without reading from file.
/// This is used as a fallback when the file has already been modified (e.g., on session resume).
pub fn preview_diff_from_strings(
    old_str: &str,
    new_str: &str,
    terminal_width: usize,
) -> (Vec<Line<'static>>, usize, usize, usize, usize) {
    // Create a line-by-line diff directly from the strings
    let diff = TextDiff::from_lines(old_str, new_str);

    let mut lines = Vec::new();
    let mut deletions = 0;
    let mut insertions = 0;
    let mut first_change_index = None;
    let mut last_change_index = 0usize;

    let mut old_line_num = 0;
    let mut new_line_num = 0;

    // Helper function to wrap content while maintaining proper indentation
    fn wrap_content(content: &str, terminal_width: usize, prefix_width: usize) -> Vec<String> {
        let available_width = terminal_width.saturating_sub(prefix_width + 4);

        if content.len() <= available_width {
            return vec![content.to_string()];
        }

        let mut wrapped_lines = Vec::new();
        let mut remaining = content;

        while !remaining.is_empty() {
            if remaining.len() <= available_width {
                wrapped_lines.push(remaining.to_string());
                break;
            }

            let mut break_point = available_width;
            let search_start = (available_width as f32 * 0.8) as usize;

            let search_start = remaining
                .char_indices()
                .find(|(idx, _)| *idx >= search_start)
                .map(|(idx, _)| idx)
                .unwrap_or(remaining.len());

            let end_idx = remaining
                .char_indices()
                .find(|(idx, _)| *idx >= available_width)
                .map(|(idx, _)| idx)
                .unwrap_or(remaining.len());

            if search_start < end_idx
                && let Some(space_pos) = remaining[search_start..end_idx].rfind(char::is_whitespace)
            {
                break_point = search_start + space_pos;
            }

            let break_point = remaining
                .char_indices()
                .find(|(idx, _)| *idx >= break_point)
                .map(|(idx, _)| idx)
                .unwrap_or(remaining.len());

            let chunk = &remaining[..break_point];
            wrapped_lines.push(chunk.to_string());

            remaining = &remaining[break_point..];
            remaining = remaining.trim_start();
        }

        wrapped_lines
    }

    for op in diff.ops() {
        let old_range = op.old_range();
        let new_range = op.new_range();

        match op.tag() {
            similar::DiffTag::Equal => {
                for idx in 0..old_range.len() {
                    old_line_num += 1;
                    new_line_num += 1;

                    let line_content = diff.old_slices()[old_range.start + idx].trim_end();
                    let prefix_width = 4 + 1 + 4 + 1 + 2;
                    let wrapped_content = wrap_content(line_content, terminal_width, prefix_width);

                    for (i, content_line) in wrapped_content.iter().enumerate() {
                        if i == 0 {
                            lines.push(Line::from(vec![
                                Span::styled(
                                    format!("{:>4} ", old_line_num),
                                    Style::default().fg(AdaptiveColors::dark_gray()),
                                ),
                                Span::styled(
                                    format!("{:>4}  ", new_line_num),
                                    Style::default().fg(AdaptiveColors::dark_gray()),
                                ),
                                Span::styled("  ", Style::default()),
                                Span::styled(
                                    content_line.clone(),
                                    Style::default().fg(AdaptiveColors::text()),
                                ),
                            ]));
                        } else {
                            lines.push(Line::from(vec![
                                Span::styled(
                                    "     ",
                                    Style::default().fg(AdaptiveColors::dark_gray()),
                                ),
                                Span::styled(
                                    "      ",
                                    Style::default().fg(AdaptiveColors::dark_gray()),
                                ),
                                Span::styled("  ", Style::default()),
                                Span::styled(
                                    content_line.clone(),
                                    Style::default().fg(AdaptiveColors::text()),
                                ),
                            ]));
                        }
                    }
                }
            }
            similar::DiffTag::Delete => {
                for idx in 0..old_range.len() {
                    old_line_num += 1;
                    deletions += 1;

                    if first_change_index.is_none() {
                        first_change_index = Some(lines.len());
                    }
                    last_change_index = lines.len();

                    let line_content = diff.old_slices()[old_range.start + idx].trim_end();
                    let prefix_width = 4 + 1 + 5 + 3;
                    let wrapped_content = wrap_content(line_content, terminal_width, prefix_width);

                    for (i, content_line) in wrapped_content.iter().enumerate() {
                        let mut line_spans = vec![];

                        if i == 0 {
                            line_spans.push(Span::styled(
                                format!("{:>4} ", old_line_num),
                                Style::default()
                                    .fg(AdaptiveColors::red())
                                    .bg(AdaptiveColors::dark_red()),
                            ));
                            line_spans.push(Span::styled(
                                "     ",
                                Style::default().bg(AdaptiveColors::dark_red()),
                            ));
                            line_spans.push(Span::styled(
                                " - ",
                                Style::default()
                                    .fg(AdaptiveColors::red())
                                    .add_modifier(Modifier::BOLD)
                                    .bg(AdaptiveColors::dark_red()),
                            ));
                        } else {
                            line_spans.push(Span::styled(
                                "     ",
                                Style::default().bg(AdaptiveColors::dark_red()),
                            ));
                            line_spans.push(Span::styled(
                                "     ",
                                Style::default().bg(AdaptiveColors::dark_red()),
                            ));
                            line_spans.push(Span::styled(
                                "   ",
                                Style::default().bg(AdaptiveColors::dark_red()),
                            ));
                        }

                        line_spans.push(Span::styled(
                            content_line.clone(),
                            Style::default()
                                .fg(AdaptiveColors::text())
                                .bg(AdaptiveColors::dark_red()),
                        ));

                        let current_width = prefix_width + content_line.len();
                        let target_width = terminal_width.saturating_sub(4);
                        let padding_needed = target_width.saturating_sub(current_width);
                        if padding_needed > 0 {
                            line_spans.push(Span::styled(
                                " ".repeat(padding_needed),
                                Style::default().bg(AdaptiveColors::dark_red()),
                            ));
                        }

                        lines.push(Line::from(line_spans));
                    }
                }
            }
            similar::DiffTag::Insert => {
                for idx in 0..new_range.len() {
                    new_line_num += 1;
                    insertions += 1;

                    if first_change_index.is_none() {
                        first_change_index = Some(lines.len());
                    }
                    last_change_index = lines.len();

                    let line_content = diff.new_slices()[new_range.start + idx].trim_end();
                    let prefix_width = 5 + 4 + 1 + 3;
                    let wrapped_content = wrap_content(line_content, terminal_width, prefix_width);

                    for (i, content_line) in wrapped_content.iter().enumerate() {
                        let mut line_spans = vec![];

                        if i == 0 {
                            line_spans.push(Span::styled(
                                "     ",
                                Style::default().bg(AdaptiveColors::dark_green()),
                            ));
                            line_spans.push(Span::styled(
                                format!("{:>4} ", new_line_num),
                                Style::default()
                                    .fg(AdaptiveColors::green())
                                    .bg(AdaptiveColors::dark_green()),
                            ));
                            line_spans.push(Span::styled(
                                " + ",
                                Style::default()
                                    .fg(AdaptiveColors::green())
                                    .add_modifier(Modifier::BOLD)
                                    .bg(AdaptiveColors::dark_green()),
                            ));
                        } else {
                            line_spans.push(Span::styled(
                                "     ",
                                Style::default().bg(AdaptiveColors::dark_green()),
                            ));
                            line_spans.push(Span::styled(
                                "     ",
                                Style::default().bg(AdaptiveColors::dark_green()),
                            ));
                            line_spans.push(Span::styled(
                                "   ",
                                Style::default().bg(AdaptiveColors::dark_green()),
                            ));
                        }

                        line_spans.push(Span::styled(
                            content_line.clone(),
                            Style::default()
                                .fg(AdaptiveColors::text())
                                .bg(AdaptiveColors::dark_green()),
                        ));

                        let current_width = prefix_width + content_line.len();
                        let target_width = terminal_width.saturating_sub(4);
                        let padding_needed = target_width.saturating_sub(current_width);
                        if padding_needed > 0 {
                            line_spans.push(Span::styled(
                                " ".repeat(padding_needed),
                                Style::default().bg(AdaptiveColors::dark_green()),
                            ));
                        }

                        lines.push(Line::from(line_spans));
                    }
                }
            }
            similar::DiffTag::Replace => {
                // First show deletes
                for idx in 0..old_range.len() {
                    old_line_num += 1;
                    deletions += 1;

                    if first_change_index.is_none() {
                        first_change_index = Some(lines.len());
                    }
                    last_change_index = lines.len();

                    let line_content = diff.old_slices()[old_range.start + idx].trim_end();
                    let prefix_width = 4 + 1 + 5 + 3;
                    let wrapped_content = wrap_content(line_content, terminal_width, prefix_width);

                    for (i, content_line) in wrapped_content.iter().enumerate() {
                        let mut line_spans = vec![];

                        if i == 0 {
                            line_spans.push(Span::styled(
                                format!("{:>4} ", old_line_num),
                                Style::default()
                                    .fg(AdaptiveColors::red())
                                    .bg(AdaptiveColors::dark_red()),
                            ));
                            line_spans.push(Span::styled(
                                "     ",
                                Style::default().bg(AdaptiveColors::dark_red()),
                            ));
                            line_spans.push(Span::styled(
                                " - ",
                                Style::default()
                                    .fg(AdaptiveColors::red())
                                    .add_modifier(Modifier::BOLD)
                                    .bg(AdaptiveColors::dark_red()),
                            ));
                        } else {
                            line_spans.push(Span::styled(
                                "     ",
                                Style::default().bg(AdaptiveColors::dark_red()),
                            ));
                            line_spans.push(Span::styled(
                                "     ",
                                Style::default().bg(AdaptiveColors::dark_red()),
                            ));
                            line_spans.push(Span::styled(
                                "   ",
                                Style::default().bg(AdaptiveColors::dark_red()),
                            ));
                        }

                        line_spans.push(Span::styled(
                            content_line.clone(),
                            Style::default()
                                .fg(AdaptiveColors::text())
                                .bg(AdaptiveColors::dark_red()),
                        ));

                        let current_width = prefix_width + content_line.len();
                        let target_width = terminal_width.saturating_sub(4);
                        let padding_needed = target_width.saturating_sub(current_width);
                        if padding_needed > 0 {
                            line_spans.push(Span::styled(
                                " ".repeat(padding_needed),
                                Style::default().bg(AdaptiveColors::dark_red()),
                            ));
                        }

                        lines.push(Line::from(line_spans));
                    }
                }

                // Then show inserts
                for idx in 0..new_range.len() {
                    new_line_num += 1;
                    insertions += 1;
                    last_change_index = lines.len();
                    let line_content = diff.new_slices()[new_range.start + idx].trim_end();
                    let prefix_width = 5 + 4 + 1 + 3;
                    let wrapped_content = wrap_content(line_content, terminal_width, prefix_width);

                    for (i, content_line) in wrapped_content.iter().enumerate() {
                        let mut line_spans = vec![];

                        if i == 0 {
                            line_spans.push(Span::styled(
                                "     ",
                                Style::default().bg(AdaptiveColors::dark_green()),
                            ));
                            line_spans.push(Span::styled(
                                format!("{:>4} ", new_line_num),
                                Style::default()
                                    .fg(AdaptiveColors::green())
                                    .bg(AdaptiveColors::dark_green()),
                            ));
                            line_spans.push(Span::styled(
                                " + ",
                                Style::default()
                                    .fg(AdaptiveColors::green())
                                    .add_modifier(Modifier::BOLD)
                                    .bg(AdaptiveColors::dark_green()),
                            ));
                        } else {
                            line_spans.push(Span::styled(
                                "     ",
                                Style::default().bg(AdaptiveColors::dark_green()),
                            ));
                            line_spans.push(Span::styled(
                                "     ",
                                Style::default().bg(AdaptiveColors::dark_green()),
                            ));
                            line_spans.push(Span::styled(
                                "   ",
                                Style::default().bg(AdaptiveColors::dark_green()),
                            ));
                        }

                        line_spans.push(Span::styled(
                            content_line.clone(),
                            Style::default()
                                .fg(AdaptiveColors::text())
                                .bg(AdaptiveColors::dark_green()),
                        ));

                        let current_width = prefix_width + content_line.len();
                        let target_width = terminal_width.saturating_sub(4);
                        let padding_needed = target_width.saturating_sub(current_width);
                        if padding_needed > 0 {
                            line_spans.push(Span::styled(
                                " ".repeat(padding_needed),
                                Style::default().bg(AdaptiveColors::dark_green()),
                            ));
                        }

                        lines.push(Line::from(line_spans));
                    }
                }
            }
        }
    }

    // Update last_change_index to point to the last line (after all changes have been processed)
    if !lines.is_empty() {
        last_change_index = lines.len().saturating_sub(1);
    }

    (
        lines,
        deletions,
        insertions,
        first_change_index.unwrap_or(0),
        last_change_index,
    )
}

/// Render a diff block directly from tool call arguments (old_str and new_str).
/// This function SKIPS file-based diff entirely and is used for fullscreen popup
/// when the file has already been modified (e.g., on session resume).
pub fn render_file_diff_block_from_args(
    tool_call: &ToolCall,
    terminal_width: usize,
) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let args: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
        .unwrap_or_else(|_| serde_json::json!({}));

    let old_str = args.get("old_str").and_then(|v| v.as_str()).unwrap_or("");
    let new_str = if crate::utils::strip_tool_name(&tool_call.function.name) == "create" {
        args.get("file_text").and_then(|v| v.as_str()).unwrap_or("")
    } else {
        args.get("new_str").and_then(|v| v.as_str()).unwrap_or("")
    };
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");

    // If both old and new are empty, no diff to show
    if old_str.is_empty() && new_str.is_empty() {
        return (vec![], vec![]);
    }

    // Generate diff directly from the strings
    let (diff_lines, deletions, insertions, first_change_index, last_change_index) =
        preview_diff_from_strings(old_str, new_str, terminal_width);

    if deletions == 0 && insertions == 0 {
        return (vec![], vec![]);
    }

    let mut lines = Vec::new();

    // Add file path with changes summary
    lines.push(Line::from(vec![
        Span::styled(
            "1/1 ".to_string(),
            Style::default().fg(AdaptiveColors::text()),
        ),
        Span::styled(
            path.to_string(),
            Style::default()
                .fg(AdaptiveColors::text())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" +{}", insertions),
            Style::default().fg(Color::Green),
        ),
        Span::styled(format!(" -{}", deletions), Style::default().fg(Color::Red)),
    ]));

    // Extract only the changed range (from first change to last change)
    let change_range_end = (last_change_index + 1).min(diff_lines.len());
    let full_change_lines = if first_change_index < change_range_end {
        diff_lines[first_change_index..change_range_end].to_vec()
    } else {
        diff_lines.clone()
    };

    let mut truncated_diff_lines;

    // Count how many lines in the change range
    let change_lines_count = full_change_lines.len();

    if change_lines_count > 10 {
        // Show first 10 lines of changes
        let change_lines = full_change_lines[..10].to_vec();
        let remaining_count = change_lines_count - 10;

        let truncation_line = Line::from(vec![Span::styled(
            format!(
                "... truncated ({} more lines) . ctrl+t to review",
                remaining_count
            ),
            Style::default().fg(Color::Yellow),
        )]);

        truncated_diff_lines = change_lines;
        truncated_diff_lines.push(truncation_line);
    } else {
        // Show all change lines
        truncated_diff_lines = full_change_lines.clone();
    }

    truncated_diff_lines = [lines.clone(), truncated_diff_lines].concat();
    let full_diff_lines = [lines, full_change_lines].concat();

    (truncated_diff_lines, full_diff_lines)
}
