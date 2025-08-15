use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use regex::Regex;
use std::time::Instant;

use crate::services::syntax_highlighter;

// Simplified component enum with all the variants you mentioned
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum MarkdownComponent {
    H1(String),
    H2(String),
    H3(String),
    H4(String),
    H5(String),
    H6(String),
    Bold(String),
    Italic(String),
    BoldItalic(String),
    Strikethrough(String),
    Code(String),
    Link {
        text: String,
        url: String,
    },
    Image {
        alt: String,
        url: String,
    },
    UnorderedList(Vec<MarkdownComponent>),
    OrderedList(Vec<MarkdownComponent>),
    ListItem(String),
    Paragraph(String),
    CodeBlock {
        language: Option<String>,
        content: String,
    },
    Quote(String),
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Important(String),
    Note(String),
    Tip(String),
    Warning(String),
    Caution(String),
    TaskOpen(String),
    TaskComplete(String),
    HorizontalSeparator,
    PlainText(String),
    Word(String),
    EmptyLine,
    MixedContent(Vec<Span<'static>>),
}

#[derive(Clone)]
pub struct MarkdownStyle {
    pub h1_style: Style,
    pub h2_style: Style,
    pub h3_style: Style,
    pub h4_style: Style,
    pub h5_style: Style,
    pub h6_style: Style,
    pub bold_style: Style,
    pub italic_style: Style,
    pub bold_italic_style: Style,
    pub strikethrough_style: Style,
    pub code_style: Style,
    pub code_block_style: Style,
    pub link_style: Style,
    pub quote_style: Style,
    pub list_bullet_style: Style,
    pub task_open_style: Style,
    pub task_complete_style: Style,
    pub important_style: Style,
    pub note_style: Style,
    pub tip_style: Style,
    pub warning_style: Style,
    pub caution_style: Style,
    pub text_style: Style,
    pub separator_style: Style,
    pub table_header_style: Style,
    pub table_cell_style: Style,
}

impl Default for MarkdownStyle {
    fn default() -> Self {
        Self {
            h1_style: Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            h2_style: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            h3_style: Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            h4_style: Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
            h5_style: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            h6_style: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            bold_style: Style::default().add_modifier(Modifier::BOLD),
            italic_style: Style::default().add_modifier(Modifier::ITALIC),
            bold_italic_style: Style::default().add_modifier(Modifier::BOLD | Modifier::ITALIC),
            strikethrough_style: Style::default().add_modifier(Modifier::CROSSED_OUT),
            code_style: Style::default()
                .fg(Color::LightRed)
                .bg(Color::Rgb(48, 48, 48)),
            code_block_style: Style::default().fg(Color::Green),
            link_style: Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::UNDERLINED),
            quote_style: Style::default().fg(Color::Gray),
            list_bullet_style: Style::default().fg(Color::Rgb(180, 180, 180)),
            task_open_style: Style::default().fg(Color::Yellow),
            task_complete_style: Style::default().fg(Color::Green),
            important_style: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            note_style: Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            tip_style: Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            warning_style: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            caution_style: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            text_style: Style::default().fg(Color::White),
            separator_style: Style::default().fg(Color::Gray),
            table_header_style: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            table_cell_style: Style::default().fg(Color::White),
        }
    }
}

pub struct MarkdownRenderer {
    pub style: MarkdownStyle,
}

impl MarkdownRenderer {
    pub fn new(style: MarkdownStyle) -> Self {
        Self { style }
    }

    // Improved parser with performance optimizations and limits
    pub fn parse_markdown(
        &self,
        input: &str,
    ) -> Result<Vec<MarkdownComponent>, Box<dyn std::error::Error>> {
        let start = Instant::now();

        // Input validation and limits
        if input.is_empty() {
            return Ok(vec![]);
        }

        if input.len() > 1_000_000 {
            return Err("Markdown content too large (max 1MB)".into());
        }

        // Pre-process problematic patterns
        let cleaned_input = self.preprocess_input(input);

        let mut components = Vec::new();
        let lines: Vec<&str> = cleaned_input.lines().collect();

        // Limit number of lines to prevent infinite processing
        let max_lines = 2000;
        let process_lines = if lines.len() > max_lines {
            &lines[..max_lines]
        } else {
            &lines
        };

        let mut i = 0;
        while i < process_lines.len() {
            let original_line = process_lines[i];

            // Strip line numbers if present (e.g., "1: # Header" -> "# Header")
            let stripped_line = self.strip_line_number(original_line);
            let line = stripped_line.trim();

            // Skip empty lines but add them for spacing
            if line.is_empty() {
                components.push(MarkdownComponent::EmptyLine);
                i += 1;
                continue;
            }

            // Parse different markdown elements with early returns
            if let Some(component) = self.parse_line_optimized(line, process_lines, &mut i) {
                components.push(component);
            }

            i += 1;

            // Yield control every 50 lines to keep UI responsive
            if i % 50 == 0 {
                std::thread::yield_now();

                // Timeout protection
                if start.elapsed().as_secs() > 5 {
                    components.push(MarkdownComponent::Paragraph(
                        "... (content truncated due to timeout)".to_string(),
                    ));
                    break;
                }
            }
        }

        // Performance monitoring removed

        Ok(components)
    }

    // Pre-process input to handle problematic patterns
    fn preprocess_input(&self, input: &str) -> String {
        input
            // Fix complex badge patterns like [![text](img)](link)
            .replace("[![", "🔗 [")
            // Simplify image shields/badges
            .replace("](https://img.shields.io", "](shield")
            // Clean up excessive formatting
            .replace("****", "**")
            // Normalize line endings
            .replace("\r\n", "\n")
            .replace('\r', "\n")
    }

    // Strip line numbers like "1: ", "22: ", "78:", "80:", etc.
    fn strip_line_number(&self, line: &str) -> String {
        let trimmed = line.trim();

        // Look for pattern like "number:" at the beginning (with or without space after colon)
        if let Some(colon_pos) = trimmed.find(':') {
            if colon_pos < 10 {
                // Reasonable limit for line numbers
                let prefix = &trimmed[..colon_pos];
                if prefix.chars().all(|c| c.is_ascii_digit()) && !prefix.is_empty() {
                    // Remove the colon and any following whitespace
                    let after_colon = &trimmed[colon_pos + 1..];
                    return after_colon.trim_start().to_string();
                }
            }
        }

        line.to_string()
    }

    fn parse_line_optimized(
        &self,
        line: &str,
        all_lines: &[&str],
        index: &mut usize,
    ) -> Option<MarkdownComponent> {
        // Early returns for performance
        if line.len() > 10000 {
            return Some(MarkdownComponent::Paragraph(
                "(Line too long, truncated)".to_string(),
            ));
        }

        // Code blocks - handle first to avoid conflicts
        if line.starts_with("```") {
            return self.parse_code_block_safe(all_lines, index);
        }

        // Headings - check BEFORE other patterns to avoid conflicts
        if line.starts_with('#') {
            return self.parse_heading_fast(line);
        }

        // Handle complex badges/links early
        if line.contains("🔗 [") && line.contains("](") {
            return self.parse_simplified_link(line);
        }

        // Images - handle ![alt](url) syntax
        if line.contains("![") && line.contains("](") {
            if let Some((alt, url)) = self.parse_image_safe(line) {
                return Some(MarkdownComponent::Image { alt, url });
            }
        }

        // Links - handle [text](url) syntax
        if line.contains('[') && line.contains("](") && line.contains(')') {
            if let Some((text, url)) = self.parse_link_safe(line) {
                return Some(MarkdownComponent::Link { text, url });
            }
        }

        // Tasks
        if line.starts_with("- [x]") || line.starts_with("- [X]") {
            return Some(MarkdownComponent::TaskComplete(
                line[5..].trim().to_string(),
            ));
        }
        if let Some(stripped) = line.strip_prefix("- [ ]") {
            return Some(MarkdownComponent::TaskOpen(stripped.trim().to_string()));
        }

        // Lists
        if line.starts_with("- ") || line.starts_with("* ") {
            let content = line[2..].trim().to_string();
            return self.parse_list_item_safe(&content);
        }

        // Numbered lists
        if let Some(captures) = self.parse_numbered_list(line) {
            return self.parse_list_item_safe(&captures);
        }

        // Quotes
        if line.starts_with("> ") {
            return self.parse_quote_safe(line, all_lines, index);
        }

        // Horizontal separator
        if line.starts_with("---") || line.starts_with("***") || line.starts_with("___") {
            return Some(MarkdownComponent::HorizontalSeparator);
        }

        // Callouts
        if let Some(callout) = self.parse_callout(line) {
            return Some(callout);
        }

        // Check for inline formatting (optimized)
        if self.has_inline_formatting_fast(line) {
            return Some(self.parse_inline_formatting_safe(line));
        }

        // Default to paragraph
        Some(MarkdownComponent::Paragraph(line.to_string()))
    }

    fn parse_heading_fast(&self, line: &str) -> Option<MarkdownComponent> {
        let hash_count = line.chars().take_while(|&c| c == '#').count();
        if hash_count == 0 || hash_count > 6 {
            return None;
        }

        let content = if line.len() > hash_count {
            let after_hashes = &line[hash_count..];
            if let Some(stripped) = after_hashes.strip_prefix(' ') {
                stripped.trim()
            } else {
                after_hashes.trim()
            }
        } else {
            ""
        };

        match hash_count {
            1 => Some(MarkdownComponent::H1(content.to_string())),
            2 => Some(MarkdownComponent::H2(content.to_string())),
            3 => Some(MarkdownComponent::H3(content.to_string())),
            4 => Some(MarkdownComponent::H4(content.to_string())),
            5 => Some(MarkdownComponent::H5(content.to_string())),
            6 => Some(MarkdownComponent::H6(content.to_string())),
            _ => None,
        }
    }

    fn parse_simplified_link(&self, line: &str) -> Option<MarkdownComponent> {
        // Handle simplified links from badges
        if let Some(start) = line.find("🔗 [") {
            if let Some(middle) = line[start..].find("](") {
                if let Some(end) = line[start + middle + 2..].find(')') {
                    let text_part = &line[start + 4..start + middle];
                    let url_start = start + middle + 2;
                    let url_part = &line[url_start..url_start + end];
                    return Some(MarkdownComponent::Link {
                        text: text_part.to_string(),
                        url: url_part.to_string(),
                    });
                }
            }
        }
        None
    }

    fn parse_code_block_safe(
        &self,
        all_lines: &[&str],
        index: &mut usize,
    ) -> Option<MarkdownComponent> {
        let stripped_start_line = self.strip_line_number(all_lines[*index]);
        let start_line = stripped_start_line.trim();
        let language = if start_line.len() > 3 {
            let lang = start_line[3..].trim();
            if lang.is_empty() {
                None
            } else {
                Some(lang.to_string())
            }
        } else {
            None
        };

        let mut code_lines = Vec::new();
        let mut j = *index + 1;
        let max_code_lines = 500; // Limit code block size

        // Collect lines until closing ``` or limit reached
        while j < all_lines.len() && code_lines.len() < max_code_lines {
            let code_line = self.strip_line_number(all_lines[j]);
            if code_line.trim().starts_with("```") {
                break;
            }
            code_lines.push(code_line);
            j += 1;
        }

        *index = j; // Skip to closing ```

        let content = if code_lines.len() >= max_code_lines {
            let mut truncated = code_lines.join("\n");
            truncated.push_str("\n... (code block truncated)");
            truncated
        } else {
            code_lines.join("\n")
        };

        Some(MarkdownComponent::CodeBlock { language, content })
    }

    fn parse_list_item_safe(&self, content: &str) -> Option<MarkdownComponent> {
        // Simplified list item parsing to avoid infinite loops
        if content.len() > 1000 {
            return Some(MarkdownComponent::ListItem(
                "(List item too long)".to_string(),
            ));
        }

        if self.has_simple_formatting(content) {
            let inline_component = self.parse_inline_formatting_safe(content);
            match inline_component {
                MarkdownComponent::MixedContent(spans) => {
                    let mut list_spans = vec![Span::styled("• ", self.style.list_bullet_style)];
                    list_spans.extend(spans);
                    return Some(MarkdownComponent::MixedContent(list_spans));
                }
                _ => {
                    return Some(MarkdownComponent::ListItem(content.to_string()));
                }
            }
        }

        Some(MarkdownComponent::ListItem(content.to_string()))
    }

    fn parse_quote_safe(
        &self,
        line: &str,
        all_lines: &[&str],
        index: &mut usize,
    ) -> Option<MarkdownComponent> {
        let mut quote_lines = vec![line[2..].trim().to_string()];
        let mut j = *index + 1;
        let max_quote_lines = 50; // Limit quote block size

        // Collect consecutive quote lines
        while j < all_lines.len() && quote_lines.len() < max_quote_lines {
            let stripped_quote_line = self.strip_line_number(all_lines[j]);
            let quote_line = stripped_quote_line.trim();
            if let Some(stripped) = quote_line.strip_prefix("> ") {
                quote_lines.push(stripped.trim().to_string());
                j += 1;
            } else {
                break;
            }
        }

        *index = j - 1; // Adjust index to skip processed lines
        Some(MarkdownComponent::Quote(quote_lines.join(" ")))
    }

    fn parse_callout(&self, line: &str) -> Option<MarkdownComponent> {
        let lower_line = line.to_lowercase();
        if lower_line.contains("[!important]") {
            return Some(MarkdownComponent::Important(
                line.replace("[!important]", "")
                    .replace("[!IMPORTANT]", "")
                    .trim()
                    .to_string(),
            ));
        }
        if lower_line.contains("[!note]") {
            return Some(MarkdownComponent::Note(
                line.replace("[!note]", "")
                    .replace("[!NOTE]", "")
                    .trim()
                    .to_string(),
            ));
        }
        if lower_line.contains("[!tip]") {
            return Some(MarkdownComponent::Tip(
                line.replace("[!tip]", "")
                    .replace("[!TIP]", "")
                    .trim()
                    .to_string(),
            ));
        }
        if lower_line.contains("[!warning]") {
            return Some(MarkdownComponent::Warning(
                line.replace("[!warning]", "")
                    .replace("[!WARNING]", "")
                    .trim()
                    .to_string(),
            ));
        }
        if lower_line.contains("[!caution]") {
            return Some(MarkdownComponent::Caution(
                line.replace("[!caution]", "")
                    .replace("[!CAUTION]", "")
                    .trim()
                    .to_string(),
            ));
        }
        None
    }

    fn parse_numbered_list(&self, line: &str) -> Option<String> {
        // Match patterns like "1. ", "2. ", etc.
        let trimmed = line.trim();
        if let Some(dot_pos) = trimmed.find(". ") {
            if dot_pos < 5 {
                // Reasonable limit for list numbers
                let prefix = &trimmed[..dot_pos];
                if prefix.chars().all(|c| c.is_ascii_digit()) && !prefix.is_empty() {
                    return Some(trimmed[dot_pos + 2..].to_string());
                }
            }
        }
        None
    }

    // Optimized inline formatting detection
    fn has_inline_formatting_fast(&self, line: &str) -> bool {
        if line.len() < 3 || line.len() > 5000 {
            return false;
        }

        // Quick pattern matching without complex loops
        let has_bold = line.contains("**") && line.matches("**").count() >= 2;
        let has_italic =
            line.contains('*') && !line.contains("**") && line.matches('*').count() >= 2;
        let has_code =
            line.contains('`') && line.matches('`').count() >= 2 && line.matches('`').count() <= 10;
        let has_strikethrough = line.contains("~~") && line.matches("~~").count() >= 2;
        let has_links = line.contains('[') && line.contains("](");

        has_bold || has_italic || has_code || has_strikethrough || has_links
    }

    // Simplified check for basic formatting
    fn has_simple_formatting(&self, line: &str) -> bool {
        line.len() < 1000 && (line.contains("**") || line.contains('`') || line.contains('['))
    }

    fn parse_inline_formatting_safe(&self, line: &str) -> MarkdownComponent {
        if line.len() > 2000 {
            return MarkdownComponent::Paragraph(line.to_string());
        }

        // Simple pattern replacement for performance
        let mut spans = Vec::new();
        let mut remaining = line.to_string();

        // Handle bold first (greedy matching)
        while let Some(start) = remaining.find("**") {
            // Add text before bold
            if start > 0 {
                spans.push(Span::styled(
                    remaining[..start].to_string(),
                    self.style.text_style,
                ));
            }

            // Find closing **
            if let Some(end) = remaining[start + 2..].find("**") {
                let bold_text = &remaining[start + 2..start + 2 + end];
                spans.push(Span::styled(bold_text.to_string(), self.style.bold_style));
                remaining = remaining[start + 2 + end + 2..].to_string();
            } else {
                // No closing **, treat as regular text
                spans.push(Span::styled(remaining.clone(), self.style.text_style));
                break;
            }
        }

        // Handle inline code (backticks)
        let mut remaining_for_code = remaining.clone();
        while let Some(start) = remaining_for_code.find('`') {
            // Add text before code
            if start > 0 {
                spans.push(Span::styled(
                    remaining_for_code[..start].to_string(),
                    self.style.text_style,
                ));
            }

            // Find closing backtick
            if let Some(end) = remaining_for_code[start + 1..].find('`') {
                let code_text = &remaining_for_code[start + 1..start + 1 + end];
                spans.push(Span::styled(code_text.to_string(), self.style.code_style));
                remaining_for_code = remaining_for_code[start + 1 + end + 1..].to_string();
            } else {
                // No closing backtick, treat as regular text
                spans.push(Span::styled(
                    remaining_for_code.clone(),
                    self.style.text_style,
                ));
                break;
            }
        }

        // Add any remaining text
        if !remaining_for_code.is_empty() {
            spans.push(Span::styled(
                remaining_for_code.clone(),
                self.style.text_style,
            ));
        }

        match spans.len().cmp(&1) {
            std::cmp::Ordering::Greater => MarkdownComponent::MixedContent(spans),
            std::cmp::Ordering::Equal => MarkdownComponent::Paragraph(spans[0].content.to_string()),
            std::cmp::Ordering::Less => MarkdownComponent::Paragraph(line.to_string()),
        }
    }

    fn parse_image_safe(&self, text: &str) -> Option<(String, String)> {
        if text.len() > 500 {
            // Limit to prevent DoS
            return None;
        }

        if let Some(start) = text.find("![") {
            if let Some(middle) = text[start..].find("](") {
                if start + middle < text.len() {
                    let alt_part = &text[start + 2..start + middle];
                    let url_start = start + middle + 2;
                    if let Some(end) = text[url_start..].find(')') {
                        if url_start + end <= text.len() {
                            let url_part = &text[url_start..url_start + end];
                            return Some((alt_part.to_string(), url_part.to_string()));
                        }
                    }
                }
            }
        }
        None
    }

    fn parse_link_safe(&self, text: &str) -> Option<(String, String)> {
        if text.len() > 500 {
            // Limit to prevent DoS
            return None;
        }

        if let Some(start) = text.find('[') {
            if let Some(middle) = text[start..].find("](") {
                if start + middle < text.len() {
                    let text_part = &text[start + 1..start + middle];
                    let url_start = start + middle + 2;
                    if let Some(end) = text[url_start..].find(')') {
                        if url_start + end <= text.len() {
                            let url_part = &text[url_start..url_start + end];
                            return Some((text_part.to_string(), url_part.to_string()));
                        }
                    }
                }
            }
        }
        None
    }

    // Convert components to styled lines for ratatui with limits
    pub fn render_to_lines(&self, components: Vec<MarkdownComponent>) -> Vec<Line<'static>> {
        let mut lines = Vec::with_capacity(components.len() * 2);
        let max_lines = 5000; // Prevent memory issues

        for (i, component) in components.into_iter().enumerate() {
            if lines.len() >= max_lines {
                lines.push(Line::from(vec![Span::styled(
                    "... (content truncated for performance)",
                    self.style.text_style,
                )]));
                break;
            }

            lines.extend(self.component_to_lines(component));

            // Yield control periodically
            if i % 100 == 0 {
                std::thread::yield_now();
            }
        }

        lines
    }

    fn component_to_lines(&self, component: MarkdownComponent) -> Vec<Line<'static>> {
        match component {
            MarkdownComponent::H1(text) => {
                vec![Line::from(vec![Span::styled(text, self.style.h1_style)])]
            }
            MarkdownComponent::H2(text) => {
                vec![Line::from(vec![Span::styled(text, self.style.h2_style)])]
            }
            MarkdownComponent::H3(text) => {
                vec![Line::from(vec![Span::styled(text, self.style.h3_style)])]
            }
            MarkdownComponent::H4(text) => {
                vec![Line::from(vec![Span::styled(text, self.style.h4_style)])]
            }
            MarkdownComponent::H5(text) => {
                vec![Line::from(vec![Span::styled(text, self.style.h5_style)])]
            }
            MarkdownComponent::H6(text) => {
                vec![Line::from(vec![Span::styled(text, self.style.h6_style)])]
            }

            MarkdownComponent::Bold(text) => {
                vec![Line::from(vec![Span::styled(text, self.style.bold_style)])]
            }
            MarkdownComponent::Italic(text) => vec![Line::from(vec![Span::styled(
                text,
                self.style.italic_style,
            )])],
            MarkdownComponent::BoldItalic(text) => vec![Line::from(vec![Span::styled(
                text,
                self.style.bold_italic_style,
            )])],
            MarkdownComponent::Strikethrough(text) => vec![Line::from(vec![Span::styled(
                text,
                self.style.strikethrough_style,
            )])],
            MarkdownComponent::Code(text) => {
                vec![Line::from(vec![Span::styled(text, self.style.code_style)])]
            }

            MarkdownComponent::Link { text, url: _ } => vec![Line::from(vec![Span::styled(
                text.to_string(),
                self.style.link_style,
            )])],

            MarkdownComponent::Image { alt, url: _ } => vec![Line::from(vec![Span::styled(
                format!("🖼️ {}", alt),
                self.style.link_style,
            )])],

            MarkdownComponent::UnorderedList(items) => {
                let mut list_lines = Vec::new();
                for item in items.into_iter().take(100) {
                    // Limit list items
                    let item_lines = self.component_to_lines(item);
                    for (i, line) in item_lines.into_iter().enumerate() {
                        if i == 0 {
                            let mut spans = vec![Span::styled("• ", self.style.list_bullet_style)];
                            spans.extend(line.spans);
                            list_lines.push(Line::from(spans));
                        } else {
                            let mut spans = vec![Span::styled("  ", self.style.text_style)];
                            spans.extend(line.spans);
                            list_lines.push(Line::from(spans));
                        }
                    }
                }
                list_lines
            }

            MarkdownComponent::OrderedList(items) => {
                let mut list_lines = Vec::new();
                for (index, item) in items.into_iter().enumerate().take(100) {
                    // Limit list items
                    let item_lines = self.component_to_lines(item);
                    for (i, line) in item_lines.into_iter().enumerate() {
                        if i == 0 {
                            let mut spans = vec![Span::styled(
                                format!("{}. ", index + 1),
                                self.style.list_bullet_style,
                            )];
                            spans.extend(line.spans);
                            list_lines.push(Line::from(spans));
                        } else {
                            let mut spans = vec![Span::styled("   ", self.style.text_style)];
                            spans.extend(line.spans);
                            list_lines.push(Line::from(spans));
                        }
                    }
                }
                list_lines
            }

            MarkdownComponent::ListItem(text) => {
                vec![Line::from(vec![
                    Span::styled("• ", self.style.list_bullet_style),
                    Span::styled(text, self.style.text_style),
                ])]
            }

            MarkdownComponent::CodeBlock { language, content } => {
                let mut code_lines = Vec::new();

                // Limit code block size for performance
                let lines: Vec<&str> = content.lines().take(200).collect();

                // Try to use syntax highlighting if available
                if let Ok(highlighted_lines) = self.try_syntax_highlighting(&content, &language) {
                    code_lines.extend(highlighted_lines);
                } else {
                    // Fallback to simple styling
                    for line in lines {
                        code_lines.push(Line::from(vec![Span::styled(
                            line.to_string(),
                            self.style.code_block_style.bg(Color::Rgb(30, 30, 30)),
                        )]));
                    }
                }

                if content.lines().count() > 200 {
                    code_lines.push(Line::from(vec![Span::styled(
                        "... (code block truncated)",
                        self.style.code_block_style,
                    )]));
                }

                code_lines
            }

            MarkdownComponent::Quote(text) => {
                let mut quote_lines = Vec::new();
                for line in text.lines().take(50) {
                    // Limit quote lines
                    quote_lines.push(Line::from(vec![
                        Span::styled("│ ", self.style.quote_style),
                        Span::styled(line.to_string(), self.style.quote_style),
                    ]));
                }
                quote_lines
            }

            MarkdownComponent::Table { headers, rows } => {
                let mut table_lines = Vec::new();

                // Render headers
                if !headers.is_empty() {
                    let header_line = headers
                        .iter()
                        .take(10) // Limit columns
                        .map(|h| format!("│ {} ", h))
                        .collect::<String>()
                        + "│";
                    table_lines.push(Line::from(vec![Span::styled(
                        header_line,
                        self.style.table_header_style,
                    )]));

                    // Header separator
                    let separator = headers
                        .iter()
                        .take(10)
                        .map(|h| "─".repeat(h.len().min(20) + 2))
                        .collect::<Vec<_>>()
                        .join("┼");
                    table_lines.push(Line::from(vec![Span::styled(
                        format!("├{}┤", separator),
                        self.style.table_header_style,
                    )]));
                }

                // Render rows (limit for performance)
                for row in rows.into_iter().take(50) {
                    let row_line = row
                        .iter()
                        .take(10)
                        .map(|cell| format!("│ {} ", cell))
                        .collect::<String>()
                        + "│";
                    table_lines.push(Line::from(vec![Span::styled(
                        row_line,
                        self.style.table_cell_style,
                    )]));
                }

                table_lines
            }

            MarkdownComponent::TaskOpen(text) => vec![Line::from(vec![
                Span::styled("☐ ", self.style.task_open_style),
                Span::styled(text, self.style.text_style),
            ])],

            MarkdownComponent::TaskComplete(text) => vec![Line::from(vec![
                Span::styled("☑ ", self.style.task_complete_style),
                Span::styled(text, self.style.text_style),
            ])],

            MarkdownComponent::Important(text) => vec![
                Line::from(vec![Span::styled(
                    "🔒 IMPORTANT",
                    self.style.important_style,
                )]),
                Line::from(vec![Span::styled(text, self.style.text_style)]),
            ],

            MarkdownComponent::Note(text) => vec![
                Line::from(vec![Span::styled("📝 NOTE", self.style.note_style)]),
                Line::from(vec![Span::styled(text, self.style.text_style)]),
            ],

            MarkdownComponent::Tip(text) => vec![
                Line::from(vec![Span::styled("💡 TIP", self.style.tip_style)]),
                Line::from(vec![Span::styled(text, self.style.text_style)]),
            ],

            MarkdownComponent::Warning(text) => vec![
                Line::from(vec![Span::styled("⚠️  WARNING", self.style.warning_style)]),
                Line::from(vec![Span::styled(text, self.style.text_style)]),
            ],

            MarkdownComponent::Caution(text) => vec![
                Line::from(vec![Span::styled("⚡ CAUTION", self.style.caution_style)]),
                Line::from(vec![Span::styled(text, self.style.text_style)]),
            ],

            MarkdownComponent::HorizontalSeparator => vec![Line::from(vec![Span::styled(
                "─".repeat(50),
                self.style.separator_style,
            )])],

            MarkdownComponent::Paragraph(text) => {
                vec![Line::from(vec![Span::styled(text, self.style.text_style)])]
            }

            MarkdownComponent::PlainText(text) => {
                vec![Line::from(vec![Span::styled(text, self.style.text_style)])]
            }

            MarkdownComponent::Word(text) => {
                vec![Line::from(vec![Span::styled(text, self.style.text_style)])]
            }

            MarkdownComponent::EmptyLine => vec![Line::from("")],

            MarkdownComponent::MixedContent(spans) => {
                vec![Line::from(spans)]
            }
        }
    }

    // Try to apply syntax highlighting if the feature is available
    fn try_syntax_highlighting(
        &self,
        content: &str,
        language: &Option<String>,
    ) -> Result<Vec<Line<'static>>, Box<dyn std::error::Error>> {
        // This is a placeholder - you can implement syntax highlighting here
        // For now, we'll use a simple fallback

        // If you have the syntax_highlighter module, uncomment this:
        let extension = language
            .as_ref()
            .map(|lang| match lang.to_lowercase().as_str() {
                "rust" | "rs" => "rs",
                "javascript" | "js" => "js",
                "typescript" | "ts" => "ts",
                "python" | "py" => "py",
                "php" => "php",
                "bash" | "sh" | "shell" => "sh",
                "go" => "go",
                "java" => "java",
                "cpp" | "c++" => "cpp",
                "c" => "c",
                "html" => "html",
                "css" => "css",
                "sql" => "sql",
                "yaml" | "yml" => "yml",
                "toml" => "toml",
                "json" => "json",
                "markdown" | "md" => "md",
                _ => "txt",
            });

        Ok(syntax_highlighter::apply_syntax_highlighting(
            content, extension,
        ))
    }
}

// Simple public function for easy use with performance monitoring
pub fn render_markdown_to_lines(
    markdown_content: &str,
) -> Result<Vec<Line<'static>>, Box<dyn std::error::Error>> {
    let parsed_content = xml_tags_to_markdown_headers(markdown_content);

    let style = MarkdownStyle::default();
    let renderer = MarkdownRenderer::new(style);
    let components = renderer.parse_markdown(parsed_content.as_str())?;
    let lines = renderer.render_to_lines(components);
    Ok(lines)
}

fn xml_tags_to_markdown_headers(input: &str) -> String {
    // Use match to handle regex compilation errors gracefully
    let tag_regex = match Regex::new(r"<([a-zA-Z_][a-zA-Z0-9_-]*)[^>]*>") {
        Ok(regex) => regex,
        Err(_) => return input.to_string(), // Return original input if regex fails
    };

    let closing_tag_regex = match Regex::new(r"</([a-zA-Z_][a-zA-Z0-9_-]*)>") {
        Ok(regex) => regex,
        Err(_) => return input.to_string(), // Return original input if regex fails
    };

    let mut result = input.to_string();

    // Replace opening tags with markdown headers (skip checkpoint tags)
    result = tag_regex
        .replace_all(&result, |caps: &regex::Captures| {
            let tag_name = &caps[1];

            // Skip checkpoint tags - leave them untouched
            if tag_name == "checkpoint_id" {
                caps[0].to_string() // Return the original tag unchanged
            } else {
                let formatted_name = format_header_name(tag_name);
                format!("#### {}", formatted_name) // Makes it a level 3 markdown header
            }
        })
        .to_string();

    // Remove closing tags (except checkpoint)
    result = closing_tag_regex
        .replace_all(&result, |caps: &regex::Captures| {
            let tag_name = &caps[1];
            // Skip checkpoint closing tags - leave them untouched
            if tag_name == "checkpoint_id" {
                caps[0].to_string() // Return the original closing tag unchanged
            } else {
                "SPACING_MARKER".to_string() // Remove other closing tags
            }
        })
        .to_string();

    result
}

fn format_header_name(name: &str) -> String {
    name.split('_') // Split on underscores
        .filter(|s| !s.is_empty()) // Remove empty strings
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first
                    .to_uppercase()
                    .chain(chars.as_str().to_lowercase().chars())
                    .collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ") // Join with spaces instead of underscores
}

// Enhanced function with timeout protection
#[allow(dead_code)]
pub fn render_markdown_to_lines_safe(
    markdown_content: &str,
) -> Result<Vec<Line<'static>>, Box<dyn std::error::Error>> {
    // Quick validation
    if markdown_content.is_empty() {
        return Ok(vec![]);
    }

    if markdown_content.len() > 2_000_000 {
        return Err("Markdown content too large (max 2MB)".into());
    }

    // Use a thread with timeout for very large content
    if markdown_content.len() > 100_000 {
        return render_with_timeout(markdown_content);
    }

    render_markdown_to_lines(markdown_content)
}

fn render_with_timeout(
    markdown_content: &str,
) -> Result<Vec<Line<'static>>, Box<dyn std::error::Error>> {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel();
    let content = markdown_content.to_string();

    thread::spawn(move || {
        let result = render_markdown_to_lines(&content);
        let _ = tx.send(result.map_err(|e| e.to_string()));
    });

    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(result) => match result {
            Ok(lines) => Ok(lines),
            Err(e) => Err(e.into()),
        },
        Err(_) => {
            // Timeout - return a simple error message
            Ok(vec![
                Line::from(vec![Span::styled(
                    "⚠️ Markdown rendering timed out",
                    Style::default().fg(Color::Yellow),
                )]),
                Line::from(vec![Span::styled(
                    "Content too complex to render safely",
                    Style::default().fg(Color::Gray),
                )]),
            ])
        }
    }
}
