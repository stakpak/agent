use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use std::sync::OnceLock;

use crate::constants::SCROLL_BUFFER_LINES;

#[derive(Debug, Clone)]
pub struct Shortcut {
    pub key: String,
    pub description: String,
    pub category: String,
}

impl Shortcut {
    pub fn new(key: &str, description: &str, category: &str) -> Self {
        Self {
            key: key.to_string(),
            description: description.to_string(),
            category: category.to_string(),
        }
    }
}

pub fn get_all_shortcuts() -> Vec<Shortcut> {
    vec![
        // Navigation
        Shortcut::new("↑/↓", "Navigate messages", "Navigation"),
        Shortcut::new("Page Up/Down", "Page through messages", "Navigation"),
        Shortcut::new("Ctrl+↑/↓", "Navigate dropdown/dialog", "Navigation"),
        Shortcut::new("Tab", "Complete command or select file", "Navigation"),
        Shortcut::new("Esc", "Close dialogs/popups", "Navigation"),
        // Text Input
        Shortcut::new("Ctrl+A", "Move cursor to start of line", "Text Input"),
        Shortcut::new("Ctrl+E", "Move cursor to end of line", "Text Input"),
        Shortcut::new("Ctrl+F", "Move cursor right", "Text Input"),
        Shortcut::new("Ctrl+B", "Move cursor left", "Text Input"),
        Shortcut::new("Alt+F", "Move cursor to next word", "Text Input"),
        Shortcut::new("Alt+B", "Move cursor to previous word", "Text Input"),
        Shortcut::new("Ctrl+U", "Delete to start of line", "Text Input"),
        Shortcut::new("Ctrl+W", "Delete previous word", "Text Input"),
        Shortcut::new("Ctrl+H", "Delete previous character", "Text Input"),
        Shortcut::new("Ctrl+J", "Insert newline", "Text Input"),
        Shortcut::new("Enter", "Submit input", "Text Input"),
        Shortcut::new("Backspace", "Delete previous character", "Text Input"),
        // Tool Management
        Shortcut::new("Ctrl+O", "Toggle auto-approve mode", "Tool Management"),
        Shortcut::new("Ctrl+Y", "Auto-approve current tool", "Tool Management"),
        Shortcut::new("Ctrl+R", "Retry last tool call", "Tool Management"),
        // UI Controls
        Shortcut::new("Ctrl+C", "Quit (double press)", "UI Controls"),
        Shortcut::new("Ctrl+T", "Toggle collapsed messages", "UI Controls"),
        Shortcut::new("Ctrl+L", "Toggle mouse capture", "UI Controls"),
        Shortcut::new("Ctrl+F", "Show profile switcher", "UI Controls"),
        Shortcut::new("Ctrl+P", "Show command palette", "UI Controls"),
        Shortcut::new("Ctrl+S", "Show shortcuts (this popup)", "UI Controls"),
        // Commands
        Shortcut::new("/help", "Show help information", "Commands"),
        Shortcut::new("/clear", "Clear screen", "Commands"),
        Shortcut::new("/status", "Show account status", "Commands"),
        Shortcut::new("/sessions", "List available sessions", "Commands"),
        Shortcut::new("/resume", "Resume last session", "Commands"),
        Shortcut::new("/memorize", "Memorize conversation", "Commands"),
        Shortcut::new("/model", "Switch model (smart/eco)", "Commands"),
        Shortcut::new(
            "/summarize",
            "Summarize session into summary.md",
            "Commands",
        ),
        Shortcut::new("/usage", "Show token usage for this session", "Commands"),
        Shortcut::new(
            "/list_approved_tools",
            "List auto-approved tools",
            "Commands",
        ),
        Shortcut::new(
            "/toggle_auto_approve",
            "Toggle auto-approve for tool",
            "Commands",
        ),
        Shortcut::new("/mouse_capture", "Toggle mouse capture", "Commands"),
        Shortcut::new("/profiles", "Switch profile", "Commands"),
        Shortcut::new("/quit", "Quit application", "Commands"),
        // File Search
        Shortcut::new("@", "Trigger file search", "File Search"),
        Shortcut::new("Tab", "Select file from search", "File Search"),
        // Mouse
        Shortcut::new("Scroll Up/Down", "Scroll messages", "Mouse"),
        Shortcut::new("Click", "Interact with UI elements", "Mouse"),
    ]
}

// Cache the shortcuts content to prevent constant recreation
static SHORTCUTS_CACHE: OnceLock<Vec<Line<'static>>> = OnceLock::new();

pub fn get_cached_shortcuts_content(width: Option<usize>) -> &'static Vec<Line<'static>> {
    SHORTCUTS_CACHE.get_or_init(|| {
        let shortcuts = get_all_shortcuts();

        // Group shortcuts by category
        let mut categories: std::collections::HashMap<&str, Vec<&Shortcut>> =
            std::collections::HashMap::new();
        for shortcut in &shortcuts {
            categories
                .entry(&shortcut.category)
                .or_default()
                .push(shortcut);
        }

        // Define the EXACT order we want categories to appear
        let category_order = vec![
            "Navigation",
            "Text Input",
            "Tool Management",
            "UI Controls",
            "Commands",
            "File Search",
            "Mouse",
        ];

        // Create all lines for the popup
        let mut all_lines = Vec::new();
        // push empty line
        all_lines.push(Line::from(""));

        // Process categories in the EXACT order defined above
        for category_name in &category_order {
            if let Some(category_shortcuts) = categories.get(category_name) {
                // Add category header
                let category_style = Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD);
                let category_width = width.unwrap_or(40) - (category_name.len() + 5);
                all_lines.push(Line::from(vec![
                    Span::styled(format!(" {} ", category_name), category_style),
                    Span::styled(
                        "─".repeat(category_width).to_string(),
                        Style::default().fg(Color::DarkGray),
                    ), // Fixed width to avoid recalculation
                ]));

                // Add shortcuts for this category - FIXED ALIGNMENT
                for shortcut in category_shortcuts {
                    // Use fixed-width formatting for perfect alignment
                    let key_formatted = format!(" {:<25}", shortcut.key); // Left-align in 25 chars
                    let description_formatted = format!("{:<40} ", shortcut.description); // Left-align in 40 chars

                    let spans = vec![
                        Span::styled(
                            key_formatted,
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(description_formatted, Style::default().fg(Color::Reset)),
                    ];

                    all_lines.push(Line::from(spans));
                }

                // Add empty line between categories
                all_lines.push(Line::from(""));
            }
        }

        all_lines
    })
}

/// Get the total count of actual shortcuts (green items only)
pub fn get_shortcuts_count() -> usize {
    get_all_shortcuts().len()
}

pub fn render_shortcuts_popup(f: &mut Frame, state: &mut crate::app::AppState) {
    // Calculate popup size (60% width, fit height to content)
    let area = centered_rect(60, 80, f.area());

    f.render_widget(ratatui::widgets::Clear, area);

    // Create the main block with border and background
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Split area for title, content, scroll indicators, and help text inside the block
    let inner_area = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width - 2,
        height: area.height - 2,
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title
            Constraint::Min(3),    // Content
            Constraint::Length(1), // Scroll indicators
            Constraint::Length(1), // Help text
        ])
        .split(inner_area);

    // Render title inside the popup
    let title = " Keyboard Shortcuts ";
    let title_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let title_line = Line::from(Span::styled(title, title_style));
    let title_paragraph = Paragraph::new(title_line);

    f.render_widget(title_paragraph, chunks[0]);

    // Get all shortcuts lines and calculate scroll info
    let all_lines = get_cached_shortcuts_content(Some(area.width as usize));
    let total_lines = all_lines.len();
    let height = chunks[1].height as usize;
    let shortcuts_count = get_shortcuts_count();

    // Calculate scroll position (similar to collapsed messages)
    let max_scroll = total_lines.saturating_sub(height.saturating_sub(SCROLL_BUFFER_LINES));

    state.shortcuts_scroll = state.shortcuts_scroll.min(max_scroll);
    let scroll = state.shortcuts_scroll;

    // Add top arrow indicator if there are hidden items above
    let mut visible_lines = Vec::new();
    let has_content_above = scroll > 0;
    if has_content_above {
        visible_lines.push(Line::from(vec![Span::styled(
            " ▲",
            Style::default().fg(Color::Reset),
        )]));
    }

    // Create visible lines (similar to collapsed messages)
    for i in 0..height {
        let line_index = scroll + i;
        if line_index < all_lines.len() {
            visible_lines.push(all_lines[line_index].clone());
        } else {
            visible_lines.push(Line::from(""));
        }
    }

    // Render as paragraph with static lines
    let content_paragraph = Paragraph::new(visible_lines)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .style(Style::default().bg(Color::Reset).fg(Color::White));

    f.render_widget(content_paragraph, chunks[1]);

    // Calculate cumulative shortcuts count (including scrolled past ones)
    let mut cumulative_shortcuts_count = 0;

    // Count shortcuts from the beginning up to the current scroll position + visible area
    for line_index in 0..=(scroll + height).min(all_lines.len().saturating_sub(1)) {
        if line_index < all_lines.len() {
            let line = &all_lines[line_index];
            // Check if this line contains a shortcut (green text)
            for span in &line.spans {
                if span.style.fg == Some(Color::Green)
                    && span.style.add_modifier.contains(Modifier::BOLD)
                {
                    cumulative_shortcuts_count += 1;
                    break; // Count each line only once
                }
            }
        }
    }

    // Scroll indicators (above help line)
    let has_content_above = scroll > 0;
    let has_content_below = scroll < max_scroll;

    if has_content_above || has_content_below {
        let mut indicator_spans = vec![];

        // Show cumulative shortcuts counter and down arrow on the left
        indicator_spans.push(Span::styled(
            format!(" ({}/{})", cumulative_shortcuts_count, shortcuts_count),
            Style::default().fg(Color::Reset),
        ));

        if has_content_below {
            indicator_spans.push(Span::styled(" ▼", Style::default().fg(Color::DarkGray)));
        }

        let indicator_paragraph = Paragraph::new(Line::from(indicator_spans));
        f.render_widget(indicator_paragraph, chunks[2]);
    } else {
        // Empty line when no scroll indicators
        f.render_widget(Paragraph::new(""), chunks[2]);
    }

    // Help text (clean, without scroll indicators)
    let help = Paragraph::new(Line::from(vec![
        Span::styled(" ↑/↓", Style::default().fg(Color::Yellow)),
        Span::raw(": Scroll  "),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::raw(": Close"),
    ]));

    f.render_widget(help, chunks[3]);

    // Render the border with title last (so it's on top)
    f.render_widget(block, area);
}

/// Helper function to create a centered rectangle
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
