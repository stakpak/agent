//! Todo Extractor
//!
//! Extracts todo items from `<todo>` XML tags in assistant messages and
//! parses them into `TodoItem` structs for display in the side panel.

use crate::services::changeset::{TodoItem, TodoStatus};

/// Extract content between `<todo>` and `</todo>` tags
pub fn extract_todos_from_xml(text: &str) -> Option<String> {
    let start_tag = "<todo>";
    let end_tag = "</todo>";

    let start_idx = text.find(start_tag)?;
    let content_start = start_idx + start_tag.len();

    // Find the closing tag after the opening tag
    let end_idx = text[content_start..].find(end_tag)?;

    Some(text[content_start..content_start + end_idx].to_string())
}

/// Parse a single line into a TodoItem
///
/// Supported formats:
/// - `- [ ]` text → Pending
/// - `- [x]` or `- [X]` text → Done
/// - `- [/]` text → InProgress
pub fn parse_todo_line(line: &str) -> Option<TodoItem> {
    let line = line.trim();

    if line.is_empty() {
        return None;
    }

    // Check for markdown-style todos: - [ ], - [x], - [X], - [/]
    if line.starts_with("- [ ]") {
        let text = line.strip_prefix("- [ ]")?.trim().to_string();
        if text.is_empty() {
            return None;
        }
        Some(TodoItem::new(text).with_status(TodoStatus::Pending))
    } else if line.starts_with("- [x]") || line.starts_with("- [X]") {
        let text = line
            .strip_prefix("- [x]")
            .or_else(|| line.strip_prefix("- [X]"))?
            .trim()
            .to_string();
        if text.is_empty() {
            return None;
        }
        Some(TodoItem::new(text).with_status(TodoStatus::Done))
    } else if line.starts_with("- [/]") {
        let text = line.strip_prefix("- [/]")?.trim().to_string();
        if text.is_empty() {
            return None;
        }
        Some(TodoItem::new(text).with_status(TodoStatus::InProgress))
    } else {
        None
    }
}

/// Extract all todos from message text containing `<todo>` blocks
///
/// Status logic:
/// - Items marked [x] → Done (green)
/// - First non-completed item → InProgress (yellow)
/// - Remaining non-completed items → Pending (gray)
pub fn extract_todos(text: &str) -> Vec<TodoItem> {
    let mut todos = Vec::new();

    if let Some(todo_content) = extract_todos_from_xml(text) {
        for line in todo_content.lines() {
            if let Some(item) = parse_todo_line(line) {
                todos.push(item);
            }
        }
    }

    // Apply status logic: first non-completed becomes InProgress
    let mut found_first_pending = false;
    for todo in &mut todos {
        if todo.status != TodoStatus::Done {
            if !found_first_pending {
                todo.status = TodoStatus::InProgress;
                found_first_pending = true;
            } else {
                todo.status = TodoStatus::Pending;
            }
        }
    }

    todos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_todos_from_xml() {
        let text = "Some text <todo>\n- [ ] Task 1\n- [x] Task 2\n</todo> more text";
        let content = extract_todos_from_xml(text);
        assert!(content.is_some());
        assert!(content.unwrap().contains("Task 1"));
    }

    #[test]
    fn test_extract_todos_from_xml_no_tag() {
        let text = "Some text without todo tags";
        assert!(extract_todos_from_xml(text).is_none());
    }

    #[test]
    fn test_extract_todos_from_xml_unclosed() {
        let text = "Some text <todo> unclosed tag";
        assert!(extract_todos_from_xml(text).is_none());
    }

    #[test]
    fn test_parse_todo_line_pending() {
        let item = parse_todo_line("- [ ] Implement feature");
        assert!(item.is_some());
        let item = item.unwrap();
        assert_eq!(item.text, "Implement feature");
        assert_eq!(item.status, TodoStatus::Pending);
    }

    #[test]
    fn test_parse_todo_line_done_lowercase() {
        let item = parse_todo_line("- [x] Completed task");
        assert!(item.is_some());
        let item = item.unwrap();
        assert_eq!(item.text, "Completed task");
        assert_eq!(item.status, TodoStatus::Done);
    }

    #[test]
    fn test_parse_todo_line_done_uppercase() {
        let item = parse_todo_line("- [X] Another completed task");
        assert!(item.is_some());
        let item = item.unwrap();
        assert_eq!(item.text, "Another completed task");
        assert_eq!(item.status, TodoStatus::Done);
    }

    #[test]
    fn test_parse_todo_line_in_progress() {
        let item = parse_todo_line("- [/] Work in progress");
        assert!(item.is_some());
        let item = item.unwrap();
        assert_eq!(item.text, "Work in progress");
        assert_eq!(item.status, TodoStatus::InProgress);
    }

    #[test]
    fn test_parse_todo_line_empty() {
        assert!(parse_todo_line("").is_none());
        assert!(parse_todo_line("   ").is_none());
    }

    #[test]
    fn test_parse_todo_line_no_text() {
        assert!(parse_todo_line("- [ ]").is_none());
        assert!(parse_todo_line("- [x]").is_none());
    }

    #[test]
    fn test_parse_todo_line_not_a_todo() {
        assert!(parse_todo_line("This is just text").is_none());
        assert!(parse_todo_line("- Regular list item").is_none());
    }

    #[test]
    fn test_extract_todos_multiple() {
        // With the new status logic:
        // - First non-completed becomes InProgress
        // - Remaining non-completed stay as Pending
        let text = r#"
<todo>
- [ ] First task
- [x] Second task completed
- [/] Third task in progress
- [ ] Fourth task
</todo>
"#;
        let todos = extract_todos(text);
        assert_eq!(todos.len(), 4);
        // First non-completed becomes InProgress
        assert_eq!(todos[0].status, TodoStatus::InProgress);
        // Completed stays Done
        assert_eq!(todos[1].status, TodoStatus::Done);
        // Third is after first non-completed, becomes Pending
        assert_eq!(todos[2].status, TodoStatus::Pending);
        // Fourth is also Pending
        assert_eq!(todos[3].status, TodoStatus::Pending);
    }

    #[test]
    fn test_extract_todos_empty_block() {
        let text = "<todo>\n\n</todo>";
        let todos = extract_todos(text);
        assert!(todos.is_empty());
    }

    #[test]
    fn test_extract_todos_with_whitespace() {
        let text = "<todo>\n  - [ ] Task with leading whitespace  \n</todo>";
        let todos = extract_todos(text);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].text, "Task with leading whitespace");
        // Single non-completed item becomes InProgress
        assert_eq!(todos[0].status, TodoStatus::InProgress);
    }
}
