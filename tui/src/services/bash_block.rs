use super::message::{extract_full_command_arguments, extract_truncated_command_arguments};
use crate::services::detect_term::AdaptiveColors;
use crate::services::file_diff::{render_file_diff_block, render_file_diff_block_from_args};
use crate::services::markdown_renderer::render_markdown_to_lines;
use crate::services::message::{BubbleColors, extract_command_purpose, get_command_type_name};
use ansi_to_tui::IntoText;
use console::strip_ansi_codes;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use regex::Regex;
use stakpak_shared::models::integrations::openai::{
    ToolCall, ToolCallResult, ToolCallResultStatus, ToolCallStreamInfo,
};
use std::sync::OnceLock;
use unicode_width::UnicodeWidthStr;

#[allow(dead_code)]
pub enum ContentAlignment {
    Left,
    Center,
}

fn term_color(color: Color) -> Color {
    if crate::services::detect_term::should_use_rgb_colors() {
        color
    } else {
        Color::Reset
    }
}

pub fn strip_all_ansi(text: &str) -> String {
    // First pass: console crate (handles 95% of cases efficiently)
    let cleaned = console::strip_ansi_codes(text);

    // Second pass: catch the specific sequences console misses
    static REMAINING: OnceLock<Option<Regex>> = OnceLock::new();
    let maybe_regex = REMAINING.get_or_init(|| {
        Regex::new(concat!(
            r"\x1b\][0-9;]*[^\x07\x1b]*(\x07|\x1b\\)|", // Window titles and other OSC sequences
            r"\\u\{[0-9a-fA-F]+\}|",                    // Unicode escapes
            r"\x07"                                     // Bell
        ))
        .ok()
    });

    if let Some(regex) = maybe_regex {
        regex.replace_all(&cleaned, "").to_string()
    } else {
        cleaned.to_string()
    }
}
// Add this function to preprocess text and handle carriage returns
pub fn preprocess_terminal_output(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut current_line = String::new();
    let text = strip_all_ansi(text);
    let text = text.replace("\r\n", "\n");
    for ch in text.chars() {
        match ch {
            '\r' => {
                // Carriage return - start overwriting the current line
                current_line.clear();
            }
            '\n' => {
                // Newline - finish the current line and start a new one
                lines.push(current_line.clone());
                current_line.clear();
            }
            '\t' => {
                current_line.push_str("    ");
            }
            _ => {
                current_line.push(ch);
            }
        }
    }

    // Don't forget the last line if it doesn't end with newline
    if !current_line.is_empty() {
        lines.push(current_line);
    }

    // Generic filtering: remove empty lines and lines that are just whitespace
    let filtered_lines: Vec<String> = lines
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .collect();

    // If we have no content after filtering, return the original to avoid losing everything
    if filtered_lines.is_empty() && !text.trim().is_empty() {
        return text.to_string();
    }
    filtered_lines.join("\n")
}

// Add this function to calculate actual display width accounting for ANSI and Unicode
fn calculate_display_width(text: &str) -> usize {
    // Strip ANSI codes first, then calculate Unicode width
    let stripped = strip_ansi_codes(text);
    stripped.width()
}

// Add this improved simple text wrapping function (character-based)
fn wrap_text_simple_unicode(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let stripped = strip_ansi_codes(text);
    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for ch in stripped.chars() {
        let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);

        if current_width + char_width > width && !current_line.is_empty() {
            lines.push(current_line.clone());
            current_line.clear();
            current_width = 0;
        }

        current_line.push(ch);
        current_width += char_width;
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

// Word-based text wrapping function
fn wrap_text_by_word(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let stripped = strip_ansi_codes(text);
    let mut lines = Vec::new();

    // First split by newlines to preserve explicit line breaks
    for input_line in stripped.split('\n') {
        let mut current_line = String::new();
        let mut current_width = 0;

        // Handle empty lines from consecutive newlines
        if input_line.is_empty() {
            lines.push(String::new());
            continue;
        }

        for word in input_line.split_inclusive(|c: char| c.is_whitespace()) {
            let word_width = calculate_display_width(word);

            if current_width + word_width > width && !current_line.is_empty() {
                // Word doesn't fit, start new line
                lines.push(current_line.trim_end().to_string());
                current_line.clear();
                current_width = 0;
            }

            // If a single word is longer than width, we need to break it
            if word_width > width && current_line.is_empty() {
                // Break the long word by character
                let mut word_part = String::new();
                let mut part_width = 0;
                for ch in word.chars() {
                    let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
                    if part_width + char_width > width && !word_part.is_empty() {
                        lines.push(word_part);
                        word_part = String::new();
                        part_width = 0;
                    }
                    word_part.push(ch);
                    part_width += char_width;
                }
                if !word_part.is_empty() {
                    current_line = word_part;
                    current_width = part_width;
                }
            } else {
                current_line.push_str(word);
                current_width += word_width;
            }
        }

        if !current_line.is_empty() {
            lines.push(current_line.trim_end().to_string());
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

// Helper function to wrap text while preserving ANSI codes
fn wrap_ansi_text(text: &str, width: usize) -> Vec<String> {
    // First preprocess to handle carriage returns and terminal control sequences
    let preprocessed = preprocess_terminal_output(text);

    // Convert to ratatui text first to parse ANSI codes
    let ratatui_text = match preprocessed.into_text() {
        Ok(parsed) => parsed,
        Err(_) => {
            // Fallback: just split by width using stripped text
            let stripped = strip_ansi_codes(&preprocessed);
            return wrap_text_simple_unicode(&stripped, width);
        }
    };

    let mut wrapped_lines = Vec::new();

    for line in ratatui_text.lines {
        if line.spans.is_empty() {
            wrapped_lines.push(String::new());
            continue;
        }

        let mut current_line = String::new();
        let mut current_width = 0;

        for span in line.spans {
            let span_text = &span.content;
            let span_display_width = calculate_display_width(span_text);

            if current_width + span_display_width <= width {
                // Span fits on current line
                current_line.push_str(span_text);
                current_width += span_display_width;
            } else if current_width == 0 {
                // Span is too long for a line by itself, so we must wrap it.
                let wrapped_span = wrap_text_simple_unicode(span_text, width);
                let num_wrapped = wrapped_span.len();
                if num_wrapped > 0 {
                    // Add all but the last part as full lines.
                    if let Some((last, elements)) = wrapped_span.split_last() {
                        for element in elements {
                            wrapped_lines.push(element.clone());
                        }
                        // The last part becomes the current line.
                        current_line = last.clone();
                        current_width = calculate_display_width(&current_line);
                    }
                }
            } else {
                // Start a new line
                wrapped_lines.push(current_line.clone());
                current_line = span_text.to_string();
                current_width = span_display_width;
            }
        }

        if !current_line.is_empty() || current_width > 0 {
            wrapped_lines.push(current_line);
        }
    }

    if wrapped_lines.is_empty() {
        wrapped_lines.push(String::new());
    }

    wrapped_lines
}

#[allow(clippy::too_many_arguments)]
pub fn render_styled_block_ansi_to_tui(
    content: &str,
    _outside_title: &str,
    bubble_title: &str,
    colors: Option<BubbleColors>,
    terminal_width: usize,
    _tool_type: &str,
    content_alignment: Option<ContentAlignment>,
) -> Vec<Line<'static>> {
    let content_width = if terminal_width > 4 {
        terminal_width - 4
    } else {
        40
    };

    let inner_width = content_width;
    let horizontal_line = "─".repeat(inner_width + 2);

    // Determine colors
    let (border_color, _title_color, content_color) = if let Some(ref c) = colors {
        (c.border_color, c.title_color, c.content_color)
    } else {
        (Color::Cyan, Color::Cyan, Color::Cyan)
    };

    // Create colored borders
    let bottom_border = Line::from(vec![Span::styled(
        format!("╰{}╯", horizontal_line),
        Style::default().fg(border_color),
    )]);

    // Strip ANSI codes for title border calculation
    let stripped_title = strip_ansi_codes(bubble_title);
    let title_border = {
        let title_width = calculate_display_width(&stripped_title);
        if title_width <= inner_width {
            let remaining_dashes = inner_width + 2 - title_width;
            Line::from(vec![Span::styled(
                format!("╭{}{}", bubble_title, "─".repeat(remaining_dashes)) + "╮",
                Style::default().fg(border_color),
            )])
        } else {
            // Truncate based on display width, not character count
            let mut truncated_chars = String::new();
            let mut current_width = 0;
            for ch in stripped_title.chars() {
                let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
                if current_width + char_width <= inner_width {
                    truncated_chars.push(ch);
                    current_width += char_width;
                } else {
                    break;
                }
            }
            Line::from(vec![Span::styled(
                format!("╭{}─╮", truncated_chars),
                Style::default().fg(border_color),
            )])
        }
    };

    // Preprocess content to handle terminal control sequences
    let preprocessed_content = preprocess_terminal_output(content);

    // Convert ANSI content to ratatui Text
    let ratatui_text = preprocessed_content
        .into_text()
        .unwrap_or_else(|_| ratatui::text::Text::from(preprocessed_content.clone()));

    // Create lines with compact style similar to result blocks
    let mut formatted_lines = Vec::new();
    formatted_lines.push(title_border);

    // Use compact indentation similar to result blocks
    let line_indent = "  "; // 2 spaces like result blocks

    // Determine alignment
    let alignment = content_alignment.unwrap_or(ContentAlignment::Left);

    for text_line in ratatui_text.lines {
        if text_line.spans.is_empty() {
            // Empty line with border
            let line_spans = vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(format!(" {}", " ".repeat(inner_width))),
                Span::styled(" │", Style::default().fg(border_color)),
            ];
            formatted_lines.push(Line::from(line_spans));
            continue;
        }

        // Check if line needs wrapping
        let display_width: usize = text_line
            .spans
            .iter()
            .map(|span| calculate_display_width(&span.content))
            .sum();

        // Add compact indentation to content width calculation
        let content_display_width = display_width + line_indent.len();

        if content_display_width <= inner_width {
            // Line fits, add with compact style
            let padding_needed = inner_width - content_display_width;
            let (left_pad, right_pad) = match alignment {
                ContentAlignment::Left => (0, padding_needed),
                ContentAlignment::Center => {
                    (padding_needed / 2, padding_needed - (padding_needed / 2))
                }
            };
            let mut line_spans = vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(format!(" {}{}", line_indent, " ".repeat(left_pad))),
            ];
            for s in &text_line.spans {
                line_spans.push(Span::styled(
                    s.content.clone(),
                    Style::default().fg(content_color),
                ));
            }
            line_spans.push(Span::from(" ".repeat(right_pad)));
            line_spans.push(Span::styled(" │", Style::default().fg(border_color)));

            formatted_lines.push(Line::from(line_spans));
        } else {
            // Line needs wrapping - use available width minus indentation
            let available_for_content = inner_width - line_indent.len();
            let original_line: String = text_line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect();

            let wrapped_lines = wrap_ansi_text(&original_line, available_for_content);

            for wrapped_line in wrapped_lines {
                let wrapped_ratatui = wrapped_line
                    .clone()
                    .into_text()
                    .unwrap_or_else(|_| ratatui::text::Text::from(wrapped_line.clone()));

                if let Some(first_line) = wrapped_ratatui.lines.first() {
                    let wrapped_display_width: usize = first_line
                        .spans
                        .iter()
                        .map(|span| calculate_display_width(&span.content))
                        .sum();

                    let total_content_width = wrapped_display_width + line_indent.len();
                    let padding_needed = inner_width.saturating_sub(total_content_width);
                    let (left_pad, right_pad) = match alignment {
                        ContentAlignment::Left => (0, padding_needed),
                        ContentAlignment::Center => {
                            (padding_needed / 2, padding_needed - (padding_needed / 2))
                        }
                    };
                    let mut line_spans = vec![
                        Span::styled("│", Style::default().fg(border_color)),
                        Span::from(format!(" {}{}", line_indent, " ".repeat(left_pad))),
                    ];
                    for s in &first_line.spans {
                        line_spans.push(Span::styled(
                            s.content.clone(),
                            Style::default().fg(content_color),
                        ));
                    }
                    line_spans.push(Span::from(" ".repeat(right_pad)));
                    line_spans.push(Span::styled(" │", Style::default().fg(border_color)));

                    formatted_lines.push(Line::from(line_spans));
                }
            }
        }
    }

    formatted_lines.push(bottom_border);

    let mut owned_lines: Vec<Line<'static>> = Vec::new();
    owned_lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));
    // Convert to owned lines for storage
    let final_lines: Vec<Line<'static>> = formatted_lines
        .into_iter()
        .map(|line| {
            let owned_spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|span| Span::styled(span.content.into_owned(), span.style))
                .collect();
            Line::from(owned_spans)
        })
        .collect();

    owned_lines.extend(final_lines);

    // add spaceing marker
    owned_lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));
    owned_lines
}

/// Simple text formatting function that processes content and wraps it to fit terminal width
/// This is a stripped-down version of RenderCommandCollapsedResult without borders or styling
pub fn format_text_content(content: &str, terminal_width: usize) -> Vec<Line<'static>> {
    let content_width = if terminal_width > 4 {
        terminal_width - 4
    } else {
        40
    };

    let inner_width = content_width;

    // Preprocess content to handle terminal control sequences
    let preprocessed_content = preprocess_terminal_output(content);

    // Convert ANSI content to ratatui Text
    let ratatui_text = preprocessed_content
        .into_text()
        .unwrap_or_else(|_| ratatui::text::Text::from(preprocessed_content.clone()));

    let mut formatted_lines = Vec::new();

    for text_line in ratatui_text.lines {
        if text_line.spans.is_empty() {
            // Empty line
            formatted_lines.push(Line::from(vec![Span::from("")]));
            continue;
        }

        // Check if line needs wrapping
        let display_width: usize = text_line
            .spans
            .iter()
            .map(|span| calculate_display_width(&span.content))
            .sum();

        if display_width <= inner_width {
            // Line fits, add as-is
            let mut line_spans = Vec::new();
            for s in &text_line.spans {
                line_spans.push(Span::styled(
                    s.content.clone(),
                    Style::default().fg(Color::Reset),
                ));
            }
            formatted_lines.push(Line::from(line_spans));
        } else {
            // Line needs wrapping
            let original_line: String = text_line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect();

            let wrapped_lines = wrap_ansi_text(&original_line, inner_width);

            for wrapped_line in wrapped_lines {
                let wrapped_ratatui = wrapped_line
                    .clone()
                    .into_text()
                    .unwrap_or_else(|_| ratatui::text::Text::from(wrapped_line.clone()));

                if let Some(first_line) = wrapped_ratatui.lines.first() {
                    let mut line_spans = Vec::new();
                    for s in &first_line.spans {
                        line_spans.push(Span::styled(
                            s.content.clone(),
                            Style::default().fg(Color::Reset),
                        ));
                    }
                    formatted_lines.push(Line::from(line_spans));
                }
            }
        }
    }

    // Convert to owned lines
    formatted_lines
        .into_iter()
        .map(|line| {
            let owned_spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|span| Span::styled(span.content.into_owned(), span.style))
                .collect();
            Line::from(owned_spans)
        })
        .collect()
}

pub fn extract_bash_block_info(
    tool_call: &ToolCall,
    output: &str,
) -> (String, String, String, BubbleColors) {
    let full_command = extract_full_command_arguments(tool_call);
    let command = if full_command == "unknown command" {
        output.to_string()
    } else {
        full_command
    };
    let outside_title = get_command_type_name(tool_call);
    let bubble_title = extract_command_purpose(&command, &outside_title);
    let colors = match crate::utils::strip_tool_name(&tool_call.function.name) {
        "create_file" => BubbleColors {
            border_color: Color::Green,
            title_color: term_color(Color::Gray),
            content_color: Color::LightGreen,
            tool_type: "Create File".to_string(),
        },
        "edit_file" => BubbleColors {
            border_color: Color::Yellow,
            title_color: term_color(Color::Gray),
            content_color: Color::LightYellow,
            tool_type: "Edit File".to_string(),
        },
        "run_command" => BubbleColors {
            border_color: Color::Cyan,
            title_color: Color::Yellow,
            content_color: term_color(Color::Gray),
            tool_type: "Run Command".to_string(),
        },
        "read_file" => BubbleColors {
            border_color: Color::Magenta,
            title_color: term_color(Color::Gray),
            content_color: Color::LightMagenta,
            tool_type: "Read File".to_string(),
        },
        "delete_file" => BubbleColors {
            border_color: Color::Red,
            title_color: term_color(Color::Gray),
            content_color: Color::LightRed,
            tool_type: "Delete File".to_string(),
        },
        "dynamic_subagent_task" => {
            let is_sandbox =
                serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments)
                    .ok()
                    .and_then(|a| a.get("enable_sandbox").and_then(|v| v.as_bool()))
                    .unwrap_or(false);
            if is_sandbox {
                BubbleColors {
                    border_color: Color::Green,
                    title_color: Color::Green,
                    content_color: term_color(Color::Gray),
                    tool_type: "Subagent [sandboxed]".to_string(),
                }
            } else {
                BubbleColors {
                    border_color: Color::Magenta,
                    title_color: Color::Magenta,
                    content_color: term_color(Color::Gray),
                    tool_type: "Subagent".to_string(),
                }
            }
        }
        _ => BubbleColors {
            border_color: Color::Cyan,
            title_color: term_color(Color::White),
            content_color: term_color(Color::Gray),
            tool_type: "unknown".to_string(),
        },
    };
    (command, outside_title, bubble_title, colors)
}

/// Render a streaming block showing only the last 3 lines with a "ctrl+t to expand" hint
/// This is used for run_command tool calls that are actively streaming output
pub fn render_streaming_block_compact(
    content: &str,
    bubble_title: &str,
    colors: Option<BubbleColors>,
    terminal_width: usize,
) -> Vec<Line<'static>> {
    let content_width = if terminal_width > 4 {
        terminal_width - 4
    } else {
        40
    };

    let inner_width = content_width;
    let horizontal_line = "─".repeat(inner_width + 2);

    // Determine colors
    let (border_color, _title_color, content_color) = if let Some(ref c) = colors {
        (c.border_color, c.title_color, c.content_color)
    } else {
        (Color::DarkGray, Color::DarkGray, Color::DarkGray)
    };

    // Create colored borders
    let bottom_border = Line::from(vec![Span::styled(
        format!("╰{}╯", horizontal_line),
        Style::default().fg(border_color),
    )]);

    // Strip ANSI codes for title border calculation
    let stripped_title = strip_ansi_codes(bubble_title);
    let title_border = {
        let title_width = calculate_display_width(&stripped_title);
        if title_width <= inner_width {
            let remaining_dashes = inner_width + 2 - title_width;
            Line::from(vec![Span::styled(
                format!("╭{}{}", bubble_title, "─".repeat(remaining_dashes)) + "╮",
                Style::default().fg(border_color),
            )])
        } else {
            let mut truncated_chars = String::new();
            let mut current_width = 0;
            for ch in stripped_title.chars() {
                let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
                if current_width + char_width <= inner_width {
                    truncated_chars.push(ch);
                    current_width += char_width;
                } else {
                    break;
                }
            }
            Line::from(vec![Span::styled(
                format!("╭{}─╮", truncated_chars),
                Style::default().fg(border_color),
            )])
        }
    };

    // Preprocess content to handle terminal control sequences
    let preprocessed_content = preprocess_terminal_output(content);

    // Split into lines and take only the last 3
    let all_content_lines: Vec<&str> = preprocessed_content.lines().collect();

    let content_joined_lines = all_content_lines.join("\n");
    let ratatui_text = content_joined_lines
        .clone()
        .into_text()
        .unwrap_or_else(|_| ratatui::text::Text::from(content_joined_lines.clone()));

    let mut formatted_lines = Vec::new();
    formatted_lines.push(title_border);

    let line_indent = "  ";

    for text_line in ratatui_text.lines {
        if text_line.spans.is_empty() {
            let line_spans = vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(format!(" {}", " ".repeat(inner_width))),
                Span::styled(" │", Style::default().fg(border_color)),
            ];
            formatted_lines.push(Line::from(line_spans));
            continue;
        }

        let display_width: usize = text_line
            .spans
            .iter()
            .map(|span| calculate_display_width(&span.content))
            .sum();

        let content_display_width = display_width + line_indent.len();

        if content_display_width <= inner_width {
            let padding_needed = inner_width - content_display_width;
            let mut line_spans = vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(format!(" {}", line_indent)),
            ];
            for s in &text_line.spans {
                line_spans.push(Span::styled(
                    s.content.clone(),
                    Style::default().fg(content_color),
                ));
            }
            line_spans.push(Span::from(" ".repeat(padding_needed)));
            line_spans.push(Span::styled(" │", Style::default().fg(border_color)));
            formatted_lines.push(Line::from(line_spans));
        } else {
            // Line needs wrapping - use available width minus indentation
            let available_for_content = inner_width - line_indent.len();
            let original_line: String = text_line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect();

            let wrapped_lines = wrap_ansi_text(&original_line, available_for_content);

            for wrapped_line in wrapped_lines {
                let wrapped_ratatui = wrapped_line
                    .clone()
                    .into_text()
                    .unwrap_or_else(|_| ratatui::text::Text::from(wrapped_line.clone()));

                if let Some(first_line) = wrapped_ratatui.lines.first() {
                    let wrapped_display_width: usize = first_line
                        .spans
                        .iter()
                        .map(|span| calculate_display_width(&span.content))
                        .sum();

                    let total_content_width = wrapped_display_width + line_indent.len();
                    let padding_needed = inner_width.saturating_sub(total_content_width);
                    let mut line_spans = vec![
                        Span::styled("│", Style::default().fg(border_color)),
                        Span::from(format!(" {}", line_indent)),
                    ];
                    for s in &first_line.spans {
                        line_spans.push(Span::styled(
                            s.content.clone(),
                            Style::default().fg(content_color),
                        ));
                    }
                    line_spans.push(Span::from(" ".repeat(padding_needed)));
                    line_spans.push(Span::styled(" │", Style::default().fg(border_color)));
                    formatted_lines.push(Line::from(line_spans));
                }
            }
        }
    }

    formatted_lines.push(bottom_border);

    let mut owned_lines: Vec<Line<'static>> = Vec::new();
    owned_lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));

    let final_lines: Vec<Line<'static>> = formatted_lines
        .into_iter()
        .map(|line| {
            let owned_spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|span| Span::styled(span.content.into_owned(), span.style))
                .collect();
            Line::from(owned_spans)
        })
        .collect();

    owned_lines.extend(final_lines);
    owned_lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));
    owned_lines
}

pub fn render_styled_header_and_borders(
    title: &str,
    content_lines: Vec<Line<'static>>,
    colors: Option<BubbleColors>,
    terminal_width: usize,
) -> Vec<Line<'static>> {
    let content_width = if terminal_width > 4 {
        terminal_width - 4
    } else {
        40
    };
    let inner_width = content_width;
    let horizontal_line = "─".repeat(inner_width + 2);

    let border_color = colors.map(|c| c.border_color).unwrap_or(Color::Cyan);

    // Create title border
    let stripped_title = strip_ansi_codes(title);
    let title_border = {
        let title_width = calculate_display_width(&stripped_title);
        if title_width <= inner_width {
            let remaining_dashes = inner_width + 2 - title_width;
            Line::from(vec![Span::styled(
                format!("╭{}{}╮", title, "─".repeat(remaining_dashes)),
                Style::default().fg(border_color),
            )])
        } else {
            let mut truncated_chars = String::new();
            let mut current_width = 0;
            for ch in stripped_title.chars() {
                let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
                if current_width + char_width <= inner_width {
                    truncated_chars.push(ch);
                    current_width += char_width;
                } else {
                    break;
                }
            }
            Line::from(vec![Span::styled(
                format!("╭{}─╮", truncated_chars),
                Style::default().fg(border_color),
            )])
        }
    };

    let bottom_border = Line::from(vec![Span::styled(
        format!("╰{}╯", horizontal_line),
        Style::default().fg(border_color),
    )]);

    let mut result = Vec::new();
    result.push(title_border);
    // Add side borders to each content line
    for line in content_lines {
        let mut bordered_line = Vec::new();
        bordered_line.push(Span::styled("│", Style::default().fg(border_color)));
        bordered_line.push(Span::from(" "));

        // Calculate content width and truncate if needed
        let mut total_width: usize = 0;
        let mut truncated_spans = Vec::new();

        for span in line.spans.iter() {
            let span_width = calculate_display_width(&span.content);
            if total_width + span_width <= inner_width {
                // Span fits completely
                truncated_spans.push(span.clone());
                total_width += span_width;
            } else if total_width < inner_width {
                // Need to truncate this span
                let remaining_width = inner_width - total_width;
                let mut truncated_content = String::new();
                let mut char_width = 0;
                for ch in span.content.chars() {
                    let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
                    if char_width + ch_width <= remaining_width {
                        truncated_content.push(ch);
                        char_width += ch_width;
                    } else {
                        break;
                    }
                }
                if !truncated_content.is_empty() {
                    truncated_spans.push(Span::styled(truncated_content, span.style));
                }
                total_width = inner_width;
                break; // No more content can fit
            }
            // else: already at or past inner_width, skip this span
        }

        // Add the truncated content spans
        bordered_line.extend(truncated_spans);

        // Add padding to fill the width
        let padding_needed = inner_width.saturating_sub(total_width);
        if padding_needed > 0 {
            bordered_line.push(Span::from(" ".repeat(padding_needed)));
        }

        bordered_line.push(Span::styled(" │", Style::default().fg(border_color)));
        result.push(Line::from(bordered_line));
    }
    result.push(bottom_border);
    result
}

/// Render file diff for full screen popup - shows diff lines with context
/// Uses the same diff-only approach as the TUI view for consistency
/// Returns None if there's no diff to show (e.g., old_str not found)
pub fn render_file_diff_full(
    tool_call: &ToolCall,
    terminal_width: usize,
    do_show: Option<bool>,
) -> Option<Vec<Line<'static>>> {
    // Get diff lines - use the truncated version which starts from first change
    // but we'll show all diff lines without truncation for the full screen view
    let (_truncated_diff_lines, full_diff_lines) =
        render_file_diff_block_from_args(tool_call, terminal_width);

    let title: String = get_command_type_name(tool_call);

    // If diff is empty, return None to signal caller should use fallback rendering
    if full_diff_lines.is_empty() {
        return None;
    }

    if !do_show.unwrap_or(false) {
        return Some(Vec::new());
    }

    // render header dot - don't show path since it's already in the diff content header line
    let spacing_marker = Line::from(vec![Span::from("SPACING_MARKER")]);

    let mut result = vec![spacing_marker.clone()];
    result.extend(render_styled_header_with_dot(
        &title,
        "", // Hide path here - it's shown in the diff content below
        Some(LinesColors {
            dot: Color::LightGreen,
            title: Color::White,
            command: AdaptiveColors::text(),
            message: Color::LightGreen,
        }),
        Some(terminal_width),
    ));
    result.push(spacing_marker.clone());
    result.extend(full_diff_lines);
    result.push(spacing_marker); // Add spacing marker at the end

    Some(result)
}

pub fn render_file_diff(tool_call: &ToolCall, terminal_width: usize) -> Vec<Line<'static>> {
    let tool_name = crate::utils::strip_tool_name(&tool_call.function.name);
    if tool_name == "str_replace" || tool_name == "create" {
        // Use full diff (not truncated) for pending approval blocks
        let (_, mut diff_lines) = render_file_diff_block(tool_call, terminal_width);
        // render header dot
        let spacing_marker = Line::from(vec![Span::from("SPACING_MARKER")]);
        if diff_lines.is_empty() {
            return Vec::new();
        }
        diff_lines = [
            vec![Line::from(vec![Span::from(" ")])],
            diff_lines,
            vec![Line::from(vec![Span::from(" ")])],
        ]
        .concat();

        let title = get_command_type_name(tool_call);

        let formatted_title = format!(" {} ", title);
        let colors = Some(BubbleColors {
            border_color: Color::DarkGray,
            title_color: term_color(Color::Reset),
            content_color: Color::Reset,
            tool_type: title,
        });

        let result =
            render_styled_header_and_borders(&formatted_title, diff_lines, colors, terminal_width);

        let adjusted_result = [
            vec![spacing_marker.clone()],
            result,
            vec![spacing_marker.clone()],
        ]
        .concat();

        return adjusted_result;
    }

    Vec::new()
}

pub fn render_bash_block(
    tool_call: &ToolCall,
    output: &str,
    _accepted: bool,
    terminal_width: usize,
    _is_auto_approved: bool,
) -> Vec<Line<'static>> {
    let (command, outside_title, bubble_title, colors) = extract_bash_block_info(tool_call, output);

    render_styled_block_ansi_to_tui(
        &command,
        &outside_title,
        &bubble_title,
        Some(colors.clone()),
        terminal_width,
        crate::utils::strip_tool_name(&tool_call.function.name),
        None,
    )
}

pub fn render_markdown_block(
    preprocessed_result: String,
    command_args: String,
    title: String,
) -> Vec<Line<'static>> {
    let processed_result = preprocess_terminal_output(&preprocessed_result);
    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));
    lines.extend(render_styled_header_with_dot(
        &title,
        &command_args,
        Some(LinesColors {
            dot: Color::LightGreen,
            title: Color::White,
            command: AdaptiveColors::text(),
            message: Color::LightGreen,
        }),
        None, // No width available here
    ));
    lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));
    let content_lines = render_markdown_to_lines(&processed_result.to_string()).unwrap_or_default();

    for line in content_lines {
        lines.push(line);
    }

    lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));
    lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));
    lines
}

/// Render str_replace/create results - clean diff view without borders
/// Uses the same approach as fullscreen popup for consistency
/// Returns None if there's no diff (fallback to standard result rendering)
pub fn render_diff_result_block(tool_call: &ToolCall, width: usize) -> Option<Vec<Line<'static>>> {
    // Use the same clean diff rendering as the fullscreen popup
    render_file_diff_full(tool_call, width, Some(true))
}

pub fn render_result_block(tool_call_result: &ToolCallResult, width: usize) -> Vec<Line<'static>> {
    let tool_call = tool_call_result.call.clone();
    let result = tool_call_result.result.clone();
    let tool_call_status = tool_call_result.status.clone();

    let title: String = get_command_type_name(&tool_call);
    let command_args = extract_truncated_command_arguments(&tool_call, None);

    let is_collapsed = is_collapsed_tool_call(&tool_call);

    if tool_call_status == ToolCallResultStatus::Error {
        return render_bash_block_rejected(&command_args, &title, Some(result.to_string()), None);
    }
    if tool_call_status == ToolCallResultStatus::Cancelled {
        return render_bash_block_rejected(
            &command_args,
            &title,
            Some("Interrupted by user".to_string()),
            None,
        );
    }

    // Handle str_replace/create with diff-only content
    // If render_diff_result_block returns None (no diff), fall through to standard rendering
    let tool_name = crate::utils::strip_tool_name(&tool_call.function.name);
    if tool_name == "str_replace" || tool_name == "create" {
        // Check for rejected/cancelled in result text
        if result.contains("TOOL_CALL_REJECTED") {
            return render_bash_block_rejected(
                &command_args,
                &title,
                Some("Rejected by user".to_string()),
                None,
            );
        }
        if result.contains("TOOL_CALL_CANCELLED") {
            return render_bash_block_rejected(
                &command_args,
                &title,
                Some("Interrupted by user".to_string()),
                None,
            );
        }

        if let Some(diff_lines) = render_diff_result_block(&tool_call, width) {
            return diff_lines;
        }
        // Fall through to standard result rendering if no diff
    }

    if command_args.contains(".md") && is_collapsed {
        return render_markdown_block(result.clone(), command_args.clone(), title.clone());
    }

    let terminal_width = width;
    let content_width = if terminal_width > 4 {
        terminal_width - 4
    } else {
        40
    };
    let inner_width = content_width;

    let mut lines = Vec::new();

    // Only add borders if not collapsed
    let horizontal_line = if !is_collapsed {
        "─".repeat(inner_width + 2)
    } else {
        String::new()
    };
    let top_border = if !is_collapsed {
        Line::from(vec![Span::styled(
            format!("╭{}╮", horizontal_line),
            Style::default().fg(term_color(Color::Gray)),
        )])
    } else {
        Line::from(vec![Span::from("")])
    };
    let bottom_border = if !is_collapsed {
        Line::from(vec![Span::styled(
            format!("╰{}╯", horizontal_line),
            Style::default().fg(term_color(Color::Gray)),
        )])
    } else {
        Line::from(vec![Span::from("")])
    };

    if !is_collapsed {
        lines.push(top_border);
    }

    // Header line with border - handle multi-line command arguments
    let title_with_args = format!("{} ({})", title, command_args);

    // Calculate available width for the title and arguments
    let available_width = inner_width - 2; // Account for borders and spacing
    let dot_color = if is_collapsed {
        Color::Magenta
    } else {
        Color::LightGreen
    };
    let title_color = if is_collapsed {
        Color::Yellow
    } else {
        term_color(Color::White)
    };
    // Check if the title with arguments fits on one line
    // Also check for embedded newlines - if present, always use multi-line rendering
    let has_newlines = command_args.contains('\n');
    if !has_newlines && title_with_args.len() <= available_width {
        // Single line header
        let mut header_spans = vec![];

        if !is_collapsed {
            header_spans.push(Span::styled(
                "│",
                Style::default().fg(term_color(Color::Gray)),
            ));
            header_spans.push(Span::from(" "));
        }

        header_spans.push(Span::styled(
            "● ",
            Style::default().fg(dot_color).add_modifier(Modifier::BOLD),
        ));
        header_spans.push(Span::styled(
            title.to_string(),
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        ));
        header_spans.push(Span::styled(
            format!(" ({})", command_args),
            Style::default().fg(term_color(Color::Gray)),
        ));

        if !is_collapsed {
            let header_content_width = 2 + title_with_args.len();
            let header_padding = inner_width.saturating_sub(header_content_width);
            header_spans.push(Span::from(" ".repeat(header_padding)));
            header_spans.push(Span::styled(
                " │",
                Style::default().fg(term_color(Color::Gray)),
            ));
        }

        lines.push(Line::from(header_spans));
    } else {
        // Multi-line header - title on first line, arguments on subsequent lines
        let mut header_spans = vec![];

        if !is_collapsed {
            header_spans.push(Span::styled(
                "│",
                Style::default().fg(term_color(Color::Gray)),
            ));
            header_spans.push(Span::from(" "));
        }

        header_spans.push(Span::styled(
            "● ",
            Style::default().fg(dot_color).add_modifier(Modifier::BOLD),
        ));
        header_spans.push(Span::styled(
            title.to_string(),
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        ));

        if !is_collapsed {
            let title_content_width = 2 + title.len();
            let title_padding = inner_width.saturating_sub(title_content_width);
            header_spans.push(Span::from(" ".repeat(title_padding)));
            header_spans.push(Span::styled(
                " │",
                Style::default().fg(term_color(Color::Gray)),
            ));
        }

        lines.push(Line::from(header_spans));

        // Render command arguments exactly like content lines
        let line_indent = "  "; // 2 spaces for compact style

        // Wrap the command arguments
        let available_for_content = inner_width - line_indent.len();
        let wrapped_args = wrap_ansi_text(&command_args, available_for_content);

        for (i, wrapped_line) in wrapped_args.iter().enumerate() {
            let wrapped_ratatui = wrapped_line
                .clone()
                .into_text()
                .unwrap_or_else(|_| ratatui::text::Text::from(wrapped_line.clone()));

            if let Some(first_line) = wrapped_ratatui.lines.first() {
                let wrapped_display_width: usize = first_line
                    .spans
                    .iter()
                    .map(|span| calculate_display_width(&span.content))
                    .sum();

                let total_content_width = wrapped_display_width + line_indent.len();
                let padding_needed = inner_width.saturating_sub(total_content_width);
                let padding = " ".repeat(padding_needed);

                let mut line_spans = vec![];

                if !is_collapsed {
                    line_spans.push(Span::styled(
                        "│",
                        Style::default().fg(term_color(Color::Gray)),
                    ));
                    line_spans.push(Span::from(format!(" {}", line_indent)));
                } else {
                    line_spans.push(Span::from(line_indent));
                }

                // Add the argument content with parentheses
                if i == 0 {
                    // First line - start with opening parenthesis
                    if let Some(first_span) = first_line.spans.first() {
                        line_spans.push(Span::styled(
                            format!("{}", first_span.content),
                            Style::default().fg(term_color(Color::Gray)),
                        ));
                    }
                } else {
                    // Continuation lines - just the content
                    line_spans.extend(first_line.spans.clone());
                }

                line_spans.push(Span::from(padding));

                if !is_collapsed {
                    line_spans.push(Span::styled(
                        " │",
                        Style::default().fg(term_color(Color::Gray)),
                    ));
                }

                lines.push(Line::from(line_spans));
            }
        }

        // Close the parentheses on the last line
        if let Some(last_line) = lines.last_mut()
            && let Some(last_content_span) = last_line
                .spans
                .iter_mut()
                .rev()
                .find(|span| span.style.fg == Some(Color::White) && !span.content.contains("│"))
        {
            last_content_span.content = format!("{}", last_content_span.content).into();
        }
    }
    if is_collapsed {
        lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));
    }

    // Use compact indentation like bash blocks
    let line_indent = "  "; // 2 spaces for compact style

    let preprocessed_result: String = preprocess_terminal_output(&result);
    let result_text = ratatui::text::Text::from(preprocessed_result);

    for text_line in result_text.iter() {
        if text_line.spans.is_empty() {
            // Empty line with border
            let mut line_spans = vec![];
            if !is_collapsed {
                line_spans.push(Span::styled(
                    "│",
                    Style::default().fg(term_color(Color::Gray)),
                ));
                line_spans.push(Span::from(format!(" {}", " ".repeat(inner_width))));
                line_spans.push(Span::styled(
                    " │",
                    Style::default().fg(term_color(Color::Gray)),
                ));
            }
            lines.push(Line::from(line_spans));
            continue;
        }

        // Check if line fits within available width
        let display_width: usize = text_line
            .spans
            .iter()
            .map(|span| calculate_display_width(&span.content))
            .sum();

        let content_display_width = display_width + line_indent.len();

        if content_display_width <= inner_width {
            // Line fits, add with compact style and border
            let padding_needed = inner_width - content_display_width;
            let padding = " ".repeat(padding_needed);

            let mut line_spans = vec![];

            if !is_collapsed {
                line_spans.push(Span::styled(
                    "│",
                    Style::default().fg(term_color(Color::Gray)),
                ));
                line_spans.push(Span::from(format!(" {}", line_indent)));
            } else {
                line_spans.push(Span::from(line_indent));
            }

            // Apply Rgb(180,180,180) color to result text
            for span in &text_line.spans {
                line_spans.push(Span::styled(
                    span.content.clone(),
                    Style::default().fg(term_color(Color::White)),
                ));
            }
            line_spans.push(Span::from(padding));

            if !is_collapsed {
                line_spans.push(Span::styled(
                    " │",
                    Style::default().fg(term_color(Color::Gray)),
                ));
            }

            lines.push(Line::from(line_spans));
        } else {
            // Line needs wrapping - use available width minus indentation
            let available_for_content = inner_width - line_indent.len();
            let original_line: String = text_line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect();

            let wrapped_lines = wrap_ansi_text(&original_line, available_for_content);

            for wrapped_line in wrapped_lines {
                let wrapped_ratatui = wrapped_line
                    .clone()
                    .into_text()
                    .unwrap_or_else(|_| ratatui::text::Text::from(wrapped_line.clone()));

                if let Some(first_line) = wrapped_ratatui.lines.first() {
                    let wrapped_display_width: usize = first_line
                        .spans
                        .iter()
                        .map(|span| calculate_display_width(&span.content))
                        .sum();

                    let total_content_width = wrapped_display_width + line_indent.len();
                    let padding_needed = inner_width.saturating_sub(total_content_width);
                    let padding = " ".repeat(padding_needed);

                    let mut line_spans = vec![];

                    if !is_collapsed {
                        line_spans.push(Span::styled(
                            "│",
                            Style::default().fg(term_color(Color::Gray)),
                        ));
                        line_spans.push(Span::from(format!(" {}", line_indent)));
                    } else {
                        line_spans.push(Span::from(line_indent));
                    }

                    line_spans.extend(first_line.spans.clone());
                    line_spans.push(Span::from(padding));

                    if !is_collapsed {
                        line_spans.push(Span::styled(
                            " │",
                            Style::default().fg(term_color(Color::Gray)),
                        ));
                    }

                    lines.push(Line::from(line_spans));
                }
            }
        }
    }

    if !is_collapsed {
        lines.push(bottom_border);
    }
    lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));

    // Convert to owned lines
    let owned_lines: Vec<Line<'static>> = lines
        .into_iter()
        .map(|line| {
            let owned_spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|span| Span::styled(span.content.into_owned(), span.style))
                .collect();
            Line::from(owned_spans)
        })
        .collect();

    owned_lines
}

// Function to render a rejected bash command (when user selects "No")
pub fn render_bash_block_rejected(
    command_name: &str,
    title: &str,
    message: Option<String>,
    color: Option<Color>,
) -> Vec<Line<'static>> {
    let colors = color.map(|c| LinesColors {
        dot: c,
        title: term_color(Color::White),
        command: AdaptiveColors::text(),
        message: c,
    });
    render_styled_lines(command_name, title, message, colors, None)
}

#[derive(Clone)]
pub struct LinesColors {
    pub dot: Color,
    pub title: Color,
    pub command: Color,
    pub message: Color,
}

/// Public version of render_styled_header_with_dot for use in message.rs
pub fn render_styled_header_with_dot_public(
    title: &str,
    command_name: &str,
    colors: Option<LinesColors>,
    terminal_width: Option<usize>,
) -> Vec<Line<'static>> {
    render_styled_header_with_dot(title, command_name, colors, terminal_width)
}

fn render_styled_header_with_dot(
    title: &str,
    command_name: &str,
    colors: Option<LinesColors>,
    terminal_width: Option<usize>,
) -> Vec<Line<'static>> {
    let colors = colors.unwrap_or(LinesColors {
        dot: Color::LightRed,
        title: term_color(Color::White),
        command: AdaptiveColors::text(),
        message: Color::LightRed,
    });

    // Use actual terminal width if provided, otherwise fall back to 100
    let max_line_width: usize = terminal_width.unwrap_or(100);
    // First line prefix: "● " (2) + title + " (" (2)
    let first_line_prefix_len: usize = 2 + title.chars().count() + 2;
    // Continuation line prefix: just some indentation (2 spaces)
    let continuation_indent = "  ";
    let continuation_prefix_len: usize = continuation_indent.len();

    // Calculate available width for command on first line
    let first_line_available = max_line_width.saturating_sub(first_line_prefix_len + 1); // +1 for closing paren

    // Wrap the command text
    let wrapped_lines = wrap_text_simple_unicode(command_name, first_line_available);

    let mut result_lines = Vec::new();

    if wrapped_lines.len() <= 1 {
        // Single line - command fits on one line with title
        let mut spans = vec![
            Span::styled(
                "● ",
                Style::default().fg(colors.dot).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                title.to_string(),
                Style::default()
                    .fg(colors.title)
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        // Only add command in parentheses if it's not empty
        if !command_name.is_empty() {
            spans.push(Span::styled(
                format!(" ({})", command_name),
                Style::default().fg(colors.command),
            ));
        }
        result_lines.push(Line::from(spans));
    } else {
        // Multi-line - need to wrap
        // First line: title + start of command
        result_lines.push(Line::from(vec![
            Span::styled(
                "● ",
                Style::default().fg(colors.dot).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                title.to_string(),
                Style::default()
                    .fg(colors.title)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" ({}", wrapped_lines.first().unwrap_or(&String::new())),
                Style::default().fg(colors.command),
            ),
        ]));

        // Middle lines - use wider width since we only have the indent
        let continuation_available = max_line_width.saturating_sub(continuation_prefix_len);
        let remaining_text: String = wrapped_lines[1..].join(" ");
        let rewrapped = wrap_text_simple_unicode(&remaining_text, continuation_available);

        for (i, line) in rewrapped.iter().enumerate() {
            let is_last = i == rewrapped.len() - 1;
            let line_content = if is_last {
                format!("{})", line)
            } else {
                line.clone()
            };

            result_lines.push(Line::from(vec![
                Span::from(continuation_indent.to_string()),
                Span::styled(line_content, Style::default().fg(colors.command)),
            ]));
        }
    }

    result_lines
}

pub fn render_styled_lines(
    command_name: &str,
    title: &str,
    message: Option<String>,
    colors: Option<LinesColors>,
    terminal_width: Option<usize>,
) -> Vec<Line<'static>> {
    render_styled_lines_with_content(command_name, title, None, message, colors, terminal_width)
}

pub fn render_styled_lines_with_content(
    command_name: &str,
    title: &str,
    content: Option<Vec<Line<'static>>>,
    message: Option<String>,
    colors: Option<LinesColors>,
    terminal_width: Option<usize>,
) -> Vec<Line<'static>> {
    let colors = colors.unwrap_or(LinesColors {
        dot: Color::LightRed,
        title: term_color(Color::White),
        command: AdaptiveColors::text(),
        message: Color::LightRed,
    });

    let mut lines = Vec::new();

    lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));

    // Always use single-line truncated header - command stays on same line as title
    lines.extend(render_styled_header_with_dot(
        title,
        command_name,
        Some(colors.clone()),
        terminal_width,
    ));

    // Render optional content lines between header and message
    if let Some(content_lines) = content {
        for content_line in content_lines {
            // Build spans with indentation prefix and DarkGray color
            // Strip leading whitespace from each span and add consistent 3-space indent
            let mut styled_spans: Vec<Span<'static>> = Vec::new();
            styled_spans.push(Span::styled("  ", Style::default())); // 3-space indent
            for span in content_line.spans.into_iter() {
                let trimmed = span.content.trim_start().to_string();
                styled_spans.push(Span::styled(trimmed, Style::default().fg(Color::DarkGray)));
            }
            lines.push(Line::from(styled_spans));
        }
    }

    let message = message.unwrap_or("No (tell Stakpak what to do differently)".to_string());
    let message = preprocess_terminal_output(&message);

    // Handle multi-line error messages
    for (i, line) in message.lines().enumerate() {
        let prefix = if i == 0 { "  L " } else { "    " };
        lines.push(Line::from(vec![Span::styled(
            format!("{}{}", prefix, line),
            Style::default()
                .fg(colors.message)
                .add_modifier(Modifier::BOLD),
        )]));
    }

    lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));

    let owned_lines: Vec<Line<'static>> = lines
        .into_iter()
        .map(|line| {
            let owned_spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|span| Span::styled(span.content.into_owned(), span.style))
                .collect();
            Line::from(owned_spans)
        })
        .collect();

    owned_lines
}

pub fn render_refreshed_terminal_bubble(
    title: &str,
    content: &[Line<'static>],
    colors: Option<BubbleColors>,
    terminal_width: usize,
) -> Vec<Line<'static>> {
    render_styled_header_and_borders(title, content.to_vec(), colors, terminal_width)
}

pub fn is_collapsed_tool_call(tool_call: &ToolCall) -> bool {
    let tool_name = crate::utils::strip_tool_name(&tool_call.function.name);
    if tool_name == "run_command_task" {
        return false;
    }
    true
}

pub fn render_collapsed_command_message(
    tool_call_result: &ToolCallResult,
    lines: Vec<Line<'static>>,
    terminal_width: usize,
) -> Vec<Line<'static>> {
    let result = tool_call_result.result.clone();
    let command_args = extract_truncated_command_arguments(&tool_call_result.call, None);
    let title = get_command_type_name(&tool_call_result.call);

    let message = format!("Read {} lines (ctrl+t to expand)", result.lines().count());
    let colors = LinesColors {
        dot: Color::LightGreen,
        title: term_color(Color::White),
        command: AdaptiveColors::text(),
        message: AdaptiveColors::text(),
    };

    // if lines are more than 3 lines get the last 3 lines
    let lines = if lines.len() > 3 {
        lines[lines.len() - 3..].to_vec()
    } else {
        lines
    };

    render_styled_lines_with_content(
        &command_args,
        &title,
        Some(lines),
        Some(message),
        Some(colors),
        Some(terminal_width),
    )
}

/// Renders a compact view file result block with borders
/// Format: View path/to/file.rs - 123 lines [grep: pattern] [glob: pattern]
pub fn render_view_file_block(
    file_path: &str,
    total_lines: usize,
    terminal_width: usize,
    grep: Option<&str>,
    glob: Option<&str>,
) -> Vec<Line<'static>> {
    let content_width = if terminal_width > 4 {
        terminal_width - 4
    } else {
        40
    };
    let inner_width = content_width;

    let border_color = Color::DarkGray;
    let icon = "";

    // Determine title based on operation type
    let title = if grep.is_some() && glob.is_some() {
        "Grep+Glob"
    } else if grep.is_some() {
        "Grep"
    } else if glob.is_some() {
        "Glob"
    } else {
        "View"
    };

    let lines_text = format!("- {} lines", total_lines);

    // Build optional grep/glob suffix
    let suffix = match (grep, glob) {
        (Some(g), Some(gl)) => format!(" [grep: {} | glob: {}]", g, gl),
        (Some(g), None) => format!(" [grep: {}]", g),
        (None, Some(g)) => format!(" [glob: {}]", g),
        _ => String::new(),
    };

    // Calculate display widths
    let icon_width = calculate_display_width(icon);
    let title_width = calculate_display_width(title);
    let path_width = calculate_display_width(file_path);
    let lines_text_width = calculate_display_width(&lines_text);
    let suffix_width = calculate_display_width(&suffix);

    // Total content: icon + " " + title + " " + path + " " + lines_text + suffix
    let total_content_width =
        icon_width + 1 + title_width + 1 + path_width + 1 + lines_text_width + suffix_width;

    // Check if we need to truncate the path
    let (display_path, display_path_width) = if total_content_width > inner_width {
        // Need to truncate path
        let available_for_path = inner_width.saturating_sub(
            icon_width + 1 + title_width + 1 + 1 + lines_text_width + suffix_width + 3,
        ); // 3 for "..."
        let truncated = truncate_path_to_width(file_path, available_for_path);
        let w = calculate_display_width(&truncated);
        (truncated, w)
    } else {
        (file_path.to_string(), path_width)
    };

    let actual_content_width =
        icon_width + 1 + title_width + 1 + display_path_width + 1 + lines_text_width + suffix_width;
    let padding = inner_width.saturating_sub(actual_content_width);

    let mut spans = vec![
        Span::styled("│", Style::default().fg(border_color)),
        Span::from(" "),
        Span::styled(icon.to_string(), Style::default().fg(Color::DarkGray)),
        Span::from(" "),
        Span::styled(
            title.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::from(" "),
        Span::styled(display_path, Style::default().fg(AdaptiveColors::text())),
        Span::from(" "),
        Span::styled(lines_text, Style::default().fg(Color::DarkGray)),
    ];

    if !suffix.is_empty() {
        spans.push(Span::styled(suffix, Style::default().fg(Color::Cyan)));
    }

    spans.push(Span::from(" ".repeat(padding)));
    spans.push(Span::styled(" │", Style::default().fg(border_color)));

    let content_line = Line::from(spans);

    let horizontal_line = "─".repeat(inner_width + 2);
    let top_border = Line::from(vec![Span::styled(
        format!("╭{}╮", horizontal_line),
        Style::default().fg(border_color),
    )]);
    let bottom_border = Line::from(vec![Span::styled(
        format!("╰{}╯", horizontal_line),
        Style::default().fg(border_color),
    )]);

    vec![top_border, content_line, bottom_border]
}

/// Renders a compact view file block without borders (for full screen popup)
/// Format: Stack View path/to/file.rs - 123 lines [grep: pattern] [glob: pattern]
pub fn render_view_file_block_no_border(
    file_path: &str,
    total_lines: usize,
    terminal_width: usize,
    grep: Option<&str>,
    glob: Option<&str>,
) -> Vec<Line<'static>> {
    let content_width = if terminal_width > 2 {
        terminal_width - 2
    } else {
        40
    };

    let icon = "";

    // Determine title based on operation type
    let title = if grep.is_some() && glob.is_some() {
        "Grep+Glob"
    } else if grep.is_some() {
        "Grep"
    } else if glob.is_some() {
        "Glob"
    } else {
        "View"
    };

    let lines_text = format!("- {} lines", total_lines);

    // Build optional grep/glob suffix
    let suffix = match (grep, glob) {
        (Some(g), Some(gl)) => format!(" [grep: {} | glob: {}]", g, gl),
        (Some(g), None) => format!(" [grep: {}]", g),
        (None, Some(g)) => format!(" [glob: {}]", g),
        _ => String::new(),
    };

    // Calculate display widths
    let icon_width = calculate_display_width(icon);
    let title_width = calculate_display_width(title);
    let path_width = calculate_display_width(file_path);
    let lines_text_width = calculate_display_width(&lines_text);
    let suffix_width = calculate_display_width(&suffix);

    // Total content: icon + " " + title + " " + path + " " + lines_text + suffix
    let total_content_width =
        icon_width + 1 + title_width + 1 + path_width + 1 + lines_text_width + suffix_width;

    // Check if we need to truncate the path
    let (display_path, _display_path_width) = if total_content_width > content_width {
        // Need to truncate path
        let available_for_path = content_width.saturating_sub(
            icon_width + 1 + title_width + 1 + 1 + lines_text_width + suffix_width + 3,
        ); // 3 for "..."
        let truncated = truncate_path_to_width(file_path, available_for_path);
        let w = calculate_display_width(&truncated);
        (truncated, w)
    } else {
        (file_path.to_string(), path_width)
    };

    let mut spans = vec![
        Span::styled(icon.to_string(), Style::default().fg(Color::DarkGray)),
        Span::from(" "),
        Span::styled(
            title.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::from(" "),
        Span::styled(display_path, Style::default().fg(AdaptiveColors::text())),
        Span::from(" "),
        Span::styled(lines_text, Style::default().fg(Color::DarkGray)),
    ];

    if !suffix.is_empty() {
        spans.push(Span::styled(suffix, Style::default().fg(Color::Cyan)));
    }

    let content_line = Line::from(spans);

    vec![content_line]
}

/// Truncate a file path to fit within a given display width
fn truncate_path_to_width(path: &str, max_width: usize) -> String {
    let path_width = calculate_display_width(path);
    if path_width <= max_width {
        return path.to_string();
    }

    // Try to show ".../" + filename
    if let Some(file_name) = path.rsplit('/').next() {
        let file_name_width = calculate_display_width(file_name);
        if file_name_width + 4 <= max_width {
            // ".../" + filename fits
            return format!(".../{}", file_name);
        }
    }

    // Fall back to truncating from the start
    let mut result = String::new();
    let mut current_width = 3; // For "..."

    for ch in path.chars().rev() {
        let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        if current_width + char_width > max_width {
            break;
        }
        result.insert(0, ch);
        current_width += char_width;
    }

    format!("...{}", result)
}

/// State for the unified run command block
#[derive(Clone, Debug, PartialEq)]
pub enum RunCommandState {
    /// Initial state - waiting for user approval (Reset dot)
    Pending,
    /// Running state - command is executing (Yellow dot, "Running...")
    Running,
    /// Running with stall warning - command may be waiting for input (Yellow dot, warning message)
    RunningWithStallWarning(String),
    /// Completed successfully (Green dot)
    Completed,
    /// Failed/Error state (LightRed dot)
    Error,
    /// Cancelled by user (LightRed dot)
    Cancelled,
    /// Rejected by user (LightRed dot)
    Rejected,
    /// Skipped due to sequential execution failure (Yellow dot)
    Skipped,
}

/// Renders a unified run command block with consistent appearance across all states.
///
/// Layout:
/// ```text
/// ╭─● Run Command ──────────────────────────────────────╮
/// │ $ command --args here wrapped nicely                │
/// │   continuation of long command                      │
/// │                                                     │
/// │ Result:                                             │
/// │   output line 1                                     │
/// │   output line 2                                     │
/// ╰─────────────────────────────────────────────────────╯
/// ```
///
/// - Pending: Reset dot, no result section
/// - Running: Yellow dot + " - Running...", streaming result
/// - Completed: Green dot, final result
/// - Error: Red dot, error message
pub fn render_run_command_block(
    command: &str,
    result: Option<&str>,
    state: RunCommandState,
    terminal_width: usize,
) -> Vec<Line<'static>> {
    let content_width = if terminal_width > 4 {
        terminal_width - 4
    } else {
        40
    };
    let inner_width = content_width;
    let horizontal_line = "─".repeat(inner_width + 2);

    // Border color: DarkGray for error/cancelled/rejected/skipped states, Gray otherwise
    let border_color = match state {
        RunCommandState::Error
        | RunCommandState::Cancelled
        | RunCommandState::Rejected
        | RunCommandState::Skipped => Color::DarkGray,
        _ => term_color(Color::Gray),
    };

    // Dot color and title suffix based on state
    // For error states, both dot and suffix text are LightRed
    // For running/skipped, both dot and suffix text are Yellow
    let (dot_color, title_suffix, suffix_color) = match &state {
        RunCommandState::Pending => (term_color(Color::Reset), "".to_string(), None),
        RunCommandState::Running => (
            Color::Yellow,
            " - Running...".to_string(),
            Some(Color::Yellow),
        ),
        RunCommandState::RunningWithStallWarning(msg) => {
            // Show the stall warning message in the title
            (Color::Yellow, format!(" - {}", msg), Some(Color::Yellow))
        }
        RunCommandState::Completed => (Color::LightGreen, "".to_string(), None),
        RunCommandState::Error => (
            Color::LightRed,
            " - Errored".to_string(),
            Some(Color::LightRed),
        ),
        RunCommandState::Cancelled => (
            Color::LightRed,
            " - Cancelled".to_string(),
            Some(Color::LightRed),
        ),
        RunCommandState::Rejected => (
            Color::LightRed,
            " - Rejected".to_string(),
            Some(Color::LightRed),
        ),
        RunCommandState::Skipped => (Color::Yellow, " - Skipped".to_string(), Some(Color::Yellow)),
    };

    // Title structure: "╭─" + "●" + " Run Command" + suffix + " " + dashes + "╮"
    let base_title = "Run Command";
    let title_text = format!("{}{}", base_title, title_suffix);
    // Title border parts: "╭─" (2) + "●" (1) + " title " (title_text.len + 2) + dashes + "╮" (1)
    // Total should equal inner_width + 4
    // So: 2 + 1 + (title_text.len + 2) + dashes + 1 = inner_width + 4
    // dashes = inner_width + 4 - 2 - 1 - title_text.len - 2 - 1 = inner_width - title_text.len - 2
    let title_display_len = calculate_display_width(&title_text);
    let remaining_dashes = inner_width.saturating_sub(title_display_len + 2);

    // Build title spans - suffix gets special color for error states
    let title_border = if let Some(color) = suffix_color {
        // Split rendering: "Run Command" in white, suffix in special color
        let suffix_style = Style::default().fg(color).add_modifier(Modifier::BOLD);
        Line::from(vec![
            Span::styled("╭─", Style::default().fg(border_color)),
            Span::styled(
                "●",
                Style::default().fg(dot_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", base_title),
                Style::default()
                    .fg(term_color(Color::White))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{} ", title_suffix.trim_start()), suffix_style),
            Span::styled(
                format!("{}╮", "─".repeat(remaining_dashes)),
                Style::default().fg(border_color),
            ),
        ])
    } else {
        // Single color rendering for all text
        Line::from(vec![
            Span::styled("╭─", Style::default().fg(border_color)),
            Span::styled(
                "●",
                Style::default().fg(dot_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", title_text),
                Style::default()
                    .fg(term_color(Color::White))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}╮", "─".repeat(remaining_dashes)),
                Style::default().fg(border_color),
            ),
        ])
    };

    let bottom_border = Line::from(vec![Span::styled(
        format!("╰{}╯", horizontal_line),
        Style::default().fg(border_color),
    )]);

    let mut formatted_lines = Vec::new();
    formatted_lines.push(title_border);

    // Render command with $ prefix
    // Line structure: "│" (1) + " " (1) + content + padding + " " (1) + "│" (1) = inner_width + 4
    // So: content + padding = inner_width
    let command_with_prefix = format!("$ {}", command);

    // Available width for content = inner_width (we have 1 space padding on each side)
    let max_content_width = inner_width;
    let wrapped_lines = wrap_text_by_word(&command_with_prefix, max_content_width);

    for (i, wrapped_line) in wrapped_lines.iter().enumerate() {
        let line_display_width = calculate_display_width(wrapped_line);
        // padding = max_content_width - content_width
        let padding_needed = max_content_width.saturating_sub(line_display_width);

        // First line has "$ " in magenta, continuation lines are plain
        if i == 0 && wrapped_line.starts_with("$ ") {
            let cmd_part = &wrapped_line[2..]; // Skip "$ "
            let line_spans = vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(" "),
                Span::styled(
                    "$ ".to_string(),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    cmd_part.to_string(),
                    Style::default().fg(AdaptiveColors::text()),
                ),
                Span::from(" ".repeat(padding_needed)),
                Span::styled(" │", Style::default().fg(border_color)),
            ];
            formatted_lines.push(Line::from(line_spans));
        } else {
            let line_spans = vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(" "),
                Span::styled(
                    wrapped_line.clone(),
                    Style::default().fg(AdaptiveColors::text()),
                ),
                Span::from(" ".repeat(padding_needed)),
                Span::styled(" │", Style::default().fg(border_color)),
            ];
            formatted_lines.push(Line::from(line_spans));
        }
    }

    // Add result section if we have non-empty result content
    if let Some(result_content) = result.filter(|s| !s.is_empty()) {
        // Check if this is an error state (no "Result:" label, colored message)
        let is_error_state = matches!(
            state,
            RunCommandState::Error
                | RunCommandState::Cancelled
                | RunCommandState::Rejected
                | RunCommandState::Skipped
        );

        // Replace raw status strings with friendly messages
        let result_content = if result_content.contains("TOOL_CALL_REJECTED") {
            "Command was rejected"
        } else if result_content.contains("TOOL_CALL_CANCELLED") {
            "Command was cancelled"
        } else {
            result_content
        };

        // Message color for error states
        let error_message_color = match state {
            RunCommandState::Skipped => Color::Yellow,
            _ => Color::LightRed,
        };

        // Empty line separator
        // Line structure: "│" (1) + spaces (inner_width + 2) + "│" (1) = inner_width + 4
        formatted_lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::from(" ".repeat(inner_width + 2)),
            Span::styled("│", Style::default().fg(border_color)),
        ]));

        // Only show "Result:" label for non-error states
        if !is_error_state {
            // Result: label
            // Line structure: "│" (1) + " " (1) + label + padding + " " (1) + "│" (1)
            // So: content + padding = inner_width
            let result_label = "Result:";
            let label_padding = inner_width.saturating_sub(result_label.len());
            formatted_lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(" "),
                Span::styled(
                    result_label.to_string(),
                    Style::default()
                        .fg(term_color(Color::White))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::from(" ".repeat(label_padding)),
                Span::styled(" │", Style::default().fg(border_color)),
            ]));
        }

        // Strip ANSI codes, preprocess, and word-wrap the result content
        let cleaned_result = strip_ansi_codes(result_content).to_string();
        let preprocessed = preprocess_terminal_output(&cleaned_result);

        // Determine text color based on state
        let text_color = if is_error_state {
            error_message_color
        } else {
            AdaptiveColors::text()
        };

        // Process each line from the preprocessed result
        for source_line in preprocessed.lines() {
            if source_line.is_empty() {
                // Empty line
                formatted_lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(border_color)),
                    Span::from(" ".repeat(inner_width + 2)),
                    Span::styled("│", Style::default().fg(border_color)),
                ]));
            } else {
                // Word-wrap this line
                let wrapped = wrap_text_by_word(source_line, inner_width);
                for line_text in wrapped {
                    let line_width = calculate_display_width(&line_text);
                    let padding = inner_width.saturating_sub(line_width);
                    formatted_lines.push(Line::from(vec![
                        Span::styled("│", Style::default().fg(border_color)),
                        Span::from(" "),
                        Span::styled(line_text, Style::default().fg(text_color)),
                        Span::from(" ".repeat(padding)),
                        Span::styled(" │", Style::default().fg(border_color)),
                    ]));
                }
            }
        }
    }

    formatted_lines.push(bottom_border);

    // Convert to owned lines
    let owned_lines: Vec<Line<'static>> = formatted_lines
        .into_iter()
        .map(|line| {
            let owned_spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|span| Span::styled(span.content.into_owned(), span.style))
                .collect();
            Line::from(owned_spans)
        })
        .collect();

    owned_lines
}

/// Render an ask_user tool block inline, similar to render_run_command_block.
/// Shows a bordered block with tab bar, question content or review, and help text.
pub fn render_ask_user_block(
    questions: &[stakpak_shared::models::integrations::openai::AskUserQuestion],
    answers: &std::collections::HashMap<
        String,
        stakpak_shared::models::integrations::openai::AskUserAnswer,
    >,
    current_tab: usize,
    selected_option: usize,
    custom_input: &str,
    terminal_width: usize,
    focused: bool,
) -> Vec<Line<'static>> {
    let content_width = if terminal_width > 4 {
        terminal_width - 4
    } else {
        40
    };
    let inner_width = content_width;
    let horizontal_line = "─".repeat(inner_width + 2);
    let border_color = if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let dot_color = if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    // Title with focus indicator
    let base_title = if focused {
        "Ask User (Tab to scroll)"
    } else {
        "Ask User (Tab to focus)"
    };
    let title_display_len = calculate_display_width(base_title);
    let remaining_dashes = inner_width.saturating_sub(title_display_len + 2);

    let title_border = Line::from(vec![
        Span::styled("╭─", Style::default().fg(border_color)),
        Span::styled(
            "●",
            Style::default().fg(dot_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", base_title),
            Style::default()
                .fg(term_color(Color::White))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{}╮", "─".repeat(remaining_dashes)),
            Style::default().fg(border_color),
        ),
    ]);

    let bottom_border = Line::from(vec![Span::styled(
        format!("╰{}╯", horizontal_line),
        Style::default().fg(border_color),
    )]);

    let mut formatted_lines = Vec::new();
    formatted_lines.push(title_border);

    let max_content_width = inner_width;
    let all_required_answered = questions
        .iter()
        .filter(|q| q.required)
        .all(|q| answers.contains_key(&q.label));
    let is_submit_tab = current_tab >= questions.len();

    // --- Tab bar ---
    {
        let mut tab_spans = Vec::new();
        tab_spans.push(Span::styled(" ← ", Style::default().fg(Color::DarkGray)));

        for (i, q) in questions.iter().enumerate() {
            let is_current = i == current_tab;
            let is_answered = answers.contains_key(&q.label);
            let checkbox = if is_answered { "✓ " } else { "□ " };
            let style = if is_current {
                Style::default().bg(Color::Cyan).fg(Color::Black)
            } else if is_answered {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let label = if q.label.chars().count() > 15 {
                format!("{}...", q.label.chars().take(12).collect::<String>())
            } else {
                q.label.clone()
            };
            tab_spans.push(Span::styled(format!("{}{}", checkbox, label), style));
            tab_spans.push(Span::raw("   "));
        }

        let submit_style = if is_submit_tab {
            Style::default().bg(Color::Cyan).fg(Color::Black)
        } else if all_required_answered {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        tab_spans.push(Span::styled("Review", submit_style));
        tab_spans.push(Span::styled(" →", Style::default().fg(Color::DarkGray)));

        let tab_text: String = tab_spans.iter().map(|s| s.content.as_ref()).collect();
        let tab_display_width = calculate_display_width(&tab_text);
        let tab_padding = max_content_width.saturating_sub(tab_display_width);

        let mut line_spans = vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::from(" "),
        ];
        line_spans.extend(tab_spans);
        line_spans.push(Span::from(" ".repeat(tab_padding)));
        line_spans.push(Span::styled(" │", Style::default().fg(border_color)));
        formatted_lines.push(Line::from(line_spans));
    }

    // --- Separator ---
    formatted_lines.push(Line::from(vec![
        Span::styled("├", Style::default().fg(border_color)),
        Span::styled(
            "─".repeat(inner_width + 2),
            Style::default().fg(border_color),
        ),
        Span::styled("┤", Style::default().fg(border_color)),
    ]));

    if is_submit_tab {
        // --- Review / Submit content ---
        // Empty line
        formatted_lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::from(" ".repeat(inner_width + 2)),
            Span::styled("│", Style::default().fg(border_color)),
        ]));

        for q in questions {
            let required_marker = if q.required { " *" } else { "" };
            let label_text = format!("{}{}", q.label, required_marker);
            let label_width = calculate_display_width(&label_text);
            let label_padding = max_content_width.saturating_sub(label_width);
            formatted_lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(" "),
                Span::styled(
                    q.label.clone(),
                    Style::default()
                        .fg(term_color(Color::White))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(required_marker, Style::default().fg(Color::Red)),
                Span::from(" ".repeat(label_padding)),
                Span::styled(" │", Style::default().fg(border_color)),
            ]));

            if let Some(answer) = answers.get(&q.label) {
                let display = if answer.is_custom {
                    answer.answer.clone()
                } else {
                    q.options
                        .iter()
                        .find(|o| o.value == answer.answer)
                        .map(|o| o.label.clone())
                        .unwrap_or_else(|| answer.answer.clone())
                };
                let max_display = max_content_width.saturating_sub(4);
                let display = if display.chars().count() > max_display {
                    format!(
                        "{}…",
                        display
                            .chars()
                            .take(max_display.saturating_sub(1))
                            .collect::<String>()
                    )
                } else {
                    display
                };
                let answer_text = format!("    {}", display);
                let answer_width = calculate_display_width(&answer_text);
                let answer_padding = max_content_width.saturating_sub(answer_width);
                formatted_lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(border_color)),
                    Span::from(" "),
                    Span::raw("    "),
                    Span::styled(display, Style::default().fg(Color::Cyan)),
                    Span::from(" ".repeat(answer_padding)),
                    Span::styled(" │", Style::default().fg(border_color)),
                ]));
            } else if q.required {
                let text = "  □ not answered";
                let text_width = calculate_display_width(text);
                let text_padding = max_content_width.saturating_sub(text_width);
                formatted_lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(border_color)),
                    Span::from(" "),
                    Span::styled("  □ ", Style::default().fg(Color::Yellow)),
                    Span::styled("not answered", Style::default().fg(Color::Yellow)),
                    Span::from(" ".repeat(text_padding)),
                    Span::styled(" │", Style::default().fg(border_color)),
                ]));
            } else {
                let text = "  — skipped";
                let text_width = calculate_display_width(text);
                let text_padding = max_content_width.saturating_sub(text_width);
                formatted_lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(border_color)),
                    Span::from(" "),
                    Span::styled("  — ", Style::default().fg(Color::DarkGray)),
                    Span::styled("skipped", Style::default().fg(Color::DarkGray)),
                    Span::from(" ".repeat(text_padding)),
                    Span::styled(" │", Style::default().fg(border_color)),
                ]));
            }

            // Spacing between questions
            formatted_lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(" ".repeat(inner_width + 2)),
                Span::styled("│", Style::default().fg(border_color)),
            ]));
        }

        if !all_required_answered {
            let warn = "Answer all required (*) questions to submit";
            let warn_width = calculate_display_width(warn);
            let warn_padding = max_content_width.saturating_sub(warn_width);
            formatted_lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(" "),
                Span::styled(warn, Style::default().fg(Color::Yellow)),
                Span::from(" ".repeat(warn_padding)),
                Span::styled(" │", Style::default().fg(border_color)),
            ]));
        }
    } else if let Some(q) = questions.get(current_tab) {
        // --- Question content ---
        let previous_answer = answers.get(&q.label);

        // Empty line
        formatted_lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::from(" ".repeat(inner_width + 2)),
            Span::styled("│", Style::default().fg(border_color)),
        ]));

        // Question text (wrapped)
        let text_width = max_content_width.saturating_sub(2);
        let wrapped_question = wrap_text_by_word(&q.question, text_width);
        for line in &wrapped_question {
            let line_width = calculate_display_width(line);
            let padding = max_content_width.saturating_sub(line_width);
            formatted_lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(" "),
                Span::styled(
                    line.clone(),
                    Style::default()
                        .fg(term_color(Color::White))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::from(" ".repeat(padding)),
                Span::styled(" │", Style::default().fg(border_color)),
            ]));
        }

        // Empty line
        formatted_lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::from(" ".repeat(inner_width + 2)),
            Span::styled("│", Style::default().fg(border_color)),
        ]));

        // Options
        for (i, opt) in q.options.iter().enumerate() {
            let is_selected = i == selected_option;
            let is_answered = previous_answer
                .map(|a| !a.is_custom && a.answer == opt.value)
                .unwrap_or(false);

            let bracket = if is_answered {
                "[✓]".to_string()
            } else if is_selected {
                "[*]".to_string()
            } else {
                format!("[{}]", i + 1)
            };

            let bracket_style = if is_answered {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let label_style = if is_answered {
                Style::default().fg(Color::Cyan)
            } else if is_selected {
                Style::default()
                    .fg(term_color(Color::White))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let opt_text = format!("{} {}", bracket, opt.label);
            let opt_width = calculate_display_width(&opt_text);
            let opt_padding = max_content_width.saturating_sub(opt_width);
            formatted_lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(" "),
                Span::styled(bracket.clone(), bracket_style),
                Span::raw(" "),
                Span::styled(opt.label.clone(), label_style),
                Span::from(" ".repeat(opt_padding)),
                Span::styled(" │", Style::default().fg(border_color)),
            ]));

            if let Some(desc) = &opt.description {
                let desc_style = if is_selected || is_answered {
                    Style::default().fg(Color::Gray)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let desc_text = format!("       {}", desc);
                let desc_width = calculate_display_width(&desc_text);
                let desc_padding = max_content_width.saturating_sub(desc_width);
                formatted_lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(border_color)),
                    Span::from(" "),
                    Span::styled(desc_text, desc_style),
                    Span::from(" ".repeat(desc_padding)),
                    Span::styled(" │", Style::default().fg(border_color)),
                ]));
            }
        }

        // Custom input option
        if q.allow_custom {
            let custom_idx = q.options.len();
            let is_selected = selected_option == custom_idx;
            let is_custom_answered = previous_answer.map(|a| a.is_custom).unwrap_or(false);

            let (bracket, bracket_style) = if is_custom_answered {
                (
                    "[✓]".to_string(),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
            } else if is_selected {
                (
                    "[*]".to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                (
                    format!("[{}]", custom_idx + 1),
                    Style::default().fg(Color::DarkGray),
                )
            };

            if is_selected {
                if custom_input.is_empty() {
                    let text = format!("{} │Type your answer...", bracket);
                    let text_width = calculate_display_width(&text);
                    let text_padding = max_content_width.saturating_sub(text_width);
                    formatted_lines.push(Line::from(vec![
                        Span::styled("│", Style::default().fg(border_color)),
                        Span::from(" "),
                        Span::styled(bracket, bracket_style),
                        Span::raw(" "),
                        Span::styled("│", Style::default().fg(Color::Cyan)),
                        Span::styled("Type your answer...", Style::default().fg(Color::DarkGray)),
                        Span::from(" ".repeat(text_padding)),
                        Span::styled(" │", Style::default().fg(border_color)),
                    ]));
                } else {
                    let text = format!("{} {}│", bracket, custom_input);
                    let text_width = calculate_display_width(&text);
                    let text_padding = max_content_width.saturating_sub(text_width);
                    formatted_lines.push(Line::from(vec![
                        Span::styled("│", Style::default().fg(border_color)),
                        Span::from(" "),
                        Span::styled(bracket, bracket_style),
                        Span::raw(" "),
                        Span::styled(
                            custom_input.to_string(),
                            Style::default()
                                .fg(term_color(Color::White))
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("│", Style::default().fg(Color::Cyan)),
                        Span::from(" ".repeat(text_padding)),
                        Span::styled(" │", Style::default().fg(border_color)),
                    ]));
                }
            } else if is_custom_answered && let Some(answer) = previous_answer {
                let text = format!("{} {}", bracket, answer.answer);
                let text_width = calculate_display_width(&text);
                let text_padding = max_content_width.saturating_sub(text_width);
                formatted_lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(border_color)),
                    Span::from(" "),
                    Span::styled(bracket, bracket_style),
                    Span::raw(" "),
                    Span::styled(answer.answer.clone(), Style::default().fg(Color::Cyan)),
                    Span::from(" ".repeat(text_padding)),
                    Span::styled(" │", Style::default().fg(border_color)),
                ]));
            } else {
                let text = format!("{} Other...", bracket);
                let text_width = calculate_display_width(&text);
                let text_padding = max_content_width.saturating_sub(text_width);
                formatted_lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(border_color)),
                    Span::from(" "),
                    Span::styled(bracket, bracket_style),
                    Span::raw(" "),
                    Span::styled("Other...", Style::default().fg(Color::DarkGray)),
                    Span::from(" ".repeat(text_padding)),
                    Span::styled(" │", Style::default().fg(border_color)),
                ]));
            }
        }

        // Empty line after options
        formatted_lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::from(" ".repeat(inner_width + 2)),
            Span::styled("│", Style::default().fg(border_color)),
        ]));
    }

    // --- Separator before help ---
    formatted_lines.push(Line::from(vec![
        Span::styled("├", Style::default().fg(border_color)),
        Span::styled(
            "─".repeat(inner_width + 2),
            Style::default().fg(border_color),
        ),
        Span::styled("┤", Style::default().fg(border_color)),
    ]));

    // --- Help text ---
    {
        let help_spans = if !focused {
            // Unfocused: just show how to focus
            vec![
                Span::styled("Tab", Style::default().fg(Color::DarkGray)),
                Span::styled(" focus", Style::default().fg(Color::Cyan)),
                Span::raw(" · "),
                Span::styled("Esc", Style::default().fg(Color::DarkGray)),
                Span::styled(" cancel", Style::default().fg(Color::Cyan)),
            ]
        } else if is_submit_tab && all_required_answered {
            vec![
                Span::styled("Enter", Style::default().fg(Color::DarkGray)),
                Span::styled(" submit", Style::default().fg(Color::Green)),
                Span::raw(" · "),
                Span::styled("←/→", Style::default().fg(Color::DarkGray)),
                Span::styled(" questions", Style::default().fg(Color::Cyan)),
                Span::raw(" · "),
                Span::styled("Tab", Style::default().fg(Color::DarkGray)),
                Span::styled(" scroll", Style::default().fg(Color::Cyan)),
                Span::raw(" · "),
                Span::styled("Esc", Style::default().fg(Color::DarkGray)),
                Span::styled(" unfocus", Style::default().fg(Color::Cyan)),
            ]
        } else if is_submit_tab {
            vec![
                Span::styled("←/→", Style::default().fg(Color::DarkGray)),
                Span::styled(" questions", Style::default().fg(Color::Cyan)),
                Span::raw(" · "),
                Span::styled("Tab", Style::default().fg(Color::DarkGray)),
                Span::styled(" scroll", Style::default().fg(Color::Cyan)),
                Span::raw(" · "),
                Span::styled("Esc", Style::default().fg(Color::DarkGray)),
                Span::styled(" unfocus", Style::default().fg(Color::Cyan)),
            ]
        } else {
            let is_custom_selected = questions
                .get(current_tab)
                .map(|q| q.allow_custom && selected_option == q.options.len())
                .unwrap_or(false);

            if is_custom_selected {
                vec![
                    Span::styled("Type", Style::default().fg(Color::DarkGray)),
                    Span::styled(" your answer", Style::default().fg(Color::Cyan)),
                    Span::raw(" · "),
                    Span::styled("Enter", Style::default().fg(Color::DarkGray)),
                    Span::styled(" confirm", Style::default().fg(Color::Cyan)),
                    Span::raw(" · "),
                    Span::styled("↑/↓", Style::default().fg(Color::DarkGray)),
                    Span::styled(" options", Style::default().fg(Color::Cyan)),
                    Span::raw(" · "),
                    Span::styled("Tab", Style::default().fg(Color::DarkGray)),
                    Span::styled(" scroll", Style::default().fg(Color::Cyan)),
                ]
            } else {
                vec![
                    Span::styled("Enter", Style::default().fg(Color::DarkGray)),
                    Span::styled(" select", Style::default().fg(Color::Cyan)),
                    Span::raw(" · "),
                    Span::styled("↑/↓", Style::default().fg(Color::DarkGray)),
                    Span::styled(" options", Style::default().fg(Color::Cyan)),
                    Span::raw(" · "),
                    Span::styled("←/→", Style::default().fg(Color::DarkGray)),
                    Span::styled(" questions", Style::default().fg(Color::Cyan)),
                    Span::raw(" · "),
                    Span::styled("Tab", Style::default().fg(Color::DarkGray)),
                    Span::styled(" scroll", Style::default().fg(Color::Cyan)),
                ]
            }
        };

        let help_text: String = help_spans.iter().map(|s| s.content.as_ref()).collect();
        let help_width = calculate_display_width(&help_text);
        let help_padding = max_content_width.saturating_sub(help_width);

        let mut line_spans = vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::from(" "),
        ];
        line_spans.extend(help_spans);
        line_spans.push(Span::from(" ".repeat(help_padding)));
        line_spans.push(Span::styled(" │", Style::default().fg(border_color)));
        formatted_lines.push(Line::from(line_spans));
    }

    formatted_lines.push(bottom_border);

    // Convert to owned lines
    formatted_lines
        .into_iter()
        .map(|line| {
            let owned_spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|span| Span::styled(span.content.into_owned(), span.style))
                .collect();
            Line::from(owned_spans)
        })
        .collect()
}

/// Render a task wait block showing progress of background tasks
/// Displays a bordered box with task statuses and overall progress
pub fn render_task_wait_block(
    task_updates: &[stakpak_shared::models::integrations::openai::TaskUpdate],
    progress: f64,
    target_task_ids: &[String],
    terminal_width: usize,
) -> Vec<Line<'static>> {
    let content_width = if terminal_width > 4 {
        terminal_width - 4
    } else {
        40
    };
    let inner_width = content_width;
    let horizontal_line = "─".repeat(inner_width + 2);

    // Border color - gray for all states (could differentiate later)
    let border_color = term_color(Color::Gray);

    // Check if all tasks are completed
    let all_completed = progress >= 100.0;

    // Dot color and title suffix based on progress
    let (dot_color, title_suffix, suffix_color) = if all_completed {
        (Color::LightGreen, "".to_string(), None)
    } else {
        let completed_count = task_updates
            .iter()
            .filter(|t| {
                t.is_target
                    && (t.status == "Completed"
                        || t.status == "Failed"
                        || t.status == "Cancelled"
                        || t.status == "TimedOut")
            })
            .count();
        let total_count = target_task_ids.len();
        (
            Color::Yellow,
            format!(" - Waiting ({}/{})", completed_count, total_count),
            Some(Color::Yellow),
        )
    };

    // Build title
    let base_title = "Wait for Tasks";
    let title_text = format!("{}{}", base_title, title_suffix);
    let title_display_len = calculate_display_width(&title_text);
    let remaining_dashes = inner_width.saturating_sub(title_display_len + 2);

    // Build title spans
    let title_border = if let Some(color) = suffix_color {
        Line::from(vec![
            Span::styled("╭─", Style::default().fg(border_color)),
            Span::styled(
                "●",
                Style::default().fg(dot_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", base_title),
                Style::default()
                    .fg(term_color(Color::White))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} ", title_suffix.trim_start()),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}╮", "─".repeat(remaining_dashes)),
                Style::default().fg(border_color),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("╭─", Style::default().fg(border_color)),
            Span::styled(
                "●",
                Style::default().fg(dot_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", title_text),
                Style::default()
                    .fg(term_color(Color::White))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}╮", "─".repeat(remaining_dashes)),
                Style::default().fg(border_color),
            ),
        ])
    };

    let bottom_border = Line::from(vec![Span::styled(
        format!("╰{}╯", horizontal_line),
        Style::default().fg(border_color),
    )]);

    let mut formatted_lines = Vec::new();
    formatted_lines.push(title_border);

    // Filter to show only target tasks, sorted by status (running first, then completed)
    let mut target_tasks: Vec<_> = task_updates.iter().filter(|t| t.is_target).collect();

    // Sort: Running tasks first, then by task_id
    target_tasks.sort_by(|a, b| {
        let a_running = a.status == "Running" || a.status == "Pending";
        let b_running = b.status == "Running" || b.status == "Pending";
        match (a_running, b_running) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.task_id.cmp(&b.task_id),
        }
    });

    // Render each task
    for task in &target_tasks {
        let (status_icon, status_color) = match task.status.as_str() {
            "Running" => ("◐", Color::Yellow),
            "Pending" => ("○", Color::DarkGray),
            "Paused" => ("⏸", Color::Magenta),
            "Completed" => ("✓", Color::LightGreen),
            "Failed" => ("✗", Color::LightRed),
            "Cancelled" => ("⊘", Color::LightRed),
            "TimedOut" => ("⏱", Color::LightRed),
            _ => ("?", Color::DarkGray),
        };

        // Format duration
        let duration_str = task
            .duration_secs
            .map(|d| {
                if d < 60.0 {
                    format!("{:.1}s", d)
                } else {
                    format!("{:.0}m{:.0}s", d / 60.0, d % 60.0)
                }
            })
            .unwrap_or_else(|| "...".to_string());

        // Truncate task_id for display (show first 8 chars)
        let task_id_display = if task.task_id.chars().count() > 8 {
            let truncated: String = task.task_id.chars().take(8).collect();
            format!("{}…", truncated)
        } else {
            task.task_id.clone()
        };

        // Get description or fall back to truncated task_id
        let raw_description = task
            .description
            .as_ref()
            .cloned()
            .unwrap_or_else(|| task_id_display.clone());

        // Detect and strip [sandboxed] tag for separate rendering
        let is_sandboxed = raw_description.contains("[sandboxed]");
        let clean_description = raw_description
            .replace(" [sandboxed]", "")
            .replace("[sandboxed]", "");

        let display_name = if clean_description.chars().count() > 30 {
            let truncated: String = clean_description.chars().take(30).collect();
            format!("{}…", truncated)
        } else {
            clean_description
        };

        let sandboxed_tag = if is_sandboxed { " [sandboxed]" } else { "" };

        // Build the task line: "│ ● description [sandboxed] [duration] status │"
        let task_content = format!(
            "{}{} {} [{}]",
            display_name, sandboxed_tag, task.status, duration_str
        );
        let content_display_width = calculate_display_width(&task_content) + 2; // +2 for icon and space
        let padding_needed = inner_width.saturating_sub(content_display_width);

        let mut line_spans = vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::from(" "),
            Span::styled(
                status_icon.to_string(),
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::from(" "),
            Span::styled(display_name, Style::default().fg(AdaptiveColors::text())),
        ];
        if is_sandboxed {
            line_spans.push(Span::styled(
                " [sandboxed]",
                Style::default().fg(Color::Green),
            ));
        }
        line_spans.extend([
            Span::styled(
                format!(" [{}]", duration_str),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!(" {}", task.status),
                Style::default().fg(status_color),
            ),
            Span::from(" ".repeat(padding_needed)),
            Span::styled(" │", Style::default().fg(border_color)),
        ]);
        formatted_lines.push(Line::from(line_spans));

        // If task is paused, show pause info (agent message and pending tool calls)
        if let Some(pause_info) = &task.pause_info {
            // Show agent message if present
            if let Some(agent_msg) = &pause_info.agent_message {
                let trimmed_msg = agent_msg.trim();
                if !trimmed_msg.is_empty() {
                    // Truncate long messages (char-aware to avoid slicing mid-character)
                    let max_msg_chars = inner_width.saturating_sub(7);
                    let display_msg = if calculate_display_width(trimmed_msg)
                        > inner_width.saturating_sub(6)
                    {
                        let truncated: String = trimmed_msg.chars().take(max_msg_chars).collect();
                        format!("{}…", truncated)
                    } else {
                        trimmed_msg.to_string()
                    };
                    let msg_padding =
                        inner_width.saturating_sub(calculate_display_width(&display_msg) + 4);
                    formatted_lines.push(Line::from(vec![
                        Span::styled("│", Style::default().fg(border_color)),
                        Span::from("     "),
                        Span::styled(
                            display_msg,
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::ITALIC),
                        ),
                        Span::from(" ".repeat(msg_padding)),
                        Span::styled("│", Style::default().fg(border_color)),
                    ]));
                }
            }

            // Show pending tool calls
            if let Some(tool_calls) = &pause_info.pending_tool_calls {
                for tc in tool_calls {
                    // Format: "  → tool_name(args_preview)"
                    let args_preview = {
                        let args_str = tc.arguments.to_string();
                        if args_str == "null" || args_str == "{}" {
                            String::new()
                        } else if args_str.len() > 40 {
                            // Find a valid UTF-8 boundary near 40 chars
                            let truncate_at = args_str
                                .char_indices()
                                .take_while(|(i, _)| *i < 40)
                                .last()
                                .map(|(i, c)| i + c.len_utf8())
                                .unwrap_or(0);
                            format!("{}…", &args_str[..truncate_at])
                        } else {
                            args_str
                        }
                    };

                    let tool_display = if args_preview.is_empty() {
                        format!("→ {}", tc.name)
                    } else {
                        format!("→ {}({})", tc.name, args_preview)
                    };

                    let tool_display_width = calculate_display_width(&tool_display) + 4;
                    let tool_padding = inner_width.saturating_sub(tool_display_width);

                    formatted_lines.push(Line::from(vec![
                        Span::styled("│", Style::default().fg(border_color)),
                        Span::from("     "),
                        Span::styled("→ ", Style::default().fg(Color::Magenta)),
                        Span::styled(
                            tc.name.clone(),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            if args_preview.is_empty() {
                                String::new()
                            } else {
                                format!("({})", args_preview)
                            },
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::from(" ".repeat(tool_padding)),
                        Span::styled("│", Style::default().fg(border_color)),
                    ]));
                }
            }
        }
    }

    // If no target tasks, show a message
    if target_tasks.is_empty() {
        let msg = "No tasks to display";
        let padding = inner_width.saturating_sub(msg.len());
        formatted_lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::from(" "),
            Span::styled(msg.to_string(), Style::default().fg(Color::DarkGray)),
            Span::from(" ".repeat(padding)),
            Span::styled(" │", Style::default().fg(border_color)),
        ]));
    }

    formatted_lines.push(bottom_border);

    // Add spacing marker
    formatted_lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));

    formatted_lines
}

/// Render a pending block for resume_subagent_task showing what the subagent wants to do
pub fn render_subagent_resume_pending_block<'a>(
    tool_call: &ToolCall,
    is_auto_approved: bool,
    pause_info: Option<&stakpak_shared::models::integrations::openai::TaskPauseInfo>,
    width: usize,
) -> Vec<Line<'a>> {
    let mut formatted_lines: Vec<Line<'a>> = Vec::new();

    let border_color = if is_auto_approved {
        Color::Green
    } else {
        Color::Cyan
    };
    let inner_width = width.saturating_sub(4);

    // Parse arguments to determine resume type
    let args = serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments).ok();

    // Extract task_id from arguments
    let task_id = args
        .as_ref()
        .and_then(|a| a.get("task_id").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_else(|| "unknown".to_string());

    // Check if this is an input-based resume (for completed agents) or tool approval resume
    let input_text = args
        .as_ref()
        .and_then(|a| a.get("input").and_then(|v| v.as_str()).map(String::from));

    let has_approve_all = args
        .as_ref()
        .and_then(|a| a.get("approve_all").and_then(|v| v.as_bool()))
        .unwrap_or(false);

    // Title
    let title = format!(" Resume Subagent [{}] ", task_id);
    let title_len = calculate_display_width(&title);
    let dashes_after = inner_width.saturating_sub(title_len + 1);

    // Top border with title
    let top_border = Line::from(vec![
        Span::styled("╭─", Style::default().fg(border_color)),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{}╮", "─".repeat(dashes_after)),
            Style::default().fg(border_color),
        ),
    ]);
    formatted_lines.push(top_border);

    // Handle input-based resume (completed agent, continuing with user input)
    if let Some(input) = input_text {
        let header = "Continue with input:";
        let header_padding = inner_width.saturating_sub(calculate_display_width(header));
        formatted_lines.push(Line::from(vec![
            Span::styled("│ ", Style::default().fg(border_color)),
            Span::styled(
                header.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::from(" ".repeat(header_padding)),
            Span::styled(" │", Style::default().fg(border_color)),
        ]));

        // Empty line
        formatted_lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::from(" ".repeat(inner_width + 2)),
            Span::styled("│", Style::default().fg(border_color)),
        ]));

        // Show the input text, wrapped if necessary
        let input_lines = wrap_text_to_lines(&input, inner_width.saturating_sub(4));
        for line in input_lines {
            let line_width = calculate_display_width(&line);
            let line_padding = inner_width.saturating_sub(line_width + 2);
            formatted_lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(border_color)),
                Span::styled("  ", Style::default()),
                Span::styled(line, Style::default().fg(Color::White)),
                Span::from(" ".repeat(line_padding)),
                Span::styled(" │", Style::default().fg(border_color)),
            ]));
        }

        // Empty line
        formatted_lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::from(" ".repeat(inner_width + 2)),
            Span::styled("│", Style::default().fg(border_color)),
        ]));
    } else if has_approve_all || pause_info.is_some() {
        // Handle tool approval resume - show what the subagent wants to execute
        if let Some(pi) = pause_info {
            // Header line
            let header = "Subagent wants to execute:";
            let header_padding = inner_width.saturating_sub(calculate_display_width(header));
            formatted_lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(border_color)),
                Span::styled(
                    header.to_string(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::from(" ".repeat(header_padding)),
                Span::styled(" │", Style::default().fg(border_color)),
            ]));

            // Empty line
            formatted_lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(" ".repeat(inner_width + 2)),
                Span::styled("│", Style::default().fg(border_color)),
            ]));

            // Show pending tool calls
            if let Some(tool_calls) = &pi.pending_tool_calls {
                for tc in tool_calls {
                    // Tool name line
                    let tool_header = format!("  → {}", tc.name);
                    let tool_header_width = calculate_display_width(&tool_header);
                    let tool_header_padding = inner_width.saturating_sub(tool_header_width);

                    formatted_lines.push(Line::from(vec![
                        Span::styled("│ ", Style::default().fg(border_color)),
                        Span::styled("  → ", Style::default().fg(Color::Magenta)),
                        Span::styled(
                            tc.name.clone(),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::from(" ".repeat(tool_header_padding)),
                        Span::styled(" │", Style::default().fg(border_color)),
                    ]));

                    // Show arguments in a readable format
                    if !tc.arguments.is_null() {
                        let formatted_args =
                            format_tool_arguments_readable(&tc.arguments, inner_width - 6);
                        for arg_line in formatted_args {
                            let arg_display_width = calculate_display_width(&arg_line);
                            let arg_padding = inner_width.saturating_sub(arg_display_width + 4);

                            formatted_lines.push(Line::from(vec![
                                Span::styled("│ ", Style::default().fg(border_color)),
                                Span::from("    "),
                                Span::styled(arg_line, Style::default().fg(Color::DarkGray)),
                                Span::from(" ".repeat(arg_padding)),
                                Span::styled(" │", Style::default().fg(border_color)),
                            ]));
                        }
                    }

                    // Empty line between tool calls
                    formatted_lines.push(Line::from(vec![
                        Span::styled("│", Style::default().fg(border_color)),
                        Span::from(" ".repeat(inner_width + 2)),
                        Span::styled("│", Style::default().fg(border_color)),
                    ]));
                }
            }
        } else {
            // approve_all but no pause_info cached
            let msg = "Approve all pending tool calls";
            let msg_padding = inner_width.saturating_sub(calculate_display_width(msg));
            formatted_lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(border_color)),
                Span::styled(msg.to_string(), Style::default().fg(Color::Yellow)),
                Span::from(" ".repeat(msg_padding)),
                Span::styled(" │", Style::default().fg(border_color)),
            ]));

            // Empty line
            formatted_lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(border_color)),
                Span::from(" ".repeat(inner_width + 2)),
                Span::styled("│", Style::default().fg(border_color)),
            ]));
        }
    } else {
        // No pause info and no input - generic message
        let msg = "Resume subagent task";
        let msg_padding = inner_width.saturating_sub(calculate_display_width(msg));
        formatted_lines.push(Line::from(vec![
            Span::styled("│ ", Style::default().fg(border_color)),
            Span::styled(msg.to_string(), Style::default().fg(Color::DarkGray)),
            Span::from(" ".repeat(msg_padding)),
            Span::styled(" │", Style::default().fg(border_color)),
        ]));

        // Empty line
        formatted_lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::from(" ".repeat(inner_width + 2)),
            Span::styled("│", Style::default().fg(border_color)),
        ]));
    }

    // Bottom border
    let bottom_border = Line::from(vec![
        Span::styled("╰", Style::default().fg(border_color)),
        Span::styled(
            "─".repeat(inner_width + 2),
            Style::default().fg(border_color),
        ),
        Span::styled("╯", Style::default().fg(border_color)),
    ]);
    formatted_lines.push(bottom_border);

    // Add spacing marker
    formatted_lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));

    formatted_lines
}

/// Wrap text to fit within a given width, respecting word boundaries
fn wrap_text_to_lines(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        if current_line.is_empty() {
            if word.chars().count() > max_width {
                // Word is too long, truncate it
                let truncated: String = word.chars().take(max_width.saturating_sub(1)).collect();
                lines.push(format!("{}…", truncated));
            } else {
                current_line = word.to_string();
            }
        } else if current_line.chars().count() + 1 + word.chars().count() <= max_width {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            if word.chars().count() > max_width {
                let truncated: String = word.chars().take(max_width.saturating_sub(1)).collect();
                lines.push(format!("{}…", truncated));
                current_line = String::new();
            } else {
                current_line = word.to_string();
            }
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    // Limit to 5 lines max
    if lines.len() > 5 {
        lines.truncate(4);
        lines.push("...".to_string());
    }

    lines
}

/// Format tool arguments in a readable way for display
fn format_tool_arguments_readable(args: &serde_json::Value, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();

    if let Some(obj) = args.as_object() {
        for (key, value) in obj {
            let value_str = match value {
                serde_json::Value::String(s) => {
                    // For long strings, truncate and show preview
                    let max_value_len = max_width.saturating_sub(key.len() + 4);
                    if s.chars().count() > max_value_len {
                        let truncated: String =
                            s.chars().take(max_value_len.saturating_sub(3)).collect();
                        format!("\"{}…\"", truncated)
                    } else {
                        format!("\"{}\"", s)
                    }
                }
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Array(arr) => {
                    if arr.is_empty() {
                        "[]".to_string()
                    } else {
                        format!("[{} items]", arr.len())
                    }
                }
                serde_json::Value::Object(_) => "{...}".to_string(),
                serde_json::Value::Null => "null".to_string(),
            };

            let line = format!("{}: {}", key, value_str);
            // Truncate if still too long (respecting char boundaries)
            if line.chars().count() > max_width {
                let truncated: String = line.chars().take(max_width.saturating_sub(1)).collect();
                lines.push(format!("{}…", truncated));
            } else {
                lines.push(line);
            }
        }
    } else {
        // Not an object, just show the raw value truncated
        let s = args.to_string();
        if s.chars().count() > max_width {
            let truncated: String = s.chars().take(max_width.saturating_sub(1)).collect();
            lines.push(format!("{}…", truncated));
        } else {
            lines.push(s);
        }
    }

    lines
}

/// Render a preview block showing tool calls being streamed/generated by the LLM.
/// Compact layout: shows a summary line with total count + total tokens,
/// plus individual tool rows (capped at MAX_VISIBLE_TOOLS to keep the block short).
pub fn render_tool_call_stream_block(
    infos: &[ToolCallStreamInfo],
    width: usize,
) -> Vec<Line<'static>> {
    let border_color = Color::DarkGray;
    // inner_width = usable content width between the border+padding chars
    // Each line is: "│ " + content(inner_width) + " │" = inner_width + 4
    let inner_width = if width > 4 { width - 4 } else { 40 };
    let horizontal_line = "─".repeat(inner_width + 2);
    let mut lines = Vec::new();

    lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));

    // Title: "Generating N tool calls..."
    let title = if infos.len() == 1 {
        " Generating 1 tool call... ".to_string()
    } else {
        format!(" Generating {} tool calls... ", infos.len())
    };
    let title_display_width = UnicodeWidthStr::width(title.as_str());
    // Top border: "╭" + title + "─"×remaining + "╮" must equal inner_width + 4
    let remaining_dashes = (inner_width + 2).saturating_sub(title_display_width);
    lines.push(Line::from(vec![
        Span::styled("╭", Style::default().fg(border_color)),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{}╮", "─".repeat(remaining_dashes)),
            Style::default().fg(border_color),
        ),
    ]));

    // Each line structure: "│" + fill(fill_width) + " │"
    // Total = 1 + fill_width + 2 = width, so fill_width = width - 3
    let fill_width = width.saturating_sub(3);

    // Show individual tool rows, capped to keep the block compact
    const MAX_VISIBLE_TOOLS: usize = 5;
    let visible_count = infos.len().min(MAX_VISIBLE_TOOLS);

    for (i, info) in infos.iter().take(visible_count).enumerate() {
        let number = circled_number(i + 1);
        let base_name = if info.name.is_empty() {
            "streaming...".to_string()
        } else {
            format_tool_display_name(&info.name)
        };
        let tokens = if info.args_tokens > 0 {
            format_token_count(info.args_tokens)
        } else {
            "...".to_string()
        };

        // Append description if available, truncating to fit
        let number_width = UnicodeWidthStr::width(number.as_str());
        let base_name_width = UnicodeWidthStr::width(base_name.as_str());
        let tokens_width = UnicodeWidthStr::width(tokens.as_str());
        let fixed_used = 2 + number_width + 1 + base_name_width + tokens_width;

        let name = if let Some(desc) = &info.description {
            let sep = " - ";
            let sep_width = sep.len();
            // Need at least 4 chars for a meaningful truncated description ("X…")
            let available = fill_width.saturating_sub(fixed_used + sep_width + 1);
            if available >= 2 {
                let desc_display: String = desc.chars().take(available).collect();
                let truncated = if UnicodeWidthStr::width(desc_display.as_str())
                    < UnicodeWidthStr::width(desc.as_str())
                {
                    let trimmed: String = desc.chars().take(available.saturating_sub(1)).collect();
                    format!("{}…", trimmed)
                } else {
                    desc_display
                };
                format!("{}{}{}", base_name, sep, truncated)
            } else {
                base_name
            }
        } else {
            base_name
        };

        let name_width = UnicodeWidthStr::width(name.as_str());
        let used = 2 + number_width + 1 + name_width + tokens_width;
        let padding = fill_width.saturating_sub(used);

        lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::raw("  "),
            Span::styled(number, Style::default().fg(AdaptiveColors::orange())),
            Span::raw(" "),
            Span::styled(name, Style::default().fg(Color::White)),
            Span::raw(" ".repeat(padding)),
            Span::styled(tokens, Style::default().fg(Color::DarkGray)),
            Span::styled(" │", Style::default().fg(border_color)),
        ]));
    }

    // If there are more tools than we can show, add a summary row
    if infos.len() > MAX_VISIBLE_TOOLS {
        let hidden_count = infos.len() - MAX_VISIBLE_TOOLS;
        let hidden_tokens: usize = infos
            .iter()
            .skip(MAX_VISIBLE_TOOLS)
            .map(|i| i.args_tokens)
            .sum();
        let summary = format!(
            " +{} more{}",
            hidden_count,
            if hidden_tokens > 0 {
                format!(" ({})", format_token_count(hidden_tokens))
            } else {
                String::new()
            }
        );
        let summary_width = UnicodeWidthStr::width(summary.as_str());
        let padding = fill_width.saturating_sub(summary_width);

        lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::styled(summary, Style::default().fg(Color::DarkGray)),
            Span::raw(" ".repeat(padding)),
            Span::styled(" │", Style::default().fg(border_color)),
        ]));
    }

    // Total tokens line
    let total_tokens: usize = infos.iter().map(|i| i.args_tokens).sum();
    if total_tokens > 0 {
        let total_text = format!(" total: {}", format_token_count(total_tokens));
        let total_width = UnicodeWidthStr::width(total_text.as_str());
        let padding = fill_width.saturating_sub(total_width);

        lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(border_color)),
            Span::styled(total_text, Style::default().fg(Color::DarkGray)),
            Span::raw(" ".repeat(padding)),
            Span::styled(" │", Style::default().fg(border_color)),
        ]));
    }

    // Bottom border
    lines.push(Line::from(vec![Span::styled(
        format!("╰{}╯", horizontal_line),
        Style::default().fg(border_color),
    )]));

    lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));
    lines
}

fn circled_number(n: usize) -> String {
    format!("{}", n)
}

fn format_tool_display_name(name: &str) -> String {
    let stripped = crate::utils::strip_tool_name(name);
    stripped
        .replace('_', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

fn format_token_count(tokens: usize) -> String {
    if tokens >= 1000 {
        format!("{:.1}k tokens", tokens as f64 / 1000.0)
    } else {
        format!("{} tokens", tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_text_by_word_preserves_newlines() {
        // Multi-line command should preserve explicit line breaks
        let input = "echo \"line 1\" \\\n  && echo \"line 2\" \\\n  && echo \"line 3\"";
        let result = wrap_text_by_word(input, 80);

        assert_eq!(result.len(), 3);
        assert_eq!(result[0], "echo \"line 1\" \\");
        assert_eq!(result[1], "  && echo \"line 2\" \\");
        assert_eq!(result[2], "  && echo \"line 3\"");
    }

    #[test]
    fn test_wrap_text_by_word_empty_lines() {
        // Consecutive newlines should produce empty lines
        let input = "line 1\n\nline 3";
        let result = wrap_text_by_word(input, 80);

        assert_eq!(result.len(), 3);
        assert_eq!(result[0], "line 1");
        assert_eq!(result[1], "");
        assert_eq!(result[2], "line 3");
    }

    #[test]
    fn test_wrap_text_by_word_wraps_long_lines() {
        // Long lines should still wrap at width boundary
        let input = "this is a very long line that should wrap";
        let result = wrap_text_by_word(input, 20);

        assert!(result.len() > 1);
        for line in &result {
            assert!(line.len() <= 20);
        }
    }

    #[test]
    fn test_wrap_text_by_word_mixed_newlines_and_wrapping() {
        // Combine explicit newlines with width-based wrapping
        let input = "short\nthis is a longer line that needs wrapping\nend";
        let result = wrap_text_by_word(input, 20);

        // First line: "short"
        assert_eq!(result[0], "short");
        // Middle lines: wrapped version of the long line
        // Last line: "end"
        assert_eq!(result[result.len() - 1], "end");
        assert!(result.len() >= 3);
    }

    #[test]
    fn test_wrap_text_by_word_single_line_no_newlines() {
        let input = "simple command";
        let result = wrap_text_by_word(input, 80);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "simple command");
    }

    #[test]
    fn test_wrap_text_by_word_empty_input() {
        let result = wrap_text_by_word("", 80);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "");
    }

    #[test]
    fn test_tool_call_stream_block_border_alignment() {
        use stakpak_shared::models::integrations::openai::ToolCallStreamInfo;

        let infos = vec![
            ToolCallStreamInfo {
                name: "stakpak__create".to_string(),
                args_tokens: 3241,
                description: None,
            },
            ToolCallStreamInfo {
                name: "stakpak__run_command".to_string(),
                args_tokens: 412,
                description: None,
            },
            ToolCallStreamInfo {
                name: "".to_string(),
                args_tokens: 0,
                description: None,
            },
        ];

        let width = 80;
        let lines = render_tool_call_stream_block(&infos, width);

        // Check that all non-SPACING_MARKER lines have consistent display width
        for line in &lines {
            let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            if text == "SPACING_MARKER" {
                continue;
            }
            let display_width = UnicodeWidthStr::width(text.as_str());
            assert_eq!(
                display_width, width,
                "Line has wrong width {}: {:?}",
                display_width, text
            );
        }
    }

    #[test]
    fn test_tool_call_stream_block_overflow_summary() {
        use stakpak_shared::models::integrations::openai::ToolCallStreamInfo;

        let infos: Vec<ToolCallStreamInfo> = (0..8)
            .map(|i| ToolCallStreamInfo {
                name: format!("stakpak__tool_{}", i),
                args_tokens: 100 * (i + 1),
                description: None,
            })
            .collect();

        let width = 80;
        let lines = render_tool_call_stream_block(&infos, width);

        // Should have: SPACING + top border + 5 tool rows + 1 summary + 1 total + bottom border + SPACING = 11
        let content_lines: Vec<_> = lines
            .iter()
            .filter(|l| {
                let text: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
                text != "SPACING_MARKER"
            })
            .collect();
        // top + 5 visible + 1 "+3 more" + 1 total + bottom = 9
        assert_eq!(content_lines.len(), 9);

        // Verify all lines have correct width
        for line in &content_lines {
            let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            let display_width = UnicodeWidthStr::width(text.as_str());
            assert_eq!(
                display_width, width,
                "Line has wrong width {}: {:?}",
                display_width, text
            );
        }
    }
}
