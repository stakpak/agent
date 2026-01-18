//! Approval Popup Service
//!
//! This module provides a popup service for displaying and managing tool call approvals.
//! The popup matches the design from the image with cyan borders, dark background, and
//! horizontal tabs showing each tool call with approval/rejection status.
//!
//! Example usage:
//! ```rust,no_run
//! # use stakpak_shared::models::integrations::openai::{ToolCall, FunctionCall};
//! # use ratatui::layout::Size;
//! # // Note: PopupService would need to be imported in actual usage
//!
//! // Create tool calls (example)
//! let tool_calls = vec![
//!     ToolCall {
//!         id: "call_1".to_string(),
//!         r#type: "function".to_string(),
//!         function: FunctionCall {
//!             name: "example_function".to_string(),
//!             arguments: "{}".to_string(),
//!         },
//!     }
//! ];
//!
//! // Create popup service
//! let terminal_size = Size { width: 80, height: 24 };
//! // let mut popup_service = PopupService::new_with_tool_calls(tool_calls, terminal_size);
//!
//! // Handle events
//! // popup_service.next_tab();        // Navigate to next tool call
//! // popup_service.prev_tab();        // Navigate to previous tool call
//! // popup_service.toggle_approval_status(); // Toggle approval status
//! // popup_service.escape();          // Close popup
//!
//! // Check approval status
//! // let all_approved = popup_service.all_approved();
//! // let approvals = popup_service.get_all_approvals();
//! ```

use crate::constants::APPROVAL_POPUP_WIDTH_PERCENT;
use crate::services::bash_block::{format_text_content, preprocess_terminal_output};
use crate::services::detect_term::{self, is_unsupported_terminal};
use crate::services::file_diff::render_file_diff_block;
use crate::services::markdown_renderer::render_markdown_to_lines;
use crate::services::message::{extract_full_command_arguments, get_command_type_name};
use crate::services::message_pattern::spans_to_string;
use popup_widget::{PopupConfig, PopupPosition, PopupWidget, StyledLineContent, Tab};
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use stakpak_shared::models::integrations::openai::ToolCall;

/// Tool call approval status
#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalStatus {
    Approved,
    Rejected,
    Pending,
}

/// Tool call information for the popup
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub tool_call: ToolCall,
    pub status: ApprovalStatus,
}

/// Popup service that manages its own state and event handling
pub struct PopupService {
    popup: PopupWidget,
    tool_calls: Vec<ToolCallInfo>,
    selected_index: usize,
    terminal_size: ratatui::layout::Rect,
    is_maximized: bool,
    /// Per-tab scroll positions for smart scrolling
    tab_scroll_positions: Vec<Option<usize>>,
    /// Cached input area for position calculation (to avoid recalculating every frame)
    cached_input_area: Option<ratatui::layout::Rect>,
}

impl Default for PopupService {
    fn default() -> Self {
        Self::new()
    }
}

impl PopupService {
    /// Create a new popup service
    pub fn new() -> Self {
        Self {
            popup: Self::create_empty_popup(),
            tool_calls: Vec::new(),
            selected_index: 0,
            terminal_size: ratatui::layout::Rect::new(0, 0, 80, 24), // Default terminal size
            is_maximized: false,
            tab_scroll_positions: Vec::new(),
            cached_input_area: None,
        }
    }

    /// Create a new popup service with tool calls
    pub fn new_with_tool_calls(tool_calls: Vec<ToolCall>, terminal_size: Size) -> Self {
        let term_rect = ratatui::layout::Rect::new(0, 0, terminal_size.width, terminal_size.height);
        let tool_call_infos: Vec<ToolCallInfo> = tool_calls
            .into_iter()
            .map(|tool_call| ToolCallInfo {
                tool_call,
                status: ApprovalStatus::Approved, // All tool calls approved by default
            })
            .collect();

        let mut service = Self {
            popup: Self::create_empty_popup(),
            tool_calls: tool_call_infos.clone(),
            selected_index: 0,
            terminal_size: term_rect,
            is_maximized: false,
            tab_scroll_positions: vec![None; tool_call_infos.len()],
            cached_input_area: None,
        };

        // Create the popup with the actual content
        service.popup = service.create_popup_with_tool_calls(&tool_call_infos, term_rect);
        service.popup.show();

        // Apply smart scroll for the first tab if it's a diff
        service.apply_smart_scroll_for_current_tab();

        service
    }

    /// Check if the popup is visible
    pub fn is_visible(&self) -> bool {
        self.popup.is_visible()
    }

    /// Render the popup if visible
    pub fn render(&mut self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        if self.is_visible() {
            self.popup.render(f, area);
        }
    }

    /// Toggle the popup visibility
    pub fn toggle(&mut self) {
        let _ = self.popup.handle_event(popup_widget::PopupEvent::Toggle);
    }

    /// Handle scroll up
    pub fn scroll_up(&mut self) {
        let _ = self.popup.handle_event(popup_widget::PopupEvent::ScrollUp);
    }

    /// Handle scroll down
    pub fn scroll_down(&mut self) {
        let _ = self
            .popup
            .handle_event(popup_widget::PopupEvent::ScrollDown);
    }

    /// Handle previous tab
    pub fn prev_tab(&mut self) {
        // Save current scroll position before switching
        self.save_current_scroll_position();

        let _ = self.popup.handle_event(popup_widget::PopupEvent::PrevTab);
        // Update our selected index to match the popup's selected tab
        self.selected_index = self.popup.state().selected_tab;

        // Restore scroll position for the new tab
        self.restore_scroll_position();
    }

    /// Handle next tab
    pub fn next_tab(&mut self) {
        // Save current scroll position before switching
        self.save_current_scroll_position();

        let _ = self.popup.handle_event(popup_widget::PopupEvent::NextTab);
        // Update our selected index to match the popup's selected tab
        self.selected_index = self.popup.state().selected_tab;

        // Restore scroll position for the new tab
        self.restore_scroll_position();
    }

    /// Handle escape
    pub fn escape(&mut self) {
        let _ = self.popup.handle_event(popup_widget::PopupEvent::Escape);
    }

    /// Get approval status for a specific tool call
    pub fn get_approval_status(&self, index: usize) -> Option<&ApprovalStatus> {
        self.tool_calls.get(index).map(|info| &info.status)
    }

    /// Get all tool calls with their approval status
    pub fn get_all_approvals(&self) -> Vec<(usize, &ApprovalStatus)> {
        self.tool_calls
            .iter()
            .enumerate()
            .map(|(index, info)| (index, &info.status))
            .collect()
    }

    /// Check if all tool calls are approved
    pub fn all_approved(&self) -> bool {
        self.tool_calls
            .iter()
            .all(|info| info.status == ApprovalStatus::Approved)
    }

    /// Check if any tool calls are rejected
    pub fn has_rejected(&self) -> bool {
        self.tool_calls
            .iter()
            .any(|info| info.status == ApprovalStatus::Rejected)
    }

    /// Get the current selected tool call index
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Set the selected tool call index
    pub fn set_selected_index(&mut self, index: usize) {
        if index < self.tool_calls.len() {
            self.selected_index = index;
        }
    }

    /// Calculate dynamic popup size based on content and terminal size
    fn calculate_dynamic_popup_size(
        &self,
        tool_call_infos: &[ToolCallInfo],
        terminal_size: ratatui::layout::Rect,
    ) -> (f32, f32) {
        // Constants for size calculation
        const MIN_HEIGHT_PERCENT: f32 = 0.4;
        const MAX_HEIGHT_PERCENT: f32 = 0.85;
        const WIDTH_PERCENT: f32 = APPROVAL_POPUP_WIDTH_PERCENT;

        // UI component heights (in lines)
        const BORDER_HEIGHT: usize = 2; // Top and bottom borders
        const TITLE_HEIGHT: usize = 2; // Title + spacing
        const TAB_HEADER_HEIGHT: usize = 2; // Tab headers + spacing
        const FOOTER_HEIGHT: usize = 1; // Footer
        const SUBHEADER_HEIGHT: usize = 4; // Average subheader + spacing
        const SPACING_BUFFER: usize = 2; // Extra spacing buffer

        let total_ui_overhead = BORDER_HEIGHT
            + TITLE_HEIGHT
            + TAB_HEADER_HEIGHT
            + FOOTER_HEIGHT
            + SUBHEADER_HEIGHT
            + SPACING_BUFFER;

        // Ensure minimum terminal height
        let safe_terminal_height = terminal_size.height.max(32);

        // Calculate available space for content
        let max_popup_height_lines = (safe_terminal_height as f32 * MAX_HEIGHT_PERCENT) as usize;
        let max_content_lines = max_popup_height_lines.saturating_sub(total_ui_overhead);

        // Find the tallest content among all tool calls
        let mut max_tool_content_height = 0;
        for tool_call_info in tool_call_infos.iter() {
            let content = self.create_tool_call_content(&tool_call_info.tool_call, tool_call_info);
            let content_height = content.lines.len() + 2;
            max_tool_content_height = max_tool_content_height.max(content_height);
        }

        // Determine optimal height based on content
        let optimal_content_height = if max_tool_content_height <= max_content_lines {
            // Content fits within max space - size to content
            max_tool_content_height
        } else {
            // Content exceeds max space - use max and enable scrolling
            max_content_lines
        };

        // Calculate total popup height needed
        let required_popup_height = optimal_content_height + total_ui_overhead;

        // Convert to percentage, ensuring it's within bounds
        let height_percent = (required_popup_height as f32 / safe_terminal_height as f32)
            .clamp(MIN_HEIGHT_PERCENT, MAX_HEIGHT_PERCENT);

        (WIDTH_PERCENT, height_percent)
    }

    /// Update popup size when terminal is resized
    pub fn update_terminal_size(&mut self, new_size: ratatui::layout::Rect) {
        self.terminal_size = new_size;
        // Position will be updated by update_position_for_input_area
    }

    /// Update popup position to be anchored at the bottom of the terminal
    /// This positions the popup at the very bottom, overlaying the input area and hint
    /// Uses caching to avoid expensive recalculations on every frame
    pub fn update_position_for_input_area(
        &mut self,
        terminal_size: ratatui::layout::Rect,
        input_area: ratatui::layout::Rect,
    ) {
        // Check if we need to recalculate (terminal size or input area changed)
        let needs_recalc =
            self.terminal_size != terminal_size || self.cached_input_area != Some(input_area);

        if !needs_recalc {
            return; // Position is already correct, skip expensive calculation
        }

        self.terminal_size = terminal_size;
        self.cached_input_area = Some(input_area);

        if self.is_maximized {
            // Full screen when maximized - use absolute positioning
            self.popup.config_mut().position = PopupPosition::Absolute {
                x: 0,
                y: 0,
                width: terminal_size.width,
                height: terminal_size.height,
            };
            return;
        }

        // Fallback to centered responsive if input_area is invalid (e.g., sessions dialog is open)
        if input_area.width == 0 || input_area.height == 0 {
            let (width_percent, height_percent) =
                self.calculate_dynamic_popup_size(&self.tool_calls, terminal_size);
            self.popup.config_mut().position = PopupPosition::Responsive {
                width_percent,
                height_percent,
                min_width: 80,
                min_height: 15,
            };
            return;
        }

        // Calculate the width to match the input area (full width minus margins)
        let popup_width = input_area.width;
        let left_margin = input_area.x;

        // Bottom offset of 0 means the popup's bottom edge touches the terminal's bottom edge
        let bottom_offset = 0u16;

        // Calculate dynamic height based on content
        let (_, height_percent) =
            self.calculate_dynamic_popup_size(&self.tool_calls, terminal_size);

        // Calculate actual height, but cap it so popup doesn't go above terminal top
        let max_available_height = terminal_size.height; // Full terminal height available
        let desired_height = (terminal_size.height as f32 * height_percent) as u16;
        let popup_height = desired_height.min(max_available_height).max(15); // Minimum height of 15

        // Update the popup's position configuration
        self.popup.config_mut().position = PopupPosition::BottomAnchored {
            left_margin,
            bottom_offset,
            width: popup_width,
            height: popup_height,
        };
    }

    /// Get access to popup state for external updates
    pub fn popup_state(&self) -> &popup_widget::PopupState {
        self.popup.state()
    }

    /// Get mutable access to popup state for external updates  
    pub fn popup_state_mut(&mut self) -> &mut popup_widget::PopupState {
        self.popup.state_mut()
    }

    /// Create popup with tool calls
    fn create_popup_with_tool_calls(
        &self,
        tool_call_infos: &[ToolCallInfo],
        terminal_size: ratatui::layout::Rect,
    ) -> PopupWidget {
        if tool_call_infos.is_empty() {
            return Self::create_empty_popup();
        }

        // Create subheaders for all tool calls first
        let subheaders: Vec<Vec<(Line<'static>, Style)>> = tool_call_infos
            .iter()
            .map(|tool_call_info| self.render_subheader(&tool_call_info.tool_call, tool_call_info))
            .collect();

        // Use dynamic height calculation that adapts to content and terminal size
        let (width_percent, height_percent) = if self.is_maximized {
            (1.0, 1.0) // Full screen when maximized
        } else {
            self.calculate_dynamic_popup_size(tool_call_infos, terminal_size)
        };

        let tabs: Vec<Tab> = tool_call_infos
            .iter()
            .enumerate()
            .map(|(index, tool_call_info)| {
                let tool_call = &tool_call_info.tool_call;
                let tool_name = get_command_type_name(tool_call);

                // Create status symbol with color
                let (status_symbol, status_color) = match tool_call_info.status {
                    ApprovalStatus::Approved => (" ✓", Color::Cyan),
                    ApprovalStatus::Rejected => (" ✗", Color::Red),
                    ApprovalStatus::Pending => ("", Color::Gray),
                };

                // Create styled title line with colored status symbol and strikethrough for rejected
                let styled_title = if status_symbol.is_empty() {
                    // No status symbol, just the title
                    Line::from(format!("{}.{}", index + 1, tool_name))
                } else {
                    // Title with colored status symbol
                    let mut title_style = Style::default();
                    if tool_call_info.status == ApprovalStatus::Rejected {
                        title_style = title_style
                            .fg(Color::Red)
                            .add_modifier(Modifier::CROSSED_OUT);
                    }

                    Line::from(vec![
                        Span::styled(format!("{}.{}", index + 1, tool_name), title_style),
                        Span::styled(status_symbol, Style::default().fg(status_color)),
                    ])
                };

                // Create content for this tab
                let content = self.create_tool_call_content(tool_call, tool_call_info);

                // Get the subheader for this tab
                let subheader = subheaders.get(index).cloned();

                Tab::new_with_custom_title_and_subheader(
                    format!("tool_call_{}", index),
                    format!("{}.{}{}", index + 1, tool_name, status_symbol), // Keep plain title as fallback
                    TabContent::new(
                        format!("{}.{}{}", index + 1, tool_name, status_symbol),
                        format!("tool_call_{}", index),
                        content,
                    ),
                    styled_title,
                    subheader,
                )
            })
            .collect();

        // height_percent is already clamped in calculate_dynamic_popup_size

        let has_diff_tabs = tool_call_infos.iter().any(|info| {
            let name = crate::utils::strip_tool_name(&info.tool_call.function.name);
            name == "str_replace" || name == "create"
        });

        // Create popup configuration with tabs using responsive positioning
        let mut config = PopupConfig::new()
            .title("Permission Required")
            .title_alignment(popup_widget::Alignment::Left)
            .title_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .background_style(Style::default().bg(Color::Reset))
            .popup_background_style(Style::default().bg(Color::Reset))
            .show_tabs(true)
            .tab_alignment(popup_widget::Alignment::Left)
            .tab_style(Style::default().fg(Color::White).bg(Color::Indexed(235)))
            .selected_tab_style(Style::default().fg(Color::Black).bg(Color::Cyan))
            .tab_borders(false)
            .use_fallback_colors(true)
            .terminal_detector(|| {
                let terminal_info = detect_term::detect_terminal();
                is_unsupported_terminal(&terminal_info.emulator)
            })
            .styled_footer(Some(vec![Line::from(vec![
                Span::styled("enter", Style::default().fg(Color::DarkGray)),
                Span::styled(" submit", Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled("←→", Style::default().fg(Color::DarkGray)),
                Span::styled(" select", Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled("space", Style::default().fg(Color::DarkGray)),
                Span::styled(" approve/reject", Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled("ctrl+t", Style::default().fg(Color::DarkGray)),
                Span::styled(" max/min", Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled("↑↓", Style::default().fg(Color::DarkGray)),
                Span::styled(" scroll", Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled("esc", Style::default().fg(Color::DarkGray)),
                Span::styled(" exit", Style::default().fg(Color::Cyan)),
            ])]))
            .footer_style(Some(Style::default().fg(Color::Gray)))
            .position(PopupPosition::Responsive {
                width_percent,
                height_percent,
                min_width: 80,
                min_height: 15,
            })
            .text_between_tabs(Some("→".to_string()))
            .text_between_tabs_style(Style::default().fg(Color::Gray));

        // Set fixed header lines for diff content (1 line for the file path header)
        if has_diff_tabs {
            config = config.fixed_header_lines(1);
        }

        // Add all tabs
        for tab in tabs {
            config = config.add_tab(tab);
        }

        PopupWidget::new(config)
    }

    /// Create content for a specific tool call
    fn create_tool_call_content(
        &self,
        tool_call: &ToolCall,
        _tool_call_info: &ToolCallInfo,
    ) -> StyledLineContent {
        let mut lines = Vec::new();

        // lines.push((Line::from(""), Style::default()));
        let output = extract_full_command_arguments(tool_call);
        let tool_name = crate::utils::strip_tool_name(&tool_call.function.name);
        let output = if tool_name == "run_command" {
            output.replace("command = ", "$ ")
        } else {
            output
        };

        // Use the popup's inner width for text formatting
        let inner_width = self.inner_width() - 2;
        let rendered_lines = if tool_name == "str_replace" || tool_name == "create" {
            let (_diff_lines, full_diff_lines) = render_file_diff_block(tool_call, inner_width);
            if !full_diff_lines.is_empty() {
                full_diff_lines
            } else {
                format_text_content(&output, inner_width)
            }
        } else if tool_name == "run_command" {
            let processed_result = preprocess_terminal_output(&output);
            let bash_text = format!("```bash\n{processed_result}\n```");
            render_markdown_to_lines(&bash_text).unwrap_or_default()
        } else {
            format_text_content(&output, inner_width)
        };

        lines.extend(rendered_lines.into_iter().map(|line| {
            let line_text = spans_to_string(&line);
            if line_text.trim() == "SPACING_MARKER" {
                (Line::from(""), Style::default())
            } else {
                (line, Style::default())
            }
        }));

        // Check if terminal is unsupported for background color
        let is_unsupported =
            detect_term::is_unsupported_terminal(&detect_term::detect_terminal().emulator);
        StyledLineContent::new_with_terminal_detection(lines, is_unsupported)
    }

    /// Create an empty popup (used as placeholder)
    fn create_empty_popup() -> PopupWidget {
        let config = PopupConfig::new()
            .title("Permission Required")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .title_alignment(popup_widget::Alignment::Left)
            .border_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .background_style(Style::default().bg(Color::Rgb(25, 26, 38)))
            .popup_background_style(Style::default().bg(Color::Rgb(25, 26, 38)))
            .show_tabs(false)
            .use_fallback_colors(true)
            .text_between_tabs(Some("→".to_string()))
            .text_between_tabs_style(Style::default().fg(Color::Gray))
            .terminal_detector(|| {
                let terminal_info = detect_term::detect_terminal();
                is_unsupported_terminal(&terminal_info.emulator)
            })
            .fixed_header_lines(0)
            .position(PopupPosition::Responsive {
                width_percent: APPROVAL_POPUP_WIDTH_PERCENT,
                height_percent: 0.7,
                min_width: 30,
                min_height: 10,
            });

        PopupWidget::new(config)
    }

    /// Toggle the approval status of the currently selected tool call
    pub fn toggle_approval_status(&mut self) {
        if let Some(tool_call_info) = self.tool_calls.get_mut(self.selected_index) {
            tool_call_info.status = match tool_call_info.status {
                ApprovalStatus::Approved => ApprovalStatus::Rejected,
                ApprovalStatus::Rejected => ApprovalStatus::Approved,
                ApprovalStatus::Pending => ApprovalStatus::Approved,
            };

            // Recreate the popup with updated status and preserve selected tab
            self.popup = self.create_popup_with_tool_calls(&self.tool_calls, self.terminal_size);
            // Set the selected tab to maintain the current selection
            self.popup.set_selected_tab(self.selected_index);
            // Make sure the popup stays visible after recreation
            self.popup.show();
            // Invalidate position cache so it gets recalculated on next render
            self.cached_input_area = None;
        }
    }

    /// Get all approved tool calls
    pub fn get_approved_tool_calls(&self) -> Vec<&ToolCall> {
        self.tool_calls
            .iter()
            .filter(|info| info.status == ApprovalStatus::Approved)
            .map(|info| &info.tool_call)
            .collect()
    }

    /// Get all rejected tool calls
    pub fn get_rejected_tool_calls(&self) -> Vec<&ToolCall> {
        self.tool_calls
            .iter()
            .filter(|info| info.status == ApprovalStatus::Rejected)
            .map(|info| &info.tool_call)
            .collect()
    }

    /// Get all pending tool calls
    pub fn get_pending_tool_calls(&self) -> Vec<&ToolCall> {
        self.tool_calls
            .iter()
            .filter(|info| info.status == ApprovalStatus::Pending)
            .map(|info| &info.tool_call)
            .collect()
    }

    /// Get approval status summary
    pub fn get_approval_summary(&self) -> (usize, usize, usize) {
        let approved = self.get_approved_tool_calls().len();
        let rejected = self.get_rejected_tool_calls().len();
        let pending = self.get_pending_tool_calls().len();
        (approved, rejected, pending)
    }

    /// Handle popup events and update selected index accordingly
    pub fn handle_event(
        &mut self,
        event: popup_widget::PopupEvent,
    ) -> popup_widget::PopupEventResult {
        // Save current scroll position before handling events that might change tabs
        let should_save_scroll = matches!(
            event,
            popup_widget::PopupEvent::NextTab
                | popup_widget::PopupEvent::PrevTab
                | popup_widget::PopupEvent::SwitchTab(_)
        );

        if should_save_scroll {
            self.save_current_scroll_position();
        }

        let result = self.popup.handle_event(event);

        // Update our selected index to match the popup's selected tab
        let new_selected_index = self.popup.state().selected_tab;
        let tab_changed = new_selected_index != self.selected_index;
        self.selected_index = new_selected_index;

        // If tab changed, restore scroll position for the new tab
        if tab_changed && should_save_scroll {
            self.restore_scroll_position();
        }

        result
    }

    /// Get the current selected tab index
    pub fn selected_tab_index(&self) -> usize {
        self.selected_index
    }

    /// Get the current selected tool call
    pub fn selected_tool_call(&self) -> Option<&ToolCall> {
        self.tool_calls
            .get(self.selected_index)
            .map(|info| &info.tool_call)
    }

    /// Recreate the popup with new terminal size while preserving state
    pub fn recreate_with_terminal_size(&mut self, new_terminal_size: Size) {
        if self.tool_calls.is_empty() {
            return;
        }

        // Store current state
        let was_visible = self.is_visible();
        let current_selected_index = self.selected_index;
        let current_tool_calls = self.tool_calls.clone();
        let current_scroll_positions = self.tab_scroll_positions.clone();

        // Create new popup with updated terminal size
        let term_rect =
            ratatui::layout::Rect::new(0, 0, new_terminal_size.width, new_terminal_size.height);
        self.terminal_size = term_rect;
        self.popup = self.create_popup_with_tool_calls(&current_tool_calls, term_rect);

        // Restore state
        self.popup.set_selected_tab(current_selected_index);
        self.tab_scroll_positions = current_scroll_positions;

        if was_visible {
            self.popup.show();
            // Restore scroll position for the current tab
            self.restore_scroll_position();
        }
    }

    /// Get the inner width of the popup
    pub fn inner_width(&self) -> usize {
        let term_width = self.terminal_size.width as usize;
        if self.is_maximized {
            term_width
        } else {
            (term_width as f32 * APPROVAL_POPUP_WIDTH_PERCENT) as usize
        }
    }

    /// Toggle maximize/minimize state of the popup
    pub fn toggle_maximize(&mut self) {
        self.is_maximized = !self.is_maximized;

        // Recreate the popup with the new size
        if !self.tool_calls.is_empty() {
            let was_visible = self.is_visible();
            let current_selected_index = self.selected_index;
            let current_tool_calls = self.tool_calls.clone();
            let current_scroll_positions = self.tab_scroll_positions.clone();

            self.popup = self.create_popup_with_tool_calls(&current_tool_calls, self.terminal_size);
            self.popup.set_selected_tab(current_selected_index);
            self.tab_scroll_positions = current_scroll_positions;

            if was_visible {
                self.popup.show();
                // Restore scroll position for the current tab
                self.restore_scroll_position();
            }
        }
    }

    /// Check if the popup is maximized
    pub fn is_maximized(&self) -> bool {
        self.is_maximized
    }

    /// Save the current scroll position for the selected tab
    fn save_current_scroll_position(&mut self) {
        if self.selected_index < self.tab_scroll_positions.len() {
            self.tab_scroll_positions[self.selected_index] = Some(self.popup.state().scroll);
        }
    }

    /// Restore the scroll position for the selected tab
    fn restore_scroll_position(&mut self) {
        if self.selected_index < self.tab_scroll_positions.len() {
            if let Some(saved_scroll) = self.tab_scroll_positions[self.selected_index] {
                // Set the scroll position in the popup
                self.popup.state_mut().scroll = saved_scroll;
            } else {
                // First time viewing this tab, calculate smart scroll position
                self.apply_smart_scroll_for_current_tab();
            }
        }
    }

    /// Calculate and apply smart scroll position for diff content
    fn apply_smart_scroll_for_current_tab(&mut self) {
        if let Some(tool_call_info) = self.tool_calls.get(self.selected_index) {
            let tool_call = &tool_call_info.tool_call;

            // Only apply smart scrolling for str_replace and create tool calls
            let tool_name = crate::utils::strip_tool_name(&tool_call.function.name);
            if (tool_name == "str_replace" || tool_name == "create")
                && let Some(target_scroll) = self.calculate_smart_scroll_position(tool_call)
            {
                self.popup.state_mut().scroll = target_scroll;
                // Save this position for future tab switches
                self.tab_scroll_positions[self.selected_index] = Some(target_scroll);
            }
        }
    }

    /// Calculate the smart scroll position for diff content
    fn calculate_smart_scroll_position(&self, tool_call: &ToolCall) -> Option<usize> {
        use crate::services::file_diff::preview_str_replace_editor_style;

        let args: serde_json::Value = serde_json::from_str(&tool_call.function.arguments).ok()?;

        let old_str = args.get("old_str").and_then(|v| v.as_str()).unwrap_or("");
        let new_str = if crate::utils::strip_tool_name(&tool_call.function.name) == "create" {
            args.get("file_text").and_then(|v| v.as_str()).unwrap_or("")
        } else {
            args.get("new_str").and_then(|v| v.as_str()).unwrap_or("")
        };
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let replace_all = args
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Get the first change index from the diff
        let (_, _, _, first_change_index) = preview_str_replace_editor_style(
            path,
            old_str,
            new_str,
            replace_all,
            self.inner_width(),
            crate::utils::strip_tool_name(&tool_call.function.name),
        )
        .ok()?;

        // Apply smart scrolling logic:
        // 1. If first diff line < 3, do nothing (return None)
        // 2. If first diff line >= 3, scroll to show 2 lines before the first diff line
        // Note: The first_change_index is relative to the diff content, but we need to account
        // for the fixed header (1 line) that won't scroll
        if first_change_index < 3 {
            None
        } else {
            // Scroll to 2 lines before the first change, but not less than 0
            // The fixed header (1 line) is always visible, so we scroll the content area
            Some(first_change_index.saturating_sub(2))
        }
    }

    /// Render subheader for a tool call tab
    fn render_subheader(
        &self,
        tool_call: &ToolCall,
        tool_call_info: &ToolCallInfo,
    ) -> Vec<(Line<'static>, Style)> {
        let mut lines = Vec::new();

        // Get tool details
        let tool_name = get_command_type_name(tool_call).to_string();

        // Use the status from the specific tool call info
        let status = &tool_call_info.status;

        let (status_text, status_color) = match status {
            ApprovalStatus::Approved => ("Approved".to_string(), Color::Green),
            ApprovalStatus::Rejected => ("Rejected".to_string(), Color::Red),
            ApprovalStatus::Pending => ("Pending".to_string(), Color::Yellow),
        };

        // push empty line
        // lines.push((Line::from(""), Style::default()));

        // Create a line with tool name and status on the same line
        let tool_status_line = Line::from(vec![
            ratatui::text::Span::styled(
                format!(" {} ", tool_name),
                Style::default().fg(Color::Gray),
            ),
            ratatui::text::Span::styled("(", Style::default().fg(Color::DarkGray)),
            ratatui::text::Span::styled(status_text, Style::default().fg(status_color)),
            ratatui::text::Span::styled(")", Style::default().fg(Color::DarkGray)),
        ]);
        lines.push((tool_status_line, Style::default()));

        lines
    }
}

/// Tab content that wraps StyledLineContent for popup tabs
#[derive(Debug)]
struct TabContent {
    title: String,
    id: String,
    styled_content: StyledLineContent,
}

impl TabContent {
    fn new(title: String, id: String, styled_content: StyledLineContent) -> Self {
        Self {
            title,
            id,
            styled_content,
        }
    }
}

impl popup_widget::traits::TabContent for TabContent {
    fn title(&self) -> &str {
        &self.title
    }

    fn id(&self) -> &str {
        &self.id
    }
}

impl popup_widget::traits::PopupContent for TabContent {
    fn render(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect, scroll: usize) {
        self.styled_content.render(f, area, scroll);
    }

    fn height(&self) -> usize {
        self.styled_content.height()
    }

    fn width(&self) -> usize {
        self.styled_content.width()
    }

    fn get_lines(&self) -> Vec<String> {
        self.styled_content.get_lines()
    }

    fn calculate_rendered_height(&self) -> usize {
        self.styled_content.calculate_rendered_height()
    }

    fn clone_box(&self) -> Box<dyn popup_widget::traits::PopupContent + Send + Sync> {
        Box::new(TabContent {
            title: self.title.clone(),
            id: self.id.clone(),
            styled_content: StyledLineContent::new_with_terminal_detection(
                self.styled_content.lines.clone(),
                self.styled_content.is_unsupported_terminal,
            ),
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
