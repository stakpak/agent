use crate::{app::AppState, services::detect_term::AdaptiveColors};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

fn term_color(color: Color) -> Color {
    if crate::services::detect_term::should_use_rgb_colors() {
        color
    } else {
        Color::Reset
    }
}

pub fn render_helper_dropdown(f: &mut Frame, state: &AppState, dropdown_area: Rect) {
    let input = state.input().trim();
    let show = input == "/" || (input.starts_with('/') && !state.filtered_helpers.is_empty());
    if state.show_helper_dropdown && show {
        use ratatui::widgets::{List, ListItem, ListState};
        let item_style = Style::default();
        // Find the longest command name to calculate padding
        let commands_to_show = if state.input() == "/" {
            &state.helpers
        } else {
            &state.filtered_helpers
        };

        let max_command_length = commands_to_show
            .iter()
            .map(|h| h.command.len())
            .max()
            .unwrap_or(0);

        let items: Vec<ListItem> = commands_to_show
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let padding_needed = max_command_length - h.command.len();
                let padding = " ".repeat(padding_needed);
                let is_selected = i == state.helper_selected;

                let command_style = if is_selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Cyan)
                };

                let description_style = if is_selected {
                    Style::default()
                        .fg(term_color(Color::Black))
                        .bg(AdaptiveColors::text())
                } else {
                    Style::default().fg(AdaptiveColors::text())
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {}  ", h.command), command_style),
                    Span::styled(padding, Style::default().fg(Color::DarkGray)),
                    Span::styled(format!(" – {}", h.description), description_style),
                ]))
                .style(item_style)
            })
            .collect();
        // No background block
        let mut list_state = ListState::default();
        list_state.select(Some(
            state.helper_selected.min(items.len().saturating_sub(1)),
        ));
        let dropdown_widget = List::new(items).block(Block::default());
        f.render_stateful_widget(dropdown_widget, dropdown_area, &mut list_state);
    }
}

pub fn render_file_search_dropdown(f: &mut Frame, state: &AppState, area: Rect) {
    if !state.show_helper_dropdown {
        return;
    }
    if !state.filtered_files.is_empty() {
        render_file_dropdown(f, state, area);
    } else if !state.filtered_helpers.is_empty() {
        render_helper_dropdown(f, state, area);
    }
}

fn render_file_dropdown(f: &mut Frame, state: &AppState, area: Rect) {
    let files = state.file_search.get_filtered_files();
    if files.is_empty() {
        return;
    }

    // Set title and styling based on trigger
    let (title, title_color) = match state.file_search.trigger_char {
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
                Style::default().fg(AdaptiveColors::text())
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

    match filename.split('.').next_back() {
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
