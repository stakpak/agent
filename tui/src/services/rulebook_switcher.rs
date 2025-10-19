use crate::app::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

pub fn render_rulebook_switcher_popup(f: &mut Frame, state: &AppState) {
    // Calculate popup size (80% width, 70% height for more space)
    let area = centered_rect(80, 80, f.area());

    // Clear background
    f.render_widget(ratatui::widgets::Clear, area);

    // Create the main block with border
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

    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title
            Constraint::Length(3), // Search input (with empty lines above and below)
            Constraint::Min(3),    // Content (left + right columns)
            Constraint::Length(1), // Help text
        ])
        .split(inner_area);

    // Render title
    let title = " Select Rulebooks";
    let title_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let title_line = Line::from(Span::styled(title, title_style));
    let title_paragraph = Paragraph::new(title_line);
    f.render_widget(title_paragraph, vertical_chunks[0]);

    // Render search input
    let search_prompt = ">";
    let cursor = "|";
    let placeholder = "Type to filter";

    let search_spans = if state.rulebook_search_input.is_empty() {
        vec![
            Span::raw(" "), // Small space before
            Span::styled(search_prompt, Style::default().fg(Color::Magenta)),
            Span::raw(" "),
            Span::styled(cursor, Style::default().fg(Color::Cyan)),
            Span::styled(placeholder, Style::default().fg(Color::DarkGray)),
            Span::raw(" "), // Small space after
        ]
    } else {
        vec![
            Span::raw(" "), // Small space before
            Span::styled(search_prompt, Style::default().fg(Color::Magenta)),
            Span::raw(" "),
            Span::styled(
                &state.rulebook_search_input,
                Style::default()
                    .fg(Color::Reset)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(cursor, Style::default().fg(Color::Cyan)),
            Span::raw(" "), // Small space after
        ]
    };

    let search_text = Text::from(vec![
        Line::from(""), // Empty line above
        Line::from(search_spans),
        Line::from(""), // Empty line below
    ]);
    let search_paragraph = Paragraph::new(search_text);
    f.render_widget(search_paragraph, vertical_chunks[1]);

    // Split content area into left (list) and right (details) columns
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40), // Left column for URIs
            Constraint::Length(1),      // Vertical separator
            Constraint::Percentage(60), // Right column for details
        ])
        .split(vertical_chunks[2]);

    // Left column: Rulebook list with URIs (using filtered list)
    let list_items: Vec<ListItem> = state
        .filtered_rulebooks
        .iter()
        .map(|rulebook| {
            let is_checked = state.selected_rulebooks.contains(&rulebook.uri);

            let mut lines: Vec<Line> = Vec::new();

            // Better checkbox with [ ] and checkmark
            let checkbox = if is_checked { "[✓] " } else { "[ ] " };
            let checkbox_style = if is_checked {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray) // Unchecked rulebooks in DarkGray
            };

            // Calculate available width for URI (accounting for checkbox and padding)
            let available_width = content_chunks[0].width.saturating_sub(6); // 4 for checkbox + 2 for padding

            // Wrap the URI to fit the available width
            let wrapped_uri = textwrap::wrap(&rulebook.uri, available_width as usize);

            // First line with checkbox and first part of URI
            let mut first_line_spans = vec![];
            first_line_spans.push(Span::styled(checkbox, checkbox_style));

            if let Some(first_uri_line) = wrapped_uri.first() {
                let uri_style = if is_checked {
                    Style::default()
                        .fg(Color::Reset) // Use Reset for checked items
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray) // Use DarkGray for unchecked items
                };
                first_line_spans.push(Span::styled(first_uri_line.to_string(), uri_style));
            }
            lines.push(Line::from(first_line_spans));

            // Subsequent wrapped lines (indented)
            for line in wrapped_uri.iter().skip(1) {
                let uri_style = if is_checked {
                    Style::default()
                        .fg(Color::Reset) // Use Reset for checked items
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray) // Use DarkGray for unchecked items
                };
                lines.push(Line::from(Span::styled(format!("    {}", line), uri_style)));
            }

            ListItem::new(Text::from(lines))
        })
        .collect();

    // Create list for left column with padding
    let list = List::new(list_items)
        .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black))
        .block(Block::default().borders(Borders::NONE));

    // Create list state for highlighting
    // Since we now use Text for multi-line items, we can use the rulebook index directly
    let mut list_state = ListState::default();
    list_state.select(Some(state.rulebook_switcher_selected));

    // Render list in left column with left/right padding
    let list_area = Rect {
        x: content_chunks[0].x + 1, // Add left padding
        y: content_chunks[0].y,
        width: content_chunks[0].width.saturating_sub(2), // Add right padding
        height: content_chunks[0].height,
    };
    f.render_stateful_widget(list, list_area, &mut list_state);

    // Vertical separator line with Cyan color
    let separator = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::Cyan));
    f.render_widget(separator, content_chunks[1]);

    // Right column: Rulebook details
    // Always show the currently highlighted rulebook from the filtered list
    let rulebook_to_show = state
        .filtered_rulebooks
        .get(state.rulebook_switcher_selected);

    if let Some(selected_rulebook) = rulebook_to_show {
        // Add padding to details area
        let details_area = Rect {
            x: content_chunks[2].x + 1, // Add left padding
            y: content_chunks[2].y,
            width: content_chunks[2].width.saturating_sub(2), // Add right padding
            height: content_chunks[2].height,
        };

        // Create detailed information
        let mut detail_lines = vec![];

        // Description
        detail_lines.push(Line::from(vec![Span::styled(
            "Description:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));

        // Wrap description text
        let wrapped_desc = textwrap::wrap(
            &selected_rulebook.description,
            (details_area.width as usize).saturating_sub(2),
        );
        for line in wrapped_desc {
            detail_lines.push(Line::from(vec![Span::styled(
                format!("  {}", line),
                Style::default().fg(Color::White),
            )]));
        }

        detail_lines.push(Line::from("")); // Empty line

        // Tags
        if !selected_rulebook.tags.is_empty() {
            detail_lines.push(Line::from(vec![Span::styled(
                "Tags:",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]));
            let tags_text = selected_rulebook.tags.join(", ");
            let wrapped_tags =
                textwrap::wrap(&tags_text, (details_area.width as usize).saturating_sub(2));
            for line in wrapped_tags {
                detail_lines.push(Line::from(vec![Span::styled(
                    format!("  {}", line),
                    Style::default().fg(Color::Cyan),
                )]));
            }
            detail_lines.push(Line::from("")); // Empty line
        }

        // URI (full)
        detail_lines.push(Line::from(vec![Span::styled(
            "URI:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
        let wrapped_uri = textwrap::wrap(
            &selected_rulebook.uri,
            (details_area.width as usize).saturating_sub(2),
        );
        for line in wrapped_uri {
            detail_lines.push(Line::from(vec![Span::styled(
                format!("  {}", line),
                Style::default().fg(Color::DarkGray),
            )]));
        }

        // Visibility
        detail_lines.push(Line::from(""));
        detail_lines.push(Line::from(vec![Span::styled(
            "Visibility:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
        let visibility_text = match selected_rulebook.visibility {
            stakpak_api::RuleBookVisibility::Public => "Public",
            stakpak_api::RuleBookVisibility::Private => "Private",
        };
        detail_lines.push(Line::from(vec![Span::styled(
            format!("  {}", visibility_text),
            Style::default().fg(Color::Green),
        )]));

        // Updated dates if available
        if let Some(updated_at) = &selected_rulebook.updated_at {
            detail_lines.push(Line::from(vec![Span::styled(
                "Updated:",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]));
            detail_lines.push(Line::from(vec![Span::styled(
                format!("  {}", updated_at.format("%Y-%m-%d %H:%M")),
                Style::default().fg(Color::DarkGray),
            )]));
        }

        // Render details
        let details_paragraph =
            Paragraph::new(detail_lines).block(Block::default().borders(Borders::NONE));
        f.render_widget(details_paragraph, details_area);
    }

    // Help text
    let help = Paragraph::new(Line::from(vec![
        Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
        Span::raw(": Navigate  "),
        Span::styled("Space", Style::default().fg(Color::Cyan)),
        Span::raw(": Toggle  "),
        Span::styled("Ctrl+S", Style::default().fg(Color::Magenta)),
        Span::raw(": Select All  "),
        Span::styled("Ctrl+D", Style::default().fg(Color::Magenta)),
        Span::raw(": Deselect All  "),
        Span::styled("↵", Style::default().fg(Color::Green)),
        Span::raw(": Confirm  "),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::raw(": Cancel"),
    ]));

    let help_area = Rect {
        x: vertical_chunks[3].x + 1,
        y: vertical_chunks[3].y,
        width: vertical_chunks[3].width.saturating_sub(2),
        height: vertical_chunks[3].height,
    };

    f.render_widget(help, help_area);

    // Render the border with title last (so it's on top)
    f.render_widget(block, area);
}

/// Helper function to create a centered rect
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
