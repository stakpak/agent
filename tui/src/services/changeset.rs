//! Changeset tracking for the Session Side Panel
//!
//! This module tracks all file modifications made during a session, including:
//! - Files created, modified, or deleted
//! - Edit history with timestamps and diff previews
//! - Backup paths for revert functionality

use chrono::{DateTime, Utc};
use stakpak_shared::models::integrations::openai::ToolCall;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// State of a tracked file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    Created,  // File created during session
    Modified, // File was modified
    Removed,  // File removed by remove tool (can restore)
    Reverted, // File was reverted to original
    Deleted,  // Created file was then deleted
}

impl FileState {
    pub fn label(&self) -> &'static str {
        match self {
            FileState::Created => "[+]",
            FileState::Modified => "[~]",
            FileState::Removed => "[-]",
            FileState::Reverted => "[R]",
            FileState::Deleted => "[X]",
        }
    }
}

/// A single edit to a file
#[derive(Debug, Clone)]
pub struct FileEdit {
    /// When the edit occurred
    pub timestamp: DateTime<Utc>,
    /// Brief description of the edit (e.g., "Added login function")
    pub summary: String,
    /// Number of lines added
    pub lines_added: usize,
    /// Number of lines removed
    pub lines_removed: usize,
    /// Path to backup for revert (from FileBackupManager)
    pub backup_path: Option<String>,
    /// First few lines of the diff for preview
    pub diff_preview: Option<String>,
    /// Original tool call for reverse replay during revert
    pub tool_call: Option<ToolCall>,
}

impl FileEdit {
    pub fn new(summary: String) -> Self {
        Self {
            timestamp: Utc::now(),
            summary,
            lines_added: 0,
            lines_removed: 0,
            backup_path: None,
            diff_preview: None,
            tool_call: None,
        }
    }

    pub fn with_stats(mut self, added: usize, removed: usize) -> Self {
        self.lines_added = added;
        self.lines_removed = removed;
        self
    }

    pub fn with_backup(mut self, path: String) -> Self {
        self.backup_path = Some(path);
        self
    }

    pub fn with_diff_preview(mut self, preview: String) -> Self {
        self.diff_preview = Some(preview);
        self
    }

    pub fn with_tool_call(mut self, tool_call: ToolCall) -> Self {
        self.tool_call = Some(tool_call);
        self
    }
}

/// Represents a tracked file in the changeset
#[derive(Debug, Clone)]
pub struct TrackedFile {
    /// Absolute path to the file
    pub path: String,
    /// Edit history (most recent last)
    pub edits: Vec<FileEdit>,
    /// Current state of the file
    pub state: FileState,
    /// Whether this file is expanded in the UI
    pub is_expanded: bool,
    /// Selected edit index when expanded
    pub selected_edit: usize,
    /// Path to backup if the file was removed
    pub backup_path: Option<String>,
}

impl TrackedFile {
    pub fn new(path: String) -> Self {
        Self {
            path,
            edits: Vec::new(),
            state: FileState::Modified, // Default assumption, updated on tracking
            is_expanded: false,
            selected_edit: 0,
            backup_path: None,
        }
    }

    /// Add a new edit to this file
    pub fn add_edit(&mut self, edit: FileEdit) {
        self.edits.push(edit);
        // Keep only the last MAX_EDITS_PER_FILE edits
        const MAX_EDITS_PER_FILE: usize = 10;
        if self.edits.len() > MAX_EDITS_PER_FILE {
            self.edits.remove(0);
        }
    }

    /// Get total lines added across all edits
    pub fn total_lines_added(&self) -> usize {
        self.edits.iter().map(|e| e.lines_added).sum()
    }

    /// Get total lines removed across all edits
    pub fn total_lines_removed(&self) -> usize {
        self.edits.iter().map(|e| e.lines_removed).sum()
    }

    /// Get the display name (file basename)
    pub fn display_name(&self) -> &str {
        std::path::Path::new(&self.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.path)
    }
}

/// The changeset tracks all file modifications in a session
#[derive(Debug, Clone, Default)]
pub struct Changeset {
    /// Map of file path to tracked file
    pub files: HashMap<String, TrackedFile>,
    /// Order of files (first modified first)
    pub order: Vec<String>,
    /// Currently selected file index in the UI
    pub selected_index: usize,
}

impl Changeset {
    pub fn new() -> Self {
        Self::default()
    }

    /// Track a file modification
    pub fn track_file(&mut self, path: &str, edit: FileEdit) {
        // Check if this is a file creation based on tool call
        let is_creation = edit.tool_call.as_ref().is_some_and(|tc| {
            let name = tc.function.name.as_str();
            name == "stakpak__create"
                || name == "stakpak__write"
                || name == "stakpak__create_file"
                || name == "create"
                || name == "create_file"
        });

        if let Some(file) = self.files.get_mut(path) {
            file.add_edit(edit);
            // If it was removed/deleted but now we are writing to it, it's modified (or created if it was deleted)
            if file.state == FileState::Removed || file.state == FileState::Deleted {
                file.state = if is_creation {
                    FileState::Created
                } else {
                    FileState::Modified
                };
            }
        } else {
            let mut tracked = TrackedFile::new(path.to_string());
            tracked.state = if is_creation {
                FileState::Created
            } else {
                FileState::Modified
            };
            tracked.add_edit(edit);
            self.files.insert(path.to_string(), tracked);
            if !self.order.contains(&path.to_string()) {
                self.order.push(path.to_string());
            }
        }
    }

    /// Mark a file as removed (deleted by tool)
    pub fn mark_removed(&mut self, path: &str, backup_path: Option<String>) {
        if let Some(file) = self.files.get_mut(path) {
            // If file was Created or Deleted, now it becomes Deleted
            // If file was Modified or Removed, it becomes Removed
            if file.state == FileState::Created || file.state == FileState::Deleted {
                file.state = FileState::Deleted;
            } else {
                file.state = FileState::Removed;
            }
            file.backup_path = backup_path.clone(); // Store main backup path for restore

            let mut edit = FileEdit::new("File removed".to_string());
            if let Some(bp) = backup_path {
                edit = edit.with_backup(bp);
            }
            file.add_edit(edit);
        } else {
            let mut tracked = TrackedFile::new(path.to_string());
            tracked.state = FileState::Removed;
            tracked.backup_path = backup_path.clone();

            let mut edit = FileEdit::new("File removed".to_string());
            if let Some(bp) = backup_path {
                edit = edit.with_backup(bp);
            }
            tracked.add_edit(edit);
            self.files.insert(path.to_string(), tracked);
            if !self.order.contains(&path.to_string()) {
                self.order.push(path.to_string());
            }
        }
    }

    /// Get the number of tracked files (excluding reverted/deleted files that shouldn't show)
    pub fn file_count(&self) -> usize {
        self.files.values().count()
    }

    /// Revert a single file to its original state
    pub fn revert_file(tracked_file: &TrackedFile, session_id: &str) -> Result<String, String> {
        match tracked_file.state {
            FileState::Removed | FileState::Deleted => {
                Self::restore_removed_file(tracked_file, session_id)
            }
            FileState::Created => Self::delete_created_file(&tracked_file.path),
            FileState::Modified | FileState::Reverted => Self::replay_edits_reverse(tracked_file),
        }
    }

    /// Restore a removed file from backup
    pub fn restore_removed_file(
        tracked_file: &TrackedFile,
        _session_id: &str,
    ) -> Result<String, String> {
        // Find the backup path from the file state
        let backup_path = tracked_file
            .backup_path
            .as_ref()
            .ok_or_else(|| "No backup path found for removed file".to_string())?;

        // Copy from backup to original location
        fs::copy(backup_path, &tracked_file.path)
            .map_err(|e| format!("Failed to restore file: {}", e))?;

        Ok(format!(
            "Restored {} from backup",
            tracked_file.display_name()
        ))
    }

    /// Delete a file that was created during the session
    fn delete_created_file(path: &str) -> Result<String, String> {
        if !Path::new(path).exists() {
            return Ok(format!("File {} already deleted", path));
        }

        fs::remove_file(path).map_err(|e| format!("Failed to delete file: {}", e))?;

        Ok(format!("Deleted created file {}", path))
    }

    /// Replay edits in reverse order to restore original state
    fn replay_edits_reverse(tracked_file: &TrackedFile) -> Result<String, String> {
        let mut content = fs::read_to_string(&tracked_file.path)
            .map_err(|e| format!("Failed to read file: {}", e))?;

        // Replay edits in reverse order
        for edit in tracked_file.edits.iter().rev() {
            if let Some(tool_call) = &edit.tool_call {
                content = Self::apply_reverse_edit(&content, tool_call)?;
            }
        }

        // Write the reverted content back
        fs::write(&tracked_file.path, content)
            .map_err(|e| format!("Failed to write file: {}", e))?;

        Ok(format!(
            "Reverted {} ({} edits)",
            tracked_file.display_name(),
            tracked_file.edits.len()
        ))
    }

    /// Apply a single edit in reverse (replace new_str with old_str)
    fn apply_reverse_edit(content: &str, tool_call: &ToolCall) -> Result<String, String> {
        // Only handle str_replace tool calls
        if tool_call.function.name != "stakpak__str_replace" {
            return Ok(content.to_string());
        }

        // Parse the tool call arguments
        let args: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
            .map_err(|e| format!("Failed to parse tool call arguments: {}", e))?;

        let old_str = args["old_str"]
            .as_str()
            .ok_or_else(|| "Missing old_str in tool call".to_string())?;
        let new_str = args["new_str"]
            .as_str()
            .ok_or_else(|| "Missing new_str in tool call".to_string())?;

        // Replace new_str with old_str (reverse the edit)
        let reverted = content.replace(new_str, old_str);

        Ok(reverted)
    }

    pub fn files_in_order(&self) -> Vec<&TrackedFile> {
        self.order
            .iter()
            .filter_map(|path| self.files.get(path))
            .collect()
    }

    /// Get currently selected file
    pub fn selected_file(&self) -> Option<&TrackedFile> {
        self.order
            .get(self.selected_index)
            .and_then(|path| self.files.get(path))
    }

    /// Get currently selected file mutably
    pub fn selected_file_mut(&mut self) -> Option<&mut TrackedFile> {
        if let Some(path) = self.order.get(self.selected_index).cloned() {
            self.files.get_mut(&path)
        } else {
            None
        }
    }

    /// Move selection up
    pub fn select_prev(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.selected_index < self.order.len().saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    /// Toggle expansion of selected file
    pub fn toggle_selected(&mut self) {
        if let Some(file) = self.selected_file_mut() {
            file.is_expanded = !file.is_expanded;
        }
    }

    /// Clear all tracked files
    pub fn clear(&mut self) {
        self.files.clear();
        self.order.clear();
        self.selected_index = 0;
    }
}

/// Status of a todo item
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
}

impl TodoStatus {
    pub fn symbol(&self) -> &'static str {
        match self {
            TodoStatus::Pending => "[ ]",
            TodoStatus::InProgress => "[/]",
            TodoStatus::Done => "[x]",
        }
    }
}

/// Type of task item for visual hierarchy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoItemType {
    /// Top-level card/task
    Card,
    /// Checklist item under a card
    ChecklistItem,
    /// Collapsed indicator showing count of hidden items
    CollapsedIndicator,
}

/// A task item for the Tasks section (from agent-board cards)
#[derive(Debug, Clone)]
pub struct TodoItem {
    pub text: String,
    pub status: TodoStatus,
    pub item_type: TodoItemType,
}

impl TodoItem {
    pub fn new(text: String) -> Self {
        Self {
            text,
            status: TodoStatus::Pending,
            item_type: TodoItemType::Card,
        }
    }

    pub fn checklist_item(text: String) -> Self {
        Self {
            text,
            status: TodoStatus::Pending,
            item_type: TodoItemType::ChecklistItem,
        }
    }

    pub fn with_status(mut self, status: TodoStatus) -> Self {
        self.status = status;
        self
    }

    pub fn into_collapsed_indicator(mut self) -> Self {
        self.item_type = TodoItemType::CollapsedIndicator;
        self
    }
}

/// Side panel section identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidePanelSection {
    Context,
    Billing,
    Tasks,
    Changeset,
}

impl SidePanelSection {
    pub fn title(&self) -> &'static str {
        match self {
            SidePanelSection::Context => "Context",
            SidePanelSection::Billing => "Billing",
            SidePanelSection::Tasks => "Tasks",
            SidePanelSection::Changeset => "Changeset",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            SidePanelSection::Context => SidePanelSection::Billing,
            SidePanelSection::Billing => SidePanelSection::Tasks,
            SidePanelSection::Tasks => SidePanelSection::Changeset,
            SidePanelSection::Changeset => SidePanelSection::Context,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            SidePanelSection::Context => SidePanelSection::Changeset,
            SidePanelSection::Billing => SidePanelSection::Context,
            SidePanelSection::Tasks => SidePanelSection::Billing,
            SidePanelSection::Changeset => SidePanelSection::Tasks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_changeset_track_file() {
        let mut changeset = Changeset::new();

        changeset.track_file(
            "/path/to/file.rs",
            FileEdit::new("Initial creation".to_string())
                .with_stats(10, 0)
                .with_tool_call(ToolCall {
                    id: "call_1".to_string(),
                    r#type: "function".to_string(),
                    function: stakpak_shared::models::integrations::openai::FunctionCall {
                        name: "stakpak__create".to_string(),
                        arguments: "{}".to_string(),
                    },
                    metadata: None,
                }),
        );

        assert_eq!(changeset.file_count(), 1);
        let file = changeset.files.get("/path/to/file.rs").unwrap();
        assert_eq!(file.edits.len(), 1);
        assert_eq!(file.total_lines_added(), 10);
        assert_eq!(file.state, FileState::Created);
    }

    #[test]
    fn test_changeset_multiple_edits() {
        let mut changeset = Changeset::new();

        changeset.track_file(
            "/path/to/file.rs",
            FileEdit::new("Initial".to_string()).with_stats(10, 0),
        );
        changeset.track_file(
            "/path/to/file.rs",
            FileEdit::new("Update".to_string()).with_stats(5, 3),
        );

        let file = changeset.files.get("/path/to/file.rs").unwrap();
        assert_eq!(file.edits.len(), 2);
        assert_eq!(file.total_lines_added(), 15);
        assert_eq!(file.total_lines_removed(), 3);
        assert_eq!(file.state, FileState::Modified);
    }
}
