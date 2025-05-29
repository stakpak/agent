use ratatui::{
    prelude::{Line, Span, Style},
    style::Color,
};
use regex::Regex;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PatternMatch {
    pub full_match: String, // The entire match including tags
    pub content: String,    // Just the content inside the tags
    pub start: usize,       // Start position in the original text
    pub end: usize,         // End position in the original text
}

/// Extract all matches for a given pattern from text
/// Pattern should be a regex with one capture group for the content
pub fn extract_pattern_matches(text: &str, pattern: &str) -> Vec<PatternMatch> {
    let regex = match Regex::new(pattern) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    regex
        .captures_iter(text)
        .filter_map(|cap| {
            cap.get(0).map(|full_match| {
                let content = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                PatternMatch {
                    full_match: full_match.as_str().to_string(),
                    content: content.to_string(),
                    start: full_match.start(),
                    end: full_match.end(),
                }
            })
        })
        .collect()
}

/// Transform a line by applying a pattern and transformation function
pub fn transform_line_with_pattern<F>(text: &str, pattern: &str, transform_fn: F) -> Line<'static>
where
    F: Fn(&str) -> (String, Style),
{
    let matches = extract_pattern_matches(text, pattern);

    if matches.is_empty() {
        return Line::from(text.to_string());
    }

    let mut spans = Vec::new();
    let mut last_end = 0;

    for pattern_match in matches {
        // Add text before the match (if any)
        if pattern_match.start > last_end {
            let before_text = &text[last_end..pattern_match.start];
            if !before_text.is_empty() {
                spans.push(Span::raw(before_text.to_string()));
            }
        }

        // Transform and add the matched content
        let (transformed_text, style) = transform_fn(&pattern_match.content);
        spans.push(Span::styled(transformed_text, style));

        last_end = pattern_match.end;
    }

    // Add remaining text after the last match
    if last_end < text.len() {
        let after_text = &text[last_end..];
        if !after_text.is_empty() {
            spans.push(Span::raw(after_text.to_string()));
        }
    }

    Line::from(spans)
}

/// Helper function to convert spans back to plain text
pub fn spans_to_string(line: &Line) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// Process all lines with a single pattern transformation
pub fn process_lines_with_pattern<F>(
    lines: &[(Line, Style)],
    pattern: &str,
    transform_fn: F,
) -> Vec<(Line<'static>, Style)>
where
    F: Fn(&str) -> (String, Style),
{
    lines
        .iter()
        .map(|(line, style)| {
            let line_text = spans_to_string(line);
            let transformed_line = transform_line_with_pattern(&line_text, pattern, &transform_fn);
            (transformed_line, *style)
        })
        .collect()
}

/// Process checkpoint_id patterns specifically
pub fn process_checkpoint_patterns(
    lines: &[(Line, Style)],
    terminal_width: usize,
) -> Vec<(Line<'static>, Style)> {
    let checkpoint_formatter = |content: &str| -> (String, Style) {
        let checkpoint_text = format!("Checkpoint ID: {}", content);
        let total_len = checkpoint_text.len();
        let terminal_width = terminal_width.max(total_len + 2); // Ensure at least enough space

        // Calculate dashes for left and right
        let dash_total = terminal_width.saturating_sub(total_len);
        let dash_left = dash_total / 2;
        let dash_right = dash_total - dash_left;

        let line = format!(
            "{}{}{}",
            "-".repeat(dash_left),
            checkpoint_text,
            "-".repeat(dash_right)
        );
        (line, Style::default().fg(Color::DarkGray))
    };
    process_lines_with_pattern(
        lines,
        r"<checkpoint_id>([^<]*)</checkpoint_id>",
        checkpoint_formatter,
    )
}

pub fn process_agent_mode_patterns(lines: &[(Line, Style)]) -> Vec<(Line<'static>, Style)> {
    let agent_mode_formatter = |content: &str| -> (String, Style) {
        let icon = "🤖";
        let static_text = "[Agent Mode]:";
        let dynamic = content[..1].to_uppercase() + &content[1..].to_lowercase();
        let formatted = format!("{icon} {static_text} {dynamic}");
        (formatted, Style::default().fg(Color::Cyan))
    };
    process_lines_with_pattern(
        lines,
        r"<agent_mode>([^<]*)</agent_mode>",
        agent_mode_formatter,
    )
}

pub fn process_section_title_patterns(
    lines: &[(Line, Style)],
    tag: &str,
) -> Vec<(Line<'static>, Style)> {
    let pattern = format!(r"<{}>", tag);
    let title = tag[..1].to_uppercase() + &tag[1..].to_lowercase();
    let section_formatter = move |_content: &str| -> (String, Style) {
        (
            title.clone(),
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
    };
    process_lines_with_pattern(lines, &pattern, section_formatter)
}

#[allow(dead_code)]
/// Apply multiple pattern transformations in sequence
pub fn apply_all_pattern_transformations(
    lines: &[(Line, Style)],
    terminal_width: usize,
) -> Vec<(Line<'static>, Style)> {
    // Only process checkpoint patterns for now to avoid the styling loss issue
    process_checkpoint_patterns(lines, terminal_width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_pattern_matches() {
        let text =
            "Hello <checkpoint_id>123</checkpoint_id> world <checkpoint_id>456</checkpoint_id>";
        let pattern = r"<checkpoint_id>([^<]*)</checkpoint_id>";
        let matches = extract_pattern_matches(text, pattern);

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].content, "123");
        assert_eq!(matches[0].full_match, "<checkpoint_id>123</checkpoint_id>");
        assert_eq!(matches[0].start, 6);
        assert_eq!(matches[0].end, 40);

        assert_eq!(matches[1].content, "456");
        assert_eq!(matches[1].full_match, "<checkpoint_id>456</checkpoint_id>");
        assert_eq!(matches[1].start, 47);
        assert_eq!(matches[1].end, 81);
    }

    #[test]
    fn test_extract_pattern_matches_no_matches() {
        let text = "Hello world, no patterns here";
        let pattern = r"<checkpoint_id>([^<]*)</checkpoint_id>";
        let matches = extract_pattern_matches(text, pattern);

        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_extract_pattern_matches_empty_content() {
        let text = "Empty <checkpoint_id></checkpoint_id> content";
        let pattern = r"<checkpoint_id>([^<]*)</checkpoint_id>";
        let matches = extract_pattern_matches(text, pattern);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].content, "");
    }

    #[test]
    fn test_transform_line_with_pattern_simple() {
        let text = "Test <checkpoint_id>123</checkpoint_id> message";
        let pattern = r"<checkpoint_id>([^<]*)</checkpoint_id>";

        let line = transform_line_with_pattern(text, pattern, |content| {
            (format!("[{}]", content), Style::default().fg(Color::Yellow))
        });

        // Should have 3 spans: "Test ", "[123]", " message"
        assert_eq!(line.spans.len(), 3);
        assert_eq!(line.spans[0].content, "Test ");
        assert_eq!(line.spans[1].content, "[123]");
        assert_eq!(line.spans[2].content, " message");

        // Check styling
        assert_eq!(line.spans[1].style.fg, Some(Color::Yellow));
    }

    #[test]
    fn test_transform_line_with_pattern_no_matches() {
        let text = "No patterns in this text";
        let pattern = r"<checkpoint_id>([^<]*)</checkpoint_id>";

        let line = transform_line_with_pattern(text, pattern, |content| {
            (format!("[{}]", content), Style::default().fg(Color::Yellow))
        });

        // Should have 1 span with original text
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "No patterns in this text");
    }

    #[test]
    fn test_transform_line_with_pattern_multiple_matches() {
        let text = "Start <checkpoint_id>123</checkpoint_id> middle <checkpoint_id>456</checkpoint_id> end";
        let pattern = r"<checkpoint_id>([^<]*)</checkpoint_id>";

        let line = transform_line_with_pattern(text, pattern, |content| {
            (format!("[{}]", content), Style::default().fg(Color::Yellow))
        });

        // Should have 5 spans: "Start ", "[123]", " middle ", "[456]", " end"
        assert_eq!(line.spans.len(), 5);
        assert_eq!(line.spans[0].content, "Start ");
        assert_eq!(line.spans[1].content, "[123]");
        assert_eq!(line.spans[2].content, " middle ");
        assert_eq!(line.spans[3].content, "[456]");
        assert_eq!(line.spans[4].content, " end");
    }

    #[test]
    fn test_transform_line_with_pattern_adjacent_matches() {
        let text = "<checkpoint_id>123</checkpoint_id><checkpoint_id>456</checkpoint_id>";
        let pattern = r"<checkpoint_id>([^<]*)</checkpoint_id>";

        let line = transform_line_with_pattern(text, pattern, |content| {
            (format!("[{}]", content), Style::default().fg(Color::Yellow))
        });

        // Should have 2 spans: "[123]", "[456]"
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content, "[123]");
        assert_eq!(line.spans[1].content, "[456]");
    }

    #[test]
    fn test_spans_to_string() {
        let spans = vec![
            Span::raw("Hello "),
            Span::styled("world", Style::default().fg(Color::Red)),
            Span::raw("!"),
        ];
        let line = Line::from(spans);

        let result = spans_to_string(&line);
        assert_eq!(result, "Hello world!");
    }

    #[test]
    fn test_spans_to_string_empty() {
        let line = Line::from("");
        let result = spans_to_string(&line);
        assert_eq!(result, "");
    }

    #[test]
    fn test_process_lines_with_pattern() {
        let lines = vec![
            (
                Line::from("Test <checkpoint_id>123</checkpoint_id> message"),
                Style::default(),
            ),
            (
                Line::from("Another <checkpoint_id>456</checkpoint_id> line"),
                Style::default(),
            ),
            (Line::from("No patterns here"), Style::default()),
        ];

        let processed = process_lines_with_pattern(
            &lines,
            r"<checkpoint_id>([^<]*)</checkpoint_id>",
            |content| (format!("[{}]", content), Style::default().fg(Color::Cyan)),
        );

        assert_eq!(processed.len(), 3);

        // First line should have transformed content
        assert_eq!(processed[0].0.spans.len(), 3);
        assert_eq!(processed[0].0.spans[1].content, "[123]");

        // Second line should have transformed content
        assert_eq!(processed[1].0.spans.len(), 3);
        assert_eq!(processed[1].0.spans[1].content, "[456]");

        // Third line should be unchanged
        assert_eq!(processed[2].0.spans.len(), 1);
        assert_eq!(processed[2].0.spans[0].content, "No patterns here");
    }

    #[test]
    fn test_process_checkpoint_patterns() {
        let lines = vec![
            (
                Line::from("Hello <checkpoint_id>test123</checkpoint_id> world"),
                Style::default(),
            ),
            (Line::from("No checkpoint here"), Style::default()),
        ];

        let processed = process_checkpoint_patterns(&lines, 100);

        assert_eq!(processed.len(), 2);

        // First line should have uppercase content in cyan
        assert_eq!(processed[0].0.spans.len(), 3);
        assert_eq!(
            processed[0].0.spans[1].content,
            "---------------------------------------Checkpoint ID: test123---------------------------------------"
        );
        assert_eq!(
            processed[0].0.spans[1].style.fg,
            Some(Color::DarkGray)
        );

        // Second line should be unchanged
        assert_eq!(processed[1].0.spans.len(), 1);
        assert_eq!(processed[1].0.spans[0].content, "No checkpoint here");
    }

    #[test]
    fn test_apply_all_pattern_transformations() {
        let lines = vec![
            (
                Line::from("Test <checkpoint_id>abc</checkpoint_id> message"),
                Style::default(),
            ),
            (Line::from("Normal line"), Style::default()),
        ];

        let processed = apply_all_pattern_transformations(&lines, 100);

        assert_eq!(processed.len(), 2);

        // First line should have checkpoint transformed to uppercase cyan
        assert_eq!(processed[0].0.spans.len(), 3); // "Test ", "ABC", " message"
        assert_eq!(processed[0].0.spans[0].content, "Test ");
        assert_eq!(
            processed[0].0.spans[1].content,
            "-----------------------------------------Checkpoint ID: abc-----------------------------------------"
        );
        assert_eq!(
            processed[0].0.spans[1].style.fg,
            Some(Color::DarkGray)
        );
        assert_eq!(processed[0].0.spans[2].content, " message");

        // Second line should be unchanged (only 1 span)
        assert_eq!(processed[1].0.spans.len(), 1);
        assert_eq!(processed[1].0.spans[0].content, "Normal line");
    }

    #[test]
    fn test_multiple_patterns_in_sequence() {
        // Test that only checkpoint patterns are processed in apply_all_pattern_transformations
        let lines = vec![(
            Line::from("Start <checkpoint_id>abc</checkpoint_id> end"),
            Style::default(),
        )];

        let processed = apply_all_pattern_transformations(&lines, 100);

        assert_eq!(processed.len(), 1);

        // Should have checkpoint transformation applied
        let text = spans_to_string(&processed[0].0);
        assert!(
            text.contains("-----------------------------------------Checkpoint ID: abc-----------------------------------------")
        ); // Checkpoint should be uppercase
        assert!(text.contains("Start"));
        assert!(text.contains("end"));

        // Verify the actual spans structure
        assert_eq!(processed[0].0.spans.len(), 3); // "Start ", "ABC", " end"
        assert_eq!(
            processed[0].0.spans[1].content,
            "-----------------------------------------Checkpoint ID: abc-----------------------------------------"
        );
        assert_eq!(
            processed[0].0.spans[1].style.fg,
            Some(Color::DarkGray)
        );
    }
}
