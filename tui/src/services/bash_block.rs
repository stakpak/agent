use crate::app::AppState;
use crate::services::message::{
    BubbleColors, Message, MessageContent, extract_and_truncate_command, extract_command_purpose,
    extract_file_info, extract_full_command, format_command_content, get_command_type_name,
    wrap_text,
};
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use stakpak_shared::models::integrations::openai::ToolCall;
use uuid::Uuid;

pub fn render_bash_block(
    tool_call: &ToolCall,
    output: &str,
    _accepted: bool,
    state: &mut AppState,
    terminal_size: Size,
) -> Uuid {
    let terminal_width = terminal_size.width as usize;
    // Use full terminal width minus small margin
    let content_width = if terminal_width > 4 {
        terminal_width - 4
    } else {
        40 // minimum fallback
    };

    // Get the command from the tool call
    let full_command = extract_full_command(tool_call);
    // if full_command is "unknown command" then use the output as the command
    let command = if full_command == "unknown command" {
        output.to_string()
    } else {
        full_command
    };

    // Get the outside title (command type name)
    let outside_title = get_command_type_name(tool_call);

    // Get the bubble title (what the command is trying to do)
    let bubble_title = extract_command_purpose(&command, &outside_title);

    let content_lines = format_command_content(output);

    // Use the full content width
    let inner_width = content_width;

    // Create borders for full width
    let horizontal_line = "─".repeat(inner_width + 2);
    let bottom_border = format!("╰{}╯", horizontal_line);

    // Create title border with the bubble title
    let title_border = {
        let title_width = bubble_title.chars().count();
        if title_width <= inner_width {
            let remaining_dashes = inner_width + 2 - title_width;
            format!("╭{}{}", bubble_title, "─".repeat(remaining_dashes)) + "╮"
        } else {
            let truncated_title = bubble_title.chars().take(inner_width).collect::<String>();
            format!("╭{}─╮", truncated_title)
        }
    };

    // Build the bubble lines
    let mut bubble_lines = vec![];

    // Add title border
    bubble_lines.push(title_border);

    // Add content lines with wrapping
    for line in &content_lines {
        let trimmed_line = line.trim_end();

        if trimmed_line.is_empty() {
            let padding = " ".repeat(inner_width);
            bubble_lines.push(format!("│ {} │", padding));
            continue;
        }

        // Wrap long lines
        let wrapped_lines = wrap_text(trimmed_line, inner_width);

        for wrapped_line in wrapped_lines {
            let line_char_count = wrapped_line.chars().count();
            let padding_needed = inner_width - line_char_count;
            let padding = " ".repeat(padding_needed);

            let formatted_line = format!("│ {}{} │", wrapped_line, padding);
            bubble_lines.push(formatted_line);
        }
    }

    // Add bottom border
    bubble_lines.push(bottom_border);

    // Choose colors based on command type
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

    // Create and add message
    let message_id = Uuid::new_v4();
    state.messages.push(Message {
        id: message_id,
        content: MessageContent::BashBubble {
            title: outside_title, // This is the outside title
            content: bubble_lines,
            colors,
            tool_type: tool_call.function.name.clone(),
        },
    });

    message_id
}

pub fn render_bash_result_block(tool_call: &ToolCall, result: &str, state: &mut AppState) {
    let mut lines = Vec::new();

    // Extract the actual command that was executed
    let full_command = extract_full_command(tool_call);
    let file_info = extract_file_info(&full_command);

    // Header line with approved colors (green bullet, white text)
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
        if let Some(file_desc) = file_info {
            Span::styled(
                format!(" ({})", file_desc),
                Style::default().fg(Color::White),
            )
        } else {
            Span::styled(
                format!(" ({})", extract_and_truncate_command(tool_call)),
                Style::default().fg(Color::Gray),
            )
        },
        Span::styled("...", Style::default().fg(Color::Gray)),
    ]));

    // Show the command output
    let output_pad = "    "; // 4 spaces for indentation
    for (i, line) in result.lines().enumerate() {
        let prefix = if i == 0 { "└ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{output_pad}{prefix}"),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(line, Style::default().fg(Color::Gray)),
        ]));
    }

    let mut owned_lines: Vec<Line<'static>> = lines
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
    owned_lines.push(Line::from(vec![Span::styled(
        "  ",
        Style::default().fg(Color::Gray),
    )]));

    state.messages.push(Message {
        id: Uuid::new_v4(),
        content: MessageContent::StyledBlock(owned_lines),
    });
}

// Function to render a rejected bash command (when user selects "No")
pub fn render_bash_block_rejected(command_name: &str, state: &mut AppState) {
    let mut lines = Vec::new();

    // Header - similar to regular bash block
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

    // Add the rejection line
    lines.push(Line::from(vec![Span::styled(
        "  L No (tell Stakpak what to do differently)",
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    )]));

    let mut owned_lines: Vec<Line<'static>> = lines
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
    owned_lines.push(Line::from(vec![Span::styled(
        "  ",
        Style::default().fg(Color::Gray),
    )]));

    state.messages.push(Message {
        id: Uuid::new_v4(),
        content: MessageContent::StyledBlock(owned_lines),
    });
}
