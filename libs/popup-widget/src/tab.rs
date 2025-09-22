use crate::traits::TabContent;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// A tab in the popup widget
pub struct Tab {
    pub id: String,
    pub title: String,
    pub content: Box<dyn TabContent + Send + Sync>,
    pub scroll: usize,
    pub status_color: Option<Color>,
}

impl Tab {
    pub fn new<C: TabContent + Send + Sync + 'static>(
        id: String,
        title: String,
        content: C,
    ) -> Self {
        Self {
            id,
            title,
            content: Box::new(content),
            scroll: 0,
            status_color: None,
        }
    }

    pub fn new_with_status<C: TabContent + Send + Sync + 'static>(
        id: String,
        title: String,
        content: C,
        status_color: Option<Color>,
    ) -> Self {
        Self {
            id,
            title,
            content: Box::new(content),
            scroll: 0,
            status_color,
        }
    }

    /// Render the tab content
    pub fn render_content(&self, f: &mut Frame, area: Rect) {
        self.content.render(f, area, self.scroll);
    }

    /// Render the tab content with fixed header lines
    pub fn render_content_with_fixed_header(&self, f: &mut Frame, area: Rect, fixed_header_lines: usize) {
        if fixed_header_lines == 0 {
            // No fixed header, render normally
            self.content.render(f, area, self.scroll);
            return;
        }

        // Get all lines from content
        let all_lines = self.content.get_lines();
        let total_lines = all_lines.len();
        
        if total_lines <= fixed_header_lines {
            // Not enough content for scrolling, just render everything
            self.content.render(f, area, 0);
            return;
        }

        // Split the area into fixed header and scrollable content
        let constraints = vec![
            Constraint::Length(fixed_header_lines as u16), // Fixed header
            Constraint::Min(1), // Scrollable content
        ];

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // Render fixed header (first few lines without scroll)
        let header_area = chunks[0];
        self.render_fixed_header(f, header_area, fixed_header_lines);

        // Render scrollable content (remaining lines with scroll)
        let content_area = chunks[1];
        self.render_scrollable_content(f, content_area, fixed_header_lines);
    }

    /// Render only the fixed header lines
    fn render_fixed_header(&self, f: &mut Frame, area: Rect, fixed_header_lines: usize) {
        // Create a custom content that only renders the first fixed_header_lines
        if let Some(styled_content) = self.content.as_any().downcast_ref::<crate::traits::StyledLineContent>() {
            let header_lines: Vec<_> = styled_content.lines
                .iter()
                .take(fixed_header_lines)
                .map(|(line, style)| line.clone().patch_style(*style))
                .collect();
            
            let widget = Paragraph::new(header_lines).wrap(ratatui::widgets::Wrap { trim: false });
            f.render_widget(widget, area);
        } else {
            // Fallback for other content types
            self.content.render(f, area, 0);
        }
    }

    /// Render only the scrollable content lines
    fn render_scrollable_content(&self, f: &mut Frame, area: Rect, fixed_header_lines: usize) {
        // Create a custom content that renders from fixed_header_lines + scroll onwards
        if let Some(styled_content) = self.content.as_any().downcast_ref::<crate::traits::StyledLineContent>() {
            let scrollable_lines: Vec<_> = styled_content.lines
                .iter()
                .skip(fixed_header_lines + self.scroll)
                .map(|(line, style)| line.clone().patch_style(*style))
                .collect();
            
            let widget = Paragraph::new(scrollable_lines).wrap(ratatui::widgets::Wrap { trim: false });
            f.render_widget(widget, area);
        } else {
            // Fallback for other content types
            self.content.render(f, area, fixed_header_lines + self.scroll);
        }
    }

    /// Update scroll position
    pub fn set_scroll(&mut self, scroll: usize) {
        self.scroll = scroll;
    }

    /// Get current scroll position
    pub fn get_scroll(&self) -> usize {
        self.scroll
    }
}

/// Custom tab rendering function that creates styled tab buttons
pub fn render_custom_tabs(
    f: &mut Frame,
    area: Rect,
    tabs: &[Tab],
    selected_tab: usize,
    tab_style: Style,
    selected_tab_style: Style,
    show_borders: bool,
    alignment: crate::Alignment,
) {
    if tabs.is_empty() {
        return;
    }

    // Calculate total width needed for all tabs with spacing
    let tab_spacing = 1; // 1 space between tabs
    let total_tab_width: u16 = tabs
        .iter()
        .map(|tab| tab.title.len() as u16 + 2) // text + 1 space padding on each side
        .sum::<u16>()
        + (tabs.len() as u16 - 1) * tab_spacing;

    // Create constraints based on alignment
    let mut constraints = Vec::new();

    match alignment {
        crate::Alignment::Left => {
            // Tabs aligned to the left
            for (i, tab) in tabs.iter().enumerate() {
                let tab_width = tab.title.len() as u16 + 2; // text + padding
                constraints.push(Constraint::Length(tab_width));
                if i < tabs.len() - 1 {
                    constraints.push(Constraint::Length(tab_spacing));
                }
            }
            // Add remaining space to fill the area
            if total_tab_width < area.width {
                constraints.push(Constraint::Min(0));
            }
        }
        crate::Alignment::Center => {
            // Center the tabs
            if total_tab_width < area.width {
                let remaining_space = area.width - total_tab_width;
                constraints.push(Constraint::Length(remaining_space / 2));
            }
            for (i, tab) in tabs.iter().enumerate() {
                let tab_width = tab.title.len() as u16 + 2; // text + padding
                constraints.push(Constraint::Length(tab_width));
                if i < tabs.len() - 1 {
                    constraints.push(Constraint::Length(tab_spacing));
                }
            }
            // Add remaining space to fill the area
            if total_tab_width < area.width {
                let remaining_space = area.width - total_tab_width;
                constraints.push(Constraint::Length(remaining_space - remaining_space / 2));
            }
        }
        crate::Alignment::Right => {
            // Tabs aligned to the right
            if total_tab_width < area.width {
                constraints.push(Constraint::Min(0));
            }
            for (i, tab) in tabs.iter().enumerate() {
                let tab_width = tab.title.len() as u16 + 2; // text + padding
                constraints.push(Constraint::Length(tab_width));
                if i < tabs.len() - 1 {
                    constraints.push(Constraint::Length(tab_spacing));
                }
            }
        }
    }

    let tab_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    // Calculate starting index based on alignment
    let mut area_index = match alignment {
        crate::Alignment::Left => 0,
        crate::Alignment::Center => {
            if total_tab_width < area.width {
                1
            } else {
                0
            }
        }
        crate::Alignment::Right => {
            if total_tab_width < area.width {
                1
            } else {
                0
            }
        }
    };

    for (i, tab) in tabs.iter().enumerate() {
        let tab_area = tab_areas[area_index];

        // Style based on selection and status
        let tab_style_to_use = if i == selected_tab {
            // Selected tab: use selected style (white text)
            selected_tab_style
        } else {
            // Non-selected tab: use status color if available, otherwise default tab style
            if let Some(status_color) = tab.status_color {
                Style::default().fg(status_color)
            } else {
                tab_style
            }
        };

        // Create tab button with padding and centered text
        let tab_text = format!(" {} ", tab.title);
        let tab_span = Span::styled(tab_text, tab_style_to_use);
        let tab_line = Line::from(tab_span);

        // Create paragraph with optional borders
        let tab_paragraph = if show_borders {
            use ratatui::widgets::{Block, Borders};
            Paragraph::new(tab_line)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .style(tab_style_to_use),
                )
                .style(tab_style_to_use)
                .alignment(ratatui::layout::Alignment::Center)
        } else {
            Paragraph::new(tab_line)
                .style(tab_style_to_use)
                .alignment(ratatui::layout::Alignment::Center)
        };

        f.render_widget(tab_paragraph, tab_area);

        // Move to next area (skip spacing areas)
        area_index += if i < tabs.len() - 1 { 2 } else { 1 };
    }
}
