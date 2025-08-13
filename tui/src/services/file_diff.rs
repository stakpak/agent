use similar::TextDiff;
use ratatui::text::{Line, Span};
use ratatui::style::{Color, Style, Modifier};
use std::fs;

pub fn preview_str_replace_editor_style(
    file_path: &str,
    old_str: &str,
    new_str: &str,
    replace_all: bool,
) -> Result<Vec<Line<'static>>, std::io::Error> {
    // Read the current file content
    let original_content = fs::read_to_string(file_path)?;
    
    // Create the new content with the replacement
    let new_content = if replace_all {
        original_content.replace(old_str, new_str)
    } else {
        original_content.replacen(old_str, new_str, 1)
    };
    
    // Count changes
    let changes_count = original_content.matches(old_str).count();
    let replacements = if replace_all { changes_count } else { 1.min(changes_count) };
    
    // Create a line-by-line diff
    let diff = TextDiff::from_lines(&original_content, &new_content);
    
    let mut lines = Vec::new();
    
    // Add header
    lines.push(Line::from(vec![
        Span::styled(
            format!("{} file edited", if replacements > 0 { "1" } else { "0" }).to_string(),
            Style::default().fg(Color::White),
        ),
        Span::styled(
            "                                                                                           esc ".to_string(),
            Style::default().fg(Color::Gray),
        ),
        Span::styled(
            "when done".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    
    // Add file path with changes summary
    lines.push(Line::from(vec![
        Span::styled("1/1 ".to_string(), Style::default().fg(Color::White)),
        Span::styled(file_path.to_string(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(" +{} -{}", replacements, replacements).to_string(),
            Style::default().fg(Color::Green),
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
                        Span::styled(
                            line_content.to_string(),
                            Style::default().fg(Color::White),
                        ),
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
                            Style::default().fg(Color::Red),
                        ),
                        Span::styled(
                            "     ".to_string(),  // Empty space for new line number
                            Style::default(),
                        ),
                        Span::styled("- ".to_string(), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                    ];
                    
                    // If the line contains the old string, highlight it
                    if line_content.contains(old_str) {
                        // Split the line around the old string to highlight it
                        let parts: Vec<&str> = line_content.splitn(2, old_str).collect();
                        if parts.len() == 2 {
                            // Before the match
                            if !parts[0].is_empty() {
                                line_spans.push(Span::styled(
                                    parts[0].to_string(),
                                    Style::default().fg(Color::White).bg(Color::Rgb(80, 0, 0)),
                                ));
                            }
                            // The matched string
                            line_spans.push(Span::styled(
                                old_str.to_string(),
                                Style::default().fg(Color::White).bg(Color::Rgb(139, 0, 0)),
                            ));
                            // After the match
                            if !parts[1].is_empty() {
                                line_spans.push(Span::styled(
                                    parts[1].to_string(),
                                    Style::default().fg(Color::White).bg(Color::Rgb(80, 0, 0)),
                                ));
                            }
                        } else {
                            line_spans.push(Span::styled(
                                line_content.to_string(),
                                Style::default().fg(Color::White).bg(Color::Rgb(80, 0, 0)),
                            ));
                        }
                    } else {
                        line_spans.push(Span::styled(
                            line_content.to_string(),
                            Style::default().fg(Color::White).bg(Color::Rgb(80, 0, 0)),
                        ));
                    }
                    
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
                            "     ".to_string(),  // Empty space for old line number
                            Style::default(),
                        ),
                        Span::styled(
                            format!("{:>4} ", new_line_num).to_string(),
                            Style::default().fg(Color::Green),
                        ),
                        Span::styled("+ ".to_string(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    ];
                    
                    // If the line contains the new string, highlight it
                    if line_content.contains(new_str) {
                        // Split the line around the new string to highlight it
                        let parts: Vec<&str> = line_content.splitn(2, new_str).collect();
                        if parts.len() == 2 {
                            // Before the match
                            if !parts[0].is_empty() {
                                line_spans.push(Span::styled(
                                    parts[0].to_string(),
                                    Style::default().fg(Color::White).bg(Color::Rgb(0, 50, 0)),
                                ));
                            }
                            // The matched string
                            line_spans.push(Span::styled(
                                new_str.to_string(),
                                Style::default().fg(Color::White).bg(Color::Rgb(0, 100, 0)),
                            ));
                            // After the match
                            if !parts[1].is_empty() {
                                line_spans.push(Span::styled(
                                    parts[1].to_string(),
                                    Style::default().fg(Color::White).bg(Color::Rgb(0, 50, 0)),
                                ));
                            }
                        } else {
                            line_spans.push(Span::styled(
                                line_content.to_string(),
                                Style::default().fg(Color::White).bg(Color::Rgb(0, 50, 0)),
                            ));
                        }
                    } else {
                        line_spans.push(Span::styled(
                            line_content.to_string(),
                            Style::default().fg(Color::White).bg(Color::Rgb(0, 50, 0)),
                        ));
                    }
                    
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
                            Style::default().fg(Color::Red),
                        ),
                        Span::styled("     ".to_string(), Style::default()),
                        Span::styled("- ".to_string(), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                    ];
                    
                    if line_content.contains(old_str) {
                        let parts: Vec<&str> = line_content.splitn(2, old_str).collect();
                        if parts.len() == 2 {
                            if !parts[0].is_empty() {
                                line_spans.push(Span::styled(
                                    parts[0].to_string(),
                                    Style::default().fg(Color::White).bg(Color::Rgb(80, 0, 0)),
                                ));
                            }
                            line_spans.push(Span::styled(
                                old_str.to_string(),
                                Style::default().fg(Color::White).bg(Color::Rgb(139, 0, 0)),
                            ));
                            if !parts[1].is_empty() {
                                line_spans.push(Span::styled(
                                    parts[1].to_string(),
                                    Style::default().fg(Color::White).bg(Color::Rgb(80, 0, 0)),
                                ));
                            }
                        } else {
                            line_spans.push(Span::styled(
                                line_content.to_string(),
                                Style::default().fg(Color::White).bg(Color::Rgb(80, 0, 0)),
                            ));
                        }
                    } else {
                        line_spans.push(Span::styled(
                            line_content.to_string(),
                            Style::default().fg(Color::White).bg(Color::Rgb(80, 0, 0)),
                        ));
                    }
                    
                    lines.push(Line::from(line_spans));
                }
                
                // Then show inserts
                for idx in 0..new_range.len() {
                    new_line_num += 1;
                    let line_content = diff.new_slices()[new_range.start + idx].trim_end();
                    
                    let mut line_spans = vec![
                        Span::styled("     ".to_string(), Style::default()),
                        Span::styled(
                            format!("{:>4} ", new_line_num).to_string(),
                            Style::default().fg(Color::Green),
                        ),
                        Span::styled("+ ".to_string(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    ];
                    
                    if line_content.contains(new_str) {
                        let parts: Vec<&str> = line_content.splitn(2, new_str).collect();
                        if parts.len() == 2 {
                            if !parts[0].is_empty() {
                                line_spans.push(Span::styled(
                                    parts[0].to_string(),
                                    Style::default().fg(Color::White).bg(Color::Rgb(0, 50, 0)),
                                ));
                            }
                            line_spans.push(Span::styled(
                                new_str.to_string(),
                                Style::default().fg(Color::White).bg(Color::Rgb(0, 100, 0)),
                            ));
                            if !parts[1].is_empty() {
                                line_spans.push(Span::styled(
                                    parts[1].to_string(),
                                    Style::default().fg(Color::White).bg(Color::Rgb(0, 50, 0)),
                                ));
                            }
                        } else {
                            line_spans.push(Span::styled(
                                line_content.to_string(),
                                Style::default().fg(Color::White).bg(Color::Rgb(0, 50, 0)),
                            ));
                        }
                    } else {
                        line_spans.push(Span::styled(
                            line_content.to_string(),
                            Style::default().fg(Color::White).bg(Color::Rgb(0, 50, 0)),
                        ));
                    }
                    
                    lines.push(Line::from(line_spans));
                }
            }
        }
    }
    
    Ok(lines)
}