# Popup Widget

A flexible popup widget for Ratatui applications with support for tabs, scrolling, and fixed header lines.

## Features

- **Tabs**: Multiple tabs with custom styling
- **Scrolling**: Vertical scrolling with configurable behavior
- **Fixed Header Lines**: Control which parts of content scroll and which remain fixed
- **Responsive**: Adapts to different terminal sizes
- **Styling**: Full control over colors, borders, and text formatting
- **Terminal Detection**: Automatic fallback for unsupported terminals

## Basic Usage

```rust
use popup_widget::{PopupWidget, PopupConfig, PopupPosition, StyledLineContent, Tab};
use ratatui::style::{Color, Style, Modifier};
use ratatui::text::Line;

// Create content
let content = StyledLineContent::new(vec![
    (Line::from("Fixed Header Line 1"), Style::default().fg(Color::Yellow)),
    (Line::from("Fixed Header Line 2"), Style::default().fg(Color::Yellow)),
    (Line::from(""), Style::default()),
    (Line::from("Scrollable Content Line 1"), Style::default().fg(Color::White)),
    (Line::from("Scrollable Content Line 2"), Style::default().fg(Color::White)),
    // ... more scrollable content
]);

// Create tab
let tab = Tab::new(
    "tab1".to_string(),
    "Example".to_string(),
    content,
);

// Create popup with fixed header lines
let config = PopupConfig::new()
    .title("Example Popup")
    .show_tabs(true)
    .fixed_header_lines(3) // First 3 lines won't scroll
    .add_tab(tab)
    .position(PopupPosition::Responsive {
        width_percent: 0.8,
        height_percent: 0.7,
        min_width: 30,
        min_height: 20,
    });

let mut popup = PopupWidget::new(config);
popup.show();
```

## Fixed Header Lines

The `fixed_header_lines` feature allows you to specify which lines at the top of the content should remain fixed while the rest scrolls. This is perfect for:

- **Tool Details**: Keep tool name, path, and status visible
- **Headers**: Keep section headers visible
- **Important Info**: Keep critical information always visible

### Example: Tool Call Approval Popup

```rust
// Create popup with tool details that don't scroll
let config = PopupConfig::new()
    .title("Permission Required")
    .fixed_header_lines(8) // Tool details + content header
    .add_tab(tool_call_tab)
    .footer(Some(vec![
        "Space: toggle approve/reject  ↑↓: scroll  ←→: switch tabs".to_string(),
    ]));
```

In this example:
- **Lines 1-8**: Tool details (Tool, Path, Status) and "Content:" header - **FIXED**
- **Lines 9+**: Tool arguments and other content - **SCROLLABLE**

## API Reference

### PopupConfig Methods

- `fixed_header_lines(lines: usize)` - Set number of fixed header lines
- `title(title: impl Into<String>)` - Set popup title
- `show_tabs(show: bool)` - Enable/disable tabs
- `add_tab(tab: Tab)` - Add a tab
- `position(position: PopupPosition)` - Set popup position and size
- `border_style(style: Style)` - Set border styling
- `background_style(style: Style)` - Set background styling

### Events

- `PopupEvent::ScrollUp` / `PopupEvent::ScrollDown` - Scroll content
- `PopupEvent::NextTab` / `PopupEvent::PrevTab` - Switch tabs
- `PopupEvent::Toggle` - Show/hide popup
- `PopupEvent::Escape` - Close popup

## Advanced Example

```rust
use popup_widget::*;
use ratatui::style::{Color, Style, Modifier};
use ratatui::text::Line;

// Create multiple tabs with different content
let mut config = PopupConfig::new()
    .title("Multi-Tab Popup")
    .show_tabs(true)
    .fixed_header_lines(5) // Keep headers fixed
    .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    .background_style(Style::default().bg(Color::Rgb(25, 26, 38)))
    .tab_style(Style::default().fg(Color::White).bg(Color::DarkGray))
    .selected_tab_style(Style::default().fg(Color::White).bg(Color::Cyan))
    .position(PopupPosition::Responsive {
        width_percent: 0.8,
        height_percent: 0.7,
        min_width: 30,
        min_height: 20,
    });

// Add tabs
for i in 0..3 {
    let content = create_tab_content(i);
    let tab = Tab::new(
        format!("tab_{}", i),
        format!("Tab {}", i + 1),
        content,
    );
    config = config.add_tab(tab);
}

let mut popup = PopupWidget::new(config);
```

## License

This project is licensed under the MIT License.