use ratatui::{layout::Rect, style::Style, text::Line, widgets::Paragraph, Frame};

/// Trait for popup content that can be rendered
pub trait PopupContent: std::fmt::Debug {
    /// Render the content for the given area with scroll offset
    fn render(&self, f: &mut Frame, area: Rect, scroll: usize);

    /// Get the height needed for the content (used for scrolling calculations)
    fn height(&self) -> usize;

    /// Get the width needed for the content
    fn width(&self) -> usize;

    /// Get the raw lines of content for text wrapping calculations
    fn get_lines(&self) -> Vec<String>;

    /// Calculate the actual rendered height with text wrapping
    fn calculate_rendered_height(&self) -> usize;

    /// Clone the content (required for trait objects)
    fn clone_box(&self) -> Box<dyn PopupContent + Send + Sync>;

    /// Get a reference to the concrete type for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Content that renders styled lines
#[derive(Debug, Clone)]
pub struct StyledLineContent {
    pub lines: Vec<(Line<'static>, Style)>,
    pub width: usize,
    pub height: usize,
}

impl StyledLineContent {
    pub fn new(lines: Vec<(Line<'static>, Style)>) -> Self {
        let height = lines.len();
        let width = lines
            .iter()
            .map(|(line, _)| line.width())
            .max()
            .unwrap_or(0);

        Self {
            lines,
            width,
            height,
        }
    }
}

impl PopupContent for StyledLineContent {
    fn render(&self, f: &mut Frame, area: Rect, scroll: usize) {
        let styled_lines: Vec<Line> = self
            .lines
            .iter()
            .skip(scroll) // Skip lines based on scroll offset
            .map(|(line, style)| line.clone().patch_style(*style))
            .collect();

        let widget = Paragraph::new(styled_lines)
            .style(ratatui::style::Style::default().bg(ratatui::style::Color::Rgb(24, 25, 36)))
            .wrap(ratatui::widgets::Wrap { trim: false });

        f.render_widget(widget, area);
    }

    fn height(&self) -> usize {
        self.height
    }

    fn width(&self) -> usize {
        self.width
    }

    fn get_lines(&self) -> Vec<String> {
        self.lines
            .iter()
            .map(|(line, _)| line.to_string())
            .collect()
    }

    /// Calculate the actual rendered height with text wrapping
    fn calculate_rendered_height(&self) -> usize {
        // Simple approach: just use the raw line count without complex wrapping calculation
        let raw_line_count = self.lines.len();

        // Just use raw line count - no adjustments needed
        raw_line_count
    }

    fn clone_box(&self) -> Box<dyn PopupContent + Send + Sync> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Trait for tab content that can be rendered
pub trait TabContent: PopupContent {
    /// Get the tab title
    fn title(&self) -> &str;

    /// Get the tab identifier
    fn id(&self) -> &str;
}

/// Simple text content implementation
#[derive(Debug, Clone)]
pub struct TextContent {
    pub text: String,
    pub width: usize,
    pub height: usize,
}

impl TextContent {
    pub fn new(text: String) -> Self {
        let lines = text.lines().count();
        let max_width = text.lines().map(|line| line.len()).max().unwrap_or(0);
        Self {
            text,
            width: max_width,
            height: lines,
        }
    }
}

impl PopupContent for TextContent {
    fn render(&self, f: &mut Frame, area: Rect, scroll: usize) {
        use ratatui::text::Line;
        use ratatui::widgets::Paragraph;

        let lines: Vec<Line> = self
            .text
            .lines()
            .skip(scroll) // Skip lines based on scroll offset
            .map(|line| Line::from(line))
            .collect();

        let widget = Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false });

        f.render_widget(widget, area);
    }

    fn height(&self) -> usize {
        self.height
    }

    fn width(&self) -> usize {
        self.width
    }

    fn get_lines(&self) -> Vec<String> {
        self.text.lines().map(|s| s.to_string()).collect()
    }

    fn calculate_rendered_height(&self) -> usize {
        // Simple approach: just use the raw line count without complex wrapping calculation
        let lines: Vec<&str> = self.text.lines().collect();
        let raw_line_count = lines.len();

        // Just use raw line count - no adjustments needed
        raw_line_count
    }

    fn clone_box(&self) -> Box<dyn PopupContent + Send + Sync> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
