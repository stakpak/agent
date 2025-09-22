use crate::{
    tab::render_custom_tabs, Alignment, PopupConfig, PopupContent, PopupEvent, PopupEventResult,
    PopupState,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

/// The main popup widget
pub struct PopupWidget {
    config: PopupConfig,
    state: PopupState,
    content: Option<Box<dyn PopupContent + Send + Sync>>,
}

impl PopupWidget {
    /// Create a new popup widget with the given configuration
    pub fn new(config: PopupConfig) -> Self {
        let mut widget = Self {
            config,
            state: PopupState::default(),
            content: None,
        };

        // Apply fallback colors if needed
        widget.apply_fallback_colors();

        widget
    }

    /// Create a new popup widget with content
    pub fn with_content<C: PopupContent + Send + Sync + 'static>(
        config: PopupConfig,
        content: C,
    ) -> Self {
        let mut widget = Self {
            config,
            state: PopupState::default(),
            content: Some(Box::new(content)),
        };

        // Apply fallback colors if needed
        widget.apply_fallback_colors();

        widget
    }

    /// Set the content for the popup
    pub fn set_content<C: PopupContent + Send + Sync + 'static>(&mut self, content: C) {
        self.content = Some(Box::new(content));
    }

    /// Show the popup
    pub fn show(&mut self) {
        self.state.visible = true;
    }

    /// Hide the popup
    pub fn hide(&mut self) {
        self.state.visible = false;
    }

    /// Toggle popup visibility
    pub fn toggle(&mut self) {
        self.state.visible = !self.state.visible;
    }

    /// Check if popup is visible
    pub fn is_visible(&self) -> bool {
        self.state.visible
    }

    /// Handle events
    pub fn handle_event(&mut self, event: PopupEvent) -> PopupEventResult {
        match event {
            PopupEvent::Show => {
                self.show();
                PopupEventResult::Handled
            }
            PopupEvent::Hide => {
                self.hide();
                PopupEventResult::Handled
            }
            PopupEvent::Toggle => {
                self.toggle();
                PopupEventResult::Handled
            }
            PopupEvent::ScrollUp => {
                if !self.state.visible {
                    return PopupEventResult::NotHandled;
                }
                self.scroll_up();
                PopupEventResult::Handled
            }
            PopupEvent::ScrollDown => {
                if !self.state.visible {
                    return PopupEventResult::NotHandled;
                }
                self.scroll_down();
                PopupEventResult::Handled
            }
            PopupEvent::PageUp => {
                if !self.state.visible {
                    return PopupEventResult::NotHandled;
                }
                self.page_up();
                PopupEventResult::Handled
            }
            PopupEvent::PageDown => {
                if !self.state.visible {
                    return PopupEventResult::NotHandled;
                }
                self.page_down();
                PopupEventResult::Handled
            }
            PopupEvent::NextTab => {
                if !self.state.visible {
                    return PopupEventResult::NotHandled;
                }
                self.next_tab();
                PopupEventResult::Handled
            }
            PopupEvent::PrevTab => {
                if !self.state.visible {
                    return PopupEventResult::NotHandled;
                }
                self.prev_tab();
                PopupEventResult::Handled
            }
            PopupEvent::SwitchTab(index) => {
                if !self.state.visible {
                    return PopupEventResult::NotHandled;
                }
                self.switch_tab(index);
                PopupEventResult::Handled
            }
            PopupEvent::Escape => {
                if !self.state.visible {
                    return PopupEventResult::NotHandled;
                }
                self.hide();
                PopupEventResult::Close
            }
        }
    }

    /// Render the popup widget
    pub fn render(&mut self, f: &mut Frame, terminal_size: Rect) {
        if !self.state.visible {
            return;
        }

        let popup_area = self.config.position.calculate_rect(terminal_size);

        // Clear the popup area
        f.render_widget(Clear, popup_area);

        // Render popup background
        let popup_background = Block::default().style(self.config.popup_background_style);
        f.render_widget(popup_background, popup_area);

        // Check if viewport is too small
        if self.config.position.is_viewport_too_small(terminal_size) {
            self.render_viewport_too_small(f, popup_area);
            return;
        }

        // Calculate and clamp scroll to maximum
        let max_scroll = self.calculate_max_scroll(terminal_size);
        self.state.scroll = self.state.scroll.min(max_scroll);

        // Create the main block
        let block = self.create_block();
        f.render_widget(block, popup_area);

        // Calculate content area
        let content_area = self.calculate_content_area(popup_area);

        if self.config.show_tabs && !self.config.tabs.is_empty() {
            self.render_with_tabs(f, content_area);
        } else {
            self.render_without_tabs(f, content_area);
        }
    }

    /// Create the main block for the popup
    fn create_block(&self) -> Block<'_> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(self.config.border_style)
            .style(self.config.background_style)
    }

    /// Calculate the content area (inside borders with padding)
    fn calculate_content_area(&self, popup_area: Rect) -> Rect {
        Rect {
            x: popup_area.x + 2,                         // 2 for padding (left)
            y: popup_area.y + 2,                         // 2 for padding (top)
            width: popup_area.width.saturating_sub(4),   // 2 for padding (left + right)
            height: popup_area.height.saturating_sub(4), // 2 for padding (top + bottom)
        }
    }

    /// Render popup with tabs
    fn render_with_tabs(&mut self, f: &mut Frame, content_area: Rect) {
        if self.config.tabs.is_empty() {
            return;
        }

        // Split content area into title, tabs, content, and footer
        let mut constraints = vec![];

        // Add title constraint if title is enabled
        if self.config.show_title {
            constraints.push(Constraint::Length(1)); // Title line
            constraints.push(Constraint::Length(1)); // Space after title
        }

        // Add footer constraint if footer is enabled
        let has_footer = self.config.footer.is_some();
        let footer_height = if has_footer {
            self.config
                .footer
                .as_ref()
                .map(|footer| footer.len())
                .unwrap_or(0)
        } else {
            0
        };

        if has_footer {
            constraints.extend([
                Constraint::Length(1),                    // Tab headers
                Constraint::Length(1),                    // Space after tabs
                Constraint::Min(1),                       // Tab content (flexible)
                Constraint::Length(1),                    // Space before footer
                Constraint::Length(footer_height as u16), // Footer lines
            ]);
        } else {
            constraints.extend([
                Constraint::Length(1), // Tab headers
                Constraint::Length(1), // Space after tabs
                Constraint::Min(1),    // Tab content
            ]);
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(content_area);

        let mut chunk_index = 0;

        // Render title if enabled
        if self.config.show_title {
            if let Some(title) = &self.config.title {
                let title_area = chunks[chunk_index];
                let title_style = self
                    .config
                    .title_style
                    .add_modifier(ratatui::style::Modifier::BOLD);
                let title_line = Line::from(Span::styled(title, title_style));
                let alignment = match self.config.title_alignment {
                    Alignment::Left => ratatui::layout::Alignment::Left,
                    Alignment::Center => ratatui::layout::Alignment::Center,
                    Alignment::Right => ratatui::layout::Alignment::Right,
                };
                let title_paragraph = Paragraph::new(title_line)
                    .style(self.config.background_style)
                    .alignment(alignment);
                f.render_widget(title_paragraph, title_area);
            }
            chunk_index += 2; // Skip title and space
        }

        let tab_header_area = chunks[chunk_index];
        let tab_content_area = chunks[chunk_index + 2]; // Skip space after tabs

        // Render tab headers
        self.render_tab_headers(f, tab_header_area);

        // Render selected tab content
        if let Some(selected_tab) = self.config.tabs.get_mut(self.state.selected_tab) {
            // Update tab scroll to match popup scroll
            // The tab's internal scroll handling will account for fixed header lines
            selected_tab.set_scroll(self.state.scroll);
            selected_tab.render_content_with_fixed_header(f, tab_content_area, self.config.fixed_header_lines);
        }

        // Render footer if present
        if let Some(footer_lines) = &self.config.footer {
            let footer_area = if has_footer {
                chunks[chunk_index + 4] // Skip tab headers, space, content, and space
            } else {
                return; // No footer area allocated
            };

            let footer_style = self.config.footer_style
                .unwrap_or_else(|| Style::default().fg(Color::Gray).add_modifier(Modifier::DIM));
            let footer_lines: Vec<Line> = footer_lines
                .iter()
                .map(|line| Line::from(Span::styled(line, footer_style)))
                .collect();
            let footer_paragraph = Paragraph::new(footer_lines).style(self.config.background_style);
            f.render_widget(footer_paragraph, footer_area);
        }
    }

    /// Render popup without tabs
    fn render_without_tabs(&mut self, f: &mut Frame, content_area: Rect) {
        // Split content area into title, content, and footer
        let mut constraints = vec![];

        // Add title constraint if title is enabled
        if self.config.show_title {
            constraints.push(Constraint::Length(1)); // Title line
            constraints.push(Constraint::Length(1)); // Space after title
        }

        // Add footer constraint if footer is enabled
        let has_footer = self.config.footer.is_some();
        let footer_height = if has_footer {
            self.config
                .footer
                .as_ref()
                .map(|footer| footer.len())
                .unwrap_or(0)
        } else {
            0
        };

        if has_footer {
            constraints.push(Constraint::Min(1)); // Content (flexible)
            constraints.push(Constraint::Length(1)); // Space before footer
            constraints.push(Constraint::Length(footer_height as u16)); // Footer lines
        } else {
            constraints.push(Constraint::Min(1)); // Content
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(content_area);

        let mut chunk_index = 0;

        // Render title if enabled
        if self.config.show_title {
            if let Some(title) = &self.config.title {
                let title_area = chunks[chunk_index];
                let title_style = self
                    .config
                    .title_style
                    .add_modifier(ratatui::style::Modifier::BOLD);
                let title_line = Line::from(Span::styled(title, title_style));
                let alignment = match self.config.title_alignment {
                    Alignment::Left => ratatui::layout::Alignment::Left,
                    Alignment::Center => ratatui::layout::Alignment::Center,
                    Alignment::Right => ratatui::layout::Alignment::Right,
                };
                let title_paragraph = Paragraph::new(title_line)
                    .style(self.config.background_style)
                    .alignment(alignment);
                f.render_widget(title_paragraph, title_area);
            }
            chunk_index += 2; // Skip title and space
        }

        // Render content
        if let Some(content) = &self.content {
            let content_area = chunks[chunk_index];
            content.render(f, content_area, self.state.scroll);

            // Render footer if present
            if let Some(footer_lines) = &self.config.footer {
                let footer_area = if has_footer {
                    chunks[chunk_index + 2] // Skip content and space
                } else {
                    return; // No footer area allocated
                };

                let footer_style = self.config.footer_style
                .unwrap_or_else(|| Style::default().fg(Color::Gray).add_modifier(Modifier::DIM));
                let footer_lines: Vec<Line> = footer_lines
                    .iter()
                    .map(|line| Line::from(Span::styled(line, footer_style)))
                    .collect();
                let footer_paragraph =
                    Paragraph::new(footer_lines).style(self.config.background_style);
                f.render_widget(footer_paragraph, footer_area);
            }
        }
    }

    /// Render the "Viewport is too small" message
    fn render_viewport_too_small(&self, f: &mut Frame, _popup_area: Rect) {
        // Get the terminal size to center the message properly
        let terminal_size = f.area();

        // Create a centered message area that's independent of popup constraints
        let message = "Viewport is too small!";
        let message_width = message.len() as u16 + 4; // Add padding for borders
        let message_height = 3; // Height for text + borders

        // Center the message in the terminal, not the popup area
        let x = (terminal_size.width.saturating_sub(message_width)) / 2;
        let y = (terminal_size.height.saturating_sub(message_height)) / 2;

        let message_area = Rect {
            x,
            y,
            width: message_width,
            height: message_height,
        };

        // Create a bordered block for the message
        let message_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(self.config.border_style);

        // Render the message with the same border style as the popup
        let message_paragraph = Paragraph::new(Line::from(Span::styled(
            message,
            Style::default().fg(Color::White),
        )))
        .style(self.config.popup_background_style)
        .block(message_block)
        .alignment(ratatui::layout::Alignment::Center);

        f.render_widget(message_paragraph, message_area);
    }

    /// Render tab headers using custom styled tab buttons
    fn render_tab_headers(&mut self, f: &mut Frame, area: Rect) {
        if self.config.tabs.is_empty() {
            return;
        }

        render_custom_tabs(
            f,
            area,
            &self.config.tabs,
            self.state.selected_tab,
            self.config.tab_style,
            self.config.selected_tab_style,
            self.config.tab_borders,
            self.config.tab_alignment,
        );
    }

    /// Scroll up
    fn scroll_up(&mut self) {
        eprintln!("DEBUG: scroll_up called - current: {}", self.state.scroll);
        if self.state.scroll > 0 {
            self.state.scroll -= 1;
            eprintln!("DEBUG: scroll_up - new: {}", self.state.scroll);
        }
    }

    /// Scroll down
    fn scroll_down(&mut self) {
        eprintln!("DEBUG: scroll_down called - current: {}", self.state.scroll);
        // We'll calculate max scroll in render method and store it in state
        // For now, just increment scroll - it will be clamped in render
        self.state.scroll += 1;
        eprintln!("DEBUG: scroll_down - new: {}", self.state.scroll);
    }

    /// Page up
    fn page_up(&mut self) {
        self.state.scroll = self.state.scroll.saturating_sub(10);
    }

    /// Page down
    fn page_down(&mut self) {
        // We'll calculate max scroll in render method and store it in state
        // For now, just increment scroll - it will be clamped in render
        self.state.scroll += 10;
    }

    /// Switch to next tab
    fn next_tab(&mut self) {
        if !self.config.tabs.is_empty() {
            self.state.selected_tab = (self.state.selected_tab + 1) % self.config.tabs.len();
            self.state.scroll = 0; // Reset scroll when switching tabs
        }
    }

    /// Switch to previous tab
    fn prev_tab(&mut self) {
        if !self.config.tabs.is_empty() {
            self.state.selected_tab = if self.state.selected_tab == 0 {
                self.config.tabs.len() - 1
            } else {
                self.state.selected_tab - 1
            };
            self.state.scroll = 0; // Reset scroll when switching tabs
        }
    }

    /// Calculate the maximum scroll position based on content height
    fn calculate_max_scroll(&self, terminal_size: Rect) -> usize {
        // Calculate the actual popup area
        let popup_area = self.config.position.calculate_rect(terminal_size);

        // Calculate the content area (popup minus borders and padding)
        let content_area = self.calculate_content_area(popup_area);

        // Check if footer is present
        let has_footer = self.config.footer.is_some();

        // Calculate available height for content using the same logic as rendering
        let available_height = if self.config.show_tabs && !self.config.tabs.is_empty() {
            // Calculate the same way as in render_with_tabs
            let mut fixed_height = 0;
            if self.config.show_title {
                fixed_height += 2; // Title + space
            }
            fixed_height += 2; // Tabs + space
            if has_footer {
                let footer_height = self
                    .config
                    .footer
                    .as_ref()
                    .map(|footer| footer.len())
                    .unwrap_or(0);
                fixed_height += 1 + footer_height; // Space before footer + footer lines
            }
            let available = content_area.height.saturating_sub(fixed_height as u16) as usize;
            available
        } else {
            // Calculate the same way as in render_without_tabs
            let mut fixed_height = 0;
            if self.config.show_title {
                fixed_height += 2; // Title + space
            }
            if has_footer {
                let footer_height = self
                    .config
                    .footer
                    .as_ref()
                    .map(|footer| footer.len())
                    .unwrap_or(0);
                fixed_height += 1 + footer_height; // Space before footer + footer lines
            }
            let available = content_area.height.saturating_sub(fixed_height as u16) as usize;
            available
        };

        // Subtract fixed header lines from available height for scrollable content
        let scrollable_available_height = available_height.saturating_sub(self.config.fixed_header_lines);

        // Calculate the actual content height including text wrapping
        let content_height = if self.config.show_tabs && !self.config.tabs.is_empty() {
            if let Some(selected_tab) = self.config.tabs.get(self.state.selected_tab) {
                selected_tab.content.calculate_rendered_height()
            } else {
                0
            }
        } else if let Some(content) = &self.content {
            content.calculate_rendered_height()
        } else {
            0
        };

        // Subtract fixed header lines from content height for scroll calculation
        let scrollable_content_height = content_height.saturating_sub(self.config.fixed_header_lines);

        // Calculate max scroll: if scrollable content is taller than available space, allow scrolling
        let max_scroll = if scrollable_content_height > scrollable_available_height {
            let scroll = scrollable_content_height - scrollable_available_height;
            eprintln!("DEBUG: Scrolling enabled - content: {}, available: {}, max_scroll: {}", 
                     scrollable_content_height, scrollable_available_height, scroll);
            scroll
        } else {
            eprintln!("DEBUG: No scrolling - content: {}, available: {}", 
                     scrollable_content_height, scrollable_available_height);
            0
        };

        max_scroll
    }

    /// Switch to specific tab
    fn switch_tab(&mut self, index: usize) {
        if index < self.config.tabs.len() {
            self.state.selected_tab = index;
            self.state.scroll = 0; // Reset scroll when switching tabs
        }
    }

    /// Get current configuration
    pub fn config(&self) -> &PopupConfig {
        &self.config
    }

    /// Render tab headers with custom styling options
    pub fn render_tab_headers_custom(
        &mut self,
        f: &mut Frame,
        area: Rect,
        tab_style: Style,
        selected_tab_style: Style,
        show_borders: bool,
        alignment: Alignment,
    ) {
        if self.config.tabs.is_empty() {
            return;
        }

        render_custom_tabs(
            f,
            area,
            &self.config.tabs,
            self.state.selected_tab,
            tab_style,
            selected_tab_style,
            show_borders,
            alignment,
        );
    }

    /// Get mutable reference to configuration
    pub fn config_mut(&mut self) -> &mut PopupConfig {
        &mut self.config
    }

    /// Detect if the current terminal supports RGB colors using the existing detect_term module
    fn is_unsupported_terminal() -> bool {
        // Use the existing terminal detection from the TUI service
        // We'll check TERM_PROGRAM and TERM directly since we can't import the TUI service
        let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let term = std::env::var("TERM").unwrap_or_default();

        // Check TERM_PROGRAM first (matches detect_term.rs logic)
        let is_unsupported_by_program = match term_program.as_str() {
            "Apple_Terminal" | "Terminal" => true, // macOS Terminal built-in
            "Terminus" => true,                    // highly configurable terminal emulator
            "Terminology" => true,                 // Enlightenment terminal
            "Hyper" => true,                       // cross-platform, HTML/CSS/JS-based (Electron)
            _ => false,                            // Assume RGB support for unknown terminals
        };

        // Check TERM variable for basic terminals
        let is_unsupported_by_term = match term.as_str() {
            "dumb" => true,            // Very basic terminal
            "unknown" => true,         // Unknown terminal
            "vt100" | "vt220" => true, // Old VT terminals
            _ => false,                // Assume RGB support for other TERM values
        };

        // Return true if either TERM_PROGRAM or TERM indicates unsupported terminal
        is_unsupported_by_program || is_unsupported_by_term
    }

    /// Apply fallback colors for unsupported terminals
    fn apply_fallback_colors(&mut self) {
        // Use custom terminal detector if provided, otherwise use built-in detection
        let is_unsupported = if let Some(detector) = &self.config.terminal_detector {
            detector()
        } else {
            Self::is_unsupported_terminal()
        };

        if self.config.use_fallback_colors && is_unsupported {
            // Reset text color
            self.config.title_style = Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD);

            // Cyan border
            self.config.border_style = Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD);

            // Cyan selected tab
            self.config.selected_tab_style = Style::default()
                .bg(Color::Cyan)
                .fg(Color::Reset)
                .add_modifier(Modifier::BOLD);

            // Indexed(235) background for content and popup
            self.config.background_style = Style::default().bg(Color::Indexed(235));
            self.config.popup_background_style = Style::default().bg(Color::Indexed(235));
        }
    }

    /// Get current state
    pub fn state(&self) -> &PopupState {
        &self.state
    }

    /// Get mutable reference to state
    pub fn state_mut(&mut self) -> &mut PopupState {
        &mut self.state
    }

    /// Set the selected tab index
    pub fn set_selected_tab(&mut self, index: usize) {
        if index < self.config.tabs.len() {
            self.config.selected_tab = index;
            self.state.selected_tab = index;
        }
    }
}
