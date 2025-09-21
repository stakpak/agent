# Popup Widget

A flexible, trait-based popup widget crate for Ratatui applications.

## Features

- **Trait-based content system**: Implement custom content using the `PopupContent` trait
- **Tab support**: Optional tabbed interface with keyboard navigation
- **Flexible positioning**: Centered, absolute, or relative positioning
- **Built-in scrolling**: Automatic scroll handling with keyboard controls
- **Customizable appearance**: Configurable colors, borders, and titles
- **Stateless design**: Easy to show/hide and manage multiple popups
- **Event handling**: Comprehensive event system for user interaction

## Quick Start

```rust
use popup_widget::{PopupWidget, PopupConfig, PopupPosition, TextContent, PopupEvent, StyledLineContent};
use ratatui::style::{Color, Style, Modifier};
use ratatui::text::Line;

// Create configuration with enhanced styling
let config = PopupConfig::new()
    .title("My Popup")
    .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    .background_style(Style::default().bg(Color::Black))
    .show_tabs(false)
    .position(PopupPosition::Centered { width: 50, height: 20 });

// Create popup with styled content
let styled_content = StyledLineContent::new(vec![
    (Line::from("Hello, World!"), Style::default().fg(Color::Green)),
    (Line::from("Styled text"), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
]);

let mut popup = PopupWidget::with_content(config, styled_content);

// Show the popup
popup.show();

// Handle events
let result = popup.handle_event(PopupEvent::ScrollUp);
```

## Configuration

### PopupConfig

```rust
pub struct PopupConfig {
    pub show_title: bool,           // Whether to show a title
    pub title: Option<String>,      // Title text
    pub title_color: Color,         // Title color
    pub border_color: Color,        // Border color
    pub show_tabs: bool,            // Whether to show tabs
    pub tabs: Vec<Tab>,             // Tab configuration
    pub selected_tab: usize,        // Currently selected tab
    pub position: PopupPosition,    // Position and size
    pub background_style: Style,    // Background style
    pub border_style: Style,        // Border style
}
```

### Positioning

```rust
// Centered popup
PopupPosition::Centered { width: 50, height: 20 }

// Absolute position
PopupPosition::Absolute { x: 10, y: 5, width: 40, height: 15 }

// Relative position (percentages)
PopupPosition::Relative { 
    x_percent: 0.1, 
    y_percent: 0.1, 
    width_percent: 0.8, 
    height_percent: 0.8 
}
```

## Custom Content

### StyledLineContent

For content with custom styling, use `StyledLineContent`:

```rust
use popup_widget::StyledLineContent;
use ratatui::text::Line;
use ratatui::style::{Color, Style, Modifier};

let styled_content = StyledLineContent::new(vec![
    (Line::from("Header"), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    (Line::from("Regular text"), Style::default().fg(Color::White)),
    (Line::from("Error message"), Style::default().fg(Color::Red).add_modifier(Modifier::ITALIC)),
]);
```

### Custom Content Types

Implement the `PopupContent` trait for custom content:

```rust
use popup_widget::traits::PopupContent;

struct MyContent {
    data: String,
}

impl PopupContent for MyContent {
    fn render(&self, f: &mut Frame, area: Rect) {
        // Your custom rendering logic
    }
    
    fn height(&self) -> usize {
        // Return content height
    }
    
    fn width(&self) -> usize {
        // Return content width
    }
    
    fn clone_box(&self) -> Box<dyn PopupContent + Send + Sync> {
        Box::new(self.clone())
    }
}
```

## Tab Support

Create tabs with custom content:

```rust
use popup_widget::{Tab, PopupConfig};

let tabs = vec![
    Tab::new(
        "tab1".to_string(),
        "First Tab".to_string(),
        MyContent::new("Tab 1 content".to_string())
    ),
    Tab::new(
        "tab2".to_string(),
        "Second Tab".to_string(),
        MyContent::new("Tab 2 content".to_string())
    ),
];

let config = PopupConfig {
    show_tabs: true,
    tabs,
    ..Default::default()
};
```

## Events

Handle user input with the event system:

```rust
// Show/hide
popup.handle_event(PopupEvent::Show);
popup.handle_event(PopupEvent::Hide);
popup.handle_event(PopupEvent::Toggle);

// Scrolling
popup.handle_event(PopupEvent::ScrollUp);
popup.handle_event(PopupEvent::ScrollDown);
popup.handle_event(PopupEvent::PageUp);
popup.handle_event(PopupEvent::PageDown);

// Tab navigation
popup.handle_event(PopupEvent::NextTab);
popup.handle_event(PopupEvent::PrevTab);
popup.handle_event(PopupEvent::SwitchTab(1));

// Close
popup.handle_event(PopupEvent::Escape);
```

## Integration with Ratatui

```rust
use ratatui::{Frame, Terminal};

// In your render function
fn render(f: &mut Frame, state: &mut AppState) {
    // Render your main content
    // ...
    
    // Render popups
    for popup in &mut state.popups {
        popup.render(f, f.size());
    }
}

// In your event handling
fn handle_key(key: KeyCode, state: &mut AppState) {
    match key {
        KeyCode::Char('p') => {
            state.popups[0].handle_event(PopupEvent::Toggle);
        }
        // ... other key handling
    }
}
```

## Examples

See the `examples/` directory for complete usage examples:

- `basic_usage.rs` - Simple popup with tabs and keyboard controls
- `custom_content.rs` - Custom content implementation
- `multiple_popups.rs` - Managing multiple popups

## Dependencies

- `ratatui` - TUI framework
- `crossterm` - Terminal handling

## License

This crate is part of the Stakpak project.
