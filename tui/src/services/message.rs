use crate::AppState;
use crate::app::RenderedMessageCache;
use crate::services::bash_block::{
    format_text_content, render_bash_block, render_collapsed_command_message, render_file_diff,
    render_file_diff_full, render_result_block, render_streaming_block_compact,
};
use crate::services::detect_term::AdaptiveColors;
use crate::services::markdown_renderer::render_markdown_to_lines_with_width;
use crate::services::shell_mode::SHELL_PROMPT_PREFIX;
use ratatui::style::Color;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use regex::Regex;
use serde_json::Value;
#[cfg(test)]
use stakpak_shared::models::integrations::openai::FunctionCall;
use stakpak_shared::models::integrations::openai::{
    ToolCall, ToolCallResult, ToolCallResultStatus, ToolCallStreamInfo,
};
use stakpak_shared::utils::strip_tool_name;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Instant;
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
    /// User message with special rendering (cyan bar prefix, word wrapping)
    UserMessage(String),
    RenderPendingBorderBlock(ToolCall, bool),
    RenderPendingBorderBlockWithStallWarning(ToolCall, bool, String),
    RenderStreamingBorderBlock(String, String, String, Option<BubbleColors>, String),
    RenderResultBorderBlock(ToolCallResult),
    RenderCommandCollapsedResult(ToolCallResult),
    RenderCollapsedMessage(ToolCall),
    RenderFullContentMessage(ToolCallResult), // Full content for popup view
    RenderEscapedTextBlock(String),
    BashBubble {
        title: String,
        content: Vec<String>,
        colors: BubbleColors,
        tool_type: String,
    },
    RenderRefreshedTerminal(
        String,             // Title
        Vec<Line<'static>>, // Content
        Option<BubbleColors>,
        usize, // Width
    ),
    /// Unified run command block - shows command, state, and result in one bordered box
    /// (command: String, result: Option<String>, state: RunCommandState)
    RenderRunCommandBlock(
        String,
        Option<String>,
        crate::services::bash_block::RunCommandState,
    ),
    /// View file block - compact display showing file path, line count, and optional grep/glob
    /// (file_path: String, total_lines: usize, grep: Option<String>, glob: Option<String>)
    RenderViewFileBlock(String, usize, Option<String>, Option<String>),
    /// Task wait block - shows progress of background tasks being waited on
    /// (task_updates: Vec<TaskUpdate>, overall_progress: f64, target_task_ids: Vec<String>)
    RenderTaskWaitBlock(
        Vec<stakpak_shared::models::integrations::openai::TaskUpdate>,
        f64,
        Vec<String>,
    ),
    /// Subagent resume pending block - shows what the subagent wants to do
    /// (tool_call: ToolCall, is_auto_approved: bool, pause_info: Option<TaskPauseInfo>)
    RenderSubagentResumePendingBlock(
        ToolCall,
        bool,
        Option<stakpak_shared::models::integrations::openai::TaskPauseInfo>,
    ),
    /// Tool call streaming preview - shows tools being generated with token counters
    /// (tool_infos: Vec<ToolCallStreamInfo>)
    RenderToolCallStreamBlock(Vec<ToolCallStreamInfo>),
    /// Ask user inline block - renders questions and options inline in the message area
    RenderAskUserBlock {
        questions: Vec<stakpak_shared::models::integrations::openai::AskUserQuestion>,
        answers: std::collections::HashMap<
            String,
            stakpak_shared::models::integrations::openai::AskUserAnswer,
        >,
        current_tab: usize,
        selected_option: usize,
        custom_input: String,
        focused: bool,
    },
}

/// Compute a hash of the MessageContent for cache invalidation.
/// This is a fast hash that captures content changes without deep comparison.
pub fn hash_message_content(content: &MessageContent) -> u64 {
    let mut hasher = DefaultHasher::new();

    match content {
        MessageContent::Plain(text, _) => {
            0u8.hash(&mut hasher);
            text.hash(&mut hasher);
        }
        MessageContent::AssistantMD(text, _) => {
            1u8.hash(&mut hasher);
            text.hash(&mut hasher);
        }
        MessageContent::Styled(line) => {
            2u8.hash(&mut hasher);
            // Hash the span contents
            for span in &line.spans {
                span.content.as_ref().hash(&mut hasher);
            }
        }
        MessageContent::StyledBlock(lines) => {
            3u8.hash(&mut hasher);
            lines.len().hash(&mut hasher);
            // Hash first and last line for speed
            if let Some(first) = lines.first() {
                for span in &first.spans {
                    span.content.as_ref().hash(&mut hasher);
                }
            }
            if let Some(last) = lines.last() {
                for span in &last.spans {
                    span.content.as_ref().hash(&mut hasher);
                }
            }
        }
        MessageContent::Markdown(text) => {
            4u8.hash(&mut hasher);
            text.hash(&mut hasher);
        }
        MessageContent::PlainText(text) => {
            5u8.hash(&mut hasher);
            text.hash(&mut hasher);
        }
        MessageContent::RenderPendingBorderBlock(tool_call, is_auto) => {
            6u8.hash(&mut hasher);
            tool_call.id.hash(&mut hasher);
            tool_call.function.arguments.hash(&mut hasher);
            is_auto.hash(&mut hasher);
        }
        MessageContent::RenderPendingBorderBlockWithStallWarning(tool_call, is_auto, msg) => {
            7u8.hash(&mut hasher);
            tool_call.id.hash(&mut hasher);
            is_auto.hash(&mut hasher);
            msg.hash(&mut hasher);
        }
        MessageContent::RenderStreamingBorderBlock(content, title, bubble, _, tool_type) => {
            8u8.hash(&mut hasher);
            content.hash(&mut hasher);
            title.hash(&mut hasher);
            bubble.hash(&mut hasher);
            tool_type.hash(&mut hasher);
        }
        MessageContent::RenderResultBorderBlock(result) => {
            9u8.hash(&mut hasher);
            result.call.id.hash(&mut hasher);
            result.result.len().hash(&mut hasher);
            // Hash a portion of the result for efficiency
            result
                .result
                .get(..100.min(result.result.len()))
                .hash(&mut hasher);
        }
        MessageContent::RenderCommandCollapsedResult(result) => {
            10u8.hash(&mut hasher);
            result.call.id.hash(&mut hasher);
            result.result.len().hash(&mut hasher);
        }
        MessageContent::RenderCollapsedMessage(tool_call) => {
            11u8.hash(&mut hasher);
            tool_call.id.hash(&mut hasher);
        }
        MessageContent::RenderFullContentMessage(result) => {
            12u8.hash(&mut hasher);
            result.call.id.hash(&mut hasher);
            result.result.len().hash(&mut hasher);
        }
        MessageContent::RenderEscapedTextBlock(content) => {
            13u8.hash(&mut hasher);
            content.hash(&mut hasher);
        }
        MessageContent::BashBubble {
            title,
            content,
            colors: _,
            tool_type,
        } => {
            14u8.hash(&mut hasher);
            title.hash(&mut hasher);
            content.len().hash(&mut hasher);
            tool_type.hash(&mut hasher);
        }
        MessageContent::RenderRefreshedTerminal(title, lines, _, width) => {
            15u8.hash(&mut hasher);
            title.hash(&mut hasher);
            lines.len().hash(&mut hasher);
            width.hash(&mut hasher);
        }
        MessageContent::RenderRunCommandBlock(command, result, state) => {
            16u8.hash(&mut hasher);
            command.hash(&mut hasher);
            if let Some(r) = result {
                r.len().hash(&mut hasher);
            }
            // Hash state discriminant and inner value for stall warning
            std::mem::discriminant(state).hash(&mut hasher);
            if let crate::services::bash_block::RunCommandState::RunningWithStallWarning(msg) =
                state
            {
                msg.hash(&mut hasher);
            }
        }
        MessageContent::RenderViewFileBlock(file_path, total_lines, grep, glob) => {
            17u8.hash(&mut hasher);
            file_path.hash(&mut hasher);
            total_lines.hash(&mut hasher);
            grep.hash(&mut hasher);
            glob.hash(&mut hasher);
        }
        MessageContent::UserMessage(text) => {
            18u8.hash(&mut hasher);
            text.hash(&mut hasher);
        }
        MessageContent::RenderTaskWaitBlock(task_updates, progress, target_ids) => {
            19u8.hash(&mut hasher);
            task_updates.len().hash(&mut hasher);
            // Hash progress as integer to avoid float hashing issues
            (*progress as u64).hash(&mut hasher);
            target_ids.hash(&mut hasher);
            // Hash task statuses and durations for change detection
            for task in task_updates {
                task.task_id.hash(&mut hasher);
                task.status.hash(&mut hasher);
                // Hash duration as integer seconds for change detection
                if let Some(d) = task.duration_secs {
                    (d as u64).hash(&mut hasher);
                }
            }
        }
        MessageContent::RenderSubagentResumePendingBlock(tool_call, is_auto, pause_info) => {
            20u8.hash(&mut hasher);
            tool_call.id.hash(&mut hasher);
            is_auto.hash(&mut hasher);
            if let Some(pi) = pause_info {
                if let Some(msg) = &pi.agent_message {
                    msg.hash(&mut hasher);
                }
                if let Some(calls) = &pi.pending_tool_calls {
                    calls.len().hash(&mut hasher);
                }
            }
        }
        MessageContent::RenderToolCallStreamBlock(infos) => {
            21u8.hash(&mut hasher);
            infos.len().hash(&mut hasher);
            for info in infos {
                info.name.hash(&mut hasher);
                info.args_tokens.hash(&mut hasher);
            }
        }
        MessageContent::RenderAskUserBlock {
            questions,
            answers,
            current_tab,
            selected_option,
            custom_input,
            focused,
        } => {
            22u8.hash(&mut hasher);
            questions.len().hash(&mut hasher);
            current_tab.hash(&mut hasher);
            selected_option.hash(&mut hasher);
            custom_input.hash(&mut hasher);
            focused.hash(&mut hasher);
            answers.len().hash(&mut hasher);
            for q in questions {
                q.label.hash(&mut hasher);
            }
            for (k, v) in answers {
                k.hash(&mut hasher);
                v.answer.hash(&mut hasher);
            }
        }
    }

    hasher.finish()
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

/// Tags to strip from user message display
const HIDDEN_XML_TAGS: &[&str] = &[
    "local_context",
    "rulebooks",
    "agent_mode",
    "agents_md",
    "apps_md",
];

/// Strip XML-style blocks from text (e.g., <tag>...</tag>)
fn strip_xml_block(text: &mut String, tag: &str) {
    let open_tag = format!("<{}>", tag);
    let close_tag = format!("</{}>", tag);

    while let Some(start) = text.find(&open_tag) {
        if let Some(end) = text.find(&close_tag) {
            let end_pos = end + close_tag.len();
            *text = format!("{}{}", &text[..start], &text[end_pos..]);
        } else {
            break;
        }
    }
}

/// Strip hidden XML blocks from user message display
fn strip_context_blocks(text: &str) -> String {
    let mut result = text.to_string();

    for tag in HIDDEN_XML_TAGS {
        strip_xml_block(&mut result, tag);
    }

    // Trim leading/trailing whitespace
    result.trim().to_string()
}

/// Render user message with cyan bar prefix and proper word wrapping
fn render_user_message_lines(text: &str, width: usize) -> Vec<(Line<'static>, Style)> {
    use ratatui::text::{Line, Span};
    use textwrap::{Options, wrap};

    let mut lines = Vec::new();
    let accent_color = Color::DarkGray;
    let text_color = AdaptiveColors::user_text();

    // The bar takes 2 chars "┃ ", so content width is width - 2
    let content_width = width.saturating_sub(2).max(10);

    // Process each paragraph (split by newlines)
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            // Empty line - just the bar
            lines.push((
                Line::from(vec![Span::styled(
                    "┃ ".to_string(),
                    Style::default().fg(accent_color),
                )]),
                Style::default(),
            ));
        } else {
            // Wrap the paragraph text
            let wrap_options = Options::new(content_width);
            let wrapped = wrap(paragraph, wrap_options);

            for wrapped_line in wrapped {
                lines.push((
                    Line::from(vec![
                        Span::styled("┃ ".to_string(), Style::default().fg(accent_color)),
                        Span::styled(wrapped_line.to_string(), Style::default().fg(text_color)),
                    ]),
                    Style::default(),
                ));
            }
        }
    }

    lines
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
    pub fn user(text: impl Into<String>, _style: Option<Style>) -> Self {
        let text_str = text.into();
        // Strip <local_context>...</local_context> and <rulebooks>...</rulebooks> blocks from display
        let display_text = strip_context_blocks(&text_str);

        Message {
            id: Uuid::new_v4(),
            content: MessageContent::UserMessage(display_text),
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

    pub fn render_collapsed_command_message(tool_call_result: ToolCallResult) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::RenderCommandCollapsedResult(tool_call_result),
            // is_collapsed: None means it shows in main TUI view
            // Full screen popup gets content via separate mechanism
            is_collapsed: None,
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
        // is_collapsed: None means it shows in main TUI view
        // For str_replace/create, we want the diff block to show in TUI
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::RenderResultBorderBlock(tool_call_result),
            is_collapsed: None,
        }
    }

    /// Render full content message for full screen popup
    /// Shows the complete tool result without truncation
    pub fn render_full_content_message(tool_call_result: ToolCallResult) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::RenderFullContentMessage(tool_call_result),
            // is_collapsed: Some(true) means it shows in full screen popup only
            is_collapsed: Some(true),
        }
    }

    /// Create a unified run command block message
    /// Shows command, state indicator, and optional result in one bordered box
    pub fn render_run_command_block(
        command: String,
        result: Option<String>,
        state: crate::services::bash_block::RunCommandState,
        message_id: Option<Uuid>,
    ) -> Self {
        Message {
            id: message_id.unwrap_or_else(Uuid::new_v4),
            content: MessageContent::RenderRunCommandBlock(command, result, state),
            is_collapsed: None,
        }
    }

    pub fn render_ask_user_block(
        questions: Vec<stakpak_shared::models::integrations::openai::AskUserQuestion>,
        answers: std::collections::HashMap<
            String,
            stakpak_shared::models::integrations::openai::AskUserAnswer,
        >,
        current_tab: usize,
        selected_option: usize,
        custom_input: String,
        focused: bool,
        message_id: Option<Uuid>,
    ) -> Self {
        Message {
            id: message_id.unwrap_or_else(Uuid::new_v4),
            content: MessageContent::RenderAskUserBlock {
                questions,
                answers,
                current_tab,
                selected_option,
                custom_input,
                focused,
            },
            is_collapsed: None,
        }
    }

    /// Create a view file block message
    /// Shows a compact display with file icon, \"View\", file path, line count, and optional grep/glob
    pub fn render_view_file_block(
        file_path: String,
        total_lines: usize,
        grep: Option<String>,
        glob: Option<String>,
    ) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::RenderViewFileBlock(file_path, total_lines, grep, glob),
            is_collapsed: None,
        }
    }

    /// Create a view file block message for the full screen popup (no borders)
    /// Shows a compact display with file icon, \"View\", file path, line count, and optional grep/glob
    pub fn render_view_file_block_popup(
        file_path: String,
        total_lines: usize,
        grep: Option<String>,
        glob: Option<String>,
    ) -> Self {
        Message {
            id: Uuid::new_v4(),
            content: MessageContent::RenderViewFileBlock(file_path, total_lines, grep, glob),
            // is_collapsed: Some(true) means it shows in full screen popup only
            is_collapsed: Some(true),
        }
    }

    /// Create a task wait block message
    /// Shows progress of background tasks being waited on with status indicators
    pub fn render_task_wait_block(
        task_updates: Vec<stakpak_shared::models::integrations::openai::TaskUpdate>,
        progress: f64,
        target_task_ids: Vec<String>,
        message_id: Option<Uuid>,
    ) -> Self {
        Message {
            id: message_id.unwrap_or_else(Uuid::new_v4),
            content: MessageContent::RenderTaskWaitBlock(task_updates, progress, target_task_ids),
            is_collapsed: None,
        }
    }

    /// Create a subagent resume pending block message
    /// Shows what the subagent wants to do (pending tool calls)
    pub fn render_subagent_resume_pending_block(
        tool_call: ToolCall,
        is_auto_approved: bool,
        pause_info: Option<stakpak_shared::models::integrations::openai::TaskPauseInfo>,
        message_id: Option<Uuid>,
    ) -> Self {
        Message {
            id: message_id.unwrap_or_else(Uuid::new_v4),
            content: MessageContent::RenderSubagentResumePendingBlock(
                tool_call,
                is_auto_approved,
                pause_info,
            ),
            is_collapsed: None,
        }
    }

    /// Create a tool call streaming preview block
    /// Shows tools being generated by the LLM with token counters
    pub fn render_tool_call_stream_block(
        infos: Vec<ToolCallStreamInfo>,
        message_id: Option<Uuid>,
    ) -> Self {
        Message {
            id: message_id.unwrap_or_else(Uuid::new_v4),
            content: MessageContent::RenderToolCallStreamBlock(infos),
            is_collapsed: None,
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

pub fn get_wrapped_markdown_lines(markdown: &str, width: usize) -> Vec<(Line<'_>, Style)> {
    let mut result = Vec::new();
    let rendered_lines = render_markdown_to_lines_with_width(markdown, width).unwrap_or_default();
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
    let inner_width = border_width.saturating_sub(4); // Account for "│ " and " │"
    let horizontal = "─".repeat(border_width - 2);

    // Top border
    lines.push(Line::from(vec![Span::styled(
        format!("╭{}╮", horizontal),
        Style::default().fg(Color::Magenta),
    )]));

    // Command line - truncate if too long
    let cmd_line = format!("{}{}", SHELL_PROMPT_PREFIX, &command[1..].trim());
    let cmd_display_width = unicode_width::UnicodeWidthStr::width(cmd_line.as_str());
    let truncated_cmd = if cmd_display_width > inner_width {
        // Truncate command
        let mut truncated = String::new();
        let mut char_width = 0;
        for ch in cmd_line.chars() {
            let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
            if char_width + ch_width <= inner_width {
                truncated.push(ch);
                char_width += ch_width;
            } else {
                break;
            }
        }
        truncated
    } else {
        cmd_line.clone()
    };
    let cmd_content_width = unicode_width::UnicodeWidthStr::width(truncated_cmd.as_str());
    let cmd_padding = inner_width.saturating_sub(cmd_content_width);
    lines.push(Line::from(vec![
        Span::styled("│ ", Style::default().fg(Color::Magenta)),
        Span::styled(
            truncated_cmd,
            Style::default().fg(term_color(Color::LightYellow)),
        ),
        Span::from(" ".repeat(cmd_padding)),
        Span::styled(" │", Style::default().fg(Color::Magenta)),
    ]));

    // Output lines - truncate if too long
    for out in output_lines {
        let out_display_width = unicode_width::UnicodeWidthStr::width(out.as_str());
        let truncated_out = if out_display_width > inner_width {
            // Truncate output line
            let mut truncated = String::new();
            let mut char_width = 0;
            for ch in out.chars() {
                let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
                if char_width + ch_width <= inner_width {
                    truncated.push(ch);
                    char_width += ch_width;
                } else {
                    break;
                }
            }
            truncated
        } else {
            out.clone()
        };
        let content_width = unicode_width::UnicodeWidthStr::width(truncated_out.as_str());
        let padding = inner_width.saturating_sub(content_width);
        lines.push(Line::from(vec![
            Span::styled("│ ", Style::default().fg(Color::Magenta)),
            Span::styled(truncated_out, Style::default().fg(AdaptiveColors::text())),
            Span::from(" ".repeat(padding)),
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

#[allow(dead_code)]
pub fn get_wrapped_message_lines(
    messages: &[Message],
    width: usize,
) -> Vec<(Line<'static>, Style)> {
    get_wrapped_message_lines_internal(messages, width, false)
}

/// Compute a cache key that uniquely identifies the current message state.
/// This key changes when:
/// - Width changes
/// - Shell popup visibility changes  
/// - Side panel visibility changes
/// - Messages are added, removed, or resumed (via message count and last message ID)
fn compute_cache_key(state: &AppState, width: usize) -> u64 {
    let mut hasher = DefaultHasher::new();

    // Include width
    width.hash(&mut hasher);

    // Include visibility states
    state.shell_popup_visible.hash(&mut hasher);
    state.show_side_panel.hash(&mut hasher);

    // Include message count (filters out collapsed messages)
    let visible_messages: Vec<&Message> = state
        .messages
        .iter()
        .filter(|m| m.is_collapsed.is_none())
        .collect();
    visible_messages.len().hash(&mut hasher);

    // Include last message ID to detect content changes at the end (streaming)
    if let Some(last_msg) = visible_messages.last() {
        last_msg.id.hash(&mut hasher);
    }

    // Include first message ID to detect changes at the beginning (resume)
    if let Some(first_msg) = visible_messages.first() {
        first_msg.id.hash(&mut hasher);
    }

    hasher.finish()
}

/// Get the total number of cached lines without cloning.
/// This is useful for scroll calculations where we only need the count.
#[allow(dead_code)]
pub fn get_cached_line_count(state: &AppState, width: usize) -> Option<usize> {
    let cache_key = compute_cache_key(state, width);

    if let Some((cached_key, ref cached_lines, _)) = state.assembled_lines_cache
        && cached_key == cache_key
    {
        return Some(cached_lines.len());
    }
    None
}

/// Get visible lines with aggressive caching.
/// NOTE: This function is no longer used since Ratatui requires owned data.
/// Use `get_visible_lines_owned` instead.
#[allow(dead_code)]
pub fn get_visible_lines_arc(
    state: &mut AppState,
    width: usize,
    start: usize,
    count: usize,
) -> Arc<Vec<Line<'static>>> {
    // Ensure assembled cache is populated first
    ensure_cache_populated(state, width);

    let generation = state.cache_generation;

    // FAST PATH: Check if visible lines cache is still valid
    if let Some(ref cache) = state.visible_lines_cache
        && cache.scroll == start
        && cache.width == width
        && cache.height == count
        && cache.source_generation == generation
    {
        // Perfect cache hit - return Arc without any cloning
        return cache.lines.clone();
    }

    // MEDIUM PATH: Assembled cache is valid, just need to slice
    let visible = if let Some((_, ref cached_lines, _)) = state.assembled_lines_cache {
        let end = (start + count).min(cached_lines.len());
        let mut visible = Vec::with_capacity(count);
        for line in cached_lines.iter().take(end).skip(start) {
            visible.push(line.clone());
        }
        // Pad with empty lines if needed
        while visible.len() < count {
            visible.push(Line::from(""));
        }
        visible
    } else {
        vec![Line::from(""); count]
    };

    let arc_visible = Arc::new(visible);

    // Update visible lines cache
    state.visible_lines_cache = Some(crate::app::VisibleLinesCache {
        scroll: start,
        width,
        height: count,
        lines: arc_visible.clone(),
        source_generation: generation,
    });

    arc_visible
}

/// Get visible lines as owned Vec, optimized to avoid unnecessary cloning.
/// NOTE: This function is deprecated - prefer using get_wrapped_message_lines_cached
/// with Paragraph::scroll() for better performance.
#[allow(dead_code)]
pub fn get_visible_lines_owned(
    state: &mut AppState,
    width: usize,
    start: usize,
    count: usize,
) -> Vec<Line<'static>> {
    // Ensure assembled cache is populated first
    ensure_cache_populated(state, width);

    // Slice from assembled cache
    if let Some((_, ref cached_lines, _)) = state.assembled_lines_cache {
        let end = (start + count).min(cached_lines.len());
        let mut visible = Vec::with_capacity(count);
        for line in cached_lines.iter().take(end).skip(start) {
            visible.push(line.clone());
        }
        // Pad with empty lines if needed
        while visible.len() < count {
            visible.push(Line::from(""));
        }
        visible
    } else {
        vec![Line::from(""); count]
    }
}

/// Ensure the cache is populated without returning the lines.
/// This is more efficient when you just need to ensure the cache exists.
#[allow(dead_code)]
fn ensure_cache_populated(state: &mut AppState, width: usize) {
    let cache_key = compute_cache_key(state, width);

    if let Some((cached_key, _, _)) = &state.assembled_lines_cache
        && *cached_key == cache_key
    {
        return; // Cache is valid
    }

    // Cache miss - need to rebuild
    let _ = get_wrapped_message_lines_cached(state, width);
}

/// Main cached message rendering function with per-message caching.
/// This function uses a two-level caching strategy:
/// 1. Per-message cache: Each message's rendered lines are cached individually
/// 2. Assembled cache: The final combined output is cached for fast returns
///
/// When a single message changes (e.g., during streaming), only that message
/// is re-rendered, and the assembled output is rebuilt from cached parts.
///
/// NOTE: Prefer using `get_visible_lines_cached` when you only need a slice,
/// as it avoids cloning the entire vector.
pub fn get_wrapped_message_lines_cached(state: &mut AppState, width: usize) -> Vec<Line<'static>> {
    // FAST PATH: If assembled cache exists and key matches, return it immediately.
    // The cache key is a hash that includes width, visibility states, message count,
    // and first/last message IDs to detect changes from resume, streaming, etc.
    let cache_key = compute_cache_key(state, width);

    if let Some((cached_key, cached_lines, _)) = &state.assembled_lines_cache
        && *cached_key == cache_key
    {
        // Cache hit - return immediately without any processing
        return cached_lines.clone();
    }

    // SLOW PATH: Need to rebuild (cache was invalidated or width changed)
    let render_start = Instant::now();
    let mut cache_hits = 0usize;
    let mut cache_misses = 0usize;

    // Filter messages based on shell popup visibility
    let message_refs: Vec<&Message> = if state.shell_popup_visible {
        state
            .messages
            .iter()
            .filter(|m| !matches!(&m.content, MessageContent::RenderRefreshedTerminal(..)))
            .filter(|m| m.is_collapsed.is_none())
            .collect()
    } else {
        state
            .messages
            .iter()
            .filter(|m| m.is_collapsed.is_none())
            .collect()
    };

    // Pre-allocate with estimated capacity
    let estimated_lines = message_refs.len() * 10; // Rough estimate of 10 lines per message
    let mut all_processed_lines: Vec<Line<'static>> = Vec::with_capacity(estimated_lines);

    // Build line-to-message mapping for click detection
    // Format: (start_line, end_line, message_id, is_user_message, message_text, user_message_index)
    let mut line_to_message_map: Vec<(usize, usize, Uuid, bool, String, usize)> = Vec::new();
    let mut user_message_counter: usize = 0;

    // Process each message, using cache when available
    for msg in &message_refs {
        let content_hash = hash_message_content(&msg.content);
        let start_line = all_processed_lines.len();

        // Check if this is a user message and extract text
        let (is_user_message, message_text) = match &msg.content {
            MessageContent::UserMessage(text) => {
                user_message_counter += 1;
                (true, text.clone())
            }
            _ => (false, String::new()),
        };

        // Check if we have a valid cached render for this message
        if let Some(cached) = state.per_message_cache.get(&msg.id)
            && cached.width == width
            && cached.content_hash == content_hash
        {
            // Cache hit! Reuse rendered lines
            cache_hits += 1;
            all_processed_lines.extend(cached.rendered_lines.iter().cloned());
        } else {
            // Cache miss - render this single message
            cache_misses += 1;
            let rendered_lines = render_single_message(msg, width);

            // Store in per-message cache
            state.per_message_cache.insert(
                msg.id,
                RenderedMessageCache {
                    content_hash,
                    rendered_lines: Arc::new(rendered_lines.clone()),
                    width,
                },
            );

            all_processed_lines.extend(rendered_lines);
        }

        let end_line = all_processed_lines.len();

        // Only track user messages in the map (for efficiency)
        if is_user_message && end_line > start_line {
            line_to_message_map.push((
                start_line,
                end_line,
                msg.id,
                true,
                message_text,
                user_message_counter,
            ));
        }
    }

    // Collapse consecutive empty lines (max 2 consecutive empty lines)
    // Also build a mapping from old line index to new line index
    let mut collapsed_lines: Vec<Line<'static>> = Vec::with_capacity(all_processed_lines.len());
    let mut old_to_new_index: Vec<Option<usize>> = Vec::with_capacity(all_processed_lines.len());
    let mut consecutive_empty = 0;

    for line in all_processed_lines {
        let is_empty = line.spans.is_empty()
            || (line.spans.len() == 1 && line.spans[0].content.trim().is_empty());
        if is_empty {
            consecutive_empty += 1;
            if consecutive_empty <= 2 {
                old_to_new_index.push(Some(collapsed_lines.len()));
                collapsed_lines.push(line);
            } else {
                old_to_new_index.push(None); // This line was removed
            }
        } else {
            consecutive_empty = 0;
            old_to_new_index.push(Some(collapsed_lines.len()));
            collapsed_lines.push(line);
        }
    }
    let mut all_processed_lines = collapsed_lines;

    // Adjust line_to_message_map indices based on collapsed lines
    let adjusted_line_to_message_map: Vec<(usize, usize, Uuid, bool, String, usize)> =
        line_to_message_map
            .into_iter()
            .filter_map(|(start, end, id, is_user, text, user_idx)| {
                // Find the new start index (first non-None mapping at or after old start)
                let new_start =
                    (start..end).find_map(|i| old_to_new_index.get(i).and_then(|&idx| idx))?;

                // Find the new end index (last non-None mapping before old end, +1)
                let new_end = (start..end)
                    .rev()
                    .find_map(|i| old_to_new_index.get(i).and_then(|&idx| idx))
                    .map(|i| i + 1)?;

                if new_end > new_start {
                    Some((new_start, new_end, id, is_user, text, user_idx))
                } else {
                    None
                }
            })
            .collect();

    let line_to_message_map = adjusted_line_to_message_map;

    // Add trailing empty lines if we have content
    if !all_processed_lines.is_empty() {
        all_processed_lines.push(Line::from(""));
        all_processed_lines.push(Line::from(""));
    }

    // Increment generation counter and update the assembled cache
    state.cache_generation = state.cache_generation.wrapping_add(1);
    state.assembled_lines_cache = Some((
        cache_key,
        all_processed_lines.clone(),
        state.cache_generation,
    ));
    // Invalidate visible lines cache since source changed
    state.visible_lines_cache = None;
    state.last_render_width = width;

    // Update line-to-message map for click detection
    state.line_to_message_map = line_to_message_map.clone();

    // Record performance metrics
    let render_time_us = render_start.elapsed().as_micros() as u64;
    state.render_metrics.record_render(
        render_time_us,
        cache_hits,
        cache_misses,
        all_processed_lines.len(),
    );

    all_processed_lines
}

/// Render a single message to lines.
/// This is extracted to allow per-message caching.
fn render_single_message(msg: &Message, width: usize) -> Vec<Line<'static>> {
    use crate::services::message_pattern::spans_to_string;

    // Render the message using the internal function
    let raw_lines = render_single_message_internal(msg, width);

    // Post-process: filter checkpoint lines and handle spacing markers
    let mut processed: Vec<Line<'static>> = Vec::with_capacity(raw_lines.len());

    for (line, _style) in raw_lines {
        let line_text = spans_to_string(&line);

        // Skip checkpoint_id lines entirely
        if line_text.contains("<checkpoint_id>") {
            continue;
        }

        // Convert spacing markers to empty lines
        if line_text.trim() == "SPACING_MARKER" {
            processed.push(Line::from(""));
        } else {
            processed.push(line);
        }
    }

    processed
}

/// Internal function to render a single message to raw lines.
/// This matches the logic from get_wrapped_message_lines_internal but for a single message.
fn render_single_message_internal(msg: &Message, width: usize) -> Vec<(Line<'static>, Style)> {
    let mut lines: Vec<(Line<'static>, Style)> = Vec::new();

    match &msg.content {
        MessageContent::AssistantMD(text, style) => {
            let mut cleaned = text.to_string();

            // Strip markdown delimiters first (for session resume)
            cleaned = strip_markdown_delimiters(&cleaned);

            // Remove agent_mode tags
            if let Some(start) = cleaned.find("<agent_mode>")
                && let Some(end) = cleaned.find("</agent_mode>")
            {
                cleaned.replace_range(start..end + "</agent_mode>".len(), "");
            }

            // Remove checkpoint_id tags and surrounding newlines
            if let Some(start) = cleaned.find("<checkpoint_id>")
                && let Some(end) = cleaned.find("</checkpoint_id>")
            {
                let before_checkpoint = &cleaned[..start];
                let after_checkpoint = &cleaned[end + "</checkpoint_id>".len()..];

                let cleaned_before = if let Some(stripped) = before_checkpoint.strip_suffix('\n') {
                    stripped
                } else {
                    before_checkpoint
                };

                cleaned = format!("{}{}", cleaned_before, after_checkpoint);
            }

            let borrowed_lines =
                render_markdown_to_lines_with_width(&cleaned, width).unwrap_or_default();
            for line in borrowed_lines {
                lines.push((convert_line_to_owned(line), *style));
            }
            // Add extra space after assistant message
            lines.push((
                Line::from(vec![Span::from("SPACING_MARKER")]),
                Style::default(),
            ));
        }
        MessageContent::Plain(text, style) => {
            let mut cleaned = text.to_string();

            // Strip local_context blocks
            while let Some(start) = cleaned.find("<local_context>") {
                if let Some(end) = cleaned[start..].find("</local_context>") {
                    let end_pos = start + end + "</local_context>".len();
                    cleaned.replace_range(start..end_pos, "");
                } else {
                    break;
                }
            }

            // Strip rulebooks blocks
            while let Some(start) = cleaned.find("<rulebooks>") {
                if let Some(end) = cleaned[start..].find("</rulebooks>") {
                    let end_pos = start + end + "</rulebooks>".len();
                    cleaned.replace_range(start..end_pos, "");
                } else {
                    break;
                }
            }

            // Handle shell history
            if cleaned.contains("Here's my shell history:") && cleaned.contains("```shell") {
                let shell_lines = render_shell_history_lines(&cleaned, style, width);
                lines.extend(shell_lines);
            } else if cleaned.contains("\n\n") {
                // Handle double newlines
                for (i, section) in cleaned.split("\n\n").enumerate() {
                    if i > 0 {
                        lines.push((
                            Line::from(vec![Span::from("SPACING_MARKER")]),
                            Style::default(),
                        ));
                    }
                    for line in section.split('\n') {
                        let borrowed = get_wrapped_plain_lines(line, style, width);
                        lines.extend(convert_to_owned_lines(borrowed));
                    }
                }
            } else if cleaned.contains('\n') {
                // Handle single newlines
                for line in cleaned.split('\n') {
                    let borrowed = get_wrapped_plain_lines(line, style, width);
                    lines.extend(convert_to_owned_lines(borrowed));
                }
            } else {
                let borrowed = get_wrapped_plain_lines(text, style, width);
                lines.extend(convert_to_owned_lines(borrowed));
            }
        }
        MessageContent::Styled(line) => {
            let borrowed = get_wrapped_styled_lines(line, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::StyledBlock(block_lines) => {
            let borrowed = get_wrapped_styled_block_lines(block_lines, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::RenderPendingBorderBlock(tool_call, is_auto_approved) => {
            let full_command = extract_full_command_arguments(tool_call);
            let tool_name = strip_tool_name(&tool_call.function.name);
            let rendered = if (tool_name == "str_replace" || tool_name == "create")
                && !render_file_diff(tool_call, width).is_empty()
            {
                render_file_diff(tool_call, width)
            } else {
                render_bash_block(tool_call, &full_command, false, width, *is_auto_approved)
            };
            let borrowed = get_wrapped_styled_block_lines(&rendered, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::RenderPendingBorderBlockWithStallWarning(
            tool_call,
            is_auto_approved,
            stall_message,
        ) => {
            let full_command = extract_full_command_arguments(tool_call);
            let mut rendered =
                render_bash_block(tool_call, &full_command, false, width, *is_auto_approved);

            // Insert stall warning
            if rendered.len() >= 2 {
                let insert_pos = rendered.len() - 2;
                let inner_width = if width > 4 { width - 4 } else { 40 };
                let border_color = Color::Cyan;

                let warning_text = stall_message;
                let warning_display_width =
                    unicode_width::UnicodeWidthStr::width(warning_text.as_str());
                let warning_padding = inner_width.saturating_sub(warning_display_width + 1);
                let warning_line = Line::from(vec![
                    Span::styled("│", Style::default().fg(border_color)),
                    Span::styled(
                        format!("  {}{}", warning_text, " ".repeat(warning_padding)),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" │", Style::default().fg(border_color)),
                ]);

                rendered.insert(insert_pos, warning_line);
            }

            let borrowed = get_wrapped_styled_block_lines(&rendered, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::RenderCollapsedMessage(tool_call) => {
            let tool_name = strip_tool_name(&tool_call.function.name);
            if (tool_name == "str_replace" || tool_name == "create")
                && let Some(rendered) = render_file_diff_full(tool_call, width, Some(true), None)
                && !rendered.is_empty()
            {
                let borrowed = get_wrapped_styled_block_lines(&rendered, width);
                lines.extend(convert_to_owned_lines(borrowed));
            }
        }
        MessageContent::RenderCommandCollapsedResult(tool_call_result) => {
            let content_lines = format_text_content(&tool_call_result.result.clone(), width);
            let rendered = render_collapsed_command_message(tool_call_result, content_lines, width);
            let borrowed = get_wrapped_styled_block_lines(&rendered, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::RenderStreamingBorderBlock(
            content,
            _outside_title,
            bubble_title,
            colors,
            _tool_type,
        ) => {
            let rendered =
                render_streaming_block_compact(content, bubble_title, colors.clone(), width);
            let borrowed = get_wrapped_styled_block_lines(&rendered, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::RenderResultBorderBlock(tool_call_result) => {
            let rendered = render_result_block(tool_call_result, width);
            let borrowed = get_wrapped_styled_block_lines(&rendered, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::RenderFullContentMessage(tool_call_result) => {
            let tool_name =
                stakpak_shared::utils::strip_tool_name(&tool_call_result.call.function.name);

            // For str_replace/create, use the diff view with proper line numbers
            if (tool_name == "str_replace" || tool_name == "create")
                && let Some(rendered) = render_file_diff_full(
                    &tool_call_result.call,
                    width,
                    Some(true),
                    Some(&tool_call_result.result),
                )
                && !rendered.is_empty()
            {
                let borrowed = get_wrapped_styled_block_lines(&rendered, width);
                lines.extend(convert_to_owned_lines(borrowed));
            } else {
                // For other tools, show the raw result
                let title = get_command_type_name(&tool_call_result.call);
                let command_args =
                    extract_truncated_command_arguments(&tool_call_result.call, None);
                let result = &tool_call_result.result;

                let spacing_marker = Line::from(vec![Span::from("SPACING_MARKER")]);
                lines.push((spacing_marker.clone(), Style::default()));

                let dot_color = if tool_call_result.status == ToolCallResultStatus::Success {
                    Color::LightGreen
                } else {
                    Color::Red
                };

                let message_color = if tool_call_result.status == ToolCallResultStatus::Success {
                    AdaptiveColors::text()
                } else {
                    Color::Red
                };

                let header_lines =
                    crate::services::bash_block::render_styled_header_with_dot_public(
                        &title,
                        &command_args,
                        Some(crate::services::bash_block::LinesColors {
                            dot: dot_color,
                            title: Color::White,
                            command: AdaptiveColors::text(),
                            message: message_color,
                        }),
                        Some(width),
                    );
                for line in header_lines {
                    lines.push((convert_line_to_owned(line), Style::default()));
                }

                lines.push((spacing_marker.clone(), Style::default()));

                let content_lines = format_text_content(result, width);
                for line in content_lines {
                    lines.push((line, Style::default()));
                }

                lines.push((spacing_marker, Style::default()));
            }
        }
        MessageContent::RenderEscapedTextBlock(content) => {
            let rendered = format_text_content(content, width);
            let borrowed = get_wrapped_styled_block_lines(&rendered, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::Markdown(markdown) => {
            let borrowed = get_wrapped_markdown_lines(markdown, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::PlainText(text) => {
            let owned_line = Line::from(vec![Span::styled(text.clone(), Style::default())]);
            lines.push((owned_line, Style::default()));
        }
        MessageContent::UserMessage(text) => {
            // Render user message with cyan bar prefix and proper word wrapping
            let rendered = render_user_message_lines(text, width);
            let total_lines = rendered.len();
            const MAX_PREVIEW_LINES: usize = 15;

            if total_lines > MAX_PREVIEW_LINES {
                lines.extend(rendered.into_iter().take(MAX_PREVIEW_LINES));
                // Add collapsed hint line
                let hidden = total_lines - MAX_PREVIEW_LINES;
                let accent_color = Color::DarkGray;
                lines.push((
                    Line::from(vec![
                        Span::styled("┃ ".to_string(), Style::default().fg(accent_color)),
                        Span::styled(
                            format!("... {} more lines (ctrl+t to expand)", hidden),
                            Style::default()
                                .fg(accent_color)
                                .add_modifier(Modifier::ITALIC),
                        ),
                    ]),
                    Style::default(),
                ));
            } else {
                lines.extend(rendered);
            }
        }
        MessageContent::BashBubble {
            title,
            content,
            colors,
            tool_type: _,
        } => {
            let borrowed = get_wrapped_bash_bubble_lines(title, content, colors);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::RenderRefreshedTerminal(title, content, colors, _stored_width) => {
            let rendered = crate::services::bash_block::render_refreshed_terminal_bubble(
                title,
                content,
                colors.clone(),
                width,
            );
            let borrowed = get_wrapped_styled_block_lines(&rendered, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::RenderRunCommandBlock(command, result, state) => {
            let rendered = crate::services::bash_block::render_run_command_block(
                command,
                result.as_deref(),
                state.clone(),
                width,
            );
            let borrowed = get_wrapped_styled_block_lines(&rendered, width);
            lines.push((
                Line::from(vec![Span::from("SPACING_MARKER")]),
                Style::default(),
            ));
            lines.extend(convert_to_owned_lines(borrowed));
            // Add spacing after run command block
            lines.push((
                Line::from(vec![Span::from("SPACING_MARKER")]),
                Style::default(),
            ));
        }
        MessageContent::RenderViewFileBlock(file_path, total_lines, grep, glob) => {
            // Use no-border version for popup (is_collapsed: Some(true))
            let rendered = if msg.is_collapsed == Some(true) {
                crate::services::bash_block::render_view_file_block_no_border(
                    file_path,
                    *total_lines,
                    width,
                    grep.as_deref(),
                    glob.as_deref(),
                )
            } else {
                crate::services::bash_block::render_view_file_block(
                    file_path,
                    *total_lines,
                    width,
                    grep.as_deref(),
                    glob.as_deref(),
                )
            };
            let borrowed = get_wrapped_styled_block_lines(&rendered, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::RenderTaskWaitBlock(task_updates, progress, target_task_ids) => {
            let rendered = crate::services::bash_block::render_task_wait_block(
                task_updates,
                *progress,
                target_task_ids,
                width,
            );
            let borrowed = get_wrapped_styled_block_lines(&rendered, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::RenderSubagentResumePendingBlock(
            tool_call,
            is_auto_approved,
            pause_info,
        ) => {
            let rendered = crate::services::bash_block::render_subagent_resume_pending_block(
                tool_call,
                *is_auto_approved,
                pause_info.as_ref(),
                width,
            );
            let borrowed = get_wrapped_styled_block_lines(&rendered, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::RenderToolCallStreamBlock(infos) => {
            let rendered = crate::services::bash_block::render_tool_call_stream_block(infos, width);
            let borrowed = get_wrapped_styled_block_lines(&rendered, width);
            lines.extend(convert_to_owned_lines(borrowed));
        }
        MessageContent::RenderAskUserBlock {
            questions,
            answers,
            current_tab,
            selected_option,
            custom_input,
            focused,
        } => {
            let rendered = crate::services::bash_block::render_ask_user_block(
                questions,
                answers,
                *current_tab,
                *selected_option,
                custom_input,
                width,
                *focused,
            );
            let borrowed = get_wrapped_styled_block_lines(&rendered, width);
            lines.push((
                Line::from(vec![Span::from("SPACING_MARKER")]),
                Style::default(),
            ));
            lines.extend(convert_to_owned_lines(borrowed));
            lines.push((
                Line::from(vec![Span::from("SPACING_MARKER")]),
                Style::default(),
            ));
        }
    }

    lines
}

/// Helper to render shell history lines (extracted from Plain handling)
fn render_shell_history_lines(
    text: &str,
    style: &Style,
    width: usize,
) -> Vec<(Line<'static>, Style)> {
    let mut lines = Vec::new();
    let mut remaining = text;

    while let Some(start) = remaining.find("```shell") {
        let before = &remaining[..start];
        if !before.trim().is_empty() {
            let borrowed = get_wrapped_plain_lines(before, style, width);
            lines.extend(convert_to_owned_lines(borrowed));
            lines.push((
                Line::from(vec![Span::from("SPACING_MARKER")]),
                Style::default(),
            ));
        }

        let after_start = &remaining[start + "```shell".len()..];
        if let Some(end) = after_start.find("```") {
            let shell_block = &after_start[..end];
            let mut bubble_lines = Vec::new();
            let mut current_command: Option<String> = None;
            let mut current_output = Vec::new();

            for line in shell_block.lines() {
                if line.trim().starts_with(SHELL_PROMPT_PREFIX.trim()) {
                    if let Some(cmd) = current_command.take() {
                        bubble_lines.push(render_shell_bubble_with_unicode_border(
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
                bubble_lines.push(render_shell_bubble_with_unicode_border(
                    &cmd,
                    &current_output,
                    width,
                ));
            }

            for bubble in bubble_lines {
                for l in bubble {
                    lines.push((convert_line_to_owned(l), Style::default()));
                }
            }

            remaining = &after_start[end + "```".len()..];
            lines.push((
                Line::from(vec![Span::from("SPACING_MARKER")]),
                Style::default(),
            ));
        } else {
            if !after_start.trim().is_empty() {
                let borrowed = get_wrapped_plain_lines(after_start, style, width);
                lines.extend(convert_to_owned_lines(borrowed));
            }
            break;
        }
    }

    if !remaining.trim().is_empty() && !remaining.contains("```shell") {
        let borrowed = get_wrapped_plain_lines(remaining, style, width);
        lines.extend(convert_to_owned_lines(borrowed));
    }

    lines
}

/// Legacy function for backwards compatibility.
/// New code should use get_wrapped_message_lines_cached with AppState.
#[allow(dead_code)]
pub fn get_processed_message_lines(messages: &[Message], width: usize) -> Vec<Line<'static>> {
    use crate::services::message_pattern::spans_to_string;

    let all_lines: Vec<(Line, Style)> = get_wrapped_message_lines(messages, width);

    let estimated_capacity = all_lines.len() + (all_lines.len() / 10);
    let mut processed_lines: Vec<Line> = Vec::with_capacity(estimated_capacity);

    for (line, _style) in all_lines.iter() {
        let line_text = spans_to_string(line);
        if line_text.contains("<checkpoint_id>") {
            continue;
        } else if line_text.trim() == "SPACING_MARKER" {
            processed_lines.push(Line::from(""));
        } else {
            processed_lines.push(line.clone());
        }
    }

    processed_lines
}

/// Invalidate the message lines cache when messages change.
/// Smart invalidation: Skip when user has scrolled up to avoid jitter during streaming.
///
/// With per-message caching, this only invalidates the assembled cache.
/// Individual message caches remain valid and will be reused if content hasn't changed.
pub fn invalidate_message_lines_cache(state: &mut AppState) {
    // If user has scrolled up (reading old messages), don't invalidate cache
    // This prevents jitter when new streaming chunks arrive while scrolled up
    if !state.stay_at_bottom && state.is_streaming {
        return;
    }

    // Invalidate both assembled and visible caches
    state.assembled_lines_cache = None;
    state.visible_lines_cache = None;

    // Legacy cache invalidation for backwards compatibility
    state.message_lines_cache = None;
    state.collapsed_message_lines_cache = None;
}

/// Invalidate cache for a specific message (e.g., during streaming).
/// This is more efficient than invalidating the entire cache.
pub fn invalidate_message_cache(state: &mut AppState, message_id: Uuid) {
    // Remove the specific message from per-message cache
    state.per_message_cache.remove(&message_id);
    // Invalidate assembled and visible caches since they need rebuilding
    state.assembled_lines_cache = None;
    state.visible_lines_cache = None;
}

/// Clean up stale entries from the per-message cache.
/// Call this periodically or when messages are removed.
#[allow(dead_code)]
pub fn cleanup_message_cache(state: &mut AppState) {
    let valid_ids: std::collections::HashSet<Uuid> = state.messages.iter().map(|m| m.id).collect();
    state
        .per_message_cache
        .retain(|id, _| valid_ids.contains(id));
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
                    render_markdown_to_lines_with_width(&cleaned.to_string(), width)
                        .unwrap_or_default();
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
                let tool_name = strip_tool_name(&tool_call.function.name);
                let rendered_lines = if (tool_name == "str_replace" || tool_name == "create")
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

            MessageContent::RenderPendingBorderBlockWithStallWarning(
                tool_call,
                is_auto_approved,
                stall_message,
            ) => {
                let full_command = extract_full_command_arguments(tool_call);
                let mut rendered_lines =
                    render_bash_block(tool_call, &full_command, false, width, *is_auto_approved);

                // Insert stall warning inside the block (before the bottom border)
                // Find the bottom border line (last line before SPACING_MARKER)
                if rendered_lines.len() >= 2 {
                    let insert_pos = rendered_lines.len() - 2; // Before bottom border and SPACING_MARKER
                    let inner_width = if width > 4 { width - 4 } else { 40 };
                    let border_color = Color::Cyan;

                    // Add warning line inside the block (use simple ASCII to avoid width issues)
                    let warning_text = stall_message;
                    let warning_display_width =
                        unicode_width::UnicodeWidthStr::width(warning_text.as_str());
                    let warning_padding = inner_width.saturating_sub(warning_display_width + 1); // +4 for "  " prefix and " │" suffix spacing
                    let warning_line = Line::from(vec![
                        Span::styled("│", Style::default().fg(border_color)),
                        Span::styled(
                            format!("  {}{}", warning_text, " ".repeat(warning_padding)),
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" │", Style::default().fg(border_color)),
                    ]);

                    rendered_lines.insert(insert_pos, warning_line);
                }

                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }

            MessageContent::RenderCollapsedMessage(tool_call) => {
                let tool_name = strip_tool_name(&tool_call.function.name);
                if (tool_name == "str_replace" || tool_name == "create")
                    && let Some(rendered_lines) =
                        render_file_diff_full(tool_call, width, Some(true), None)
                    && !rendered_lines.is_empty()
                {
                    let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                    let owned_lines = convert_to_owned_lines(borrowed_lines);
                    all_lines.extend(owned_lines);
                }
            }

            MessageContent::RenderCommandCollapsedResult(tool_call_result) => {
                let lines = format_text_content(&tool_call_result.result.clone(), width);
                let rendered_lines =
                    render_collapsed_command_message(tool_call_result, lines, width);
                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }

            MessageContent::RenderStreamingBorderBlock(
                content,
                _outside_title,
                bubble_title,
                colors,
                _tool_type,
            ) => {
                // Use compact streaming renderer that shows only last 3 lines with hint
                let rendered_lines =
                    render_streaming_block_compact(content, bubble_title, colors.clone(), width);
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
            MessageContent::RenderFullContentMessage(tool_call_result) => {
                let tool_name =
                    stakpak_shared::utils::strip_tool_name(&tool_call_result.call.function.name);

                // For str_replace/create, use the diff view with proper line numbers
                if (tool_name == "str_replace" || tool_name == "create")
                    && let Some(rendered_lines) = render_file_diff_full(
                        &tool_call_result.call,
                        width,
                        Some(true),
                        Some(&tool_call_result.result),
                    )
                    && !rendered_lines.is_empty()
                {
                    let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                    let owned_lines = convert_to_owned_lines(borrowed_lines);
                    all_lines.extend(owned_lines);
                } else {
                    // Full content view for popup - shows complete result without truncation
                    let title =
                        crate::services::message::get_command_type_name(&tool_call_result.call);
                    let command_args =
                        extract_truncated_command_arguments(&tool_call_result.call, None);
                    let result = &tool_call_result.result;

                    // Render header with dot
                    let spacing_marker = Line::from(vec![Span::from("SPACING_MARKER")]);
                    all_lines.push((spacing_marker.clone(), Style::default()));

                    let dot_color = if tool_call_result.status == ToolCallResultStatus::Success {
                        Color::LightGreen
                    } else {
                        Color::Red
                    };

                    let message_color = if tool_call_result.status == ToolCallResultStatus::Success
                    {
                        crate::services::detect_term::AdaptiveColors::text()
                    } else {
                        Color::Red
                    };

                    let header_lines =
                        crate::services::bash_block::render_styled_header_with_dot_public(
                            &title,
                            &command_args,
                            Some(crate::services::bash_block::LinesColors {
                                dot: dot_color,
                                title: Color::White,
                                command: crate::services::detect_term::AdaptiveColors::text(),
                                message: message_color,
                            }),
                            Some(width),
                        );
                    for line in header_lines {
                        all_lines.push((convert_line_to_owned(line), Style::default()));
                    }

                    all_lines.push((spacing_marker.clone(), Style::default()));

                    // Render full content
                    let content_lines = format_text_content(result, width);
                    for line in content_lines {
                        all_lines.push((line, Style::default()));
                    }

                    all_lines.push((spacing_marker, Style::default()));
                }
            }
            MessageContent::RenderEscapedTextBlock(content) => {
                let rendered_lines = format_text_content(content, width);
                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::Markdown(markdown) => {
                let borrowed_lines = get_wrapped_markdown_lines(markdown, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::PlainText(text) => {
                let owned_line = Line::from(vec![Span::styled(text.clone(), Style::default())]);
                all_lines.push((owned_line, Style::default()));
            }
            MessageContent::UserMessage(text) => {
                // Render user message with cyan bar prefix and proper word wrapping
                let rendered = render_user_message_lines(text, width);
                all_lines.extend(rendered);
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
            MessageContent::RenderRefreshedTerminal(title, content, colors, _stored_width) => {
                // Use the current terminal width, not the stored width
                let rendered_lines = crate::services::bash_block::render_refreshed_terminal_bubble(
                    title,
                    content,
                    colors.clone(),
                    width,
                );
                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::RenderRunCommandBlock(command, result, state) => {
                let rendered_lines = crate::services::bash_block::render_run_command_block(
                    command,
                    result.as_deref(),
                    state.clone(),
                    width,
                );
                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::RenderViewFileBlock(file_path, total_lines, grep, glob) => {
                // Use no-border version for popup (is_collapsed: Some(true))
                let rendered_lines = if msg.is_collapsed == Some(true) {
                    crate::services::bash_block::render_view_file_block_no_border(
                        file_path,
                        *total_lines,
                        width,
                        grep.as_deref(),
                        glob.as_deref(),
                    )
                } else {
                    crate::services::bash_block::render_view_file_block(
                        file_path,
                        *total_lines,
                        width,
                        grep.as_deref(),
                        glob.as_deref(),
                    )
                };
                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::RenderTaskWaitBlock(task_updates, progress, target_task_ids) => {
                let rendered_lines = crate::services::bash_block::render_task_wait_block(
                    task_updates,
                    *progress,
                    target_task_ids,
                    width,
                );
                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::RenderSubagentResumePendingBlock(
                tool_call,
                is_auto_approved,
                pause_info,
            ) => {
                let rendered_lines =
                    crate::services::bash_block::render_subagent_resume_pending_block(
                        tool_call,
                        *is_auto_approved,
                        pause_info.as_ref(),
                        width,
                    );
                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::RenderToolCallStreamBlock(infos) => {
                let rendered_lines =
                    crate::services::bash_block::render_tool_call_stream_block(infos, width);
                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
                let owned_lines = convert_to_owned_lines(borrowed_lines);
                all_lines.extend(owned_lines);
            }
            MessageContent::RenderAskUserBlock {
                questions,
                answers,
                current_tab,
                selected_option,
                custom_input,
                focused,
            } => {
                let rendered_lines = crate::services::bash_block::render_ask_user_block(
                    questions,
                    answers,
                    *current_tab,
                    *selected_option,
                    custom_input,
                    width,
                    *focused,
                );
                let borrowed_lines = get_wrapped_styled_block_lines(&rendered_lines, width);
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

    // For subagent tasks, show description + tools summary instead of raw args
    let tool_name = strip_tool_name(&tool_call.function.name);
    if tool_name == "dynamic_subagent_task"
        && let Ok(ref args) = arguments
    {
        let desc = args
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("subagent");
        let tools_count = args
            .get("tools")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let is_sandbox = args
            .get("enable_sandbox")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let sandbox_tag = if is_sandbox { " [sandboxed]" } else { "" };
        return format!("{}{} ({} tools)", desc, sandbox_tag, tools_count);
    }

    // For ask_user, show question labels
    if tool_name == "ask_user"
        && let Ok(request) = serde_json::from_str::<
            stakpak_shared::models::integrations::openai::AskUserRequest,
        >(&tool_call.function.arguments)
    {
        let labels: Vec<&str> = request.questions.iter().map(|q| q.label.as_str()).collect();
        if !labels.is_empty() {
            return format!("questions: {}", labels.join(", "));
        }
        return format!("{} question(s)", request.questions.len());
    }

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
    match strip_tool_name(&tool_call.function.name) {
        "create_file" => "Create file".to_string(),
        "create" => "Create".to_string(),
        "edit_file" => "Edit file".to_string(),
        "str_replace" => "Str Replace".to_string(),
        "run_command" => "Run command".to_string(),
        "read_file" => "Read file".to_string(),
        "delete_file" => "Delete file".to_string(),
        "list_directory" => "List directory".to_string(),
        "search_files" => "Search files".to_string(),
        "dynamic_subagent_task" => {
            let args =
                serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments).ok();
            let desc = args
                .as_ref()
                .and_then(|a| a.get("description").and_then(|v| v.as_str()))
                .unwrap_or("Subagent");
            let is_sandbox = args
                .as_ref()
                .and_then(|a| a.get("enable_sandbox").and_then(|v| v.as_bool()))
                .unwrap_or(false);
            if is_sandbox {
                format!("Subagent [sandboxed]: {}", desc)
            } else {
                format!("Subagent: {}", desc)
            }
        }
        "ask_user" => "Ask User".to_string(),
        _ => {
            // Convert function name to title case
            strip_tool_name(&tool_call.function.name)
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
                metadata: None,
            };

            let result = extract_full_command_arguments(&tool_call);
            println!(
                "Input: '{}' -> Output: '{}' (Expected: '{}')",
                input, result, expected
            );
        }
    }
}
