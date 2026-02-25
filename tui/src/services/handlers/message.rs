//! Message Event Handlers
//!
//! Handles all message-related events including streaming messages, adding user messages, and usage tracking.

use crate::app::AppState;
use crate::services::helper_block::push_usage_message;
use crate::services::message::{
    Message, MessageContent, invalidate_message_cache, invalidate_message_lines_cache,
};
use stakpak_shared::models::llm::LLMTokenUsage;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

use crate::app::OutputEvent;

/// Handle streaming assistant message
pub fn handle_stream_message(
    state: &mut AppState,
    id: Uuid,
    s: String,
    message_area_height: usize,
) {
    // Ignore late streaming events after cancellation was requested
    if state.cancel_requested {
        return;
    }

    if let Some(message) = state.messages.iter_mut().find(|m| m.id == id) {
        state.is_streaming = true;
        if !state.loading {
            state.loading = true;
        }
        if let MessageContent::AssistantMD(text, _) = &mut message.content {
            text.push_str(&s);

            // Extract todos from the accumulated message content
            let extracted_todos = crate::services::todo_extractor::extract_todos(text);
            if !extracted_todos.is_empty() {
                state.todos = extracted_todos;
            }
        }

        // If user is scrolled up, don't invalidate cache - just mark that content changed
        // This prevents jittery scrolling while streaming when user is reading old messages
        if state.stay_at_bottom {
            // Use per-message cache invalidation for better performance during streaming
            // This only invalidates the specific message that changed, not all messages
            invalidate_message_cache(state, id);

            // Adjust scroll to follow the streaming content
            let input_height = 3;
            let total_lines = state.messages.len() * 2;
            let max_visible_lines =
                std::cmp::max(1, message_area_height.saturating_sub(input_height));
            let max_scroll = total_lines.saturating_sub(max_visible_lines);
            state.scroll = max_scroll;
        } else {
            // Mark that content changed while scrolled up - cache will be rebuilt when user scrolls back
            state.content_changed_while_scrolled_up = true;
        }
        state.is_streaming = false;
    } else {
        let input_height = 3;
        let total_lines = state.messages.len() * 2;
        let max_visible_lines = std::cmp::max(1, message_area_height.saturating_sub(input_height));
        let max_scroll = total_lines.saturating_sub(max_visible_lines);
        let was_at_bottom = state.scroll == max_scroll;
        state
            .messages
            .push(Message::assistant(Some(id), s.clone(), None));

        // Invalidate cache since messages changed
        invalidate_message_lines_cache(state);

        // Note: Don't clear input here - it was already cleared when user submitted their message.
        // Clearing here would wipe out any new input the user started typing while waiting for the response.

        if !was_at_bottom {
            state.content_changed_while_scrolled_up = true;
        }

        // Auto-show side panel
        state.auto_show_side_panel();

        let total_lines = state.messages.len() * 2;
        let max_scroll = total_lines.saturating_sub(max_visible_lines);

        if was_at_bottom {
            state.scroll = max_scroll;
            state.scroll_to_bottom = true;
            state.stay_at_bottom = true;
        }
        state.is_streaming = false;
    }
}

/// Handle adding user message
pub fn handle_add_user_message(state: &mut AppState, s: String) {
    // Increment user message count (used for tracking file edits for selective revert)
    state.user_message_count += 1;

    // Add extra spacing before user message if not the first message
    if !state.messages.is_empty() {
        state.messages.push(Message::plain_text(""));
        state.messages.push(Message::plain_text(""));
    }
    state.messages.push(Message::user(s, None));
    // Add extra spacing after user message
    state.messages.push(Message::plain_text(""));
    state.messages.push(Message::plain_text(""));

    // Invalidate cache since messages changed
    invalidate_message_lines_cache(state);

    // Scroll to bottom to show the new message
    state.scroll_to_bottom = true;
    state.stay_at_bottom = true;
}

/// Handle has user message event
pub fn handle_has_user_message(state: &mut AppState) {
    state.has_user_messages = true;
    state.toggle_approved_message = true;
    state.message_approved_tools.clear();
    state.message_rejected_tools.clear();
    state.message_tool_calls = None;
    state.tool_call_execution_order.clear();
    state.is_dialog_open = false;
    // Clear any pending cancellation from a previous interaction
    state.cancel_requested = false;
}

/// Handle stream usage event
pub fn handle_stream_usage(state: &mut AppState, usage: LLMTokenUsage) {
    state.current_message_usage = usage;
}

/// Handle request total usage event
pub fn handle_request_total_usage(output_tx: &Sender<OutputEvent>) {
    // Request total usage from CLI
    let _ = output_tx.try_send(OutputEvent::RequestTotalUsage);
}

/// Handle total usage event
pub fn handle_total_usage(state: &mut AppState, usage: LLMTokenUsage) {
    // Update total session usage from CLI
    state.total_session_usage = usage;
    // If cost message was just displayed, update it
    let should_update = state
        .messages
        .last()
        .and_then(|msg| {
            if let MessageContent::StyledBlock(lines) = &msg.content {
                lines
                    .first()
                    .and_then(|l| l.spans.first())
                    .map(|s| s.content.contains("Token Usage & Costs"))
            } else {
                None
            }
        })
        .unwrap_or(false);

    if should_update {
        state.messages.pop(); // Remove old message
        push_usage_message(state);
    }
}
