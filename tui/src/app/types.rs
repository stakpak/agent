//! Type Definitions Module
//!
//! This module contains all type definitions used throughout the TUI application.
//! Types are organized here for better maintainability and code organization.

use crate::services::message::Message;
use ratatui::text::Line;

// Type alias to reduce complexity - now stores processed lines for better performance
pub type MessageLinesCache = (Vec<Message>, usize, Vec<Line<'static>>);

/// Async file_search result struct
pub struct FileSearchResult {
    pub filtered_helpers: Vec<HelperCommand>,
    pub filtered_files: Vec<String>,
    pub cursor_position: usize,
    pub input: String,
}

#[derive(Debug, Clone)]
pub struct HelperCommand {
    pub command: &'static str,
    pub description: &'static str,
}

#[derive(Debug)]
pub struct SessionInfo {
    pub title: String,
    pub id: String,
    pub updated_at: String,
    pub checkpoints: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub enum LoadingType {
    Llm,
    Sessions,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LoadingOperation {
    LlmRequest,
    ToolExecution,
    SessionsList,
    StreamProcessing,
    LocalContext,
    Rulebooks,
    CheckpointResume,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolCallStatus {
    Approved,
    Rejected,
    Executed,
    Skipped,
    Pending,
}

#[derive(Debug)]
pub struct LoadingStateManager {
    active_operations: std::collections::HashSet<LoadingOperation>,
}

impl Default for LoadingStateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadingStateManager {
    pub fn new() -> Self {
        Self {
            active_operations: std::collections::HashSet::new(),
        }
    }

    pub fn start_operation(&mut self, operation: LoadingOperation) {
        self.active_operations.insert(operation);
    }

    pub fn end_operation(&mut self, operation: LoadingOperation) {
        self.active_operations.remove(&operation);
    }

    pub fn is_loading(&self) -> bool {
        !self.active_operations.is_empty()
    }

    pub fn get_loading_type(&self) -> LoadingType {
        if self
            .active_operations
            .contains(&LoadingOperation::SessionsList)
        {
            LoadingType::Sessions
        } else {
            LoadingType::Llm
        }
    }

    pub fn clear_all(&mut self) {
        self.active_operations.clear();
    }
}
