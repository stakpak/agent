use crate::app::AppState;
use crate::services::detect_term::AdaptiveColors;
use crate::services::helper_block::render_loading_spinner;
use crate::services::helper_dropdown::{render_file_search_dropdown, render_helper_dropdown};
use crate::services::hint_helper::render_hint_or_shortcuts;
use crate::services::message::{
    get_wrapped_collapsed_message_lines_cached, get_wrapped_message_lines_cached,
};
use crate::services::message_pattern::spans_to_string;
use crate::services::sessions_dialog::render_sessions_dialog;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

const DROPDOWN_MAX_HEIGHT: usize = 8;
const SCROLL_BUFFER_LINES: usize = 2;

pub fn view(f: &mut Frame, state: &mut AppState) {
    // Calculate the required height for the input area based on content
    let input_area_width = f.area().width.saturating_sub(4) as usize;
    let input_lines = calculate_input_lines(state, input_area_width); // -4 for borders and padding
    let input_height = (input_lines + 2) as u16;
    let margin_height = 2;
    let dropdown_showing = state.show_helper_dropdown
        && ((!state.filtered_helpers.is_empty() && state.input().starts_with('/'))
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

    // Layout: [messages][loading_line][dialog_margin][dialog][input][dropdown][hint]
    let mut constraints = vec![
        Constraint::Min(1),    // messages
        Constraint::Length(1), // reserved line for loading indicator
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
    let loading_area = chunks[1]; // Reserved line for loading indicator

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
        input_area = chunks[4]; // Updated index due to loading line
        dropdown_area = chunks.get(5).copied().unwrap_or(input_area);
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

    // Render loading indicator in dedicated area
    render_loading_indicator(f, state, loading_area);

    if state.show_collapsed_messages {
        render_collapsed_messages_popup(f, state);
    } else if state.show_sessions_dialog {
        render_sessions_dialog(f, state);
    } else if state.is_dialog_open {
    } else {
        render_multiline_input(f, state, input_area);
        render_helper_dropdown(f, state, dropdown_area);
        render_file_search_dropdown(f, state, dropdown_area);
    }
    // Render hint/shortcuts if not hiding for dropdown and not showing collapsed messages (unless dialog is open)
    if !state.show_helper_dropdown && !state.show_collapsed_messages {
        render_hint_or_shortcuts(f, state, hint_area);
    }

    // Render approval popup LAST to ensure it appears on top of everything
    if state.approval_popup.is_visible() {
        state.approval_popup.render(f, f.area());
    }

    // Render profile switcher
    if state.show_profile_switcher {
        crate::services::profile_switcher::render_profile_switcher_popup(f, state);
    }

    // Render profile switch overlay
    if state.profile_switching_in_progress {
        crate::services::profile_switcher::render_profile_switch_overlay(f, state);
    }
}

// Calculate how many lines the input will take up when wrapped
fn calculate_input_lines(state: &AppState, width: usize) -> usize {
    let prompt_width = 2; // "> " prefix
    let available_width = width.saturating_sub(prompt_width);
    if available_width <= 1 {
        return 1; // Fallback if width is too small
    }

    // Use TextArea's desired_height method for accurate line calculation
    state.text_area.desired_height(available_width as u16) as usize
}

fn render_messages(f: &mut Frame, state: &mut AppState, area: Rect, width: usize, height: usize) {
    f.render_widget(ratatui::widgets::Clear, area);

    let processed_lines = get_wrapped_message_lines_cached(state, width);
    let total_lines = processed_lines.len();

    // Handle edge case where we have no content
    if total_lines == 0 {
        let message_widget =
            Paragraph::new(Vec::<Line>::new()).wrap(ratatui::widgets::Wrap { trim: false });
        f.render_widget(message_widget, area);
        return;
    }

    // Use consistent scroll calculation with buffer (matching update.rs)
    let max_scroll = total_lines.saturating_sub(height.saturating_sub(SCROLL_BUFFER_LINES));

    // Calculate scroll position - ensure it doesn't exceed max_scroll
    let scroll = if state.stay_at_bottom {
        max_scroll
    } else {
        state.scroll.min(max_scroll)
    };

    // Create visible lines with pre-allocated capacity for better performance
    let mut visible_lines = Vec::with_capacity(height);

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

fn render_collapsed_messages_popup(f: &mut Frame, state: &mut AppState) {
    let screen = f.area();
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
        .style(ratatui::style::Style::default())
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
    render_collapsed_messages_content(f, state, content_area);
}

fn render_collapsed_messages_content(f: &mut Frame, state: &mut AppState, area: Rect) {
    let width = area.width as usize;
    let height = area.height as usize;

    // Messages are already owned, no need to clone
    let all_lines: Vec<Line> = get_wrapped_collapsed_message_lines_cached(state, width);

    if all_lines.is_empty() {
        let empty_widget =
            Paragraph::new("No collapsed messages found").style(ratatui::style::Style::default());
        f.render_widget(empty_widget, area);
        return;
    }

    // Pre-process lines (same as render_messages)
    let mut processed_lines: Vec<Line> = Vec::new();

    for line in all_lines.iter() {
        let line_text = spans_to_string(line);
        // Process the line (simplified version)
        if line_text.trim() == "SPACING_MARKER" {
            processed_lines.push(Line::from(""));
        } else {
            processed_lines.push(line.clone());
        }
    }

    let total_lines = processed_lines.len();
    // Use consistent scroll calculation with buffer (matching update.rs)

    let max_scroll = total_lines.saturating_sub(height.saturating_sub(SCROLL_BUFFER_LINES));

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

fn render_multiline_input(f: &mut Frame, state: &mut AppState, area: Rect) {
    // Create a block for the input area
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if state.show_shell_mode {
            Style::default().fg(AdaptiveColors::dark_magenta())
        } else {
            Style::default().fg(Color::DarkGray)
        });

    // Create content area inside the block
    let content_area = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };

    // Render the block
    f.render_widget(block, area);

    // Render the TextArea with state, handling password masking if needed
    if state.show_shell_mode && state.waiting_for_shell_input {
        state.text_area.render_with_state(
            content_area,
            f.buffer_mut(),
            &mut state.text_area_state,
            state.waiting_for_shell_input,
        );
    } else {
        f.render_stateful_widget_ref(&state.text_area, content_area, &mut state.text_area_state);
    }
}

fn render_loading_indicator(f: &mut Frame, state: &mut AppState, area: Rect) {
    if state.loading {
        let loading_line = render_loading_spinner(state);
        let loading_widget =
            Paragraph::new(loading_line).wrap(ratatui::widgets::Wrap { trim: false });
        f.render_widget(loading_widget, area);
    } else {
        // Clear the area when not loading
        f.render_widget(ratatui::widgets::Clear, area);
    }
}
