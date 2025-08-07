use ratatui::{
    style::{Color, Modifier, Style}, text::{Line, Span}
};

// Simplified component enum with all the variants you mentioned
#[derive(Debug, Clone)]
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
    Link { text: String, url: String },
    UnorderedList(Vec<MarkdownComponent>),
    OrderedList(Vec<MarkdownComponent>),
    ListItem(String),
    Paragraph(String),
    CodeBlock { language: Option<String>, content: String },
    Quote(String),
    Table { headers: Vec<String>, rows: Vec<Vec<String>> },
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
            h1_style: Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
            h2_style: Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            h3_style: Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            h4_style: Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            h5_style: Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            h6_style: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            bold_style: Style::default().add_modifier(Modifier::BOLD),
            italic_style: Style::default().add_modifier(Modifier::ITALIC),
            bold_italic_style: Style::default().add_modifier(Modifier::BOLD | Modifier::ITALIC),
            strikethrough_style: Style::default().add_modifier(Modifier::CROSSED_OUT),
            code_style: Style::default().fg(Color::Yellow),
            code_block_style: Style::default().fg(Color::Green),
            link_style: Style::default().fg(Color::Blue).add_modifier(Modifier::UNDERLINED),
            quote_style: Style::default().fg(Color::Gray),
            list_bullet_style: Style::default().fg(Color::Yellow),
            task_open_style: Style::default().fg(Color::Red),
            task_complete_style: Style::default().fg(Color::Green),
            important_style: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            note_style: Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
            tip_style: Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            warning_style: Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            caution_style: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            text_style: Style::default().fg(Color::White),
            separator_style: Style::default().fg(Color::Gray),
            table_header_style: Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
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
    
    // Improved parser that handles line numbers and better markdown detection
    pub fn parse_markdown(&self, input: &str) -> Result<Vec<MarkdownComponent>, Box<dyn std::error::Error>> {
        let mut components = Vec::new();
        let lines: Vec<&str> = input.lines().collect();
        let mut i = 0;
        
        while i < lines.len() {
            let original_line = lines[i];
            
            // Strip line numbers if present (e.g., "1: # Header" -> "# Header")
            let stripped_line = self.strip_line_number(original_line);
            let line = stripped_line.trim();
            
            // Skip empty lines but add them for spacing
            if line.is_empty() {
                components.push(MarkdownComponent::EmptyLine);
                i += 1;
                continue;
            }
            
            // Parse different markdown elements
            if let Some(component) = self.parse_line(line, &lines, &mut i) {
                components.push(component);
            }
            
            i += 1;
        }
        
        Ok(components)
    }
    
    // Strip line numbers like "1: ", "22: ", etc.
    fn strip_line_number(&self, line: &str) -> String {
        let trimmed = line.trim();
        
        // Look for pattern like "number: " at the beginning
        if let Some(colon_pos) = trimmed.find(": ") {
            let prefix = &trimmed[..colon_pos];
            if prefix.chars().all(|c| c.is_ascii_digit()) {
                return trimmed[colon_pos + 2..].to_string();
            }
        }
        
        line.to_string()
    }
    
    fn parse_line(&self, line: &str, all_lines: &[&str], index: &mut usize) -> Option<MarkdownComponent> {
        // Headings - more specific parsing
        if line.starts_with("######") && line.len() > 6 && line.chars().nth(6) == Some(' ') {
            return Some(MarkdownComponent::H6(line[7..].trim().to_string()));
        }
        if line.starts_with("#####") && line.len() > 5 && line.chars().nth(5) == Some(' ') {
            return Some(MarkdownComponent::H5(line[6..].trim().to_string()));
        }
        if line.starts_with("####") && line.len() > 4 && line.chars().nth(4) == Some(' ') {
            return Some(MarkdownComponent::H4(line[5..].trim().to_string()));
        }
        if line.starts_with("###") && line.len() > 3 && line.chars().nth(3) == Some(' ') {
            return Some(MarkdownComponent::H3(line[4..].trim().to_string()));
        }
        if line.starts_with("##") && line.len() > 2 && line.chars().nth(2) == Some(' ') {
            return Some(MarkdownComponent::H2(line[3..].trim().to_string()));
        }
        if line.starts_with('#') && line.len() > 1 && line.chars().nth(1) == Some(' ') {
            return Some(MarkdownComponent::H1(line[2..].trim().to_string()));
        }
        
        // Code blocks
        if line.starts_with("```") {
            return self.parse_code_block(all_lines, index);
        }
        
        // Tasks
        if line.starts_with("- [x]") || line.starts_with("- [X]") {
            return Some(MarkdownComponent::TaskComplete(line[5..].trim().to_string()));
        }
        if line.starts_with("- [ ]") {
            return Some(MarkdownComponent::TaskOpen(line[5..].trim().to_string()));
        }
        
        // Lists
        if line.starts_with("- ") || line.starts_with("* ") {
            return Some(MarkdownComponent::ListItem(line[2..].trim().to_string()));
        }
        
        // Numbered lists
        if let Some(captures) = self.parse_numbered_list(line) {
            return Some(MarkdownComponent::ListItem(captures));
        }
        
        // Quotes
        if line.starts_with("> ") {
            let mut quote_lines = vec![line[2..].trim().to_string()];
            let mut j = *index + 1;
            
            // Collect consecutive quote lines
            while j < all_lines.len() {
                let stripped_quote_line = self.strip_line_number(all_lines[j]);
                let quote_line = stripped_quote_line.trim();
                if quote_line.starts_with("> ") {
                    quote_lines.push(quote_line[2..].trim().to_string());
                    j += 1;
                } else {
                    break;
                }
            }
            
            *index = j - 1; // Adjust index to skip processed lines
            return Some(MarkdownComponent::Quote(quote_lines.join(" ")));
        }
        
        // Horizontal separator
        if line.starts_with("---") || line.starts_with("***") || line.starts_with("___") {
            return Some(MarkdownComponent::HorizontalSeparator);
        }
        
        // Callouts
        if line.contains("[!important]") {
            return Some(MarkdownComponent::Important(line.replace("[!important]", "").trim().to_string()));
        }
        if line.contains("[!note]") {
            return Some(MarkdownComponent::Note(line.replace("[!note]", "").trim().to_string()));
        }
        if line.contains("[!tip]") {
            return Some(MarkdownComponent::Tip(line.replace("[!tip]", "").trim().to_string()));
        }
        if line.contains("[!warning]") {
            return Some(MarkdownComponent::Warning(line.replace("[!warning]", "").trim().to_string()));
        }
        if line.contains("[!caution]") {
            return Some(MarkdownComponent::Caution(line.replace("[!caution]", "").trim().to_string()));
        }
        
        // Check for inline formatting in paragraph text
        if self.has_inline_formatting(line) {
            return Some(self.parse_inline_formatting(line));
        }
        
        // Default to paragraph - but still check for basic formatting
        Some(MarkdownComponent::Paragraph(line.to_string()))
    }
    
    fn parse_code_block(&self, all_lines: &[&str], index: &mut usize) -> Option<MarkdownComponent> {
        let stripped_start_line = self.strip_line_number(all_lines[*index]);
        let start_line = stripped_start_line.trim();
        let language = if start_line.len() > 3 {
            let lang = start_line[3..].trim();
            if lang.is_empty() { None } else { Some(lang.to_string()) }
        } else {
            None
        };
        
        let mut code_lines = Vec::new();
        let mut j = *index + 1;
        
        // Collect lines until closing ```
        while j < all_lines.len() {
            let code_line = self.strip_line_number(all_lines[j]);
            if code_line.trim().starts_with("```") {
                break;
            }
            code_lines.push(code_line);
            j += 1;
        }
        
        *index = j; // Skip to closing ```
        Some(MarkdownComponent::CodeBlock {
            language,
            content: code_lines.join("\n"),
        })
    }
    
    fn parse_numbered_list(&self, line: &str) -> Option<String> {
        // Match patterns like "1. ", "2. ", etc.
        let trimmed = line.trim();
        if let Some(dot_pos) = trimmed.find(". ") {
            let prefix = &trimmed[..dot_pos];
            if prefix.chars().all(|c| c.is_ascii_digit()) {
                return Some(trimmed[dot_pos + 2..].to_string());
            }
        }
        None
    }
    
    fn has_inline_formatting(&self, line: &str) -> bool {
        line.contains("**") || 
        line.contains("*") || 
        line.contains("`") ||
        line.contains("~~") ||
        (line.contains('[') && line.contains("]("))
    }
    
    fn parse_inline_formatting(&self, line: &str) -> MarkdownComponent {
        // Parse mixed content within a line
        let mut spans = Vec::new();
        let mut current_pos = 0;
        let mut content = line.to_string();
        
        // Find all formatting markers and sort them by position
        let mut markers = Vec::new();
        
        // Find bold markers
        let mut pos = 0;
        while let Some(start) = content[pos..].find("**") {
            let start_pos = pos + start;
            if let Some(end) = content[start_pos + 2..].find("**") {
                let end_pos = start_pos + 2 + end;
                markers.push((start_pos, end_pos + 2, "bold"));
                pos = end_pos + 2;
            } else {
                break;
            }
        }
        
        // Find code markers
        pos = 0;
        while let Some(start) = content[pos..].find("`") {
            let start_pos = pos + start;
            if let Some(end) = content[start_pos + 1..].find("`") {
                let end_pos = start_pos + 1 + end;
                markers.push((start_pos, end_pos + 1, "code"));
                pos = end_pos + 1;
            } else {
                break;
            }
        }
        
        // Sort markers by position
        markers.sort_by_key(|&(start, _, _)| start);
        
        // Build spans
        let mut last_end = 0;
        for (start, end, format_type) in markers {
            // Add text before this marker
            if start > last_end {
                let text = &content[last_end..start];
                if !text.is_empty() {
                    spans.push(Span::styled(text.to_string(), self.style.text_style));
                }
            }
            
            // Add formatted text
            let formatted_text = &content[start + 2..end - 2]; // Remove markers
            match format_type {
                "bold" => spans.push(Span::styled(formatted_text.to_string(), self.style.bold_style)),
                "code" => spans.push(Span::styled(format!("`{}`", formatted_text), self.style.code_style)),
                _ => spans.push(Span::styled(formatted_text.to_string(), self.style.text_style)),
            }
            
            last_end = end;
        }
        
        // Add remaining text
        if last_end < content.len() {
            let text = &content[last_end..];
            if !text.is_empty() {
                spans.push(Span::styled(text.to_string(), self.style.text_style));
            }
        }
        
        // If we have mixed content, return a custom component
        if spans.len() > 1 {
            MarkdownComponent::MixedContent(spans)
        } else if spans.len() == 1 {
            // Single span - determine type
            let span = &spans[0];
            if span.style.add_modifier(Modifier::BOLD) == span.style {
                MarkdownComponent::Bold(span.content.to_string())
            } else if span.style.fg == Some(Color::Yellow) {
                MarkdownComponent::Code(span.content.to_string().trim_matches('`').to_string())
            } else {
                MarkdownComponent::Paragraph(span.content.to_string())
            }
        } else {
            MarkdownComponent::Paragraph(content)
        }
    }
    
    fn extract_between(&self, text: &str, start: &str, end: &str) -> Option<String> {
        if let Some(start_pos) = text.find(start) {
            let search_start = start_pos + start.len();
            if let Some(end_pos) = text[search_start..].find(end) {
                let content = &text[search_start..search_start + end_pos];
                return Some(content.to_string());
            }
        }
        None
    }
    
    fn extract_link(&self, text: &str) -> Option<(String, String)> {
        if let Some(start) = text.find('[') {
            if let Some(middle) = text[start..].find("](") {
                let text_part = &text[start + 1..start + middle];
                let url_start = start + middle + 2;
                if let Some(end) = text[url_start..].find(')') {
                    let url_part = &text[url_start..url_start + end];
                    return Some((text_part.to_string(), url_part.to_string()));
                }
            }
        }
        None
    }

    // Convert components to styled lines for ratatui
    pub fn render_to_lines(&self, components: Vec<MarkdownComponent>) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        
        for component in components {
            lines.extend(self.component_to_lines(component));
        }
        
        lines
    }
    
    fn component_to_lines(&self, component: MarkdownComponent) -> Vec<Line<'static>> {
        match component {
            MarkdownComponent::H1(text) => vec![Line::from(vec![
                Span::styled(format!("# {}", text), self.style.h1_style)
            ])],
            MarkdownComponent::H2(text) => vec![Line::from(vec![
                Span::styled(format!("## {}", text), self.style.h2_style)
            ])],
            MarkdownComponent::H3(text) => vec![Line::from(vec![
                Span::styled(format!("### {}", text), self.style.h3_style)
            ])],
            MarkdownComponent::H4(text) => vec![Line::from(vec![
                Span::styled(format!("#### {}", text), self.style.h4_style)
            ])],
            MarkdownComponent::H5(text) => vec![Line::from(vec![
                Span::styled(format!("##### {}", text), self.style.h5_style)
            ])],
            MarkdownComponent::H6(text) => vec![Line::from(vec![
                Span::styled(format!("###### {}", text), self.style.h6_style)
            ])],
            
            MarkdownComponent::Bold(text) => vec![Line::from(vec![
                Span::styled(text, self.style.bold_style)
            ])],
            MarkdownComponent::Italic(text) => vec![Line::from(vec![
                Span::styled(text, self.style.italic_style)
            ])],
            MarkdownComponent::BoldItalic(text) => vec![Line::from(vec![
                Span::styled(text, self.style.bold_italic_style)
            ])],
            MarkdownComponent::Strikethrough(text) => vec![Line::from(vec![
                Span::styled(text, self.style.strikethrough_style)
            ])],
            MarkdownComponent::Code(text) => vec![Line::from(vec![
                Span::styled("`", self.style.code_style),
                Span::styled(text, self.style.code_style),
                Span::styled("`", self.style.code_style)
            ])],
            
            MarkdownComponent::Link { text, url } => vec![Line::from(vec![
                Span::styled(format!("{}", text), self.style.link_style)
            ])],
            
            MarkdownComponent::UnorderedList(items) => {
                let mut list_lines = Vec::new();
                for item in items {
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
            },
            
            MarkdownComponent::OrderedList(items) => {
                let mut list_lines = Vec::new();
                for (index, item) in items.into_iter().enumerate() {
                    let item_lines = self.component_to_lines(item);
                    for (i, line) in item_lines.into_iter().enumerate() {
                        if i == 0 {
                            let mut spans = vec![Span::styled(
                                format!("{}. ", index + 1), 
                                self.style.list_bullet_style
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
            },
            
            MarkdownComponent::ListItem(text) => vec![Line::from(vec![
                Span::styled("• ", self.style.list_bullet_style),
                Span::styled(text, self.style.text_style)
            ])],
            
            MarkdownComponent::CodeBlock { language, content } => {
                let mut code_lines = Vec::new();
                let language_clone = language.clone();
                if let Some(lang) = language {
                    code_lines.push(Line::from(vec![
                        Span::styled(format!("```{}", lang), self.style.code_block_style)
                    ]));
                } else {
                    code_lines.push(Line::from(vec![
                        Span::styled("```", self.style.code_block_style)
                    ]));
                }
                
                // Use syntax highlighting for code content
                use crate::services::syntax_highlighter::apply_syntax_highlighting;
                let highlighted_lines = apply_syntax_highlighting(&content, language_clone.as_ref().map(|s| s.as_str()));
                code_lines.extend(highlighted_lines);
                
                code_lines.push(Line::from(vec![
                    Span::styled("```", self.style.code_block_style)
                ]));
                
                code_lines
            },
            
            MarkdownComponent::Quote(text) => {
                let mut quote_lines = Vec::new();
                for line in text.lines() {
                    quote_lines.push(Line::from(vec![
                        Span::styled("│ ", self.style.quote_style),
                        Span::styled(line.to_string(), self.style.quote_style)
                    ]));
                }
                quote_lines
            },
            
            MarkdownComponent::Table { headers, rows } => {
                let mut table_lines = Vec::new();
                
                // Render headers
                if !headers.is_empty() {
                    let header_line = headers.iter()
                        .map(|h| format!("│ {} ", h))
                        .collect::<String>() + "│";
                    table_lines.push(Line::from(vec![
                        Span::styled(header_line, self.style.table_header_style)
                    ]));
                    
                    // Header separator
                    let separator = headers.iter()
                        .map(|h| "─".repeat(h.len() + 2))
                        .collect::<Vec<_>>()
                        .join("┼");
                    table_lines.push(Line::from(vec![
                        Span::styled(format!("├{}┤", separator), self.style.table_header_style)
                    ]));
                }
                
                // Render rows
                for row in rows {
                    let row_line = row.iter()
                        .map(|cell| format!("│ {} ", cell))
                        .collect::<String>() + "│";
                    table_lines.push(Line::from(vec![
                        Span::styled(row_line, self.style.table_cell_style)
                    ]));
                }
                
                table_lines
            },
            
            MarkdownComponent::TaskOpen(text) => vec![Line::from(vec![
                Span::styled("☐ ", self.style.task_open_style),
                Span::styled(text, self.style.text_style)
            ])],
            
            MarkdownComponent::TaskComplete(text) => vec![Line::from(vec![
                Span::styled("☑ ", self.style.task_complete_style),
                Span::styled(text, self.style.text_style)
            ])],
            
            MarkdownComponent::Important(text) => vec![
                Line::from(vec![Span::styled("🔒 IMPORTANT", self.style.important_style)]),
                Line::from(vec![Span::styled(text, self.style.text_style)])
            ],
            
            MarkdownComponent::Note(text) => vec![
                Line::from(vec![Span::styled("📝 NOTE", self.style.note_style)]),
                Line::from(vec![Span::styled(text, self.style.text_style)])
            ],
            
            MarkdownComponent::Tip(text) => vec![
                Line::from(vec![Span::styled("💡 TIP", self.style.tip_style)]),
                Line::from(vec![Span::styled(text, self.style.text_style)])
            ],
            
            MarkdownComponent::Warning(text) => vec![
                Line::from(vec![Span::styled("⚠️  WARNING", self.style.warning_style)]),
                Line::from(vec![Span::styled(text, self.style.text_style)])
            ],
            
            MarkdownComponent::Caution(text) => vec![
                Line::from(vec![Span::styled("⚡ CAUTION", self.style.caution_style)]),
                Line::from(vec![Span::styled(text, self.style.text_style)])
            ],
            
            MarkdownComponent::HorizontalSeparator => vec![Line::from(vec![
                Span::styled("─".repeat(50), self.style.separator_style)
            ])],
            
            MarkdownComponent::Paragraph(text) => vec![Line::from(vec![
                Span::styled(text, self.style.text_style)
            ])],
            
            MarkdownComponent::PlainText(text) => vec![Line::from(vec![
                Span::styled(text, self.style.text_style)
            ])],
            
            MarkdownComponent::Word(text) => vec![Line::from(vec![
                Span::styled(text, self.style.text_style)
            ])],
            
            MarkdownComponent::EmptyLine => vec![Line::from("")],
            
            MarkdownComponent::MixedContent(spans) => vec![Line::from(spans)],
        }
    }
}

// Simple public function for easy use
pub fn render_markdown_to_lines(
    markdown_content: &str,
) -> Result<Vec<Line<'static>>, Box<dyn std::error::Error>> {
    let style = MarkdownStyle::default();
    let renderer = MarkdownRenderer::new(style);
    let components = renderer.parse_markdown(markdown_content)?;
    
    eprintln!("DEBUG: Parsed {} components", components.len());
    for (i, component) in components.iter().enumerate().take(10) {
        eprintln!("  Component {}: {:?}", i, component);
    }
    
    Ok(renderer.render_to_lines(components))
}

// Helper function to detect if content is markdown
pub fn is_likely_markdown(content: &str) -> bool {
    let indicators = ["# ", "## ", "### ", "```", "- ", "* ", "> ", "**", "*"];
    indicators.iter().any(|&indicator| content.contains(indicator)) ||
    (content.contains('[') && content.contains("]("))
}