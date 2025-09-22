pub mod popup;
pub mod position;
pub mod tab;
pub mod traits;

pub use popup::PopupWidget;
pub use position::PopupPosition;
pub use tab::Tab;
pub use traits::{PopupContent, StyledLineContent, TextContent};

use ratatui::style::{Color, Style};

/// Text alignment options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Center,
    Right,
}

/// Configuration for popup appearance and behavior
pub struct PopupConfig {
    /// Whether to show a title
    pub show_title: bool,
    /// Title text (only used if show_title is true)
    pub title: Option<String>,
    /// Title style (color, modifiers, etc.)
    pub title_style: Style,
    /// Title alignment
    pub title_alignment: Alignment,
    /// Border style (color, modifiers, etc.)
    pub border_style: Style,
    /// Whether to show tabs
    pub show_tabs: bool,
    /// Tab alignment
    pub tab_alignment: Alignment,
    /// Tab configuration
    pub tabs: Vec<Tab>,
    /// Currently selected tab index
    pub selected_tab: usize,
    /// Popup position and size
    pub position: PopupPosition,
    /// Background style for content area
    pub background_style: Style,
    /// Popup background style (entire popup area)
    pub popup_background_style: Style,
    /// Tab style (for unselected tabs)
    pub tab_style: Style,
    /// Selected tab style
    pub selected_tab_style: Style,
    /// Whether to show borders around tab buttons
    pub tab_borders: bool,
    /// Whether to use fallback colors for unsupported terminals
    pub use_fallback_colors: bool,
    /// Custom terminal detection function
    pub terminal_detector: Option<Box<dyn Fn() -> bool + Send + Sync>>,
    /// Footer text (optional) - can be multiple lines
    pub footer: Option<Vec<String>>,
    /// Footer style (color, modifiers, etc.)
    pub footer_style: Option<Style>,
    /// Number of fixed lines at the top that should not scroll
    pub fixed_header_lines: usize,
}

impl Default for PopupConfig {
    fn default() -> Self {
        Self {
            show_title: true,
            title: Some("Popup".to_string()),
            title_style: Style::default()
                .fg(Color::White)
                .add_modifier(ratatui::style::Modifier::BOLD),
            title_alignment: Alignment::Center,
            border_style: Style::default().fg(Color::White),
            show_tabs: false,
            tab_alignment: Alignment::Left,
            tabs: Vec::new(),
            selected_tab: 0,
            position: PopupPosition::Centered {
                width: 50,
                height: 20,
            },
            background_style: Style::default(),
            popup_background_style: Style::default(),
            tab_style: Style::default().fg(Color::Gray),
            selected_tab_style: Style::default()
                .fg(Color::Yellow)
                .add_modifier(ratatui::style::Modifier::BOLD),
            tab_borders: false,
            use_fallback_colors: false,
            terminal_detector: None,
            footer: None,
            footer_style: None,
            fixed_header_lines: 0,
        }
    }
}

impl PopupConfig {
    /// Create a new popup configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the title
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the border style
    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    /// Set the border color (convenience method)
    pub fn border_color(mut self, color: Color) -> Self {
        self.border_style = Style::default().fg(color);
        self
    }

    /// Set the title style
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }

    /// Set the title color (convenience method)
    pub fn title_color(mut self, color: Color) -> Self {
        self.title_style = Style::default()
            .fg(color)
            .add_modifier(ratatui::style::Modifier::BOLD);
        self
    }

    /// Set the tab style
    pub fn tab_style(mut self, style: Style) -> Self {
        self.tab_style = style;
        self
    }

    /// Set the selected tab style
    pub fn selected_tab_style(mut self, style: Style) -> Self {
        self.selected_tab_style = style;
        self
    }

    /// Set whether to show borders around tab buttons
    pub fn tab_borders(mut self, show: bool) -> Self {
        self.tab_borders = show;
        self
    }

    /// Set whether to use fallback colors for unsupported terminals
    pub fn use_fallback_colors(mut self, use_fallback: bool) -> Self {
        self.use_fallback_colors = use_fallback;
        self
    }

    /// Set a custom terminal detection function
    pub fn terminal_detector<F>(mut self, detector: F) -> Self
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        self.terminal_detector = Some(Box::new(detector));
        self
    }

    /// Set the background style for content area
    pub fn background_style(mut self, style: Style) -> Self {
        self.background_style = style;
        self
    }

    /// Set the popup background style (entire popup area)
    pub fn popup_background_style(mut self, style: Style) -> Self {
        self.popup_background_style = style;
        self
    }

    /// Enable or disable tabs
    pub fn show_tabs(mut self, show: bool) -> Self {
        self.show_tabs = show;
        self
    }

    /// Add a tab
    pub fn add_tab(mut self, tab: Tab) -> Self {
        self.tabs.push(tab);
        self
    }

    /// Set the position
    pub fn position(mut self, position: PopupPosition) -> Self {
        self.position = position;
        self
    }

    /// Set the footer text (can be multiple lines)
    pub fn footer(mut self, footer: Option<Vec<String>>) -> Self {
        self.footer = footer;
        self
    }

    /// Set the footer style
    pub fn footer_style(mut self, footer_style: Option<Style>) -> Self {
        self.footer_style = footer_style;
        self
    }

    /// Set the title alignment
    pub fn title_alignment(mut self, alignment: Alignment) -> Self {
        self.title_alignment = alignment;
        self
    }

    /// Set the tab alignment
    pub fn tab_alignment(mut self, alignment: Alignment) -> Self {
        self.tab_alignment = alignment;
        self
    }

    /// Set the number of fixed header lines that should not scroll
    pub fn fixed_header_lines(mut self, lines: usize) -> Self {
        self.fixed_header_lines = lines;
        self
    }
}

/// Internal state for the popup widget
#[derive(Debug, Clone)]
pub struct PopupState {
    /// Scroll position for the current tab
    pub scroll: usize,
    /// Whether the popup is visible
    pub visible: bool,
    /// Selected tab index
    pub selected_tab: usize,
}

impl Default for PopupState {
    fn default() -> Self {
        Self {
            scroll: 0,
            visible: false,
            selected_tab: 0,
        }
    }
}

/// Events that the popup can handle
#[derive(Debug, Clone, PartialEq)]
pub enum PopupEvent {
    /// Show the popup
    Show,
    /// Hide the popup
    Hide,
    /// Toggle visibility
    Toggle,
    /// Scroll up
    ScrollUp,
    /// Scroll down
    ScrollDown,
    /// Page up
    PageUp,
    /// Page down
    PageDown,
    /// Switch to next tab
    NextTab,
    /// Switch to previous tab
    PrevTab,
    /// Switch to specific tab
    SwitchTab(usize),
    /// Handle escape key
    Escape,
}

/// Result of handling a popup event
#[derive(Debug, Clone, PartialEq)]
pub enum PopupEventResult {
    /// Event was handled
    Handled,
    /// Event was not handled (popup not visible or not applicable)
    NotHandled,
    /// Popup should be closed
    Close,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::TextContent;

    #[test]
    fn test_popup_config_builder() {
        let config = PopupConfig::new()
            .title("Test Popup")
            .border_color(Color::Red)
            .title_color(Color::Blue)
            .show_tabs(true);

        assert_eq!(config.title, Some("Test Popup".to_string()));
        assert_eq!(config.border_style.fg, Some(Color::Red));
        assert_eq!(config.title_style.fg, Some(Color::Blue));
        assert!(config.show_tabs);
    }

    #[test]
    fn test_popup_widget_creation() {
        let config = PopupConfig::new()
            .title("Test")
            .position(PopupPosition::Centered {
                width: 40,
                height: 10,
            });

        let popup = PopupWidget::new(config);
        assert!(!popup.is_visible());
    }

    #[test]
    fn test_popup_with_content() {
        let config = PopupConfig::new().title("Content Test");
        let content = TextContent::new("Hello, World!".to_string());
        let popup = PopupWidget::with_content(config, content);

        assert!(!popup.is_visible());
    }

    #[test]
    fn test_styled_line_content() {
        use crate::traits::StyledLineContent;
        use ratatui::style::{Color, Style};
        use ratatui::text::Line;

        let lines = vec![
            (Line::from("Hello"), Style::default().fg(Color::Green)),
            (
                Line::from("World"),
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        ];

        let content = StyledLineContent::new(lines);
        assert_eq!(content.height(), 2);
        assert!(content.width() > 0);
    }

    #[test]
    fn test_popup_events() {
        let config = PopupConfig::new().title("Event Test");
        let mut popup = PopupWidget::new(config);

        // Initially not visible
        assert!(!popup.is_visible());

        // Test show event
        let result = popup.handle_event(PopupEvent::Show);
        assert_eq!(result, PopupEventResult::Handled);
        assert!(popup.is_visible());

        // Test hide event
        let result = popup.handle_event(PopupEvent::Hide);
        assert_eq!(result, PopupEventResult::Handled);
        assert!(!popup.is_visible());

        // Test toggle event
        let result = popup.handle_event(PopupEvent::Toggle);
        assert_eq!(result, PopupEventResult::Handled);
        assert!(popup.is_visible());
    }
}
