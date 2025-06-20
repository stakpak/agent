use super::message::{extract_full_command_arguments, extract_truncated_command_arguments};
use crate::app::AppState;
use crate::services::message::{
    BubbleColors, Message, MessageContent, extract_command_purpose, get_command_type_name,
};
use ansi_to_tui::IntoText;
use console::strip_ansi_codes;
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use stakpak_shared::models::integrations::openai::ToolCall;
use uuid::Uuid;

// Helper function to wrap text while preserving ANSI codes
fn wrap_ansi_text(text: &str, width: usize) -> Vec<String> {
    // Convert to ratatui text first to parse ANSI codes
    let ratatui_text = match text.into_text() {
        Ok(parsed) => parsed,
        Err(_) => {
            // Fallback: just split by width using stripped text
            let stripped = strip_ansi_codes(text);
            return wrap_text_simple(&stripped, width);
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
            let span_display_width = strip_ansi_codes(span_text).chars().count();

            if current_width + span_display_width <= width {
                // Span fits on current line
                current_line.push_str(span_text);
                current_width += span_display_width;
            } else if current_width == 0 {
                // Span is too long for any line, just add it
                current_line.push_str(span_text);
                wrapped_lines.push(current_line.clone());
                current_line.clear();
                current_width = 0;
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

// Simple fallback text wrapping
fn wrap_text_simple(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut start = 0;

    while start < chars.len() {
        let end = std::cmp::min(start + width, chars.len());
        let line: String = chars[start..end].iter().collect();
        lines.push(line);
        start = end;
    }

    lines
}

pub fn render_styled_block_ansi_to_tui(
    content: &str,
    _outside_title: &str,
    bubble_title: &str,
    _colors: Option<BubbleColors>,
    state: &mut AppState,
    terminal_size: Size,
    _tool_type: &str,
    message_id: Option<Uuid>,
) -> Uuid {
    let terminal_width = terminal_size.width as usize;
    let content_width = if terminal_width > 4 {
        terminal_width - 4
    } else {
        40
    };

    let inner_width = content_width;
    let horizontal_line = "─".repeat(inner_width + 2);

    // Create cyan-colored borders
    let bottom_border = Line::from(vec![Span::styled(
        format!("╰{}╯", horizontal_line),
        Style::default().fg(Color::Cyan),
    )]);

    // Strip ANSI codes for title border calculation
    let stripped_title = strip_ansi_codes(bubble_title);
    let title_border = {
        let title_width = stripped_title.chars().count();
        if title_width <= inner_width {
            let remaining_dashes = inner_width + 2 - title_width;
            Line::from(vec![Span::styled(
                format!("╭{}{}", bubble_title, "─".repeat(remaining_dashes)) + "╮",
                Style::default().fg(Color::Cyan),
            )])
        } else {
            let truncated_title = stripped_title.chars().take(inner_width).collect::<String>();
            Line::from(vec![Span::styled(
                format!("╭{}─╮", truncated_title),
                Style::default().fg(Color::Cyan),
            )])
        }
    };

    // Convert ANSI content to ratatui Text
    let ratatui_text = content
        .into_text()
        .unwrap_or_else(|_| ratatui::text::Text::from(content));

    // Create lines with compact style similar to result blocks
    let mut formatted_lines = Vec::new();
    formatted_lines.push(title_border);

    // Use compact indentation similar to result blocks
    let line_indent = "  "; // 2 spaces like result blocks

    for text_line in ratatui_text.lines {
        if text_line.spans.is_empty() {
            // Empty line with border
            let line_spans = vec![
                Span::styled("│", Style::default().fg(Color::Cyan)),
                Span::from(format!(" {}", " ".repeat(inner_width))),
                Span::styled(" │", Style::default().fg(Color::Cyan)),
            ];
            formatted_lines.push(Line::from(line_spans));
            continue;
        }

        // Check if line needs wrapping
        let display_width: usize = text_line
            .spans
            .iter()
            .map(|span| strip_ansi_codes(&span.content).chars().count())
            .sum();

        // Add compact indentation to content width calculation
        let content_display_width = display_width + line_indent.len();

        if content_display_width <= inner_width {
            // Line fits, add with compact style
            let padding_needed = inner_width - content_display_width;
            let padding = " ".repeat(padding_needed);

            let mut line_spans = vec![
                Span::styled("│", Style::default().fg(Color::Cyan)),
                Span::from(format!(" {}", line_indent)),
            ];
            line_spans.extend(text_line.spans);
            line_spans.push(Span::from(padding));
            line_spans.push(Span::styled(" │", Style::default().fg(Color::Cyan)));

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
                        .map(|span| strip_ansi_codes(&span.content).chars().count())
                        .sum();

                    let total_content_width = wrapped_display_width + line_indent.len();
                    let padding_needed = if total_content_width <= inner_width {
                        inner_width - total_content_width
                    } else {
                        0
                    };
                    let padding = " ".repeat(padding_needed);

                    let mut line_spans = vec![
                        Span::styled("│", Style::default().fg(Color::Cyan)),
                        Span::from(format!(" {}", line_indent)),
                    ];
                    line_spans.extend(first_line.spans.clone());
                    line_spans.push(Span::from(padding));
                    line_spans.push(Span::styled(" │", Style::default().fg(Color::Cyan)));

                    formatted_lines.push(Line::from(line_spans));
                }
            }
        }
    }

    formatted_lines.push(bottom_border);

    let message_id = message_id.unwrap_or_else(Uuid::new_v4);

    // Convert to owned lines for storage
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

    // Store as StyledBlock (same as result block) instead of BashBubble
    state.messages.push(Message {
        id: message_id,
        content: MessageContent::StyledBlock(owned_lines), // Changed from BashBubble to StyledBlock
    });
    message_id
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
            title_color: Color::White,
            content_color: Color::LightGreen,
            tool_type: "create_file".to_string(),
        },
        "edit_file" => BubbleColors {
            border_color: Color::Yellow,
            title_color: Color::White,
            content_color: Color::LightYellow,
            tool_type: "edit_file".to_string(),
        },
        "run_command" => BubbleColors {
            border_color: Color::Cyan,
            title_color: Color::Yellow,
            content_color: Color::Gray,
            tool_type: "run_command".to_string(),
        },
        "read_file" => BubbleColors {
            border_color: Color::Magenta,
            title_color: Color::White,
            content_color: Color::LightMagenta,
            tool_type: "read_file".to_string(),
        },
        "delete_file" => BubbleColors {
            border_color: Color::Red,
            title_color: Color::White,
            content_color: Color::LightRed,
            tool_type: "delete_file".to_string(),
        },
        _ => BubbleColors {
            border_color: Color::Cyan,
            title_color: Color::White,
            content_color: Color::Gray,
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
    state: &mut AppState,
    terminal_size: Size,
    tool_type: &str,
    message_id: Option<Uuid>,
) -> Uuid {
    // Just delegate to the ANSI-aware version
    render_styled_block_ansi_to_tui(
        content,
        outside_title,
        bubble_title,
        colors,
        state,
        terminal_size,
        tool_type,
        message_id,
    )
}

pub fn render_bash_block(
    tool_call: &ToolCall,
    output: &str,
    _accepted: bool,
    state: &mut AppState,
    terminal_size: Size,
) -> Uuid {
    let (command, outside_title, bubble_title, colors) = extract_bash_block_info(tool_call, output);
    render_styled_block_ansi_to_tui(
        &command,
        &outside_title,
        &bubble_title,
        Some(colors.clone()),
        state,
        terminal_size,
        &tool_call.function.name,
        None,
    )
}

// FIXED: render_result_block with ANSI support and cyan borders
pub fn render_result_block(
    tool_call: &ToolCall,
    result: &str,
    state: &mut AppState,
    terminal_size: Size,
) {
    let terminal_width = terminal_size.width as usize;
    let content_width = if terminal_width > 4 {
        terminal_width - 4
    } else {
        40
    };
    let inner_width = content_width;

    let mut lines = Vec::new();

    // Create cyan-colored top border
    let horizontal_line = "─".repeat(inner_width + 2);
    let top_border = Line::from(vec![Span::styled(
        format!("╭{}╮", horizontal_line),
        Style::default().fg(Color::Cyan),
    )]);
    let bottom_border = Line::from(vec![Span::styled(
        format!("╰{}╯", horizontal_line),
        Style::default().fg(Color::Cyan),
    )]);

    lines.push(top_border);

    // Header line with border
    let mut header_spans = vec![
        Span::styled("│", Style::default().fg(Color::Cyan)),
        Span::from(" "),
        Span::styled(
            "● ",
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            tool_call.function.name.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({})", extract_truncated_command_arguments(tool_call)),
            Style::default().fg(Color::Gray),
        ),
    ];

    // Calculate padding for header
    let header_content_width = 2
        + tool_call.function.name.len()
        + extract_truncated_command_arguments(tool_call).len()
        + 3; // "● " + " (" + ")"
    let header_padding = if header_content_width <= inner_width {
        inner_width - header_content_width
    } else {
        0
    };
    header_spans.push(Span::from(" ".repeat(header_padding)));
    header_spans.push(Span::styled(" │", Style::default().fg(Color::Cyan)));

    lines.push(Line::from(header_spans));

    // Convert result to ratatui Text with ANSI support
    let result_text = match result.into_text() {
        Ok(text) => text,
        Err(_) => ratatui::text::Text::from(result),
    };

    // Use compact indentation like bash blocks
    let line_indent = "  "; // 2 spaces for compact style

    for text_line in result_text.lines.iter() {
        if text_line.spans.is_empty() {
            // Empty line with border
            let line_spans = vec![
                Span::styled("│", Style::default().fg(Color::Cyan)),
                Span::from(format!(" {}", " ".repeat(inner_width))),
                Span::styled(" │", Style::default().fg(Color::Cyan)),
            ];
            lines.push(Line::from(line_spans));
            continue;
        }

        // Check if line fits within available width
        let display_width: usize = text_line
            .spans
            .iter()
            .map(|span| strip_ansi_codes(&span.content).chars().count())
            .sum();

        let content_display_width = display_width + line_indent.len();

        if content_display_width <= inner_width {
            // Line fits, add with compact style and border
            let padding_needed = inner_width - content_display_width;
            let padding = " ".repeat(padding_needed);

            let mut line_spans = vec![
                Span::styled("│", Style::default().fg(Color::Cyan)),
                Span::from(format!(" {}", line_indent)),
            ];
            line_spans.extend(text_line.spans.clone());
            line_spans.push(Span::from(padding));
            line_spans.push(Span::styled(" │", Style::default().fg(Color::Cyan)));

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
                        .map(|span| strip_ansi_codes(&span.content).chars().count())
                        .sum();

                    let total_content_width = wrapped_display_width + line_indent.len();
                    let padding_needed = if total_content_width <= inner_width {
                        inner_width - total_content_width
                    } else {
                        0
                    };
                    let padding = " ".repeat(padding_needed);

                    let mut line_spans = vec![
                        Span::styled("│", Style::default().fg(Color::Cyan)),
                        Span::from(format!(" {}", line_indent)),
                    ];
                    line_spans.extend(first_line.spans.clone());
                    line_spans.push(Span::from(padding));
                    line_spans.push(Span::styled(" │", Style::default().fg(Color::Cyan)));

                    lines.push(Line::from(line_spans));
                }
            }
        }
    }

    lines.push(bottom_border);

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

    state.messages.push(Message {
        id: Uuid::new_v4(),
        content: MessageContent::StyledBlock(owned_lines),
    });
}

// Function to render a rejected bash command (when user selects "No")
pub fn render_bash_block_rejected(command_name: &str, state: &mut AppState) {
    let mut lines = Vec::new();

    lines.push(Line::from(vec![
        Span::styled(
            "● ",
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Bash",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({})", command_name),
            Style::default().fg(Color::Gray),
        ),
        Span::styled("...", Style::default().fg(Color::Gray)),
    ]));

    lines.push(Line::from(vec![Span::styled(
        "  L No (tell Stakpak what to do differently)",
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    )]));

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

    state.messages.push(Message {
        id: Uuid::new_v4(),
        content: MessageContent::StyledBlock(owned_lines),
    });
}
