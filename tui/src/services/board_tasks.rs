//! Board Tasks Service
//!
//! Fetches tasks from agent-board CLI and converts them to TodoItem structs
//! for display in the side panel Tasks section.

use crate::services::changeset::{TodoItem, TodoStatus};
use serde::Deserialize;
use std::process::Command;

/// Card data from agent-board JSON output
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct BoardCard {
    pub id: String,
    pub board_id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub assigned_to: Option<String>,
    pub tags: Vec<String>,
    pub checklist: Vec<ChecklistItem>,
    pub created_at: String,
    pub updated_at: String,
}

/// Checklist item from agent-board
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ChecklistItem {
    pub id: String,
    pub text: String,
    pub checked: bool,
}

/// Get the path to the agent-board binary
fn get_agent_board_path() -> Option<String> {
    // First try the plugins directory
    if let Some(home) = dirs::home_dir() {
        let plugin_path = home.join(".stakpak/plugins/agent-board");
        if plugin_path.exists() {
            return Some(plugin_path.to_string_lossy().to_string());
        }
    }

    // Fall back to PATH
    if Command::new("agent-board")
        .arg("--version")
        .output()
        .is_ok()
    {
        return Some("agent-board".to_string());
    }

    None
}

/// Fetch cards assigned to the current agent from agent-board
pub fn fetch_agent_tasks(agent_id: &str) -> Result<Vec<BoardCard>, String> {
    let board_path = get_agent_board_path().ok_or("agent-board not found")?;

    let output = Command::new(&board_path)
        .arg("mine")
        .arg("--format")
        .arg("json")
        .env("AGENT_BOARD_AGENT_ID", agent_id)
        .output()
        .map_err(|e| format!("Failed to execute agent-board: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("agent-board failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).map_err(|e| format!("Failed to parse JSON: {}", e))
}

/// Task progress statistics
#[derive(Debug, Clone, Default)]
pub struct TaskProgress {
    pub completed: usize,
    pub total: usize,
}

impl TaskProgress {
    /// Format as "X/Y" for display
    pub fn display(&self) -> String {
        format!("{}/{}", self.completed, self.total)
    }
}

/// Calculate progress from cards: checked items / total checklist items
/// Cards without checklists count as 1 item (done if card is done)
pub fn calculate_progress(cards: &[BoardCard]) -> TaskProgress {
    let mut completed = 0;
    let mut total = 0;

    for card in cards {
        if card.checklist.is_empty() {
            // Card with no checklist counts as 1 item
            total += 1;
            if card.status == "done" {
                completed += 1;
            }
        } else {
            // Count checklist items
            for item in &card.checklist {
                total += 1;
                if item.checked {
                    completed += 1;
                }
            }
        }
    }

    TaskProgress { completed, total }
}

/// Convert board cards to TodoItems for display with smart collapsing
/// Strategy: Show last completed + all in-progress/pending, collapse older completed items
pub fn cards_to_todo_items(cards: &[BoardCard]) -> Vec<TodoItem> {
    let mut items = Vec::new();

    for card in cards {
        // Convert card status to TodoStatus
        let status = match card.status.as_str() {
            "done" => TodoStatus::Done,
            "in_progress" | "in-progress" => TodoStatus::InProgress,
            "pending_review" | "pending-review" => TodoStatus::InProgress,
            _ => TodoStatus::Pending, // "todo" or unknown
        };

        // Add the card itself as a task
        items.push(TodoItem::new(card.name.clone()).with_status(status));

        // Process checklist items with smart collapsing
        let checklist_items: Vec<_> = card
            .checklist
            .iter()
            .map(|item| {
                let checklist_status = if item.checked {
                    TodoStatus::Done
                } else {
                    TodoStatus::Pending
                };
                (item.text.clone(), checklist_status)
            })
            .collect();

        // Find indices of done items and first non-done item
        let done_count = checklist_items.iter().filter(|(_, s)| *s == TodoStatus::Done).count();
        let first_pending_idx = checklist_items.iter().position(|(_, s)| *s != TodoStatus::Done);

        // Determine which items to show vs collapse
        // Show: last done item (if any) + all pending items
        // Collapse: all done items except the last one before pending items
        let mut collapsed_done_count = 0;
        let mut last_shown_done_idx: Option<usize> = None;

        // Find the last done item before the first pending (or last done overall)
        if done_count > 0 {
            if let Some(pending_idx) = first_pending_idx {
                // Last done before first pending
                if pending_idx > 0 {
                    last_shown_done_idx = Some(pending_idx - 1);
                    collapsed_done_count = pending_idx.saturating_sub(1);
                }
            } else {
                // All items are done - show only the last one
                last_shown_done_idx = Some(checklist_items.len() - 1);
                collapsed_done_count = checklist_items.len().saturating_sub(1);
            }
        }

        // Add collapsed indicator if there are hidden done items
        if collapsed_done_count > 0 {
            items.push(
                TodoItem::checklist_item(format!("({} completed)", collapsed_done_count))
                    .with_status(TodoStatus::Done)
                    .into_collapsed_indicator(),
            );
        }

        // Add visible items
        for (idx, (text, status)) in checklist_items.iter().enumerate() {
            let is_last_shown_done = last_shown_done_idx == Some(idx);
            let is_pending = *status != TodoStatus::Done;

            if is_last_shown_done || is_pending {
                items.push(TodoItem::checklist_item(text.clone()).with_status(*status));
            }
        }
    }

    items
}

/// Result of fetching tasks including items and progress
#[derive(Debug, Clone)]
pub struct FetchTasksResult {
    pub items: Vec<TodoItem>,
    pub progress: TaskProgress,
}

/// Fetch and convert tasks in one call, including progress stats
pub fn fetch_tasks_as_todo_items(agent_id: &str) -> Result<FetchTasksResult, String> {
    let cards = fetch_agent_tasks(agent_id)?;
    Ok(FetchTasksResult {
        items: cards_to_todo_items(&cards),
        progress: calculate_progress(&cards),
    })
}

/// Extract board agent ID from message history by scanning backwards for the pattern
/// `AGENT_BOARD_AGENT_ID=agent_XXXX` or just `agent_XXXX` in tool results/arguments
pub fn extract_board_agent_id_from_messages(
    messages: &[crate::services::message::Message],
) -> Option<String> {
    use crate::services::message::MessageContent;
    use regex::Regex;

    // Pattern to match agent IDs: agent_ followed by hex characters
    let agent_id_pattern = Regex::new(r"agent_[a-f0-9]{12}").ok()?;

    // Scan messages backwards (most recent first)
    for message in messages.iter().rev() {
        let text_to_search = match &message.content {
            // Tool call results contain the output
            MessageContent::RenderResultBorderBlock(result) => Some(result.result.as_str()),
            MessageContent::RenderFullContentMessage(result) => Some(result.result.as_str()),
            MessageContent::RenderCommandCollapsedResult(result) => Some(result.result.as_str()),

            // Tool call arguments (for run_command, the command might set the env var)
            MessageContent::RenderPendingBorderBlock(tool_call, _) => {
                Some(tool_call.function.arguments.as_str())
            }
            MessageContent::RenderCollapsedMessage(tool_call) => {
                Some(tool_call.function.arguments.as_str())
            }

            // Run command blocks contain command and result
            MessageContent::RenderRunCommandBlock(command, result, _) => {
                // Check command first, then result
                if let Some(cap) = agent_id_pattern.find(command) {
                    return Some(cap.as_str().to_string());
                }
                result.as_deref()
            }

            // Plain text and assistant messages might contain agent IDs
            MessageContent::Plain(text, _) => Some(text.as_str()),
            MessageContent::AssistantMD(text, _) => Some(text.as_str()),
            MessageContent::PlainText(text) => Some(text.as_str()),

            _ => None,
        };

        if let Some(cap) = text_to_search.and_then(|text| agent_id_pattern.find(text)) {
            return Some(cap.as_str().to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cards_to_todo_items_empty() {
        let cards: Vec<BoardCard> = vec![];
        let items = cards_to_todo_items(&cards);
        assert!(items.is_empty());
    }

    #[test]
    fn test_extract_agent_id_pattern() {
        use regex::Regex;
        let pattern = Regex::new(r"agent_[a-f0-9]{12}").unwrap();

        // Valid agent IDs
        assert!(pattern.is_match("agent_48741c1a8a0f"));
        assert!(pattern.is_match("export AGENT_BOARD_AGENT_ID=agent_48741c1a8a0f"));
        assert!(pattern.is_match("Created agent: agent_c4ee049a764e (Name: test)"));

        // Invalid patterns
        assert!(!pattern.is_match("agent_123")); // too short
        assert!(!pattern.is_match("agent_XXXX")); // not hex
    }

    #[test]
    fn test_status_conversion() {
        let card = BoardCard {
            id: "card_123".to_string(),
            board_id: "board_456".to_string(),
            name: "Test Task".to_string(),
            description: None,
            status: "in_progress".to_string(),
            assigned_to: Some("agent_789".to_string()),
            tags: vec![],
            checklist: vec![],
            created_at: "2026-01-25T00:00:00Z".to_string(),
            updated_at: "2026-01-25T00:00:00Z".to_string(),
        };

        let items = cards_to_todo_items(&[card]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text, "Test Task");
        assert_eq!(items[0].status, TodoStatus::InProgress);
    }

    #[test]
    fn test_checklist_items_with_collapsing() {
        let card = BoardCard {
            id: "card_123".to_string(),
            board_id: "board_456".to_string(),
            name: "Main Task".to_string(),
            description: None,
            status: "in_progress".to_string(),
            assigned_to: Some("agent_789".to_string()),
            tags: vec![],
            checklist: vec![
                ChecklistItem {
                    id: "item_1".to_string(),
                    text: "Sub-task 1".to_string(),
                    checked: true,
                },
                ChecklistItem {
                    id: "item_2".to_string(),
                    text: "Sub-task 2".to_string(),
                    checked: false,
                },
            ],
            created_at: "2026-01-25T00:00:00Z".to_string(),
            updated_at: "2026-01-25T00:00:00Z".to_string(),
        };

        let items = cards_to_todo_items(&[card]);
        // With collapsing: Card + last done (Sub-task 1) + pending (Sub-task 2)
        // No collapsed indicator since only 1 done item (nothing to collapse)
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].text, "Main Task");
        assert_eq!(items[1].text, "Sub-task 1");
        assert_eq!(items[1].status, TodoStatus::Done);
        assert_eq!(items[2].text, "Sub-task 2");
        assert_eq!(items[2].status, TodoStatus::Pending);
    }

    #[test]
    fn test_multiple_cards() {
        let cards = vec![
            BoardCard {
                id: "card_1".to_string(),
                board_id: "board_1".to_string(),
                name: "Card 1 - Done".to_string(),
                description: None,
                status: "done".to_string(),
                assigned_to: None,
                tags: vec![],
                checklist: vec![
                    ChecklistItem {
                        id: "1".to_string(),
                        text: "Item 1".to_string(),
                        checked: true,
                    },
                    ChecklistItem {
                        id: "2".to_string(),
                        text: "Item 2".to_string(),
                        checked: true,
                    },
                ],
                created_at: "".to_string(),
                updated_at: "".to_string(),
            },
            BoardCard {
                id: "card_2".to_string(),
                board_id: "board_1".to_string(),
                name: "Card 2 - In Progress".to_string(),
                description: None,
                status: "in_progress".to_string(),
                assigned_to: None,
                tags: vec![],
                checklist: vec![
                    ChecklistItem {
                        id: "3".to_string(),
                        text: "Item 3".to_string(),
                        checked: false,
                    },
                ],
                created_at: "".to_string(),
                updated_at: "".to_string(),
            },
        ];

        let items = cards_to_todo_items(&cards);
        
        // Card 1: Card + collapsed(1) + Item 2 (last done)
        // Card 2: Card + Item 3 (pending)
        // Total: 5 items
        assert_eq!(items.len(), 5, "Expected 5 items, got: {:?}", items.iter().map(|i| &i.text).collect::<Vec<_>>());
        
        assert_eq!(items[0].text, "Card 1 - Done");
        assert_eq!(items[1].text, "(1 completed)"); // collapsed indicator
        assert_eq!(items[2].text, "Item 2"); // last done
        assert_eq!(items[3].text, "Card 2 - In Progress");
        assert_eq!(items[4].text, "Item 3"); // pending
    }
}
