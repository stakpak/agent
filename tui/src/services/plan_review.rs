//! Plan review mode: full-screen plan viewer with inline comments.
//!
//! Renders plan.md as a scrollable markdown document with:
//! - Left gutter: comment count badges
//! - Main area: plan content with basic markdown styling
//! - Right panel: comment threads for the selected line
//! - Bottom bar: key hints

use crate::app::AppState;
use crate::services::plan_comments::{
    AnchorType, CommentAnchor, CommentAuthor, MatchQuality, PlanComments,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::collections::HashMap;

/// Open the plan review overlay, loading content and comments from disk.
pub fn open_plan_review(state: &mut AppState) {
    let session_dir = std::path::Path::new(".stakpak/session");

    // Load plan content
    let plan_path = crate::services::plan::plan_file_path(session_dir);
    let content = match std::fs::read_to_string(&plan_path) {
        Ok(c) => c,
        Err(_) => {
            // No plan file — show a message instead of opening
            crate::services::helper_block::push_styled_message(
                state,
                " No plan to review yet. The agent hasn't created plan.md.",
                ratatui::style::Color::Yellow,
                "⚠ ",
                ratatui::style::Color::Yellow,
            );
            return;
        }
    };

    // Extract body (skip front matter)
    let body = crate::services::plan::extract_plan_body(&content);

    state.plan_review_content = content.clone();
    state.plan_review_lines = body.lines().map(String::from).collect();
    state.plan_review_scroll = 0;
    state.plan_review_cursor_line = 0;
    state.plan_review_show_comment_modal = false;
    state.plan_review_comment_input.clear();
    state.plan_review_selected_comment = None;

    // Start with empty comments (in-memory only, no persistence)
    state.plan_review_resolved_anchors.clear();
    state.plan_review_comments = None;

    state.show_plan_review = true;
}

/// Close the plan review overlay.
pub fn close_plan_review(state: &mut AppState) {
    state.show_plan_review = false;
    state.plan_review_confirm = None;
}

/// Move cursor up in the plan review.
pub fn cursor_up(state: &mut AppState) {
    if state.plan_review_cursor_line > 0 {
        state.plan_review_cursor_line -= 1;
        ensure_cursor_visible(state);
    }
}

/// Move cursor down in the plan review.
pub fn cursor_down(state: &mut AppState) {
    let max_line = state.plan_review_lines.len().saturating_sub(1);
    if state.plan_review_cursor_line < max_line {
        state.plan_review_cursor_line += 1;
        ensure_cursor_visible(state);
    }
}

/// Scroll up by a page.
pub fn page_up(state: &mut AppState, visible_height: usize) {
    let jump = visible_height.saturating_sub(2); // overlap 2 lines for context
    state.plan_review_cursor_line = state.plan_review_cursor_line.saturating_sub(jump);
    state.plan_review_scroll = state.plan_review_scroll.saturating_sub(jump);
}

/// Scroll down by a page.
pub fn page_down(state: &mut AppState, visible_height: usize) {
    let max_line = state.plan_review_lines.len().saturating_sub(1);
    let jump = visible_height.saturating_sub(2);
    state.plan_review_cursor_line = (state.plan_review_cursor_line + jump).min(max_line);
    let max_scroll = state.plan_review_lines.len().saturating_sub(visible_height);
    state.plan_review_scroll = (state.plan_review_scroll + jump).min(max_scroll);
}

/// Jump to the next line that has comments.
pub fn next_comment(state: &mut AppState) {
    let comment_lines = commented_line_numbers(state);
    if let Some(&next) = comment_lines
        .iter()
        .find(|&&ln| ln > state.plan_review_cursor_line)
    {
        state.plan_review_cursor_line = next;
        ensure_cursor_visible(state);
    } else if let Some(&first) = comment_lines.first() {
        // Wrap around
        state.plan_review_cursor_line = first;
        ensure_cursor_visible(state);
    }
}

/// Jump to the previous line that has comments.
pub fn prev_comment(state: &mut AppState) {
    let comment_lines = commented_line_numbers(state);
    if let Some(&prev) = comment_lines
        .iter()
        .rev()
        .find(|&&ln| ln < state.plan_review_cursor_line)
    {
        state.plan_review_cursor_line = prev;
        ensure_cursor_visible(state);
    } else if let Some(&last) = comment_lines.last() {
        // Wrap around
        state.plan_review_cursor_line = last;
        ensure_cursor_visible(state);
    }
}

/// Ensure the cursor is within the visible scroll window.
fn ensure_cursor_visible(state: &mut AppState) {
    // We don't know the viewport height here, so use a reasonable default.
    // The actual clamping happens in render, but we do basic bounds:
    if state.plan_review_cursor_line < state.plan_review_scroll {
        state.plan_review_scroll = state.plan_review_cursor_line;
    }
    // Upper bound will be clamped during render when we know viewport height
}

/// Get sorted unique line numbers that have comments.
fn commented_line_numbers(state: &AppState) -> Vec<usize> {
    let mut lines: Vec<usize> = state
        .plan_review_resolved_anchors
        .iter()
        .filter(|(_, a)| a.match_quality != MatchQuality::Orphaned)
        .map(|(_, a)| a.line_number)
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// Build a map: line_number → count of comments anchored there.
fn comment_counts_by_line(state: &AppState) -> HashMap<usize, usize> {
    let mut counts: HashMap<usize, usize> = HashMap::new();
    for (_, anchor) in &state.plan_review_resolved_anchors {
        if anchor.match_quality != MatchQuality::Orphaned {
            *counts.entry(anchor.line_number).or_insert(0) += 1;
        }
    }
    counts
}

/// A visual row produced by soft-wrapping a logical line.
struct VisualRow {
    /// Index of the logical line this row belongs to.
    logical_line: usize,
    /// Whether this is the first visual row of the logical line.
    is_first: bool,
    /// The text content for this visual row.
    text: String,
}

/// Pre-wrap all logical lines into visual rows for a given width.
///
/// Wraps at word boundaries (spaces) when possible, falling back to
/// hard breaks only for words longer than the available width.
fn build_visual_rows(lines: &[String], width: usize) -> (Vec<VisualRow>, Vec<usize>) {
    let mut rows: Vec<VisualRow> = Vec::new();
    // first_visual_row[i] = index into `rows` where logical line i starts
    let mut first_visual_row: Vec<usize> = Vec::with_capacity(lines.len());

    let w = width.max(1);

    for (logical_idx, line) in lines.iter().enumerate() {
        first_visual_row.push(rows.len());

        if line.is_empty() {
            rows.push(VisualRow {
                logical_line: logical_idx,
                is_first: true,
                text: String::new(),
            });
            continue;
        }

        let mut remaining = line.as_str();
        let mut is_first = true;

        while !remaining.is_empty() {
            // Count characters, not bytes — handles multi-byte UTF-8 (e.g. →, emoji)
            let char_count = remaining.chars().count();
            if char_count <= w {
                // Fits on one row
                rows.push(VisualRow {
                    logical_line: logical_idx,
                    is_first,
                    text: remaining.to_string(),
                });
                break;
            }

            // Find the byte offset of the w-th character
            let byte_limit = remaining
                .char_indices()
                .nth(w)
                .map(|(idx, _)| idx)
                .unwrap_or(remaining.len());

            // Find the last space at or before that byte offset
            let break_at = remaining[..byte_limit]
                .rfind(' ')
                .map(|pos| pos + 1) // include the space on this row
                .unwrap_or(byte_limit); // no space found — hard break at width

            let (chunk, rest) = remaining.split_at(break_at);
            rows.push(VisualRow {
                logical_line: logical_idx,
                is_first,
                text: chunk.to_string(),
            });
            is_first = false;
            remaining = rest;
        }
    }

    (rows, first_visual_row)
}

/// Render the full-screen plan review overlay.
pub fn render_plan_review(f: &mut Frame, state: &mut AppState, area: Rect) {
    // Clear the area
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            " Plan Review ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 3 || inner.width < 20 {
        return;
    }

    // Layout: [gutter][plan content][comment panel]
    // Bottom row: key hints
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let content_area = vertical[0];
    let hints_area = vertical[1];

    // Horizontal: gutter (5) + plan (60%) + comments (40%)
    let gutter_width = 5u16;
    let remaining = content_area.width.saturating_sub(gutter_width);
    let plan_width = (remaining * 60) / 100;
    let comment_width = remaining.saturating_sub(plan_width);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(gutter_width),
            Constraint::Length(plan_width),
            Constraint::Length(comment_width),
        ])
        .split(content_area);

    let gutter_area = horizontal[0];
    let plan_area = horizontal[1];
    let comment_area = horizontal[2];

    let visible_height = plan_area.height as usize;

    // Build visual rows (soft-wrapped) for the plan width
    let (visual_rows, first_visual_row) =
        build_visual_rows(&state.plan_review_lines, plan_area.width as usize);

    // Convert logical cursor/scroll to visual row space
    let cursor_visual = if state.plan_review_cursor_line < first_visual_row.len() {
        first_visual_row[state.plan_review_cursor_line]
    } else {
        0
    };

    // Scroll is stored in logical lines — convert to visual row for rendering
    let mut scroll_visual = if state.plan_review_scroll < first_visual_row.len() {
        first_visual_row[state.plan_review_scroll]
    } else {
        0
    };

    // Clamp scroll so cursor is visible in visual space
    if cursor_visual >= scroll_visual + visible_height {
        scroll_visual = cursor_visual.saturating_sub(visible_height - 1);
    }
    if cursor_visual < scroll_visual {
        scroll_visual = cursor_visual;
    }

    // Write back logical scroll from visual position
    // Find which logical line owns scroll_visual
    if let Some(row) = visual_rows.get(scroll_visual) {
        state.plan_review_scroll = row.logical_line;
    }

    let comment_counts = comment_counts_by_line(state);

    // Render gutter
    render_gutter(
        f,
        state,
        gutter_area,
        &comment_counts,
        visible_height,
        &visual_rows,
        scroll_visual,
    );

    // Render plan content
    render_plan_content(
        f,
        state,
        plan_area,
        visible_height,
        &visual_rows,
        scroll_visual,
    );

    // Render comment panel
    render_comment_panel(f, state, comment_area);

    // Render key hints
    render_key_hints(f, hints_area);

    // Render comment modal on top (if open)
    render_comment_modal(f, state, area);

    // Render confirmation dialog on top (if open)
    render_confirm_modal(f, state, area);
}
/// Render the left gutter with comment count badges.
fn render_gutter(
    f: &mut Frame,
    state: &AppState,
    area: Rect,
    comment_counts: &HashMap<usize, usize>,
    visible_height: usize,
    visual_rows: &[VisualRow],
    scroll_visual: usize,
) {
    let mut lines: Vec<Line<'_>> = Vec::with_capacity(visible_height);

    for i in 0..visible_height {
        let vrow_idx = scroll_visual + i;
        if vrow_idx >= visual_rows.len() {
            lines.push(Line::from(""));
            continue;
        }

        let vrow = &visual_rows[vrow_idx];
        let logical = vrow.logical_line;
        let is_cursor = logical == state.plan_review_cursor_line;

        // Only show gutter content on the first visual row of a logical line
        if !vrow.is_first {
            if is_cursor {
                lines.push(Line::from(Span::styled(
                    "   · ",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                lines.push(Line::from("     "));
            }
            continue;
        }

        if let Some(&count) = comment_counts.get(&logical) {
            let badge = format!("[{}]", count);
            let style = if is_cursor {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow)
            };
            lines.push(Line::from(Span::styled(format!("{:>4} ", badge), style)));
        } else if is_cursor {
            lines.push(Line::from(Span::styled(
                "   > ",
                Style::default().fg(Color::Cyan),
            )));
        } else {
            lines.push(Line::from("     "));
        }
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

/// Render the plan content with basic markdown styling.
fn render_plan_content(
    f: &mut Frame,
    state: &AppState,
    area: Rect,
    visible_height: usize,
    visual_rows: &[VisualRow],
    scroll_visual: usize,
) {
    let md_style = crate::services::markdown_renderer::MarkdownStyle::adaptive();

    // Pre-compute code block state for all logical lines so we know which lines
    // fall inside fenced code blocks. We only need to scan up to the last visible
    // logical line to keep this cheap.
    let last_visible_logical = visual_rows
        .get(scroll_visual + visible_height)
        .map(|r| r.logical_line)
        .unwrap_or(state.plan_review_lines.len());
    let code_block_map = build_code_block_map(&state.plan_review_lines, last_visible_logical);

    let mut lines: Vec<Line<'_>> = Vec::with_capacity(visible_height);

    for i in 0..visible_height {
        let vrow_idx = scroll_visual + i;
        if vrow_idx >= visual_rows.len() {
            lines.push(Line::from(""));
            continue;
        }

        let vrow = &visual_rows[vrow_idx];
        let logical = vrow.logical_line;
        let is_cursor = logical == state.plan_review_cursor_line;
        let in_code_block = code_block_map.get(logical).copied().unwrap_or(false);

        // Use the original logical line text for block-level markdown detection
        let original_trimmed = state
            .plan_review_lines
            .get(logical)
            .map(|s| s.trim())
            .unwrap_or("");

        // Continuation rows get a small indent to visually distinguish wrapping
        let display_text = if vrow.is_first {
            vrow.text.clone()
        } else {
            format!("  {}", vrow.text)
        };

        // Build styled spans for this row
        let styled_spans =
            style_plan_line(&display_text, original_trimmed, in_code_block, &md_style);

        // Apply cursor highlight as background overlay on each span
        let final_spans = if is_cursor {
            styled_spans
                .into_iter()
                .map(|span| {
                    let mut s = span.style;
                    s = s.bg(Color::DarkGray);
                    Span::styled(span.content, s)
                })
                .collect()
        } else {
            styled_spans
        };

        lines.push(Line::from(final_spans));
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

/// Build a map of logical_line_index → is_inside_code_block.
///
/// Scans lines up to `max_line` (inclusive) tracking fenced code block toggles.
fn build_code_block_map(lines: &[String], max_line: usize) -> Vec<bool> {
    let limit = max_line.min(lines.len());
    let mut map = vec![false; limit];
    let mut in_code_block = false;

    for (i, line) in lines.iter().take(limit).enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            // The fence line itself is styled as code block delimiter
            map[i] = true;
            in_code_block = !in_code_block;
        } else {
            map[i] = in_code_block;
        }
    }
    map
}

/// Render a single plan line into styled spans with inline markdown formatting.
///
/// Block-level context (heading, code block, quote) is determined from the original
/// logical line. Inline formatting (bold, code, italic, links) is parsed from the
/// display text (which may be a wrapped substring).
fn style_plan_line(
    display_text: &str,
    original_trimmed: &str,
    in_code_block: bool,
    md_style: &crate::services::markdown_renderer::MarkdownStyle,
) -> Vec<Span<'static>> {
    // Inside a fenced code block — render as code, no inline parsing
    if in_code_block {
        return vec![Span::styled(
            display_text.to_string(),
            md_style.code_block_style,
        )];
    }

    // Fence delimiter line (``` or ```lang)
    if original_trimmed.starts_with("```") {
        return vec![Span::styled(
            display_text.to_string(),
            Style::default().fg(Color::DarkGray),
        )];
    }

    // Headings — styled whole-line, no inline parsing needed
    if original_trimmed.starts_with('#') {
        let heading_style = heading_style_for(original_trimmed, md_style);
        return vec![Span::styled(display_text.to_string(), heading_style)];
    }

    // Horizontal rules
    if original_trimmed.starts_with("---")
        || original_trimmed.starts_with("***")
        || original_trimmed.starts_with("___")
    {
        return vec![Span::styled(
            display_text.to_string(),
            md_style.separator_style,
        )];
    }

    // Task list items
    if original_trimmed.starts_with("- [x]") || original_trimmed.starts_with("- [X]") {
        return vec![Span::styled(
            display_text.to_string(),
            md_style.task_complete_style,
        )];
    }
    if original_trimmed.starts_with("- [ ]") {
        return vec![Span::styled(
            display_text.to_string(),
            md_style.task_open_style,
        )];
    }

    // Blockquotes — italic base with inline formatting
    if original_trimmed.starts_with("> ") {
        let mut spans = parse_inline_spans(display_text, md_style);
        // Apply quote style (italic + gray) as base, preserving inline overrides
        for span in &mut spans {
            if span.style == md_style.text_style || span.style == Style::default() {
                span.style = md_style.quote_style.add_modifier(Modifier::ITALIC);
            }
        }
        return spans;
    }

    // List items — bullet prefix + inline formatting for content
    if original_trimmed.starts_with("- ") || original_trimmed.starts_with("* ") {
        return parse_inline_spans(display_text, md_style);
    }

    // Numbered list items (e.g. "1. ", "12. ")
    if is_numbered_list(original_trimmed) {
        return parse_inline_spans(display_text, md_style);
    }

    // Regular paragraph — full inline formatting
    parse_inline_spans(display_text, md_style)
}

/// Determine heading style based on heading level.
fn heading_style_for(
    trimmed: &str,
    md_style: &crate::services::markdown_renderer::MarkdownStyle,
) -> Style {
    let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
    match hash_count {
        1 => md_style.h1_style,
        2 => md_style.h2_style,
        3 => md_style.h3_style,
        4 => md_style.h4_style,
        5 => md_style.h5_style,
        6 => md_style.h6_style,
        _ => md_style.h1_style,
    }
}

/// Check if a line starts with a numbered list pattern like "1. " or "12. ".
fn is_numbered_list(trimmed: &str) -> bool {
    let mut chars = trimmed.chars();
    // Must start with a digit
    if !chars.next().is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    // Consume remaining digits
    for c in chars.by_ref() {
        if c == '.' {
            // Next char must be a space
            return chars.next() == Some(' ');
        }
        if !c.is_ascii_digit() {
            return false;
        }
    }
    false
}

/// Parse inline markdown formatting in a text string into styled spans.
///
/// Handles: **bold**, *italic*, `code`, ~~strikethrough~~, and [links](url).
/// Falls back gracefully — unmatched markers are rendered as plain text.
fn parse_inline_spans(
    text: &str,
    md_style: &crate::services::markdown_renderer::MarkdownStyle,
) -> Vec<Span<'static>> {
    if text.is_empty() {
        return vec![Span::raw("")];
    }

    // Quick check: if no formatting markers present, return plain text
    if !has_inline_markers(text) {
        return vec![Span::styled(text.to_string(), md_style.text_style)];
    }

    // Iterate by char_indices — every (byte_offset, char) is a valid boundary.
    for (pos, ch) in text.char_indices() {
        match ch {
            // Bold: **text**
            '*' if text[pos..].starts_with("**") => {
                if let Some(end) = text[pos + 2..].find("**").map(|o| pos + 2 + o) {
                    let mut spans = Vec::new();
                    if pos > 0 {
                        spans.extend(parse_inline_no_bold(&text[..pos], md_style));
                    }
                    spans.push(Span::styled(
                        text[pos + 2..end].to_string(),
                        md_style.bold_style,
                    ));
                    spans.extend(parse_inline_spans(&text[end + 2..], md_style));
                    return spans;
                }
            }
            // Italic: *text* (single *, not **)
            '*' if !text[pos..].starts_with("**") => {
                if let Some(end) = text[pos + 1..].find('*').map(|o| pos + 1 + o) {
                    let mut spans = Vec::new();
                    if pos > 0 {
                        spans.extend(parse_inline_no_bold(&text[..pos], md_style));
                    }
                    spans.push(Span::styled(
                        text[pos + 1..end].to_string(),
                        md_style.italic_style,
                    ));
                    spans.extend(parse_inline_spans(&text[end + 1..], md_style));
                    return spans;
                }
            }
            // Inline code: `text`
            '`' => {
                if let Some(end) = text[pos + 1..].find('`').map(|o| pos + 1 + o) {
                    let mut spans = Vec::new();
                    if pos > 0 {
                        spans.extend(parse_inline_spans(&text[..pos], md_style));
                    }
                    spans.push(Span::styled(
                        text[pos + 1..end].to_string(),
                        md_style.code_style,
                    ));
                    spans.extend(parse_inline_spans(&text[end + 1..], md_style));
                    return spans;
                }
            }
            // Strikethrough: ~~text~~
            '~' if text[pos..].starts_with("~~") => {
                if let Some(end) = text[pos + 2..].find("~~").map(|o| pos + 2 + o) {
                    let mut spans = Vec::new();
                    if pos > 0 {
                        spans.extend(parse_inline_spans(&text[..pos], md_style));
                    }
                    spans.push(Span::styled(
                        text[pos + 2..end].to_string(),
                        md_style.strikethrough_style,
                    ));
                    spans.extend(parse_inline_spans(&text[end + 2..], md_style));
                    return spans;
                }
            }
            // Link: [text](url)
            '[' => {
                if let Some((link_text, _url, end_pos)) = parse_link_at(text, pos) {
                    let mut spans = Vec::new();
                    if pos > 0 {
                        spans.extend(parse_inline_spans(&text[..pos], md_style));
                    }
                    spans.push(Span::styled(link_text.to_string(), md_style.link_style));
                    spans.extend(parse_inline_spans(&text[end_pos..], md_style));
                    return spans;
                }
            }
            _ => {}
        }
    }

    // No formatting found — return as plain text
    vec![Span::styled(text.to_string(), md_style.text_style)]
}

/// Parse inline formatting excluding bold (to avoid infinite recursion when
/// processing text segments between bold markers).
fn parse_inline_no_bold(
    text: &str,
    md_style: &crate::services::markdown_renderer::MarkdownStyle,
) -> Vec<Span<'static>> {
    if text.is_empty() {
        return vec![];
    }

    for (pos, ch) in text.char_indices() {
        if ch == '`'
            && let Some(end) = text[pos + 1..].find('`').map(|o| pos + 1 + o)
        {
            let mut spans = Vec::new();
            if pos > 0 {
                spans.push(Span::styled(text[..pos].to_string(), md_style.text_style));
            }
            spans.push(Span::styled(
                text[pos + 1..end].to_string(),
                md_style.code_style,
            ));
            spans.extend(parse_inline_no_bold(&text[end + 1..], md_style));
            return spans;
        }
    }

    vec![Span::styled(text.to_string(), md_style.text_style)]
}

/// Quick check for any inline formatting markers in text.
fn has_inline_markers(text: &str) -> bool {
    text.contains('*')
        || text.contains('`')
        || text.contains('~')
        || (text.contains('[') && text.contains("]("))
}

/// Try to parse a markdown link at the given position: [text](url)
/// All slicing uses positions from `str::find()` — always valid char boundaries.
/// Returns (link_text, url, end_position_after_closing_paren).
fn parse_link_at(text: &str, start: usize) -> Option<(&str, &str, usize)> {
    // All markers ([, ], (, )) are ASCII = 1 byte, so +1 is always a valid boundary.
    if !text[start..].starts_with('[') {
        return None;
    }
    let after_bracket = start + 1;
    let close_bracket = text[after_bracket..].find(']').map(|o| after_bracket + o)?;
    let link_text = &text[after_bracket..close_bracket];

    // Must be immediately followed by (
    let paren_start = close_bracket + 1;
    if !text[paren_start..].starts_with('(') {
        return None;
    }
    let url_start = paren_start + 1;
    let close_paren = text[url_start..].find(')').map(|o| url_start + o)?;
    let url = &text[url_start..close_paren];

    Some((link_text, url, close_paren + 1))
}

/// Render the right panel showing comments for the current cursor line.
fn render_comment_panel(f: &mut Frame, state: &AppState, area: Rect) {
    let cursor_line = state.plan_review_cursor_line;

    // Find comments anchored to this line
    let comment_ids: Vec<&str> = state
        .plan_review_resolved_anchors
        .iter()
        .filter(|(_, a)| a.line_number == cursor_line && a.match_quality != MatchQuality::Orphaned)
        .map(|(id, _)| id.as_str())
        .collect();

    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            " Comments ",
            Style::default().fg(Color::DarkGray),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if comment_ids.is_empty() {
        let no_comments = Paragraph::new(Line::from(Span::styled(
            " No comments on this line",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
        f.render_widget(no_comments, inner);
        return;
    }

    let Some(ref pc) = state.plan_review_comments else {
        return;
    };

    let mut lines: Vec<Line<'_>> = Vec::new();

    for &cid in &comment_ids {
        if let Some(comment) = pc.comments.iter().find(|c| c.id == cid) {
            // Comment header
            let author_label = match comment.author {
                CommentAuthor::User => "You",
                CommentAuthor::Agent => "Agent",
            };
            let resolved_mark = if comment.resolved { " ✓" } else { "" };
            let match_info = state
                .plan_review_resolved_anchors
                .iter()
                .find(|(id, _)| id == cid)
                .map(|(_, a)| match &a.match_quality {
                    MatchQuality::Exact => "",
                    MatchQuality::Fuzzy(_) => " ~shifted",
                    MatchQuality::Orphaned => " ⚠orphaned",
                })
                .unwrap_or("");

            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {}{}", author_label, resolved_mark),
                    Style::default()
                        .fg(if comment.resolved {
                            Color::DarkGray
                        } else {
                            Color::Yellow
                        })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(match_info.to_string(), Style::default().fg(Color::DarkGray)),
            ]));

            // Comment text
            let text_style = if comment.resolved {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(
                format!(" {}", comment.text),
                text_style,
            )));

            lines.push(Line::from("")); // spacing
        }
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, inner);
}

// ─── Confirmation Modal ──────────────────────────────────────────────────────

/// Render a lightweight confirmation dialog.
fn render_confirm_modal(f: &mut Frame, state: &AppState, area: Rect) {
    let Some(ref action) = state.plan_review_confirm else {
        return;
    };

    let (title, message, color) = match action {
        ConfirmAction::Approve => (
            " Approve Plan ",
            "Approve this plan and start execution?".to_string(),
            Color::Green,
        ),
        ConfirmAction::Feedback { count } => (
            " Submit Feedback ",
            format!(
                "Submit {} comment{} as feedback?",
                count,
                if *count == 1 { "" } else { "s" }
            ),
            Color::Yellow,
        ),
        ConfirmAction::DeleteComments { comment_ids, .. } => {
            let n = comment_ids.len();
            (
                " Delete Comments ",
                format!(
                    "Delete {} comment{} on this line?",
                    n,
                    if n == 1 { "" } else { "s" }
                ),
                Color::Red,
            )
        }
    };

    let modal_width = 50u16.min(area.width.saturating_sub(4));

    let lines: Vec<Line<'_>> = vec![
        Line::from(""),
        Line::from(Span::styled(message, Style::default().fg(Color::White))),
        Line::from(""),
        Line::from(vec![
            Span::styled("y", Style::default().fg(Color::Green)),
            Span::styled("/", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter", Style::default().fg(Color::Green)),
            Span::styled("=confirm  ", Style::default().fg(Color::DarkGray)),
            Span::styled("n", Style::default().fg(Color::Red)),
            Span::styled("/", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc", Style::default().fg(Color::Red)),
            Span::styled("=cancel", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let content_lines = lines.len() as u16;
    let modal_height = (content_lines + 2)
        .min(area.height.saturating_sub(4))
        .max(4);

    let x = area.x + (area.width - modal_width) / 2;
    let y = area.y + (area.height - modal_height) / 2;
    let modal_area = Rect::new(x, y, modal_width, modal_height);

    f.render_widget(Clear, modal_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color))
        .title(Span::styled(
            title,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(modal_area);
    f.render_widget(block, modal_area);

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, inner);
}

// ─── Feedback & Approval ─────────────────────────────────────────────────────

/// Format all unresolved comments into a structured feedback message.
///
/// Each comment includes its anchor text (the exact heading or line it's attached to)
/// so the agent knows which part of the plan to revise.
/// Skips resolved comments.
pub fn format_feedback_message(comments: &PlanComments, _plan_content: &str) -> Option<String> {
    // Only unresolved comments
    let unresolved: Vec<_> = comments.comments.iter().filter(|c| !c.resolved).collect();

    if unresolved.is_empty() {
        return None;
    }

    let mut output =
        String::from("I've reviewed the plan and have feedback on specific sections:\n\n");

    // Group comments by anchor text so we don't repeat the same anchor header
    let mut grouped: Vec<(&str, Vec<&str>)> = Vec::new();
    for comment in &unresolved {
        let anchor_text = comment.anchor.text.as_str();
        if let Some(group) = grouped.iter_mut().find(|(a, _)| *a == anchor_text) {
            group.1.push(comment.text.as_str());
        } else {
            grouped.push((anchor_text, vec![comment.text.as_str()]));
        }
    }

    for (anchor_text, texts) in &grouped {
        output.push_str(&format!("> On: `{}`\n", anchor_text));
        for text in texts {
            output.push_str(&format!("- {}\n", text));
        }
        output.push('\n');
    }

    output.push_str(
        "Please revise the plan to address this feedback, then set status back to `pending_review`.\n",
    );

    Some(output)
}

/// Handle the feedback action ('f' key).
///
/// Formats unresolved comments and sends as OutputEvent::PlanFeedback.
/// Returns the feedback message if there were unresolved comments.
pub fn handle_feedback(
    state: &mut AppState,
    output_tx: &tokio::sync::mpsc::Sender<crate::app::OutputEvent>,
) {
    let Some(ref pc) = state.plan_review_comments else {
        return;
    };

    let Some(feedback) = format_feedback_message(pc, &state.plan_review_content) else {
        // No unresolved comments — show message
        crate::services::helper_block::push_styled_message(
            state,
            " No unresolved comments. Approve or add comments first.",
            ratatui::style::Color::Yellow,
            "⚠ ",
            ratatui::style::Color::Yellow,
        );
        return;
    };

    // Close review
    close_plan_review(state);

    // Send feedback
    let _ = output_tx.try_send(crate::app::OutputEvent::PlanFeedback(feedback));
}

/// Handle the approve action ('a' key).
///
/// Sends OutputEvent::PlanApproved and closes the review.
/// The agent is responsible for updating plan.md front matter to `status: approved`.
pub fn handle_approve(
    state: &mut AppState,
    output_tx: &tokio::sync::mpsc::Sender<crate::app::OutputEvent>,
) {
    // Close review
    close_plan_review(state);

    // Send approval event — the agent will update plan.md status to approved
    let _ = output_tx.try_send(crate::app::OutputEvent::PlanApproved);
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::plan_comments::{
        AnchorType, CommentAnchor, CommentAuthor, PlanComment, PlanComments,
    };
    use chrono::Utc;

    const TEST_PLAN: &str = "\
---
title: Deploy Auth Service
status: pending_review
version: 2
---

# Deploy Auth Service

## Overview

Implement OAuth-based authentication.

## Step 1: Database

Use PostgreSQL on RDS.

## Step 2: Endpoints

Build login and refresh endpoints.
";

    fn make_plan_comment(
        id: &str,
        anchor_type: AnchorType,
        anchor_text: &str,
        text: &str,
        resolved: bool,
    ) -> PlanComment {
        PlanComment {
            id: id.to_string(),
            anchor: CommentAnchor {
                anchor_type,
                text: anchor_text.to_string(),
            },
            author: CommentAuthor::User,
            text: text.to_string(),
            created_at: Utc::now(),
            resolved,
        }
    }

    fn make_plan_comments(comments: Vec<PlanComment>) -> PlanComments {
        PlanComments {
            plan_file: "plan.md".to_string(),
            plan_hash: crate::services::plan::compute_plan_hash(TEST_PLAN),
            comments,
        }
    }

    #[test]
    fn test_format_feedback_basic() {
        let pc = make_plan_comments(vec![make_plan_comment(
            "cmt_01",
            AnchorType::Heading,
            "## Step 1: Database",
            "Should we use Aurora?",
            false,
        )]);

        let feedback = format_feedback_message(&pc, TEST_PLAN);
        assert!(feedback.is_some());
        let msg = feedback.unwrap();
        assert!(msg.contains("feedback on specific sections"));
        assert!(msg.contains("> On: `## Step 1: Database`"));
        assert!(msg.contains("Should we use Aurora?"));
        assert!(msg.contains("set status back to `pending_review`"));
    }

    #[test]
    fn test_format_feedback_skips_resolved() {
        let pc = make_plan_comments(vec![make_plan_comment(
            "cmt_01",
            AnchorType::Heading,
            "## Step 1: Database",
            "Resolved issue",
            true, // resolved
        )]);

        let feedback = format_feedback_message(&pc, TEST_PLAN);
        assert!(feedback.is_none()); // All resolved → no feedback
    }

    #[test]
    fn test_format_feedback_groups_by_heading() {
        let pc = make_plan_comments(vec![
            make_plan_comment(
                "cmt_01",
                AnchorType::Heading,
                "## Step 1: Database",
                "Consider Aurora",
                false,
            ),
            make_plan_comment(
                "cmt_02",
                AnchorType::Heading,
                "## Step 2: Endpoints",
                "Add rate limiting",
                false,
            ),
        ]);

        let feedback = format_feedback_message(&pc, TEST_PLAN);
        assert!(feedback.is_some());
        let msg = feedback.unwrap();
        // Should reference both anchors
        assert!(msg.contains("> On: `## Step 1: Database`"));
        assert!(msg.contains("Consider Aurora"));
        assert!(msg.contains("> On: `## Step 2: Endpoints`"));
        assert!(msg.contains("Add rate limiting"));
    }

    #[test]
    fn test_format_feedback_groups_same_anchor() {
        let pc = make_plan_comments(vec![
            make_plan_comment(
                "cmt_01",
                AnchorType::Heading,
                "## Step 1: Database",
                "Consider Aurora",
                false,
            ),
            make_plan_comment(
                "cmt_02",
                AnchorType::Heading,
                "## Step 1: Database",
                "Add read replicas",
                false,
            ),
        ]);

        let feedback = format_feedback_message(&pc, TEST_PLAN);
        assert!(feedback.is_some());
        let msg = feedback.unwrap();
        // Anchor should appear only once
        assert_eq!(
            msg.matches("> On: `## Step 1: Database`").count(),
            1,
            "Same anchor should not be repeated. Got:\n{}",
            msg
        );
        assert!(msg.contains("Consider Aurora"));
        assert!(msg.contains("Add read replicas"));
    }

    #[test]
    fn test_format_feedback_empty_comments() {
        let pc = make_plan_comments(vec![]);
        let feedback = format_feedback_message(&pc, TEST_PLAN);
        assert!(feedback.is_none());
    }

    #[test]
    fn test_format_feedback_mixed_resolved_unresolved() {
        let pc = make_plan_comments(vec![
            make_plan_comment(
                "cmt_01",
                AnchorType::Heading,
                "## Overview",
                "Resolved thing",
                true,
            ),
            make_plan_comment(
                "cmt_02",
                AnchorType::Heading,
                "## Step 1: Database",
                "Open issue",
                false,
            ),
        ]);

        let feedback = format_feedback_message(&pc, TEST_PLAN);
        assert!(feedback.is_some());
        let msg = feedback.unwrap();
        assert!(!msg.contains("Resolved thing"));
        assert!(msg.contains("Open issue"));
    }

    // ─── Inline Markdown Rendering Tests ─────────────────────────────────────

    fn md_style() -> crate::services::markdown_renderer::MarkdownStyle {
        crate::services::markdown_renderer::MarkdownStyle::adaptive()
    }

    fn spans_text(spans: &[Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn test_parse_inline_plain_text() {
        let style = md_style();
        let spans = parse_inline_spans("hello world", &style);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans_text(&spans), "hello world");
    }

    #[test]
    fn test_parse_inline_bold() {
        let style = md_style();
        let spans = parse_inline_spans("before **bold** after", &style);
        assert_eq!(spans_text(&spans), "before bold after");
        // The bold span should have bold modifier
        let bold_span = spans.iter().find(|s| s.content.as_ref() == "bold");
        assert!(bold_span.is_some());
        assert!(
            bold_span
                .unwrap()
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn test_parse_inline_italic() {
        let style = md_style();
        let spans = parse_inline_spans("before *italic* after", &style);
        assert_eq!(spans_text(&spans), "before italic after");
        let italic_span = spans.iter().find(|s| s.content.as_ref() == "italic");
        assert!(italic_span.is_some());
        assert!(
            italic_span
                .unwrap()
                .style
                .add_modifier
                .contains(Modifier::ITALIC)
        );
    }

    #[test]
    fn test_parse_inline_code() {
        let style = md_style();
        let spans = parse_inline_spans("use `kubectl apply`", &style);
        assert_eq!(spans_text(&spans), "use kubectl apply");
        let code_span = spans.iter().find(|s| s.content.as_ref() == "kubectl apply");
        assert!(code_span.is_some());
        assert_eq!(code_span.unwrap().style, style.code_style);
    }

    #[test]
    fn test_parse_inline_strikethrough() {
        let style = md_style();
        let spans = parse_inline_spans("~~removed~~ kept", &style);
        assert_eq!(spans_text(&spans), "removed kept");
        let strike_span = spans.iter().find(|s| s.content.as_ref() == "removed");
        assert!(strike_span.is_some());
        assert!(
            strike_span
                .unwrap()
                .style
                .add_modifier
                .contains(Modifier::CROSSED_OUT)
        );
    }

    #[test]
    fn test_parse_inline_link() {
        let style = md_style();
        let spans = parse_inline_spans("see [docs](https://example.com) here", &style);
        assert_eq!(spans_text(&spans), "see docs here");
        let link_span = spans.iter().find(|s| s.content.as_ref() == "docs");
        assert!(link_span.is_some());
        assert_eq!(link_span.unwrap().style, style.link_style);
    }

    #[test]
    fn test_parse_inline_multiple_formats() {
        let style = md_style();
        let spans = parse_inline_spans("**bold** and `code`", &style);
        assert_eq!(spans_text(&spans), "bold and code");
    }

    #[test]
    fn test_parse_inline_unmatched_markers() {
        let style = md_style();
        // Single * without closing should be plain text
        let spans = parse_inline_spans("hello * world", &style);
        // The * finds "world" as italic content — that's fine, it's greedy
        // Just verify no panic
        assert!(!spans.is_empty());
    }

    #[test]
    fn test_parse_inline_empty() {
        let style = md_style();
        let spans = parse_inline_spans("", &style);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans_text(&spans), "");
    }

    #[test]
    fn test_parse_inline_unicode() {
        let style = md_style();
        // Emoji and multi-byte chars should not panic
        let spans = parse_inline_spans("→ **héllo** wörld 🚀 `cödé`", &style);
        let text = spans_text(&spans);
        assert!(text.contains("héllo"));
        assert!(text.contains("wörld"));
        assert!(text.contains("🚀"));
        assert!(text.contains("cödé"));
    }

    #[test]
    fn test_parse_inline_unicode_around_markers() {
        let style = md_style();
        // Markers surrounded by multi-byte chars
        let spans = parse_inline_spans("日本語**太字**テスト", &style);
        let text = spans_text(&spans);
        assert!(text.contains("太字"));
        assert!(text.contains("日本語"));
        assert!(text.contains("テスト"));
    }

    #[test]
    fn test_parse_inline_emoji_markers() {
        let style = md_style();
        // Emoji right next to markers
        let spans = parse_inline_spans("🔥`hot`🔥", &style);
        let text = spans_text(&spans);
        assert!(text.contains("hot"));
        assert!(text.contains("🔥"));
    }

    #[test]
    fn test_build_code_block_map_basic() {
        let lines: Vec<String> = vec![
            "normal".into(),
            "```rust".into(),
            "let x = 1;".into(),
            "```".into(),
            "after".into(),
        ];
        let map = build_code_block_map(&lines, 5);
        assert!(!map[0]); // normal
        assert!(map[1]); // ``` opening fence
        assert!(map[2]); // inside code block
        assert!(map[3]); // ``` closing fence
        assert!(!map[4]); // after
    }

    #[test]
    fn test_build_code_block_map_empty() {
        let lines: Vec<String> = vec![];
        let map = build_code_block_map(&lines, 0);
        assert!(map.is_empty());
    }

    #[test]
    fn test_build_code_block_map_nested_fences() {
        let lines: Vec<String> = vec![
            "```".into(),
            "code".into(),
            "```".into(),
            "gap".into(),
            "```".into(),
            "more code".into(),
            "```".into(),
        ];
        let map = build_code_block_map(&lines, 7);
        assert!(map[0]); // opening
        assert!(map[1]); // inside
        assert!(map[2]); // closing
        assert!(!map[3]); // gap
        assert!(map[4]); // opening
        assert!(map[5]); // inside
        assert!(map[6]); // closing
    }

    #[test]
    fn test_style_plan_line_heading_levels() {
        let style = md_style();
        let spans = style_plan_line("# H1", "# H1", false, &style);
        assert_eq!(spans[0].style, style.h1_style);

        let spans = style_plan_line("## H2", "## H2", false, &style);
        assert_eq!(spans[0].style, style.h2_style);

        let spans = style_plan_line("### H3", "### H3", false, &style);
        assert_eq!(spans[0].style, style.h3_style);
    }

    #[test]
    fn test_style_plan_line_code_block() {
        let style = md_style();
        let spans = style_plan_line("let x = 1;", "let x = 1;", true, &style);
        assert_eq!(spans[0].style, style.code_block_style);
    }

    #[test]
    fn test_style_plan_line_fence_delimiter() {
        let style = md_style();
        let spans = style_plan_line("```rust", "```rust", false, &style);
        assert_eq!(spans[0].style, Style::default().fg(Color::DarkGray));
    }

    #[test]
    fn test_style_plan_line_task_items() {
        let style = md_style();
        let spans = style_plan_line("- [x] done", "- [x] done", false, &style);
        assert_eq!(spans[0].style, style.task_complete_style);

        let spans = style_plan_line("- [ ] todo", "- [ ] todo", false, &style);
        assert_eq!(spans[0].style, style.task_open_style);
    }

    #[test]
    fn test_style_plan_line_horizontal_rule() {
        let style = md_style();
        let spans = style_plan_line("---", "---", false, &style);
        assert_eq!(spans[0].style, style.separator_style);
    }

    #[test]
    fn test_is_numbered_list() {
        assert!(is_numbered_list("1. First"));
        assert!(is_numbered_list("12. Twelfth"));
        assert!(is_numbered_list("999. Big"));
        assert!(!is_numbered_list("not a list"));
        assert!(!is_numbered_list("1.no space"));
        assert!(!is_numbered_list(""));
        assert!(!is_numbered_list("1."));
    }

    #[test]
    fn test_parse_link_at_basic() {
        let (text, url, end) = parse_link_at("[click](https://x.com)", 0).unwrap();
        assert_eq!(text, "click");
        assert_eq!(url, "https://x.com");
        assert_eq!(end, 22);
    }

    #[test]
    fn test_parse_link_at_offset() {
        let input = "see [link](url) here";
        let (text, url, end) = parse_link_at(input, 4).unwrap();
        assert_eq!(text, "link");
        assert_eq!(url, "url");
        assert_eq!(end, 15);
    }

    #[test]
    fn test_parse_link_at_no_link() {
        assert!(parse_link_at("no link here", 0).is_none());
        assert!(parse_link_at("[unclosed", 0).is_none());
        assert!(parse_link_at("[text]no-paren", 0).is_none());
    }

    #[test]
    fn test_parse_link_at_unicode() {
        let (text, _url, _end) = parse_link_at("[日本語](https://jp.com)", 0).unwrap();
        assert_eq!(text, "日本語");
    }
}

/// Render the bottom key hints bar.
fn render_key_hints(f: &mut Frame, area: Rect) {
    let hints = Line::from(vec![
        Span::styled(" c", Style::default().fg(Color::Cyan)),
        Span::styled("=comment ", Style::default().fg(Color::DarkGray)),
        Span::styled("d", Style::default().fg(Color::Red)),
        Span::styled("=delete ", Style::default().fg(Color::DarkGray)),
        Span::styled("Enter", Style::default().fg(Color::Green)),
        Span::styled("=submit ", Style::default().fg(Color::DarkGray)),
        Span::styled("Tab", Style::default().fg(Color::Cyan)),
        Span::styled("=next ", Style::default().fg(Color::DarkGray)),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::styled("=close", Style::default().fg(Color::DarkGray)),
    ]);

    let paragraph = Paragraph::new(hints);
    f.render_widget(paragraph, area);
}

// ─── Comment Modal ───────────────────────────────────────────────────────────

/// The kind of comment action the modal is for.
#[derive(Debug, Clone, PartialEq)]
pub enum CommentModalKind {
    /// New top-level comment anchored to a line.
    NewComment { anchor_text: String },
}

/// Confirmation dialog variants for destructive/important actions.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    /// Approve the plan.
    Approve,
    /// Submit feedback (N unresolved comments).
    Feedback { count: usize },
    /// Delete all comments on a specific logical line.
    DeleteComments {
        line: usize,
        comment_ids: Vec<String>,
    },
}

/// Open the comment modal for a new comment on the current cursor line.
pub fn open_comment_modal(state: &mut AppState) {
    if state.plan_review_lines.is_empty() {
        return;
    }

    let cursor = state.plan_review_cursor_line;
    let anchor_text = state
        .plan_review_lines
        .get(cursor)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    if anchor_text.is_empty() {
        return; // Don't comment on blank lines
    }

    state.plan_review_comment_input.clear();
    state.plan_review_show_comment_modal = true;
    state.plan_review_selected_comment = None; // new comment, not a reply
    state.plan_review_modal_kind = Some(CommentModalKind::NewComment { anchor_text });
}

/// Submit the comment from the modal.
///
/// Adds comment to in-memory state and refreshes resolved anchors.
pub fn submit_comment(state: &mut AppState) {
    let text = state.plan_review_comment_input.trim().to_string();
    if text.is_empty() {
        return;
    }

    // Initialize PlanComments if it doesn't exist yet
    let mut pc = state
        .plan_review_comments
        .take()
        .unwrap_or_else(|| PlanComments {
            plan_file: "plan.md".to_string(),
            plan_hash: crate::services::plan::compute_plan_hash(&state.plan_review_content),
            comments: Vec::new(),
        });

    match &state.plan_review_modal_kind {
        Some(CommentModalKind::NewComment { anchor_text }) => {
            let anchor_type = if anchor_text.starts_with('#') {
                AnchorType::Heading
            } else {
                AnchorType::Line
            };
            crate::services::plan_comments::add_comment(
                &mut pc,
                CommentAnchor {
                    anchor_type,
                    text: anchor_text.clone(),
                },
                CommentAuthor::User,
                text,
            );
        }
        None => return,
    }

    // Refresh anchors
    let body = crate::services::plan::extract_plan_body(&state.plan_review_content);
    state.plan_review_resolved_anchors =
        crate::services::plan_comments::resolve_anchors(body, &pc.comments);

    state.plan_review_comments = Some(pc);
    close_comment_modal(state);
}

/// Unified submit: if comments exist → feedback, otherwise → approve.
pub fn open_submit_confirm(state: &mut AppState) {
    let unresolved_count = state
        .plan_review_comments
        .as_ref()
        .map(|pc| pc.comments.iter().filter(|c| !c.resolved).count())
        .unwrap_or(0);

    if unresolved_count > 0 {
        state.plan_review_confirm = Some(ConfirmAction::Feedback {
            count: unresolved_count,
        });
    } else {
        state.plan_review_confirm = Some(ConfirmAction::Approve);
    }
}

/// Open confirmation dialog for deleting comments on the current line.
pub fn open_delete_confirm(state: &mut AppState) {
    let cursor = state.plan_review_cursor_line;

    // Collect all comment IDs anchored to this line
    let comment_ids: Vec<String> = state
        .plan_review_resolved_anchors
        .iter()
        .filter(|(_, a)| a.line_number == cursor && a.match_quality != MatchQuality::Orphaned)
        .map(|(id, _)| id.clone())
        .collect();

    if comment_ids.is_empty() {
        return; // No comments on this line
    }

    state.plan_review_confirm = Some(ConfirmAction::DeleteComments {
        line: cursor,
        comment_ids,
    });
}

/// Execute the confirmed action and close the dialog.
pub fn execute_confirm(
    state: &mut AppState,
    output_tx: &tokio::sync::mpsc::Sender<crate::app::OutputEvent>,
) {
    let Some(action) = state.plan_review_confirm.take() else {
        return;
    };

    match action {
        ConfirmAction::Approve => {
            handle_approve(state, output_tx);
        }
        ConfirmAction::Feedback { .. } => {
            handle_feedback(state, output_tx);
        }
        ConfirmAction::DeleteComments { comment_ids, .. } => {
            let Some(ref mut pc) = state.plan_review_comments else {
                return;
            };

            // Remove all matching comments
            pc.comments.retain(|c| !comment_ids.contains(&c.id));

            // Refresh resolved anchors
            let body = crate::services::plan::extract_plan_body(&state.plan_review_content);
            state.plan_review_resolved_anchors =
                crate::services::plan_comments::resolve_anchors(body, &pc.comments);
        }
    }
}

/// Close the comment modal without saving.
pub fn close_comment_modal(state: &mut AppState) {
    state.plan_review_show_comment_modal = false;
    state.plan_review_comment_input.clear();
    state.plan_review_selected_comment = None;
    state.plan_review_modal_kind = None;
}

/// Handle a character input in the comment modal.
pub fn modal_input_char(state: &mut AppState, c: char) {
    if state.plan_review_show_comment_modal {
        state.plan_review_comment_input.push(c);
    }
}

/// Handle backspace in the comment modal.
pub fn modal_input_backspace(state: &mut AppState) {
    if state.plan_review_show_comment_modal {
        state.plan_review_comment_input.pop();
    }
}

/// Handle newline in the comment modal (Enter key adds newline).
pub fn modal_input_newline(state: &mut AppState) {
    if state.plan_review_show_comment_modal {
        state.plan_review_comment_input.push('\n');
    }
}

/// Render the comment modal overlay.
pub fn render_comment_modal(f: &mut Frame, state: &AppState, area: Rect) {
    if !state.plan_review_show_comment_modal {
        return;
    }

    // Center modal: 60% width, dynamic height based on content
    let modal_width = (area.width * 60 / 100).max(40).min(area.width - 4);

    // Pre-compute lines to determine height
    let mut lines: Vec<Line<'_>> = Vec::new();

    let title = " Add Comment ";

    // Anchor preview
    if let Some(CommentModalKind::NewComment { anchor_text }) = &state.plan_review_modal_kind {
        let max_chars = (modal_width as usize).saturating_sub(17);
        let display: String = anchor_text.chars().take(max_chars).collect();
        lines.push(Line::from(vec![
            Span::styled("On: ", Style::default().fg(Color::DarkGray)),
            Span::styled(display, Style::default().fg(Color::White)),
        ]));
        lines.push(Line::from("")); // padding below anchor
    }

    // Input area with inline cursor
    let input_text = &state.plan_review_comment_input;
    if input_text.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::DarkGray)),
            Span::styled("█", Style::default().fg(Color::Cyan)),
            Span::styled(
                " Type your comment...",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]));
    } else {
        let input_lines: Vec<&str> = input_text.lines().collect();
        let last_idx = input_lines.len().saturating_sub(1);
        // If text ends with newline, there's an implicit empty line after
        let trailing_newline = input_text.ends_with('\n');

        for (i, input_line) in input_lines.iter().enumerate() {
            if i == last_idx && !trailing_newline {
                // Last line — append cursor
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("> {}", input_line),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled("█", Style::default().fg(Color::Cyan)),
                ]));
            } else {
                lines.push(Line::from(Span::styled(
                    format!("> {}", input_line),
                    Style::default().fg(Color::White),
                )));
            }
        }
        if trailing_newline {
            lines.push(Line::from(vec![
                Span::styled("> ", Style::default().fg(Color::White)),
                Span::styled("█", Style::default().fg(Color::Cyan)),
            ]));
        }
    }

    // Hints
    lines.push(Line::from("")); // padding above hints
    lines.push(Line::from(vec![
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::styled("=submit  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Ctrl+J", Style::default().fg(Color::Cyan)),
        Span::styled("=newline  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::styled("=cancel", Style::default().fg(Color::DarkGray)),
    ]));

    // Dynamic height: content lines + 2 for border
    let content_lines = lines.len() as u16;
    let modal_height = (content_lines + 2).min(area.height - 4).max(4);

    let x = area.x + (area.width - modal_width) / 2;
    let y = area.y + (area.height - modal_height) / 2;
    let modal_area = Rect::new(x, y, modal_width, modal_height);

    f.render_widget(Clear, modal_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(modal_area);
    f.render_widget(block, modal_area);

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, inner);
}
