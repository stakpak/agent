//! File Changes Popup Rendering
//!
//! Renders the popup showing modified files with revert options.

use crate::app::AppState;
use crate::services::changeset::FileState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};

/// Helper to start centering the rect
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

pub fn render_file_changes_popup(f: &mut Frame, state: &AppState) {
    // Calculate popup size: Height a bit more (30 -> 40), Width less (60 -> 50)
    let area = centered_rect(50, 40, f.area());

    f.render_widget(Clear, area);

    // Create the main block with border and background
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    f.render_widget(block, area);

    // Split area for title, search, content, scroll indicators, andfooter
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
            Constraint::Length(3), // Search
            Constraint::Min(3),    // Content
            Constraint::Length(1), // Footer
        ])
        .split(inner_area);

    // Filter files
    let query = state.file_changes_search.to_lowercase();
    let binding = state.changeset.files_in_order();
    let filtered_files: Vec<_> = binding
        .iter()
        .filter(|file| query.is_empty() || file.display_name().to_lowercase().contains(&query))
        .collect();

    // Render title
    // "Modified Files" in Yellow Bold on left
    // "N files changed" in Cyan on right
    let count = filtered_files.len();
    let count_text = if count == 1 {
        format!("{} file changed", count)
    } else {
        format!("{} files changed", count)
    };

    // Calculate spacing for right alignment
    let available_width = inner_area.width as usize;
    let title_left = " Modified Files";
    let spacing = available_width.saturating_sub(title_left.len() + count_text.len() + 1);

    let title_spans = vec![
        Span::styled(
            title_left,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(spacing)),
        Span::styled(count_text, Style::default().fg(Color::Cyan)),
        Span::raw(" "), // right padding
    ];

    let title = Paragraph::new(Line::from(title_spans));

    f.render_widget(title, chunks[0]);

    // Render search input
    let search_prompt = ">";
    let cursor = "|";
    let placeholder = "Type to filter";

    let search_spans = if state.file_changes_search.is_empty() {
        vec![
            Span::raw(" "),
            Span::styled(search_prompt, Style::default().fg(Color::Magenta)),
            Span::raw(" "),
            Span::styled(cursor, Style::default().fg(Color::Cyan)),
            Span::styled(placeholder, Style::default().fg(Color::DarkGray)),
        ]
    } else {
        vec![
            Span::raw(" "),
            Span::styled(search_prompt, Style::default().fg(Color::Magenta)),
            Span::raw(" "),
            Span::styled(
                &state.file_changes_search,
                Style::default()
                    .fg(Color::Reset)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(cursor, Style::default().fg(Color::Cyan)),
        ]
    };

    let search_paragraph = Paragraph::new(Text::from(vec![
        Line::from(""),
        Line::from(search_spans),
        Line::from(""),
    ]));
    f.render_widget(search_paragraph, chunks[1]);

    // Render Content
    let height = chunks[2].height as usize;
    let total_items = filtered_files.len();
    let scroll = state.file_changes_scroll;

    let mut visible_lines = Vec::new();

    for i in 0..height {
        let idx = scroll + i;
        if idx >= total_items {
            break;
        }

        let file = filtered_files[idx];
        let is_selected = idx == state.file_changes_selected;

        let bg_color = if is_selected {
            Color::Cyan
        } else {
            Color::Reset
        };

        // Unselected file names to DarkGray
        let name_style = match file.state {
            FileState::Reverted | FileState::Deleted | FileState::Removed => Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::CROSSED_OUT),
            _ => {
                let style = if is_selected {
                    Style::default().fg(Color::Black)
                } else {
                    Style::default().fg(Color::Reset)
                };
                style.add_modifier(Modifier::UNDERLINED)
            }
        };

        // Stats
        let added = file.total_lines_added();
        let removed = file.total_lines_removed();

        let added_str = format!("+{}", added);
        let removed_str = format!("-{}", removed);
        let revert_icon = "↩"; // display width usually 1

        // Calculate spacing
        let name = file.display_name();

        // Ensure stats take up fixed width or just right align?
        // Right align within the row: spacing between name and stats.
        // We have 1 char padding left, 1 char padding right.
        // len = 1 + name.len() + spacing + added.len() + 1 + removed.len() + 1 + icon.len() + 1

        // Check if file is reverted
        // Use FileState

        // Check if file is reverted, deleted or removed
        let (stats_spans, stats_len) = if file.state == FileState::Reverted
            || file.state == FileState::Deleted
            || file.state == FileState::Removed
        {
            // Show "REVERTED", "DELETED" or "REMOVED" in dark gray instead of stats
            let state_text = match file.state {
                FileState::Reverted => "REVERTED",
                FileState::Deleted => "DELETED",
                FileState::Removed => "REMOVED",
                _ => "UNKNOWN",
            };
            (
                vec![
                    Span::styled(
                        state_text,
                        Style::default()
                            .fg(if is_selected {
                                Color::Black
                            } else {
                                Color::DarkGray
                            })
                            .bg(bg_color),
                    ),
                    Span::styled(" ", Style::default().bg(bg_color)), // padding Right
                ],
                state_text.len() + 1,
            )
        } else {
            // Show normal stats
            let stats_len_calc = added_str.len() + 1 + removed_str.len() + 1 + 1; // 1 for visual width of "↩"
            (
                vec![
                    Span::styled(
                        added_str,
                        Style::default()
                            .fg(if is_selected {
                                Color::Black
                            } else {
                                Color::Green
                            })
                            .bg(bg_color),
                    ),
                    Span::styled(" ", Style::default().bg(bg_color)),
                    Span::styled(
                        removed_str,
                        Style::default()
                            .fg(if is_selected {
                                Color::Black
                            } else {
                                Color::Red
                            })
                            .bg(bg_color),
                    ),
                    Span::styled(" ", Style::default().bg(bg_color)),
                    Span::styled(
                        revert_icon,
                        Style::default()
                            .fg(if is_selected {
                                Color::Black
                            } else {
                                Color::Blue
                            })
                            .bg(bg_color),
                    ),
                    Span::styled(" ", Style::default().bg(bg_color)), // padding Right
                ],
                stats_len_calc,
            )
        };

        let available_content_width = inner_area.width as usize; // Full inner width

        // Padding L (1) + Name + Spacing + Stats + Padding R (1) = Width
        // Spacing = Width - 2 - Name - Stats
        let spacing = available_content_width.saturating_sub(2 + name.len() + stats_len);

        let mut spans = vec![
            Span::styled(" ", Style::default().bg(bg_color)), // padding Left
            Span::styled(name, name_style.bg(bg_color)),
            Span::styled(" ".repeat(spacing), Style::default().bg(bg_color)),
        ];
        spans.extend(stats_spans);

        visible_lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(visible_lines), chunks[2]);

    // Render Footer
    // Updated shortcuts: Ctrl+X revert single, Ctrl+Z revert all, Ctrl+N open editor
    let footer_text = vec![
        Span::raw(" "),
        Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
        Span::raw(": Navigate  "),
        Span::styled("Ctrl+x", Style::default().fg(Color::Green)),
        Span::raw(": Revert  "),
        Span::styled("Ctrl+z", Style::default().fg(Color::Magenta)),
        Span::raw(": Revert All  "),
        Span::styled("Ctrl+n", Style::default().fg(Color::Blue)),
        Span::raw(": Edit  "),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::raw(": Close"),
    ];

    let footer =
        Paragraph::new(Line::from(footer_text)).alignment(ratatui::layout::Alignment::Left);

    f.render_widget(footer, chunks[3]);
}
