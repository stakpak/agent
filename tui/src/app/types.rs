//! Type Definitions Module
//!
//! This module contains all type definitions used throughout the TUI application.
//! Types are organized here for better maintainability and code organization.

use crate::services::message::Message;
use ratatui::text::Line;
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

/// Mode for the unified shortcuts/commands popup
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ShortcutsPopupMode {
    #[default]
    Commands,
    Shortcuts,
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
