//! Inline Approval Bar Component
//!
//! A compact approval bar that sits above the input area showing tool call tabs.
//! The actual tool call details are rendered in the messages area based on selection.
//!
//! Design:
//! ```text
//! ┌─ Approval Required ──────────────────────────────────────────────────────┐
//! │ ✓ Run Command   ✓ Create   ✓ Str Replace             space · ←→ · enter │
//! └──────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! - All tools start as Approved (✓) by default
//! - Space toggles between Approved (✓) and Rejected (✗)
//! - Left/Right arrows navigate between tabs
//! - Enter confirms all decisions and executes

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use stakpak_shared::models::integrations::openai::ToolCall;
use stakpak_shared::utils::strip_tool_name;

/// Approval status for a tool call
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ApprovalStatus {
    Approved,
    Rejected,
}

/// A single action in the approval queue
#[derive(Debug, Clone)]
pub struct ApprovalAction {
    pub tool_call: ToolCall,
    pub status: ApprovalStatus,
    /// Display label (e.g., "Run Command", "Create", "Str Replace")
    pub label: String,
}

impl ApprovalAction {
    pub fn new(tool_call: ToolCall) -> Self {
        let tool_name = strip_tool_name(&tool_call.function.name);

        let label = if tool_name == "dynamic_subagent_task" {
            // Parse description and sandbox flag from arguments for a meaningful label
            let args =
                serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments).ok();
            let desc = args
                .as_ref()
                .and_then(|a| a.get("description").and_then(|v| v.as_str()))
                .unwrap_or("Subagent");
            let is_sandbox = args
                .as_ref()
                .and_then(|a| a.get("enable_sandbox").and_then(|v| v.as_bool()))
                .unwrap_or(false);
            if is_sandbox {
                format!("{} [sandboxed]", desc)
            } else {
                desc.to_string()
            }
        } else {
            // Convert snake_case to Title Case (e.g., "get_pak_content" -> "Get Pak Content")
            tool_name
                .split('_')
                .map(|word| {
                    let mut chars = word.chars();
                    match chars.next() {
                        Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };

        Self {
            tool_call,
            // Default to Approved - user can reject with Space
            status: ApprovalStatus::Approved,
            label,
        }
    }

    /// Toggle between Approved and Rejected
    pub fn toggle(&mut self) {
        self.status = match self.status {
            ApprovalStatus::Approved => ApprovalStatus::Rejected,
            ApprovalStatus::Rejected => ApprovalStatus::Approved,
        };
    }
}

/// The inline approval bar state
#[derive(Debug)]
pub struct ApprovalBar {
    /// Queue of actions awaiting approval
    actions: Vec<ApprovalAction>,
    /// Currently selected/focused action index (this controls which tool is shown in messages)
    selected_index: usize,
    /// Whether the bar is visible
    visible: bool,
    /// Whether ESC was pressed once (waiting for confirmation)
    esc_pressed_once: bool,
}

impl Default for ApprovalBar {
    fn default() -> Self {
        Self::new()
    }
}

impl ApprovalBar {
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
            selected_index: 0,
            visible: false,
            esc_pressed_once: false,
        }
    }

    /// Check if ESC was pressed once (waiting for second ESC to confirm rejection)
    pub fn is_esc_pending(&self) -> bool {
        self.esc_pressed_once
    }

    /// Set ESC pending state
    pub fn set_esc_pending(&mut self, pending: bool) {
        self.esc_pressed_once = pending;
    }

    /// Check if the bar is visible (has actions)
    pub fn is_visible(&self) -> bool {
        self.visible && !self.actions.is_empty()
    }

    /// Show the bar
    pub fn show(&mut self) {
        self.visible = true;
    }

    /// Hide the bar
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Add a new tool call to the approval queue
    pub fn add_action(&mut self, tool_call: ToolCall) {
        self.actions.push(ApprovalAction::new(tool_call));
        self.visible = true;
    }

    /// Clear all actions and hide
    pub fn clear(&mut self) {
        self.actions.clear();
        self.selected_index = 0;
        self.visible = false;
        self.esc_pressed_once = false;
    }

    /// Get number of rejected actions
    pub fn rejected_count(&self) -> usize {
        self.actions
            .iter()
            .filter(|a| a.status == ApprovalStatus::Rejected)
            .count()
    }

    /// Get total count of actions
    pub fn total_count(&self) -> usize {
        self.actions.len()
    }

    /// Get all actions
    pub fn actions(&self) -> &[ApprovalAction] {
        &self.actions
    }

    /// Get the currently selected action
    pub fn selected_action(&self) -> Option<&ApprovalAction> {
        self.actions.get(self.selected_index)
    }

    /// Get the currently selected tool call
    pub fn selected_tool_call(&self) -> Option<&ToolCall> {
        self.selected_action().map(|a| &a.tool_call)
    }

    /// Get the currently selected index
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Move selection left (wraps around)
    pub fn select_prev(&mut self) {
        if !self.actions.is_empty() {
            if self.selected_index == 0 {
                self.selected_index = self.actions.len() - 1;
            } else {
                self.selected_index -= 1;
            }
        }
    }

    /// Move selection right (wraps around)
    pub fn select_next(&mut self) {
        if !self.actions.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.actions.len();
        }
    }

    /// Toggle approve/reject for the currently selected action
    pub fn toggle_selected(&mut self) {
        if let Some(action) = self.actions.get_mut(self.selected_index) {
            action.toggle();
        }
    }

    /// Approve all actions (no-op since all start approved, but keeps rejected ones rejected)
    pub fn approve_all(&mut self) {
        // All actions are already approved by default, this is called on Enter
        // We don't change rejected ones back to approved
    }

    /// Reject all actions
    pub fn reject_all(&mut self) {
        for action in &mut self.actions {
            action.status = ApprovalStatus::Rejected;
        }
    }

    /// Get all approved tool calls
    pub fn get_approved(&self) -> Vec<&ToolCall> {
        self.actions
            .iter()
            .filter(|a| a.status == ApprovalStatus::Approved)
            .map(|a| &a.tool_call)
            .collect()
    }

    /// Get all approved tool calls (alias for compatibility with approval_popup)
    pub fn get_approved_tool_calls(&self) -> Vec<&ToolCall> {
        self.get_approved()
    }

    /// Get all rejected tool calls
    pub fn get_rejected(&self) -> Vec<&ToolCall> {
        self.actions
            .iter()
            .filter(|a| a.status == ApprovalStatus::Rejected)
            .map(|a| &a.tool_call)
            .collect()
    }

    /// Get all rejected tool calls (alias for compatibility with approval_popup)
    pub fn get_rejected_tool_calls(&self) -> Vec<&ToolCall> {
        self.get_rejected()
    }

    /// Check if all actions have been resolved (always true now since no Pending state)
    pub fn all_resolved(&self) -> bool {
        !self.actions.is_empty()
    }

    /// Calculate the height needed for rendering
    /// Returns: top border (1) + content lines (with spacing) + empty line (1) + footer (1) + bottom border (1)
    pub fn calculate_height(&self, terminal_width: u16) -> u16 {
        if !self.is_visible() {
            return 0;
        }

        // Calculate how many lines the tabs will wrap to
        let inner_width = terminal_width.saturating_sub(4) as usize; // -2 for borders, -2 for padding
        let tab_width = inner_width.saturating_sub(2); // margins inside content area

        let mut num_lines = 1usize;
        let mut current_width = 0usize;

        for action in &self.actions {
            // Calculate button width: " ✓ Label " with spaces
            let button_text = format!(" ✓ {} ", action.label);
            let button_width = button_text.chars().count();
            let separator_len = if current_width == 0 { 0 } else { 1 }; // " " between buttons
            let needed_width = button_width + separator_len;

            // Check if we need to wrap to next line
            if current_width > 0 && current_width + needed_width > tab_width {
                num_lines += 1;
                current_width = button_width;
            } else {
                current_width += needed_width;
            }
        }

        // Height calculation:
        // - top border: 1
        // - content lines: num_lines (first line) + (num_lines - 1) spacing lines between rows
        // - empty line before footer: 1
        // - footer: 1
        // - bottom border: 1
        // Total: 1 + num_lines + (num_lines - 1) + 1 + 1 + 1 = 2 * num_lines + 3
        let content_height = num_lines + num_lines.saturating_sub(1); // lines + spacing between them
        let total_height = 1 + content_height + 1 + 1 + 1; // top border + content + empty + footer + bottom

        // Cap at reasonable height (e.g., 15 lines max to avoid taking over the screen)
        (total_height as u16).min(15)
    }

    /// Render the approval bar with wrapping support
    pub fn render(&self, f: &mut Frame, area: Rect) {
        if !self.is_visible() || area.height < 4 {
            return;
        }

        // Clear the area first
        f.render_widget(Clear, area);

        let border_color = Color::DarkGray;
        let inner_width = area.width.saturating_sub(2) as usize;

        // Available width for tabs (full inner width, tabs wrap to next line)
        let tab_width = inner_width.saturating_sub(2); // just margins

        // Build lines of tabs with popup-style buttons
        let mut lines: Vec<Vec<Span>> = Vec::new();
        let mut current_line: Vec<Span> = Vec::new();
        let mut current_width = 0;

        for (idx, action) in self.actions.iter().enumerate() {
            let is_selected = idx == self.selected_index;

            // Status indicator (checkmark or X)
            let (indicator, indicator_color) = match action.status {
                ApprovalStatus::Approved => ("✓", Color::Green),
                ApprovalStatus::Rejected => ("✗", Color::Red),
            };

            // Calculate button width: " ✓ Label " with spaces
            let button_text = format!(" {} {} ", indicator, action.label);
            let button_width = button_text.chars().count();
            let separator_len = if current_line.is_empty() { 0 } else { 1 }; // " " between buttons
            let needed_width = button_width + separator_len;

            // Check if we need to wrap to next line
            if !current_line.is_empty() && current_width + needed_width > tab_width {
                // Start a new line
                lines.push(current_line);
                current_line = Vec::new();
                current_width = 0;
            }

            // Add separator between buttons (not at start of line)
            if !current_line.is_empty() {
                current_line.push(Span::raw(" "));
                current_width += 1;
            }

            // Style like approval popup tabs
            if is_selected {
                // Selected: black text on cyan background
                // First render the background with indicator colored
                current_line.push(Span::styled(
                    " ",
                    Style::default().fg(Color::Black).bg(Color::Cyan),
                ));
                current_line.push(Span::styled(
                    indicator,
                    Style::default().fg(indicator_color).bg(Color::Cyan),
                ));
                current_line.push(Span::styled(
                    format!(" {} ", action.label),
                    Style::default().fg(Color::Black).bg(Color::Cyan),
                ));
            } else {
                // Unselected: white text on dark background (Indexed 235)
                current_line.push(Span::styled(
                    " ",
                    Style::default().fg(Color::White).bg(Color::Indexed(235)),
                ));
                current_line.push(Span::styled(
                    indicator,
                    Style::default().fg(indicator_color).bg(Color::Indexed(235)),
                ));
                current_line.push(Span::styled(
                    format!(" {} ", action.label),
                    Style::default().fg(Color::White).bg(Color::Indexed(235)),
                ));
            }
            current_width += button_width;
        }

        // Don't forget the last line
        if !current_line.is_empty() {
            lines.push(current_line);
        }

        // If no lines, create empty one
        if lines.is_empty() {
            lines.push(Vec::new());
        }

        // Title
        let title = " Approval Required ";
        let title_len = title.len();

        // Top border with title
        let dashes_before = 1;
        let dashes_after = inner_width.saturating_sub(dashes_before + title_len);

        let top_line = Line::from(vec![
            Span::styled("┌", Style::default().fg(border_color)),
            Span::styled("─".repeat(dashes_before), Style::default().fg(border_color)),
            Span::styled(
                title,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("─".repeat(dashes_after), Style::default().fg(border_color)),
            Span::styled("┐", Style::default().fg(border_color)),
        ]);

        // Render top border
        f.render_widget(
            Paragraph::new(top_line),
            Rect::new(area.x, area.y, area.width, 1),
        );

        // Render content lines (tabs/buttons) with spacing between rows
        let mut current_y = area.y + 1;
        for (line_idx, tab_spans) in lines.iter().enumerate() {
            if current_y >= area.y + area.height.saturating_sub(2) {
                break; // Leave room for footer and bottom border
            }

            // Add empty line before each button row (except the first)
            if line_idx > 0 {
                let spacing_line = Line::from(vec![
                    Span::styled("│", Style::default().fg(border_color)),
                    Span::raw(" ".repeat(inner_width)),
                    Span::styled("│", Style::default().fg(border_color)),
                ]);
                f.render_widget(
                    Paragraph::new(spacing_line),
                    Rect::new(area.x, current_y, area.width, 1),
                );
                current_y += 1;

                if current_y >= area.y + area.height.saturating_sub(2) {
                    break;
                }
            }

            // Calculate content width for this line
            let content_width: usize = tab_spans.iter().map(|s| s.content.chars().count()).sum();

            let mut line_spans = vec![Span::styled("│", Style::default().fg(border_color))];
            line_spans.push(Span::raw(" ")); // space after left border
            line_spans.extend(tab_spans.clone());

            // Padding to fill the rest of the line
            let padding = inner_width.saturating_sub(content_width + 2);
            line_spans.push(Span::raw(" ".repeat(padding)));
            line_spans.push(Span::raw(" ")); // space before right border
            line_spans.push(Span::styled("│", Style::default().fg(border_color)));

            f.render_widget(
                Paragraph::new(Line::from(line_spans)),
                Rect::new(area.x, current_y, area.width, 1),
            );
            current_y += 1;
        }

        // Footer line with controls (directly after buttons, no empty line)
        let footer_y = current_y;
        if footer_y < area.y + area.height.saturating_sub(1) {
            // Build footer controls with same style as approval popup
            let footer_controls = vec![
                Span::styled("space", Style::default().fg(Color::Cyan)),
                Span::styled(" toggle", Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled("←→", Style::default().fg(Color::Cyan)),
                Span::styled(" navigate", Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled("enter", Style::default().fg(Color::Cyan)),
                Span::styled(" submit", Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled("esc", Style::default().fg(Color::Cyan)),
                Span::styled(" reject all", Style::default().fg(Color::DarkGray)),
            ];

            let footer_content_width: usize = footer_controls
                .iter()
                .map(|s| s.content.chars().count())
                .sum();

            let mut footer_spans = vec![Span::styled("│", Style::default().fg(border_color))];
            footer_spans.push(Span::raw(" ")); // space after left border
            footer_spans.extend(footer_controls);

            // Padding to fill the rest of the line
            let padding = inner_width.saturating_sub(footer_content_width + 2);
            footer_spans.push(Span::raw(" ".repeat(padding)));
            footer_spans.push(Span::raw(" ")); // space before right border
            footer_spans.push(Span::styled("│", Style::default().fg(border_color)));

            f.render_widget(
                Paragraph::new(Line::from(footer_spans)),
                Rect::new(area.x, footer_y, area.width, 1),
            );
        }

        // Bottom border
        let bottom_y = footer_y + 1;
        if bottom_y < area.y + area.height {
            let bottom_line = Line::from(vec![
                Span::styled("└", Style::default().fg(border_color)),
                Span::styled("─".repeat(inner_width), Style::default().fg(border_color)),
                Span::styled("┘", Style::default().fg(border_color)),
            ]);
            f.render_widget(
                Paragraph::new(bottom_line),
                Rect::new(area.x, bottom_y, area.width, 1),
            );
        }
    }

    // === Legacy compatibility methods (can be removed later) ===

    /// Check if expanded (always false in new design)
    pub fn is_expanded(&self) -> bool {
        false
    }

    /// Collapse (no-op in new design)
    pub fn collapse(&mut self) {}

    /// Select action by 1-based index
    pub fn select_action(&mut self, index: usize) {
        if index > 0 && index <= self.actions.len() {
            self.selected_index = index - 1;
        }
    }

    /// Approve selected
    pub fn approve_selected(&mut self) {
        if let Some(action) = self.actions.get_mut(self.selected_index) {
            action.status = ApprovalStatus::Approved;
        }
    }

    /// Reject selected
    pub fn reject_selected(&mut self) {
        if let Some(action) = self.actions.get_mut(self.selected_index) {
            action.status = ApprovalStatus::Rejected;
        }
    }

    /// Toggle expanded (no-op in new design)
    pub fn toggle_expanded(&mut self) {}

    /// Get number of pending actions (always 0 now since no Pending state)
    pub fn pending_count(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stakpak_shared::models::integrations::openai::FunctionCall;

    fn make_tool_call(name: &str, args: &str) -> ToolCall {
        ToolCall {
            id: format!("call_{}", name),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: args.to_string(),
            },
            metadata: None,
        }
    }

    #[test]
    fn test_approval_bar_basics() {
        let mut bar = ApprovalBar::new();
        assert!(!bar.is_visible());

        bar.add_action(make_tool_call(
            "run_command",
            r#"{"command": "npm install"}"#,
        ));
        assert!(bar.is_visible());
        // All actions start as Approved
        assert_eq!(bar.get_approved().len(), 1);
        assert_eq!(bar.get_rejected().len(), 0);

        bar.toggle_selected();
        assert_eq!(bar.get_approved().len(), 0);
        assert_eq!(bar.get_rejected().len(), 1);
    }

    #[test]
    fn test_navigation() {
        let mut bar = ApprovalBar::new();
        bar.add_action(make_tool_call(
            "run_command",
            r#"{"command": "npm install"}"#,
        ));
        bar.add_action(make_tool_call("create", r#"{"path": "test.ts"}"#));
        bar.add_action(make_tool_call("str_replace", r#"{"path": "index.ts"}"#));

        assert_eq!(bar.selected_index(), 0);

        bar.select_next();
        assert_eq!(bar.selected_index(), 1);

        bar.select_next();
        assert_eq!(bar.selected_index(), 2);

        bar.select_next(); // wraps
        assert_eq!(bar.selected_index(), 0);

        bar.select_prev(); // wraps back
        assert_eq!(bar.selected_index(), 2);
    }

    #[test]
    fn test_toggle() {
        let mut bar = ApprovalBar::new();
        bar.add_action(make_tool_call(
            "run_command",
            r#"{"command": "npm install"}"#,
        ));

        // Starts as Approved
        assert_eq!(
            bar.selected_action().unwrap().status,
            ApprovalStatus::Approved
        );

        bar.toggle_selected();
        assert_eq!(
            bar.selected_action().unwrap().status,
            ApprovalStatus::Rejected
        );

        bar.toggle_selected();
        assert_eq!(
            bar.selected_action().unwrap().status,
            ApprovalStatus::Approved
        );
    }

    #[test]
    fn test_default_approved() {
        let mut bar = ApprovalBar::new();
        bar.add_action(make_tool_call(
            "run_command",
            r#"{"command": "npm install"}"#,
        ));
        bar.add_action(make_tool_call("create", r#"{"path": "test.ts"}"#));
        bar.add_action(make_tool_call("str_replace", r#"{"path": "index.ts"}"#));

        // All should be approved by default
        assert_eq!(bar.get_approved().len(), 3);
        assert_eq!(bar.get_rejected().len(), 0);
    }

    #[test]
    fn test_labels() {
        let bar_action = ApprovalAction::new(make_tool_call("run_command", "{}"));
        assert_eq!(bar_action.label, "Run Command");

        let bar_action = ApprovalAction::new(make_tool_call("str_replace", "{}"));
        assert_eq!(bar_action.label, "Str Replace");

        let bar_action = ApprovalAction::new(make_tool_call("create", "{}"));
        assert_eq!(bar_action.label, "Create");
    }

    #[test]
    fn test_calculate_height_single_row() {
        let mut bar = ApprovalBar::new();
        bar.add_action(make_tool_call("run_command", "{}"));
        bar.add_action(make_tool_call("create", "{}"));

        // With wide terminal (100 chars), should fit on one row
        // Height = top border (1) + 1 content line + empty (1) + footer (1) + bottom (1) = 5
        let height = bar.calculate_height(100);
        assert_eq!(height, 5);
    }

    #[test]
    fn test_calculate_height_multiple_rows() {
        let mut bar = ApprovalBar::new();
        // Add many tool calls to force wrapping
        for i in 0..8 {
            bar.add_action(make_tool_call(&format!("tool_name_{}", i), "{}"));
        }

        // With narrow terminal (50 chars), should wrap to multiple rows
        // Each button is roughly " ✓ Tool Name X " = ~16 chars
        // 50 - 6 (borders/padding) = 44 usable width
        // ~2-3 buttons per row, so 8 buttons = 3-4 rows
        let height = bar.calculate_height(50);
        assert!(
            height > 5,
            "Expected height > 5 for wrapped rows, got {}",
            height
        );
    }

    #[test]
    fn test_calculate_height_hidden() {
        let bar = ApprovalBar::new();
        // Empty/hidden bar should return 0
        assert_eq!(bar.calculate_height(100), 0);
    }
}
