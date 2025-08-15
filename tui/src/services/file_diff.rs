use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use similar::TextDiff;
use std::fs;

pub fn preview_str_replace_editor_style(
    file_path: &str,
    old_str: &str,
    new_str: &str,
    replace_all: bool,
    _terminal_size: Size,
) -> Result<Vec<Line<'static>>, std::io::Error> {
    // Read the current file content
    let original_content = fs::read_to_string(file_path)?;
    eprintln!("original_content: {}", original_content);

    // Create the new content with the replacement
    let new_content = if replace_all {
        original_content.replace(old_str, new_str)
    } else {
        original_content.replacen(old_str, new_str, 1)
    };

    // Count changes
    let changes_count = original_content.matches(old_str).count();
    let replacements = if replace_all {
        changes_count
    } else {
        1.min(changes_count)
    };

    // Create a line-by-line diff
    let diff = TextDiff::from_lines(&original_content, &new_content);

    let mut lines = Vec::new();

    // Add header
    lines.push(Line::from(vec![Span::styled(
        format!("{} file edited", if replacements > 0 { "1" } else { "0" }).to_string(),
        Style::default().fg(Color::White),
    )]));

    // Add file path with changes summary
    lines.push(Line::from(vec![
        Span::styled("1/1 ".to_string(), Style::default().fg(Color::White)),
        Span::styled(
            file_path.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" +{}", replacements).to_string(),
            Style::default().fg(Color::Green),
        ),
        Span::styled(
            format!(" -{}", replacements).to_string(),
            Style::default().fg(Color::Red),
        ),
    ]));

    lines.push(Line::from("")); // Empty line

    let mut old_line_num = 0;
    let mut new_line_num = 0;

    for op in diff.ops() {
        let old_range = op.old_range();
        let new_range = op.new_range();

        match op.tag() {
            similar::DiffTag::Equal => {
                // Show equal lines
                for idx in 0..old_range.len() {
                    old_line_num += 1;
                    new_line_num += 1;

                    let line_content = diff.old_slices()[old_range.start + idx].trim_end();

                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{:>4} ", old_line_num).to_string(),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(
                            format!("{:>4}  ", new_line_num).to_string(),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled("  ".to_string(), Style::default()),
                        Span::styled(line_content.to_string(), Style::default().fg(Color::White)),
                    ]));
                }
            }
            similar::DiffTag::Delete => {
                // Show deleted lines
                for idx in 0..old_range.len() {
                    old_line_num += 1;

                    let line_content = diff.old_slices()[old_range.start + idx].trim_end();

                    // Check if this line contains the old string to highlight it
                    let mut line_spans = vec![
                        Span::styled(
                            format!("{:>4} ", old_line_num).to_string(),
                            Style::default().fg(Color::Red).bg(Color::Rgb(80, 0, 0)),
                        ),
                        Span::styled(
                            "     ".to_string(), // Empty space for new line number (since this line was deleted)
                            Style::default().bg(Color::Rgb(80, 0, 0)),
                        ),
                        Span::styled(
                            " - ".to_string(),
                            Style::default()
                                .fg(Color::Red)
                                .add_modifier(Modifier::BOLD)
                                .bg(Color::Rgb(80, 0, 0)),
                        ),
                    ];

                    // Highlight the entire line with red background
                    line_spans.push(Span::styled(
                        line_content.to_string(),
                        Style::default().fg(Color::White).bg(Color::Rgb(80, 0, 0)),
                    ));

                    lines.push(Line::from(line_spans));
                }
            }
            similar::DiffTag::Insert => {
                // Show inserted lines
                for idx in 0..new_range.len() {
                    new_line_num += 1;

                    let line_content = diff.new_slices()[new_range.start + idx].trim_end();

                    let mut line_spans = vec![
                        Span::styled(
                            "     ".to_string(), // Empty space for old line number (like delete lines)
                            Style::default().bg(Color::Rgb(0, 50, 0)),
                        ),
                        Span::styled(
                            format!("{:>4} ", new_line_num).to_string(),
                            Style::default().fg(Color::Green).bg(Color::Rgb(0, 50, 0)),
                        ),
                        Span::styled(
                            " + ".to_string(),
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD)
                                .bg(Color::Rgb(0, 50, 0)),
                        ),
                    ];

                    // Highlight the entire line with green background
                    line_spans.push(Span::styled(
                        line_content.to_string(),
                        Style::default().fg(Color::White).bg(Color::Rgb(0, 50, 0)),
                    ));

                    lines.push(Line::from(line_spans));
                }
            }
            similar::DiffTag::Replace => {
                // Handle replacements (show both delete and insert)
                // First show deletes
                for idx in 0..old_range.len() {
                    old_line_num += 1;
                    let line_content = diff.old_slices()[old_range.start + idx].trim_end();

                    let mut line_spans = vec![
                        Span::styled(
                            format!("{:>4} ", old_line_num).to_string(),
                            Style::default().fg(Color::Red).bg(Color::Rgb(80, 0, 0)),
                        ),
                        Span::styled(
                            "     ".to_string(), // Empty space for new line number (since this line was deleted)
                            Style::default().bg(Color::Rgb(80, 0, 0)),
                        ),
                        Span::styled(
                            " - ".to_string(),
                            Style::default()
                                .fg(Color::Red)
                                .add_modifier(Modifier::BOLD)
                                .bg(Color::Rgb(80, 0, 0)),
                        ),
                    ];

                    // Highlight the entire line with red background
                    line_spans.push(Span::styled(
                        line_content.to_string(),
                        Style::default().fg(Color::White).bg(Color::Rgb(80, 0, 0)),
                    ));

                    lines.push(Line::from(line_spans));
                }

                // Then show inserts
                for idx in 0..new_range.len() {
                    new_line_num += 1;
                    let line_content = diff.new_slices()[new_range.start + idx].trim_end();

                    let mut line_spans = vec![
                        Span::styled(
                            "     ".to_string(), // Empty space for old line number (like delete lines)
                            Style::default().bg(Color::Rgb(0, 50, 0)),
                        ),
                        Span::styled(
                            format!("{:>4} ", new_line_num).to_string(),
                            Style::default().fg(Color::Green).bg(Color::Rgb(0, 50, 0)),
                        ),
                        Span::styled(
                            " + ".to_string(),
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD)
                                .bg(Color::Rgb(0, 50, 0)),
                        ),
                    ];

                    // Highlight the entire line with green background
                    line_spans.push(Span::styled(
                        line_content.to_string(),
                        Style::default().fg(Color::White).bg(Color::Rgb(0, 50, 0)),
                    ));

                    lines.push(Line::from(line_spans));
                }
            }
        }
    }

    Ok(lines)
}
