//! Approval Popup Service
//!
//! This module provides a popup service for displaying and managing tool call approvals.
//! The popup matches the design from the image with cyan borders, dark background, and
//! horizontal tabs showing each tool call with approval/rejection status.
//!
//! Example usage:
//! ```rust
//! use stakpak_shared::models::integrations::openai::ToolCall;
//!
//! // Create tool calls (example)
//! let tool_calls = vec![tool_call1, tool_call2, tool_call3];
//!
//! // Create popup service
//! let mut popup_service = PopupService::new_with_tool_calls(tool_calls);
//!
//! // Show the popup
//! popup_service.toggle();
//!
//! // Handle events
//! popup_service.next_tab();        // Navigate to next tool call
//! popup_service.prev_tab();        // Navigate to previous tool call
//! popup_service.toggle_approval(); // Toggle approval status
//! popup_service.escape();          // Close popup
//!
//! // Check approval status
//! let all_approved = popup_service.all_approved();
//! let approvals = popup_service.get_all_approvals();
//! ```

use crate::services::bash_block::{format_text_content, render_file_diff_full};
use crate::services::detect_term::{self, is_unsupported_terminal};
use crate::services::file_diff::render_file_diff_block;
use crate::services::message::{
    extract_full_command_arguments, extract_truncated_command_arguments, get_command_type_name,
};
use crate::services::message_pattern::spans_to_string;
use popup_widget::{PopupConfig, PopupPosition, PopupWidget, StyledLineContent, Tab};
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
}

impl PopupService {
    /// Create a new popup service
    pub fn new() -> Self {
        Self {
            popup: Self::create_empty_popup(),
            tool_calls: Vec::new(),
            selected_index: 0,
        }
    }

    /// Create a new popup service with tool calls
    pub fn new_with_tool_calls(tool_calls: Vec<ToolCall>) -> Self {
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
        };

        // Create the popup with the actual content
        service.popup = service.create_popup_with_tool_calls(&tool_call_infos);
        service.popup.show();
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
        let _ = self.popup.handle_event(popup_widget::PopupEvent::PrevTab);
        // Update our selected index to match the popup's selected tab
        self.selected_index = self.popup.state().selected_tab;
        eprintln!("prev_tab - selected_index: {}", self.selected_index);
    }

    /// Handle next tab
    pub fn next_tab(&mut self) {
        let _ = self.popup.handle_event(popup_widget::PopupEvent::NextTab);
        // Update our selected index to match the popup's selected tab
        self.selected_index = self.popup.state().selected_tab;
        eprintln!("next_tab - selected_index: {}", self.selected_index);
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

    /// Create popup with tool calls
    fn create_popup_with_tool_calls(&self, tool_call_infos: &[ToolCallInfo]) -> PopupWidget {
        if tool_call_infos.is_empty() {
            return Self::create_empty_popup();
        }

        let tabs: Vec<Tab> = tool_call_infos
            .iter()
            .enumerate()
            .map(|(index, tool_call_info)| {
                let tool_call = &tool_call_info.tool_call;
                let tool_name = get_command_type_name(tool_call);

                // Create status symbol with color
                let (status_symbol, status_color) = match tool_call_info.status {
                    ApprovalStatus::Approved => (" ✓", Color::Green),
                    ApprovalStatus::Rejected => (" ✗", Color::LightRed),
                    ApprovalStatus::Pending => ("", Color::Gray),
                };

                eprintln!(
                    "DEBUG: Tab {} - Status: {:?}, Symbol: '{}', Color: {:?}",
                    index, tool_call_info.status, status_symbol, status_color
                );

                // Create styled tab title with separate spans for text and status
                let tab_title_line = Line::from(vec![
                    Span::styled(format!("{}.{}", index + 1, tool_name), Style::default()),
                    Span::styled(status_symbol, Style::default().fg(status_color)),
                ]);

                // Create content for this tab
                let content = self.create_tool_call_content(tool_call, &tool_call_info);

                Tab::new_with_custom_title(
                    format!("tool_call_{}", index),
                    format!("{}.{}{}", index + 1, tool_name, status_symbol),
                    TabContent::new(
                        format!("{}.{}{}", index + 1, tool_name, status_symbol),
                        format!("tool_call_{}", index),
                        content,
                    ),
                    tab_title_line,
                )
            })
            .collect();

        // Create popup configuration with tabs
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
            .background_style(Style::default().bg(Color::Rgb(25, 26, 38)))
            .popup_background_style(Style::default().bg(Color::Rgb(25, 26, 38)))
            .show_tabs(true)
            .tab_alignment(popup_widget::Alignment::Left)
            .tab_style(Style::default().fg(Color::White).bg(Color::DarkGray))
            .selected_tab_style(Style::default().fg(Color::White).bg(Color::Cyan))
            .tab_borders(false)
            .use_fallback_colors(true)
            .terminal_detector(|| {
                let terminal_info = detect_term::detect_terminal();
                is_unsupported_terminal(&terminal_info.emulator)
            })
            .fixed_header_lines(8) // Fixed header: Tool Details + Content sections
            .footer(Some(vec![
                "Enter submit    ←→ for action    Space toggle approve/reject   ↑↓ to scroll    Esc exit".to_string(),
            ]))
            .footer_style(Some(Style::default().fg(Color::Gray)))
            .position(PopupPosition::Responsive {
                width_percent: 0.8,
                height_percent: 0.7,
                min_width: 30,
                min_height: 20,
            });

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
        tool_call_info: &ToolCallInfo,
    ) -> StyledLineContent {
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
        lines.push((Line::from(""), Style::default()));

        // Create a line with tool name and status on the same line
        let tool_status_line = Line::from(vec![
            ratatui::text::Span::styled("Tool".to_string(), Style::default().fg(Color::DarkGray)),
            ratatui::text::Span::styled(
                format!(" {}", tool_name),
                Style::default().fg(Color::Gray),
            ),
            ratatui::text::Span::styled("       ".to_string(), Style::default()),
            ratatui::text::Span::styled("Status ", Style::default().fg(Color::DarkGray)),
            ratatui::text::Span::styled(status_text, Style::default().fg(status_color)),
        ]);
        lines.push((tool_status_line, Style::default()));

        lines.push((Line::from(""), Style::default()));
        lines.push((Line::from(""), Style::default()));

        let output = extract_full_command_arguments(tool_call);

        // remove first line of output and return the rest
        let output = output.lines().skip(1).collect::<Vec<_>>().join("\n");

        // Use the popup's inner width for text formatting
        let inner_width = self.inner_width();
        eprintln!("inner_width: {}", inner_width);
        let rendered_lines = if tool_call.function.name == "str_replace" {
            let (_diff_lines, full_diff_lines) = render_file_diff_block(tool_call, inner_width);
            if !full_diff_lines.is_empty() {
                full_diff_lines
            } else {
                format_text_content(&output, inner_width)
            }
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

        // add simulated dummy text in lines
        // for _ in 0..50 {
        //     lines.push((
        //         Line::from("This is a simulated dummy text"),
        //         Style::default().fg(Color::Gray),
        //     ));
        // }
        // lines.push((
        //     Line::from("This is a simulated THIS IS THE END OF DUMMY TEXT"),
        //     Style::default().fg(Color::Gray),
        // ));

        StyledLineContent::new(lines)
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
            .title_alignment(popup_widget::Alignment::Center)
            .border_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .background_style(Style::default().bg(Color::Rgb(25, 26, 38)))
            .popup_background_style(Style::default().bg(Color::Rgb(25, 26, 38)))
            .show_tabs(false)
            .use_fallback_colors(true)
            .terminal_detector(|| {
                let terminal_info = detect_term::detect_terminal();
                is_unsupported_terminal(&terminal_info.emulator)
            })
            .position(PopupPosition::Responsive {
                width_percent: 0.8,
                height_percent: 0.7,
                min_width: 30,
                min_height: 20,
            });

        PopupWidget::new(config)
    }

    /// Toggle the approval status of the currently selected tool call
    pub fn toggle_approval_status(&mut self) {
        eprintln!(
            "toggle_approval_status - selected_index: {}",
            self.selected_index
        );
        if let Some(tool_call_info) = self.tool_calls.get_mut(self.selected_index) {
            let old_status = tool_call_info.status.clone();
            tool_call_info.status = match tool_call_info.status {
                ApprovalStatus::Approved => ApprovalStatus::Rejected,
                ApprovalStatus::Rejected => ApprovalStatus::Approved,
                ApprovalStatus::Pending => ApprovalStatus::Approved,
            };

            eprintln!(
                "DEBUG: Toggled tool call {} from {:?} to {:?}",
                self.selected_index, old_status, tool_call_info.status
            );

            // Recreate the popup with updated status and preserve selected tab
            self.popup = self.create_popup_with_tool_calls(&self.tool_calls);
            // Set the selected tab to maintain the current selection
            self.popup.set_selected_tab(self.selected_index);
            // Make sure the popup stays visible after recreation
            self.popup.show();

            // Debug: Print approval lists
            let approved = self.get_approved_tool_calls().len();
            let rejected = self.get_rejected_tool_calls().len();
            let pending = self.get_pending_tool_calls().len();
            eprintln!(
                "DEBUG: Approval counts - Approved: {}, Rejected: {}, Pending: {}",
                approved, rejected, pending
            );
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
        let result = self.popup.handle_event(event);

        // Update our selected index to match the popup's selected tab
        let old_index = self.selected_index;
        self.selected_index = self.popup.config().selected_tab;
        if old_index != self.selected_index {
            eprintln!(
                "handle_event - selected_index changed from {} to {}",
                old_index, self.selected_index
            );
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

    /// Get the inner width of the popup
    pub fn inner_width(&self) -> usize {
        self.popup.inner_width()
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
            styled_content: StyledLineContent::new(self.styled_content.lines.clone()),
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
