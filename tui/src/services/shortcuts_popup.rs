use ratatui::{
    layout::{Constraint, Direction, Layout, Rect}, 
    style::{Color, Modifier, Style}, 
    text::{Line, Span}, 
    widgets::{Block, Borders, Paragraph}, 
    Frame
};
use std::sync::OnceLock;

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
        Shortcut::new("Ctrl+P", "Show profile switcher", "UI Controls"),
        Shortcut::new("Ctrl+S", "Show shortcuts (this popup)", "UI Controls"),
        
        // Commands
        Shortcut::new("/help", "Show help information", "Commands"),
        Shortcut::new("/clear", "Clear screen", "Commands"),
        Shortcut::new("/status", "Show account status", "Commands"),
        Shortcut::new("/sessions", "List available sessions", "Commands"),
        Shortcut::new("/resume", "Resume last session", "Commands"),
        Shortcut::new("/memorize", "Memorize conversation", "Commands"),
        Shortcut::new("/list_approved_tools", "List auto-approved tools", "Commands"),
        Shortcut::new("/toggle_auto_approve", "Toggle auto-approve for tool", "Commands"),
        Shortcut::new("/mouse_capture", "Toggle mouse capture", "Commands"),
        Shortcut::new("/switch_profile", "Switch profile", "Commands"),
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
        let mut categories: std::collections::HashMap<String, Vec<&Shortcut>> = std::collections::HashMap::new();
        for shortcut in &shortcuts {
            categories.entry(shortcut.category.clone()).or_default().push(shortcut);
        }
        
        // Create all lines for the popup
        let mut all_lines = Vec::new();
        // push empty line
        all_lines.push(Line::from(""));
        
        for (category, category_shortcuts) in &categories {
            // Add category header
            let category_style = Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD);
            let category_width = width.unwrap_or(40) - (category.len() + 5);
            all_lines.push(Line::from(vec![
                Span::styled(format!(" {} ", category), category_style),
                Span::styled(format!("{}", "─".repeat(category_width)), Style::default().fg(Color::DarkGray)), // Fixed width to avoid recalculation
            ]));
            
            // Add shortcuts for this category
            for shortcut in category_shortcuts {
                // Calculate spacing to align descriptions
                let key_width: usize = shortcut.key.len();
                let max_key_width: usize = 23; // Maximum width for key column
                let spacing = " ".repeat(max_key_width.saturating_sub(key_width));
                
                let spans = vec![
                    Span::styled(format!(" {}", shortcut.key), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::raw(spacing),
                    Span::styled(shortcut.description.clone(), Style::default().fg(Color::Gray)),
                ];
                
                all_lines.push(Line::from(spans));
            }
            
            // Add empty line between categories
            all_lines.push(Line::from(""));
        }
        
        all_lines
    })
}

pub fn render_shortcuts_popup(f: &mut Frame, state: &crate::app::AppState) {
    // Calculate popup size (60% width, fit height to content)
    let area = centered_rect(60, 80, f.area());

    f.render_widget(ratatui::widgets::Clear, area);

    // Create the main block with border and background
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Split area for title, content and help text inside the block
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

    // Get all shortcuts lines
    let all_lines = get_cached_shortcuts_content(Some(area.width as usize));
    let total_lines = all_lines.len();
    let height = chunks[1].height as usize;
    
    // Calculate scroll position (similar to collapsed messages)
    const SCROLL_BUFFER_LINES: usize = 2;
    let max_scroll = total_lines.saturating_sub(height.saturating_sub(SCROLL_BUFFER_LINES));
    let scroll = if state.shortcuts_scroll > max_scroll {
        max_scroll
    } else {
        state.shortcuts_scroll
    };
    
    // Create visible lines (similar to collapsed messages)
    let mut visible_lines = Vec::new();
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

    // Help text
    let help = Paragraph::new(Line::from(vec![
        Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
        Span::raw(": Scroll  "),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::raw(": Close"),
    ]));

    f.render_widget(help, chunks[2]);

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
