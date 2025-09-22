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

use crate::services::detect_term::{self, is_unsupported_terminal};
use crate::services::message::{get_command_type_name, extract_truncated_command_arguments};
use popup_widget::{PopupConfig, PopupPosition, PopupWidget, StyledLineContent, Tab};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
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
    }

    /// Handle next tab
    pub fn next_tab(&mut self) {
        let _ = self.popup.handle_event(popup_widget::PopupEvent::NextTab);
        // Update our selected index to match the popup's selected tab
        self.selected_index = self.popup.state().selected_tab;
    }

    /// Handle escape
    pub fn escape(&mut self) {
        let _ = self.popup.handle_event(popup_widget::PopupEvent::Escape);
    }

    /// Toggle approval status of current tool call
    pub fn toggle_approval(&mut self) {
        if let Some(tool_call_info) = self.tool_calls.get_mut(self.selected_index) {
            tool_call_info.status = match tool_call_info.status {
                ApprovalStatus::Approved => ApprovalStatus::Rejected,
                ApprovalStatus::Rejected => ApprovalStatus::Approved,
                ApprovalStatus::Pending => ApprovalStatus::Approved,
            };
            
            // Update the tab title in place without recreating the entire popup
            self.update_tab_title(self.selected_index);
        }
    }

    /// Update a specific tab title without recreating the entire popup
    fn update_tab_title(&mut self, index: usize) {
        if let Some(tool_call_info) = self.tool_calls.get(index) {
            let tool_call = &tool_call_info.tool_call;
            let tool_name = get_command_type_name(tool_call);
            let status_symbol = match tool_call_info.status {
                ApprovalStatus::Approved => " ✓",
                ApprovalStatus::Rejected => " ✗",
                ApprovalStatus::Pending => "",
            };
            
            let tab_title = format!("{}.{}{}", index + 1, tool_name, status_symbol);
            
            // Update the tab title in the popup configuration
            if let Some(tab) = self.popup.config_mut().tabs.get_mut(index) {
                tab.title = tab_title;
            }
        }
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
                    ApprovalStatus::Rejected => (" ✗", Color::Red),
                    ApprovalStatus::Pending => ("", Color::Gray),
                };
                
                // Format tab title like: "1.Create ✓" with colored status
                let tab_title = format!("{}.{}{}", index + 1, tool_name, status_symbol);
                
                // Create content for this tab
                let content = self.create_tool_call_content(tool_call);
                
                Tab::new_with_status(
                    format!("tool_call_{}", index),
                    tab_title.clone(),
                    TabContent::new(tab_title, format!("tool_call_{}", index), content),
                    Some(status_color),
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
    fn create_tool_call_content(&self, tool_call: &ToolCall) -> StyledLineContent {
        let mut lines = Vec::new();
        
        // Get tool details
        let tool_name = get_command_type_name(tool_call).to_string();
        let tool_path = extract_truncated_command_arguments(tool_call, Some("".to_string())).to_string();
        
      
        
        // Get the actual status for this tool call
        let status = if let Some(tool_call_info) = self.tool_calls.get(self.selected_index) {
            &tool_call_info.status
        } else {
            &ApprovalStatus::Approved // default
        };
        
        let (status_text, status_color) = match status {
            ApprovalStatus::Approved => ("Approved".to_string(), Color::Green),
            ApprovalStatus::Rejected => ("Rejected".to_string(), Color::Red),
            ApprovalStatus::Pending => ("Pending".to_string(), Color::Yellow),
        };
        
        // Create a line with tool name and status on the same line
        let tool_status_line = Line::from(vec![
            ratatui::text::Span::styled(format!("Tool {}", tool_name), Style::default().fg(Color::DarkGray)),
            ratatui::text::Span::styled("                  ", Style::default()), // spacing
            ratatui::text::Span::styled("Status ", Style::default().fg(Color::DarkGray)),
            ratatui::text::Span::styled(status_text, Style::default().fg(status_color)),
        ]);
        lines.push((tool_status_line, Style::default()));
        
        if !tool_path.is_empty() {
            lines.push((
                Line::from(tool_path),
                Style::default().fg(Color::DarkGray),
            ));
        }
        
        lines.push((Line::from(""), Style::default()));
        lines.push((Line::from(""), Style::default()));
        
        // Parse and display tool call arguments in a more readable format
        if let Ok(args) = serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments) {
            // Display each argument on its own line
            for (key, value) in args.as_object().unwrap_or(&serde_json::Map::new()) {
                let value_str = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                
                let arg_line = Line::from(vec![
                    ratatui::text::Span::styled(format!("{} ", key), Style::default().fg(Color::DarkGray)),
                    ratatui::text::Span::styled(value_str, Style::default().fg(Color::Gray)),
                ]);
                lines.push((arg_line, Style::default()));
            }
        } else {
            // Fallback: display raw arguments
            let arguments = tool_call.function.arguments.clone();
            let arg_line = Line::from(vec![
                ratatui::text::Span::styled("Arguments ".to_string(), Style::default().fg(Color::DarkGray)),
                ratatui::text::Span::styled(arguments, Style::default().fg(Color::Gray)),
            ]);
            lines.push((arg_line, Style::default()));
        }
        
        // add simulated dummy text in lines 
        for _ in 0..50 {
            lines.push((
                Line::from("This is a simulated dummy text"),
                Style::default().fg(Color::Gray),
            ));
        }
        lines.push((
            Line::from("This is a simulated THIS IS THE END OF DUMMY TEXT"),
            Style::default().fg(Color::Gray),
        ));
        
        
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
