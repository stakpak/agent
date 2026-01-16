//! Navigation Event Handlers
//!
//! Handles all navigation-related events including scrolling, page navigation, and dropdown navigation.

use crate::app::AppState;
use crate::constants::SCROLL_LINES;
use crate::services::commands::filter_commands;
use crate::services::message::{
    get_wrapped_collapsed_message_lines_cached, get_wrapped_message_lines_cached,
};

/// Updates helper dropdown scroll position to keep selected item visible
fn update_helper_dropdown_scroll(state: &mut AppState) {
    let commands_to_show = if state.input() == "/" {
        &state.helpers
    } else {
        &state.filtered_helpers
    };

    let total_commands = commands_to_show.len();
    if total_commands == 0 {
        return;
    }

    const MAX_VISIBLE_ITEMS: usize = 5;
    let visible_height = MAX_VISIBLE_ITEMS.min(total_commands);

    // Calculate the scroll position to keep the selected item visible
    if state.helper_selected < state.helper_scroll {
        // Selected item is above visible area, scroll up
        state.helper_scroll = state.helper_selected;
    } else if state.helper_selected >= state.helper_scroll + visible_height {
        // Selected item is below visible area, scroll down
        state.helper_scroll = state.helper_selected - visible_height + 1;
    }

    // Ensure scroll doesn't go beyond bounds
    let max_scroll = total_commands.saturating_sub(visible_height);
    if state.helper_scroll > max_scroll {
        state.helper_scroll = max_scroll;
    }
}

/// Updates command palette scroll position to keep selected item visible
fn update_command_palette_scroll(state: &mut AppState) {
    let filtered_commands = filter_commands(&state.command_palette_search);
    let total_commands = filtered_commands.len();

    if total_commands == 0 {
        return;
    }

    // Assume a fixed height for the command list (adjust based on your popup height)
    let visible_height = 6; // Adjust this based on your actual popup height

    // Calculate the scroll position to keep the selected item visible
    if state.command_palette_selected < state.command_palette_scroll {
        // Selected item is above visible area, scroll up
        state.command_palette_scroll = state.command_palette_selected;
    } else if state.command_palette_selected >= state.command_palette_scroll + visible_height {
        // Selected item is below visible area, scroll down
        state.command_palette_scroll = state.command_palette_selected - visible_height + 1;
    }

    // Ensure scroll doesn't go beyond bounds
    let max_scroll = total_commands.saturating_sub(visible_height);
    if state.command_palette_scroll > max_scroll {
        state.command_palette_scroll = max_scroll;
    }
}

/// Handle dropdown up navigation
pub fn handle_dropdown_up(state: &mut AppState) {
    if state.show_helper_dropdown && state.helper_selected > 0 {
        if state.file_search.is_active() {
            // File file_search mode
            state.helper_selected -= 1;
        } else {
            // Regular helper mode
            if !state.filtered_helpers.is_empty() && state.input().starts_with('/') {
                state.helper_selected -= 1;
                update_helper_dropdown_scroll(state);
            }
        }
    }
}

/// Handle dropdown down navigation
pub fn handle_dropdown_down(state: &mut AppState) {
    if state.show_helper_dropdown {
        if state.file_search.is_active() {
            // File file_search mode
            if state.helper_selected + 1 < state.file_search.filtered_count() {
                state.helper_selected += 1;
            }
        } else {
            // Regular helper mode
            let commands_to_show = if state.input() == "/" {
                &state.helpers
            } else {
                &state.filtered_helpers
            };

            if !commands_to_show.is_empty()
                && state.input().starts_with('/')
                && state.helper_selected + 1 < commands_to_show.len()
            {
                state.helper_selected += 1;
                update_helper_dropdown_scroll(state);
            }
        }
    }
}

/// Handles upward navigation with approval popup check
pub fn handle_up_navigation(state: &mut AppState) {
    if state.show_profile_switcher {
        if state.profile_switcher_selected > 0 {
            state.profile_switcher_selected -= 1;
        } else {
            state.profile_switcher_selected = state.available_profiles.len().saturating_sub(1);
        }
        return;
    }
    if state.show_shortcuts_popup {
        match state.shortcuts_popup_mode {
            crate::app::ShortcutsPopupMode::Commands => {
                // Navigate commands list
                let filtered_commands = filter_commands(&state.command_palette_search);
                if state.command_palette_selected > 0 {
                    state.command_palette_selected -= 1;
                } else {
                    state.command_palette_selected = filtered_commands.len().saturating_sub(1);
                }
                update_command_palette_scroll(state);
            }
            crate::app::ShortcutsPopupMode::Shortcuts => {
                // Scroll shortcuts content
                state.shortcuts_scroll = state.shortcuts_scroll.saturating_sub(SCROLL_LINES);
            }
        }
        return;
    }
    if state.show_rulebook_switcher {
        if state.rulebook_switcher_selected > 0 {
            state.rulebook_switcher_selected -= 1;
        } else {
            state.rulebook_switcher_selected = state.filtered_rulebooks.len().saturating_sub(1);
        }
        return;
    }
    // Check if approval popup is visible and should consume the event
    if state.approval_popup.is_visible() {
        state.approval_popup.scroll_up();
        return; // Event was consumed by popup
    }

    // Handle different UI states
    if state.show_sessions_dialog {
        if state.session_selected > 0 {
            state.session_selected -= 1;
        }
    } else if state.show_helper_dropdown {
        handle_dropdown_up(state);
    } else if state.is_dialog_open && state.dialog_focused {
        // Handle dialog navigation only when dialog is focused
        if state.dialog_selected > 0 {
            state.dialog_selected -= 1;
        } else {
            // Wrap to the last option
            state.dialog_selected = 2;
        }
    } else {
        handle_scroll_up(state);
    }
}

/// Handles downward navigation with approval popup check
pub fn handle_down_navigation(
    state: &mut AppState,
    message_area_height: usize,
    message_area_width: usize,
) {
    if state.show_profile_switcher {
        if state.profile_switcher_selected < state.available_profiles.len().saturating_sub(1) {
            state.profile_switcher_selected += 1;
        } else {
            state.profile_switcher_selected = 0;
        }
        return;
    }
    if state.show_shortcuts_popup {
        match state.shortcuts_popup_mode {
            crate::app::ShortcutsPopupMode::Commands => {
                // Navigate commands list
                let filtered_commands = filter_commands(&state.command_palette_search);
                if state.command_palette_selected < filtered_commands.len().saturating_sub(1) {
                    state.command_palette_selected += 1;
                } else {
                    state.command_palette_selected = 0;
                }
                update_command_palette_scroll(state);
            }
            crate::app::ShortcutsPopupMode::Shortcuts => {
                // Scroll shortcuts content
                state.shortcuts_scroll = state.shortcuts_scroll.saturating_add(SCROLL_LINES);
            }
        }
        return;
    }
    if state.show_rulebook_switcher {
        if state.rulebook_switcher_selected < state.filtered_rulebooks.len().saturating_sub(1) {
            state.rulebook_switcher_selected += 1;
        } else {
            state.rulebook_switcher_selected = 0;
        }
    }
    // Check if approval popup is visible and should consume the event
    if state.approval_popup.is_visible() {
        state.approval_popup.scroll_down();
        return; // Event was consumed by popup
    }

    // Handle different UI states
    if state.show_sessions_dialog {
        if state.session_selected + 1 < state.sessions.len() {
            state.session_selected += 1;
        }
    } else if state.show_helper_dropdown {
        handle_dropdown_down(state);
    } else if state.is_dialog_open && state.dialog_focused {
        // Handle dialog navigation only when dialog is focused
        if state.dialog_selected < 2 {
            state.dialog_selected += 1;
        } else {
            // Wrap to the first option
            state.dialog_selected = 0;
        }
    } else {
        handle_scroll_down(state, message_area_height, message_area_width);
    }
}

/// Handle scroll up
fn handle_scroll_up(state: &mut AppState) {
    if state.show_collapsed_messages {
        if state.collapsed_messages_scroll >= SCROLL_LINES {
            state.collapsed_messages_scroll -= SCROLL_LINES;
        } else {
            state.collapsed_messages_scroll = 0;
        }
    } else if state.scroll >= SCROLL_LINES {
        state.scroll -= SCROLL_LINES;
        state.stay_at_bottom = false;
    } else {
        state.scroll = 0;
        state.stay_at_bottom = false;
    }
}

/// Handle scroll down
fn handle_scroll_down(state: &mut AppState, message_area_height: usize, message_area_width: usize) {
    if state.show_collapsed_messages {
        // For collapsed messages popup, we need to calculate scroll based on collapsed messages only
        let total_lines = if let Some((_, _, cached_lines)) = &state.collapsed_message_lines_cache {
            cached_lines.len()
        } else {
            // Fallback: calculate once and cache
            let all_lines = get_wrapped_collapsed_message_lines_cached(state, message_area_width);
            all_lines.len()
        };

        let max_scroll = total_lines.saturating_sub(message_area_height);
        if state.collapsed_messages_scroll + SCROLL_LINES < max_scroll {
            state.collapsed_messages_scroll += SCROLL_LINES;
        } else {
            state.collapsed_messages_scroll = max_scroll;
        }
    } else {
        // Use cached line count instead of recalculating every scroll
        let total_lines = if let Some((_, _, cached_lines)) = &state.message_lines_cache {
            cached_lines.len()
        } else {
            // Fallback: calculate once and cache
            let all_lines = get_wrapped_message_lines_cached(state, message_area_width);
            all_lines.len()
        };

        let max_scroll = total_lines.saturating_sub(message_area_height);
        if state.scroll + SCROLL_LINES < max_scroll {
            state.scroll += SCROLL_LINES;
            state.stay_at_bottom = false;
        } else {
            state.scroll = max_scroll;
            state.stay_at_bottom = true;

            // If content changed while we were scrolled up, invalidate cache once
            // to catch up with new content that arrived while scrolled up
            if state.content_changed_while_scrolled_up {
                crate::services::message::invalidate_message_lines_cache(state);
                state.content_changed_while_scrolled_up = false;
            }
        }
    }
}

/// Handle page up navigation
pub fn handle_page_up(state: &mut AppState, message_area_height: usize, message_area_width: usize) {
    state.stay_at_bottom = false; // unlock from bottom
    let input_height = 3;
    let page = std::cmp::max(1, message_area_height.saturating_sub(input_height));
    if state.scroll >= page {
        state.scroll -= page;
    } else {
        state.scroll = 0;
    }
    adjust_scroll(state, message_area_height, message_area_width);
}

/// Handle page down navigation
pub fn handle_page_down(
    state: &mut AppState,
    message_area_height: usize,
    message_area_width: usize,
) {
    state.stay_at_bottom = false; // unlock from bottom
    // Use cached line count instead of recalculating every page operation
    let total_lines = if let Some((_, _, cached_lines)) = &state.message_lines_cache {
        cached_lines.len()
    } else {
        // Fallback: calculate once and cache
        let all_lines = get_wrapped_message_lines_cached(state, message_area_width);
        all_lines.len()
    };

    let max_scroll = total_lines.saturating_sub(message_area_height);
    let page = std::cmp::max(1, message_area_height);
    if state.scroll < max_scroll {
        state.scroll = (state.scroll + page).min(max_scroll);
        if state.scroll == max_scroll {
            state.stay_at_bottom = true;

            // If content changed while we were scrolled up, invalidate cache once
            if state.content_changed_while_scrolled_up {
                crate::services::message::invalidate_message_lines_cache(state);
                state.content_changed_while_scrolled_up = false;
            }
        }
    } else {
        state.stay_at_bottom = true;
    }
    adjust_scroll(state, message_area_height, message_area_width);
}

/// Adjust scroll position based on state
pub fn adjust_scroll(state: &mut AppState, message_area_height: usize, message_area_width: usize) {
    // Use cached line count instead of recalculating every adjustment
    let total_lines = if let Some((_, _, cached_lines)) = &state.message_lines_cache {
        cached_lines.len()
    } else {
        // Fallback: calculate once and cache
        let all_lines = get_wrapped_message_lines_cached(state, message_area_width);
        all_lines.len()
    };

    let max_scroll = total_lines.saturating_sub(message_area_height);
    if state.stay_at_bottom {
        state.scroll = max_scroll;
    } else if state.scroll_to_bottom {
        state.scroll = max_scroll;
        state.scroll_to_bottom = false;
    } else if state.scroll > max_scroll {
        state.scroll = max_scroll;
    }
}
