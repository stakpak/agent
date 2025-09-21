use popup_widget::{PopupWidget, PopupConfig, PopupPosition, StyledLineContent, Tab};
use ratatui::style::{Color, Style, Modifier};
use crate::services::detect_term::{self, is_unsupported_terminal};
use ratatui::text::Line;

/// Popup service that manages its own state and event handling
pub struct PopupService {
    popup: PopupWidget,
}

impl PopupService {
    /// Create a new popup service
    pub fn new() -> Self {
        Self {
            popup: Self::create_popup(),
        }
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
        let _ = self.popup.handle_event(popup_widget::PopupEvent::ScrollDown);
    }
    
    /// Handle previous tab
    pub fn prev_tab(&mut self) {
        let _ = self.popup.handle_event(popup_widget::PopupEvent::PrevTab);
    }
    
    /// Handle next tab
    pub fn next_tab(&mut self) {
        let _ = self.popup.handle_event(popup_widget::PopupEvent::NextTab);
    }
    
    /// Handle escape
    pub fn escape(&mut self) {
        let _ = self.popup.handle_event(popup_widget::PopupEvent::Escape);
    }

    /// Create the popup with 3 tabs
    fn create_popup() -> PopupWidget {
        // Create styled content for each tab
        let tab1_content = StyledLineContent::new(vec![
            (Line::from("Welcome to Tab 1!"), 
             Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            (Line::from(""), Style::default()),
            (Line::from("This is the first tab of our test popup."), 
             Style::default().fg(Color::White)),
            (Line::from(""), Style::default()),
            (Line::from("Features demonstrated:"), 
             Style::default().fg(Color::Yellow)),
            (Line::from("• Styled text with colors"), 
             Style::default().fg(Color::Green)),
            (Line::from("• Multiple tabs"), 
             Style::default().fg(Color::Magenta)),
            (Line::from("• Keyboard navigation"), 
             Style::default().fg(Color::Blue)),
            (Line::from(""), Style::default()),
            (Line::from("Use Ctrl+P to toggle this popup!"), 
             Style::default().fg(Color::Red).add_modifier(Modifier::ITALIC)),
        ]);
        
        let tab2_content = StyledLineContent::new(vec![
            (Line::from("Tab 2 - Information & Scrolling Test"), 
             Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            (Line::from(""), Style::default()),
            (Line::from("This tab has lots of content to test scrolling!"), 
             Style::default().fg(Color::White)),
            (Line::from(""), Style::default()),
            (Line::from("Navigation:"), 
             Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            (Line::from("• Left/Right arrows: Switch tabs"), 
             Style::default().fg(Color::Green)),
            (Line::from("• Up/Down arrows: Scroll content"), 
             Style::default().fg(Color::Green)),
            (Line::from("• Esc: Close popup"), 
             Style::default().fg(Color::Red)),
            (Line::from(""), Style::default()),
            (Line::from("The popup widget is fully integrated"), 
             Style::default().fg(Color::Blue)),
            (Line::from("with your TUI application!"), 
             Style::default().fg(Color::Blue)),
            (Line::from(""), Style::default()),
            (Line::from("Scroll Test Content:"), 
             Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            (Line::from(""), Style::default()),
            (Line::from("Line 1: This is a test line for scrolling"), 
             Style::default().fg(Color::White)),
            (Line::from("Line 2: Another line to test scroll functionality"), 
             Style::default().fg(Color::Gray)),
            (Line::from("Line 3: More content to make the tab scrollable"), 
             Style::default().fg(Color::White)),
            (Line::from("Line 4: Testing vertical scrolling in popup"), 
             Style::default().fg(Color::Gray)),
            (Line::from("Line 5: This should be scrollable content"), 
             Style::default().fg(Color::White)),
            (Line::from("Line 6: Keep scrolling to see more lines"), 
             Style::default().fg(Color::Gray)),
            (Line::from(""), Style::default()),
            (Line::from("Long Content Test:"), 
             Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            (Line::from(""), Style::default()),
            (Line::from("This is a very long line that should wrap around the popup content area and demonstrate how text wrapping works in the popup widget. The text should automatically wrap to the next line when it reaches the edge of the content area, making it easy to read long content without horizontal scrolling."), 
             Style::default().fg(Color::White)),
            (Line::from(""), Style::default()),
            (Line::from("Another long paragraph with lots of text to test the wrapping functionality. This paragraph contains multiple sentences and should demonstrate how the popup widget handles long text content that needs to be wrapped to fit within the available width of the popup. The wrapping should be smooth and natural, making the content easy to read."), 
             Style::default().fg(Color::Yellow)),
            (Line::from(""), Style::default()),
            (Line::from("Technical details about the popup widget implementation: The popup widget uses Ratatui's text rendering capabilities to handle text wrapping automatically. It supports styled text with different colors and modifiers, and can handle both short and long content efficiently. The scrolling functionality allows users to navigate through content that exceeds the visible area."), 
             Style::default().fg(Color::Green)),
            (Line::from(""), Style::default()),
            (Line::from("More test content to ensure scrolling works properly with wrapped text. This line is intentionally long to test the wrapping behavior and ensure that the popup widget can handle various types of content without issues. The text should wrap naturally and maintain readability."), 
             Style::default().fg(Color::Magenta)),
            (Line::from("Line 7: The popup should handle this well"), 
             Style::default().fg(Color::White)),
            (Line::from("Line 8: More test content for scrolling"), 
             Style::default().fg(Color::Gray)),
            (Line::from("Line 9: This is getting quite long now"), 
             Style::default().fg(Color::White)),
            (Line::from("Line 10: But we need more to test scrolling"), 
             Style::default().fg(Color::Gray)),
            (Line::from("Line 11: The content should scroll smoothly"), 
             Style::default().fg(Color::White)),
            (Line::from("Line 12: Up and down arrows should work"), 
             Style::default().fg(Color::Gray)),
            (Line::from("Line 13: This is a longer line to test word wrapping"), 
             Style::default().fg(Color::White)),
            (Line::from("Line 14: And this line is even longer to test how the popup handles very long lines that might wrap around the screen"), 
             Style::default().fg(Color::Gray)),
            (Line::from("Line 15: More content for scrolling test"), 
             Style::default().fg(Color::White)),
            (Line::from("Line 16: Keep going down to test scroll"), 
             Style::default().fg(Color::Gray)),
            (Line::from("Line 17: This should be scrollable"), 
             Style::default().fg(Color::White)),
            (Line::from("Line 18: Testing the scroll functionality"), 
             Style::default().fg(Color::Gray)),
            (Line::from("Line 19: More lines to make it scrollable"), 
             Style::default().fg(Color::White)),
            (Line::from("Line 20: This is the last line of test content"), 
             Style::default().fg(Color::Gray)),
            (Line::from(""), Style::default()),
            (Line::from("End of scrollable content!"), 
             Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            (Line::from("Use Up/Down arrows to scroll through this content"), 
             Style::default().fg(Color::Cyan)),
        ]);
        
        let tab3_content = StyledLineContent::new(vec![
            (Line::from("Tab 3 - Code Example"), 
             Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            (Line::from(""), Style::default()),
            (Line::from("Here's how to create a popup:"), 
             Style::default().fg(Color::White)),
            (Line::from(""), Style::default()),
            (Line::from("```rust"), 
             Style::default().fg(Color::Gray)),
            (Line::from("let config = PopupConfig::new()"), 
             Style::default().fg(Color::Cyan)),
            (Line::from("    .title(\"My Popup\")"), 
             Style::default().fg(Color::Cyan)),
            (Line::from("    .show_tabs(true)"), 
             Style::default().fg(Color::Cyan)),
            (Line::from("    .add_tab(tab1)"), 
             Style::default().fg(Color::Cyan)),
            (Line::from("    .add_tab(tab2);"), 
             Style::default().fg(Color::Cyan)),
            (Line::from("```"), 
             Style::default().fg(Color::Gray)),
            (Line::from(""), Style::default()),
            (Line::from("That's it! Simple and powerful."), 
             Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]);
        
        // Create tab content wrappers
        let tab1 = Tab::new(
            "tab1".to_string(),
            "Welcome".to_string(),
            TabContent::new("Welcome".to_string(), "tab1".to_string(), tab1_content),
        );
        
        let tab2 = Tab::new(
            "tab2".to_string(),
            "Info".to_string(),
            TabContent::new("Info".to_string(), "tab2".to_string(), tab2_content),
        );
        
        let tab3 = Tab::new(
            "tab3".to_string(),
            "Code".to_string(),
            TabContent::new("Code".to_string(), "tab3".to_string(), tab3_content),
        );
        
        // Create popup configuration
        let config = PopupConfig::new()
            .title("Test Popup - 3 Tabs")
            .title_style(Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD))
            .title_alignment(popup_widget::Alignment::Center) // Center the title
            .border_style(Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD))
            .background_style(Style::default().bg(Color::Rgb(25, 26, 38))) // 24-bit RGB ANSI: ESC[48;2;25;26;38m
            .popup_background_style(Style::default().bg(Color::Rgb(25, 26, 38))) // 24-bit RGB ANSI: ESC[48;2;25;26;38m
            .show_tabs(true)
            .tab_alignment(popup_widget::Alignment::Center) // Center the tabs
            .tab_style(Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray))
            .selected_tab_style(Style::default()
                .fg(Color::White)
                .bg(Color::Magenta))
            .tab_borders(false) // No borders for button-like appearance
            .use_fallback_colors(true) // Enable terminal detection and fallback colors
            .terminal_detector(|| {
                // Use the existing terminal detection from detect_term.rs
                let terminal_info = detect_term::detect_terminal();
                is_unsupported_terminal(&terminal_info.emulator)
            })
            .add_tab(tab1)
            .add_tab(tab2)
            .add_tab(tab3)
            .footer(Some(vec![
                "ESC to close • Ctrl+P to toggle • ← → switch tabs • ↑ ↓ scroll".to_string(),
                "Press 'q' to quit the application".to_string(),
            ]))
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
    
    fn calculate_rendered_height(&self, width: u16) -> usize {
        self.styled_content.calculate_rendered_height(width)
    }
    
    fn clone_box(&self) -> Box<dyn popup_widget::traits::PopupContent + Send + Sync> {
        Box::new(TabContent {
            title: self.title.clone(),
            id: self.id.clone(),
            styled_content: StyledLineContent::new(self.styled_content.lines.clone()),
        })
    }
}
