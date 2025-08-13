use crate::app::AppState;
use crate::services::confirmation_dialog::render_confirmation_dialog;
use crate::services::helper_block::render_loading_spinner;
use crate::services::helper_dropdown::{render_autocomplete_dropdown, render_helper_dropdown};
use crate::services::hint_helper::render_hint_or_shortcuts;
use crate::services::message::{
    Message, get_wrapped_collapsed_message_lines, get_wrapped_message_lines,
};
use crate::services::message_pattern::{
    process_agent_mode_patterns, process_checkpoint_patterns, spans_to_string,
};
use crate::services::sessions_dialog::render_sessions_dialog;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

const DROPDOWN_MAX_HEIGHT: usize = 8;

pub fn view(f: &mut Frame, state: &AppState) {
    // Calculate the required height for the input area based on content
    let input_area_width = f.area().width.saturating_sub(4) as usize;
    let input_lines = calculate_input_lines(&state.input, input_area_width); // -4 for borders and padding
    let input_height = (input_lines + 2) as u16;
    let margin_height = 2;
    let dropdown_showing = state.show_helper_dropdown
        && ((!state.filtered_helpers.is_empty() && state.input.starts_with('/'))
            || !state.filtered_files.is_empty());
    let dropdown_height = if dropdown_showing {
        if !state.filtered_files.is_empty() {
            DROPDOWN_MAX_HEIGHT as u16
        } else {
            state.filtered_helpers.len() as u16
        }
    } else {
        0
    };
    let hint_height = if state.show_helper_dropdown && !state.is_dialog_open {
        0
    } else {
        margin_height
    };

    let dialog_height = if state.show_sessions_dialog { 11 } else { 0 };
    let dialog_margin = if state.show_sessions_dialog || state.is_dialog_open {
        2
    } else {
        0
    };

    // Layout: [messages][dialog_margin][dialog][input][dropdown][hint]
    let mut constraints = vec![
        Constraint::Min(1), // messages
        Constraint::Length(dialog_margin),
        Constraint::Length(dialog_height),
    ];
    if !state.show_sessions_dialog {
        constraints.push(Constraint::Length(input_height));
        constraints.push(Constraint::Length(dropdown_height));
    }
    constraints.push(Constraint::Length(hint_height)); // Always include hint height (may be 0)
    let chunks = ratatui::layout::Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(f.area());

    let message_area = chunks[0];

    // Create padded message area for content rendering
    let padded_message_area = Rect {
        x: message_area.x + 1,
        y: message_area.y,
        width: message_area.width.saturating_sub(2),
        height: message_area.height,
    };

    let mut input_area = Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };
    let mut dropdown_area = Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };
    let hint_area = chunks.last().copied().unwrap_or(message_area);

    if !state.show_sessions_dialog {
        input_area = chunks[3];
        dropdown_area = chunks.get(4).copied().unwrap_or(input_area);
    }

    let message_area_width = padded_message_area.width as usize;
    let message_area_height = message_area.height as usize;

    render_messages(
        f,
        state,
        padded_message_area,
        message_area_width,
        message_area_height,
    );

    if state.show_collapsed_messages {
        render_collapsed_messages_popup(f, state);
    } else if state.show_sessions_dialog {
        render_sessions_dialog(f, state);
    } else if state.is_dialog_open {
        render_confirmation_dialog(f, state);
    } else {
        render_multiline_input(f, state, input_area);
        render_helper_dropdown(f, state, dropdown_area);
        render_autocomplete_dropdown(f, state, dropdown_area);
    }
    // Render hint/shortcuts if not hiding for dropdown and not showing collapsed messages (unless dialog is open)
    if !state.show_helper_dropdown && !state.show_collapsed_messages {
        render_hint_or_shortcuts(f, state, hint_area);
    }
}

// Calculate how many lines the input will take up when wrapped
fn calculate_input_lines(input: &str, width: usize) -> usize {
    if input.is_empty() {
        return 1; // At least one line
    }
    let prompt_width = 2; // "> " prefix
    let first_line_width = width.saturating_sub(prompt_width);
    let available_width = width;
    if available_width <= 1 {
        return input.len(); // Fallback if width is too small
    }

    // Split by explicit newlines first
    let mut total_lines = 0;
    for line in input.split('\n') {
        // For each line segment after splitting by newlines
        let mut words = line.split_whitespace().peekable();
        let mut current_width = 0;
        let mut is_first_line_in_segment = true;

        while words.peek().is_some() {
            let word = words.next().unwrap_or_default();
            let word_width = word
                .chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
                .sum::<usize>();

            // Determine available width for this line
            let line_width_limit = if is_first_line_in_segment && total_lines == 0 {
                first_line_width
            } else {
                available_width
            };

            // Add space before word (except at start of line)
            if current_width > 0 {
                current_width += 1; // Space width
            }

            // Check if word fits on current line
            if current_width + word_width <= line_width_limit {
                current_width += word_width;
            } else {
                // Word doesn't fit, start new line
                total_lines += 1;
                current_width = word_width;
                is_first_line_in_segment = false;
            }
        }

        total_lines += 1;
    }

    total_lines
}

fn render_messages(f: &mut Frame, state: &AppState, area: Rect, width: usize, height: usize) {
    f.render_widget(ratatui::widgets::Clear, area);
    let mut all_lines: Vec<(Line, Style)> = get_wrapped_message_lines(&state.messages, width);
    if state.loading {
        let loading_line = render_loading_spinner(state);
        all_lines.push((loading_line, Style::default()));
    }

    // Pre-process ALL lines completely and consistently
    let mut processed_lines: Vec<Line> = Vec::new();

    for (i, (line, _style)) in all_lines.iter().enumerate() {
        let line_text = spans_to_string(line);
        let mut should_add_spacing = false;

        // Check if we need spacing before this line (but not for the first line)
        if i > 0 {
            if line_text.contains("<checkpoint_id>") || line_text.contains("<agent_mode>") {
                should_add_spacing = true;
            } else if line_text.contains('<') && line_text.contains('>') {
                // Only add spacing for opening tags, not closing tags
                if let Some(tag_start) = line_text.find('<') {
                    if let Some(tag_end) = line_text.find('>') {
                        let tag_content = &line_text[tag_start + 1..tag_end];
                        if !tag_content.starts_with('/') {
                            // Only opening tags get spacing
                            should_add_spacing = true;
                        }
                    }
                }
            }
        }

        // Add spacing before the line if needed
        if should_add_spacing {
            // Only add spacing if the last line wasn't already empty
            if processed_lines.is_empty()
                || !processed_lines
                    .last()
                    .is_none_or(|last| last.spans.is_empty())
            {
                processed_lines.push(Line::from(""));
            }
        }

        // Process the line and add all resulting lines
        if line_text.contains("<checkpoint_id>") {
            let processed = process_checkpoint_patterns(&[(line.clone(), Style::default())], width);
            for (processed_line, _) in processed {
                processed_lines.push(processed_line);
            }
        } else if line_text.contains("<agent_mode>") {
            let processed = process_agent_mode_patterns(&[(line.clone(), Style::default())]);
            for (processed_line, _) in processed {
                processed_lines.push(processed_line);
            }
        } else if line_text.contains('<') && line_text.contains('>') {
            // Dynamic XML tag processing - extract tag name and process
            if let Some(tag_start) = line_text.find('<') {
                if let Some(tag_end) = line_text.find('>') {
                    let tag_content = &line_text[tag_start + 1..tag_end];

                    if tag_content.starts_with('/') {
                        // Closing tag - just add spacing, don't include the tag
                        // Only add spacing if the last line wasn't already empty
                        if processed_lines.is_empty()
                            || !processed_lines
                                .last()
                                .is_none_or(|last| last.spans.is_empty())
                        {
                            processed_lines.push(Line::from(""));
                        }
                    } else {
                        // Opening tag - create header directly
                        let tag_name = tag_content;
                        let title = tag_name[..1].to_uppercase() + &tag_name[1..].to_lowercase();
                        let header_line = Line::from(vec![ratatui::text::Span::styled(
                            title,
                            ratatui::style::Style::default()
                                .fg(ratatui::style::Color::LightMagenta)
                                .add_modifier(ratatui::style::Modifier::BOLD),
                        )]);
                        processed_lines.push(header_line);
                    }
                } else {
                    processed_lines.push(line.clone());
                }
            } else {
                processed_lines.push(line.clone());
            }
        } else if line_text.trim() == "SPACING_MARKER" {
            processed_lines.push(Line::from(""));
        } else {
            processed_lines.push(line.clone());
        }
    }

    let total_lines = processed_lines.len();

    // Handle edge case where we have no content
    if total_lines == 0 {
        let message_widget =
            Paragraph::new(Vec::<Line>::new()).wrap(ratatui::widgets::Wrap { trim: false });
        f.render_widget(message_widget, area);
        return;
    }

    let max_scroll = total_lines.saturating_sub(height);

    // Prevent snapping by adjusting scroll relative to the processed content
    let original_total = all_lines.len();
    let processed_total = total_lines;

    let scroll = if state.stay_at_bottom {
        max_scroll
    } else {
        // Scale the scroll position to account for processed content size difference
        let adjusted_scroll = if original_total > 0 {
            (state.scroll * processed_total) / original_total
        } else {
            state.scroll
        };
        adjusted_scroll.min(max_scroll)
    };

    // Create visible lines with simple, consistent indexing
    let mut visible_lines = Vec::new();

    for i in 0..height {
        let line_index = scroll + i;
        if line_index < processed_lines.len() {
            visible_lines.push(processed_lines[line_index].clone());
        } else {
            visible_lines.push(Line::from(""));
        }
    }

    // Add a space after the last message if we have content

    let message_widget = Paragraph::new(visible_lines).wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(message_widget, area);
}

fn render_collapsed_messages_popup(f: &mut Frame, state: &AppState) {
    let screen = f.area();

    // Get only collapsed messages
    let collapsed_messages: Vec<&Message> = state
        .messages
        .iter()
        .filter(|m| m.is_collapsed == Some(true))
        .collect();
    // Create a full-screen popup
    let popup_area = Rect {
        x: 0,
        y: 0,
        width: screen.width,
        height: screen.height,
    };

    // Clear the entire screen first to ensure nothing shows through
    f.render_widget(ratatui::widgets::Clear, popup_area);

    // Create a block with title and background
    let block = Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(ratatui::style::Color::LightMagenta))
        .style(ratatui::style::Style::default().bg(ratatui::style::Color::Rgb(31, 32, 44)))
        .title(ratatui::text::Span::styled(
            "Expanded Messages (Ctrl+T to close, Tab to previous message, ↑/↓ to scroll)",
            ratatui::style::Style::default()
                .fg(ratatui::style::Color::LightMagenta)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));

    // Calculate content area (inside borders)
    let content_area = Rect {
        x: popup_area.x + 3,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(6),
        height: popup_area.height.saturating_sub(2),
    };

    // Render the block with background
    f.render_widget(block, popup_area);

    // Render collapsed messages using the same logic as render_messages
    render_collapsed_messages_content(f, state, &collapsed_messages, content_area);
}

fn render_collapsed_messages_content(
    f: &mut Frame,
    state: &AppState,
    messages: &[&Message],
    area: Rect,
) {
    let width = area.width as usize;
    let height = area.height as usize;

    // Convert references to owned messages for get_wrapped_message_lines
    let owned_messages: Vec<Message> = messages.iter().map(|m| (*m).clone()).collect();
    let all_lines: Vec<(Line, Style)> = get_wrapped_collapsed_message_lines(&owned_messages, width);

    if all_lines.is_empty() {
        let empty_widget = Paragraph::new("No collapsed messages found")
            .style(ratatui::style::Style::default().fg(ratatui::style::Color::Gray));
        f.render_widget(empty_widget, area);
        return;
    }

    // Pre-process lines (same as render_messages)
    let mut processed_lines: Vec<Line> = Vec::new();

    for (i, (line, _style)) in all_lines.iter().enumerate() {
        let line_text = spans_to_string(line);
        let mut should_add_spacing = false;

        if i > 0 {
            if line_text.contains("<checkpoint_id>") || line_text.contains("<agent_mode>") {
                should_add_spacing = true;
            } else {
                let section_tags = [
                    "planning",
                    "reasoning",
                    "notes",
                    "progress",
                    "local_context",
                    "todo",
                    "application_analysis",
                    "scratchpad",
                    "report",
                    "current_context",
                    "rulebooks",
                    "current_analysis",
                ];

                for tag in &section_tags {
                    if line_text.contains(&format!("<{}>", tag)) {
                        should_add_spacing = true;
                        break;
                    }
                }
            }
        }

        if should_add_spacing {
            processed_lines.push(Line::from(""));
        }

        // Process the line (simplified version)
        if line_text.trim() == "SPACING_MARKER" {
            processed_lines.push(Line::from(""));
        } else {
            processed_lines.push(line.clone());
        }
    }

    let total_lines = processed_lines.len();
    let max_scroll = total_lines.saturating_sub(height);

    // Use collapsed_messages_scroll for this popup
    let scroll = if state.collapsed_messages_scroll > max_scroll {
        max_scroll
    } else {
        state.collapsed_messages_scroll
    };

    // Create visible lines
    let mut visible_lines = Vec::new();
    for i in 0..height {
        let line_index = scroll + i;
        if line_index < processed_lines.len() {
            visible_lines.push(processed_lines[line_index].clone());
        } else {
            visible_lines.push(Line::from(""));
        }
    }

    let message_widget = Paragraph::new(visible_lines).wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(message_widget, area);
}

fn render_multiline_input(f: &mut Frame, state: &AppState, area: Rect) {
    let area_width = area.width as usize;
    let (lines, _cursor_rendered) = state.render_input(area_width);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if state.show_shell_mode {
            Style::default().fg(Color::Rgb(160, 92, 158))
        } else {
            Style::default().fg(Color::DarkGray)
        });
    let input_widget = Paragraph::new(lines)
        .style(Style::default())
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(input_widget, area);
}
