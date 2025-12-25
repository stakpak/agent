//! Shell Popup Component
//!
//! A unified popup for shell/command execution that appears above the input area.
//! Supports expand/shrink modes and proper cursor handling.

use crate::app::AppState;

use crate::services::handlers::shell::{capture_styled_screen, trim_shell_lines};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

/// Minimum height when shrinked (2 lines of content + 2 for borders)
pub const SHELL_POPUP_MIN_HEIGHT: u16 = 4;

/// Maximum height as percentage of terminal height
pub const SHELL_POPUP_MAX_HEIGHT_PERCENT: f32 = 0.6;

/// Calculate the popup height based on state and content
pub fn calculate_popup_height(state: &AppState, terminal_height: u16) -> u16 {
    if !state.shell_popup_visible {
        return 0;
    }

    if !state.shell_popup_expanded {
        return SHELL_POPUP_MIN_HEIGHT;
    }

    // Count actual non-empty lines from the screen for dynamic sizing
    let screen = state.shell_screen.screen();
    let (rows, cols) = screen.size();

    // Count non-empty rows from the end
    let mut content_lines: u16 = 0;
    let mut found_content = false;

    for row in (0..rows).rev() {
        let mut row_empty = true;
        for col in 0..cols {
            if let Some(cell) = screen.cell(row, col)
                && !cell.contents().trim().is_empty()
            {
                row_empty = false;
                break;
            }
        }
        if !row_empty {
            found_content = true;
        }
        if found_content {
            content_lines = row + 1;
            break;
        }
    }

    // Minimum of 2 content lines
    content_lines = content_lines.max(2);

    // Add 2 for borders
    let desired_height = content_lines.saturating_add(2);

    // Calculate max height (60% of terminal)
    let max_height = (terminal_height as f32 * SHELL_POPUP_MAX_HEIGHT_PERCENT) as u16;

    // Clamp between min and max
    desired_height.clamp(
        SHELL_POPUP_MIN_HEIGHT,
        max_height.max(SHELL_POPUP_MIN_HEIGHT),
    )
}

/// Render the shell popup above the input area
pub fn render_shell_popup(f: &mut Frame, state: &mut AppState, area: Rect) {
    if !state.shell_popup_visible {
        return;
    }

    // Determine colors based on state
    let (border_color, title_suffix) = if state.shell_popup_expanded {
        if state.active_shell_command.is_some() {
            // Check if command has been executed yet (initializing vs active)
            if !state.shell_pending_command_executed && state.is_tool_call_shell_command {
                (Color::Yellow, "[Initializing...]")
            } else {
                (Color::Cyan, "[Active]")
            }
        } else {
            (Color::Green, "[Completed]")
        }
    } else {
        (Color::DarkGray, "[Background] '$' to expand")
    };

    // Build title with truncated command (max 50 chars)
    let command_name = state
        .shell_pending_command_value
        .as_ref()
        .map(|c| {
            if c.len() > 50 {
                format!("{}...", &c[..47])
            } else {
                c.clone()
            }
        })
        .unwrap_or_else(|| "Shell".to_string());

    let title = format!(" $ {} {} ", command_name, title_suffix);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            title,
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ));

    // Get content area inside borders
    let inner_area = block.inner(area);

    // Render the block first
    f.render_widget(block, area);

    if inner_area.width == 0 || inner_area.height == 0 {
        return;
    }

    // Get styled screen content - the PTY already shows the prompt and command naturally
    let screen_lines = capture_styled_screen(&mut state.shell_screen);

    // Build display lines directly from screen content
    // We trim trailing empty lines for display so we don't "scroll to bottom" of empty lines
    let display_lines: Vec<Line<'static>> = trim_shell_lines(screen_lines);

    let inner_height = inner_area.height as usize;
    let total_lines = display_lines.len();

    // Calculate visible lines based on scroll position
    // shell_popup_scroll represents how many lines from bottom we've scrolled up
    let max_scroll = total_lines.saturating_sub(inner_height);
    let scroll_from_bottom = state.shell_popup_scroll.min(max_scroll);
    let skip = max_scroll.saturating_sub(scroll_from_bottom);

    let visible_lines: Vec<Line<'static>> = display_lines
        .clone()
        .into_iter()
        .skip(skip)
        .take(inner_height)
        .collect();

    let content = Paragraph::new(visible_lines);
    f.render_widget(content, inner_area);

    // Render cursor when shell is active and expanded (only if at bottom - scroll = 0)
    if state.shell_popup_expanded
        && state.active_shell_command.is_some()
        && state.shell_popup_scroll == 0
    {
        // Only show cursor if it should be visible (blink state)
        if state.shell_cursor_visible {
            let (cursor_row, cursor_col) = state.shell_screen.screen().cursor_position();

            // Calculate screen position for cursor (directly from PTY cursor position)
            let cursor_line_in_content = cursor_row as usize;

            // Since we're at bottom (scroll=0), calculate where cursor appears
            if cursor_line_in_content >= skip && cursor_line_in_content < skip + inner_height {
                let screen_row = (cursor_line_in_content - skip) as u16;
                let screen_x = inner_area.x + cursor_col;
                let screen_y = inner_area.y + screen_row;

                if screen_x < inner_area.x + inner_area.width
                    && screen_y < inner_area.y + inner_area.height
                {
                    f.set_cursor_position(ratatui::layout::Position::new(screen_x, screen_y));
                }
            }
        }
    }
}

/// Update cursor blink state (call this from event loop tick)
pub fn update_cursor_blink(state: &mut AppState) {
    state.shell_cursor_blink_timer = state.shell_cursor_blink_timer.wrapping_add(1);

    // Toggle every 5 frames (~500ms at 10fps / 100ms interval)
    if state.shell_cursor_blink_timer % 5 == 0 {
        state.shell_cursor_visible = !state.shell_cursor_visible;
    }
}

/// Reset cursor to visible (call when input is received)
pub fn reset_cursor_blink(state: &mut AppState) {
    state.shell_cursor_visible = true;
    state.shell_cursor_blink_timer = 0;
}
