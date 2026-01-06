use crate::app::{AppState, LoadingType};
use crate::constants::{DROPDOWN_MAX_HEIGHT, SCROLL_BUFFER_LINES};
use crate::services::detect_term::AdaptiveColors;
use crate::services::helper_dropdown::{render_file_search_dropdown, render_helper_dropdown};
use crate::services::hint_helper::render_hint_or_shortcuts;
use crate::services::message::{
    get_wrapped_collapsed_message_lines_cached, get_wrapped_message_lines_cached,
};
use crate::services::message_pattern::spans_to_string;
use crate::services::sessions_dialog::render_sessions_dialog;
use crate::services::shell_popup;
use crate::services::side_panel;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn view(f: &mut Frame, state: &mut AppState) {
    // First, handle the horizontal split for the side panel
    let (main_area, side_panel_area) = if state.show_side_panel {
        // Fixed width of 35 characters for side panel
        let panel_width = 37u16;
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
    let margin_height = 2;
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

    let dialog_height = if state.show_sessions_dialog { 11 } else { 0 };
    let dialog_margin = if state.show_sessions_dialog { 2 } else { 0 };

    // Calculate shell popup height (goes above input)
    let shell_popup_height = shell_popup::calculate_popup_height(state, main_area.height);

    // Hide input when shell popup is expanded (takes over input)
    let effective_input_height = if state.shell_popup_visible && state.shell_popup_expanded {
        0 // Hide input when popup is expanded
    } else {
        input_height
    };

    // Layout: [messages][loading_line][dialog_margin][dialog][shell_popup][input][dropdown][hint]
    let mut constraints = vec![
        Constraint::Min(1),    // messages
        Constraint::Length(1), // reserved line for loading indicator (also shows tokens)
        Constraint::Length(dialog_margin),
        Constraint::Length(dialog_height),
    ];
    if !state.show_sessions_dialog {
        constraints.push(Constraint::Length(shell_popup_height)); // shell popup (0 if hidden)
        constraints.push(Constraint::Length(effective_input_height));
        constraints.push(Constraint::Length(dropdown_height));
    }
    constraints.push(Constraint::Length(hint_height)); // Always include hint height (may be 0)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(main_area);

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
    let mut shell_popup_area = Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };
    let hint_area = chunks.last().copied().unwrap_or(message_area);

    if !state.show_sessions_dialog {
        shell_popup_area = chunks[4]; // Shell popup between dialog and input
        input_area = chunks[5]; // Input after shell popup
        dropdown_area = chunks.get(6).copied().unwrap_or(input_area);
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

    // Render shell popup above input area (if visible)
    if state.shell_popup_visible && !state.show_sessions_dialog {
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

    if state.show_collapsed_messages {
        render_collapsed_messages_popup(f, state);
    } else if state.is_dialog_open {
    } else if state.shell_popup_visible && state.shell_popup_expanded {
        // Don't render input when popup is expanded - popup takes over input
    } else if !state.show_sessions_dialog {
        // Don't render input when sessions dialog is visible
        render_multiline_input(f, state, input_area);
        render_helper_dropdown(f, state, dropdown_area);
        render_file_search_dropdown(f, state, dropdown_area);
    }
    // Render hint/shortcuts if not hiding for dropdown and not showing collapsed messages (unless dialog is open)
    if !state.show_helper_dropdown && !state.show_collapsed_messages {
        let padded_hint_area = Rect {
            x: hint_area.x + 1,
            y: hint_area.y,
            width: hint_area.width.saturating_sub(2),
            height: hint_area.height,
        };
        render_hint_or_shortcuts(f, state, padded_hint_area);
    }

    // === POPUPS - rendered last to appear on top of side panel ===

    // Render sessions dialog (on top of side panel)
    if state.show_sessions_dialog {
        render_sessions_dialog(f, state);
    }

    // Render approval popup to ensure it appears on top of everything
    if state.approval_popup.is_visible() {
        state.approval_popup.render(f, f.area());
    }

    // Render profile switcher
    if state.show_profile_switcher {
        crate::services::profile_switcher::render_profile_switcher_popup(f, state);
    }

    // Render file changes popup
    if state.show_file_changes_popup {
        crate::services::file_changes_popup::render_file_changes_popup(f, state);
    }

    // Render shortcuts popup
    if state.show_shortcuts_popup {
        crate::services::shortcuts_popup::render_shortcuts_popup(f, state);
    }
    // Render command palette
    if state.show_command_palette {
        crate::services::commands::render_command_palette(f, state);
    }
    // Render rulebook switcher
    if state.show_rulebook_switcher {
        crate::services::rulebook_switcher::render_rulebook_switcher_popup(f, state);
    }

    // Render profile switch overlay
    if state.profile_switching_in_progress {
        crate::services::profile_switcher::render_profile_switch_overlay(f, state);
    }

    if state.show_context_popup {
        crate::services::context_popup::render_context_popup(f, state);
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
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(if state.show_shell_mode {
            Style::default().fg(AdaptiveColors::dark_magenta())
        } else {
            Style::default().fg(Color::DarkGray)
        });

    if !state.show_shell_mode && !state.loading {
        block = block.title(
            Line::from(Span::styled(
                "'$' for Shell mode",
                Style::default().fg(Color::DarkGray),
            ))
            .left_aligned(),
        );
    }

    // Display auth info on the right: subscription name if available, otherwise auth provider
    match &state.auth_display_info {
        (_, Some(_auth_provider), Some(subscription)) => {
            // Show subscription name (e.g., "Claude Pro", "Claude Max")
            block = block.title(
                Line::from(Span::styled(
                    subscription.as_str(),
                    Style::default().fg(Color::Reset),
                ))
                .right_aligned(),
            );
        }
        (_, Some(auth_provider), None) => {
            // Show auth provider name if no subscription (e.g., "Anthropic")
            block = block.title(
                Line::from(Span::styled(
                    auth_provider.as_str(),
                    Style::default().fg(Color::Reset),
                ))
                .right_aligned(),
            );
        }
        (Some(config_provider), None, None) => {
            // Show config provider if no auth (e.g., "Local")
            block = block.title(
                Line::from(Span::styled(
                    config_provider.as_str(),
                    Style::default().fg(Color::DarkGray),
                ))
                .right_aligned(),
            );
        }
        _ => {}
    }

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
    // Always render this line - shows spinner when loading on left, tokens always on right (if > 0)
    let mut left_spans = Vec::new();

    // Left side: spinner (if loading)
    if state.loading {
        let spinner_chars = ["▄▀", "▐▌", "▀▄", "▐▌"];
        let spinner = spinner_chars[state.spinner_frame % spinner_chars.len()];
        let spinner_text = if state.loading_type == LoadingType::Sessions {
            "Loading sessions..."
        } else {
            "Stakpaking..."
        };

        left_spans.push(Span::styled(
            format!("{} {}", spinner, spinner_text),
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        ));

        if state.loading_type == LoadingType::Llm {
            left_spans.push(Span::styled(
                " - Esc to cancel",
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    // Reset utilization warnings before calculating
    state.context_usage_percent = 0;
    let total_width = area.width as usize;
    let mut final_spans = Vec::new();

    if !state.show_sessions_dialog {
        if !state.show_side_panel {
            // No tokens and side panel is closed, show hint to open side panel
            let hint_text = "Ctrl+y side panel";
            let left_len: usize = left_spans.iter().map(|s| s.content.len()).sum();
            let total_adjusted_width = if state.loading {
                total_width + 4
            } else {
                total_width
            };
            let spacing = total_adjusted_width.saturating_sub(left_len + hint_text.len());

            final_spans.extend(left_spans);
            if spacing > 0 {
                final_spans.push(Span::styled(" ".repeat(spacing), Style::default()));
            }
            final_spans.push(Span::styled(
                hint_text,
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ));
        } else {
            // Side panel is open, no hint needed - just extend with left content
            final_spans.extend(left_spans);
        }
    } else {
        // Sessions dialog is open - just show left content
        final_spans.extend(left_spans);
    }

    let widget =
        Paragraph::new(Line::from(final_spans)).wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(widget, area);
}
