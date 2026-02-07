//! Type Definitions Module
//!
//! This module contains all type definitions used throughout the TUI application.
//! Types are organized here for better maintainability and code organization.

use crate::services::message::Message;
use ratatui::text::Line;
use stakpak_shared::models::integrations::openai::{ContentPart, ToolCallResult};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

// Type alias to reduce complexity - now stores processed lines for better performance
pub type MessageLinesCache = (Vec<Message>, usize, Vec<Line<'static>>);

/// Cached rendered lines for a single message.
/// Uses Arc to avoid expensive cloning when returning cached lines.
#[derive(Clone, Debug)]
pub struct RenderedMessageCache {
    /// Hash of the message content for change detection
    pub content_hash: u64,
    /// The rendered lines for this message (shared via Arc to avoid cloning)
    pub rendered_lines: Arc<Vec<Line<'static>>>,
    /// Width the message was rendered at
    pub width: usize,
}

/// Per-message cache for efficient incremental rendering.
/// Only re-renders messages that have actually changed.
pub type PerMessageCache = HashMap<Uuid, RenderedMessageCache>;

/// Cache for the currently visible lines on screen.
/// This avoids re-slicing and cloning on every frame when only scroll position changes.
#[derive(Clone, Debug)]
pub struct VisibleLinesCache {
    /// The scroll position these lines were computed for
    pub scroll: usize,
    /// The width these lines were computed for
    pub width: usize,
    /// The height (number of lines) requested
    pub height: usize,
    /// The visible lines (Arc to avoid cloning on every frame)
    pub lines: Arc<Vec<Line<'static>>>,
    /// Generation counter from assembled cache (to detect when source changed)
    pub source_generation: u64,
}

/// Performance metrics for render operations (for benchmarking)
#[derive(Debug, Default, Clone)]
pub struct RenderMetrics {
    /// Total time spent rendering in the last render cycle (microseconds)
    pub last_render_time_us: u64,
    /// Number of messages that hit the cache
    pub cache_hits: usize,
    /// Number of messages that missed the cache and required re-rendering
    pub cache_misses: usize,
    /// Total number of lines rendered
    pub total_lines: usize,
    /// Rolling average render time (microseconds)
    pub avg_render_time_us: u64,
    /// Number of render cycles tracked for average
    render_count: u64,
}

impl RenderMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new render cycle's metrics
    pub fn record_render(
        &mut self,
        render_time_us: u64,
        cache_hits: usize,
        cache_misses: usize,
        total_lines: usize,
    ) {
        self.last_render_time_us = render_time_us;
        self.cache_hits = cache_hits;
        self.cache_misses = cache_misses;
        self.total_lines = total_lines;

        // Update rolling average
        self.render_count += 1;
        if self.render_count == 1 {
            self.avg_render_time_us = render_time_us;
        } else {
            // Exponential moving average with alpha = 0.1
            self.avg_render_time_us = (self.avg_render_time_us * 9 + render_time_us) / 10;
        }
    }

    /// Reset metrics (useful for benchmarking specific scenarios)
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

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

#[derive(Debug, Clone)]
pub struct AttachedImage {
    pub placeholder: String,
    pub path: PathBuf,
    pub filename: String,
    pub dimensions: (u32, u32),
    pub start_pos: usize,
    pub end_pos: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingUserMessage {
    pub final_input: String,
    pub shell_tool_calls: Option<Vec<ToolCallResult>>,
    pub image_parts: Vec<ContentPart>,
    pub user_message_text: String,
}

impl PendingUserMessage {
    pub fn new(
        final_input: String,
        shell_tool_calls: Option<Vec<ToolCallResult>>,
        image_parts: Vec<ContentPart>,
        user_message_text: String,
    ) -> Self {
        Self {
            final_input,
            shell_tool_calls,
            image_parts,
            user_message_text,
        }
    }

    pub fn merge_from(&mut self, other: PendingUserMessage) {
        fn append_with_separator(target: &mut String, value: &str) {
            if value.is_empty() {
                return;
            }
            if !target.is_empty() {
                target.push_str("\n\n");
            }
            target.push_str(value);
        }

        append_with_separator(&mut self.final_input, &other.final_input);
        append_with_separator(&mut self.user_message_text, &other.user_message_text);

        self.image_parts.extend(other.image_parts);

        match (&mut self.shell_tool_calls, other.shell_tool_calls) {
            (Some(existing), Some(mut incoming)) => existing.append(&mut incoming),
            (None, Some(incoming)) => self.shell_tool_calls = Some(incoming),
            _ => {}
        }
    }
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

/// Mode for the unified shortcuts/commands/sessions popup
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ShortcutsPopupMode {
    #[default]
    Commands,
    Shortcuts,
    Sessions,
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

#[cfg(test)]
mod tests {
    use super::*;
    use stakpak_shared::models::integrations::openai::{
        FunctionCall, ToolCall, ToolCallResultStatus,
    };

    fn tool_result(id: &str) -> ToolCallResult {
        ToolCallResult {
            call: ToolCall {
                id: id.to_string(),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: "run_command".to_string(),
                    arguments: "{}".to_string(),
                },
                metadata: None,
            },
            result: format!("result-{id}"),
            status: ToolCallResultStatus::Success,
        }
    }

    #[test]
    fn pending_user_message_merge_combines_all_parts() {
        let mut first = PendingUserMessage::new(
            "first".to_string(),
            Some(vec![tool_result("t1")]),
            vec![ContentPart {
                r#type: "text".to_string(),
                text: Some("img-1".to_string()),
                image_url: None,
            }],
            "first".to_string(),
        );

        let second = PendingUserMessage::new(
            "second".to_string(),
            Some(vec![tool_result("t2")]),
            vec![ContentPart {
                r#type: "text".to_string(),
                text: Some("img-2".to_string()),
                image_url: None,
            }],
            "second".to_string(),
        );

        first.merge_from(second);

        assert_eq!(first.final_input, "first\n\nsecond");
        assert_eq!(first.user_message_text, "first\n\nsecond");
        assert_eq!(first.image_parts.len(), 2);
        assert_eq!(
            first
                .shell_tool_calls
                .as_ref()
                .map(std::vec::Vec::len)
                .unwrap_or_default(),
            2
        );
    }

    #[test]
    fn pending_user_message_merge_skips_empty_text_with_no_extra_separator() {
        let mut first = PendingUserMessage::new("".to_string(), None, Vec::new(), "".to_string());

        let second = PendingUserMessage::new(
            "second".to_string(),
            None,
            vec![ContentPart {
                r#type: "text".to_string(),
                text: Some("img-2".to_string()),
                image_url: None,
            }],
            "second".to_string(),
        );

        first.merge_from(second);

        assert_eq!(first.final_input, "second");
        assert_eq!(first.user_message_text, "second");
        assert_eq!(first.image_parts.len(), 1);
    }

    #[test]
    fn pending_user_message_merge_adopts_incoming_tool_calls_when_initially_none() {
        let mut first =
            PendingUserMessage::new("first".to_string(), None, Vec::new(), "first".to_string());

        let second = PendingUserMessage::new(
            "second".to_string(),
            Some(vec![tool_result("t2")]),
            Vec::new(),
            "second".to_string(),
        );

        first.merge_from(second);

        assert_eq!(
            first
                .shell_tool_calls
                .as_ref()
                .map(std::vec::Vec::len)
                .unwrap_or_default(),
            1
        );
    }
}
