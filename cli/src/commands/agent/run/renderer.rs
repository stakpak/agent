use crossterm::style::{Color, Stylize};
use serde_json::Value;
use stakpak_api::storage::{SessionStats, ToolUsageStats};
use stakpak_shared::models::{integrations::openai::ChatMessage, llm::LLMTokenUsage};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Json,
    Text,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Text => write!(f, "text"),
        }
    }
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(OutputFormat::Json),
            "text" => Ok(OutputFormat::Text),
            _ => Err(format!(
                "Invalid output format: {}. Valid values are 'json' or 'text'",
                s
            )),
        }
    }
}

pub struct OutputRenderer {
    format: OutputFormat,
    verbose: bool,
}

impl OutputRenderer {
    pub fn new(format: OutputFormat, verbose: bool) -> Self {
        Self { format, verbose }
    }

    // Generic rendering functions

    pub fn render_title(&self, title: &str) -> String {
        match (&self.format, self.verbose) {
            (OutputFormat::Text, true) => {
                format!(
                    "╭─────────────────────────────────────────────────────────────────────────────────╮\n│ {:<79} │\n╰─────────────────────────────────────────────────────────────────────────────────╯\n",
                    title
                )
            }
            _ => String::new(),
        }
    }

    pub fn render_step_header(&self, step: usize, tool_count: usize) -> String {
        match (&self.format, self.verbose) {
            (OutputFormat::Text, true) => {
                let header_text = if tool_count > 0 {
                    format!(
                        "Step {} - Executing {} tool{}",
                        step,
                        tool_count,
                        if tool_count == 1 { "" } else { "s" }
                    )
                } else {
                    format!("Step {} - Agent response", step)
                };

                format!(
                    "\n{}\n{}\n",
                    header_text,
                    "─".repeat(header_text.chars().count())
                )
            }
            _ => String::new(),
        }
    }

    pub fn render_section_break(&self) -> String {
        match (&self.format, self.verbose) {
            (OutputFormat::Text, true) => "\n".to_string(),
            _ => String::new(),
        }
    }

    pub fn render_assistant_message(&self, content: &str, is_final: bool) -> String {
        match (&self.format, self.verbose) {
            (OutputFormat::Text, true) => {
                let formatted_content = self.format_xml_tags_as_boxes(content);

                if is_final {
                    format!(
                        "┌─ Final Agent Response ──────────────────────────────────────────────────────────\n{}\n└─────────────────────────────────────────────────────────────────────────────────",
                        formatted_content
                            .lines()
                            .map(|line| format!("│ {}", line))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                } else {
                    let mut output = String::new();
                    output.push_str("Agent Response:\n");

                    if self.verbose {
                        // Show full response
                        for line in formatted_content.lines() {
                            output.push_str(&format!("  {}\n", line));
                        }
                    } else {
                        // Show truncated response - first 3 lines max
                        let lines: Vec<&str> = formatted_content.lines().collect();
                        let display_lines = if lines.len() > 3 { 3 } else { lines.len() };

                        for line in lines.iter().take(display_lines) {
                            let truncated_line = if line.chars().count() > 80 {
                                let truncated: String = line.chars().take(80).collect();
                                format!("{}...", truncated)
                            } else {
                                line.to_string()
                            };
                            output.push_str(&format!("  {}\n", truncated_line));
                        }

                        if lines.len() > 3 {
                            output.push_str(&format!("  ... ({} more lines)\n", lines.len() - 3));
                        }
                    }
                    output
                }
            }
            _ => String::new(),
        }
    }

    pub fn render_tool_execution(
        &self,
        tool_name: &str,
        tool_params: &str,
        tool_index: usize,
        total_tools: usize,
    ) -> String {
        match (&self.format, self.verbose) {
            (OutputFormat::Text, true) => {
                let mut output =
                    format!("Tool {}/{}: {}\n", tool_index + 1, total_tools, tool_name);

                if !tool_params.trim().is_empty() {
                    if let Ok(params_json) = serde_json::from_str::<Value>(tool_params) {
                        let truncated_params = truncate_yaml_value(&params_json, 200);
                        if let Ok(pretty_json) = serde_json::to_string_pretty(&truncated_params) {
                            output.push_str("  Arguments:\n");
                            for line in pretty_json.lines() {
                                output.push_str(&format!("    {}\n", line));
                            }
                        } else {
                            output.push_str(&format!("  Arguments: {}\n", tool_params));
                        }
                    } else {
                        output.push_str(&format!("  Arguments: {}\n", tool_params));
                    }
                }
                output
            }
            _ => String::new(),
        }
    }

    pub fn render_tool_result(&self, result: &str) -> String {
        match (&self.format, self.verbose) {
            (OutputFormat::Text, true) => {
                let mut output = String::from("  Result:\n");

                if self.verbose {
                    for line in result.lines() {
                        output.push_str(&format!("    {}\n", line));
                    }
                    output.push('\n'); // Add blank line after verbose tool output
                } else {
                    // Show truncated result
                    let first_line = result.lines().next().unwrap_or("").trim();
                    if !first_line.is_empty() {
                        let truncated = if first_line.chars().count() > 80 {
                            let truncated_chars: String = first_line.chars().take(80).collect();
                            format!("{}...", truncated_chars)
                        } else {
                            first_line.to_string()
                        };
                        output.push_str(&format!("    {}\n", truncated));
                    }
                }
                output
            }
            _ => String::new(),
        }
    }

    pub fn render_info(&self, message: &str) -> String {
        match (&self.format, self.verbose) {
            (OutputFormat::Text, true) => format!("[info] {}\n", message),
            _ => String::new(),
        }
    }

    pub fn render_success(&self, message: &str) -> String {
        match (&self.format, self.verbose) {
            (OutputFormat::Text, true) => format!("[success] {}\n", message),
            _ => String::new(),
        }
    }

    pub fn render_warning(&self, message: &str) -> String {
        match self.format {
            OutputFormat::Json => String::new(),
            OutputFormat::Text => format!("[warning] {}\n", message),
        }
    }

    pub fn render_error(&self, message: &str) -> String {
        match self.format {
            OutputFormat::Json => String::new(),
            OutputFormat::Text => format!("[error] {}\n", message),
        }
    }

    pub fn render_stat_line(&self, label: &str, value: &str) -> String {
        match self.format {
            OutputFormat::Json => String::new(),
            OutputFormat::Text => self.render_info(&format!("{}: {}", label, value)),
        }
    }

    pub fn render_final_completion(&self, messages: &[ChatMessage]) -> String {
        match self.format {
            OutputFormat::Json => {
                if self.verbose {
                    serde_json::to_string_pretty(messages).unwrap_or_default()
                } else {
                    // Find the last assistant message
                    let final_message = messages.iter().rev().find(|m| {
                        m.role == stakpak_shared::models::integrations::openai::Role::Assistant
                    });

                    if let Some(message) = final_message {
                        serde_json::to_string_pretty(message).unwrap_or_default()
                    } else {
                        "{}".to_string()
                    }
                }
            }
            OutputFormat::Text => {
                // if self.verbose {
                let mut output = String::new();

                // Show final assistant message
                if let Some(final_message) = messages.iter().rev().find(|m| {
                    m.role == stakpak_shared::models::integrations::openai::Role::Assistant
                }) && let Some(content) = &final_message.content
                {
                    let content_str = self.extract_content_string(content);
                    if !content_str.trim().is_empty() {
                        output.push_str(&self.format_final_assistant_message(&content_str));
                    }
                }
                output
                // } else {
                //     // Show just the final assistant message content
                //     if let Some(final_message) = messages.iter().rev().find(|m| {
                //         m.role == stakpak_shared::models::integrations::openai::Role::Assistant
                //     }) {
                //         if let Some(content) = &final_message.content {
                //             let content_str = self.extract_content_string(content);
                //             if !content_str.trim().is_empty() {
                //                 return content_str;
                //             }
                //         }
                //     }
                //     String::new()
                // }
            }
        }
    }

    fn extract_content_string(
        &self,
        content: &stakpak_shared::models::integrations::openai::MessageContent,
    ) -> String {
        match content {
            stakpak_shared::models::integrations::openai::MessageContent::String(s) => s.clone(),
            stakpak_shared::models::integrations::openai::MessageContent::Array(parts) => parts
                .iter()
                .filter_map(|part| part.text.as_ref())
                .map(|text| text.as_str())
                .filter(|text| !text.starts_with("<checkpoint_id>"))
                .collect::<Vec<&str>>()
                .join("\n"),
        }
    }

    fn format_final_assistant_message(&self, content: &str) -> String {
        let formatted_content = self.format_xml_tags_as_boxes(content);
        format!(
            "┌─ Final Agent Response ──────────────────────────────────────────────────────────\n{}\n└─────────────────────────────────────────────────────────────────────────────────\n",
            formatted_content
                .lines()
                .map(|line| format!("│ {}", line))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }

    fn format_xml_tags_as_boxes(&self, content: &str) -> String {
        let mut result = content.to_string();

        // Handle common XML tags manually to avoid regex dependency
        let tags_to_handle = ["reasoning", "report", "todo"];

        for tag in &tags_to_handle {
            let start_tag = format!("<{}>", tag);
            let end_tag = format!("</{}>", tag);

            while let Some(start_pos) = result.find(&start_tag) {
                if let Some(relative_end_pos) = result[start_pos..].find(&end_tag) {
                    let actual_end_pos = start_pos + relative_end_pos;
                    let content_start = start_pos + start_tag.len();
                    let tag_content = result[content_start..actual_end_pos].trim();
                    let full_end_pos = actual_end_pos + end_tag.len();

                    if !tag_content.is_empty() {
                        // Wrap content to 76 characters (leaving room for "│ " prefix)
                        let wrapped_lines = self.wrap_text(tag_content, 76);

                        // Calculate the actual box width needed
                        let max_content_width = wrapped_lines
                            .iter()
                            .map(|line| line.chars().count())
                            .max()
                            .unwrap_or(0);

                        // Calculate minimum width needed for header: "┌─ tag ─┐"
                        let tag_name = tag;
                        let min_header_width = tag_name.chars().count() + 7; // "┌─ " + tag + " ─┐"
                        let content_width = max_content_width + 4; // "│ " + content + " │"
                        let box_width = std::cmp::max(min_header_width, content_width).max(20);

                        // Create header: ┌─ TAG ─────────────┐
                        let tag_part = format!("─ {} ", tag_name);
                        let remaining_width = box_width - 2; // Subtract "┌" and "┐"
                        let padding_width =
                            remaining_width.saturating_sub(tag_part.chars().count());
                        let header = format!("┌{}{}┐", tag_part, "─".repeat(padding_width));
                        let footer = format!("└{}┘", "─".repeat(box_width - 2));

                        // Format content lines with proper padding
                        let content_lines = wrapped_lines
                            .iter()
                            .map(|line| {
                                let padding =
                                    " ".repeat(box_width.saturating_sub(line.chars().count() + 4));
                                format!("│ {}{} │", line, padding)
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        let box_content = format!("{}\n{}\n{}", header, content_lines, footer);

                        // Replace by reconstructing the string with before + replacement + after
                        let before = &result[..start_pos];
                        let after = &result[full_end_pos..];
                        result = format!("{}{}{}", before, box_content, after);
                    } else {
                        // If empty content, just remove the tags by reconstructing without them
                        let before = &result[..start_pos];
                        let after = &result[full_end_pos..];
                        result = format!("{}{}", before, after);
                    }
                } else {
                    break; // No matching end tag found
                }
            }
        }

        result
    }

    fn wrap_text(&self, text: &str, max_width: usize) -> Vec<String> {
        let mut wrapped_lines = Vec::new();

        for line in text.lines() {
            if line.trim().is_empty() {
                wrapped_lines.push(String::new());
                continue;
            }

            if line.chars().count() <= max_width {
                wrapped_lines.push(line.to_string());
            } else {
                let mut current_line = String::new();
                for word in line.split_whitespace() {
                    if current_line.is_empty() {
                        current_line = word.to_string();
                    } else if current_line.chars().count() + 1 + word.chars().count() <= max_width {
                        current_line.push(' ');
                        current_line.push_str(word);
                    } else {
                        wrapped_lines.push(current_line);
                        current_line = word.to_string();
                    }
                }
                if !current_line.is_empty() {
                    wrapped_lines.push(current_line);
                }
            }
        }

        wrapped_lines
    }

    pub fn render_session_stats(&self, stats: &SessionStats) -> String {
        match &self.format {
            OutputFormat::Json => {
                serde_json::to_string_pretty(stats).unwrap_or_else(|_| "{}".to_string())
            }
            OutputFormat::Text => {
                if let Some(total_time_saved) = stats.total_time_saved_seconds {
                    self.render_time_saved_stats(total_time_saved, &stats.tools_usage)
                } else {
                    String::new()
                }
            }
        }
    }

    fn render_time_saved_stats(&self, total_seconds: u32, tool_stats: &[ToolUsageStats]) -> String {
        if total_seconds == 0 {
            return String::new();
        }

        let mut output = String::new();

        // Convert seconds to minutes for display
        let total_minutes = total_seconds / 60;
        let remaining_seconds = total_seconds % 60;

        // Main ROI-focused header
        output.push_str(&format!("\n{}\n", "━".repeat(50).with(Color::Cyan)));

        if total_minutes >= 60 {
            let hours = total_minutes / 60;
            let mins = total_minutes % 60;
            output.push_str(&format!(
                "{}\n\n",
                format!("You just saved {}h {}m of work!", hours, mins)
                    .with(Color::Cyan)
                    .bold()
            ));
        } else {
            output.push_str(&format!(
                "{}\n\n",
                format!(
                    "You just saved {}m {}s of work!",
                    total_minutes, remaining_seconds
                )
                .with(Color::Cyan)
                .bold()
            ));
        }

        // Filter tools that actually saved time and sort by time saved
        let mut time_saving_tools: Vec<_> = tool_stats
            .iter()
            .filter(|tool| tool.time_saved_seconds.unwrap_or(0) > 0)
            .collect();

        time_saving_tools.sort_by(|a, b| {
            b.time_saved_seconds
                .unwrap_or(0)
                .cmp(&a.time_saved_seconds.unwrap_or(0))
        });

        if !time_saving_tools.is_empty() {
            output.push_str(&format!(
                "{}\n",
                "Top time savers:".with(Color::Cyan).bold()
            ));

            for (i, tool) in time_saving_tools.iter().take(3).enumerate() {
                let saved_seconds = tool.time_saved_seconds.unwrap_or(0);
                let saved_minutes = saved_seconds / 60;
                let remaining_secs = saved_seconds % 60;

                let time_display = if saved_minutes > 0 {
                    format!("{}m {}s", saved_minutes, remaining_secs)
                } else {
                    format!("{}s", remaining_secs)
                };

                let bullet = match i {
                    0 => "▶".with(Color::Cyan),
                    1 => "▶".with(Color::Cyan),
                    2 => "▶".with(Color::Cyan),
                    _ => "▷".with(Color::DarkGrey),
                };

                output.push_str(&format!(
                    "  {} {} - {}\n",
                    bullet,
                    tool.display_name.clone().with(Color::White),
                    time_display.with(Color::Cyan)
                ));
            }

            output.push('\n');
        }

        // Motivational ROI message
        let daily_estimate = (total_seconds as f64 / 3600.0) * 3.0; // Assume 3 sessions per day
        let weekly_estimate = daily_estimate * 5.0;

        if weekly_estimate >= 1.0 {
            output.push_str(&format!(
                "At this pace, you could save {} per week!\n",
                format!("{}h", (weekly_estimate as u32))
                    .to_string()
                    .with(Color::Magenta)
                    .bold()
            ));
        } else {
            let weekly_minutes = (weekly_estimate * 60.0) as u32;
            output.push_str(&format!(
                "At this pace, you could save {} per week!\n",
                format!("{}m", weekly_minutes).with(Color::Magenta).bold()
            ));
        }

        output.push_str(&format!("{}\n\n", "━".repeat(50).with(Color::Cyan)));

        output
    }
}

fn truncate_yaml_value(value: &Value, max_length: usize) -> Value {
    match value {
        Value::String(s) => {
            if s.chars().count() > max_length {
                let truncated: String = s.chars().take(max_length).collect();
                Value::String(format!(
                    "{}... [truncated - {} chars total]",
                    truncated,
                    s.chars().count()
                ))
            } else {
                value.clone()
            }
        }
        Value::Array(arr) => {
            if arr.len() > 5 {
                let mut truncated = arr.iter().take(5).cloned().collect::<Vec<_>>();
                truncated.push(Value::String(format!(
                    "... [truncated - {} items total]",
                    arr.len()
                )));
                Value::Array(truncated)
            } else {
                Value::Array(
                    arr.iter()
                        .map(|v| truncate_yaml_value(v, max_length))
                        .collect(),
                )
            }
        }
        Value::Object(obj) => {
            let mut truncated_obj = serde_json::Map::new();
            for (k, v) in obj {
                truncated_obj.insert(k.clone(), truncate_yaml_value(v, max_length));
            }
            Value::Object(truncated_obj)
        }
        _ => value.clone(),
    }
}

impl OutputRenderer {
    pub fn render_token_usage_stats(&self, usage: &LLMTokenUsage) -> String {
        match &self.format {
            OutputFormat::Json => {
                serde_json::to_string_pretty(usage).unwrap_or_else(|_| "{}".to_string())
            }
            OutputFormat::Text => {
                let mut output = String::new();

                // Header (no border, no gap - flows directly after time saved stats)
                output.push_str(&format!("{}\n\n", "Session Usage".with(Color::Cyan).bold()));

                // Format numbers with thousands separator
                let format_num = |n: u32| {
                    let s = n.to_string();
                    let mut result = String::new();
                    for (count, c) in s.chars().rev().enumerate() {
                        if count > 0 && count % 3 == 0 {
                            result.push(',');
                        }
                        result.push(c);
                    }
                    result.chars().rev().collect::<String>()
                };

                // Manually format each line with fixed spacing to align all numbers (no colons)
                output.push_str(&format!(
                    " Prompt tokens      {}\n", // 6 spaces to align numbers
                    format_num(usage.prompt_tokens).with(Color::Yellow).bold()
                ));

                // Show prompt token details if available
                if let Some(details) = &usage.prompt_tokens_details {
                    // Always show fields except output_tokens (redundant), using 0 if None, with fixed spacing
                    output.push_str(&format!(
                        "  ├─ Input tokens   {}\n", // 3 spaces to align numbers
                        format_num(details.input_tokens.unwrap_or(0)).with(Color::DarkGrey)
                    ));
                    output.push_str(&format!(
                        "  ├─ Cache write    {}\n", // 4 spaces to align numbers
                        format_num(details.cache_write_input_tokens.unwrap_or(0))
                            .with(Color::DarkGrey)
                    ));
                    output.push_str(&format!(
                        "  └─ Cache read     {}\n", // 5 spaces to align numbers
                        format_num(details.cache_read_input_tokens.unwrap_or(0))
                            .with(Color::DarkGrey)
                    ));
                }

                output.push_str(&format!(
                    " Completion tokens  {}\n", // 2 spaces to align numbers
                    format_num(usage.completion_tokens)
                        .with(Color::Yellow)
                        .bold()
                ));
                output.push_str(&format!(
                    " Total tokens       {}\n", // 7 spaces to align numbers
                    format_num(usage.total_tokens).with(Color::Green).bold()
                ));

                output
            }
        }
    }
}
