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
    /// Index of the user message that triggered this edit (for selective revert)
    pub user_message_index: Option<usize>,
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
            user_message_index: None,
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

    pub fn with_user_message_index(mut self, index: usize) -> Self {
        self.user_message_index = Some(index);
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
    /// Path to backup of original file content before any modifications
    /// Used for reliable revert when replaying edits fails
    pub original_backup_path: Option<String>,
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
            original_backup_path: None,
        }
    }

    /// Set the original backup path (for reverting to original state)
    pub fn with_original_backup(mut self, path: String) -> Self {
        self.original_backup_path = Some(path);
        self
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
    /// For created files, reads actual file content for accurate count
    pub fn total_lines_added(&self) -> usize {
        // For created files, the "lines added" is the current file content
        // Read from disk for accurate count (handles reverts, manual edits, etc.)
        if self.state == FileState::Created
            && let Ok(content) = fs::read_to_string(&self.path)
        {
            return content.lines().count();
        }
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

    /// Revert all file changes made at or after the given user message index.
    /// This is used for the "revert to message" feature.
    /// The target_index message itself and all messages after it will be reverted.
    ///
    /// Returns (files_reverted, files_deleted) counts on success.
    pub fn revert_from_user_message(
        &mut self,
        target_index: usize,
        _session_id: &str,
    ) -> Result<(usize, usize), String> {
        let mut files_reverted = 0;
        let mut files_deleted = 0;
        let mut files_to_remove: Vec<String> = Vec::new();

        // Collect file paths to process (avoid borrow issues)
        let paths: Vec<String> = self.files.keys().cloned().collect();

        for path in paths {
            let file = match self.files.get(&path) {
                Some(f) => f.clone(),
                None => continue,
            };

            // Find the first edit index for this file (to determine if file was created at/after target)
            let first_edit_index = file
                .edits
                .first()
                .and_then(|e| e.user_message_index)
                .unwrap_or(0);

            // Collect edits that happened at or after the target message
            let edits_to_revert: Vec<&FileEdit> = file
                .edits
                .iter()
                .filter(|e| e.user_message_index.unwrap_or(0) >= target_index)
                .collect();

            if edits_to_revert.is_empty() {
                // No edits at or after target, nothing to revert for this file
                continue;
            }

            // Case 1: File was created at/after target - delete it entirely
            if first_edit_index >= target_index && file.state == FileState::Created {
                if Path::new(&path).exists() {
                    if let Err(e) = fs::remove_file(&path) {
                        log::warn!("Failed to delete created file {}: {}", path, e);
                    } else {
                        files_deleted += 1;
                    }
                }
                files_to_remove.push(path.clone());
                continue;
            }

            // Case 2: File was removed at/after target - restore it
            if file.state == FileState::Removed || file.state == FileState::Deleted {
                // Check if the removal happened at or after target
                let removal_at_or_after_target = file.edits.iter().any(|e| {
                    e.summary == "File removed" && e.user_message_index.unwrap_or(0) >= target_index
                });

                if removal_at_or_after_target {
                    if let Some(backup_path) = &file.backup_path {
                        if let Err(e) = fs::copy(backup_path, &path) {
                            log::warn!("Failed to restore removed file {}: {}", path, e);
                        } else {
                            // Count lines in restored file for accurate stats
                            let current_line_count = fs::read_to_string(&path)
                                .map(|c| c.lines().count())
                                .unwrap_or(0);

                            // Update file state
                            if let Some(tracked) = self.files.get_mut(&path) {
                                // Remove edits at or after target (keep only edits before target)
                                tracked
                                    .edits
                                    .retain(|e| e.user_message_index.unwrap_or(0) < target_index);

                                if tracked.edits.is_empty() {
                                    // No edits remaining - remove from changeset entirely
                                    files_to_remove.push(path.clone());
                                } else {
                                    // Get the earliest user_message_index from remaining edits
                                    let earliest_idx =
                                        tracked.edits.first().and_then(|e| e.user_message_index);

                                    // Consolidate remaining edits with accurate stats
                                    let mut consolidated_edit =
                                        FileEdit::new("Modified".to_string())
                                            .with_stats(current_line_count, 0);
                                    if let Some(idx) = earliest_idx {
                                        consolidated_edit =
                                            consolidated_edit.with_user_message_index(idx);
                                    }
                                    tracked.edits = vec![consolidated_edit];
                                    tracked.state = FileState::Modified;
                                }
                            }
                            files_reverted += 1;
                        }
                    }
                    continue;
                }
            }

            // Case 3: File was modified at/after target - restore to state before target
            // Try original backup first (most reliable), then replay edits in reverse
            if let Some(original_backup) = &file.original_backup_path {
                // Check if we need to restore to original (all edits are at or after target)
                let all_edits_at_or_after_target = file
                    .edits
                    .iter()
                    .all(|e| e.user_message_index.unwrap_or(0) >= target_index);

                if all_edits_at_or_after_target && Path::new(original_backup).exists() {
                    // Restore from original backup
                    if let Err(e) = fs::copy(original_backup, &path) {
                        log::warn!(
                            "Failed to restore from original backup {}: {}",
                            original_backup,
                            e
                        );
                    } else {
                        files_to_remove.push(path.clone());
                        files_reverted += 1;
                        continue;
                    }
                }
            }

            // Replay edits in reverse for edits at or after target
            if Path::new(&path).exists() {
                match fs::read_to_string(&path) {
                    Ok(mut content) => {
                        // Get edits at or after target in reverse order
                        let mut edits_to_reverse: Vec<&FileEdit> = file
                            .edits
                            .iter()
                            .filter(|e| e.user_message_index.unwrap_or(0) >= target_index)
                            .collect();
                        edits_to_reverse.reverse();

                        let mut success = true;
                        for edit in edits_to_reverse {
                            if let Some(tool_call) = &edit.tool_call {
                                match Self::apply_reverse_edit(&content, tool_call) {
                                    Ok(new_content) => content = new_content,
                                    Err(e) => {
                                        log::warn!("Failed to reverse edit for {}: {}", path, e);
                                        success = false;
                                        break;
                                    }
                                }
                            }
                        }

                        if success {
                            // Count lines in the reverted content for accurate stats
                            let current_line_count = content.lines().count();

                            if let Err(e) = fs::write(&path, &content) {
                                log::warn!("Failed to write reverted content to {}: {}", path, e);
                            } else {
                                // Update tracked file state - keep only edits before target
                                if let Some(tracked) = self.files.get_mut(&path) {
                                    tracked.edits.retain(|e| {
                                        e.user_message_index.unwrap_or(0) < target_index
                                    });
                                    if tracked.edits.is_empty() {
                                        // No edits remaining - remove from changeset entirely
                                        files_to_remove.push(path.clone());
                                    } else {
                                        // Get the earliest user_message_index from remaining edits
                                        let earliest_idx = tracked
                                            .edits
                                            .first()
                                            .and_then(|e| e.user_message_index);

                                        // Consolidate remaining edits into a single edit with accurate stats
                                        // The old individual edit stats are no longer accurate after revert
                                        let mut consolidated_edit =
                                            FileEdit::new("Modified".to_string())
                                                .with_stats(current_line_count, 0);
                                        if let Some(idx) = earliest_idx {
                                            consolidated_edit =
                                                consolidated_edit.with_user_message_index(idx);
                                        }
                                        tracked.edits = vec![consolidated_edit];
                                    }
                                    // Keep the existing state (Modified) - don't change to Reverted
                                }
                                files_reverted += 1;
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("Failed to read file {} for revert: {}", path, e);
                    }
                }
            }
        }

        // Remove files from changeset that were fully reverted
        for path in files_to_remove {
            self.files.remove(&path);
            self.order.retain(|p| p != &path);
        }

        // Adjust selected_index if needed
        if self.selected_index >= self.order.len() {
            self.selected_index = self.order.len().saturating_sub(1);
        }

        Ok((files_reverted, files_deleted))
    }

    /// Set the original backup path for a tracked file
    pub fn set_original_backup(&mut self, path: &str, backup_path: String) {
        if let Some(file) = self.files.get_mut(path)
            && file.original_backup_path.is_none()
        {
            file.original_backup_path = Some(backup_path);
        }
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
    Plan,
    Context,
    Billing,
    Tasks,
    Changeset,
}

impl SidePanelSection {
    pub fn title(&self) -> &'static str {
        match self {
            SidePanelSection::Plan => "Plan",
            SidePanelSection::Context => "Context",
            SidePanelSection::Billing => "Billing",
            SidePanelSection::Tasks => "Tasks",
            SidePanelSection::Changeset => "Changeset",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            SidePanelSection::Plan => SidePanelSection::Context,
            SidePanelSection::Context => SidePanelSection::Billing,
            SidePanelSection::Billing => SidePanelSection::Tasks,
            SidePanelSection::Tasks => SidePanelSection::Changeset,
            SidePanelSection::Changeset => SidePanelSection::Plan,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            SidePanelSection::Plan => SidePanelSection::Changeset,
            SidePanelSection::Context => SidePanelSection::Plan,
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
