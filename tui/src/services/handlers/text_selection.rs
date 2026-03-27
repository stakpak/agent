//! Text selection handler for mouse-based text selection in the message area.
//!
//! This module handles:
//! - Starting selection on mouse drag start
//! - Updating selection during mouse drag
//! - Ending selection and copying to clipboard on mouse release
//! - Extracting clean text (excluding borders, decorations)
//! - Cursor positioning in input area on click
//! - Showing message action popup on user message click

use crate::app::AppState;
use crate::services::message_action_popup::find_user_message_at_line;
use crate::services::text_selection::{
    SelectionState, copy_to_clipboard, extract_selected_text, extract_selected_text_from_collapsed,
};
use crate::services::toast::Toast;

/// Check if coordinates are within the input area
fn is_in_input_area(state: &AppState, col: u16, row: u16) -> bool {
    let Some(input_area) = state.input_content_area else {
        return false;
    };

    col >= input_area.x
        && col < input_area.x + input_area.width
        && row >= input_area.y
        && row < input_area.y + input_area.height
}

/// Convert terminal column to content-relative column within the message area.
/// The message content is rendered at `message_area_x`, so we subtract that offset
/// to get a 0-based column within the rendered line content.
fn content_col(state: &AppState, terminal_col: u16) -> u16 {
    terminal_col.saturating_sub(state.message_area_x)
}

/// Convert terminal column to content-relative column within the collapsed popup area.
fn popup_content_col(state: &AppState, terminal_col: u16) -> u16 {
    terminal_col.saturating_sub(state.collapsed_popup_area_x)
}

/// Handle mouse drag start - begins text selection in message area, input area, or collapsed popup
pub fn handle_drag_start(state: &mut AppState, col: u16, row: u16) {
    // Reset auto-scroll state when starting a new selection
    state.selection_auto_scroll = 0;

    // When collapsed messages popup is open, use popup geometry for selection
    if state.messages_scrolling_state.show_collapsed_messages {
        handle_popup_drag_start(state, col, row);
        return;
    }

    // Use the accurate message_area_height from the last render
    let message_area_height = state.message_area_height as usize;

    // First check if click is in input area
    if is_in_input_area(state, col, row) {
        // Click was in input area - start input selection
        if let Some(input_area) = state.input_content_area {
            state
                .input_state
                .text_area
                .start_selection(col, row, input_area, &state.input_state.text_area_state);
        }
        // Clear message area selection
        state.selection = SelectionState::default();
        return;
    }

    // Clear any input area selection when clicking outside
    state.input_state.text_area.clear_selection();

    // Check if click is within message area
    // Message area starts at message_area_y and extends for message_area_height rows
    let row_in_message_area = (row as usize).saturating_sub(state.message_area_y as usize);
    if row < state.message_area_y || row_in_message_area >= message_area_height {
        // Click is outside message area, don't start selection
        state.selection = SelectionState::default();
        return;
    }

    // Also check if side panel is shown and click is in side panel area
    if state.show_side_panel {
        // Side panel is on the right, typically 32 chars wide
        let side_panel_width = 32u16;
        let main_area_width = state
            .terminal_size
            .width
            .saturating_sub(side_panel_width + 1);
        if col >= main_area_width {
            // Click is in side panel, don't start selection
            state.selection = SelectionState::default();
            return;
        }
    }

    // Convert screen row to absolute line index (row_in_message_area already calculated above)
    let absolute_line = state.messages_scrolling_state.scroll + row_in_message_area;
    // Convert terminal column to content-relative column
    let rel_col = content_col(state, col);

    state.selection = SelectionState {
        active: true,
        start_line: Some(absolute_line),
        start_col: Some(rel_col),
        end_line: Some(absolute_line),
        end_col: Some(rel_col),
    };
}

/// Handle drag start within the collapsed messages fullscreen popup
fn handle_popup_drag_start(state: &mut AppState, col: u16, row: u16) {
    let popup_height = state.collapsed_popup_area_height as usize;

    // Check if click is within the popup content area
    let row_in_popup = (row as usize).saturating_sub(state.collapsed_popup_area_y as usize);
    if row < state.collapsed_popup_area_y || row_in_popup >= popup_height {
        state.selection = SelectionState::default();
        return;
    }

    // Convert screen row to absolute line index using popup's own scroll
    let absolute_line = state.messages_scrolling_state.collapsed_messages_scroll + row_in_popup;
    let rel_col = popup_content_col(state, col);

    state.selection = SelectionState {
        active: true,
        start_line: Some(absolute_line),
        start_col: Some(rel_col),
        end_line: Some(absolute_line),
        end_col: Some(rel_col),
    };
}

/// Handle mouse drag - updates selection in message area, input area, or collapsed popup
pub fn handle_drag(state: &mut AppState, col: u16, row: u16) {
    // When collapsed messages popup is open, use popup geometry
    if state.messages_scrolling_state.show_collapsed_messages {
        handle_popup_drag(state, col, row);
        return;
    }

    // Use the accurate message_area_height from the last render
    let message_area_height = state.message_area_height as usize;

    // Check if we're dragging in input area selection mode
    if state.input_state.text_area.selection.is_active() {
        if let Some(input_area) = state.input_content_area {
            state
                .input_state
                .text_area
                .update_selection(col, row, input_area, &state.input_state.text_area_state);
        }
        return;
    }

    // Handle message area selection
    if !state.selection.active {
        return;
    }

    // Detect if mouse is at or beyond the message area edges for auto-scroll.
    // We check for "at the edge" (<=, >=) rather than strictly beyond, because
    // the message area may start at y=0 where the mouse can never go above it,
    // and similarly the bottom edge may be at the last terminal row.
    let msg_top = state.message_area_y as usize;
    let msg_bottom = msg_top + message_area_height;

    if (row as usize) <= msg_top && state.messages_scrolling_state.scroll > 0 {
        // Mouse is at or above the top edge — trigger auto-scroll up
        // (only if there's content above to scroll to)
        state.selection_auto_scroll = -1;
        // Update selection to the topmost visible line
        let absolute_line = state.messages_scrolling_state.scroll;
        let rel_col = content_col(state, col);
        state.selection.end_line = Some(absolute_line);
        state.selection.end_col = Some(rel_col);
        return;
    } else if message_area_height > 0 && (row as usize) >= msg_bottom.saturating_sub(1) {
        // Mouse is at or below the bottom edge — check if there's content below to scroll to
        let total_lines = state.messages_scrolling_state.assembled_lines_cache
            .as_ref()
            .map(|(_, lines, _)| lines.len())
            .unwrap_or(0);
        let max_scroll = total_lines.saturating_sub(message_area_height);

        if state.messages_scrolling_state.scroll < max_scroll {
            // There's content below — trigger auto-scroll down
            state.selection_auto_scroll = 1;
            // Update selection to the bottommost visible line
            let absolute_line = state.messages_scrolling_state.scroll + message_area_height.saturating_sub(1);
            let rel_col = content_col(state, col);
            state.selection.end_line = Some(absolute_line);
            state.selection.end_col = Some(rel_col);
            return;
        }
    }

    // Mouse is inside the message area — stop auto-scroll
    state.selection_auto_scroll = 0;

    // Clamp row to message area
    // Mouse row is absolute to terminal, so subtract message_area_y to get row relative to message area
    let row_in_message_area = (row as usize).saturating_sub(state.message_area_y as usize);
    let clamped_row = row_in_message_area.min(message_area_height.saturating_sub(1));

    // Convert screen row to absolute line index
    let absolute_line = state.messages_scrolling_state.scroll + clamped_row;

    // Clamp col to main area if side panel is visible, then convert to content-relative
    let clamped_col = if state.show_side_panel {
        let side_panel_width = 32u16;
        let main_area_width = state
            .terminal_size
            .width
            .saturating_sub(side_panel_width + 1);
        col.min(main_area_width.saturating_sub(1))
    } else {
        col
    };
    let rel_col = content_col(state, clamped_col);

    state.selection.end_line = Some(absolute_line);
    state.selection.end_col = Some(rel_col);
}

/// Handle drag within the collapsed messages fullscreen popup
fn handle_popup_drag(state: &mut AppState, col: u16, row: u16) {
    if !state.selection.active {
        return;
    }

    let popup_height = state.collapsed_popup_area_height as usize;

    // Clamp row to popup content area
    let row_in_popup = (row as usize).saturating_sub(state.collapsed_popup_area_y as usize);
    let clamped_row = row_in_popup.min(popup_height.saturating_sub(1));

    // Convert screen row to absolute line index using popup's own scroll
    let absolute_line = state.messages_scrolling_state.collapsed_messages_scroll + clamped_row;
    let rel_col = popup_content_col(state, col);

    state.selection.end_line = Some(absolute_line);
    state.selection.end_col = Some(rel_col);
}

/// Handle mouse drag end - extracts text, copies to clipboard, shows toast
/// Also detects clicks on user messages to show action popup
pub fn handle_drag_end(state: &mut AppState, col: u16, row: u16) {
    // Always reset auto-scroll when mouse is released
    state.selection_auto_scroll = 0;

    // When collapsed messages popup is open, use popup-specific logic
    if state.messages_scrolling_state.show_collapsed_messages {
        handle_popup_drag_end(state, col, row);
        return;
    }

    // Check if we're ending an input area selection
    if state.input_state.text_area.selection.is_active() {
        if let Some(selected_text) = state.input_state.text_area.end_selection()
            && !selected_text.is_empty()
        {
            // Copy to clipboard
            match copy_to_clipboard(&selected_text) {
                Ok(()) => {
                    state.toast = Some(Toast::success("Copied!"));
                }
                Err(e) => {
                    log::warn!("Failed to copy to clipboard: {}", e);
                    state.toast = Some(Toast::error("Copy failed"));
                }
            }
        }
        return;
    }

    // Handle message area selection end
    if !state.selection.active {
        return;
    }

    // Update final position (may re-arm selection_auto_scroll if mouse is at edge)
    handle_drag(state, col, row);
    // Reset auto-scroll again after handle_drag to ensure it's cleared on mouse release
    state.selection_auto_scroll = 0;

    // Check if this was just a click (no actual drag)
    let is_just_click = match (
        &state.selection.start_line,
        &state.selection.end_line,
        &state.selection.start_col,
        &state.selection.end_col,
    ) {
        (Some(sl), Some(el), Some(sc), Some(ec)) => *sl == *el && *sc == *ec,
        _ => true,
    };

    if is_just_click {
        // Just a click, not a selection - check if it's on a user message
        // Mouse row is absolute to terminal, so subtract message_area_y to get row relative to message area
        let row_in_message_area = (row as usize).saturating_sub(state.message_area_y as usize);
        let absolute_line = state.messages_scrolling_state.scroll + row_in_message_area;

        // Clear selection first
        state.selection = SelectionState::default();

        // Check if clicking on a user message
        if let Some((msg_id, msg_text)) = find_user_message_at_line(state, absolute_line) {
            // Show message action popup
            state.show_message_action_popup = true;
            state.message_action_popup_selected = 0;
            state.message_action_popup_position = Some((col, row));
            state.message_action_target_message_id = Some(msg_id);
            state.message_action_target_text = Some(msg_text);
        }

        return;
    }

    // Extract selected text
    let selected_text = extract_selected_text(state);

    // Clear selection
    state.selection = SelectionState::default();

    if selected_text.is_empty() {
        return;
    }

    // Copy to clipboard
    match copy_to_clipboard(&selected_text) {
        Ok(()) => {
            state.toast = Some(Toast::success("Copied!"));
        }
        Err(e) => {
            log::warn!("Failed to copy to clipboard: {}", e);
            state.toast = Some(Toast::error("Copy failed"));
        }
    }
}

/// Handle drag end within the collapsed messages fullscreen popup
fn handle_popup_drag_end(state: &mut AppState, col: u16, row: u16) {
    if !state.selection.active {
        return;
    }

    // Update final position using popup geometry
    handle_popup_drag(state, col, row);

    // Check if this was just a click (no actual drag)
    let is_just_click = match (
        &state.selection.start_line,
        &state.selection.end_line,
        &state.selection.start_col,
        &state.selection.end_col,
    ) {
        (Some(sl), Some(el), Some(sc), Some(ec)) => *sl == *el && *sc == *ec,
        _ => true,
    };

    if is_just_click {
        // In the popup, a click just clears selection (no message action popup)
        state.selection = SelectionState::default();
        return;
    }

    // Extract selected text from the collapsed message lines cache
    let selected_text = extract_selected_text_from_collapsed(state);

    // Clear selection
    state.selection = SelectionState::default();

    if selected_text.is_empty() {
        return;
    }

    // Copy to clipboard
    match copy_to_clipboard(&selected_text) {
        Ok(()) => {
            state.toast = Some(Toast::success("Copied!"));
        }
        Err(e) => {
            log::warn!("Failed to copy to clipboard: {}", e);
            state.toast = Some(Toast::error("Copy failed"));
        }
    }
}

/// Handle scroll during active selection - extends selection in scroll direction
pub fn handle_scroll_during_selection(
    state: &mut AppState,
    direction: i32,
    _message_area_height: usize,
) {
    if !state.selection.active {
        return;
    }

    // Get current end position
    let Some(end_line) = state.selection.end_line else {
        return;
    };

    // Choose the correct cache depending on whether the collapsed popup is open
    let cached_lines: Option<&Vec<ratatui::text::Line<'static>>> = if state.messages_scrolling_state.show_collapsed_messages
    {
        state.messages_scrolling_state.collapsed_message_lines_cache
            .as_ref()
            .map(|(_, _, lines)| lines)
    } else {
        state.messages_scrolling_state.assembled_lines_cache
            .as_ref()
            .map(|(_, lines, _)| lines)
    };

    // Calculate new end line based on scroll direction
    let new_end_line = if direction < 0 {
        // Scrolling up - extend selection upward
        end_line.saturating_sub(1)
    } else {
        // Scrolling down - extend selection downward
        // Get total lines from cache to clamp
        let max_line = cached_lines
            .map(|lines| lines.len().saturating_sub(1))
            .unwrap_or(end_line);
        (end_line + 1).min(max_line)
    };

    state.selection.end_line = Some(new_end_line);

    // Update end column to end of line when extending via scroll
    // This gives a better selection experience
    if let Some(lines) = cached_lines
        && new_end_line < lines.len()
    {
        let line_width: u16 = lines[new_end_line]
            .spans
            .iter()
            .map(|span| unicode_width::UnicodeWidthStr::width(span.content.as_ref()) as u16)
            .sum();

        // If scrolling down, select to end of line
        // If scrolling up, select from start of line
        if direction > 0 {
            state.selection.end_col = Some(line_width);
        } else {
            state.selection.end_col = Some(0);
        }
    }
}

/// Called from the event loop tick to perform auto-scrolling during drag selection.
///
/// When the user drags the mouse above or below the message area while selecting text,
/// `handle_drag` sets `selection_auto_scroll` to -1 (up) or 1 (down). This function is
/// called periodically (every ~100ms via the spinner tick) to scroll the viewport by one
/// line and extend the selection accordingly, providing the standard "drag to edge to scroll"
/// behavior found in text editors.
pub fn tick_selection_auto_scroll(state: &mut AppState) {
    // Safety: only auto-scroll when there's an active message area selection
    if !state.selection.active || state.selection_auto_scroll == 0 {
        state.selection_auto_scroll = 0;
        return;
    }

    // Don't auto-scroll for collapsed popup (it has bounded content and its own scroll)
    if state.messages_scrolling_state.show_collapsed_messages {
        state.selection_auto_scroll = 0;
        return;
    }

    let direction = state.selection_auto_scroll;

    // Get total content lines from cache for bounds checking
    let total_lines = state.messages_scrolling_state.assembled_lines_cache
        .as_ref()
        .map(|(_, lines, _)| lines.len())
        .unwrap_or(0);

    if total_lines == 0 {
        return;
    }

    let message_area_height = state.message_area_height as usize;
    let max_scroll = total_lines.saturating_sub(message_area_height);

    // Perform the scroll
    if direction < 0 {
        // Scroll up
        if state.messages_scrolling_state.scroll == 0 {
            return; // Already at top, nothing to do
        }
        state.messages_scrolling_state.scroll = state.messages_scrolling_state.scroll.saturating_sub(1);
        state.messages_scrolling_state.stay_at_bottom = false;
    } else {
        // Scroll down
        if state.messages_scrolling_state.scroll >= max_scroll {
            return; // Already at bottom, nothing to do
        }
        state.messages_scrolling_state.scroll = (state.messages_scrolling_state.scroll + 1).min(max_scroll);
        if state.messages_scrolling_state.scroll >= max_scroll {
            state.messages_scrolling_state.stay_at_bottom = true;
        }
    }

    // Extend the selection to match the new scroll position
    handle_scroll_during_selection(state, direction, message_area_height);
}
