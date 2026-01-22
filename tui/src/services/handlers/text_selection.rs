//! Text selection handler for mouse-based text selection in the message area.
//!
//! This module handles:
//! - Starting selection on mouse drag start
//! - Updating selection during mouse drag
//! - Ending selection and copying to clipboard on mouse release
//! - Extracting clean text (excluding borders, decorations)
//! - Cursor positioning in input area on click

use crate::app::AppState;
use crate::services::text_selection::{SelectionState, copy_to_clipboard, extract_selected_text};
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

/// Handle mouse drag start - begins text selection in message area or input area
pub fn handle_drag_start(state: &mut AppState, col: u16, row: u16, message_area_height: usize) {
    // First check if click is in input area
    if is_in_input_area(state, col, row) {
        // Click was in input area - start input selection
        if let Some(input_area) = state.input_content_area {
            state
                .text_area
                .start_selection(col, row, input_area, &state.text_area_state);
        }
        // Clear message area selection
        state.selection = SelectionState::default();
        return;
    }

    // Clear any input area selection when clicking outside
    state.text_area.clear_selection();

    // Check if click is within message area (top portion of screen)
    // Message area starts at row 0 and extends to message_area_height
    if row as usize >= message_area_height {
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

    // Convert screen row to absolute line index
    let absolute_line = state.scroll + row as usize;

    state.selection = SelectionState {
        active: true,
        start_line: Some(absolute_line),
        start_col: Some(col),
        end_line: Some(absolute_line),
        end_col: Some(col),
    };
}

/// Handle mouse drag - updates selection in message area or input area
pub fn handle_drag(state: &mut AppState, col: u16, row: u16, message_area_height: usize) {
    // Check if we're dragging in input area selection mode
    if state.text_area.selection.is_active() {
        if let Some(input_area) = state.input_content_area {
            state
                .text_area
                .update_selection(col, row, input_area, &state.text_area_state);
        }
        return;
    }

    // Handle message area selection
    if !state.selection.active {
        return;
    }

    // Clamp row to message area
    let clamped_row = (row as usize).min(message_area_height.saturating_sub(1));

    // Convert screen row to absolute line index
    let absolute_line = state.scroll + clamped_row;

    // Clamp col to main area if side panel is visible
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

    state.selection.end_line = Some(absolute_line);
    state.selection.end_col = Some(clamped_col);
}

/// Handle mouse drag end - extracts text, copies to clipboard, shows toast
pub fn handle_drag_end(state: &mut AppState, col: u16, row: u16, message_area_height: usize) {
    // Check if we're ending an input area selection
    if state.text_area.selection.is_active() {
        if let Some(selected_text) = state.text_area.end_selection()
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

    // Update final position
    handle_drag(state, col, row, message_area_height);

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
        // Just a click, not a selection - clear and return
        state.selection = SelectionState::default();
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

    // Calculate new end line based on scroll direction
    let new_end_line = if direction < 0 {
        // Scrolling up - extend selection upward
        end_line.saturating_sub(1)
    } else {
        // Scrolling down - extend selection downward
        // Get total lines from cache to clamp
        let max_line = state
            .assembled_lines_cache
            .as_ref()
            .map(|(_, lines, _)| lines.len().saturating_sub(1))
            .unwrap_or(end_line);
        (end_line + 1).min(max_line)
    };

    state.selection.end_line = Some(new_end_line);

    // Update end column to end of line when extending via scroll
    // This gives a better selection experience
    if let Some((_, cached_lines, _)) = &state.assembled_lines_cache
        && new_end_line < cached_lines.len()
    {
        let line_width: u16 = cached_lines[new_end_line]
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
