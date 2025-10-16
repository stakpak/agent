use super::message::{extract_full_command_arguments, extract_truncated_command_arguments};
use crate::app::AppState;
use crate::services::detect_term::AdaptiveColors;
use crate::services::file_diff::render_file_diff_block;
use crate::services::markdown_renderer::render_markdown_to_lines;
use crate::services::message::{
    BubbleColors, Message, MessageContent, extract_command_purpose, get_command_type_name,
};
use ansi_to_tui::IntoText;
use console::strip_ansi_codes;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use regex::Regex;
use stakpak_shared::models::integrations::openai::{
    ToolCall, ToolCallResult, ToolCallResultStatus,
};
use std::sync::OnceLock;
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

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
            r"\x1b\]0;[^\x07\x1b]*(\x07|\x1b\\)|", // Window titles
            r"\\u\{[0-9a-fA-F]+\}|",               // Unicode escapes
            r"\x07"                                // Bell
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

// Add this improved simple text wrapping function
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
/// This is a stripped-down version of render_styled_block_ansi_to_tui without borders or styling
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
    let colors = match tool_call.function.name.as_str() {
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
        _ => BubbleColors {
            border_color: Color::Cyan,
            title_color: term_color(Color::White),
            content_color: term_color(Color::Gray),
            tool_type: "unknown".to_string(),
        },
    };
    (command, outside_title, bubble_title, colors)
}

#[allow(clippy::too_many_arguments)]
pub fn render_styled_block(
    content: &str,
    outside_title: &str,
    bubble_title: &str,
    colors: Option<BubbleColors>,
    terminal_width: usize,
    tool_type: &str,
) -> Vec<Line<'static>> {
    // Just delegate to the ANSI-aware version
    render_styled_block_ansi_to_tui(
        content,
        outside_title,
        bubble_title,
        colors,
        terminal_width,
        tool_type,
        None,
    )
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

        // Calculate content width BEFORE moving spans
        let content_width: usize = line
            .spans
            .iter()
            .map(|span| calculate_display_width(&span.content))
            .sum();

        // Add the content spans
        bordered_line.extend(line.spans);

        // Add padding to fill the width
        let padding_needed = inner_width.saturating_sub(content_width);
        if padding_needed > 0 {
            bordered_line.push(Span::from(" ".repeat(padding_needed)));
        }

        bordered_line.push(Span::styled(" │", Style::default().fg(border_color)));
        result.push(Line::from(bordered_line));
    }
    result.push(bottom_border);
    result
}

pub fn render_file_diff_full(
    tool_call: &ToolCall,
    terminal_width: usize,
    do_show: Option<bool>,
) -> Vec<Line<'static>> {
    let (_diff_lines, mut full_diff_lines) = render_file_diff_block(tool_call, terminal_width);
    let args: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
        .unwrap_or_else(|_| serde_json::json!({}));
    let path = args["path"].as_str().unwrap_or("");

    if full_diff_lines.is_empty() && !do_show.unwrap_or(false) {
        return Vec::new();
    }
    // render header dot
    let spacing_marker = Line::from(vec![Span::from("SPACING_MARKER")]);

    full_diff_lines = [
        vec![spacing_marker.clone()],
        render_styled_header_with_dot(
            "Str Replace",
            path,
            Some(LinesColors {
                dot: Color::Magenta,
                title: Color::Yellow,
                command: AdaptiveColors::text(),
                message: Color::LightGreen,
            }),
        ),
        vec![spacing_marker.clone()],
        full_diff_lines,
    ]
    .concat();

    full_diff_lines
}

pub fn render_file_diff(tool_call: &ToolCall, terminal_width: usize) -> Vec<Line<'static>> {
    if tool_call.function.name == "str_replace" || tool_call.function.name == "create" {
        let (mut diff_lines, _) = render_file_diff_block(tool_call, terminal_width);
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

        let result =
            render_styled_header_and_borders(&formatted_title, diff_lines, None, terminal_width);

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
    is_auto_approved: bool,
) -> Vec<Line<'static>> {
    let (command, outside_title, mut bubble_title, colors) =
        extract_bash_block_info(tool_call, output);

    if is_auto_approved {
        bubble_title = format!("{} - 🔓 Auto-approved tool", bubble_title).to_string();
    }

    render_styled_block_ansi_to_tui(
        &command,
        &outside_title,
        &bubble_title,
        Some(colors.clone()),
        terminal_width,
        &tool_call.function.name,
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
            dot: Color::Magenta,
            title: Color::Yellow,
            command: AdaptiveColors::text(),
            message: Color::LightGreen,
        }),
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

pub fn render_result_block(tool_call_result: &ToolCallResult, width: usize) -> Vec<Line<'static>> {
    let tool_call = tool_call_result.call.clone();
    let result = tool_call_result.result.clone();
    let tool_call_status = tool_call_result.status.clone();

    let title: String = get_command_type_name(&tool_call);
    let command_args = extract_truncated_command_arguments(&tool_call, None);

    let is_collapsed = is_collapsed_tool_call(&tool_call) && result.lines().count() > 3;

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
    if title_with_args.len() <= available_width {
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
    render_styled_lines(command_name, title, message, colors)
}

#[derive(Clone)]
pub struct LinesColors {
    pub dot: Color,
    pub title: Color,
    pub command: Color,
    pub message: Color,
}

fn render_styled_header_with_dot(
    title: &str,
    command_name: &str,
    colors: Option<LinesColors>,
) -> Vec<Line<'static>> {
    let colors = colors.unwrap_or(LinesColors {
        dot: Color::LightRed,
        title: term_color(Color::White),
        command: AdaptiveColors::text(),
        message: Color::LightRed,
    });
    vec![Line::from(vec![
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
            format!(" ({})", command_name),
            Style::default().fg(colors.command),
        ),
        Span::styled("...", Style::default().fg(colors.command)),
    ])]
}

pub fn render_styled_lines(
    command_name: &str,
    title: &str,
    message: Option<String>,
    colors: Option<LinesColors>,
) -> Vec<Line<'static>> {
    let colors = colors.unwrap_or(LinesColors {
        dot: Color::LightRed,
        title: term_color(Color::White),
        command: AdaptiveColors::text(),
        message: Color::LightRed,
    });

    let mut lines = Vec::new();

    lines.push(Line::from(vec![Span::from("SPACING_MARKER")]));

    // Handle multi-line command name if needed
    let title_with_args = format!("{} ({})", title, command_name);
    let max_width = 120; // Reasonable max width for rejected commands

    if title_with_args.len() <= max_width {
        // Single line
        lines.extend(render_styled_header_with_dot(
            title,
            command_name,
            Some(colors.clone()),
        ));
    } else {
        // Multi-line - title on first line, arguments on subsequent lines
        lines.push(Line::from(vec![
            Span::styled(
                "● ",
                Style::default().fg(colors.dot).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                title,
                Style::default()
                    .fg(term_color(Color::White))
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        // Split command arguments into multiple lines
        // Calculate proper indentation to align under the command name
        let title_indent = 2 + title.len(); // "● " + title length
        let args_prefix = " ".repeat(title_indent); // Align directly under the command name
        let args_available_width = max_width - title_indent;

        let wrapped_args = wrap_text_simple_unicode(command_name, args_available_width);

        for (i, arg_line) in wrapped_args.iter().enumerate() {
            if i == 0 {
                // First line of arguments
                lines.push(Line::from(vec![
                    Span::from(args_prefix.clone()),
                    Span::styled(
                        format!("({}", arg_line),
                        Style::default().fg(colors.command),
                    ),
                ]));
            } else {
                // Continuation lines
                lines.push(Line::from(vec![
                    Span::from(args_prefix.clone()),
                    Span::styled(arg_line.clone(), Style::default().fg(colors.command)),
                ]));
            }
        }

        // Close the parentheses on the last line if we had multiple lines
        if wrapped_args.len() > 1
            && let Some(last_line) = lines.last_mut()
            && let Some(last_content_span) = last_line.spans.last_mut()
            && last_content_span.style.fg == Some(Color::Gray)
        {
            last_content_span.content = format!("{})", last_content_span.content).into();
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

pub fn is_collapsed_tool_call(tool_call: &ToolCall) -> bool {
    let tool_call_name = tool_call.function.name.clone();
    let tool_calls = [
        "view",
        "search_memory",
        "search_docs",
        "read_rulebook",
        "local_code_search",
    ];
    if tool_calls.contains(&tool_call_name.as_str()) {
        return true;
    }
    false
}

pub fn render_collapsed_result_block(tool_call_result: &ToolCallResult, state: &mut AppState) {
    let is_collapsed = is_collapsed_tool_call(&tool_call_result.call)
        && tool_call_result.result.lines().count() > 3;
    let result = tool_call_result.result.clone();
    let command_args = extract_truncated_command_arguments(&tool_call_result.call, None);
    let title = get_command_type_name(&tool_call_result.call);
    if is_collapsed {
        let message = format!("Read {} lines (ctrl+t to expand)", result.lines().count());
        let colors = LinesColors {
            dot: Color::LightGreen,
            title: term_color(Color::White),
            command: AdaptiveColors::text(),
            message: AdaptiveColors::text(),
        };
        let lines = render_styled_lines(&command_args, &title, Some(message), Some(colors));
        state.messages.push(Message {
            id: Uuid::new_v4(),
            content: MessageContent::StyledBlock(lines),
            is_collapsed: None,
        });
    }
}
