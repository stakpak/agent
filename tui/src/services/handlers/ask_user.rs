//! Ask User Event Handlers
//!
//! Handles all events related to the ask_user popup including navigation,
//! option selection, custom input, and submission.

use crate::app::{AppState, OutputEvent};
use stakpak_shared::models::integrations::openai::{
    AskUserAnswer, AskUserQuestion, AskUserResult, ToolCall, ToolCallResult, ToolCallResultStatus,
};
use tokio::sync::mpsc::Sender;

/// Get the total number of options for a question (including custom if allowed)
fn get_total_options(question: &AskUserQuestion) -> usize {
    if question.allow_custom {
        question.options.len() + 1
    } else {
        question.options.len()
    }
}

/// Show the ask user inline block with the given questions
pub fn handle_show_ask_user_popup(
    state: &mut AppState,
    tool_call: ToolCall,
    questions: Vec<AskUserQuestion>,
) {
    if questions.is_empty() {
        return;
    }

    state.show_ask_user_popup = true;
    state.ask_user_focused = true;
    state.ask_user_questions = questions.clone();
    state.ask_user_answers.clear();
    state.ask_user_current_tab = 0;
    state.ask_user_selected_option = 0;
    state.ask_user_custom_input.clear();
    state.ask_user_tool_call = Some(tool_call);

    // Create inline message block
    let msg = crate::services::message::Message::render_ask_user_block(
        questions,
        state.ask_user_answers.clone(),
        state.ask_user_current_tab,
        state.ask_user_selected_option,
        state.ask_user_custom_input.clone(),
        state.ask_user_focused,
        None,
    );
    state.ask_user_message_id = Some(msg.id);
    state.messages.push(msg);

    // Invalidate cache to update display
    crate::services::message::invalidate_message_lines_cache(state);

    // Auto-scroll to bottom to show the new block
    state.stay_at_bottom = true;
}

/// Public wrapper for refresh (used by handlers/mod.rs for focus toggle)
pub fn refresh_ask_user_block_pub(state: &mut AppState) {
    refresh_ask_user_block(state);
}

/// Refresh the inline ask_user message block to reflect current state
fn refresh_ask_user_block(state: &mut AppState) {
    if let Some(msg_id) = state.ask_user_message_id {
        // Update the existing message in-place
        for msg in &mut state.messages {
            if msg.id == msg_id {
                msg.content = crate::services::message::MessageContent::RenderAskUserBlock {
                    questions: state.ask_user_questions.clone(),
                    answers: state.ask_user_answers.clone(),
                    current_tab: state.ask_user_current_tab,
                    selected_option: state.ask_user_selected_option,
                    custom_input: state.ask_user_custom_input.clone(),
                    focused: state.ask_user_focused,
                };
                break;
            }
        }
        crate::services::message::invalidate_message_lines_cache(state);
    }
}

/// Navigate to the next tab (question or Submit)
pub fn handle_ask_user_next_tab(state: &mut AppState) {
    if !state.show_ask_user_popup {
        return;
    }

    let max_tab = state.ask_user_questions.len(); // questions.len() is the Submit tab
    if state.ask_user_current_tab < max_tab {
        state.ask_user_current_tab += 1;
        restore_selection_for_current_tab(state);
    }
    refresh_ask_user_block(state);
}

/// Navigate to the previous tab
pub fn handle_ask_user_prev_tab(state: &mut AppState) {
    if !state.show_ask_user_popup {
        return;
    }

    if state.ask_user_current_tab > 0 {
        state.ask_user_current_tab -= 1;
        restore_selection_for_current_tab(state);
    }
    refresh_ask_user_block(state);
}

/// Restore the cursor position when navigating back to a question tab.
///
/// If the question was previously answered, place the cursor on the answered
/// option so the `›` indicator doesn't hide the selection. Otherwise reset to 0.
fn restore_selection_for_current_tab(state: &mut AppState) {
    state.ask_user_custom_input.clear();

    // Submit tab — nothing to restore
    if state.ask_user_current_tab >= state.ask_user_questions.len() {
        state.ask_user_selected_option = 0;
        return;
    }

    let q = &state.ask_user_questions[state.ask_user_current_tab];

    if let Some(answer) = state.ask_user_answers.get(&q.label) {
        if answer.is_custom {
            // Custom answer — point to the custom input slot
            state.ask_user_selected_option = q.options.len();
            state.ask_user_custom_input.clone_from(&answer.answer);
        } else if let Some(idx) = q.options.iter().position(|o| o.value == answer.answer) {
            state.ask_user_selected_option = idx;
        } else {
            state.ask_user_selected_option = 0;
        }
    } else {
        state.ask_user_selected_option = 0;
    }
}

/// Navigate to the next option within the current question
pub fn handle_ask_user_next_option(state: &mut AppState) {
    if !state.show_ask_user_popup {
        return;
    }

    // Can't navigate options on Submit tab
    if state.ask_user_current_tab >= state.ask_user_questions.len() {
        return;
    }

    let current_q = &state.ask_user_questions[state.ask_user_current_tab];
    let total_options = get_total_options(current_q);

    if state.ask_user_selected_option < total_options.saturating_sub(1) {
        state.ask_user_selected_option += 1;
    }
    refresh_ask_user_block(state);
}

/// Navigate to the previous option within the current question
pub fn handle_ask_user_prev_option(state: &mut AppState) {
    if !state.show_ask_user_popup {
        return;
    }

    // Can't navigate options on Submit tab
    if state.ask_user_current_tab >= state.ask_user_questions.len() {
        return;
    }

    if state.ask_user_selected_option > 0 {
        state.ask_user_selected_option -= 1;
    }
    refresh_ask_user_block(state);
}

/// Select the current option (or submit if on Submit tab)
pub fn handle_ask_user_select_option(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    if !state.show_ask_user_popup {
        return;
    }

    // If on Submit tab, try to submit
    if state.ask_user_current_tab >= state.ask_user_questions.len() {
        handle_ask_user_submit(state, output_tx);
        return;
    }

    let current_q = &state.ask_user_questions[state.ask_user_current_tab];
    let question_label = current_q.label.clone();

    // Check if custom input is selected
    if current_q.allow_custom && state.ask_user_selected_option == current_q.options.len() {
        // Custom input selected - save the custom answer if not empty
        if !state.ask_user_custom_input.is_empty() {
            let answer = AskUserAnswer {
                question_label: question_label.clone(),
                answer: state.ask_user_custom_input.clone(),
                is_custom: true,
            };
            state
                .ask_user_answers
                .insert(current_q.label.clone(), answer);

            // Auto-advance to next question or Submit
            if state.ask_user_current_tab < state.ask_user_questions.len() {
                state.ask_user_current_tab += 1;
                state.ask_user_selected_option = 0;
                state.ask_user_custom_input.clear();
            }
        }
        refresh_ask_user_block(state);
        return;
    }

    // Regular option selected
    if let Some(opt) = current_q.options.get(state.ask_user_selected_option) {
        let answer = AskUserAnswer {
            question_label,
            answer: opt.value.clone(),
            is_custom: false,
        };
        state
            .ask_user_answers
            .insert(current_q.label.clone(), answer);

        // Auto-advance to next question or Submit
        if state.ask_user_current_tab < state.ask_user_questions.len() {
            state.ask_user_current_tab += 1;
            state.ask_user_selected_option = 0;
            state.ask_user_custom_input.clear();
        }
    }
    refresh_ask_user_block(state);
}

/// Handle character input for custom answer
pub fn handle_ask_user_custom_input_changed(state: &mut AppState, c: char) {
    if !state.show_ask_user_popup {
        return;
    }

    // Only accept input if on a question tab and custom option is selected
    if state.ask_user_current_tab >= state.ask_user_questions.len() {
        return;
    }

    let current_q = &state.ask_user_questions[state.ask_user_current_tab];
    if current_q.allow_custom && state.ask_user_selected_option == current_q.options.len() {
        state.ask_user_custom_input.push(c);
        refresh_ask_user_block(state);
    }
}

/// Handle backspace for custom answer
pub fn handle_ask_user_custom_input_backspace(state: &mut AppState) {
    if !state.show_ask_user_popup {
        return;
    }

    // Only accept input if on a question tab and custom option is selected
    if state.ask_user_current_tab >= state.ask_user_questions.len() {
        return;
    }

    let current_q = &state.ask_user_questions[state.ask_user_current_tab];
    if current_q.allow_custom && state.ask_user_selected_option == current_q.options.len() {
        state.ask_user_custom_input.pop();
        refresh_ask_user_block(state);
    }
}

/// Handle delete (clear all) for custom answer
pub fn handle_ask_user_custom_input_delete(state: &mut AppState) {
    if !state.show_ask_user_popup {
        return;
    }

    // Only accept input if on a question tab and custom option is selected
    if state.ask_user_current_tab >= state.ask_user_questions.len() {
        return;
    }

    let current_q = &state.ask_user_questions[state.ask_user_current_tab];
    if current_q.allow_custom && state.ask_user_selected_option == current_q.options.len() {
        state.ask_user_custom_input.clear();
        refresh_ask_user_block(state);
    }
}

/// Submit all answers
pub fn handle_ask_user_submit(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    if !state.show_ask_user_popup {
        return;
    }

    // Check if all required questions are answered
    let all_required_answered = state
        .ask_user_questions
        .iter()
        .filter(|q| q.required)
        .all(|q| state.ask_user_answers.contains_key(&q.label));

    if !all_required_answered {
        // Can't submit yet - maybe flash a warning or navigate to first unanswered
        // For now, just find the first unanswered required question and go there
        for (i, q) in state.ask_user_questions.iter().enumerate() {
            if q.required && !state.ask_user_answers.contains_key(&q.label) {
                state.ask_user_current_tab = i;
                state.ask_user_selected_option = 0;
                break;
            }
        }
        return;
    }

    // Build the structured result as documented in the tool description
    let answers: Vec<AskUserAnswer> = state
        .ask_user_questions
        .iter()
        .filter_map(|q| state.ask_user_answers.get(&q.label).cloned())
        .collect();

    let result = AskUserResult {
        answers,
        completed: true,
        reason: None,
    };

    // Serialize to JSON as documented in the tool description
    let display_result = serde_json::to_string_pretty(&result)
        .unwrap_or_else(|e| format!("{{\"error\": \"Failed to serialize result: {}\"}}", e));

    // Send the result back
    if let Some(tool_call) = state.ask_user_tool_call.take() {
        let tool_result = ToolCallResult {
            call: tool_call,
            result: display_result,
            status: ToolCallResultStatus::Success,
        };

        let _ = output_tx.try_send(OutputEvent::AskUserResponse(tool_result));
    }

    // Close the popup
    close_ask_user_popup(state);
}

/// Cancel and close the popup
pub fn handle_ask_user_cancel(state: &mut AppState, output_tx: &Sender<OutputEvent>) {
    if !state.show_ask_user_popup {
        return;
    }

    // Send the cancelled result back as JSON (matching the documented format)
    if let Some(tool_call) = state.ask_user_tool_call.take() {
        let result = AskUserResult {
            answers: vec![],
            completed: false,
            reason: Some("User cancelled the question prompt.".to_string()),
        };

        let display_result = serde_json::to_string_pretty(&result)
            .unwrap_or_else(|e| format!("{{\"error\": \"Failed to serialize result: {}\"}}", e));

        let tool_result = ToolCallResult {
            call: tool_call,
            result: display_result,
            status: ToolCallResultStatus::Cancelled,
        };

        let _ = output_tx.try_send(OutputEvent::AskUserResponse(tool_result));
    }

    // Close the popup
    close_ask_user_popup(state);
}

/// Close the ask user interaction and remove the inline block
fn close_ask_user_popup(state: &mut AppState) {
    // Remove the inline message block
    if let Some(msg_id) = state.ask_user_message_id.take() {
        state.messages.retain(|m| m.id != msg_id);
    }

    state.show_ask_user_popup = false;
    state.ask_user_focused = false;
    state.ask_user_questions.clear();
    state.ask_user_answers.clear();
    state.ask_user_current_tab = 0;
    state.ask_user_selected_option = 0;
    state.ask_user_custom_input.clear();
    state.ask_user_tool_call = None;

    // Invalidate cache to update display
    crate::services::message::invalidate_message_lines_cache(state);
}

/// Check if the current question has custom input selected
pub fn is_custom_input_selected(state: &AppState) -> bool {
    if !state.show_ask_user_popup {
        return false;
    }

    if state.ask_user_current_tab >= state.ask_user_questions.len() {
        return false;
    }

    let current_q = &state.ask_user_questions[state.ask_user_current_tab];
    current_q.allow_custom && state.ask_user_selected_option == current_q.options.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppStateOptions;
    use stakai::Model;
    use stakpak_shared::models::integrations::openai::{AskUserOption, FunctionCall};
    use tokio::sync::mpsc;

    /// Helper to create a minimal AppState for testing
    fn create_test_state() -> AppState {
        AppState::new(AppStateOptions {
            latest_version: None,
            redact_secrets: false,
            privacy_mode: false,
            is_git_repo: false,
            auto_approve_tools: None,
            allowed_tools: None,
            input_tx: None,
            model: Model::default(),
            editor_command: None,
            auth_display_info: (None, None, None),
            board_agent_id: None,
            init_prompt_content: None,
        })
    }

    /// Helper to create test questions
    fn create_test_questions() -> Vec<AskUserQuestion> {
        vec![
            AskUserQuestion {
                label: "Environment".to_string(),
                question: "Which environment?".to_string(),
                options: vec![
                    AskUserOption {
                        value: "dev".to_string(),
                        label: "Development".to_string(),
                        description: Some("For testing".to_string()),
                    },
                    AskUserOption {
                        value: "prod".to_string(),
                        label: "Production".to_string(),
                        description: None,
                    },
                ],
                allow_custom: true,
                required: true,
            },
            AskUserQuestion {
                label: "Confirm".to_string(),
                question: "Are you sure?".to_string(),
                options: vec![
                    AskUserOption {
                        value: "yes".to_string(),
                        label: "Yes".to_string(),
                        description: None,
                    },
                    AskUserOption {
                        value: "no".to_string(),
                        label: "No".to_string(),
                        description: None,
                    },
                ],
                allow_custom: false,
                required: true,
            },
        ]
    }

    /// Helper to create a test tool call
    fn create_test_tool_call() -> ToolCall {
        ToolCall {
            id: "call_test123".to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "ask_user".to_string(),
                arguments: "{}".to_string(),
            },
            metadata: None,
        }
    }

    #[tokio::test]
    async fn test_show_ask_user_popup() {
        let mut state = create_test_state();
        let questions = create_test_questions();
        let tool_call = create_test_tool_call();

        assert!(!state.show_ask_user_popup);
        assert!(state.ask_user_questions.is_empty());

        handle_show_ask_user_popup(&mut state, tool_call.clone(), questions.clone());

        assert!(state.show_ask_user_popup);
        assert_eq!(state.ask_user_questions.len(), 2);
        assert_eq!(state.ask_user_current_tab, 0);
        assert_eq!(state.ask_user_selected_option, 0);
        assert!(state.ask_user_tool_call.is_some());
    }

    #[tokio::test]
    async fn test_show_ask_user_popup_empty_questions() {
        let mut state = create_test_state();
        let tool_call = create_test_tool_call();

        handle_show_ask_user_popup(&mut state, tool_call, vec![]);

        // Should not show popup with empty questions
        assert!(!state.show_ask_user_popup);
    }

    #[tokio::test]
    async fn test_tab_navigation() {
        let mut state = create_test_state();
        let questions = create_test_questions();
        let tool_call = create_test_tool_call();

        handle_show_ask_user_popup(&mut state, tool_call, questions);

        // Start at tab 0
        assert_eq!(state.ask_user_current_tab, 0);

        // Navigate to next tab
        handle_ask_user_next_tab(&mut state);
        assert_eq!(state.ask_user_current_tab, 1);

        // Navigate to Submit tab (index 2 for 2 questions)
        handle_ask_user_next_tab(&mut state);
        assert_eq!(state.ask_user_current_tab, 2);

        // Can't go beyond Submit
        handle_ask_user_next_tab(&mut state);
        assert_eq!(state.ask_user_current_tab, 2);

        // Navigate back
        handle_ask_user_prev_tab(&mut state);
        assert_eq!(state.ask_user_current_tab, 1);

        handle_ask_user_prev_tab(&mut state);
        assert_eq!(state.ask_user_current_tab, 0);

        // Can't go before first question
        handle_ask_user_prev_tab(&mut state);
        assert_eq!(state.ask_user_current_tab, 0);
    }

    #[tokio::test]
    async fn test_option_navigation() {
        let mut state = create_test_state();
        let questions = create_test_questions();
        let tool_call = create_test_tool_call();

        handle_show_ask_user_popup(&mut state, tool_call, questions);

        // First question has 2 options + custom = 3 total
        assert_eq!(state.ask_user_selected_option, 0);

        handle_ask_user_next_option(&mut state);
        assert_eq!(state.ask_user_selected_option, 1);

        handle_ask_user_next_option(&mut state);
        assert_eq!(state.ask_user_selected_option, 2); // custom option

        // Can't go beyond last option
        handle_ask_user_next_option(&mut state);
        assert_eq!(state.ask_user_selected_option, 2);

        // Navigate back
        handle_ask_user_prev_option(&mut state);
        assert_eq!(state.ask_user_selected_option, 1);

        handle_ask_user_prev_option(&mut state);
        assert_eq!(state.ask_user_selected_option, 0);

        // Can't go before first option
        handle_ask_user_prev_option(&mut state);
        assert_eq!(state.ask_user_selected_option, 0);
    }

    #[tokio::test]
    async fn test_option_navigation_no_custom() {
        let mut state = create_test_state();
        let questions = create_test_questions();
        let tool_call = create_test_tool_call();

        handle_show_ask_user_popup(&mut state, tool_call, questions);

        // Navigate to second question (no custom option)
        handle_ask_user_next_tab(&mut state);
        assert_eq!(state.ask_user_current_tab, 1);
        assert_eq!(state.ask_user_selected_option, 0);

        // Second question has 2 options only (no custom)
        handle_ask_user_next_option(&mut state);
        assert_eq!(state.ask_user_selected_option, 1);

        // Can't go beyond (no custom option)
        handle_ask_user_next_option(&mut state);
        assert_eq!(state.ask_user_selected_option, 1);
    }

    #[tokio::test]
    async fn test_select_predefined_option() {
        let mut state = create_test_state();
        let questions = create_test_questions();
        let tool_call = create_test_tool_call();
        let (output_tx, _output_rx) = mpsc::channel(10);

        handle_show_ask_user_popup(&mut state, tool_call, questions);

        // Select first option (dev)
        handle_ask_user_select_option(&mut state, &output_tx);

        // Should have recorded the answer
        assert!(state.ask_user_answers.contains_key("Environment"));
        let answer = &state.ask_user_answers["Environment"];
        assert_eq!(answer.answer, "dev");
        assert!(!answer.is_custom);

        // Should auto-advance to next question
        assert_eq!(state.ask_user_current_tab, 1);
    }

    #[tokio::test]
    async fn test_select_custom_option() {
        let mut state = create_test_state();
        let questions = create_test_questions();
        let tool_call = create_test_tool_call();
        let (output_tx, _output_rx) = mpsc::channel(10);

        handle_show_ask_user_popup(&mut state, tool_call, questions);

        // Navigate to custom option (index 2)
        handle_ask_user_next_option(&mut state);
        handle_ask_user_next_option(&mut state);
        assert_eq!(state.ask_user_selected_option, 2);

        // Type custom input
        handle_ask_user_custom_input_changed(&mut state, 's');
        handle_ask_user_custom_input_changed(&mut state, 't');
        handle_ask_user_custom_input_changed(&mut state, 'a');
        handle_ask_user_custom_input_changed(&mut state, 'g');
        handle_ask_user_custom_input_changed(&mut state, 'i');
        handle_ask_user_custom_input_changed(&mut state, 'n');
        handle_ask_user_custom_input_changed(&mut state, 'g');

        assert_eq!(state.ask_user_custom_input, "staging");

        // Select the custom option
        handle_ask_user_select_option(&mut state, &output_tx);

        // Should have recorded custom answer
        assert!(state.ask_user_answers.contains_key("Environment"));
        let answer = &state.ask_user_answers["Environment"];
        assert_eq!(answer.answer, "staging");
        assert!(answer.is_custom);
    }

    #[tokio::test]
    async fn test_custom_input_backspace() {
        let mut state = create_test_state();
        let questions = create_test_questions();
        let tool_call = create_test_tool_call();

        handle_show_ask_user_popup(&mut state, tool_call, questions);

        // Navigate to custom option
        handle_ask_user_next_option(&mut state);
        handle_ask_user_next_option(&mut state);

        // Type and then backspace
        handle_ask_user_custom_input_changed(&mut state, 'a');
        handle_ask_user_custom_input_changed(&mut state, 'b');
        handle_ask_user_custom_input_changed(&mut state, 'c');
        assert_eq!(state.ask_user_custom_input, "abc");

        handle_ask_user_custom_input_backspace(&mut state);
        assert_eq!(state.ask_user_custom_input, "ab");

        handle_ask_user_custom_input_backspace(&mut state);
        handle_ask_user_custom_input_backspace(&mut state);
        assert_eq!(state.ask_user_custom_input, "");

        // Backspace on empty is safe
        handle_ask_user_custom_input_backspace(&mut state);
        assert_eq!(state.ask_user_custom_input, "");
    }

    #[tokio::test]
    async fn test_custom_input_delete_clears_all() {
        let mut state = create_test_state();
        let questions = create_test_questions();
        let tool_call = create_test_tool_call();

        handle_show_ask_user_popup(&mut state, tool_call, questions);

        // Navigate to custom option
        handle_ask_user_next_option(&mut state);
        handle_ask_user_next_option(&mut state);

        // Type some input
        handle_ask_user_custom_input_changed(&mut state, 't');
        handle_ask_user_custom_input_changed(&mut state, 'e');
        handle_ask_user_custom_input_changed(&mut state, 's');
        handle_ask_user_custom_input_changed(&mut state, 't');
        assert_eq!(state.ask_user_custom_input, "test");

        // Delete clears everything
        handle_ask_user_custom_input_delete(&mut state);
        assert_eq!(state.ask_user_custom_input, "");
    }

    #[tokio::test]
    async fn test_is_custom_input_selected() {
        let mut state = create_test_state();
        let questions = create_test_questions();
        let tool_call = create_test_tool_call();

        // Not selected when popup not shown
        assert!(!is_custom_input_selected(&state));

        handle_show_ask_user_popup(&mut state, tool_call, questions);

        // Not selected at option 0
        assert!(!is_custom_input_selected(&state));

        // Navigate to custom option
        handle_ask_user_next_option(&mut state);
        handle_ask_user_next_option(&mut state);
        assert!(is_custom_input_selected(&state));

        // Navigate to second question (no custom)
        handle_ask_user_next_tab(&mut state);
        state.ask_user_selected_option = 1; // Last option
        assert!(!is_custom_input_selected(&state)); // Second question has no custom
    }

    #[tokio::test]
    async fn test_submit_with_all_required_answered() {
        let mut state = create_test_state();
        let questions = create_test_questions();
        let tool_call = create_test_tool_call();
        let (output_tx, mut output_rx) = mpsc::channel(10);

        handle_show_ask_user_popup(&mut state, tool_call, questions);

        // Answer both required questions
        state.ask_user_answers.insert(
            "Environment".to_string(),
            AskUserAnswer {
                question_label: "Environment".to_string(),
                answer: "dev".to_string(),
                is_custom: false,
            },
        );
        state.ask_user_answers.insert(
            "Confirm".to_string(),
            AskUserAnswer {
                question_label: "Confirm".to_string(),
                answer: "yes".to_string(),
                is_custom: false,
            },
        );

        // Go to Submit tab
        state.ask_user_current_tab = 2;

        // Submit
        handle_ask_user_submit(&mut state, &output_tx);

        // Popup should be closed
        assert!(!state.show_ask_user_popup);
        assert!(state.ask_user_questions.is_empty());
        assert!(state.ask_user_answers.is_empty());

        // Should have sent response
        let event = output_rx.try_recv().unwrap();
        match event {
            OutputEvent::AskUserResponse(result) => {
                assert_eq!(result.status, ToolCallResultStatus::Success);
                // Result should be valid JSON
                let parsed: AskUserResult = serde_json::from_str(&result.result).unwrap();
                assert!(parsed.completed);
                assert!(parsed.reason.is_none());
                assert_eq!(parsed.answers.len(), 2);
            }
            _ => panic!("Expected AskUserResponse event"),
        }
    }

    #[tokio::test]
    async fn test_submit_blocked_without_required() {
        let mut state = create_test_state();
        let questions = create_test_questions();
        let tool_call = create_test_tool_call();
        let (output_tx, mut output_rx) = mpsc::channel(10);

        handle_show_ask_user_popup(&mut state, tool_call, questions);

        // Only answer first question
        state.ask_user_answers.insert(
            "Environment".to_string(),
            AskUserAnswer {
                question_label: "Environment".to_string(),
                answer: "dev".to_string(),
                is_custom: false,
            },
        );

        // Go to Submit tab
        state.ask_user_current_tab = 2;

        // Try to submit
        handle_ask_user_submit(&mut state, &output_tx);

        // Should NOT have sent response
        assert!(output_rx.try_recv().is_err());

        // Should navigate to first unanswered required question
        assert_eq!(state.ask_user_current_tab, 1); // Second question
        assert!(state.show_ask_user_popup); // Popup still open
    }

    #[tokio::test]
    async fn test_cancel() {
        let mut state = create_test_state();
        let questions = create_test_questions();
        let tool_call = create_test_tool_call();
        let (output_tx, mut output_rx) = mpsc::channel(10);

        handle_show_ask_user_popup(&mut state, tool_call, questions);

        // Cancel
        handle_ask_user_cancel(&mut state, &output_tx);

        // Popup should be closed
        assert!(!state.show_ask_user_popup);

        // Should have sent cancelled response
        let event = output_rx.try_recv().unwrap();
        match event {
            OutputEvent::AskUserResponse(result) => {
                assert_eq!(result.status, ToolCallResultStatus::Cancelled);
                // Result should be valid JSON
                let parsed: AskUserResult = serde_json::from_str(&result.result).unwrap();
                assert!(!parsed.completed);
                assert!(parsed.reason.is_some());
                assert!(parsed.reason.unwrap().contains("cancelled"));
            }
            _ => panic!("Expected AskUserResponse event"),
        }
    }

    #[tokio::test]
    async fn test_handlers_no_op_when_popup_not_visible() {
        let mut state = create_test_state();
        let (output_tx, _output_rx) = mpsc::channel::<OutputEvent>(10);

        // All these should be no-ops when popup is not visible
        handle_ask_user_next_tab(&mut state);
        handle_ask_user_prev_tab(&mut state);
        handle_ask_user_next_option(&mut state);
        handle_ask_user_prev_option(&mut state);
        handle_ask_user_select_option(&mut state, &output_tx);
        handle_ask_user_custom_input_changed(&mut state, 'x');
        handle_ask_user_custom_input_backspace(&mut state);
        handle_ask_user_custom_input_delete(&mut state);
        handle_ask_user_submit(&mut state, &output_tx);
        handle_ask_user_cancel(&mut state, &output_tx);

        // State should be unchanged
        assert!(!state.show_ask_user_popup);
        assert!(state.ask_user_questions.is_empty());
    }

    #[tokio::test]
    async fn test_option_navigation_on_submit_tab_no_op() {
        let mut state = create_test_state();
        let questions = create_test_questions();
        let tool_call = create_test_tool_call();

        handle_show_ask_user_popup(&mut state, tool_call, questions);

        // Navigate to Submit tab
        state.ask_user_current_tab = 2;

        // Option navigation should be no-op on Submit tab
        handle_ask_user_next_option(&mut state);
        assert_eq!(state.ask_user_selected_option, 0);

        handle_ask_user_prev_option(&mut state);
        assert_eq!(state.ask_user_selected_option, 0);
    }
}
