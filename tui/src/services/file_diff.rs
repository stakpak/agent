use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use similar::TextDiff;
use stakpak_shared::models::integrations::openai::ToolCall;
use std::fs;

const RED_COLOR: Color = Color::Rgb(239, 100, 97);
const GREEN_COLOR: Color = Color::Rgb(35, 218, 111);
const TEXT_COLOR: Color = Color::Rgb(180, 180, 180);
const DARK_GRAY_COLOR: Color = Color::Rgb(80, 80, 80);
const DARK_GREEN_COLOR: Color = Color::Rgb(44, 51, 35);
const DARK_RED_COLOR: Color = Color::Rgb(51, 36, 35);

pub fn preview_str_replace_editor_style(
    file_path: &str,
    old_str: &str,
    new_str: &str,
    replace_all: bool,
    terminal_width: usize,
) -> Result<(Vec<Line<'static>>, usize, usize, usize), std::io::Error> {
    // Read the current file content
    let original_content = fs::read_to_string(file_path)?;

    // Create the new content with the replacement
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
            if let Some(space_pos) = remaining[search_start..available_width.min(remaining.len())]
                .rfind(char::is_whitespace)
            {
                break_point = search_start + space_pos;
            }

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
                                    Style::default().fg(DARK_GRAY_COLOR),
                                ),
                                Span::styled(
                                    format!("{:>4}  ", new_line_num),
                                    Style::default().fg(DARK_GRAY_COLOR),
                                ),
                                Span::styled("  ", Style::default()),
                                Span::styled(content_line.clone(), Style::default().fg(TEXT_COLOR)),
                            ]));
                        } else {
                            // Continuation lines with proper spacing
                            lines.push(Line::from(vec![
                                Span::styled("     ", Style::default().fg(DARK_GRAY_COLOR)), // 5 spaces for old line num
                                Span::styled("      ", Style::default().fg(DARK_GRAY_COLOR)), // 6 spaces for new line num
                                Span::styled("  ", Style::default()),
                                Span::styled(content_line.clone(), Style::default().fg(TEXT_COLOR)),
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
                                Style::default().fg(RED_COLOR).bg(DARK_RED_COLOR),
                            ));
                            line_spans
                                .push(Span::styled("     ", Style::default().bg(DARK_RED_COLOR)));
                            line_spans.push(Span::styled(
                                " - ",
                                Style::default()
                                    .fg(RED_COLOR)
                                    .add_modifier(Modifier::BOLD)
                                    .bg(DARK_RED_COLOR),
                            ));
                        } else {
                            // Continuation lines with proper spacing
                            line_spans.push(Span::styled(
                                "     ", // 5 spaces for old line num
                                Style::default().bg(DARK_RED_COLOR),
                            ));
                            line_spans.push(Span::styled(
                                "     ", // 5 spaces for new line num area
                                Style::default().bg(DARK_RED_COLOR),
                            ));
                            line_spans.push(Span::styled(
                                "   ", // 3 spaces for marker area
                                Style::default().bg(DARK_RED_COLOR),
                            ));
                        }

                        line_spans.push(Span::styled(
                            content_line.clone(),
                            Style::default().fg(TEXT_COLOR).bg(DARK_RED_COLOR),
                        ));

                        // Add padding to extend background across full width
                        let current_width = prefix_width + content_line.len();
                        let target_width = terminal_width - 4;
                        let padding_needed = target_width.saturating_sub(current_width);
                        if padding_needed > 0 {
                            line_spans.push(Span::styled(
                                " ".repeat(padding_needed),
                                Style::default().bg(DARK_RED_COLOR),
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
                            line_spans
                                .push(Span::styled("     ", Style::default().bg(DARK_GREEN_COLOR)));
                            line_spans.push(Span::styled(
                                format!("{:>4} ", new_line_num),
                                Style::default().fg(GREEN_COLOR).bg(DARK_GREEN_COLOR),
                            ));
                            line_spans.push(Span::styled(
                                " + ",
                                Style::default()
                                    .fg(GREEN_COLOR)
                                    .add_modifier(Modifier::BOLD)
                                    .bg(DARK_GREEN_COLOR),
                            ));
                        } else {
                            // Continuation lines with proper spacing
                            line_spans.push(Span::styled(
                                "     ", // 5 spaces for old line num area
                                Style::default().bg(DARK_GREEN_COLOR),
                            ));
                            line_spans.push(Span::styled(
                                "     ", // 5 spaces for new line num
                                Style::default().bg(DARK_GREEN_COLOR),
                            ));
                            line_spans.push(Span::styled(
                                "   ", // 3 spaces for marker area
                                Style::default().bg(DARK_GREEN_COLOR),
                            ));
                        }

                        line_spans.push(Span::styled(
                            content_line.clone(),
                            Style::default().fg(TEXT_COLOR).bg(DARK_GREEN_COLOR),
                        ));

                        // Add padding to extend background across full width
                        let current_width = prefix_width + content_line.len();
                        let target_width = terminal_width - 4;
                        let padding_needed = target_width.saturating_sub(current_width);
                        if padding_needed > 0 {
                            line_spans.push(Span::styled(
                                " ".repeat(padding_needed),
                                Style::default().bg(DARK_GREEN_COLOR),
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
                                Style::default().fg(RED_COLOR).bg(DARK_RED_COLOR),
                            ));
                            line_spans
                                .push(Span::styled("     ", Style::default().bg(DARK_RED_COLOR)));
                            line_spans.push(Span::styled(
                                " - ",
                                Style::default()
                                    .fg(RED_COLOR)
                                    .add_modifier(Modifier::BOLD)
                                    .bg(DARK_RED_COLOR),
                            ));
                        } else {
                            line_spans
                                .push(Span::styled("     ", Style::default().bg(DARK_RED_COLOR)));
                            line_spans
                                .push(Span::styled("     ", Style::default().bg(DARK_RED_COLOR)));
                            line_spans
                                .push(Span::styled("   ", Style::default().bg(DARK_RED_COLOR)));
                        }

                        line_spans.push(Span::styled(
                            content_line.clone(),
                            Style::default().fg(TEXT_COLOR).bg(DARK_RED_COLOR),
                        ));

                        let current_width = prefix_width + content_line.len();
                        let target_width = terminal_width - 4;
                        let padding_needed = target_width.saturating_sub(current_width);
                        if padding_needed > 0 {
                            line_spans.push(Span::styled(
                                " ".repeat(padding_needed),
                                Style::default().bg(DARK_RED_COLOR),
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
                            line_spans
                                .push(Span::styled("     ", Style::default().bg(DARK_GREEN_COLOR)));
                            line_spans.push(Span::styled(
                                format!("{:>4} ", new_line_num),
                                Style::default().fg(GREEN_COLOR).bg(DARK_GREEN_COLOR),
                            ));
                            line_spans.push(Span::styled(
                                " + ",
                                Style::default()
                                    .fg(GREEN_COLOR)
                                    .add_modifier(Modifier::BOLD)
                                    .bg(DARK_GREEN_COLOR),
                            ));
                        } else {
                            line_spans
                                .push(Span::styled("     ", Style::default().bg(DARK_GREEN_COLOR)));
                            line_spans
                                .push(Span::styled("     ", Style::default().bg(DARK_GREEN_COLOR)));
                            line_spans
                                .push(Span::styled("   ", Style::default().bg(DARK_GREEN_COLOR)));
                        }

                        line_spans.push(Span::styled(
                            content_line.clone(),
                            Style::default().fg(TEXT_COLOR).bg(DARK_GREEN_COLOR),
                        ));

                        let current_width = prefix_width + content_line.len();
                        let target_width = terminal_width - 4;
                        let padding_needed = target_width.saturating_sub(current_width);
                        if padding_needed > 0 {
                            line_spans.push(Span::styled(
                                " ".repeat(padding_needed),
                                Style::default().bg(DARK_GREEN_COLOR),
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

    let old_str = args["old_str"].as_str().unwrap_or("");
    let new_str = args["new_str"].as_str().unwrap_or("");
    let path = args["path"].as_str().unwrap_or("");
    let replace_all = args["replace_all"].as_bool().unwrap_or(false);

    // Now you can use these variables with preview_str_replace_editor_style
    let (diff_lines, deletions, insertions, first_change_index) =
        preview_str_replace_editor_style(path, old_str, new_str, replace_all, terminal_width)
            .unwrap_or_else(|_| (vec![Line::from("Failed to generate diff preview")], 0, 0, 0));

    let mut lines = Vec::new();

    if deletions == 0 && insertions == 0 {
        return (vec![], vec![]);
    }
    // Add header
    lines.push(Line::from(vec![Span::styled(
        format!(
            "Editing {} file",
            if deletions > 0 || insertions > 0 {
                "1"
            } else {
                "0"
            }
        )
        .to_string(),
        Style::default().fg(TEXT_COLOR),
    )]));

    // Add file path with changes summary
    lines.push(Line::from(vec![
        Span::styled("1/1 ".to_string(), Style::default().fg(TEXT_COLOR)),
        Span::styled(
            path.to_string(),
            Style::default().fg(TEXT_COLOR).add_modifier(Modifier::BOLD),
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

    lines.push(Line::from("")); // Empty line

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
        truncated_diff_lines.push(Line::from(""));
        truncated_diff_lines.push(truncation_line);
    } else {
        // Show all change lines
        let change_lines = diff_lines[first_change_index..].to_vec();
        truncated_diff_lines = change_lines;
    }

    truncated_diff_lines = [lines.clone(), truncated_diff_lines].concat();
    full_diff_lines = [lines, full_diff_lines].concat();

    // Add summary to both versions
    truncated_diff_lines.push(Line::from(""));
    truncated_diff_lines.push(Line::from(vec![Span::styled(
        format!(
            "Total changes: {} additions, {} deletions",
            insertions, deletions
        ),
        Style::default().fg(Color::Cyan),
    )]));

    full_diff_lines.push(Line::from(""));
    full_diff_lines.push(Line::from(vec![Span::styled(
        format!(
            "Total changes: {} additions, {} deletions",
            insertions, deletions
        ),
        Style::default().fg(Color::Cyan),
    )]));

    (truncated_diff_lines, full_diff_lines)
}
