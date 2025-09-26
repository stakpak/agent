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
    pub custom_title_line: Option<Line<'static>>,
    pub subheader: Option<Vec<(Line<'static>, Style)>>,
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
            custom_title_line: None,
            subheader: None,
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
            custom_title_line: None,
            subheader: None,
        }
    }

    pub fn new_with_custom_title<C: TabContent + Send + Sync + 'static>(
        id: String,
        title: String,
        content: C,
        custom_title_line: Line<'static>,
    ) -> Self {
        Self {
            id,
            title,
            content: Box::new(content),
            scroll: 0,
            status_color: None,
            custom_title_line: Some(custom_title_line),
            subheader: None,
        }
    }

    /// Render the tab content
    pub fn render_content(&self, f: &mut Frame, area: Rect) {
        self.content.render(f, area, self.scroll);
    }

    /// Render the tab content with fixed header lines
    pub fn render_content_with_fixed_header(
        &self,
        f: &mut Frame,
        area: Rect,
        fixed_header_lines: usize,
    ) {
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
            Constraint::Min(1),                            // Scrollable content
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
        if let Some(styled_content) = self
            .content
            .as_any()
            .downcast_ref::<crate::traits::StyledLineContent>()
        {
            let header_lines: Vec<_> = styled_content
                .lines
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
        if let Some(styled_content) = self
            .content
            .as_any()
            .downcast_ref::<crate::traits::StyledLineContent>()
        {
            let scrollable_lines: Vec<_> = styled_content
                .lines
                .iter()
                .skip(fixed_header_lines + self.scroll)
                .map(|(line, style)| line.clone().patch_style(*style))
                .collect();

            let widget =
                Paragraph::new(scrollable_lines).wrap(ratatui::widgets::Wrap { trim: false });
            f.render_widget(widget, area);
        } else {
            // Fallback for other content types
            self.content
                .render(f, area, fixed_header_lines + self.scroll);
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

    /// Set the subheader for this tab
    pub fn set_subheader(&mut self, subheader: Option<Vec<(Line<'static>, Style)>>) {
        self.subheader = subheader;
    }

    /// Get the subheader for this tab
    pub fn get_subheader(&self) -> &Option<Vec<(Line<'static>, Style)>> {
        &self.subheader
    }

    /// Get the height of the subheader (number of lines)
    pub fn subheader_height(&self) -> usize {
        self.subheader
            .as_ref()
            .map(|lines| lines.len())
            .unwrap_or(0)
    }

    /// Create a new tab with subheader
    pub fn new_with_subheader<C: TabContent + Send + Sync + 'static>(
        id: String,
        title: String,
        content: C,
        subheader: Option<Vec<(Line<'static>, Style)>>,
    ) -> Self {
        Self {
            id,
            title,
            content: Box::new(content),
            scroll: 0,
            status_color: None,
            custom_title_line: None,
            subheader,
        }
    }

    pub fn new_with_custom_title_and_subheader<C: TabContent + Send + Sync + 'static>(
        id: String,
        title: String,
        content: C,
        custom_title_line: Line<'static>,
        subheader: Option<Vec<(Line<'static>, Style)>>,
    ) -> Self {
        Self {
            id,
            title,
            content: Box::new(content),
            scroll: 0,
            status_color: None,
            custom_title_line: Some(custom_title_line),
            subheader,
        }
    }
}

/// Custom tab rendering function that creates styled tab buttons with wrapping support
#[allow(clippy::too_many_arguments)]
pub fn render_custom_tabs(
    f: &mut Frame,
    area: Rect,
    tabs: &[Tab],
    selected_tab: usize,
    tab_style: Style,
    selected_tab_style: Style,
    show_borders: bool,
    alignment: crate::Alignment,
    text_between_tabs: Option<&String>,
    text_between_tabs_style: Style,
) {
    if tabs.is_empty() {
        return;
    }

    // Calculate tab spacing and between-tabs width
    let tab_spacing = if text_between_tabs.is_some() { 0 } else { 1 }; // 1 space between tabs
    let between_tabs_width = if let Some(text) = text_between_tabs {
        text.len() as u16
    } else {
        0
    };

    // Calculate individual tab widths
    let tab_widths: Vec<u16> = tabs
        .iter()
        .map(|tab| tab.title.len() as u16 + 2) // text + 1 space padding on each side
        .collect();

    // Check if tabs need to wrap
    let total_tab_width: u16 = tab_widths
        .iter()
        .enumerate()
        .map(|(i, &tab_width)| {
            let after_text_width = if i < tabs.len() - 1 {
                between_tabs_width
            } else {
                0
            };
            tab_width + after_text_width
        })
        .sum::<u16>()
        + (tabs.len() as u16 - 1) * tab_spacing;

    // If tabs fit in one line, use the original logic
    if total_tab_width <= area.width {
        render_tabs_single_line(
            f,
            area,
            tabs,
            selected_tab,
            tab_style,
            selected_tab_style,
            show_borders,
            alignment,
            text_between_tabs,
            text_between_tabs_style,
            tab_widths,
            tab_spacing,
            between_tabs_width,
            total_tab_width,
        );
    } else {
        // Tabs need to wrap to multiple lines
        render_tabs_multi_line(
            f,
            area,
            tabs,
            selected_tab,
            tab_style,
            selected_tab_style,
            show_borders,
            text_between_tabs,
            text_between_tabs_style,
            tab_widths,
            tab_spacing,
            between_tabs_width,
        );
    }
}

/// Render tabs in a single line (original behavior)
#[allow(clippy::too_many_arguments)]
fn render_tabs_single_line(
    f: &mut Frame,
    area: Rect,
    tabs: &[Tab],
    selected_tab: usize,
    tab_style: Style,
    selected_tab_style: Style,
    show_borders: bool,
    alignment: crate::Alignment,
    text_between_tabs: Option<&String>,
    text_between_tabs_style: Style,
    tab_widths: Vec<u16>,
    tab_spacing: u16,
    between_tabs_width: u16,
    total_tab_width: u16,
) {
    // Create constraints based on alignment
    let mut constraints = Vec::new();

    match alignment {
        crate::Alignment::Left => {
            // Tabs aligned to the left
            for (i, _tab) in tabs.iter().enumerate() {
                let tab_width = tab_widths[i];
                constraints.push(Constraint::Length(tab_width));
                if i < tabs.len() - 1 {
                    constraints.push(Constraint::Length(tab_spacing));
                    if between_tabs_width > 0 {
                        constraints.push(Constraint::Length(between_tabs_width));
                    }
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
            for (i, _tab) in tabs.iter().enumerate() {
                let tab_width = tab_widths[i];
                constraints.push(Constraint::Length(tab_width));
                if i < tabs.len() - 1 {
                    constraints.push(Constraint::Length(tab_spacing));
                    if between_tabs_width > 0 {
                        constraints.push(Constraint::Length(between_tabs_width));
                    }
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
            for (i, _tab) in tabs.iter().enumerate() {
                let tab_width = tab_widths[i];
                constraints.push(Constraint::Length(tab_width));
                if i < tabs.len() - 1 {
                    constraints.push(Constraint::Length(tab_spacing));
                    if between_tabs_width > 0 {
                        constraints.push(Constraint::Length(between_tabs_width));
                    }
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

        // Use custom title line if available, otherwise create from title string
        let tab_line = if let Some(ref custom_line) = tab.custom_title_line {
            // Use custom title line with padding
            let mut spans = vec![Span::styled(" ", Style::default())]; // Left padding
            spans.extend(custom_line.spans.iter().cloned());
            spans.push(Span::styled(" ", Style::default())); // Right padding
            Line::from(spans)
        } else {
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
            Line::from(tab_span)
        };

        // Determine style for paragraph (use selected style for selected tab, otherwise default)
        let paragraph_style = if i == selected_tab {
            selected_tab_style
        } else {
            tab_style
        };

        // Create paragraph with optional borders
        let tab_paragraph = if show_borders {
            use ratatui::widgets::{Block, Borders};
            Paragraph::new(tab_line)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .style(paragraph_style),
                )
                .style(paragraph_style)
                .alignment(ratatui::layout::Alignment::Center)
        } else {
            Paragraph::new(tab_line)
                .style(paragraph_style)
                .alignment(ratatui::layout::Alignment::Center)
        };

        f.render_widget(tab_paragraph, tab_area);

        // Move to next area (skip spacing areas)
        area_index += if i < tabs.len() - 1 {
            if between_tabs_width > 0 {
                3 // tab + spacing + arrow
            } else {
                2 // tab + spacing
            }
        } else {
            1 // just the tab
        };
    }

    // Render arrows between tabs in a separate pass
    if let Some(text) = text_between_tabs {
        let mut arrow_index = match alignment {
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

        for _i in 0..tabs.len() - 1 {
            // Skip to tab area
            arrow_index += 1;
            // Skip to spacing area
            arrow_index += 1;
            // Now we're at the arrow area
            if arrow_index < tab_areas.len() {
                let arrow_area = tab_areas[arrow_index];
                // Add minimal padding around the arrow for better spacing
                let arrow_text = text.to_string();
                let arrow_paragraph = Paragraph::new(Line::from(Span::styled(
                    arrow_text,
                    text_between_tabs_style,
                )))
                .alignment(ratatui::layout::Alignment::Center);
                f.render_widget(arrow_paragraph, arrow_area);
            }
            // Move to next tab
            arrow_index += 1;
        }
    }
}

/// Render tabs with wrapping to multiple lines
#[allow(clippy::too_many_arguments)]
fn render_tabs_multi_line(
    f: &mut Frame,
    area: Rect,
    tabs: &[Tab],
    selected_tab: usize,
    tab_style: Style,
    selected_tab_style: Style,
    show_borders: bool,
    text_between_tabs: Option<&String>,
    text_between_tabs_style: Style,
    tab_widths: Vec<u16>,
    tab_spacing: u16,
    between_tabs_width: u16,
) {
    // Group tabs into lines that fit within the area width
    let mut tab_lines: Vec<Vec<usize>> = Vec::new();
    let mut current_line: Vec<usize> = Vec::new();
    let mut current_line_width = 0u16;

    for (i, &tab_width) in tab_widths.iter().enumerate() {
        let required_width = tab_width
            + if !current_line.is_empty() {
                tab_spacing + between_tabs_width
            } else {
                0
            };

        if current_line_width + required_width <= area.width {
            // Tab fits on current line
            current_line.push(i);
            current_line_width += required_width;
        } else {
            // Tab doesn't fit, start new line
            if !current_line.is_empty() {
                tab_lines.push(current_line);
                current_line = Vec::new();
            }
            // Add tab to new line (even if it's too wide, we'll handle it)
            current_line.push(i);
            current_line_width = tab_width;
        }
    }

    // Add the last line if it has tabs
    if !current_line.is_empty() {
        tab_lines.push(current_line);
    }

    // Render each line of tabs
    for (line_index, tab_indices) in tab_lines.iter().enumerate() {
        if line_index >= area.height as usize {
            break; // No more vertical space
        }

        let line_area = Rect {
            x: area.x,
            y: area.y + line_index as u16 + line_index as u16, // Add 1 line spacing between each line
            width: area.width,
            height: 1,
        };

        // Create constraints for this line
        let mut constraints = Vec::new();
        let mut line_width = 0u16;

        for (i, &tab_index) in tab_indices.iter().enumerate() {
            let tab_width = tab_widths[tab_index];
            constraints.push(Constraint::Length(tab_width));
            line_width += tab_width;

            if i < tab_indices.len() - 1 {
                constraints.push(Constraint::Length(tab_spacing));
                line_width += tab_spacing;
                if between_tabs_width > 0 {
                    constraints.push(Constraint::Length(between_tabs_width));
                    line_width += between_tabs_width;
                }
            }
        }

        // Add remaining space to fill the line
        if line_width < area.width {
            constraints.push(Constraint::Min(0));
        }

        let tab_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(line_area);

        let mut area_index = 0;

        // Render tabs on this line
        for (i, &tab_index) in tab_indices.iter().enumerate() {
            let tab_area = tab_areas[area_index];
            let tab = &tabs[tab_index];

            // Use custom title line if available, otherwise create from title string
            let tab_line = if let Some(ref custom_line) = tab.custom_title_line {
                // Use custom title line with padding
                let mut spans = vec![Span::styled(" ", Style::default())]; // Left padding
                spans.extend(custom_line.spans.iter().cloned());
                spans.push(Span::styled(" ", Style::default())); // Right padding
                Line::from(spans)
            } else {
                // Style based on selection and status
                let tab_style_to_use = if tab_index == selected_tab {
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
                Line::from(tab_span)
            };

            // Determine style for paragraph (use selected style for selected tab, otherwise default)
            let paragraph_style = if tab_index == selected_tab {
                selected_tab_style
            } else {
                tab_style
            };

            // Create paragraph with optional borders
            let tab_paragraph = if show_borders {
                use ratatui::widgets::{Block, Borders};
                Paragraph::new(tab_line)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .style(paragraph_style),
                    )
                    .style(paragraph_style)
                    .alignment(ratatui::layout::Alignment::Center)
            } else {
                Paragraph::new(tab_line)
                    .style(paragraph_style)
                    .alignment(ratatui::layout::Alignment::Center)
            };

            f.render_widget(tab_paragraph, tab_area);

            // Move to next area (skip spacing areas)
            area_index += if i < tab_indices.len() - 1 {
                if between_tabs_width > 0 {
                    3 // tab + spacing + arrow
                } else {
                    2 // tab + spacing
                }
            } else {
                1 // just the tab
            };
        }

        // Render arrows between tabs on this line
        if let Some(text) = text_between_tabs {
            let mut arrow_index = 0;

            for _i in 0..tab_indices.len() - 1 {
                // Skip to tab area
                arrow_index += 1;
                // Skip to spacing area
                arrow_index += 1;
                // Now we're at the arrow area
                if arrow_index < tab_areas.len() {
                    let arrow_area = tab_areas[arrow_index];
                    // Add minimal padding around the arrow for better spacing
                    let arrow_text = text.to_string();
                    let arrow_paragraph = Paragraph::new(Line::from(Span::styled(
                        arrow_text,
                        text_between_tabs_style,
                    )))
                    .alignment(ratatui::layout::Alignment::Center);
                    f.render_widget(arrow_paragraph, arrow_area);
                }
                // Move to next tab
                arrow_index += 1;
            }
        }
    }
}

/// Calculate the number of lines needed to render tabs with wrapping
pub fn calculate_tab_lines_needed(
    tabs: &[Tab],
    area_width: u16,
    text_between_tabs: Option<&String>,
) -> usize {
    if tabs.is_empty() {
        return 1;
    }

    let tab_spacing = if text_between_tabs.is_some() { 0 } else { 1 };
    let between_tabs_width = if let Some(text) = text_between_tabs {
        text.len() as u16
    } else {
        0
    };

    // Calculate individual tab widths
    let tab_widths: Vec<u16> = tabs
        .iter()
        .map(|tab| tab.title.len() as u16 + 2) // text + 1 space padding on each side
        .collect();

    // Group tabs into lines that fit within the area width
    let mut tab_lines: Vec<Vec<usize>> = Vec::new();
    let mut current_line: Vec<usize> = Vec::new();
    let mut current_line_width = 0u16;

    for (i, &tab_width) in tab_widths.iter().enumerate() {
        let required_width = tab_width
            + if !current_line.is_empty() {
                tab_spacing + between_tabs_width
            } else {
                0
            };

        if current_line_width + required_width <= area_width {
            // Tab fits on current line
            current_line.push(i);
            current_line_width += required_width;
        } else {
            // Tab doesn't fit, start new line
            if !current_line.is_empty() {
                tab_lines.push(current_line);
                current_line = Vec::new();
            }
            // Add tab to new line (even if it's too wide, we'll handle it)
            current_line.push(i);
            current_line_width = tab_width;
        }
    }

    // Add the last line if it has tabs
    if !current_line.is_empty() {
        tab_lines.push(current_line);
    }

    let lines_needed = tab_lines.len().max(1); // Always return at least 1 line
                                               // Add spacing between lines: if we have n lines, we need n-1 spacing lines
    if lines_needed > 1 {
        lines_needed + (lines_needed - 1) // Add spacing between each line
    } else {
        lines_needed
    }
}
