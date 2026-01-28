//! Reusable text selection module for any widget/area.
//!
//! This module provides a generic selection system that can be used in:
//! - TextArea (input field)
//! - Message area
//! - Future popups or other widgets
//!
//! # Usage
//!
//! ```rust,ignore
//! use widget_selection::WidgetSelection;
//!
//! // In your widget struct:
//! struct MyWidget {
//!     selection: WidgetSelection,
//!     // ...
//! }
//!
//! // Start selection on mouse down:
//! widget.selection.start(position);
//!
//! // Update during drag:
//! widget.selection.update(position);
//!
//! // End and get selected text:
//! if let Some(text) = widget.selection.end_and_get_text(&content) {
//!     copy_to_clipboard(&text);
//! }
//! ```

use ratatui::style::{Color, Style};

/// Generic selection state for any widget
#[derive(Debug, Clone, Copy, Default)]
pub struct WidgetSelection {
    /// Start position of selection (can be byte offset, line index, etc.)
    pub start: Option<usize>,
    /// End position of selection
    pub end: Option<usize>,
    /// Whether selection is currently active (mouse is being dragged)
    pub active: bool,
}

impl WidgetSelection {
    /// Create a new empty selection
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a new selection at the given position
    pub fn start(&mut self, pos: usize) {
        self.start = Some(pos);
        self.end = Some(pos);
        self.active = true;
    }

    /// Update the selection end position (during drag)
    pub fn update(&mut self, pos: usize) {
        if self.active {
            self.end = Some(pos);
        }
    }

    /// End the selection (stop dragging) but keep the selection visible
    /// Returns true if there was an actual selection (not just a click)
    pub fn end(&mut self) -> bool {
        let had_selection = self.has_selection();
        self.active = false;
        had_selection
    }

    /// End the selection and clear it, returning the normalized bounds if any
    pub fn end_and_clear(&mut self) -> Option<(usize, usize)> {
        let bounds = self.normalized();
        self.clear();
        bounds
    }

    /// End the selection and get the selected text from the provided content
    pub fn end_and_get_text<'a>(&mut self, text: &'a str) -> Option<&'a str> {
        let result = self.get_text(text);
        self.clear();
        result
    }

    /// Get normalized selection bounds (start always <= end)
    pub fn normalized(&self) -> Option<(usize, usize)> {
        match (self.start, self.end) {
            (Some(s), Some(e)) if s != e => Some((s.min(e), s.max(e))),
            _ => None,
        }
    }

    /// Get the selected text from a string (without ending/clearing selection)
    pub fn get_text<'a>(&self, text: &'a str) -> Option<&'a str> {
        let (start, end) = self.normalized()?;
        if end <= text.len() {
            Some(&text[start..end])
        } else {
            None
        }
    }

    /// Check if there's a valid selection (not just a click)
    pub fn has_selection(&self) -> bool {
        self.normalized().is_some()
    }

    /// Check if selection is currently being made (mouse is dragging)
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Check if a position is within the selection
    pub fn contains(&self, pos: usize) -> bool {
        if let Some((start, end)) = self.normalized() {
            pos >= start && pos < end
        } else {
            false
        }
    }

    /// Clear the selection completely
    pub fn clear(&mut self) {
        self.start = None;
        self.end = None;
        self.active = false;
    }

    /// Get the selection style for highlighting
    pub fn highlight_style() -> Style {
        Style::default().fg(Color::White).bg(Color::Blue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_lifecycle() {
        let mut sel = WidgetSelection::new();

        // Initially empty
        assert!(!sel.has_selection());
        assert!(!sel.is_active());

        // Start selection
        sel.start(5);
        assert!(sel.is_active());
        assert!(!sel.has_selection()); // Same start/end = no selection

        // Update selection
        sel.update(10);
        assert!(sel.is_active());
        assert!(sel.has_selection());
        assert_eq!(sel.normalized(), Some((5, 10)));

        // End selection
        assert!(sel.end());
        assert!(!sel.is_active());
        assert!(sel.has_selection()); // Selection still visible

        // Clear
        sel.clear();
        assert!(!sel.has_selection());
    }

    #[test]
    fn test_get_text() {
        let mut sel = WidgetSelection::new();
        let text = "Hello, World!";

        sel.start(0);
        sel.update(5);

        assert_eq!(sel.get_text(text), Some("Hello"));
    }

    #[test]
    fn test_reversed_selection() {
        let mut sel = WidgetSelection::new();

        // Select backwards (end before start)
        sel.start(10);
        sel.update(5);

        // Should normalize
        assert_eq!(sel.normalized(), Some((5, 10)));
    }

    #[test]
    fn test_end_and_clear() {
        let mut sel = WidgetSelection::new();
        let text = "Hello, World!";

        sel.start(7);
        sel.update(12);

        let result = sel.end_and_get_text(text);
        assert_eq!(result, Some("World"));
        assert!(!sel.has_selection()); // Should be cleared
    }
}
