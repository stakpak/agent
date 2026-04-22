//! Popup Event Handlers
//!
//! Handles all popup-related events including profile switcher, rulebook switcher, model switcher, command palette, shortcuts, collapsed messages, and context popup.

use crate::app::{AppState, AutoApprovePopupRow, InputEvent, OutputEvent};
use crate::services::auto_approve::AutoApprovePolicy;
use crate::services::changeset::Changeset;
use crate::services::detect_term::ThemeColors;
use crate::services::helper_block::{push_error_message, push_styled_message, welcome_messages};
use crate::services::message::{
    Message, get_wrapped_collapsed_message_lines_cached, invalidate_message_lines_cache,
};
use crate::services::text_selection::SelectionState;
use ratatui::style::Style;
use stakai::Model;
use stakpak_api::models::ListRuleBook;
use tokio::sync::mpsc::Sender;

/// Format a model's provider and ID into normalized `"provider/short_name"` for recent_models.
///
/// Strips long upstream paths (e.g., `"fireworks-ai/accounts/fireworks/models/glm-5"`)
/// down to just the last segment, producing clean entries like `"stakpak/glm-5"`.
fn format_recent_model_id(provider: &str, model_id: &str) -> String {
    let short_name = model_id.rsplit('/').next().unwrap_or(model_id);
    format!("{}/{}", provider, short_name)
}

/// Filter rulebooks based on search input
fn filter_rulebooks(state: &mut AppState) {
    if state
        .rulebook_switcher_state
        .rulebook_search_input
        .is_empty()
    {
        state.rulebook_switcher_state.filtered_rulebooks =
            state.rulebook_switcher_state.available_rulebooks.clone();
    } else {
        let search_term = state
            .rulebook_switcher_state
            .rulebook_search_input
            .to_lowercase();
        state.rulebook_switcher_state.filtered_rulebooks = state
            .rulebook_switcher_state
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
    if state.rulebook_switcher_state.is_selected
        >= state.rulebook_switcher_state.filtered_rulebooks.len()
    {
        state.rulebook_switcher_state.is_selected = 0;
    }
}

// ========== Profile Switcher Handlers ==========

/// Handle show profile switcher event
pub fn handle_show_profile_switcher(state: &mut AppState) {
    // Don't show profile switcher if input is blocked or dialog is open
    if state.profile_switcher_state.switching_in_progress
        || state.dialog_approval_state.is_dialog_open
        || state.dialog_approval_state.approval_bar.is_visible()
    {
        return;
    }

    state.profile_switcher_state.show_profile_switcher = true;
    state.profile_switcher_state.selected_index = 0;

    // Pre-select current profile
    if let Some(idx) = state
        .profile_switcher_state
        .available_profiles
        .iter()
        .position(|p| p == &state.profile_switcher_state.current_profile_name)
    {
        state.profile_switcher_state.selected_index = idx;
    }
}

/// Handle profile switcher select event
pub fn handle_profile_switcher_select(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    // Don't process if switching is already in progress
    if state.profile_switcher_state.switching_in_progress {
        return;
    }

    if state.profile_switcher_state.show_profile_switcher
        && !state.profile_switcher_state.available_profiles.is_empty()
    {
        let selected_profile = state.profile_switcher_state.available_profiles
            [state.profile_switcher_state.selected_index]
            .clone();

        // Don't switch if already on this profile
        if selected_profile == state.profile_switcher_state.current_profile_name {
            state.profile_switcher_state.show_profile_switcher = false;
            return;
        }

        // Send request to switch profile
        let _ = output_tx.try_send(OutputEvent::RequestProfileSwitch(selected_profile));
    }
}

/// Handle profile switcher cancel event
pub fn handle_profile_switcher_cancel(state: &mut AppState) {
    state.profile_switcher_state.show_profile_switcher = false;
}

/// Handle profiles loaded event
pub fn handle_profiles_loaded(
    state: &mut AppState,
    profiles: Vec<String>,
    _current_profile: String,
) {
    // Only update the available profiles list
    // Do NOT update current_profile_name - it's already set correctly when TUI starts
    state.profile_switcher_state.available_profiles = profiles;
}

/// Handle profile switch requested event
pub fn handle_profile_switch_requested(state: &mut AppState, profile: String) {
    state.profile_switcher_state.switching_in_progress = true;
    state.profile_switcher_state.show_profile_switcher = false;

    // Clear profile switcher state immediately to prevent stray selects
    state.profile_switcher_state.selected_index = 0;

    state.profile_switcher_state.switch_status_message =
        Some(format!("🔄 Switching to profile: {}", profile));

    state.messages_scrolling_state.messages.push(Message::info(
        format!("🔄 Switching to profile: {}", profile),
        None,
    ));
}

/// Handle profile switch progress event
pub fn handle_profile_switch_progress(state: &mut AppState, message: String) {
    state.profile_switcher_state.switch_status_message = Some(message.clone());
    state
        .messages_scrolling_state
        .messages
        .push(Message::info(message.clone(), None));
}

/// Handle profile switch complete event
pub fn handle_profile_switch_complete(state: &mut AppState, profile: String) {
    // Clear EVERYTHING
    state.messages_scrolling_state.messages.clear();
    state
        .session_tool_calls_state
        .session_tool_calls_queue
        .clear();
    state.tool_call_state.completed_tool_calls.clear();
    state.tool_call_state.streaming_tool_results.clear();
    state.shell_popup_state.active_shell_command = None;
    state.shell_popup_state.shell_tool_calls = None;
    state.dialog_approval_state.message_tool_calls = None;
    state.dialog_approval_state.message_approved_tools.clear();
    state.dialog_approval_state.message_rejected_tools.clear();
    state.messages_scrolling_state.has_user_messages = false;
    state.messages_scrolling_state.scroll = 0;
    state.messages_scrolling_state.scroll_to_bottom = true;
    state.messages_scrolling_state.stay_at_bottom = true;
    state
        .session_tool_calls_state
        .tool_call_execution_order
        .clear();
    state
        .session_tool_calls_state
        .last_message_tool_calls
        .clear();

    // Clear shell mode state
    state.shell_popup_state.is_visible = false;
    state.shell_popup_state.is_expanded = false;
    state.shell_popup_state.waiting_for_shell_input = false;
    state.shell_popup_state.active_shell_command_output = None;
    state.shell_popup_state.is_tool_call_shell_command = false;
    state.shell_popup_state.ondemand_shell_mode = false;

    // Clear file search
    state.input_state.filtered_files.clear();

    // Clear dialog state
    state.dialog_approval_state.is_dialog_open = false;
    state.dialog_approval_state.dialog_command = None;
    state.dialog_approval_state.show_shortcuts = false;
    state.messages_scrolling_state.show_collapsed_messages = false;
    state.dialog_approval_state.approval_bar.clear();

    // Clear retry state
    state.tool_call_state.retry_attempts = 0;
    state.tool_call_state.last_user_message_for_retry = None;
    state.tool_call_state.is_retrying = false;

    // Clear changeset and todos from previous session
    state.side_panel_state.changeset = Changeset::default();
    state.side_panel_state.todos.clear();

    // CRITICAL: Close profile switcher to prevent stray selects
    state.profile_switcher_state.show_profile_switcher = false;
    state.profile_switcher_state.selected_index = 0;

    // Update profile info
    state.profile_switcher_state.current_profile_name = profile.clone();
    state.profile_switcher_state.switching_in_progress = false;
    state.profile_switcher_state.switch_status_message = None;

    // Show success and welcome messages
    state.messages_scrolling_state.messages.push(Message::info(
        format!("✅ Successfully switched to profile: {}", profile),
        Some(Style::default().fg(ThemeColors::success())),
    ));

    let welcome_msg = welcome_messages(state.configuration_state.latest_version.clone(), state);
    state.messages_scrolling_state.messages.extend(welcome_msg);

    // Invalidate all caches
    invalidate_message_lines_cache(state);
}

/// Handle profile switch failed event
pub fn handle_profile_switch_failed(state: &mut AppState, error: String) {
    state.profile_switcher_state.switching_in_progress = false;
    state.profile_switcher_state.switch_status_message = None;
    state.profile_switcher_state.show_profile_switcher = false;

    state.messages_scrolling_state.messages.push(Message::info(
        format!("❌ Profile switch failed: {}", error),
        Some(Style::default().fg(ThemeColors::danger())),
    ));
    state.messages_scrolling_state.messages.push(Message::info(
        "Staying in current profile. Press ctrl+p to try again.",
        None,
    ));
}

// ========== Rulebook Switcher Handlers ==========

/// Handle show rulebook switcher event
pub fn handle_show_rulebook_switcher(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    // Don't show rulebook switcher if input is blocked or dialog is open
    if state.profile_switcher_state.switching_in_progress
        || state.dialog_approval_state.is_dialog_open
        || state.dialog_approval_state.approval_bar.is_visible()
    {
        return;
    }

    // Clear any pending input to prevent empty message submission
    state.input_state.text_area.set_text("");

    // Request current active rulebooks to pre-select them
    let _ = output_tx.try_send(OutputEvent::RequestCurrentRulebooks);

    state.rulebook_switcher_state.show_rulebook_switcher = true;
    state.rulebook_switcher_state.is_selected = 0;
    state.rulebook_switcher_state.rulebook_search_input.clear();
    filter_rulebooks(state);
}

/// Handle rulebook switcher select event
pub fn handle_rulebook_switcher_select(state: &mut AppState) {
    if state.rulebook_switcher_state.show_rulebook_switcher
        && !state.rulebook_switcher_state.filtered_rulebooks.is_empty()
    {
        let selected_rulebook = &state.rulebook_switcher_state.filtered_rulebooks
            [state.rulebook_switcher_state.is_selected];

        // Toggle selection
        if state
            .rulebook_switcher_state
            .selected_rulebooks
            .contains(&selected_rulebook.uri)
        {
            state
                .rulebook_switcher_state
                .selected_rulebooks
                .remove(&selected_rulebook.uri);
        } else {
            state
                .rulebook_switcher_state
                .selected_rulebooks
                .insert(selected_rulebook.uri.clone());
        }
    }
}

/// Handle rulebook switcher toggle event
pub fn handle_rulebook_switcher_toggle(state: &mut AppState) {
    if state.rulebook_switcher_state.show_rulebook_switcher
        && !state.rulebook_switcher_state.filtered_rulebooks.is_empty()
    {
        let selected_rulebook = &state.rulebook_switcher_state.filtered_rulebooks
            [state.rulebook_switcher_state.is_selected];

        // Toggle selection
        if state
            .rulebook_switcher_state
            .selected_rulebooks
            .contains(&selected_rulebook.uri)
        {
            state
                .rulebook_switcher_state
                .selected_rulebooks
                .remove(&selected_rulebook.uri);
        } else {
            state
                .rulebook_switcher_state
                .selected_rulebooks
                .insert(selected_rulebook.uri.clone());
        }
    }
}

/// Handle rulebook switcher cancel event
pub fn handle_rulebook_switcher_cancel(state: &mut AppState) {
    state.rulebook_switcher_state.show_rulebook_switcher = false;
}

/// Handle rulebook switcher confirm event
pub fn handle_rulebook_switcher_confirm(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    if state.rulebook_switcher_state.show_rulebook_switcher {
        // Send the selected rulebooks to the CLI
        let selected_uris: Vec<String> = state
            .rulebook_switcher_state
            .selected_rulebooks
            .iter()
            .cloned()
            .collect();
        let _ = output_tx.try_send(OutputEvent::RequestRulebookUpdate(selected_uris));

        // Close the switcher
        state.rulebook_switcher_state.show_rulebook_switcher = false;

        // Show confirmation message
        let count = state.rulebook_switcher_state.selected_rulebooks.len();
        state.messages_scrolling_state.messages.push(Message::info(
            format!(
                "Selected {} rulebook(s). They will be applied to your next message.",
                count
            ),
            Some(Style::default().fg(ThemeColors::success())),
        ));
    }
}

/// Handle rulebook switcher select all event
pub fn handle_rulebook_switcher_select_all(state: &mut AppState) {
    if state.rulebook_switcher_state.show_rulebook_switcher {
        // Select all filtered rulebooks
        state.rulebook_switcher_state.selected_rulebooks.clear();
        for rulebook in &state.rulebook_switcher_state.filtered_rulebooks {
            state
                .rulebook_switcher_state
                .selected_rulebooks
                .insert(rulebook.uri.clone());
        }
    }
}

/// Handle rulebook switcher deselect all event
pub fn handle_rulebook_switcher_deselect_all(state: &mut AppState) {
    if state.rulebook_switcher_state.show_rulebook_switcher {
        // Deselect all rulebooks
        state.rulebook_switcher_state.selected_rulebooks.clear();
    }
}

/// Handle rulebook search input changed event
pub fn handle_rulebook_search_input_changed(state: &mut AppState, c: char) {
    if state.rulebook_switcher_state.show_rulebook_switcher {
        state.rulebook_switcher_state.rulebook_search_input.push(c);
        filter_rulebooks(state);
    }
}

/// Handle rulebook search backspace event
pub fn handle_rulebook_search_backspace(state: &mut AppState) {
    if state.rulebook_switcher_state.show_rulebook_switcher
        && !state
            .rulebook_switcher_state
            .rulebook_search_input
            .is_empty()
    {
        state.rulebook_switcher_state.rulebook_search_input.pop();
        filter_rulebooks(state);
    }
}

/// Handle rulebooks loaded event
pub fn handle_rulebooks_loaded(state: &mut AppState, rulebooks: Vec<ListRuleBook>) {
    state.rulebook_switcher_state.available_rulebooks = rulebooks;
    filter_rulebooks(state);
}

/// Handle current rulebooks loaded event
pub fn handle_current_rulebooks_loaded(state: &mut AppState, current_uris: Vec<String>) {
    // Set the currently active rulebooks as selected
    state.rulebook_switcher_state.selected_rulebooks = current_uris.into_iter().collect();
}

// ========== Command Palette Handlers ==========

/// Handle show command palette event - opens unified popup with Commands tab
pub fn handle_show_command_palette(state: &mut AppState) {
    // Don't show if input is blocked or dialog is open
    if state.profile_switcher_state.switching_in_progress
        || state.dialog_approval_state.is_dialog_open
        || state.dialog_approval_state.approval_bar.is_visible()
    {
        return;
    }

    state.shortcuts_panel_state.is_visible = true;
    state.shortcuts_panel_state.mode = crate::app::ShortcutsPopupMode::Commands;
    state.command_palette_state.is_selected = 0;
    state.command_palette_state.scroll = 0;
    state.command_palette_state.search = String::new();
}

/// Handle command palette search input changed event
pub fn handle_command_palette_search_input_changed(state: &mut AppState, c: char) {
    if state.shortcuts_panel_state.is_visible {
        state.command_palette_state.search.push(c);
        state.command_palette_state.is_selected = 0;
        // Also reset session selection to first matching result
        if state.shortcuts_panel_state.mode == crate::app::ShortcutsPopupMode::Sessions {
            let search_lower = state.command_palette_state.search.to_lowercase();
            if let Some(first_match) = state
                .sessions_state
                .sessions
                .iter()
                .enumerate()
                .find(|(_, s)| s.title.to_lowercase().contains(&search_lower))
                .map(|(i, _)| i)
            {
                state.sessions_state.session_selected = first_match;
            }
        }
    }
}

/// Handle command palette search backspace event
pub fn handle_command_palette_search_backspace(state: &mut AppState) {
    if state.shortcuts_panel_state.is_visible && !state.command_palette_state.search.is_empty() {
        state.command_palette_state.search.pop();
        state.command_palette_state.is_selected = 0;
        // Also reset session selection to first matching result
        if state.shortcuts_panel_state.mode == crate::app::ShortcutsPopupMode::Sessions {
            let search_lower = state.command_palette_state.search.to_lowercase();
            if let Some(first_match) = state
                .sessions_state
                .sessions
                .iter()
                .enumerate()
                .find(|(_, s)| {
                    state.command_palette_state.search.is_empty()
                        || s.title.to_lowercase().contains(&search_lower)
                })
                .map(|(i, _)| i)
            {
                state.sessions_state.session_selected = first_match;
            }
        }
    }
}

// ========== Shortcuts Popup Handlers ==========

/// Handle show shortcuts event - opens unified popup with Shortcuts tab
pub fn handle_show_shortcuts(state: &mut AppState) {
    // Don't show shortcuts popup if input is blocked or dialog is open
    if state.profile_switcher_state.switching_in_progress
        || state.dialog_approval_state.is_dialog_open
        || state.dialog_approval_state.approval_bar.is_visible()
        || state.profile_switcher_state.show_profile_switcher
    {
        return;
    }

    state.shortcuts_panel_state.is_visible = true;
    state.shortcuts_panel_state.mode = crate::app::ShortcutsPopupMode::Shortcuts;
    state.shortcuts_panel_state.scroll = 0;
}

/// Handle shortcuts cancel event
pub fn handle_shortcuts_cancel(state: &mut AppState) {
    state.shortcuts_panel_state.is_visible = false;
}

/// Handle toggle more shortcuts event
pub fn handle_toggle_more_shortcuts(state: &mut AppState) {
    state.dialog_approval_state.show_shortcuts = !state.dialog_approval_state.show_shortcuts;
}

// ========== Collapsed Messages Handlers ==========

/// Handle toggle collapsed messages event
pub fn handle_toggle_collapsed_messages(
    state: &mut AppState,
    message_area_height: usize,
    message_area_width: usize,
) {
    // Clear any active text selection when toggling the popup
    // (prevents stale selection from one context bleeding into the other)
    state.message_interaction_state.selection =
        crate::services::text_selection::SelectionState::default();

    // Handle collapsed messages popup
    state.messages_scrolling_state.show_collapsed_messages =
        !state.messages_scrolling_state.show_collapsed_messages;
    if state.messages_scrolling_state.show_collapsed_messages {
        // Calculate scroll position to show the top of the last message
        let collapsed_messages: Vec<Message> = state
            .messages_scrolling_state
            .messages
            .iter()
            .filter(|m| m.is_collapsed == Some(true))
            .cloned()
            .collect();

        if !collapsed_messages.is_empty() {
            // Set selected to the last message
            state.messages_scrolling_state.collapsed_messages_selected =
                collapsed_messages.len() - 1;

            // Get all collapsed message lines once
            let all_lines = get_wrapped_collapsed_message_lines_cached(state, message_area_width);

            // Calculate scroll to show the top of the last message
            // For now, just scroll to the bottom to show the last message
            let total_lines = all_lines.len();
            let max_scroll = total_lines.saturating_sub(message_area_height);
            state.messages_scrolling_state.collapsed_messages_scroll = max_scroll;
        } else {
            state.messages_scrolling_state.collapsed_messages_scroll = 0;
            state.messages_scrolling_state.collapsed_messages_selected = 0;
        }
    }
}

// ========== Side Panel Handlers ==========

/// Handle toggle side panel event
pub fn handle_toggle_side_panel(
    state: &mut AppState,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
) {
    state.side_panel_state.is_shown = !state.side_panel_state.is_shown;
    // Refresh board tasks when showing the side panel
    // The handler will extract agent_id from messages if not already set
    if state.side_panel_state.is_shown {
        let _ = input_tx.try_send(InputEvent::RefreshBoardTasks);
    }
}

/// Handle side panel section navigation
pub fn handle_side_panel_next_section(state: &mut AppState) {
    if state.side_panel_state.is_shown {
        state.side_panel_state.focused_section = state.side_panel_state.focused_section.next();
    }
}

/// Handle side panel section toggle collapse
pub fn handle_side_panel_toggle_section(state: &mut AppState) {
    if state.side_panel_state.is_shown {
        let current = state
            .side_panel_state
            .collapsed_sections
            .get(&state.side_panel_state.focused_section)
            .copied()
            .unwrap_or(false);
        state
            .side_panel_state
            .collapsed_sections
            .insert(state.side_panel_state.focused_section, !current);
    }
}

/// Handle side panel toggle section via mouse click
pub fn handle_side_panel_mouse_click(state: &mut AppState, col: u16, row: u16) {
    if !state.side_panel_state.is_shown {
        return;
    }

    // Check which section was clicked
    let mut clicked_section = None;
    for (section, area) in &state.side_panel_state.areas {
        if col >= area.x && col < area.x + area.width && row >= area.y && row < area.y + area.height
        {
            clicked_section = Some(*section);
            break;
        }
    }

    if let Some(section) = clicked_section {
        state.side_panel_state.focused_section = section;

        // Special handling for Changeset section
        if section == crate::services::changeset::SidePanelSection::Changeset {
            let Some(area) = state.side_panel_state.areas.get(&section) else {
                return;
            };
            let relative_y = row.saturating_sub(area.y);

            // Row 0 is the header
            if relative_y == 0 {
                let current = state
                    .side_panel_state
                    .collapsed_sections
                    .get(&section)
                    .copied()
                    .unwrap_or(false);
                state
                    .side_panel_state
                    .collapsed_sections
                    .insert(section, !current);
            } else {
                // Content click - if not collapsed, open file changes popup
                let collapsed = state
                    .side_panel_state
                    .collapsed_sections
                    .get(&section)
                    .copied()
                    .unwrap_or(false);

                if !collapsed {
                    // Calculate file index (row 1 is file 0)
                    // Note: We need to account for the fact that previous sections might push this down
                    // but relative_y handles that.
                    // We DO assume 1 line per file in the changeset view.
                    // Checking side_panel.rs in previous steps, it renders 1 line per file (conditionally expanded edits).
                    // If a file is expanded in the side panel, this index calculation might be off.
                    // For now, let's assume it maps to the visible list.
                    // Ideally rendering should store click areas per item.
                    // Falling back to opening popup with generic file list if any content click.
                    handle_show_file_changes_popup(state);
                }
            }
        } else {
            let current = state
                .side_panel_state
                .collapsed_sections
                .get(&section)
                .copied()
                .unwrap_or(false);
            state
                .side_panel_state
                .collapsed_sections
                .insert(section, !current);
        }
    }
}

// ========= Auto-Approve Popup Handlers ==========

fn next_policy(policy: &AutoApprovePolicy) -> AutoApprovePolicy {
    match policy {
        AutoApprovePolicy::Prompt => AutoApprovePolicy::Auto,
        AutoApprovePolicy::Auto => AutoApprovePolicy::Never,
        AutoApprovePolicy::Never => AutoApprovePolicy::Prompt,
    }
}

fn prev_policy(policy: &AutoApprovePolicy) -> AutoApprovePolicy {
    match policy {
        AutoApprovePolicy::Prompt => AutoApprovePolicy::Never,
        AutoApprovePolicy::Auto => AutoApprovePolicy::Prompt,
        AutoApprovePolicy::Never => AutoApprovePolicy::Auto,
    }
}

fn sync_auto_approve_popup_scroll(state: &mut AppState) {
    const VISIBLE_ROWS: usize = 12;
    let total = state.tool_approval_popup_state.rows.len();
    if total == 0 {
        state.tool_approval_popup_state.row_selected = 0;
        state.tool_approval_popup_state.scroll = 0;
        return;
    }

    if state.tool_approval_popup_state.row_selected >= total {
        state.tool_approval_popup_state.row_selected = total - 1;
    }

    if state.tool_approval_popup_state.row_selected < state.tool_approval_popup_state.scroll {
        state.tool_approval_popup_state.scroll = state.tool_approval_popup_state.row_selected;
    } else if state.tool_approval_popup_state.row_selected
        >= state.tool_approval_popup_state.scroll + VISIBLE_ROWS
    {
        state.tool_approval_popup_state.scroll = state
            .tool_approval_popup_state
            .row_selected
            .saturating_sub(VISIBLE_ROWS - 1);
    }
}

pub fn handle_show_auto_approve_popup(state: &mut AppState) {
    if state.profile_switcher_state.switching_in_progress
        || state.dialog_approval_state.is_dialog_open
        || state.dialog_approval_state.approval_bar.is_visible()
    {
        return;
    }

    let mut rows: Vec<AutoApprovePopupRow> = state
        .configuration_state
        .auto_approve_manager
        .get_config()
        .tools
        .iter()
        .map(|(tool_name, policy)| AutoApprovePopupRow {
            tool_name: tool_name.clone(),
            policy: policy.clone(),
            original_policy: policy.clone(),
        })
        .collect();

    rows.sort_by(|a, b| a.tool_name.cmp(&b.tool_name));

    state.tool_approval_popup_state.is_visible = true;
    state.tool_approval_popup_state.rows = rows;
    state.tool_approval_popup_state.row_selected = 0;
    state.tool_approval_popup_state.scroll = 0;
    state.tool_approval_popup_state.filter_text.clear();
    state.tool_approval_popup_state.filtered_rows.clear();
}

pub fn handle_auto_approve_popup_cancel(state: &mut AppState) {
    state.tool_approval_popup_state.reset();
}

pub fn handle_auto_approve_popup_up(state: &mut AppState) {
    if state.tool_approval_popup_state.visible_count() == 0 {
        return;
    }

    if state.tool_approval_popup_state.row_selected > 0 {
        state.tool_approval_popup_state.row_selected -= 1;
    }
    sync_auto_approve_popup_scroll(state);
}

pub fn handle_auto_approve_popup_down(state: &mut AppState) {
    if state.tool_approval_popup_state.visible_count() == 0 {
        return;
    }

    if state.tool_approval_popup_state.row_selected + 1
        < state.tool_approval_popup_state.visible_count()
    {
        state.tool_approval_popup_state.row_selected += 1;
    }
    sync_auto_approve_popup_scroll(state);
}

pub fn handle_auto_approve_popup_left(state: &mut AppState) {
    if let Some(row_idx) = state
        .tool_approval_popup_state
        .get_row_index(state.tool_approval_popup_state.row_selected)
        && let Some(row) = state.tool_approval_popup_state.rows.get_mut(row_idx)
    {
        row.policy = prev_policy(&row.policy);
    }
}

pub fn handle_auto_approve_popup_right(state: &mut AppState) {
    if let Some(row_idx) = state
        .tool_approval_popup_state
        .get_row_index(state.tool_approval_popup_state.row_selected)
        && let Some(row) = state.tool_approval_popup_state.rows.get_mut(row_idx)
    {
        row.policy = next_policy(&row.policy);
    }
}

pub fn handle_auto_approve_popup_apply(state: &mut AppState) {
    let changes: Vec<(String, AutoApprovePolicy)> = state
        .tool_approval_popup_state
        .rows
        .iter()
        .filter(|row| row.policy != row.original_policy)
        .map(|row| (row.tool_name.clone(), row.policy.clone()))
        .collect();

    if changes.is_empty() {
        state.tool_approval_popup_state.reset();
        return;
    }

    for (tool_name, policy) in changes {
        if let Err(e) = state
            .configuration_state
            .auto_approve_manager
            .update_tool_policy(&tool_name, policy)
        {
            push_error_message(
                state,
                &format!(
                    "Failed to update auto-approve policy for {}: {}",
                    tool_name, e
                ),
                None,
            );
            return;
        }
    }

    state.tool_approval_popup_state.reset();
    push_styled_message(
        state,
        "Auto-approve settings updated",
        ThemeColors::green(),
        "",
        ThemeColors::green(),
    );
}

// ========== File Changes Popup Handlers ==========

pub fn handle_show_file_changes_popup(state: &mut AppState) {
    if state.profile_switcher_state.switching_in_progress
        || state.dialog_approval_state.is_dialog_open
        || state.dialog_approval_state.approval_bar.is_visible()
    {
        return;
    }

    // Don't open if there are no changes
    if state.side_panel_state.changeset.file_count() == 0 {
        return;
    }

    state.file_changes_popup_state.is_visible = true;
    state.file_changes_popup_state.is_selected = 0;
    state.file_changes_popup_state.scroll = 0;
    state.file_changes_popup_state.search = String::new();
}

pub fn handle_file_changes_popup_cancel(state: &mut AppState) {
    state.file_changes_popup_state.is_visible = false;
}

pub fn handle_file_changes_popup_search_input(state: &mut AppState, c: char) {
    state.file_changes_popup_state.search.push(c);
    state.file_changes_popup_state.is_selected = 0;
    state.file_changes_popup_state.scroll = 0;
}

pub fn handle_file_changes_popup_backspace(state: &mut AppState) {
    if !state.file_changes_popup_state.search.is_empty() {
        state.file_changes_popup_state.search.pop();
        state.file_changes_popup_state.is_selected = 0;
        state.file_changes_popup_state.scroll = 0;
    }
}

pub fn handle_file_changes_popup_navigate(state: &mut AppState, delta: i32) {
    // Get filtered count
    let query = state.file_changes_popup_state.search.to_lowercase();
    let count = state
        .side_panel_state
        .changeset
        .files_in_order()
        .iter()
        .filter(|f| query.is_empty() || f.display_name().to_lowercase().contains(&query))
        .count();

    if count == 0 {
        return;
    }

    let new_selected = state.file_changes_popup_state.is_selected as i32 + delta;
    state.file_changes_popup_state.is_selected = new_selected.clamp(0, count as i32 - 1) as usize;

    // Adjust scroll
    // Simple logic: keep selected in view
    // Assuming visible height is around 10-20?
    // In render function we calculated height dynamically.
    // Ideally we track scroll separately.
    // For now, simple scroll following selection.
    if state.file_changes_popup_state.is_selected < state.file_changes_popup_state.scroll {
        state.file_changes_popup_state.scroll = state.file_changes_popup_state.is_selected;
    }
    // Note: We don't know the window height here easily without passing it.
    // We'll let the render function clamp scroll if needed, or implement better scroll logic later.
    // For now, ensuring scroll is at least close to selection.
    if state.file_changes_popup_state.is_selected > state.file_changes_popup_state.scroll + 10 {
        state.file_changes_popup_state.scroll = state.file_changes_popup_state.is_selected - 10;
    }
}

pub fn handle_file_changes_popup_revert(state: &mut AppState) {
    // Revert selected file
    let query = state.file_changes_popup_state.search.to_lowercase();
    let binding = state.side_panel_state.changeset.files_in_order();
    let filtered_files: Vec<_> = binding
        .iter()
        .filter(|f| query.is_empty() || f.display_name().to_lowercase().contains(&query))
        .collect();

    // Import FileState
    use crate::services::changeset::FileState;

    if let Some(file) = filtered_files.get(state.file_changes_popup_state.is_selected) {
        if file.state == FileState::Deleted && file.backup_path.is_none() {
            return;
        }
        let path = file.path.clone();
        let old_state = file.state;

        // Call the revert function
        match crate::services::changeset::Changeset::revert_file(
            file,
            &state.side_panel_state.session_id,
        ) {
            Ok(message) => {
                // Update state based on what happened
                if let Some(tracked) = state.side_panel_state.changeset.files.get_mut(&path) {
                    if !std::path::Path::new(&path).exists() {
                        // If file is gone, it's definitively Deleted
                        tracked.state = FileState::Deleted;
                    } else {
                        match old_state {
                            FileState::Deleted => tracked.state = FileState::Created, // Restored a created-then-deleted file
                            FileState::Removed => tracked.state = FileState::Modified, // Restored a removed file
                            FileState::Created => tracked.state = FileState::Deleted, // Should be caught by !exists check, but fallback
                            _ => tracked.state = FileState::Reverted, // Reverted edits
                        }
                    }
                }

                // Push success message
                push_styled_message(
                    state,
                    &message,
                    ThemeColors::green(),
                    " ✓ ",
                    ThemeColors::green(),
                );

                // Close popup if no more non-reverted files
                if state.side_panel_state.changeset.file_count() == 0 {
                    state.file_changes_popup_state.is_visible = false;
                } else {
                    // Adjust selection if needed
                    if state.file_changes_popup_state.is_selected
                        >= state.side_panel_state.changeset.file_count()
                    {
                        state.file_changes_popup_state.is_selected = state
                            .side_panel_state
                            .changeset
                            .file_count()
                            .saturating_sub(1);
                    }
                }
            }
            Err(error) => {
                push_error_message(state, &format!("Revert failed: {}", error), None);
            }
        }
    }
}

pub fn handle_file_changes_popup_revert_all(state: &mut AppState) {
    use crate::services::changeset::FileState;

    // Collect all non-reverted and non-deleted files to revert
    let files_to_revert: Vec<_> = state
        .side_panel_state
        .changeset
        .files_in_order()
        .into_iter()
        .filter(|f| {
            f.state != FileState::Reverted
                && (f.state != FileState::Deleted || f.backup_path.is_some())
        })
        .map(|f| (f.path.clone(), f.clone()))
        .collect();

    let mut reverted_count = 0;
    let mut failed_count = 0;

    for (path, file) in files_to_revert {
        let old_state = file.state;
        match crate::services::changeset::Changeset::revert_file(
            &file,
            &state.side_panel_state.session_id,
        ) {
            Ok(_) => {
                // Update state based on what happened
                if let Some(tracked) = state.side_panel_state.changeset.files.get_mut(&path) {
                    if !std::path::Path::new(&path).exists() {
                        tracked.state = FileState::Deleted;
                    } else {
                        match old_state {
                            FileState::Deleted => tracked.state = FileState::Created,
                            FileState::Removed => tracked.state = FileState::Modified,
                            FileState::Created => tracked.state = FileState::Deleted,
                            _ => tracked.state = FileState::Reverted,
                        }
                    }
                }
                reverted_count += 1;
            }
            Err(_) => {
                failed_count += 1;
            }
        }
    }

    // Show summary message
    if reverted_count > 0 {
        let message = if failed_count > 0 {
            format!(
                "Reverted {} files ({} failed)",
                reverted_count, failed_count
            )
        } else {
            format!("Reverted {} files", reverted_count)
        };
        push_styled_message(
            state,
            &message,
            ThemeColors::green(),
            " ✓ ",
            ThemeColors::green(),
        );
    } else if failed_count > 0 {
        push_error_message(
            state,
            &format!("Failed to revert {} files", failed_count),
            None,
        );
    }

    // Close popup if no more non-reverted files
    if state.side_panel_state.changeset.file_count() == 0 {
        state.file_changes_popup_state.is_visible = false;
    }
}

/// Handle opening the selected file in an external editor
pub fn handle_file_changes_popup_open_editor(state: &mut AppState) {
    let query = state.file_changes_popup_state.search.to_lowercase();
    let binding = state.side_panel_state.changeset.files_in_order();
    let filtered_files: Vec<_> = binding
        .iter()
        .filter(|f| query.is_empty() || f.display_name().to_lowercase().contains(&query))
        .collect();

    if let Some(file) = filtered_files.get(state.file_changes_popup_state.is_selected) {
        if file.state == crate::services::changeset::FileState::Deleted {
            return;
        }
        let path = file.path.clone();
        // Set the pending editor open request - will be handled by event loop
        state.side_panel_state.pending_editor_open = Some(path);
    }
}

/// Handle mouse clicks on file changes popup
pub fn handle_file_changes_popup_mouse_click(state: &mut AppState, col: u16, row: u16) {
    // Calculate popup area (same as in render_file_changes_popup)
    let term_size = crossterm::terminal::size().unwrap_or((80, 24));
    let term_width = term_size.0;
    let term_height = term_size.1;

    // Calculate centered popup area (50% width, 40% height)
    let popup_width = (term_width * 50) / 100;
    let popup_height = (term_height * 40) / 100;
    let popup_x = (term_width - popup_width) / 2;
    let popup_y = (term_height - popup_height) / 2;

    // Check if click is within popup bounds
    if col < popup_x
        || col >= popup_x + popup_width
        || row < popup_y
        || row >= popup_y + popup_height
    {
        return;
    }

    // Calculate relative position within popup
    let relative_row = row.saturating_sub(popup_y + 1); // +1 for border

    // File list starts after: title (1) + search (1) + separator (1) = 3 lines
    // And ends before: scroll indicator (1) + footer (1) = 2 lines from bottom
    // File list starts after: title (1) + search (3) + separator (0?) = 4 lines
    let file_list_start = 4;
    let file_list_end = popup_height.saturating_sub(3); // -1 border, -2 footer area

    if relative_row >= file_list_start && relative_row < file_list_end {
        // Calculate which file was clicked
        let file_index =
            (relative_row - file_list_start) as usize + state.file_changes_popup_state.scroll;

        // Get filtered files
        let query = state.file_changes_popup_state.search.to_lowercase();
        let binding = state.side_panel_state.changeset.files_in_order();
        let filtered_files: Vec<_> = binding
            .iter()
            .filter(|f| query.is_empty() || f.display_name().to_lowercase().contains(&query))
            .collect();

        if file_index < filtered_files.len() {
            let file = filtered_files[file_index];
            if file.state != crate::services::changeset::FileState::Deleted {
                // Set the pending editor open for this file
                state.side_panel_state.pending_editor_open = Some(file.path.clone());
            }
        }
    }
}

// ========== Model Switcher Handlers ==========

/// Handle show model switcher event
pub fn handle_show_model_switcher(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    // Don't show model switcher if input is blocked or dialog is open
    if state.profile_switcher_state.switching_in_progress
        || state.dialog_approval_state.is_dialog_open
        || state.dialog_approval_state.approval_bar.is_visible()
    {
        return;
    }

    // Clear any pending input
    state.input_state.text_area.set_text("");

    // Request available models from the backend
    let _ = output_tx.try_send(OutputEvent::RequestAvailableModels);

    state.model_switcher_state.is_visible = true;
    state.model_switcher_state.is_selected = 0;
    // Reset filter mode and search when opening
    state.model_switcher_state.mode = crate::app::ModelSwitcherMode::default();
    state.model_switcher_state.search.clear();
}

/// Add custom models from recent_models to available_models.
///
/// Custom models are those in recent_models but not in available_models.
/// Recent model IDs use normalized `"provider/short_name"` format.
/// Matching against available_models compares the normalized form so that
/// `"stakpak/glm-5"` matches a catalog model with
/// `id: "fireworks-ai/accounts/fireworks/models/glm-5", provider: "stakpak"`.
pub fn ensure_custom_models_in_available(state: &mut AppState) {
    // Get the default provider from the configured model
    let default_provider = state.configuration_state.model.provider.clone();

    // Collect models to add first, then extend (avoids cloning recent_models)
    let to_add: Vec<Model> = state
        .model_switcher_state
        .recent_models
        .iter()
        .filter(|recent_id| {
            // Check if any available model matches this recent ID when normalized
            !state
                .model_switcher_state
                .available_models
                .iter()
                .any(|m| format_recent_model_id(&m.provider, &m.id) == **recent_id)
        })
        .map(|recent_id| {
            // Parse "provider/short_name" format
            let (provider, short_name) = recent_id
                .split_once('/')
                .map(|(p, n)| (p.to_string(), n.to_string()))
                .unwrap_or_else(|| (default_provider.clone(), recent_id.clone()));
            Model::custom(short_name, provider)
        })
        .collect();

    state.model_switcher_state.available_models.extend(to_add);
}

/// Handle available models loaded event
pub fn handle_available_models_loaded(
    state: &mut AppState,
    models: Vec<Model>,
    output_tx: &Sender<OutputEvent>,
) {
    // Sort models by provider to match render order in model_switcher.rs
    // "stakpak" provider always first, then alphabetically
    let mut sorted_models = models;
    sorted_models.sort_by(|a, b| {
        match (
            a.provider.as_str() == "stakpak",
            b.provider.as_str() == "stakpak",
        ) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.provider.cmp(&b.provider),
        }
    });
    state.model_switcher_state.available_models = sorted_models;

    // Add custom models from recent_models that aren't in available_models
    ensure_custom_models_in_available(state);

    // Add the current/default model to recent_models if not already there.
    // Always use normalized "provider/short_name" format for storage.
    let recent_id_to_add = if let Some(current) = &state.model_switcher_state.current_model {
        Some(format_recent_model_id(&current.provider, &current.id))
    } else {
        // Use the configured default model (state.configuration_state.model)
        // Try to find matching model in available_models first (for correct provider)
        let default_model_id = &state.configuration_state.model.id;
        if let Some(matched) = state
            .model_switcher_state
            .available_models
            .iter()
            .find(|m| {
                m.id == *default_model_id
                    || m.id
                        .split('/')
                        .next_back()
                        .is_some_and(|last| last == default_model_id.as_str())
            })
        {
            Some(format_recent_model_id(&matched.provider, &matched.id))
        } else if !default_model_id.is_empty() {
            Some(format_recent_model_id(
                &state.configuration_state.model.provider,
                default_model_id,
            ))
        } else {
            None
        }
    };

    if let Some(recent_id) = recent_id_to_add
        && !state
            .model_switcher_state
            .recent_models
            .contains(&recent_id)
    {
        // Add to front of recent list
        state
            .model_switcher_state
            .recent_models
            .insert(0, recent_id);
        // Keep max 5
        state.model_switcher_state.recent_models.truncate(5);

        // Persist to config so it survives model switches
        let _ = output_tx.try_send(OutputEvent::SaveRecentModels(
            state.model_switcher_state.recent_models.clone(),
        ));
    }

    // Pre-select current model if available and it's in the filtered list
    let filtered = crate::services::model_switcher::filter_models(
        &state.model_switcher_state.available_models,
        state.model_switcher_state.mode,
        &state.model_switcher_state.search,
    );

    if let Some(current) = &state.model_switcher_state.current_model {
        // Check if current model is in the filtered list
        if let Some(idx) = state
            .model_switcher_state
            .available_models
            .iter()
            .position(|m| m.id == current.id)
        {
            if filtered.contains(&idx) {
                state.model_switcher_state.is_selected = idx;
            } else {
                // Current model not in filter, select first filtered item
                state.model_switcher_state.is_selected = filtered.first().copied().unwrap_or(0);
            }
        } else {
            state.model_switcher_state.is_selected = filtered.first().copied().unwrap_or(0);
        }
    } else {
        state.model_switcher_state.is_selected = filtered.first().copied().unwrap_or(0);
    }
}

/// Handle model switcher select event
pub fn handle_model_switcher_select(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    if state.model_switcher_state.is_visible
        && !state.model_switcher_state.available_models.is_empty()
        && state.model_switcher_state.is_selected
            < state.model_switcher_state.available_models.len()
    {
        // Verify the selected index is in the current filtered set
        let filtered = crate::services::model_switcher::filter_models(
            &state.model_switcher_state.available_models,
            state.model_switcher_state.mode,
            &state.model_switcher_state.search,
        );
        if !filtered.contains(&state.model_switcher_state.is_selected) {
            // Selected model is not in the filtered list, ignore selection
            return;
        }

        let selected_model = state.model_switcher_state.available_models
            [state.model_switcher_state.is_selected]
            .clone();

        // Don't switch if already on this model
        if state
            .model_switcher_state
            .current_model
            .as_ref()
            .is_some_and(|m| m.id == selected_model.id)
        {
            state.model_switcher_state.is_visible = false;
            state.model_switcher_state.search.clear();
            return;
        }

        // Update current model
        state.model_switcher_state.current_model = Some(selected_model.clone());

        // Close the switcher and clear search
        state.model_switcher_state.is_visible = false;
        state.model_switcher_state.search.clear();

        // Send request to switch model
        let _ = output_tx.try_send(OutputEvent::SwitchToModel(selected_model.clone()));
    }
}

/// Handle model switcher cancel event
pub fn handle_model_switcher_cancel(state: &mut AppState) {
    state.model_switcher_state.is_visible = false;
    // Clear search when closing
    state.model_switcher_state.search.clear();
}
// ========== Message Action Popup Handlers ==========

/// Close the message action popup
pub fn handle_message_action_popup_close(state: &mut AppState) {
    state.message_interaction_state.show_message_action_popup = false;
    state
        .message_interaction_state
        .message_action_popup_selected = 0;
    state
        .message_interaction_state
        .message_action_popup_position = None;
    state
        .message_interaction_state
        .message_action_target_message_id = None;
    state.message_interaction_state.message_action_target_text = None;
    // Clear any stuck text selection (popup may have intercepted drag end)
    state.message_interaction_state.selection = SelectionState::default();
    state.input_state.text_area.clear_selection();
}

/// Navigate within the message action popup
pub fn handle_message_action_popup_navigate(state: &mut AppState, direction: i32) {
    let num_actions = crate::services::message_action_popup::MessageAction::all().len();
    if num_actions == 0 {
        return;
    }

    if direction < 0 {
        if state
            .message_interaction_state
            .message_action_popup_selected
            > 0
        {
            state
                .message_interaction_state
                .message_action_popup_selected -= 1;
        } else {
            state
                .message_interaction_state
                .message_action_popup_selected = num_actions - 1;
        }
    } else {
        state
            .message_interaction_state
            .message_action_popup_selected = (state
            .message_interaction_state
            .message_action_popup_selected
            + 1)
            % num_actions;
    }
}

/// Execute the selected action in the message action popup
pub fn handle_message_action_popup_execute(state: &mut AppState) {
    use crate::services::message_action_popup::{MessageAction, get_selected_action};
    use crate::services::text_selection::copy_to_clipboard;
    use crate::services::toast::Toast;

    let Some(action) = get_selected_action(state) else {
        handle_message_action_popup_close(state);
        return;
    };

    match action {
        MessageAction::CopyMessage => {
            // Copy the message text to clipboard
            if let Some(text) = &state.message_interaction_state.message_action_target_text {
                match copy_to_clipboard(text) {
                    Ok(()) => {
                        state.toast = Some(Toast::success("Copied!"));
                    }
                    Err(e) => {
                        log::warn!("Failed to copy to clipboard: {}", e);
                        state.toast = Some(Toast::error("Copy failed"));
                    }
                }
            }
        }
        MessageAction::RevertToMessage => {
            if let Some(target_id) = state
                .message_interaction_state
                .message_action_target_message_id
            {
                // Find the user message index from the line_to_message_map
                let target_user_idx = state
                    .messages_scrolling_state
                    .line_to_message_map
                    .iter()
                    .find(|(_, _, id, is_user, _, user_idx)| {
                        *id == target_id && *is_user && *user_idx > 0
                    })
                    .map(|(_, _, _, _, _, user_idx)| *user_idx);

                if let Some(target_idx) = target_user_idx {
                    // Revert file changes for edits at or after target_idx
                    // (the clicked message and everything after it)
                    let revert_result = state
                        .side_panel_state
                        .changeset
                        .revert_from_user_message(target_idx, &state.side_panel_state.session_id);

                    // Find the TUI message index and truncate
                    if let Some(msg_idx) = state
                        .messages_scrolling_state
                        .messages
                        .iter()
                        .position(|m| m.id == target_id)
                    {
                        // Truncate messages - remove target message and everything after
                        state.messages_scrolling_state.messages.truncate(msg_idx);
                    }

                    // Store pending revert for backend sync
                    state.message_revert_state.pending_revert_index = Some(target_idx);

                    // Update user_message_count to match the new state
                    // We removed the clicked message (target_idx) and everything after,
                    // so we now have (target_idx - 1) user messages remaining
                    state.message_revert_state.user_message_count = target_idx.saturating_sub(1);

                    // Clear todos
                    state.side_panel_state.todos.clear();

                    // Invalidate message cache
                    invalidate_message_lines_cache(state);

                    // Show appropriate toast
                    match revert_result {
                        Ok((files_reverted, files_deleted)) => {
                            let message = if files_reverted > 0 || files_deleted > 0 {
                                format!(
                                    "Reverted {} file(s), deleted {} created file(s)",
                                    files_reverted, files_deleted
                                )
                            } else {
                                "Reverted to message".to_string()
                            };
                            state.toast = Some(Toast::success(&message));
                        }
                        Err(e) => {
                            log::warn!("Revert failed: {}", e);
                            state.toast =
                                Some(Toast::success("Reverted messages (file revert failed)"));
                        }
                    }
                } else {
                    state.toast = Some(Toast::error("Could not find message index"));
                }
            }
        }
    }

    handle_message_action_popup_close(state);
}
