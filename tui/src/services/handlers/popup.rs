//! Popup Event Handlers
//!
//! Handles all popup-related events including profile switcher, rulebook switcher, command palette, shortcuts, collapsed messages, and context popup.

use crate::app::{AppState, OutputEvent};
use crate::services::approval_popup::PopupService;
use crate::services::detect_term::AdaptiveColors;
use crate::services::helper_block::welcome_messages;
use crate::services::message::{
    Message, get_wrapped_collapsed_message_lines_cached, invalidate_message_lines_cache,
};
use ratatui::style::Style;
use stakpak_api::ListRuleBook;
use stakpak_api::models::RecoveryOptionsResponse;
use tokio::sync::mpsc::Sender;

/// Filter rulebooks based on search input
fn filter_rulebooks(state: &mut AppState) {
    if state.rulebook_search_input.is_empty() {
        state.filtered_rulebooks = state.available_rulebooks.clone();
    } else {
        let search_term = state.rulebook_search_input.to_lowercase();
        state.filtered_rulebooks = state
            .available_rulebooks
            .iter()
            .filter(|rulebook| {
                rulebook.uri.to_lowercase().contains(&search_term)
                    || rulebook.description.to_lowercase().contains(&search_term)
                    || rulebook
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&search_term))
            })
            .cloned()
            .collect();
    }

    // Reset selection if it's out of bounds
    if state.rulebook_switcher_selected >= state.filtered_rulebooks.len() {
        state.rulebook_switcher_selected = 0;
    }
}

// ========== Profile Switcher Handlers ==========

/// Handle show profile switcher event
pub fn handle_show_profile_switcher(state: &mut AppState) {
    // Don't show profile switcher if input is blocked or dialog is open
    if state.profile_switching_in_progress
        || state.is_dialog_open
        || state.approval_popup.is_visible()
    {
        return;
    }

    state.show_profile_switcher = true;
    state.profile_switcher_selected = 0;

    // Pre-select current profile
    if let Some(idx) = state
        .available_profiles
        .iter()
        .position(|p| p == &state.current_profile_name)
    {
        state.profile_switcher_selected = idx;
    }
}

/// Handle profile switcher select event
pub fn handle_profile_switcher_select(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    // Don't process if switching is already in progress
    if state.profile_switching_in_progress {
        return;
    }

    if state.show_profile_switcher && !state.available_profiles.is_empty() {
        let selected_profile = state.available_profiles[state.profile_switcher_selected].clone();

        // Don't switch if already on this profile
        if selected_profile == state.current_profile_name {
            state.show_profile_switcher = false;
            return;
        }

        // Send request to switch profile
        let _ = output_tx.try_send(OutputEvent::RequestProfileSwitch(selected_profile));
    }
}

/// Handle profile switcher cancel event
pub fn handle_profile_switcher_cancel(state: &mut AppState) {
    state.show_profile_switcher = false;
}

/// Handle profiles loaded event
pub fn handle_profiles_loaded(
    state: &mut AppState,
    profiles: Vec<String>,
    _current_profile: String,
) {
    // Only update the available profiles list
    // Do NOT update current_profile_name - it's already set correctly when TUI starts
    state.available_profiles = profiles;
}

/// Handle profile switch requested event
pub fn handle_profile_switch_requested(state: &mut AppState, profile: String) {
    state.profile_switching_in_progress = true;
    state.show_profile_switcher = false;

    // Clear profile switcher state immediately to prevent stray selects
    state.profile_switcher_selected = 0;

    state.profile_switch_status_message = Some(format!("🔄 Switching to profile: {}", profile));

    state.messages.push(Message::info(
        format!("🔄 Switching to profile: {}", profile),
        None,
    ));
}

/// Handle profile switch progress event
pub fn handle_profile_switch_progress(state: &mut AppState, message: String) {
    state.profile_switch_status_message = Some(message.clone());
    state.messages.push(Message::info(message.clone(), None));
}

/// Handle profile switch complete event
pub fn handle_profile_switch_complete(state: &mut AppState, profile: String) {
    // Clear EVERYTHING
    state.messages.clear();
    state.session_tool_calls_queue.clear();
    state.completed_tool_calls.clear();
    state.streaming_tool_results.clear();
    state.active_shell_command = None;
    state.shell_tool_calls = None;
    state.message_tool_calls = None;
    state.message_approved_tools.clear();
    state.message_rejected_tools.clear();
    state.has_user_messages = false;
    state.scroll = 0;
    state.scroll_to_bottom = true;
    state.stay_at_bottom = true;
    state.tool_call_execution_order.clear();
    state.last_message_tool_calls.clear();

    // Clear shell mode state
    state.show_shell_mode = false;
    state.shell_mode_input.clear();
    state.waiting_for_shell_input = false;
    state.active_shell_command_output = None;
    state.is_tool_call_shell_command = false;
    state.ondemand_shell_mode = false;

    // Clear file search
    state.filtered_files.clear();

    // Clear dialog state
    state.is_dialog_open = false;
    state.dialog_command = None;
    state.show_sessions_dialog = false;
    state.show_shortcuts = false;
    state.show_collapsed_messages = false;
    state.approval_popup = PopupService::new();

    // Clear retry state
    state.retry_attempts = 0;
    state.last_user_message_for_retry = None;
    state.is_retrying = false;

    // CRITICAL: Close profile switcher to prevent stray selects
    state.show_profile_switcher = false;
    state.profile_switcher_selected = 0;

    // Update profile info
    state.current_profile_name = profile.clone();
    state.profile_switching_in_progress = false;
    state.profile_switch_status_message = None;

    // Show success and welcome messages
    state.messages.push(Message::info(
        format!("✅ Successfully switched to profile: {}", profile),
        Some(Style::default().fg(AdaptiveColors::green())),
    ));

    let welcome_msg = welcome_messages(state.latest_version.clone(), state);
    state.messages.extend(welcome_msg);

    // Invalidate all caches
    invalidate_message_lines_cache(state);
}

/// Handle profile switch failed event
pub fn handle_profile_switch_failed(state: &mut AppState, error: String) {
    state.profile_switching_in_progress = false;
    state.profile_switch_status_message = None;
    state.show_profile_switcher = false;

    state.messages.push(Message::info(
        format!("❌ Profile switch failed: {}", error),
        Some(Style::default().fg(AdaptiveColors::red())),
    ));
    state.messages.push(Message::info(
        "Staying in current profile. Press Ctrl+P to try again.",
        None,
    ));
}

// ========== Rulebook Switcher Handlers ==========

/// Handle show rulebook switcher event
pub fn handle_show_rulebook_switcher(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    // Don't show rulebook switcher if input is blocked or dialog is open
    if state.profile_switching_in_progress
        || state.is_dialog_open
        || state.approval_popup.is_visible()
    {
        return;
    }

    // Clear any pending input to prevent empty message submission
    state.text_area.set_text("");

    // Request current active rulebooks to pre-select them
    let _ = output_tx.try_send(OutputEvent::RequestCurrentRulebooks);

    state.show_rulebook_switcher = true;
    state.rulebook_switcher_selected = 0;
    state.rulebook_search_input.clear();
    filter_rulebooks(state);
}

/// Handle rulebook switcher select event
pub fn handle_rulebook_switcher_select(state: &mut AppState) {
    if state.show_rulebook_switcher && !state.filtered_rulebooks.is_empty() {
        let selected_rulebook = &state.filtered_rulebooks[state.rulebook_switcher_selected];

        // Toggle selection
        if state.selected_rulebooks.contains(&selected_rulebook.uri) {
            state.selected_rulebooks.remove(&selected_rulebook.uri);
        } else {
            state
                .selected_rulebooks
                .insert(selected_rulebook.uri.clone());
        }
    }
}

/// Handle rulebook switcher toggle event
pub fn handle_rulebook_switcher_toggle(state: &mut AppState) {
    if state.show_rulebook_switcher && !state.filtered_rulebooks.is_empty() {
        let selected_rulebook = &state.filtered_rulebooks[state.rulebook_switcher_selected];

        // Toggle selection
        if state.selected_rulebooks.contains(&selected_rulebook.uri) {
            state.selected_rulebooks.remove(&selected_rulebook.uri);
        } else {
            state
                .selected_rulebooks
                .insert(selected_rulebook.uri.clone());
        }
    }
}

/// Handle rulebook switcher cancel event
pub fn handle_rulebook_switcher_cancel(state: &mut AppState) {
    state.show_rulebook_switcher = false;
}

/// Handle rulebook switcher confirm event
pub fn handle_rulebook_switcher_confirm(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    if state.show_rulebook_switcher {
        // Send the selected rulebooks to the CLI
        let selected_uris: Vec<String> = state.selected_rulebooks.iter().cloned().collect();
        let _ = output_tx.try_send(OutputEvent::RequestRulebookUpdate(selected_uris));

        // Close the switcher
        state.show_rulebook_switcher = false;

        // Show confirmation message
        let count = state.selected_rulebooks.len();
        state.messages.push(Message::info(
            format!(
                "Selected {} rulebook(s). They will be applied to your next message.",
                count
            ),
            Some(Style::default().fg(AdaptiveColors::green())),
        ));
    }
}

/// Handle rulebook switcher select all event
pub fn handle_rulebook_switcher_select_all(state: &mut AppState) {
    if state.show_rulebook_switcher {
        // Select all filtered rulebooks
        state.selected_rulebooks.clear();
        for rulebook in &state.filtered_rulebooks {
            state.selected_rulebooks.insert(rulebook.uri.clone());
        }
    }
}

/// Handle rulebook switcher deselect all event
pub fn handle_rulebook_switcher_deselect_all(state: &mut AppState) {
    if state.show_rulebook_switcher {
        // Deselect all rulebooks
        state.selected_rulebooks.clear();
    }
}

/// Handle rulebook search input changed event
pub fn handle_rulebook_search_input_changed(state: &mut AppState, c: char) {
    if state.show_rulebook_switcher {
        state.rulebook_search_input.push(c);
        filter_rulebooks(state);
    }
}

/// Handle rulebook search backspace event
pub fn handle_rulebook_search_backspace(state: &mut AppState) {
    if state.show_rulebook_switcher && !state.rulebook_search_input.is_empty() {
        state.rulebook_search_input.pop();
        filter_rulebooks(state);
    }
}

/// Handle rulebooks loaded event
pub fn handle_rulebooks_loaded(state: &mut AppState, rulebooks: Vec<ListRuleBook>) {
    state.available_rulebooks = rulebooks;
    filter_rulebooks(state);
}

/// Handle current rulebooks loaded event
pub fn handle_current_rulebooks_loaded(state: &mut AppState, current_uris: Vec<String>) {
    // Set the currently active rulebooks as selected
    state.selected_rulebooks = current_uris.into_iter().collect();
}

// ========== Command Palette Handlers ==========

/// Handle show command palette event
pub fn handle_show_command_palette(state: &mut AppState) {
    // Don't show command palette if input is blocked or dialog is open
    if state.profile_switching_in_progress
        || state.is_dialog_open
        || state.approval_popup.is_visible()
    {
        return;
    }

    state.show_command_palette = true;
    state.command_palette_selected = 0;
    state.command_palette_scroll = 0;
    state.command_palette_search = String::new();
}

/// Handle command palette search input changed event
pub fn handle_command_palette_search_input_changed(state: &mut AppState, c: char) {
    if state.show_command_palette {
        state.command_palette_search.push(c);
        state.command_palette_selected = 0;
    }
}

/// Handle command palette search backspace event
pub fn handle_command_palette_search_backspace(state: &mut AppState) {
    if state.show_command_palette && !state.command_palette_search.is_empty() {
        state.command_palette_search.pop();
        state.command_palette_selected = 0;
    }
}

// ========== Shortcuts Popup Handlers ==========

/// Handle show shortcuts event
pub fn handle_show_shortcuts(state: &mut AppState) {
    // Don't show shortcuts popup if input is blocked or dialog is open
    if state.profile_switching_in_progress
        || state.is_dialog_open
        || state.approval_popup.is_visible()
        || state.show_profile_switcher
    {
        return;
    }

    state.show_shortcuts_popup = true;
}

/// Handle shortcuts cancel event
pub fn handle_shortcuts_cancel(state: &mut AppState) {
    state.show_shortcuts_popup = false;
}

/// Handle toggle more shortcuts event
pub fn handle_toggle_more_shortcuts(state: &mut AppState) {
    state.show_shortcuts = !state.show_shortcuts;
}

// ========== Collapsed Messages Handlers ==========

/// Handle toggle collapsed messages event
pub fn handle_toggle_collapsed_messages(
    state: &mut AppState,
    message_area_height: usize,
    message_area_width: usize,
) {
    // If approval popup is visible, toggle its maximize state instead
    if state.approval_popup.is_visible() {
        state.approval_popup.toggle_maximize();
        return;
    }

    // Otherwise, handle collapsed messages popup as usual
    state.show_collapsed_messages = !state.show_collapsed_messages;
    if state.show_collapsed_messages {
        // Calculate scroll position to show the top of the last message
        let collapsed_messages: Vec<Message> = state
            .messages
            .iter()
            .filter(|m| m.is_collapsed == Some(true))
            .cloned()
            .collect();

        if !collapsed_messages.is_empty() {
            // Set selected to the last message
            state.collapsed_messages_selected = collapsed_messages.len() - 1;

            // Get all collapsed message lines once
            let all_lines = get_wrapped_collapsed_message_lines_cached(state, message_area_width);

            // Calculate scroll to show the top of the last message
            // For now, just scroll to the bottom to show the last message
            let total_lines = all_lines.len();
            let max_scroll = total_lines.saturating_sub(message_area_height);
            state.collapsed_messages_scroll = max_scroll;
        } else {
            state.collapsed_messages_scroll = 0;
            state.collapsed_messages_selected = 0;
        }
    }
}

// ========== Context Popup Handlers ==========

/// Handle toggle context popup event
pub fn handle_toggle_context_popup(state: &mut AppState) {
    state.show_context_popup = !state.show_context_popup;
}

// ========== Recovery Options Handlers ==========

/// Handle recovery options event
pub fn handle_recovery_options(state: &mut AppState, response: RecoveryOptionsResponse) {
    state.recovery_options = response.recovery_options.clone();
    state.recovery_response = Some(response);
    state.recovery_popup_selected = 0;
    if state.recovery_options.is_empty() {
        state.show_recovery_options_popup = false;
    }
}

/// Handle expand notifications event (recovery options popup toggle)
pub fn handle_expand_notifications(state: &mut AppState) {
    if state.recovery_options.is_empty() {
        return;
    }

    if state.show_recovery_options_popup {
        state.show_recovery_options_popup = false;
    } else {
        state.show_recovery_options_popup = true;
        state.recovery_popup_selected = 0;
    }
}
