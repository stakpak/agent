use crate::app::AppState;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

pub fn render_helper_dropdown(f: &mut Frame, state: &AppState, dropdown_area: Rect) {
    if state.show_helper_dropdown
        && !state.filtered_helpers.is_empty()
        && state.input.starts_with('/')
    {
        use ratatui::widgets::{List, ListItem, ListState};
        let item_style = Style::default().bg(Color::Black);
        let items: Vec<ListItem> = if state.input == "/" {
            state
                .helpers
                .iter()
                .map(|h| {
                    ListItem::new(Line::from(vec![Span::raw(format!("  {}  ", h))]))
                        .style(item_style)
                })
                .collect()
        } else {
            state
                .filtered_helpers
                .iter()
                .map(|h| {
                    ListItem::new(Line::from(vec![Span::raw(format!("  {}  ", h))]))
                        .style(item_style)
                })
                .collect()
        };
        let bg_block = Block::default().style(Style::default().bg(Color::Black));
        f.render_widget(bg_block, dropdown_area);
        let mut list_state = ListState::default();
        list_state.select(Some(
            state.helper_selected.min(items.len().saturating_sub(1)),
        ));
        let dropdown_widget = List::new(items)
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .bg(Color::DarkGray),
            )
            .block(Block::default());
        f.render_stateful_widget(dropdown_widget, dropdown_area, &mut list_state);
    }
}

pub fn render_autocomplete_dropdown(f: &mut Frame, state: &AppState, area: Rect) {
    if !state.show_helper_dropdown {
        return;
    }
    // Determine if we're in file autocomplete mode
    let is_file_mode = state.autocomplete.is_active();

    if is_file_mode {
        render_file_dropdown(f, state, area);
    } else if !state.filtered_helpers.is_empty() {
        render_helper_dropdown(f, state, area);
    }
}

fn render_file_dropdown(f: &mut Frame, state: &AppState, area: Rect) {
    let files = state.autocomplete.get_filtered_files();
    if files.is_empty() {
        return;
    }

    // Set title and styling based on trigger
    let (title, title_color) = match state.autocomplete.trigger_char {
        Some('@') => ("📁 Files (@)", Color::Cyan),
        None => ("📁 Files (Tab)", Color::Blue),
        _ => ("📁 Files", Color::Gray),
    };
    let items: Vec<ListItem> = files
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let style = if i == state.helper_selected {
                Style::default()
                    .bg(Color::Cyan)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(160, 160, 160))
            };

            let display_text = format!("{} {}", get_file_icon(item), item);
            ListItem::new(Line::from(Span::styled(display_text, style)))
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(state.helper_selected));

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(title_color)),
    );

    f.render_stateful_widget(list, area, &mut list_state);
}

// Helper function to get file icons based on extension
fn get_file_icon(filename: &str) -> &'static str {
    if filename.ends_with('/') {
        return "📁";
    }

    match filename.split('.').last() {
        Some("rs") => "🦀",
        Some("toml") => "⚙️",
        Some("md") => "📝",
        Some("txt") => "📄",
        Some("json") => "📋",
        Some("js") | Some("ts") => "🟨",
        Some("py") => "🐍",
        Some("html") => "🌐",
        Some("css") => "🎨",
        Some("yml") | Some("yaml") => "📄",
        Some("lock") => "🔒",
        Some("sh") => "💻",
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") => "🖼️",
        _ => "📄",
    }
}
