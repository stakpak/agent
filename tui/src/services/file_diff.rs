use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use similar::TextDiff;
use stakpak_shared::models::integrations::openai::ToolCall;
use std::fs;

use crate::services::detect_term::AdaptiveColors;

pub fn preview_str_replace_editor_style(
    file_path: &str,
    old_str: &str,
    new_str: &str,
    replace_all: bool,
    terminal_width: usize,
    diff_type: &str,
) -> Result<(Vec<Line<'static>>, usize, usize, usize), std::io::Error> {
    // Read the current file content
    let original_content = match diff_type {
        "str_replace" => fs::read_to_string(file_path)?,
        "remove" => "".to_string(),
        "create" => "".to_string(),
        _ => fs::read_to_string(file_path)?,
    };

    // Create the new content with the replacement
    // TODO:: GET THE REAL VALUE OF REDACTED SECRETS FROM SECRETS MANAGER
    let new_content = if replace_all {
        original_content.replace(old_str, new_str)
    } else {
        original_content.replacen(old_str, new_str, 1)
    };

    // Create a line-by-line diff
    let diff = TextDiff::from_lines(&original_content, &new_content);

    let mut lines = Vec::new();
    let mut deletions = 0;
    let mut insertions = 0;
    let mut first_change_index = None;

    let mut old_line_num = 0;
    let mut new_line_num = 0;

    // Helper function to wrap content while maintaining proper indentation
    fn wrap_content(content: &str, terminal_width: usize, prefix_width: usize) -> Vec<String> {
        let available_width = terminal_width.saturating_sub(prefix_width + 4); // 4 for margins

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

            // Find the best break point (prefer word boundaries)
            let mut break_point = available_width;

            // Look for a space within the last 20% of the available width
            let search_start = (available_width as f32 * 0.8) as usize;

            // Ensure indices are on character boundaries
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

            // Ensure break_point is on a character boundary
            let break_point = remaining
                .char_indices()
                .find(|(idx, _)| *idx >= break_point)
                .map(|(idx, _)| idx)
                .unwrap_or(remaining.len());

            let chunk = &remaining[..break_point];
            wrapped_lines.push(chunk.to_string());

            remaining = &remaining[break_point..];
            // Skip leading whitespace on continuation lines
            remaining = remaining.trim_start();
        }

        wrapped_lines
    }

    for op in diff.ops() {
        let old_range = op.old_range();
        let new_range = op.new_range();

        match op.tag() {
            similar::DiffTag::Equal => {
                // Show equal lines with wrapping
                for idx in 0..old_range.len() {
                    old_line_num += 1;
                    new_line_num += 1;

                    let line_content = diff.old_slices()[old_range.start + idx].trim_end();
                    let prefix_width = 4 + 1 + 4 + 1 + 2; // old_num + space + new_num + space + marker
                    let wrapped_content = wrap_content(line_content, terminal_width, prefix_width);

                    for (i, content_line) in wrapped_content.iter().enumerate() {
                        if i == 0 {
                            // First line with line numbers
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
                            // Continuation lines with proper spacing
                            lines.push(Line::from(vec![
                                Span::styled(
                                    "     ",
                                    Style::default().fg(AdaptiveColors::dark_gray()),
                                ), // 5 spaces for old line num
                                Span::styled(
                                    "      ",
                                    Style::default().fg(AdaptiveColors::dark_gray()),
                                ), // 6 spaces for new line num
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
                // Show deleted lines with wrapping
                for idx in 0..old_range.len() {
                    old_line_num += 1;
                    deletions += 1;

                    // Track the first change index
                    if first_change_index.is_none() {
                        first_change_index = Some(lines.len());
                    }

                    let line_content = diff.old_slices()[old_range.start + idx].trim_end();
                    let prefix_width = 4 + 1 + 5 + 3; // old_num + space + empty + marker
                    let wrapped_content = wrap_content(line_content, terminal_width, prefix_width);

                    for (i, content_line) in wrapped_content.iter().enumerate() {
                        let mut line_spans = vec![];

                        if i == 0 {
                            // First line with line numbers
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
                            // Continuation lines with proper spacing
                            line_spans.push(Span::styled(
                                "     ", // 5 spaces for old line num
                                Style::default().bg(AdaptiveColors::dark_red()),
                            ));
                            line_spans.push(Span::styled(
                                "     ", // 5 spaces for new line num area
                                Style::default().bg(AdaptiveColors::dark_red()),
                            ));
                            line_spans.push(Span::styled(
                                "   ", // 3 spaces for marker area
                                Style::default().bg(AdaptiveColors::dark_red()),
                            ));
                        }

                        line_spans.push(Span::styled(
                            content_line.clone(),
                            Style::default()
                                .fg(AdaptiveColors::text())
                                .bg(AdaptiveColors::dark_red()),
                        ));

                        // Add padding to extend background across full width
                        let current_width = prefix_width + content_line.len();
                        let target_width = terminal_width - 4;
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
                // Show inserted lines with wrapping
                for idx in 0..new_range.len() {
                    new_line_num += 1;
                    insertions += 1;

                    // Track the first change index
                    if first_change_index.is_none() {
                        first_change_index = Some(lines.len());
                    }

                    let line_content = diff.new_slices()[new_range.start + idx].trim_end();
                    let prefix_width = 5 + 4 + 1 + 3; // empty + line_num + space + marker
                    let wrapped_content = wrap_content(line_content, terminal_width, prefix_width);

                    for (i, content_line) in wrapped_content.iter().enumerate() {
                        let mut line_spans = vec![];

                        if i == 0 {
                            // First line with line numbers
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
                            // Continuation lines with proper spacing
                            line_spans.push(Span::styled(
                                "     ", // 5 spaces for old line num area
                                Style::default().bg(AdaptiveColors::dark_green()),
                            ));
                            line_spans.push(Span::styled(
                                "     ", // 5 spaces for new line num
                                Style::default().bg(AdaptiveColors::dark_green()),
                            ));
                            line_spans.push(Span::styled(
                                "   ", // 3 spaces for marker area
                                Style::default().bg(AdaptiveColors::dark_green()),
                            ));
                        }

                        line_spans.push(Span::styled(
                            content_line.clone(),
                            Style::default()
                                .fg(AdaptiveColors::text())
                                .bg(AdaptiveColors::dark_green()),
                        ));

                        // Add padding to extend background across full width
                        let current_width = prefix_width + content_line.len();
                        let target_width = terminal_width - 4;
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
                // Handle replacements (show both delete and insert) with wrapping
                // First show deletes
                for idx in 0..old_range.len() {
                    old_line_num += 1;
                    deletions += 1;

                    // Track the first change index
                    if first_change_index.is_none() {
                        first_change_index = Some(lines.len());
                    }

                    let line_content = diff.old_slices()[old_range.start + idx].trim_end();
                    let prefix_width = 4 + 1 + 5 + 3; // old_num + space + empty + marker
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
                        let target_width = terminal_width - 4;
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
                    let line_content = diff.new_slices()[new_range.start + idx].trim_end();
                    let prefix_width = 5 + 4 + 1 + 3; // empty + line_num + space + marker
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
                        let target_width = terminal_width - 4;
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

    Ok((
        lines,
        deletions,
        insertions,
        first_change_index.unwrap_or(0),
    ))
}

pub fn render_file_diff_block(
    tool_call: &ToolCall,
    terminal_width: usize,
) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let args: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
        .unwrap_or_else(|_| serde_json::json!({}));

    let old_str = args.get("old_str").and_then(|v| v.as_str()).unwrap_or("");
    let new_str = if tool_call.function.name == "create" {
        args.get("file_text").and_then(|v| v.as_str()).unwrap_or("")
    } else {
        args.get("new_str").and_then(|v| v.as_str()).unwrap_or("")
    };
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let replace_all = args
        .get("replace_all")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Now you can use these variables with preview_str_replace_editor_style
    let (diff_lines, deletions, insertions, first_change_index) = preview_str_replace_editor_style(
        path,
        old_str,
        new_str,
        replace_all,
        terminal_width,
        &tool_call.function.name,
    )
    .unwrap_or_else(|_| (vec![Line::from("Failed to generate diff preview")], 0, 0, 0));

    let mut lines = Vec::new();

    if deletions == 0 && insertions == 0 {
        return (vec![], vec![]);
    }

    // let title = if tool_call.function.name == "create" {
    //     "Create"
    // } else {
    //     "Editing"
    // };
    // // Add header
    // lines.push(Line::from(vec![Span::styled(
    //     format!(
    //         "{} {} file",
    //         title,
    //         if deletions > 0 || insertions > 0 {
    //             "1"
    //         } else {
    //             "0"
    //         }
    //     )
    //     .to_string(),
    //     Style::default().fg(AdaptiveColors::text()),
    // )]));

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
            format!(" +{}", insertions).to_string(),
            Style::default().fg(Color::Green),
        ),
        Span::styled(
            format!(" -{}", deletions).to_string(),
            Style::default().fg(Color::Red),
        ),
    ]));

    // lines.push(Line::from("")); // Empty line

    let mut truncated_diff_lines;
    let mut full_diff_lines = diff_lines.clone();

    // Count how many lines we have from the first change to the end
    let change_lines_count = diff_lines.len() - first_change_index;

    if change_lines_count > 10 {
        // Start from the first change line instead of first 3 lines
        let change_lines = diff_lines[first_change_index..first_change_index + 10].to_vec();
        let remaining_count = change_lines_count - 10;

        // Add truncation message
        let truncation_line = Line::from(vec![Span::styled(
            format!(
                "... truncated ({} more lines) . ctrl+t to review",
                remaining_count
            ),
            Style::default().fg(Color::Yellow),
        )]);

        // Combine change lines + truncation message for truncated version
        truncated_diff_lines = change_lines;
        // truncated_diff_lines.push(Line::from(""));
        truncated_diff_lines.push(truncation_line);
    } else {
        // Show all change lines
        let change_lines = diff_lines[first_change_index..].to_vec();
        truncated_diff_lines = change_lines;
    }

    truncated_diff_lines = [lines.clone(), truncated_diff_lines].concat();
    full_diff_lines = [lines, full_diff_lines].concat();

    (truncated_diff_lines, full_diff_lines)
}
