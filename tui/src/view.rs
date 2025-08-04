use crate::app::AppState;
use crate::services::confirmation_dialog::render_confirmation_dialog;
use crate::services::helper_block::render_loading_spinner;
use crate::services::helper_dropdown::{render_autocomplete_dropdown, render_helper_dropdown};
use crate::services::hint_helper::render_hint_or_shortcuts;
use crate::services::message::{
    Message, get_wrapped_collapsed_message_lines, get_wrapped_message_lines,
};
use crate::services::message_pattern::{
    process_agent_mode_patterns, process_checkpoint_patterns, process_section_title_patterns,
    spans_to_string,
};
use crate::services::sessions_dialog::render_sessions_dialog;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

const DROPDOWN_MAX_HEIGHT: usize = 8;

pub fn view(f: &mut Frame, state: &AppState) {
    if state.inline_mode {
        render_inline_view(f, state);
    } else {
        render_full_view(f, state);
    }
}

fn calculate_compact_input_height(state: &AppState) -> u16 {
    let mut total_height = if state.show_sessions_dialog || state.is_dialog_open {
        0 // No input height when dialogs are open
    } else {
        3 // Base input height
    };

    // Add height for content below input (dropdowns and hints)
    if state.show_helper_dropdown && !state.filtered_helpers.is_empty() && !state.is_dialog_open {
        total_height += state.filtered_helpers.len().min(6) as u16; // Limit dropdown height
    } else {
        total_height += 1; // Hint area
    }

    // Add height for content above input (dialogs)
    let mut above_input_height = 0;
    if state.show_sessions_dialog {
        // Sessions dialog should use most of the available height
        above_input_height += 30; // Much larger for sessions dialog
    }

    if state.is_dialog_open {
        above_input_height += 15; // Confirmation dialog height (increased for compact view)
    }

    // Add space for loading spinner if loading
    if state.loading {
        above_input_height += 1;
    }

    total_height += above_input_height;

    total_height
}

fn render_inline_view(f: &mut Frame, state: &AppState) {
    // For inline mode, we only render the bottom widget
    // Messages are printed to stdout, not rendered in TUI

    let area = f.area();

    // Use shared height calculation
    let total_height = calculate_compact_input_height(state);

    // Ensure we don't exceed screen height
    let final_height = if total_height > area.height {
        area.height
    } else {
        total_height
    };

    // Create layout based on whether dialogs are open
    let chunks = if state.show_sessions_dialog || state.is_dialog_open {
        // When dialogs are open, use bottom layout
        ratatui::layout::Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1), // Spacer to push to bottom
                Constraint::Length(final_height),
            ])
            .split(area)
    } else {
        // When no dialogs, position input 2 lines from top
        ratatui::layout::Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // Spacer to push down from top
                Constraint::Length(final_height),
                Constraint::Min(1), // Remaining space at bottom
            ])
            .split(area)
    };

    let widget_area = chunks[1];

    // Render the compact input widget
    render_compact_input(f, state, widget_area);
}

fn render_full_view(f: &mut Frame, state: &AppState) {
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

    let message_area_width = message_area.width as usize;
    let message_area_height = message_area.height as usize;

    render_messages(
        f,
        state,
        message_area,
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
        x: popup_area.x + 1,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(2),
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

fn render_compact_input(f: &mut Frame, state: &AppState, area: Rect) {
    // Create a compact layout for the inline widget
    let mut constraints = vec![];

    // Only add input area if no dialogs are open
    if !state.show_sessions_dialog && !state.is_dialog_open {
        constraints.push(Constraint::Length(3)); // Input area
    }

    // Add constraints for content below input (dropdowns and hints)
    if state.show_helper_dropdown && !state.filtered_helpers.is_empty() && !state.is_dialog_open {
        constraints.push(Constraint::Length(
            state.filtered_helpers.len().min(6) as u16
        )); // Helper dropdown
    } else {
        constraints.push(Constraint::Length(if state.show_shortcuts { 2 } else { 1 })); // Hint area - 2 lines for shortcuts, 1 for normal
    }

    // Add constraints for content above input (dialogs and loading)
    let mut above_constraints = vec![];

    if state.loading {
        above_constraints.push(Constraint::Length(1)); // Loading spinner
    }

    if state.show_sessions_dialog {
        above_constraints.push(Constraint::Length(30)); // Sessions dialog (much larger for compact view)
    } else if state.is_dialog_open {
        above_constraints.push(Constraint::Length(4)); // Confirmation dialog (increased for compact view)
    }

    // Combine constraints: above content + input + below content
    let above_constraints_len = above_constraints.len();
    let mut all_constraints = above_constraints;
    all_constraints.extend(constraints);

    let chunks = ratatui::layout::Layout::default()
        .direction(Direction::Vertical)
        .constraints(all_constraints)
        .split(area);

    let input_area = if !state.show_sessions_dialog && !state.is_dialog_open {
        chunks[above_constraints_len] // Input is after above content
    } else {
        Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        } // Empty area when dialogs are open
    };
    let below_content_area = chunks.get(
        above_constraints_len
            + if !state.show_sessions_dialog && !state.is_dialog_open {
                1
            } else {
                0
            },
    ); // Below input

    // Render content above input (dialogs and loading)
    let mut content_index = 0;

    // Render loading spinner if loading
    if state.loading {
        if let Some(area) = chunks.get(content_index) {
            render_compact_loading(f, state, *area);
        }
        content_index += 1;
    }

    // Render dialogs above input
    if state.show_sessions_dialog {
        if let Some(area) = chunks.get(content_index) {
            render_compact_sessions_dialog(f, state, *area);
        }
    } else if state.is_dialog_open {
        if let Some(area) = chunks.get(content_index) {
            render_compact_confirmation_dialog(f, *area);
        }
    } else {
        // Render the input with proper styling
        render_multiline_input(f, state, input_area);
    }

    // Render content below input (dropdowns and hints)
    if let Some(area) = below_content_area {
        if state.show_helper_dropdown && !state.filtered_helpers.is_empty() && !state.is_dialog_open
        {
            render_helper_dropdown(f, state, *area);
        } else {
            render_hint_or_shortcuts(f, state, *area);
        }
    }
}

fn render_compact_loading(f: &mut Frame, state: &AppState, area: Rect) {
    if area.height > 0 {
        let loading_line = render_loading_spinner(state);
        let loading_widget = Paragraph::new(vec![loading_line])
            .style(ratatui::style::Style::default().fg(Color::LightRed));
        f.render_widget(loading_widget, area);
    }
}

fn render_compact_sessions_dialog(f: &mut Frame, state: &AppState, area: Rect) {
    let dialog_height = area.height.saturating_sub(1); // Leave space for hint

    let dialog_area = Rect {
        x: area.x + 1,
        y: area.y,
        width: area.width.saturating_sub(2),
        height: dialog_height,
    };

    // Render the dialog block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightYellow))
        .title(Span::styled(
            "View session",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));
    f.render_widget(block, dialog_area);

    // Render session list
    let list_area = Rect {
        x: dialog_area.x + 2,
        y: dialog_area.y + 1,
        width: dialog_area.width - 4,
        height: dialog_area.height.saturating_sub(2), // More space for list in larger dialog
    };

    let items: Vec<ratatui::widgets::ListItem> = state
        .sessions
        .iter()
        .map(|s| {
            let formatted_datetime = if let Ok(dt) =
                chrono::DateTime::parse_from_rfc3339(&s.updated_at.replace(" UTC", "+00:00"))
            {
                dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
            } else {
                let parts = s.updated_at.split('T').collect::<Vec<_>>();
                let date = parts.first().unwrap_or(&"");
                let time = parts.get(1).and_then(|t| t.split('.').next()).unwrap_or("");
                format!("{} {} UTC", date, time)
            };

            let text = format!("{} . {}", formatted_datetime, s.title);
            ratatui::widgets::ListItem::new(Line::from(vec![Span::raw(text)]))
        })
        .collect();

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(state.session_selected));
    let list = ratatui::widgets::List::new(items)
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Cyan)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
        .style(Style::default().fg(Color::Gray))
        .block(Block::default());
    f.render_stateful_widget(list, list_area, &mut list_state);

    // Help text
    let help = "press enter to choose · esc to cancel";
    let help_area = Rect {
        x: dialog_area.x + 2,
        y: dialog_area.y + dialog_area.height - 1,
        width: dialog_area.width - 4,
        height: 1,
    };
    let help_widget = Paragraph::new(help)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(ratatui::layout::Alignment::Left);
    f.render_widget(help_widget, help_area);
}

fn render_compact_confirmation_dialog(f: &mut Frame, area: Rect) {
    let dialog_height = area.height.saturating_sub(1); // Leave space for hint

    let dialog_area = Rect {
        x: area.x + 1,
        y: area.y,
        width: area.width.saturating_sub(2),
        height: dialog_height,
    };

    // Single line message
    let message =
        "Press Enter to continue. '$' to run the command yourself or Esc to cancel and reprompt";

    let line = Line::from(vec![Span::styled(
        message,
        Style::default()
            .fg(Color::White)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )]);
    let dialog = Paragraph::new(vec![line])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightYellow))
                .title("Confirmation"),
        )
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(dialog, dialog_area);
}
