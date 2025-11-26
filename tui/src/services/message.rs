use crate::AppState;
use crate::services::bash_block::{
    format_text_content, is_collapsed_tool_call, render_bash_block, render_file_diff,
    render_file_diff_full, render_result_block, render_styled_block,
};
use crate::services::detect_term::AdaptiveColors;
use crate::services::markdown_renderer::render_markdown_to_lines;
use crate::services::shell_mode::SHELL_PROMPT_PREFIX;
use ratatui::style::Color;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use regex::Regex;
use serde_json::Value;
#[cfg(test)]
use stakpak_shared::models::integrations::openai::FunctionCall;
use stakpak_shared::models::integrations::openai::{ToolCall, ToolCallResult};
use uuid::Uuid;
#[derive(Clone, Debug)]
pub struct BubbleColors {
    pub border_color: Color,
    pub title_color: Color,
    pub content_color: Color,
    pub tool_type: String,
}

#[derive(Clone, Debug)]
pub enum MessageContent {
    Plain(String, Style),
    AssistantMD(String, Style),
    Styled(Line<'static>),
    StyledBlock(Vec<Line<'static>>),
    Markdown(String),
    PlainText(String),
    RenderPendingBorderBlock(ToolCall, bool),
    RenderStreamingBorderBlock(String, String, String, Option<BubbleColors>, String),
    RenderResultBorderBlock(ToolCallResult),
    RenderCollapsedMessage(ToolCall),
    RenderEscapedTextBlock(String),
    BashBubble {
        title: String,
        content: Vec<String>,
        colors: BubbleColors,
        tool_type: String,
    },
}

fn term_color(color: Color) -> Color {
    if crate::services::detect_term::should_use_rgb_colors() {
        color
    } else {
        Color::Reset
    }
}

// Strip markdown code block delimiters from content (for session resume)
fn strip_markdown_delimiters(text: &str) -> String {
    let mut result = text.to_string();

    // Only process if this looks like a markdown code block (contains ```markdown or ```md)
    if !result.contains("```markdown") && !result.contains("```md") {
        return result; // Return unchanged if no markdown delimiters found
    }

    // Remove opening markdown delimiters from anywhere in the text
    if let Some(pos) = result.find("```markdown") {
        // Remove everything from the start up to and including the delimiter
        let after_delimiter = &result[pos + "```markdown".len()..];
        // Remove any leading newline after the delimiter
        result = if let Some(stripped) = after_delimiter.strip_prefix('\n') {
            stripped
        } else {
            after_delimiter
        }
        .to_string();
    } else if let Some(pos) = result.find("```md") {
        // Remove everything from the start up to and including the delimiter
        let after_delimiter = &result[pos + "```md".len()..];
        // Remove any leading newline after the delimiter
        result = if let Some(stripped) = after_delimiter.strip_prefix('\n') {
            stripped
        } else {
            after_delimiter
        }
        .to_string();
    }

    // Only remove the closing delimiter if we removed an opening markdown delimiter
    // This prevents removing closing delimiters from other code blocks
    if result.contains("```") {
        // Find the last occurrence of ``` that might be our closing delimiter
        if let Some(pos) = result.rfind("```") {
            // Check if this looks like a closing delimiter (not followed by a language)
            let after_delimiter = &result[pos + 3..];
            if after_delimiter.trim().is_empty() || after_delimiter.starts_with('\n') {
                // This looks like a closing delimiter, remove it
                result = result[..pos].to_string();
                // Also remove any trailing newline
                if result.ends_with('\n') {
                    result = result[..result.len() - 1].to_string();
                }
            }
        }
    }

    result
}

#[derive(Clone, Debug)]
pub struct Message {
    pub id: Uuid,
    pub content: MessageContent,
    pub is_collapsed: Option<bool>,
}

impl Message {
    pub fn info(text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::Plain(
                text.into(),
                style.unwrap_or(Style::default().fg(Color::DarkGray)),
            ),
            is_collapsed: None,
        }
    }
    pub fn user(text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::Plain(
                format!("→ {}", text.into()),
                style.unwrap_or(Style::default().fg(AdaptiveColors::text())),
            ),
            is_collapsed: None,
        }
    }
    pub fn assistant(id: Option<Uuid>, text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            id: id.unwrap_or(Uuid::new_v4()),
            content: MessageContent::AssistantMD(text.into(), style.unwrap_or_default()),
            is_collapsed: None,
        }
    }
    pub fn submitted_with(id: Option<Uuid>, text: impl Into<String>, style: Option<Style>) -> Self {
        Message {
            id: id.unwrap_or(Uuid::new_v4()),
            content: MessageContent::Plain(text.into(), style.unwrap_or_default()),
            is_collapsed: None,
        }
    }
    pub fn styled(line: Line<'static>) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::Styled(line),
            is_collapsed: None,
        }
    }
    pub fn markdown(text: impl Into<String>) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::Markdown(text.into()),
            is_collapsed: None,
        }
    }

    pub fn plain_text(text: impl Into<String>) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::PlainText(text.into()),
            is_collapsed: None,
        }
    }

    pub fn render_collapsed_message(tool_call: ToolCall) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::RenderCollapsedMessage(tool_call),
            is_collapsed: Some(true),
        }
    }

    pub fn render_pending_border_block(
        tool_call: ToolCall,
        is_auto_approved: bool,
        message_id: Option<Uuid>,
    ) -> Self {
        Message {
            id: message_id.unwrap_or_else(Uuid::new_v4),
            content: MessageContent::RenderPendingBorderBlock(tool_call, is_auto_approved),
            is_collapsed: None,
        }
    }

    pub fn render_streaming_border_block(
        content: &str,
        outside_title: &str,
        bubble_title: &str,
        colors: Option<BubbleColors>,
        tool_type: &str,
        message_id: Option<Uuid>,
    ) -> Self {
        Message {
            id: message_id.unwrap_or_else(Uuid::new_v4),
            content: MessageContent::RenderStreamingBorderBlock(
                content.to_string(),
                outside_title.to_string(),
                bubble_title.to_string(),
                colors,
                tool_type.to_string(),
            ),
            is_collapsed: None,
        }
    }

    pub fn render_escaped_text_block(content: String) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::RenderEscapedTextBlock(content),
            is_collapsed: None,
        }
    }

    pub fn render_result_border_block(tool_call_result: ToolCallResult) -> Self {
        let is_collapsed = is_collapsed_tool_call(&tool_call_result.call)
            && tool_call_result.result.lines().count() > 3;
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::RenderResultBorderBlock(tool_call_result),
            is_collapsed: if is_collapsed { Some(true) } else { None },
        }
    }
}

pub fn get_wrapped_plain_lines<'a>(
    text: &'a str,
    style: &Style,
    width: usize,
) -> Vec<(Line<'a>, Style)> {
    let mut lines = Vec::new();
    for line in text.lines() {
        let mut current = line;
        while !current.is_empty() {
            let take = current
                .char_indices()
                .scan(0, |acc, (i, c)| {
                    *acc += unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                    Some((i, *acc))
                })
                .take_while(|&(_i, w)| w <= width)
                .last()
                .map(|(i, _w)| i + 1)
                .unwrap_or(current.len());
            if take == 0 {
                break;
            }
            let mut safe_take = take;
            while safe_take > 0 && !current.is_char_boundary(safe_take) {
                safe_take -= 1;
            }
            if safe_take == 0 {
                break;
            }
            let (part, rest) = current.split_at(safe_take);
            lines.push((Line::from(vec![Span::styled(part, *style)]), *style));
            current = rest;
        }
    }
    lines
}

pub fn get_wrapped_styled_lines<'a>(line: &Line<'a>, _width: usize) -> Vec<(Line<'a>, Style)> {
    vec![(line.clone(), Style::default())]
}

pub fn get_wrapped_styled_block_lines<'a>(
    lines: &'a [Line<'a>],
    _width: usize,
) -> Vec<(Line<'a>, Style)> {
    lines
        .iter()
        .map(|l| (l.clone(), Style::default()))
        .collect()
}

pub fn get_wrapped_markdown_lines(markdown: &str) -> Vec<(Line<'_>, Style)> {
    let mut result = Vec::new();
    let rendered_lines = render_markdown_to_lines(markdown).unwrap_or_default();
    for line in rendered_lines {
        result.push((line, Style::default()));
    }
    result
}

pub fn get_wrapped_bash_bubble_lines<'a>(
    _title: &'a str,
    content: &'a [String],
    colors: &BubbleColors,
) -> Vec<(Line<'a>, Style)> {
    let _title_style = Style::default()
        .fg(colors.title_color)
        .add_modifier(Modifier::BOLD);
    let border_style = Style::default().fg(colors.border_color);
    let content_style = Style::default().fg(colors.content_color);
    let mut lines = Vec::new();
    // lines.push((
    //     Line::from(vec![Span::styled(title, title_style)]),
    //     title_style,
    // ));
    for line in content.iter() {
        let chars: Vec<char> = line.chars().collect();
        if chars.len() > 2 && chars[0] == '│' && chars[chars.len() - 1] == '│' {
            let mut spans = Vec::new();
            spans.push(Span::styled(chars[0].to_string(), border_style));
            let content: String = chars[1..chars.len() - 1].iter().collect();
            spans.push(Span::styled(content, content_style));
            spans.push(Span::styled(
                chars[chars.len() - 1].to_string(),
                border_style,
            ));
            lines.push((Line::from(spans), border_style));
        } else if line.starts_with('╭') || line.starts_with('╰') {
            lines.push((
                Line::from(vec![Span::styled(line.clone(), border_style)]),
                border_style,
            ));
        } else {
            lines.push((
                Line::from(vec![Span::styled(line.clone(), content_style)]),
                content_style,
            ));
        }
    }
    lines
}

fn render_shell_bubble_with_unicode_border(
    command: &str,
    output_lines: &[String],
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let border_width = width.max(20); // Minimum width for the bubble
    let horizontal = "─".repeat(border_width - 2);
    // Top border
    lines.push(Line::from(vec![Span::styled(
        format!("╭{}╮", horizontal),
        Style::default().fg(Color::Magenta),
    )]));
    // Command line
    let cmd_line = format!("{}{}", SHELL_PROMPT_PREFIX, &command[1..].trim());
    let cmd_content_width = cmd_line.len();
    let cmd_padding = border_width.saturating_sub(4 + cmd_content_width);
    lines.push(Line::from(vec![
        Span::styled("│ ", Style::default().fg(Color::Magenta)),
        Span::styled(
            cmd_line,
            Style::default().fg(term_color(Color::LightYellow)),
        ),
        Span::from(" ".repeat(cmd_padding)),
        Span::styled(" │", Style::default().fg(Color::Magenta)),
    ]));
    // Output lines
    for out in output_lines {
        let padded = format!("{:<width$}", out, width = border_width - 4);
        lines.push(Line::from(vec![
            Span::styled("│ ", Style::default().fg(Color::Magenta)),
            Span::styled(padded, Style::default().fg(AdaptiveColors::text())),
            Span::styled(" │", Style::default().fg(Color::Magenta)),
        ]));
    }
    // Bottom border
    lines.push(Line::from(vec![Span::styled(
        format!("╰{}╯", horizontal),
        Style::default().fg(Color::Magenta),
    )]));
    lines
}

fn convert_to_owned_lines(borrowed_lines: Vec<(Line<'_>, Style)>) -> Vec<(Line<'static>, Style)> {
    borrowed_lines
        .into_iter()
        .map(|(line, style)| (convert_line_to_owned(line), style))
        .collect()
}

// Helper function to convert a single borrowed line to owned
fn convert_line_to_owned(line: Line<'_>) -> Line<'static> {
    let owned_spans: Vec<Span<'static>> = line
        .spans
        .into_iter()
        .map(|span| Span::styled(span.content.into_owned(), span.style))
        .collect();
    Line::from(owned_spans)
}

pub fn get_wrapped_message_lines(
    messages: &[Message],
    width: usize,
) -> Vec<(Line<'static>, Style)> {
    get_wrapped_message_lines_internal(messages, width, false)
}

pub fn get_wrapped_message_lines_cached(state: &mut AppState, width: usize) -> Vec<Line<'static>> {
    let messages = state.messages.clone();
    // Check if cache is valid
    let cache_valid = if let Some((cached_messages, cached_width, _)) = &state.message_lines_cache {
        cached_messages.len() == messages.len()
            && *cached_width == width
            && cached_messages
                .iter()
                .zip(messages.iter())
                .all(|(a, b)| a.id == b.id)
    } else {
        false
    };

    if !cache_valid {
        // Calculate and cache the processed lines directly
        let processed_lines = get_processed_message_lines(&messages, width);
        state.message_lines_cache = Some((messages.to_vec(), width, processed_lines.clone()));
        processed_lines
    } else {
        // Return cached processed lines immediately - no more processing needed!
        if let Some((_, _, cached_lines)) = &state.message_lines_cache {
            cached_lines.clone()
        } else {
            // Fallback if cache is somehow invalid
            get_processed_message_lines(&messages, width)
        }
    }
}

// New function that does all the heavy processing once and caches the result
pub fn get_processed_message_lines(messages: &[Message], width: usize) -> Vec<Line<'static>> {
    use crate::services::message_pattern::{process_checkpoint_patterns, spans_to_string};

    let all_lines: Vec<(Line, Style)> = get_wrapped_message_lines(messages, width);

    // Pre-allocate with estimated capacity to reduce reallocations
    let estimated_capacity = all_lines.len() + (all_lines.len() / 10); // +10% for processing overhead
    let mut processed_lines: Vec<Line> = Vec::with_capacity(estimated_capacity);

    for (line, _style) in all_lines.iter() {
        let line_text = spans_to_string(line);
        if line_text.contains("<checkpoint_id>") {
            processed_lines.push(Line::from(""));
            let processed = process_checkpoint_patterns(&[(line.clone(), Style::default())], width);
            for (processed_line, _) in processed {
                processed_lines.push(processed_line);
            }
            processed_lines.push(Line::from(""));
        } else {
            // local_context and rulebooks are stripped earlier in Plain message processing
            if line_text.trim() == "SPACING_MARKER" {
                processed_lines.push(Line::from(""));
            } else {
                processed_lines.push(line.clone());
            }
        }
    }

    processed_lines
}

/// Invalidate the message lines cache when messages change
/// Smart invalidation: Skip when user has scrolled up to avoid jitter during streaming
pub fn invalidate_message_lines_cache(state: &mut AppState) {
    // If user has scrolled up (reading old messages), don't invalidate cache
    // This prevents jitter when new streaming chunks arrive while scrolled up
    if !state.stay_at_bottom && state.is_streaming {
        return;
    }

    state.message_lines_cache = None;
    state.collapsed_message_lines_cache = None;
}

pub fn get_wrapped_collapsed_message_lines_cached(
    state: &mut AppState,
    width: usize,
) -> Vec<Line<'static>> {
    // Get only collapsed messages
    let collapsed_messages: Vec<Message> = state
        .messages
        .iter()
        .filter(|m| m.is_collapsed == Some(true))
        .cloned()
        .collect();

    // Check if cache is valid
    let cache_valid =
        if let Some((cached_messages, cached_width, _)) = &state.collapsed_message_lines_cache {
            cached_messages.len() == collapsed_messages.len()
                && *cached_width == width
                && (collapsed_messages.is_empty()
                    || cached_messages
                        .iter()
                        .zip(collapsed_messages.iter())
                        .all(|(a, b)| a.id == b.id))
        } else {
            false
        };

    if !cache_valid {
        // Calculate and cache the processed lines directly

        let processed_lines: Vec<Line<'static>> =
            get_wrapped_message_lines_internal(&collapsed_messages, width, true)
                .into_iter()
                .map(|(line, _style)| line)
                .collect();

        state.collapsed_message_lines_cache =
            Some((collapsed_messages.to_vec(), width, processed_lines.clone()));
        processed_lines
    } else {
        // Return cached processed lines immediately
        if let Some((_, _, cached_lines)) = &state.collapsed_message_lines_cache {
            cached_lines.clone()
        } else {
            // Fallback if cache is somehow invalid

            get_wrapped_message_lines_internal(&collapsed_messages, width, true)
                .into_iter()
                .map(|(line, _style)| line)
                .collect()
        }
    }
}

fn get_wrapped_message_lines_internal(
    messages: &[Message],
    width: usize,
    include_collapsed: bool,
) -> Vec<(Line<'static>, Style)> {
    let filtered_messages = if include_collapsed {
        messages.iter().collect::<Vec<_>>()
    } else {
        messages
            .iter()
            .filter(|m| m.is_collapsed.is_none())
            .collect::<Vec<_>>()
    };
    let mut all_lines = Vec::new();
    let mut agent_mode_removed = false;
    let mut checkpoint_id_removed = false;

    for msg in filtered_messages {
        match &msg.content {
            MessageContent::AssistantMD(text, style) => {
                let mut cleaned = text.to_string();

                // Strip markdown delimiters first (for session resume)
                cleaned = strip_markdown_delimiters(&cleaned);
                if !agent_mode_removed
                    && let Some(start) = cleaned.find("<agent_mode>")
                    && let Some(end) = cleaned.find("</agent_mode>")
                {
                    cleaned.replace_range(start..end + "</agent_mode>".len(), "");
                }
                if !checkpoint_id_removed
                    && let Some(start) = cleaned.find("<checkpoint_id>")
                    && let Some(end) = cleaned.find("</checkpoint_id>")
                {
                    // Remove the checkpoint_id tag and any preceding newline
                    let before_checkpoint = &cleaned[..start];
                    let after_checkpoint = &cleaned[end + "</checkpoint_id>".len()..];

                    // If there's a newline before the checkpoint_id, remove it too
                    let cleaned_before =
                        if let Some(stripped) = before_checkpoint.strip_suffix('\n') {
                            stripped
                        } else {
                            before_checkpoint
                        };

                    cleaned = format!("{}{}", cleaned_before, after_checkpoint);
                }

                let borrowed_lines =
                    render_markdown_to_lines(&cleaned.to_string()).unwrap_or_default();
                // let borrowed_lines = get_wrapped_plain_lines(&cleaned, style, width);
                for line in borrowed_lines {
                    all_lines.push((convert_line_to_owned(line), *style));
                }
            }
            MessageContent::Plain(text, style) => {
                let mut cleaned = text.to_string();

                // Strip local_context blocks from user messages
                while let Some(start) = cleaned.find("<local_context>") {
                    if let Some(end) = cleaned[start..].find("</local_context>") {
                        let end_pos = start + end + "</local_context>".len();
                        cleaned.replace_range(start..end_pos, "");
                    } else {
                        break;
                    }
                }

                // Strip rulebooks blocks from user messages
                while let Some(start) = cleaned.find("<rulebooks>") {
                    if let Some(end) = cleaned[start..].find("</rulebooks>") {
                        let end_pos = start + end + "</rulebooks>".len();
                        cleaned.replace_range(start..end_pos, "");
                    } else {
                        break;
                    }
                }

                // Check for shell history first (before newline processing)
                if cleaned.contains("Here's my shell history:") && cleaned.contains("```shell") {
                    let mut remaining = cleaned.as_str();
                    while let Some(start) = remaining.find("```shell") {
                        let before = &remaining[..start];
                        if !before.trim().is_empty() {
                            // Convert borrowed lines to owned
                            let borrowed_lines = get_wrapped_plain_lines(before, style, width);
                            let owned_lines = convert_to_owned_lines(borrowed_lines);
                            all_lines.extend(owned_lines);
                            all_lines.push((
                                Line::from(vec![Span::from("SPACING_MARKER")]),
                                Style::default(),
                            ));
                        }
                        let after_start = &remaining[start + "```shell".len()..];
                        if let Some(end) = after_start.find("```") {
                            let shell_block = &after_start[..end];
                            let mut lines = Vec::new();
                            let mut current_command: Option<String> = None;
                            let mut current_output = Vec::new();
                            for line in shell_block.lines() {
                                if line.trim().starts_with(SHELL_PROMPT_PREFIX.trim()) {
                                    if let Some(cmd) = current_command.take() {
                                        lines.push(render_shell_bubble_with_unicode_border(
                                            &cmd,
                                            &current_output,
                                            width,
                                        ));
                                        current_output.clear();
                                    }
                                    current_command = Some(line.trim().to_string());
                                } else {
                                    current_output.push(line.to_string());
                                }
                            }
                            if let Some(cmd) = current_command {
                                lines.push(render_shell_bubble_with_unicode_border(
                                    &cmd,
                                    &current_output,
                                    width,
                                ));
                            }
                            for bubble in lines {
                                for l in bubble {
                                    // Convert to owned line
                                    let owned_line = convert_line_to_owned(l);
                                    all_lines.push((owned_line, Style::default()));
                                }
                            }
                            remaining = &after_start[end + "```".len()..];

                            all_lines.push((
                                Line::from(vec![Span::from("SPACING_MARKER")]),
                                Style::default(),
                            ));
                        } else {
                            if !after_start.trim().is_empty() {
                                let borrowed_lines =
                                    get_wrapped_plain_lines(after_start, style, width);
                                let owned_lines = convert_to_owned_lines(borrowed_lines);
                                all_lines.extend(owned_lines);
                            }
                            break;
                        }
                    }
                    if !remaining.trim().is_empty() {
                        let borrowed_lines = get_wrapped_plain_lines(remaining, style, width);
                        let owned_lines = convert_to_owned_lines(borrowed_lines);
                        all_lines.extend(owned_lines);
                    }
                } else if cleaned.contains("\n\n") {
                    // Handle double newlines: split sections and add spacing
                    for (i, section) in cleaned.split("\n\n").enumerate() {
                        if i > 0 {
                            all_lines.push((
                                Line::from(vec![Span::from("SPACING_MARKER")]),
                                Style::default(),
                            ));
                        }
                        for line in section.split('\n') {
                            let borrowed_lines = get_wrapped_plain_lines(line, style, width);
                            all_lines.extend(convert_to_owned_lines(borrowed_lines));
                        }
                    }
                } else if cleaned.contains('\n') {
                    // Handle single newlines: split into lines
                    for line in cleaned.split('\n') {
                        let borrowed_lines = get_wrapped_plain_lines(line, style, width);
                        all_lines.extend(convert_to_owned_lines(borrowed_lines));
                    }
                } else {
                    let borrowed_lines = get_wrapped_plain_lines(text, style, width);
                    let owned_lines = convert_to_owned_lines(borrowed_lines);
                    all_lines.extend(owned_lines);
                }
            }
            MessageContent::Styled(line) => {
                let borrowed_lines = get_wrapped_styled_lines(line, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::StyledBlock(lines) => {
                let borrowed_lines = get_wrapped_styled_block_lines(lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::RenderPendingBorderBlock(tool_call, is_auto_approved) => {
                let full_command = extract_full_command_arguments(tool_call);
                let rendered_lines = if (tool_call.function.name == "str_replace"
                    || tool_call.function.name == "create")
                    && !render_file_diff(tool_call, width).is_empty()
                {
                    render_file_diff(tool_call, width)
                } else {
                    render_bash_block(tool_call, &full_command, false, width, *is_auto_approved)
                };
                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }

            MessageContent::RenderCollapsedMessage(tool_call) => {
                if tool_call.function.name == "str_replace" {
                    let rendered_lines = render_file_diff_full(tool_call, width, Some(true));
                    if !rendered_lines.is_empty() {
                        let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                        let owned_lines = convert_to_owned_lines(borrowed_lines);
                        all_lines.extend(owned_lines);
                    }
                }
            }
            MessageContent::RenderStreamingBorderBlock(
                content,
                outside_title,
                bubble_title,
                colors,
                tool_type,
            ) => {
                let rendered_lines = render_styled_block(
                    content,
                    outside_title,
                    bubble_title,
                    colors.clone(),
                    width,
                    tool_type,
                );
                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::RenderResultBorderBlock(tool_call_result) => {
                let rendered_lines = render_result_block(tool_call_result, width);
                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::RenderEscapedTextBlock(content) => {
                let rendered_lines = format_text_content(content, width);
                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::Markdown(markdown) => {
                let borrowed_lines = get_wrapped_markdown_lines(markdown);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::PlainText(text) => {
                let owned_line = Line::from(vec![Span::styled(text.clone(), Style::default())]);
                all_lines.push((owned_line, Style::default()));
            }
            MessageContent::BashBubble {
                title,
                content,
                colors,
                tool_type: _,
            } => {
                let borrowed_lines = get_wrapped_bash_bubble_lines(title, content, colors);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
        };
        agent_mode_removed = false;
        checkpoint_id_removed = false;
    }
    if !all_lines.is_empty() {
        all_lines.push((Line::from(""), Style::default()));
        all_lines.push((Line::from(""), Style::default()));
    }
    all_lines
}

pub fn extract_truncated_command_arguments(tool_call: &ToolCall, sign: Option<String>) -> String {
    let arguments = serde_json::from_str::<Value>(&tool_call.function.arguments);
    const KEYWORDS: [&str; 6] = ["path", "file", "uri", "url", "command", "keywords"];

    if let Ok(arguments) = arguments {
        // Check each keyword in order of priority
        for &keyword in &KEYWORDS {
            if let Some(value) = arguments.get(keyword) {
                let formatted_val = format_simple_value(value);
                let sign = sign
                    .map(|s| format!("{} ", s))
                    .unwrap_or_else(|| "= ".to_string());
                return format!("{} {}{}", keyword, sign, formatted_val);
            }
        }

        // If no keywords found, return the first parameter
        if let Value::Object(obj) = arguments
            && let Some((key, val)) = obj.into_iter().next()
        {
            let formatted_val = format_simple_value(&val);
            return format!("{} = {}", key, formatted_val);
        }
    }

    "no arguments".to_string()
}

pub fn extract_full_command_arguments(tool_call: &ToolCall) -> String {
    // First try to parse as valid JSON
    if let Ok(v) = serde_json::from_str::<Value>(&tool_call.function.arguments) {
        return format_json_value(&v);
    }

    // If JSON parsing fails, try regex patterns for malformed JSON
    let patterns = vec![
        // Pattern for key-value pairs with quotes
        r#"["']?(\w+)["']?\s*:\s*["']([^"']+)["']"#,
        // Pattern for simple key-value without quotes
        r#"(\w+)\s*:\s*([^,}\s]+)"#,
    ];

    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            let mut results = Vec::new();
            for caps in re.captures_iter(&tool_call.function.arguments) {
                if caps.len() >= 3 {
                    let key = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                    let value = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                    results.push(format!("{} = {}", key, value));
                }
            }
            if !results.is_empty() {
                return results.join(", ");
            }
        }
    }

    // Try to wrap in braces and parse as JSON
    let wrapped = format!("{{{}}}", tool_call.function.arguments);
    if let Ok(v) = serde_json::from_str::<Value>(&wrapped) {
        return format_json_value(&v);
    }

    // If all else fails, return the raw arguments if they're not empty
    let trimmed = tool_call.function.arguments.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }

    // Last resort
    format!("function_name={}", tool_call.function.name)
}

fn format_json_value(value: &Value) -> String {
    match value {
        Value::Object(obj) => {
            if obj.is_empty() {
                return "{}".to_string();
            }

            let mut values = obj
                .into_iter()
                .map(|(key, val)| (key, format_json_value(val)))
                .collect::<Vec<_>>();
            values.sort_by_key(|(_, val)| val.len());
            values
                .into_iter()
                .map(|(key, val)| {
                    if val.len() > 100 {
                        format!("{} = ```\n{}\n```", key, val)
                    } else {
                        format!("{} = {}", key, val)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                "[]".to_string()
            } else {
                format!(
                    "[{}]",
                    arr.iter()
                        .map(format_simple_value)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        _ => format_simple_value(value),
    }
}

fn format_simple_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Object(_) => "object".to_string(),
        Value::Array(arr) => format!("[{}]", arr.len()),
    }
}

// Helper function to extract what the command is trying to do (bubble title)
pub fn extract_command_purpose(command: &str, outside_title: &str) -> String {
    let command = command.trim();

    // File creation patterns
    if let Some(pos) = command.find(" > ") {
        let after_redirect = &command[pos + 3..];
        if let Some(filename) = after_redirect.split_whitespace().next() {
            return format!("Creating {}", filename);
        }
    }

    if command.starts_with("cat >")
        && let Some(after_cat) = command.strip_prefix("cat >")
        && let Some(filename) = after_cat.split_whitespace().next()
    {
        return format!("Creating {}", filename);
    }

    if command.contains("echo")
        && command.contains(" > ")
        && let Some(pos) = command.find(" > ")
    {
        let after_redirect = &command[pos + 3..];
        if let Some(filename) = after_redirect.split_whitespace().next() {
            return format!("Creating {}", filename);
        }
    }

    if command.starts_with("touch ") {
        let after_touch = command.strip_prefix("touch ");
        if let Some(filename) = after_touch
            && let Some(filename) = filename.split_whitespace().next()
        {
            return format!("Creating {}", filename);
        }
    }

    if command.starts_with("mkdir ") {
        let after_mkdir = command.strip_prefix("mkdir ");
        if let Some(dirname) = after_mkdir
            && let Some(dirname) = dirname.split_whitespace().next()
        {
            return format!("Creating directory {}", dirname);
        }
    }

    if command.starts_with("rm ") {
        let after_rm = command.strip_prefix("rm ");
        if let Some(filename) = after_rm
            && let Some(filename) = filename.split_whitespace().next()
        {
            return format!("Deleting {}", filename);
        }
    }

    if command.starts_with("cp ") {
        return "Copying file".to_string();
    }

    if command.starts_with("mv ") {
        return "Moving file".to_string();
    }

    if command.starts_with("ls") {
        return "Listing directory".to_string();
    }

    if command.starts_with("cd ") {
        let after_cd = command.strip_prefix("cd ");
        if let Some(dirname) = after_cd
            && let Some(dirname) = dirname.split_whitespace().next()
        {
            return format!("Changing to {}", dirname);
        }
    }

    if command.starts_with("git ") {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.len() > 1 {
            match parts[1] {
                "add" => return "Adding files to git".to_string(),
                "commit" => return "Committing changes".to_string(),
                "push" => return "Pushing to remote".to_string(),
                "pull" => return "Pulling from remote".to_string(),
                "clone" => return "Cloning repository".to_string(),
                _ => return format!("Git {}", parts[1]),
            }
        }
    }

    if command.starts_with("npm ") {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.len() > 1 {
            match parts[1] {
                "install" => return "Installing npm packages".to_string(),
                "start" => return "Starting npm script".to_string(),
                "run" => return "Running npm script".to_string(),
                "build" => return "Building project".to_string(),
                _ => return format!("Running npm {}", parts[1]),
            }
        }
    }

    if command.starts_with("python ") || command.starts_with("python3 ") {
        return "Running Python script".to_string();
    }

    if command.starts_with("node ") {
        return "Running Node.js script".to_string();
    }

    if command.starts_with("cargo ") {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.len() > 1 {
            match parts[1] {
                "build" => return "Building Rust project".to_string(),
                "run" => return "Running Rust project".to_string(),
                "test" => return "Testing Rust project".to_string(),
                _ => return format!("Cargo {}", parts[1]),
            }
        }
    }

    // Default: return the command itself (first few words)
    let words: Vec<&str> = command.split_whitespace().take(3).collect();

    if words.is_empty() {
        "Running command".to_string()
    } else if !outside_title.is_empty() {
        outside_title.to_string()
    } else {
        words.join(" ")
    }
}

// Helper function to get command name for the outside title
pub fn get_command_type_name(tool_call: &ToolCall) -> String {
    match tool_call.function.name.as_str() {
        "create_file" => "Create file".to_string(),
        "edit_file" => "Edit file".to_string(),
        "run_command" => "Run command".to_string(),
        "read_file" => "Read file".to_string(),
        "delete_file" => "Delete file".to_string(),
        "list_directory" => "List directory".to_string(),
        "search_files" => "Search files".to_string(),
        _ => {
            // Convert function name to title case
            tool_call
                .function
                .name
                .replace("_", " ")
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_various_formats() {
        // Test cases based on your examples
        let test_cases = vec![
            (r#"{"path":"."}"#, "path=."),
            (r#"{"confidence":1.0}"#, "confidence=1.0"),
            (r#"{"command":"ls -la"}"#, "command=ls -la"),
            (
                r#"{"action":"view","target":"file.txt"}"#,
                "action=view, target=file.txt",
            ),
            (r#"path: ".", mode: "list""#, "path=., mode=list"),
            ("", "function_name=test"),
        ];

        for (input, expected) in test_cases {
            let tool_call = ToolCall {
                id: "test".to_string(),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: "test".to_string(),
                    arguments: input.to_string(),
                },
            };

            let result = extract_full_command_arguments(&tool_call);
            println!(
                "Input: '{}' -> Output: '{}' (Expected: '{}')",
                input, result, expected
            );
        }
    }
}
