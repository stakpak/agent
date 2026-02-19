use crate::app::AppState;
use crate::constants::{DROPDOWN_MAX_HEIGHT, SCROLL_BUFFER_LINES};
use crate::services::detect_term::AdaptiveColors;
use crate::services::helper_dropdown::{render_file_search_dropdown, render_helper_dropdown};
use crate::services::hint_helper::render_hint_or_shortcuts;
use crate::services::message::{
    get_wrapped_collapsed_message_lines_cached, get_wrapped_message_lines_cached,
};
use crate::services::message_pattern::spans_to_string;

use crate::services::shell_popup;
use crate::services::side_panel;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn view(f: &mut Frame, state: &mut AppState) {
    // First, handle the horizontal split for the side panel
    let (main_area, side_panel_area) = if state.show_side_panel {
        // Fixed width of 32 characters for side panel
        let panel_width = 32u16;
        let horizontal_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(panel_width)])
            .split(f.area());
        // Add 1 char right margin to main area for symmetric spacing around the side panel divider
        let main_with_margin = Rect {
            x: horizontal_chunks[0].x,
            y: horizontal_chunks[0].y,
            width: horizontal_chunks[0].width.saturating_sub(1),
            height: horizontal_chunks[0].height,
        };
        (main_with_margin, Some(horizontal_chunks[1]))
    } else {
        (f.area(), None)
    };

    // Render side panel if visible
    if let Some(panel_area) = side_panel_area {
        side_panel::render_side_panel(f, state, panel_area);
    }

    // Calculate the required height for the input area based on content
    let input_area_width = main_area.width.saturating_sub(4) as usize;
    let input_lines = calculate_input_lines(state, input_area_width); // -4 for borders and padding
    let input_height = (input_lines + 2) as u16;
    let margin_height = 1;
    let dropdown_showing = state.show_helper_dropdown
        && ((!state.filtered_helpers.is_empty() && state.input().starts_with('/'))
            || !state.filtered_files.is_empty());
    let dropdown_height = if dropdown_showing {
        if !state.filtered_files.is_empty() {
            DROPDOWN_MAX_HEIGHT as u16
        } else {
            // Use compact height calculation matching helper_dropdown.rs
            const MAX_VISIBLE_ITEMS: usize = 5;
            let visible_height = MAX_VISIBLE_ITEMS.min(state.filtered_helpers.len());
            let has_content_above = state.helper_scroll > 0;
            let has_content_below =
                state.helper_scroll < state.filtered_helpers.len().saturating_sub(visible_height);
            let arrow_lines =
                if has_content_above { 1 } else { 0 } + if has_content_below { 1 } else { 0 };
            let counter_line = if has_content_above || has_content_below {
                1
            } else {
                0
            };
            (visible_height + arrow_lines + counter_line) as u16
        }
    } else {
        0
    };
    let hint_height = if state.show_helper_dropdown {
        0
    } else {
        margin_height
    };

    // Calculate shell popup height (goes above input)
    let shell_popup_height = shell_popup::calculate_popup_height(state, main_area.height);

    // Calculate approval bar height (needs terminal width for wrapping calculation)
    let approval_bar_height = state.approval_bar.calculate_height(main_area.width);
    let approval_bar_visible = state.approval_bar.is_visible();

    // Hide input when shell popup is expanded (takes over input) or when approval bar is visible
    let ask_user_visible = state.show_ask_user_popup && !state.ask_user_questions.is_empty();
    let input_visible =
        !(approval_bar_visible || state.shell_popup_visible && state.shell_popup_expanded);
    let effective_input_height = if input_visible { input_height } else { 0 };
    let queue_count = state.pending_user_messages.len();
    let queue_preview_height = if input_visible && queue_count > 0 {
        // Cap at 1/4 of the screen to avoid starving the message area
        (queue_count as u16).min(main_area.height / 4).max(1)
    } else {
        0
    };

    // Hide dropdown when approval bar is visible or ask_user popup is visible
    let effective_dropdown_height = if approval_bar_visible || ask_user_visible {
        0
    } else {
        dropdown_height
    };

    // Layout: [messages][loading_line][shell_popup][approval_bar][queue][input][dropdown][hint]
    let effective_approval_bar_height = if approval_bar_visible {
        approval_bar_height
    } else {
        0
    };

    let constraints = vec![
        Constraint::Min(1),                                // messages
        Constraint::Length(1), // reserved line for loading indicator (also shows tokens)
        Constraint::Length(shell_popup_height), // shell popup (0 if hidden)
        Constraint::Length(effective_approval_bar_height), // approval bar (0 if hidden)
        Constraint::Length(queue_preview_height), // queued messages preview (0 if hidden)
        Constraint::Length(effective_input_height), // input (0 when approval bar visible)
        Constraint::Length(effective_dropdown_height), // dropdown (0 when approval bar visible)
        Constraint::Length(hint_height), // hint
    ];
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(main_area);

    let message_area = chunks[0];
    let loading_area = chunks[1]; // Reserved line for loading indicator
    let shell_popup_area = chunks[2];
    let approval_bar_area = chunks[3];
    let queue_preview_area = chunks[4];
    let input_area = chunks[5];
    let dropdown_area = chunks[6];
    let hint_area = chunks[7];

    // Create padded message area for content rendering
    let padded_message_area = Rect {
        x: message_area.x + 1,
        y: message_area.y,
        width: message_area.width.saturating_sub(2),
        height: message_area.height,
    };

    let message_area_width = padded_message_area.width as usize;
    let message_area_height = message_area.height as usize;

    render_messages(
        f,
        state,
        padded_message_area,
        message_area_width,
        message_area_height,
    );

    // Render approval bar in its dedicated area (if visible)
    if approval_bar_visible {
        let padded_approval_bar_area = Rect {
            x: approval_bar_area.x + 1,
            y: approval_bar_area.y,
            width: approval_bar_area.width.saturating_sub(2),
            height: approval_bar_area.height,
        };
        state.approval_bar.render(f, padded_approval_bar_area);
    }

    // Render shell popup above input area (if visible)
    if state.shell_popup_visible {
        let padded_shell_popup_area = Rect {
            x: shell_popup_area.x + 1,
            y: shell_popup_area.y,
            width: shell_popup_area.width.saturating_sub(2),
            height: shell_popup_area.height,
        };
        shell_popup::render_shell_popup(f, state, padded_shell_popup_area);
    }

    let padded_loading_area = Rect {
        x: loading_area.x + 1,
        y: loading_area.y,
        width: loading_area.width.saturating_sub(2),
        height: loading_area.height,
    };
    render_loading_indicator(f, state, padded_loading_area);

    if queue_preview_height > 0 {
        let padded_queue_area = Rect {
            x: queue_preview_area.x + 1,
            y: queue_preview_area.y,
            width: queue_preview_area.width.saturating_sub(2),
            height: queue_preview_area.height,
        };
        render_queue_preview_line(f, state, padded_queue_area);
    }

    if state.show_collapsed_messages {
        render_collapsed_messages_popup(f, state);
    } else if state.is_dialog_open {
    } else if state.shell_popup_visible && state.shell_popup_expanded {
        // Don't render input when popup is expanded - popup takes over input
    } else if !approval_bar_visible {
        // Only render input/dropdown when approval bar is NOT visible
        render_multiline_input(f, state, input_area);
        render_helper_dropdown(f, state, dropdown_area);
        render_file_search_dropdown(f, state, dropdown_area);
    }
    // Render hint/shortcuts if not hiding for dropdown, not showing collapsed messages, and not showing approval bar
    if !state.show_helper_dropdown
        && !state.show_collapsed_messages
        && !approval_bar_visible
        && !ask_user_visible
    {
        let padded_hint_area = Rect {
            x: hint_area.x + 1,
            y: hint_area.y,
            width: hint_area.width.saturating_sub(2),
            height: hint_area.height,
        };
        render_hint_or_shortcuts(f, state, padded_hint_area);
    }

    // === POPUPS - rendered last to appear on top of side panel ===

    // Render profile switcher
    if state.show_profile_switcher {
        crate::services::profile_switcher::render_profile_switcher_popup(f, state);
    }

    // Render file changes popup
    if state.show_file_changes_popup {
        crate::services::file_changes_popup::render_file_changes_popup(f, state);
    }

    // Render shortcuts popup (now includes commands)
    if state.show_shortcuts_popup {
        crate::services::shortcuts_popup::render_shortcuts_popup(f, state);
    }
    // Render rulebook switcher
    if state.show_rulebook_switcher {
        crate::services::rulebook_switcher::render_rulebook_switcher_popup(f, state);
    }

    // Render model switcher
    if state.show_model_switcher {
        crate::services::model_switcher::render_model_switcher_popup(f, state);
    }

    // Render profile switch overlay
    if state.profile_switching_in_progress {
        crate::services::profile_switcher::render_profile_switch_overlay(f, state);
    }

    // Render "existing plan found" modal
    if state.existing_plan_prompt.is_some() {
        render_existing_plan_modal(f, state);
    }

    // Render plan review overlay (full-screen, on top of everything)
    if state.show_plan_review {
        crate::services::plan_review::render_plan_review(f, state, f.area());
    }
}

fn render_existing_plan_modal(f: &mut Frame, state: &AppState) {
    use ratatui::style::Modifier;
    use ratatui::widgets::{Clear, Wrap};

    let area = f.area();

    let (title_text, status_text) = state
        .existing_plan_prompt
        .as_ref()
        .and_then(|p| p.metadata.as_ref())
        .map(|m| {
            let truncated = if m.title.len() > 40 {
                format!("{}…", &m.title[..39])
            } else {
                m.title.clone()
            };
            (truncated, format!("{}  v{}", m.status, m.version))
        })
        .unwrap_or_else(|| ("(unknown)".to_string(), String::new()));

    let mut lines: Vec<Line<'_>> = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Plan: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                title_text,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];
    if !status_text.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Status: ", Style::default().fg(Color::DarkGray)),
            Span::styled(status_text, Style::default().fg(Color::Yellow)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  u", Style::default().fg(Color::Cyan)),
        Span::styled(" use existing  ", Style::default().fg(Color::DarkGray)),
        Span::styled("n", Style::default().fg(Color::Green)),
        Span::styled(" start new  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::styled(" cancel", Style::default().fg(Color::DarkGray)),
    ]));

    let modal_width = 52u16.min(area.width.saturating_sub(4));
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
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Existing Plan Found ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(modal_area);
    f.render_widget(block, modal_area);

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, inner);
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

    // Use consistent scroll calculation with buffer
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
            "Expanded Messages (ctrl+t to close, tab to previous message, ↑/↓ to scroll)",
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
    // Loading spinner is now shown in the hint area below input
    // This area is kept for potential future use (e.g., token count display)
    let _ = (f, state, area);
}

fn truncate_to(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let mut out: String = flat.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn render_queue_preview_line(f: &mut Frame, state: &AppState, area: Rect) {
    if area.width == 0 || area.height == 0 || state.pending_user_messages.is_empty() {
        return;
    }

    let max_chars = (area.width as usize).saturating_sub(4); // room for "  > "
    let mut lines: Vec<Line> = Vec::with_capacity(state.pending_user_messages.len());

    for msg in state.pending_user_messages.iter() {
        let text = if !msg.user_message_text.trim().is_empty() {
            &msg.user_message_text
        } else if !msg.final_input.trim().is_empty() {
            &msg.final_input
        } else if !msg.image_parts.is_empty() {
            "[image]"
        } else {
            "(empty)"
        };
        let preview = truncate_to(text, max_chars);
        lines.push(Line::from(Span::styled(
            format!("  > {preview}"),
            Style::default().fg(Color::DarkGray),
        )));
    }

    let widget = Paragraph::new(lines);
    f.render_widget(widget, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_to_short_string_unchanged() {
        assert_eq!(truncate_to("hello world", 20), "hello world");
    }

    #[test]
    fn truncate_to_long_string_ellipsis() {
        let out = truncate_to("this is a very long message that should be cut", 16);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= 16);
    }

    #[test]
    fn truncate_to_collapses_whitespace() {
        assert_eq!(
            truncate_to("hello   world\nnewline", 30),
            "hello world newline"
        );
    }

    #[test]
    fn truncate_to_zero_max_returns_empty() {
        assert_eq!(truncate_to("hello", 0), "");
    }
}
