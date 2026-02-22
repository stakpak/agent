use pulldown_cmark::{Alignment, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde_json::{Value, json};

/// Maximum blocks per Slack message.
const MAX_BLOCKS_PER_MESSAGE: usize = 50;

/// Maximum characters in a Slack header block.
const HEADER_CHAR_LIMIT: usize = 150;

/// Maximum rows in a Slack table block.
const MAX_TABLE_ROWS: usize = 100;

/// Maximum columns in a Slack table block.
const MAX_TABLE_COLUMNS: usize = 20;

/// Maximum indent level for nested lists (Slack renders ~8 levels).
const MAX_LIST_INDENT: u32 = 8;

/// Slack recommends keeping the `text` fallback field under 4,000 characters.
/// Messages with >40,000 characters get silently truncated by Slack.
/// We use the recommended limit for clean notifications.
const FALLBACK_TEXT_LIMIT: usize = 4_000;

/// Maximum characters in a single `rich_text_preformatted` text element.
/// Slack doesn't publish a hard limit, but very large payloads can fail.
/// We split code blocks that exceed this into multiple preformatted elements
/// across separate rich_text blocks (and thus separate messages if needed).
const MAX_PREFORMATTED_CHARS: usize = 30_000;

/// Maximum rich_text sub-elements per single rich_text block before we flush
/// to a new block. Prevents any single block from becoming excessively large.
const MAX_RT_ELEMENTS_PER_BLOCK: usize = 256;

/// A rendered Slack message ready for `chat.postMessage`.
#[derive(Debug, Clone)]
pub struct SlackMessage {
    /// Top-level blocks (header, rich_text, divider).
    pub blocks: Vec<Value>,
    /// Attachments containing a table block (at most one per message).
    pub attachments: Option<Vec<Value>>,
    /// Plain text fallback for notifications/accessibility.
    pub fallback_text: String,
}

/// Convert markdown text to a sequence of Slack messages.
///
/// Returns one or more `SlackMessage` values, split as needed for:
/// - The 50-block-per-message limit
/// - The one-table-per-message constraint
///
/// On any internal error, returns a single plain-text fallback message.
pub fn markdown_to_slack_messages(text: &str) -> Vec<SlackMessage> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    match render_blocks(text) {
        Ok(rendered) => {
            let fallback = generate_fallback_text(text);
            split_into_messages(rendered.blocks, rendered.tables, &fallback)
        }
        Err(_) => {
            // Graceful degradation: plain text fallback
            vec![SlackMessage {
                blocks: Vec::new(),
                attachments: None,
                fallback_text: text.to_string(),
            }]
        }
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// Intermediate render result before message splitting.
struct RenderedBlocks {
    /// Ordered sequence of blocks and table markers.
    blocks: Vec<BlockOrTable>,
    /// Accumulated table blocks.
    tables: Vec<Value>,
}

/// A block or a reference to a table (by index into the tables vec).
enum BlockOrTable {
    Block(Value),
    Table(usize),
}

/// Active inline styles tracked as a stack.
#[derive(Debug, Clone, Default)]
struct StyleState {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
}

impl StyleState {
    fn to_style_object(&self) -> Option<Value> {
        let mut style = serde_json::Map::new();
        if self.bold {
            style.insert("bold".to_string(), json!(true));
        }
        if self.italic {
            style.insert("italic".to_string(), json!(true));
        }
        if self.strike {
            style.insert("strike".to_string(), json!(true));
        }
        if self.code {
            style.insert("code".to_string(), json!(true));
        }
        if style.is_empty() {
            None
        } else {
            Some(Value::Object(style))
        }
    }
}

/// Context for what we're currently building.
#[derive(Debug, Clone, PartialEq)]
enum RenderContext {
    /// Top-level paragraph or inline content.
    Paragraph,
    /// Inside a heading (H1/H2 → header block, H3+ → bold rich_text).
    Heading(HeadingLevel),
    /// Inside a code block.
    CodeBlock,
    /// Inside a blockquote.
    BlockQuote,
    /// Inside a list item.
    ListItem,
    /// Inside a table cell.
    TableCell,
}

/// Tracks list nesting.
#[derive(Debug, Clone)]
struct ListInfo {
    ordered: bool,
    indent: u32,
}

/// Tracks table accumulation.
#[derive(Debug, Clone)]
struct TableState {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<Value>>,
    current_row: Vec<Value>,
    current_cell_elements: Vec<Value>,
    current_cell_has_formatting: bool,
    in_header: bool,
    column_index: usize,
    /// Total rows seen (including those dropped due to MAX_TABLE_ROWS).
    total_rows_seen: usize,
}

impl TableState {
    fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            alignments,
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell_elements: Vec::new(),
            current_cell_has_formatting: false,
            in_header: false,
            column_index: 0,
            total_rows_seen: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Core renderer
// ---------------------------------------------------------------------------

fn render_blocks(text: &str) -> Result<RenderedBlocks, ()> {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let parser = Parser::new_ext(text, options);

    let mut blocks: Vec<BlockOrTable> = Vec::new();
    let mut tables: Vec<Value> = Vec::new();

    // Current rich_text block elements accumulator.
    let mut rt_elements: Vec<Value> = Vec::new();
    // Current inline elements within a section/preformatted/quote.
    let mut inline_elements: Vec<Value> = Vec::new();

    let mut style = StyleState::default();
    let mut context_stack: Vec<RenderContext> = Vec::new();
    let mut list_stack: Vec<ListInfo> = Vec::new();
    // Accumulate list items for the current list level.
    let mut list_items_stack: Vec<Vec<Value>> = Vec::new();
    let mut table_state: Option<TableState> = None;

    // Pending link info (captured on Start, used on End).
    let mut pending_link_url: Option<String> = None;
    let mut link_text_buffer: Option<String> = None;

    // Header text accumulator (for H1/H2 which use plain_text header blocks).
    let mut header_text_buffer: Option<String> = None;

    /// Flush inline_elements into a rich_text_section and push to rt_elements.
    /// If rt_elements exceeds the per-block element limit, auto-flush to a new block.
    fn flush_section(
        inline_elements: &mut Vec<Value>,
        rt_elements: &mut Vec<Value>,
        blocks: &mut Vec<BlockOrTable>,
    ) {
        if !inline_elements.is_empty() {
            rt_elements.push(json!({
                "type": "rich_text_section",
                "elements": std::mem::take(inline_elements)
            }));
        }
        // Guard: if a single rich_text block accumulates too many sub-elements,
        // flush it to prevent excessively large payloads.
        if rt_elements.len() >= MAX_RT_ELEMENTS_PER_BLOCK {
            flush_rich_text(rt_elements, blocks);
        }
    }

    /// Flush rt_elements into a rich_text block and push to blocks.
    fn flush_rich_text(rt_elements: &mut Vec<Value>, blocks: &mut Vec<BlockOrTable>) {
        if !rt_elements.is_empty() {
            blocks.push(BlockOrTable::Block(json!({
                "type": "rich_text",
                "elements": std::mem::take(rt_elements)
            })));
        }
    }

    fn current_context(stack: &[RenderContext]) -> Option<&RenderContext> {
        stack.last()
    }

    for event in parser {
        match event {
            // ----- Block-level Start events -----
            Event::Start(Tag::Heading { level, .. }) => {
                flush_section(&mut inline_elements, &mut rt_elements, &mut blocks);
                flush_rich_text(&mut rt_elements, &mut blocks);
                context_stack.push(RenderContext::Heading(level));
                header_text_buffer = Some(String::new());
            }

            Event::Start(Tag::Paragraph) => {
                if table_state.is_some() {
                    // Inside a table cell — paragraphs are just inline content.
                    continue;
                }
                if current_context(&context_stack) == Some(&RenderContext::BlockQuote)
                    || current_context(&context_stack) == Some(&RenderContext::ListItem)
                {
                    // Inside blockquote or list item — don't create new context.
                    continue;
                }
                context_stack.push(RenderContext::Paragraph);
            }

            Event::Start(Tag::CodeBlock(_kind)) => {
                flush_section(&mut inline_elements, &mut rt_elements, &mut blocks);
                context_stack.push(RenderContext::CodeBlock);
            }

            Event::Start(Tag::BlockQuote(_)) => {
                flush_section(&mut inline_elements, &mut rt_elements, &mut blocks);
                context_stack.push(RenderContext::BlockQuote);
            }

            Event::Start(Tag::List(start_num)) => {
                flush_section(&mut inline_elements, &mut rt_elements, &mut blocks);
                let indent = if list_stack.is_empty() {
                    // Starting a new top-level list: flush any accumulated rich_text.
                    0
                } else {
                    let parent_indent = list_stack.last().map_or(0, |l| l.indent);
                    (parent_indent + 1).min(MAX_LIST_INDENT)
                };
                list_stack.push(ListInfo {
                    ordered: start_num.is_some(),
                    indent,
                });
                list_items_stack.push(Vec::new());
            }

            Event::Start(Tag::Item) => {
                flush_section(&mut inline_elements, &mut rt_elements, &mut blocks);
                context_stack.push(RenderContext::ListItem);
            }

            Event::Start(Tag::Table(alignments)) => {
                flush_section(&mut inline_elements, &mut rt_elements, &mut blocks);
                flush_rich_text(&mut rt_elements, &mut blocks);
                table_state = Some(TableState::new(alignments));
            }

            Event::Start(Tag::TableHead) => {
                if let Some(ref mut ts) = table_state {
                    ts.in_header = true;
                }
            }

            Event::Start(Tag::TableRow) => {
                if let Some(ref mut ts) = table_state {
                    ts.current_row = Vec::new();
                    ts.column_index = 0;
                }
            }

            Event::Start(Tag::TableCell) => {
                if let Some(ref mut ts) = table_state {
                    ts.current_cell_elements = Vec::new();
                    ts.current_cell_has_formatting = false;
                }
                context_stack.push(RenderContext::TableCell);
            }

            // ----- Inline Start events -----
            Event::Start(Tag::Strong) => {
                style.bold = true;
                if table_state.is_some()
                    && let Some(ref mut ts) = table_state
                {
                    ts.current_cell_has_formatting = true;
                }
            }

            Event::Start(Tag::Emphasis) => {
                style.italic = true;
                if table_state.is_some()
                    && let Some(ref mut ts) = table_state
                {
                    ts.current_cell_has_formatting = true;
                }
            }

            Event::Start(Tag::Strikethrough) => {
                style.strike = true;
                if table_state.is_some()
                    && let Some(ref mut ts) = table_state
                {
                    ts.current_cell_has_formatting = true;
                }
            }

            Event::Start(Tag::Link { dest_url, .. }) => {
                pending_link_url = Some(dest_url.to_string());
                link_text_buffer = Some(String::new());
                if table_state.is_some()
                    && let Some(ref mut ts) = table_state
                {
                    ts.current_cell_has_formatting = true;
                }
            }

            // ----- Text content events -----
            Event::Text(content) => {
                let text_str = content.as_ref();

                // Accumulate header text.
                if let Some(ref mut buf) = header_text_buffer {
                    buf.push_str(text_str);
                    continue;
                }

                // Accumulate link text.
                if let Some(ref mut buf) = link_text_buffer {
                    buf.push_str(text_str);
                    continue;
                }

                // Inside a table cell.
                if let Some(ref mut ts) = table_state {
                    let mut elem = json!({
                        "type": "text",
                        "text": text_str
                    });
                    if let Some(s) = style.to_style_object() {
                        elem.as_object_mut()
                            .map(|o| o.insert("style".to_string(), s));
                    }
                    ts.current_cell_elements.push(elem);
                    continue;
                }

                // Inside a code block.
                if current_context(&context_stack) == Some(&RenderContext::CodeBlock) {
                    inline_elements.push(json!({
                        "type": "text",
                        "text": text_str
                    }));
                    continue;
                }

                // Regular inline text.
                let mut elem = json!({
                    "type": "text",
                    "text": text_str
                });
                if let Some(s) = style.to_style_object() {
                    elem.as_object_mut()
                        .map(|o| o.insert("style".to_string(), s));
                }
                inline_elements.push(elem);
            }

            Event::Code(content) => {
                let text_str = content.as_ref();

                // Inside header — just append plain text.
                if let Some(ref mut buf) = header_text_buffer {
                    buf.push_str(text_str);
                    continue;
                }

                // Inside link text.
                if let Some(ref mut buf) = link_text_buffer {
                    buf.push_str(text_str);
                    continue;
                }

                // Inside table cell.
                if let Some(ref mut ts) = table_state {
                    ts.current_cell_has_formatting = true;
                    ts.current_cell_elements.push(json!({
                        "type": "text",
                        "text": text_str,
                        "style": { "code": true }
                    }));
                    continue;
                }

                inline_elements.push(json!({
                    "type": "text",
                    "text": text_str,
                    "style": { "code": true }
                }));
            }

            Event::SoftBreak => {
                if let Some(ref mut buf) = header_text_buffer {
                    buf.push(' ');
                    continue;
                }
                if let Some(ref mut buf) = link_text_buffer {
                    buf.push(' ');
                    continue;
                }
                if current_context(&context_stack) == Some(&RenderContext::CodeBlock) {
                    inline_elements.push(json!({
                        "type": "text",
                        "text": "\n"
                    }));
                    continue;
                }
                // In regular text, soft break = newline.
                inline_elements.push(json!({
                    "type": "text",
                    "text": "\n"
                }));
            }

            Event::HardBreak => {
                if let Some(ref mut buf) = header_text_buffer {
                    buf.push(' ');
                    continue;
                }
                inline_elements.push(json!({
                    "type": "text",
                    "text": "\n"
                }));
            }

            // ----- Block-level End events -----
            Event::End(TagEnd::Heading(level)) => {
                context_stack.pop();
                let header_text = header_text_buffer.take().unwrap_or_default();

                match level {
                    HeadingLevel::H1 | HeadingLevel::H2 => {
                        // Truncate to 150 chars (by characters, not bytes).
                        let truncated: String =
                            header_text.chars().take(HEADER_CHAR_LIMIT).collect();
                        blocks.push(BlockOrTable::Block(json!({
                            "type": "header",
                            "text": {
                                "type": "plain_text",
                                "text": truncated
                            }
                        })));
                    }
                    _ => {
                        // H3-H6: render as bold text in a rich_text block.
                        let section_elements = vec![json!({
                            "type": "text",
                            "text": header_text,
                            "style": { "bold": true }
                        })];
                        rt_elements.push(json!({
                            "type": "rich_text_section",
                            "elements": section_elements
                        }));
                    }
                }
            }

            Event::End(TagEnd::Paragraph) => {
                if table_state.is_some() {
                    continue;
                }
                let popped = context_stack.last();
                if popped == Some(&RenderContext::BlockQuote)
                    || popped == Some(&RenderContext::ListItem)
                {
                    // Don't pop — we didn't push for these.
                    continue;
                }
                context_stack.pop();
                flush_section(&mut inline_elements, &mut rt_elements, &mut blocks);
            }

            Event::End(TagEnd::CodeBlock) => {
                context_stack.pop();
                // Collect all text from inline_elements into a single string.
                let elements = std::mem::take(&mut inline_elements);
                let full_text: String = elements
                    .iter()
                    .filter_map(|e| e.get("text").and_then(|t| t.as_str()))
                    .collect();

                if full_text.is_empty() {
                    // Empty code block — emit with a single space to avoid Slack errors.
                    rt_elements.push(json!({
                        "type": "rich_text_preformatted",
                        "elements": [{ "type": "text", "text": " " }]
                    }));
                } else if full_text.chars().count() <= MAX_PREFORMATTED_CHARS {
                    // Fits in a single preformatted block.
                    rt_elements.push(json!({
                        "type": "rich_text_preformatted",
                        "elements": [{ "type": "text", "text": full_text }]
                    }));
                } else {
                    // Split large code blocks into multiple preformatted elements.
                    // Each chunk becomes its own rich_text block to stay within limits.
                    for chunk in split_text_by_chars(&full_text, MAX_PREFORMATTED_CHARS) {
                        // Flush current rt_elements first, then emit each chunk
                        // as its own rich_text block.
                        flush_rich_text(&mut rt_elements, &mut blocks);
                        rt_elements.push(json!({
                            "type": "rich_text_preformatted",
                            "elements": [{ "type": "text", "text": chunk }]
                        }));
                        flush_rich_text(&mut rt_elements, &mut blocks);
                    }
                }
            }

            Event::End(TagEnd::BlockQuote(_)) => {
                context_stack.pop();
                let elements = std::mem::take(&mut inline_elements);
                if !elements.is_empty() {
                    rt_elements.push(json!({
                        "type": "rich_text_quote",
                        "elements": elements
                    }));
                }
            }

            Event::End(TagEnd::Item) => {
                context_stack.pop();
                // Collect inline elements as a rich_text_section for this list item.
                let elements = std::mem::take(&mut inline_elements);
                let section = if elements.is_empty() {
                    json!({
                        "type": "rich_text_section",
                        "elements": [{ "type": "text", "text": " " }]
                    })
                } else {
                    json!({
                        "type": "rich_text_section",
                        "elements": elements
                    })
                };
                if let Some(items) = list_items_stack.last_mut() {
                    items.push(section);
                }
            }

            Event::End(TagEnd::List(_ordered)) => {
                if let (Some(info), Some(items)) = (list_stack.pop(), list_items_stack.pop())
                    && !items.is_empty()
                {
                    let mut list_block = json!({
                        "type": "rich_text_list",
                        "style": if info.ordered { "ordered" } else { "bullet" },
                        "elements": items
                    });
                    if info.indent > 0 {
                        list_block
                            .as_object_mut()
                            .map(|o| o.insert("indent".to_string(), json!(info.indent)));
                    }
                    rt_elements.push(list_block);
                }

                // If we're back to top-level (no more lists), flush the rich_text block.
                if list_stack.is_empty() {
                    flush_rich_text(&mut rt_elements, &mut blocks);
                }
            }

            Event::End(TagEnd::TableCell) => {
                context_stack.pop();
                if let Some(ref mut ts) = table_state {
                    let cell = if ts.current_cell_has_formatting {
                        // Use rich_text cell type.
                        json!({
                            "type": "rich_text",
                            "elements": [{
                                "type": "rich_text_section",
                                "elements": std::mem::take(&mut ts.current_cell_elements)
                            }]
                        })
                    } else {
                        // Use raw_text cell type.
                        let text: String = ts
                            .current_cell_elements
                            .drain(..)
                            .filter_map(|e| {
                                e.get("text").and_then(|t| t.as_str()).map(String::from)
                            })
                            .collect();
                        json!({
                            "type": "raw_text",
                            "text": text
                        })
                    };
                    if ts.current_row.len() < MAX_TABLE_COLUMNS {
                        ts.current_row.push(cell);
                    }
                    ts.column_index += 1;
                }
            }

            Event::End(TagEnd::TableRow) => {
                if let Some(ref mut ts) = table_state {
                    ts.total_rows_seen += 1;
                    if ts.rows.len() < MAX_TABLE_ROWS {
                        ts.rows.push(std::mem::take(&mut ts.current_row));
                    }
                }
            }

            Event::End(TagEnd::TableHead) => {
                if let Some(ref mut ts) = table_state {
                    // The header row's cells are directly inside TableHead (no TableRow wrapper).
                    // Flush accumulated cells as the header row.
                    ts.total_rows_seen += 1;
                    if !ts.current_row.is_empty() && ts.rows.len() < MAX_TABLE_ROWS {
                        ts.rows.push(std::mem::take(&mut ts.current_row));
                    }
                    ts.in_header = false;
                }
            }

            Event::End(TagEnd::Table) => {
                if let Some(ts) = table_state.take() {
                    let column_settings: Vec<Value> = ts
                        .alignments
                        .iter()
                        .take(MAX_TABLE_COLUMNS)
                        .map(|a| {
                            let align = match a {
                                Alignment::None | Alignment::Left => "left",
                                Alignment::Center => "center",
                                Alignment::Right => "right",
                            };
                            json!({ "align": align, "is_wrapped": true })
                        })
                        .collect();

                    let table_block = json!({
                        "type": "table",
                        "column_settings": column_settings,
                        "rows": ts.rows
                    });

                    let table_idx = tables.len();
                    tables.push(table_block);
                    blocks.push(BlockOrTable::Table(table_idx));

                    // If rows were truncated, add a note after the table.
                    let dropped = ts.total_rows_seen.saturating_sub(ts.rows.len());
                    if dropped > 0 {
                        let note = format!(
                            "… {dropped} more row{} not shown",
                            if dropped == 1 { "" } else { "s" }
                        );
                        blocks.push(BlockOrTable::Block(json!({
                            "type": "rich_text",
                            "elements": [{
                                "type": "rich_text_section",
                                "elements": [{
                                    "type": "text",
                                    "text": note,
                                    "style": { "italic": true }
                                }]
                            }]
                        })));
                    }
                }
            }

            // ----- Inline End events -----
            Event::End(TagEnd::Strong) => {
                style.bold = false;
            }

            Event::End(TagEnd::Emphasis) => {
                style.italic = false;
            }

            Event::End(TagEnd::Strikethrough) => {
                style.strike = false;
            }

            Event::End(TagEnd::Link) => {
                let url = pending_link_url.take().unwrap_or_default();
                let display_text = link_text_buffer.take().unwrap_or_default();

                // Inside header — just use the display text (already accumulated).
                if header_text_buffer.is_some() {
                    // Text was already accumulated in header_text_buffer via link_text_buffer.
                    // Actually, link_text_buffer captured it. We need to push it to header.
                    if let Some(ref mut buf) = header_text_buffer {
                        buf.push_str(&display_text);
                    }
                    continue;
                }

                // Inside table cell.
                if let Some(ref mut ts) = table_state {
                    ts.current_cell_has_formatting = true;
                    let mut elem = json!({
                        "type": "link",
                        "url": url
                    });
                    if !display_text.is_empty() {
                        elem.as_object_mut()
                            .map(|o| o.insert("text".to_string(), json!(display_text)));
                    }
                    if let Some(s) = style.to_style_object() {
                        elem.as_object_mut()
                            .map(|o| o.insert("style".to_string(), s));
                    }
                    ts.current_cell_elements.push(elem);
                    continue;
                }

                let mut elem = json!({
                    "type": "link",
                    "url": url
                });
                if !display_text.is_empty() {
                    elem.as_object_mut()
                        .map(|o| o.insert("text".to_string(), json!(display_text)));
                }
                if let Some(s) = style.to_style_object() {
                    elem.as_object_mut()
                        .map(|o| o.insert("style".to_string(), s));
                }
                inline_elements.push(elem);
            }

            // ----- Standalone events -----
            Event::Rule => {
                flush_section(&mut inline_elements, &mut rt_elements, &mut blocks);
                flush_rich_text(&mut rt_elements, &mut blocks);
                blocks.push(BlockOrTable::Block(json!({ "type": "divider" })));
            }

            // Ignore events we don't handle.
            Event::Start(Tag::Image { .. })
            | Event::End(TagEnd::Image)
            | Event::Start(Tag::HtmlBlock)
            | Event::End(TagEnd::HtmlBlock)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::Start(Tag::FootnoteDefinition(_))
            | Event::End(TagEnd::FootnoteDefinition)
            | Event::FootnoteReference(_)
            | Event::Start(Tag::MetadataBlock(_))
            | Event::End(TagEnd::MetadataBlock(_))
            | Event::Start(Tag::DefinitionList)
            | Event::End(TagEnd::DefinitionList)
            | Event::Start(Tag::DefinitionListTitle)
            | Event::End(TagEnd::DefinitionListTitle)
            | Event::Start(Tag::DefinitionListDefinition)
            | Event::End(TagEnd::DefinitionListDefinition)
            | Event::Start(Tag::Superscript)
            | Event::End(TagEnd::Superscript)
            | Event::Start(Tag::Subscript)
            | Event::End(TagEnd::Subscript)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::TaskListMarker(_) => {}
        }
    }

    // Flush any remaining content.
    flush_section(&mut inline_elements, &mut rt_elements, &mut blocks);
    flush_rich_text(&mut rt_elements, &mut blocks);

    Ok(RenderedBlocks { blocks, tables })
}

// ---------------------------------------------------------------------------
// Message splitting
// ---------------------------------------------------------------------------

fn split_into_messages(
    blocks: Vec<BlockOrTable>,
    tables: Vec<Value>,
    fallback: &str,
) -> Vec<SlackMessage> {
    let mut messages: Vec<SlackMessage> = Vec::new();
    let mut current_blocks: Vec<Value> = Vec::new();
    let mut fallback_offset = 0;

    for item in blocks {
        match item {
            BlockOrTable::Block(block) => {
                if current_blocks.len() >= MAX_BLOCKS_PER_MESSAGE {
                    // Split: emit current message.
                    let fb = fallback_chunk(fallback, fallback_offset, current_blocks.len());
                    fallback_offset += current_blocks.len();
                    messages.push(SlackMessage {
                        blocks: std::mem::take(&mut current_blocks),
                        attachments: None,
                        fallback_text: fb,
                    });
                }
                current_blocks.push(block);
            }
            BlockOrTable::Table(idx) => {
                // Tables go in attachments. One table per message.
                let table_block = tables.get(idx).cloned().unwrap_or(json!(null));
                let fb = fallback_chunk(fallback, fallback_offset, current_blocks.len());
                fallback_offset += current_blocks.len();

                let msg_blocks = if current_blocks.is_empty() {
                    Vec::new()
                } else {
                    std::mem::take(&mut current_blocks)
                };

                messages.push(SlackMessage {
                    blocks: msg_blocks,
                    attachments: Some(vec![json!({
                        "blocks": [table_block]
                    })]),
                    fallback_text: fb,
                });
            }
        }
    }

    // Flush remaining blocks.
    if !current_blocks.is_empty() {
        let fb = if messages.is_empty() {
            fallback.to_string()
        } else {
            fallback_chunk(fallback, fallback_offset, current_blocks.len())
        };
        messages.push(SlackMessage {
            blocks: current_blocks,
            attachments: None,
            fallback_text: fb,
        });
    }

    // Edge case: no blocks and no tables produced.
    if messages.is_empty() && !fallback.is_empty() {
        messages.push(SlackMessage {
            blocks: Vec::new(),
            attachments: None,
            fallback_text: fallback.to_string(),
        });
    }

    messages
}

/// Generate a rough fallback text chunk. For simplicity, use the full fallback
/// for the first message and truncated portions for subsequent ones.
fn fallback_chunk(fallback: &str, _offset: usize, _block_count: usize) -> String {
    // For multi-message splits, each message gets the full fallback.
    // Slack uses this for notifications — the first message's fallback is most important.
    fallback.to_string()
}

// ---------------------------------------------------------------------------
// Fallback text generator
// ---------------------------------------------------------------------------

/// Generate plain text from markdown by stripping all formatting.
fn generate_fallback_text(text: &str) -> String {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let parser = Parser::new_ext(text, options);

    let mut output = String::new();
    let mut last_was_block = false;

    for event in parser {
        match event {
            Event::Text(content) => {
                output.push_str(content.as_ref());
                last_was_block = false;
            }
            Event::Code(content) => {
                output.push_str(content.as_ref());
                last_was_block = false;
            }
            Event::SoftBreak | Event::HardBreak => {
                output.push('\n');
            }
            Event::Start(Tag::CodeBlock(_)) => {
                if !last_was_block {
                    output.push('\n');
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                output.push('\n');
                last_was_block = true;
            }
            Event::Start(Tag::Paragraph) => {
                if !output.is_empty() && !last_was_block {
                    output.push('\n');
                }
            }
            Event::End(TagEnd::Paragraph) => {
                output.push('\n');
                last_was_block = true;
            }
            Event::Start(Tag::Heading { .. }) => {
                if !output.is_empty() && !last_was_block {
                    output.push('\n');
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                output.push('\n');
                last_was_block = true;
            }
            Event::Start(Tag::Item) => {
                output.push_str("• ");
                last_was_block = false;
            }
            Event::End(TagEnd::Item) => {
                output.push('\n');
                last_was_block = false;
            }
            Event::Rule => {
                output.push_str("---\n");
                last_was_block = true;
            }
            _ => {}
        }
    }

    let result = output.trim().to_string();
    truncate_to_char_limit(&result, FALLBACK_TEXT_LIMIT)
}

/// Truncate a string to a character limit, appending "…" if truncated.
/// Always splits on a valid UTF-8 character boundary.
fn truncate_to_char_limit(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let truncated: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{truncated}…")
}

/// Split a string into chunks of at most `max_chars` characters each.
/// Prefers splitting at newline boundaries when possible.
fn split_text_by_chars(text: &str, max_chars: usize) -> Vec<String> {
    if text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current_chunk = String::new();
    let mut char_count = 0;

    for ch in text.chars() {
        current_chunk.push(ch);
        char_count += 1;

        if char_count >= max_chars {
            // Try to split at the last newline in the current chunk.
            // Try to split at the last newline in the current chunk.
            let mut last_newline_char_idx = None;
            for (i, c) in current_chunk.chars().enumerate() {
                if c == '\n' {
                    last_newline_char_idx = Some(i);
                }
            }

            if let Some(nl_char_idx) = last_newline_char_idx {
                // Split after the newline character.
                let head: String = current_chunk.chars().take(nl_char_idx + 1).collect();
                let tail: String = current_chunk.chars().skip(nl_char_idx + 1).collect();
                chunks.push(head);
                current_chunk = tail;
                char_count = current_chunk.chars().count();
            } else {
                // No newline found — hard split at the limit.
                chunks.push(std::mem::take(&mut current_chunk));
                char_count = 0;
            }
        }
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    chunks
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn render(md: &str) -> Vec<SlackMessage> {
        markdown_to_slack_messages(md)
    }

    fn first_blocks(md: &str) -> Vec<Value> {
        let msgs = render(md);
        assert!(!msgs.is_empty(), "expected at least one message");
        msgs[0].blocks.clone()
    }

    fn first_rich_text_elements(md: &str) -> Vec<Value> {
        let blocks = first_blocks(md);
        let rt = blocks
            .iter()
            .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("rich_text"))
            .expect("expected a rich_text block");
        rt.get("elements")
            .and_then(|e| e.as_array())
            .cloned()
            .unwrap_or_default()
    }

    // ---- 1. Inline Formatting ----

    #[test]
    fn bold_text() {
        let elements = first_rich_text_elements("**hello world**");
        let section = &elements[0];
        assert_eq!(section["type"], "rich_text_section");
        let text_elem = &section["elements"][0];
        assert_eq!(text_elem["type"], "text");
        assert_eq!(text_elem["text"], "hello world");
        assert_eq!(text_elem["style"]["bold"], true);
    }

    #[test]
    fn italic_text() {
        let elements = first_rich_text_elements("*hello world*");
        let section = &elements[0];
        let text_elem = &section["elements"][0];
        assert_eq!(text_elem["style"]["italic"], true);
    }

    #[test]
    fn strikethrough_text() {
        let elements = first_rich_text_elements("~~deleted~~");
        let section = &elements[0];
        let text_elem = &section["elements"][0];
        assert_eq!(text_elem["style"]["strike"], true);
    }

    #[test]
    fn inline_code() {
        let elements = first_rich_text_elements("`kubectl get pods`");
        let section = &elements[0];
        let text_elem = &section["elements"][0];
        assert_eq!(text_elem["style"]["code"], true);
        assert_eq!(text_elem["text"], "kubectl get pods");
    }

    #[test]
    fn nested_bold_in_italic() {
        let elements = first_rich_text_elements("*this is **very** important*");
        let section = &elements[0];
        let elems = section["elements"].as_array().expect("elements array");
        // Should have: italic "this is ", bold+italic "very", italic " important"
        assert!(elems.len() >= 3);
        assert_eq!(elems[0]["style"]["italic"], true);
        assert_eq!(elems[1]["style"]["bold"], true);
        assert_eq!(elems[1]["style"]["italic"], true);
        assert_eq!(elems[2]["style"]["italic"], true);
    }

    #[test]
    fn plain_text_no_formatting() {
        let elements = first_rich_text_elements("Just a normal paragraph.");
        let section = &elements[0];
        let text_elem = &section["elements"][0];
        assert_eq!(text_elem["type"], "text");
        assert_eq!(text_elem["text"], "Just a normal paragraph.");
        assert!(text_elem.get("style").is_none() || text_elem["style"].is_null());
    }

    // ---- 2. Headers ----

    #[test]
    fn h1_header() {
        let blocks = first_blocks("# Deployment Plan");
        let header = &blocks[0];
        assert_eq!(header["type"], "header");
        assert_eq!(header["text"]["type"], "plain_text");
        assert_eq!(header["text"]["text"], "Deployment Plan");
    }

    #[test]
    fn h2_header() {
        let blocks = first_blocks("## Configuration");
        let header = &blocks[0];
        assert_eq!(header["type"], "header");
        assert_eq!(header["text"]["text"], "Configuration");
    }

    #[test]
    fn h3_header_as_bold_rich_text() {
        let elements = first_rich_text_elements("### Details");
        let section = &elements[0];
        assert_eq!(section["type"], "rich_text_section");
        let text_elem = &section["elements"][0];
        assert_eq!(text_elem["style"]["bold"], true);
        assert_eq!(text_elem["text"], "Details");
    }

    #[test]
    fn header_strips_inline_formatting() {
        let blocks = first_blocks("# Deploy **v2.0** to _production_");
        let header = &blocks[0];
        assert_eq!(header["type"], "header");
        // Header blocks only support plain_text — formatting should be stripped.
        let text = header["text"]["text"].as_str().expect("header text");
        assert!(text.contains("v2.0"));
        assert!(text.contains("production"));
    }

    #[test]
    fn header_truncated_at_150_chars() {
        let long_text = "a".repeat(200);
        let md = format!("# {long_text}");
        let blocks = first_blocks(&md);
        let header = &blocks[0];
        let text = header["text"]["text"].as_str().expect("header text");
        assert_eq!(text.chars().count(), HEADER_CHAR_LIMIT);
    }

    // ---- 3. Code Blocks ----

    #[test]
    fn fenced_code_block() {
        let md = "```\necho \"hello\"\n```";
        let elements = first_rich_text_elements(md);
        let pre = &elements[0];
        assert_eq!(pre["type"], "rich_text_preformatted");
        let text_elem = &pre["elements"][0];
        assert_eq!(text_elem["text"], "echo \"hello\"\n");
    }

    #[test]
    fn fenced_code_block_with_language() {
        let md = "```bash\nkubectl apply -f deployment.yaml\n```";
        let elements = first_rich_text_elements(md);
        let pre = &elements[0];
        assert_eq!(pre["type"], "rich_text_preformatted");
    }

    #[test]
    fn multi_line_code_block() {
        let md = "```python\ndef hello():\n    print(\"world\")\n    return True\n```";
        let elements = first_rich_text_elements(md);
        let pre = &elements[0];
        assert_eq!(pre["type"], "rich_text_preformatted");
        let text = pre["elements"][0]["text"].as_str().expect("code text");
        assert!(text.contains("def hello():"));
        assert!(text.contains("return True"));
    }

    #[test]
    fn empty_code_block() {
        let md = "```\n```";
        let elements = first_rich_text_elements(md);
        let pre = &elements[0];
        assert_eq!(pre["type"], "rich_text_preformatted");
        // Should not crash — may have a space placeholder.
    }

    // ---- 4. Lists ----

    #[test]
    fn unordered_list() {
        let md = "- First item\n- Second item\n- Third item";
        let msgs = render(md);
        let blocks = &msgs[0].blocks;
        // Should have a rich_text block containing a rich_text_list.
        let rt = blocks
            .iter()
            .find(|b| b["type"] == "rich_text")
            .expect("rich_text block");
        let elements = rt["elements"].as_array().expect("elements");
        let list = elements
            .iter()
            .find(|e| e["type"] == "rich_text_list")
            .expect("rich_text_list");
        assert_eq!(list["style"], "bullet");
        let items = list["elements"].as_array().expect("list items");
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn ordered_list() {
        let md = "1. First\n2. Second\n3. Third";
        let msgs = render(md);
        let blocks = &msgs[0].blocks;
        let rt = blocks
            .iter()
            .find(|b| b["type"] == "rich_text")
            .expect("rich_text block");
        let elements = rt["elements"].as_array().expect("elements");
        let list = elements
            .iter()
            .find(|e| e["type"] == "rich_text_list")
            .expect("rich_text_list");
        assert_eq!(list["style"], "ordered");
    }

    #[test]
    fn nested_list() {
        let md = "- Parent\n  - Child A\n  - Child B\n- Another parent";
        let msgs = render(md);
        let blocks = &msgs[0].blocks;
        let rt = blocks
            .iter()
            .find(|b| b["type"] == "rich_text")
            .expect("rich_text block");
        let elements = rt["elements"].as_array().expect("elements");
        // Should have lists at different indent levels.
        let lists: Vec<&Value> = elements
            .iter()
            .filter(|e| e["type"] == "rich_text_list")
            .collect();
        assert!(
            lists.len() >= 2,
            "expected at least 2 list blocks for nesting"
        );
    }

    #[test]
    fn deeply_nested_list() {
        let md = "- Level 0\n  - Level 1\n    - Level 2\n      - Level 3";
        let msgs = render(md);
        let blocks = &msgs[0].blocks;
        let rt = blocks
            .iter()
            .find(|b| b["type"] == "rich_text")
            .expect("rich_text block");
        let elements = rt["elements"].as_array().expect("elements");
        let lists: Vec<&Value> = elements
            .iter()
            .filter(|e| e["type"] == "rich_text_list")
            .collect();
        // Should have multiple list blocks at different indent levels.
        assert!(
            lists.len() >= 3,
            "expected at least 3 list blocks for deep nesting, got {}",
            lists.len()
        );
        // Verify we have distinct indent levels covering 0-3.
        let mut indents: Vec<u64> = lists
            .iter()
            .filter_map(|l| l.get("indent").and_then(|i| i.as_u64()))
            .collect();
        // Top-level list has no indent key (defaults to 0).
        let top_level_count = lists.iter().filter(|l| l.get("indent").is_none()).count();
        indents.extend(std::iter::repeat_n(0, top_level_count));
        indents.sort();
        indents.dedup();
        assert!(
            indents.len() >= 3,
            "expected at least 3 distinct indent levels, got {:?}",
            indents
        );
    }

    #[test]
    fn mixed_ordered_and_unordered_nested() {
        let md = "1. First ordered\n   - Nested bullet\n   - Another bullet\n2. Second ordered";
        let msgs = render(md);
        let blocks = &msgs[0].blocks;
        let rt = blocks
            .iter()
            .find(|b| b["type"] == "rich_text")
            .expect("rich_text block");
        let elements = rt["elements"].as_array().expect("elements");
        let lists: Vec<&Value> = elements
            .iter()
            .filter(|e| e["type"] == "rich_text_list")
            .collect();
        assert!(
            lists.len() >= 2,
            "expected at least 2 list blocks for mixed nesting"
        );
        let styles: Vec<&str> = lists.iter().filter_map(|l| l["style"].as_str()).collect();
        assert!(styles.contains(&"ordered"), "should have an ordered list");
        assert!(styles.contains(&"bullet"), "should have a bullet list");
    }

    #[test]
    fn single_item_list() {
        let md = "- Only item";
        let msgs = render(md);
        let blocks = &msgs[0].blocks;
        let rt = blocks
            .iter()
            .find(|b| b["type"] == "rich_text")
            .expect("rich_text block");
        let elements = rt["elements"].as_array().expect("elements");
        let list = elements
            .iter()
            .find(|e| e["type"] == "rich_text_list")
            .expect("rich_text_list");
        assert_eq!(list["style"], "bullet");
        let items = list["elements"].as_array().expect("list items");
        assert_eq!(items.len(), 1);
        // Verify the item has content.
        let item_elems = items[0]["elements"].as_array().expect("item elements");
        assert!(!item_elems.is_empty());
    }

    // ---- 5. Links ----

    #[test]
    fn inline_link() {
        let md = "Check the [documentation](https://docs.example.com)";
        let elements = first_rich_text_elements(md);
        let section = &elements[0];
        let elems = section["elements"].as_array().expect("elements");
        let link = elems
            .iter()
            .find(|e| e["type"] == "link")
            .expect("link element");
        assert_eq!(link["url"], "https://docs.example.com");
        assert_eq!(link["text"], "documentation");
    }

    #[test]
    fn multiple_links() {
        let md = "See [A](https://a.com) and [B](https://b.com)";
        let elements = first_rich_text_elements(md);
        let section = &elements[0];
        let elems = section["elements"].as_array().expect("elements");
        let links: Vec<&Value> = elems.iter().filter(|e| e["type"] == "link").collect();
        assert_eq!(links.len(), 2);
    }

    // ---- 6. Blockquotes ----

    #[test]
    fn simple_blockquote() {
        let md = "> This is quoted text";
        let elements = first_rich_text_elements(md);
        let quote = elements
            .iter()
            .find(|e| e["type"] == "rich_text_quote")
            .expect("rich_text_quote");
        let text = &quote["elements"][0];
        assert_eq!(text["text"], "This is quoted text");
    }

    // ---- 7. Horizontal Rules ----

    #[test]
    fn horizontal_rule() {
        let md = "Above\n\n---\n\nBelow";
        let blocks = first_blocks(md);
        let divider = blocks
            .iter()
            .find(|b| b["type"] == "divider")
            .expect("divider block");
        assert_eq!(divider["type"], "divider");
    }

    // ---- 8. Tables ----

    #[test]
    fn simple_table() {
        let md = "| Name | Status |\n|------|--------|\n| App  | Running |\n| DB   | Stopped |";
        let msgs = render(md);
        assert!(!msgs.is_empty());
        let msg = &msgs[0];
        assert!(msg.attachments.is_some(), "table should be in attachments");
        let attachments = msg.attachments.as_ref().expect("attachments");
        let table_wrapper = &attachments[0];
        let table = &table_wrapper["blocks"][0];
        assert_eq!(table["type"], "table");
        let rows = table["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 3); // header + 2 data rows
    }

    #[test]
    fn table_with_alignment() {
        let md = "| Left | Center | Right |\n|:-----|:------:|------:|\n| A    | B      | C     |";
        let msgs = render(md);
        let msg = &msgs[0];
        let attachments = msg.attachments.as_ref().expect("attachments");
        let table = &attachments[0]["blocks"][0];
        let settings = table["column_settings"]
            .as_array()
            .expect("column_settings");
        assert_eq!(settings[0]["align"], "left");
        assert_eq!(settings[1]["align"], "center");
        assert_eq!(settings[2]["align"], "right");
    }

    #[test]
    fn two_tables_split_messages() {
        let md = "## A\n| X | Y |\n|---|---|\n| 1 | 2 |\n\n## B\n| A | B |\n|---|---|\n| 3 | 4 |";
        let msgs = render(md);
        // Each table forces a message split.
        let table_msgs: Vec<&SlackMessage> =
            msgs.iter().filter(|m| m.attachments.is_some()).collect();
        assert_eq!(table_msgs.len(), 2, "expected 2 messages with tables");
    }

    // ---- 9. Fallback Text ----

    #[test]
    fn fallback_strips_formatting() {
        let fallback = generate_fallback_text("**bold** and *italic* and `code`");
        assert!(fallback.contains("bold"));
        assert!(fallback.contains("italic"));
        assert!(fallback.contains("code"));
        assert!(!fallback.contains("**"));
        assert!(!fallback.contains("`"));
    }

    // ---- 10. Edge Cases ----

    #[test]
    fn empty_string() {
        let msgs = render("");
        assert!(msgs.is_empty());
    }

    #[test]
    fn whitespace_only() {
        let msgs = render("   \n\n   ");
        assert!(msgs.is_empty());
    }

    #[test]
    fn unicode_and_emoji() {
        let elements = first_rich_text_elements("**🚀 Deploy** to _production_ 🎉");
        let section = &elements[0];
        let elems = section["elements"].as_array().expect("elements");
        // Bold element should contain the rocket emoji.
        let bold_elem = elems
            .iter()
            .find(|e| e.get("style").is_some_and(|s| s["bold"] == true))
            .expect("bold element");
        let text = bold_elem["text"].as_str().expect("text");
        assert!(text.contains("🚀"));
    }

    #[test]
    fn header_followed_by_paragraph() {
        let md = "# Title\n\nSome content here.";
        let blocks = first_blocks(md);
        assert!(blocks.len() >= 2);
        assert_eq!(blocks[0]["type"], "header");
        assert_eq!(blocks[1]["type"], "rich_text");
    }

    #[test]
    fn code_block_between_paragraphs() {
        let md = "Before code.\n\n```\ncode here\n```\n\nAfter code.";
        let blocks = first_blocks(md);
        // Should have: rich_text (paragraph), rich_text (with preformatted), rich_text (paragraph)
        // or combined into fewer rich_text blocks.
        let rt_blocks: Vec<&Value> = blocks.iter().filter(|b| b["type"] == "rich_text").collect();
        assert!(!rt_blocks.is_empty());
    }

    #[test]
    fn consecutive_code_blocks() {
        let md = "```\nfirst\n```\n\n```\nsecond\n```";
        let msgs = render(md);
        let blocks = &msgs[0].blocks;
        let rt_blocks: Vec<&Value> = blocks.iter().filter(|b| b["type"] == "rich_text").collect();
        // Count preformatted elements across all rich_text blocks.
        let pre_count: usize = rt_blocks
            .iter()
            .filter_map(|b| b["elements"].as_array())
            .flat_map(|elems| elems.iter())
            .filter(|e| e["type"] == "rich_text_preformatted")
            .count();
        assert_eq!(pre_count, 2, "expected 2 preformatted blocks");
    }

    #[test]
    fn horizontal_rule_between_content() {
        let md = "Above\n\n---\n\nBelow";
        let blocks = first_blocks(md);
        let types: Vec<&str> = blocks.iter().filter_map(|b| b["type"].as_str()).collect();
        assert!(types.contains(&"divider"));
        // Should have content before and after the divider.
        let div_idx = types
            .iter()
            .position(|t| *t == "divider")
            .expect("divider index");
        assert!(div_idx > 0, "should have content before divider");
        assert!(
            div_idx < types.len() - 1,
            "should have content after divider"
        );
    }

    #[test]
    fn list_with_inline_formatting() {
        let md = "- **Bold item**\n- Item with `code`\n- Item with [link](https://example.com)";
        let msgs = render(md);
        let blocks = &msgs[0].blocks;
        let rt = blocks
            .iter()
            .find(|b| b["type"] == "rich_text")
            .expect("rich_text block");
        let elements = rt["elements"].as_array().expect("elements");
        let list = elements
            .iter()
            .find(|e| e["type"] == "rich_text_list")
            .expect("rich_text_list");
        let items = list["elements"].as_array().expect("list items");
        assert_eq!(items.len(), 3);
    }

    // ---- 11. Slack API Limit Enforcement ----

    #[test]
    fn fallback_text_truncated_at_limit() {
        // Generate markdown that produces a very long fallback text.
        let long_paragraph = "a".repeat(5_000);
        let md = long_paragraph.to_string();
        let msgs = render(&md);
        assert!(!msgs.is_empty());
        let fallback = &msgs[0].fallback_text;
        assert!(
            fallback.chars().count() <= FALLBACK_TEXT_LIMIT,
            "fallback text should be truncated to {} chars, got {}",
            FALLBACK_TEXT_LIMIT,
            fallback.chars().count()
        );
        assert!(
            fallback.ends_with('…'),
            "truncated fallback should end with ellipsis"
        );
    }

    #[test]
    fn fallback_text_short_not_truncated() {
        let md = "Short message.";
        let msgs = render(md);
        let fallback = &msgs[0].fallback_text;
        assert_eq!(fallback, "Short message.");
        assert!(!fallback.ends_with('…'));
    }

    #[test]
    fn large_code_block_split_into_chunks() {
        // Generate a code block larger than MAX_PREFORMATTED_CHARS.
        let big_code = "x\n".repeat(20_000); // ~40K chars
        let md = format!("```\n{big_code}```");
        let msgs = render(&md);
        // Should produce multiple messages or multiple rich_text blocks.
        let total_pre_count: usize = msgs
            .iter()
            .flat_map(|m| m.blocks.iter())
            .filter(|b| b["type"] == "rich_text")
            .filter_map(|b| b["elements"].as_array())
            .flat_map(|elems| elems.iter())
            .filter(|e| e["type"] == "rich_text_preformatted")
            .count();
        assert!(
            total_pre_count >= 2,
            "expected large code block to be split into multiple preformatted elements, got {total_pre_count}"
        );
    }

    #[test]
    fn table_row_overflow_shows_note() {
        // Generate a table with more than MAX_TABLE_ROWS rows.
        let mut md = String::from("| A | B |\n|---|---|\n");
        for i in 0..110 {
            md.push_str(&format!("| {i} | val |\n"));
        }
        let msgs = render(&md);
        // The table should be truncated and a note should appear.
        let all_blocks: Vec<&Value> = msgs.iter().flat_map(|m| m.blocks.iter()).collect();
        let note_block = all_blocks.iter().find(|b| {
            if b["type"] != "rich_text" {
                return false;
            }
            if let Some(elements) = b["elements"].as_array() {
                elements.iter().any(|e| {
                    if let Some(inner) = e["elements"].as_array() {
                        inner
                            .iter()
                            .any(|el| el["text"].as_str().is_some_and(|t| t.contains("more row")))
                    } else {
                        false
                    }
                })
            } else {
                false
            }
        });
        assert!(
            note_block.is_some(),
            "expected a note about truncated rows when table exceeds 100 rows"
        );

        // Verify the table itself has exactly MAX_TABLE_ROWS rows.
        let table_msgs: Vec<&SlackMessage> =
            msgs.iter().filter(|m| m.attachments.is_some()).collect();
        assert!(!table_msgs.is_empty());
        let table = &table_msgs[0].attachments.as_ref().expect("attachments")[0]["blocks"][0];
        let rows = table["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), MAX_TABLE_ROWS);
    }

    #[test]
    fn message_splitting_respects_50_block_limit() {
        // Generate markdown that produces many blocks (lots of headers + paragraphs).
        let mut md = String::new();
        for i in 0..60 {
            md.push_str(&format!("# Header {i}\n\nParagraph {i}.\n\n"));
        }
        let msgs = render(&md);
        for (i, msg) in msgs.iter().enumerate() {
            assert!(
                msg.blocks.len() <= MAX_BLOCKS_PER_MESSAGE,
                "message {i} has {} blocks, exceeds limit of {MAX_BLOCKS_PER_MESSAGE}",
                msg.blocks.len()
            );
        }
        // Should have been split into multiple messages.
        assert!(
            msgs.len() >= 2,
            "expected multiple messages for 60 headers+paragraphs"
        );
    }

    #[test]
    fn truncate_to_char_limit_preserves_short_text() {
        let result = truncate_to_char_limit("hello", 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn truncate_to_char_limit_truncates_long_text() {
        let result = truncate_to_char_limit("hello world", 6);
        assert_eq!(result, "hello…");
        assert_eq!(result.chars().count(), 6);
    }

    #[test]
    fn truncate_to_char_limit_handles_unicode() {
        let result = truncate_to_char_limit("🚀🎉🔥💯🌟", 3);
        assert_eq!(result, "🚀🎉…");
        assert_eq!(result.chars().count(), 3);
    }

    #[test]
    fn split_text_by_chars_short_text() {
        let chunks = split_text_by_chars("hello", 100);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn split_text_by_chars_splits_at_newlines() {
        let text = "line1\nline2\nline3\nline4";
        let chunks = split_text_by_chars(text, 12);
        assert!(chunks.len() >= 2);
        // Each chunk should be within the limit.
        for chunk in &chunks {
            assert!(chunk.chars().count() <= 12);
        }
    }
}
