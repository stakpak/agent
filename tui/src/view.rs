use crate::app::AppState;
use crate::services::confirmation_dialog::render_confirmation_dialog;
use crate::services::helper_block::render_loading_spinner;
use crate::services::helper_dropdown::{render_autocomplete_dropdown, render_helper_dropdown};
use crate::services::hint_helper::render_hint_or_shortcuts;
use crate::services::message::get_wrapped_message_lines;
use crate::services::message_pattern::{
    process_agent_mode_patterns, process_checkpoint_patterns, process_section_title_patterns,
    spans_to_string,
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
        && ((state.autocomplete.is_active() && !state.autocomplete.filtered_files.is_empty())
            || (!state.autocomplete.is_active()
                && !state.filtered_helpers.is_empty()
                && state.input.starts_with('/')));
    let dropdown_height = if dropdown_showing {
        if state.autocomplete.is_active() {
            DROPDOWN_MAX_HEIGHT as u16
        } else {
            state.filtered_helpers.len() as u16
        }
    } else {
        0
    };
    let hint_height = if dropdown_showing { 0 } else { margin_height };

    let dialog_height = if state.show_sessions_dialog { 11 } else { 0 };
    let dialog_margin = if state.show_sessions_dialog { 1 } else { 0 };

    // Layout: [messages][dialog_margin][dialog][input][dropdown][hint]
    let mut constraints = vec![
        Constraint::Min(1), // messages
        Constraint::Length(dialog_margin),
        Constraint::Length(dialog_height),
    ];
    if !state.show_sessions_dialog {
        constraints.push(Constraint::Length(input_height));
        constraints.push(Constraint::Length(dropdown_height));
        constraints.push(Constraint::Length(hint_height));
    }
    let chunks = ratatui::layout::Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(f.area());

    let message_area = chunks[0];
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
    let mut hint_area = Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };

    if !state.show_sessions_dialog {
        input_area = chunks[3];
        dropdown_area = chunks.get(4).copied().unwrap_or(input_area);
        hint_area = chunks.get(5).copied().unwrap_or(input_area);
    }

    let message_area_width = message_area.width as usize;
    let message_area_height = message_area.height as usize;

    render_messages(
        f,
        state,
        message_area,
        message_area_width,
        message_area_height,
    );

    if state.show_sessions_dialog {
        render_sessions_dialog(f, state);
        return;
    }
    if state.is_dialog_open {
        render_confirmation_dialog(f, state);
        return;
    }

    if !state.is_dialog_open {
        render_multiline_input(f, state, input_area);
        render_helper_dropdown(f, state, dropdown_area);
        render_autocomplete_dropdown(f, state, dropdown_area);
        if !dropdown_showing {
            render_hint_or_shortcuts(f, state, hint_area);
        }
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

        // Add spacing before the line if needed
        if should_add_spacing {
            processed_lines.push(Line::from(""));
        }

        // Process the line and add all resulting lines
        if line_text.contains("<checkpoint_id>") {
            let processed = process_checkpoint_patterns(
                &[(line.clone(), Style::default())],
                f.area().width as usize,
            );
            for (processed_line, _) in processed {
                processed_lines.push(processed_line);
            }
        } else if line_text.contains("<agent_mode>") {
            let processed = process_agent_mode_patterns(&[(line.clone(), Style::default())]);
            for (processed_line, _) in processed {
                processed_lines.push(processed_line);
            }
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
            let mut found = false;

            for tag in &section_tags {
                let closing_tag = format!("</{}>", tag);
                if line_text.trim() == closing_tag {
                    processed_lines.push(Line::from(""));
                    found = true;
                    break;
                }
                if line_text.contains(&format!("<{}>", tag)) {
                    let processed =
                        process_section_title_patterns(&[(line.clone(), Style::default())], tag);
                    for (processed_line, _) in processed {
                        processed_lines.push(processed_line);
                    }
                    found = true;
                    break;
                }
            }

            if !found {
                if line_text.trim() == "SPACING_MARKER" {
                    processed_lines.push(Line::from(""));
                } else {
                    processed_lines.push(line.clone());
                }
            }
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
