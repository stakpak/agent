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
        // Get the commands to show
        let commands_to_show = if state.input() == "/" {
            &state.helpers
        } else {
            &state.filtered_helpers
        };

        if commands_to_show.is_empty() {
            return;
        }

        let total_commands = commands_to_show.len();
        const MAX_VISIBLE_ITEMS: usize = 5;
        let visible_height = MAX_VISIBLE_ITEMS.min(total_commands);

        // Create a compact area for the dropdown (matching view.rs calculation)
        let has_content_above = state.helper_scroll > 0;
        let has_content_below = state.helper_scroll < total_commands.saturating_sub(visible_height);
        let arrow_lines =
            if has_content_above { 1 } else { 0 } + if has_content_below { 1 } else { 0 };
        let counter_line = if has_content_above || has_content_below {
            1
        } else {
            0
        };
        let compact_height = (visible_height + arrow_lines + counter_line) as u16;

        let compact_area = Rect {
            x: dropdown_area.x,
            y: dropdown_area.y,
            width: dropdown_area.width,
            height: compact_height,
        };

        // Calculate scroll position
        let max_scroll = total_commands.saturating_sub(visible_height);
        let scroll = if state.helper_scroll > max_scroll {
            max_scroll
        } else {
            state.helper_scroll
        };

        // Find the longest command display name to calculate padding
        let max_command_length = commands_to_show
            .iter()
            .map(|h| h.display().len())
            .max()
            .unwrap_or(0);

        // Create visible lines with scroll indicators
        let mut visible_lines = Vec::new();

        // Add top arrow indicator if there are hidden items above
        let has_content_above = scroll > 0;
        if has_content_above {
            visible_lines.push(Line::from(vec![Span::styled(
                " ▲",
                Style::default().fg(Color::DarkGray),
            )]));
        }

        // Create exactly the number of visible lines (no extra spacing)
        for i in 0..visible_height {
            let line_index = scroll + i;
            if line_index < total_commands {
                let command = &commands_to_show[line_index];
                let display_text = command.display();
                let padding_needed = max_command_length - display_text.len();
                let padding = " ".repeat(padding_needed);
                let is_selected = line_index == state.helper_selected;

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

                let spans = vec![
                    Span::styled(format!("  {}  ", display_text), command_style),
                    Span::styled(padding, Style::default().fg(Color::DarkGray)),
                    Span::styled(format!(" – {}", command.description()), description_style),
                ];

                visible_lines.push(Line::from(spans));
            } else {
                visible_lines.push(Line::from(""));
            }
        }

        // Add bottom arrow indicator if there are hidden items below
        if has_content_below {
            visible_lines.push(Line::from(vec![Span::styled(
                " ▼",
                Style::default().fg(Color::DarkGray),
            )]));
        }

        // Calculate current selected item position (1-based)
        let current_position = state.helper_selected + 1;

        // Create navigation indicators
        let mut indicator_spans = vec![];

        if has_content_above || has_content_below {
            // Show current position counter
            indicator_spans.push(Span::styled(
                format!(" ({}/{})", current_position, total_commands),
                Style::default().fg(Color::Reset),
            ));
        }

        // Add counter as a separate line if needed
        if !indicator_spans.is_empty() {
            visible_lines.push(Line::from(indicator_spans));
        }

        // Render the content using a List widget for more compact display
        let items: Vec<ListItem> = visible_lines.into_iter().map(ListItem::new).collect();

        let list = List::new(items)
            .block(Block::default())
            .style(Style::default().bg(Color::Reset).fg(Color::White));

        f.render_widget(list, compact_area);
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
