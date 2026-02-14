//! Model Switcher UI Component
//!
//! Provides a popup UI for switching between available AI models.
//! Accessible via Ctrl+G or the /model command.

use crate::app::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

/// Render the model switcher popup
pub fn render_model_switcher_popup(f: &mut Frame, state: &AppState) {
    // Calculate popup size (45% width to fit model names and costs, 60% height)
    let area = centered_rect(45, 60, f.area());

    // Clear background
    f.render_widget(ratatui::widgets::Clear, area);

    // Show loading message if no models are available yet
    if state.available_models.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Switch Model ");

        let loading = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Loading models...",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Press ESC to cancel",
                Style::default().fg(Color::DarkGray),
            )),
        ]);

        f.render_widget(block, area);
        let inner = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width - 2,
            height: area.height - 2,
        };
        f.render_widget(loading, inner);
        return;
    }

    // Group models by provider
    let mut models_by_provider: std::collections::HashMap<&str, Vec<_>> =
        std::collections::HashMap::new();
    for model in &state.available_models {
        models_by_provider
            .entry(model.provider.as_str())
            .or_default()
            .push(model);
    }

    // Create list items with provider headers
    let mut items: Vec<ListItem> = Vec::new();
    let mut item_indices: Vec<Option<usize>> = Vec::new(); // Maps display index to model index
    let mut model_idx = 0;

    // Sort providers for consistent ordering, with "stakpak" always first
    let mut providers: Vec<_> = models_by_provider.keys().collect();
    providers.sort_by(|a, b| match (**a == "stakpak", **b == "stakpak") {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    });

    for provider in providers {
        let models = &models_by_provider[provider];

        // Provider header
        let provider_name = match *provider {
            "anthropic" => "Anthropic",
            "openai" => "OpenAI",
            "google" => "Google",
            "gemini" => "Google Gemini",
            "amazon-bedrock" => "Amazon Bedrock",
            "stakpak" => "Stakpak",
            _ => *provider,
        };

        items.push(ListItem::new(Line::from(vec![Span::styled(
            format!(" {} ", provider_name),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )])));
        item_indices.push(None); // Header is not selectable

        // Model items
        for model in models.iter() {
            let is_selected = model_idx == state.model_switcher_selected;
            let is_current = state
                .current_model
                .as_ref()
                .is_some_and(|m| m.id == model.id);

            let mut spans = vec![];

            // Current indicator
            if is_current {
                spans.push(Span::styled("  ", Style::default().fg(Color::Green)));
            } else {
                spans.push(Span::raw("   "));
            }

            // Model name
            let name_style = if is_current {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            spans.push(Span::styled(model.name.clone(), name_style));

            // Reasoning indicator
            if model.reasoning {
                spans.push(Span::styled(" [R]", Style::default().fg(Color::Magenta)));
            }

            // Cost if available
            if let Some(cost) = &model.cost {
                spans.push(Span::styled(
                    format!(" ${:.2}/${:.2}", cost.input, cost.output),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            items.push(ListItem::new(Line::from(spans)));
            item_indices.push(Some(model_idx));
            model_idx += 1;
        }
    }

    // Create the main block with border
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Split area for title, list and help text inside the block
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
            Constraint::Min(3),    // List
            Constraint::Length(2), // Help text
        ])
        .split(inner_area);

    // Render title inside the popup
    let title = " Switch Model";
    let title_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let title_line = Line::from(Span::styled(title, title_style));
    let title_paragraph = Paragraph::new(title_line);

    f.render_widget(title_paragraph, chunks[0]);

    // Find the display index that corresponds to the selected model index
    let display_selected = item_indices
        .iter()
        .position(|idx| *idx == Some(state.model_switcher_selected))
        .unwrap_or(0);

    // Create list with proper block and padding
    let list = List::new(items)
        .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black))
        .block(Block::default().borders(Borders::NONE));

    // Create list state for highlighting
    let mut list_state = ListState::default();
    list_state.select(Some(display_selected));

    // Render list with proper padding
    let list_area = Rect {
        x: chunks[1].x,
        y: chunks[1].y + 1,
        width: chunks[1].width,
        height: chunks[1].height.saturating_sub(1),
    };

    f.render_stateful_widget(list, list_area, &mut list_state);

    // Help text
    let help = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("↑/↓", Style::default().fg(Color::DarkGray)),
            Span::styled(" navigate", Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled("↵", Style::default().fg(Color::DarkGray)),
            Span::styled(" select", Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled("esc", Style::default().fg(Color::DarkGray)),
            Span::styled(" cancel", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("[R]", Style::default().fg(Color::Magenta)),
            Span::styled(" = reasoning support", Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled("$in/$out", Style::default().fg(Color::DarkGray)),
            Span::styled(
                " = cost per 1M tokens",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ]);

    let help_area = Rect {
        x: chunks[2].x + 1,
        y: chunks[2].y,
        width: chunks[2].width.saturating_sub(2),
        height: chunks[2].height,
    };

    f.render_widget(help, help_area);

    // Render the border last (so it's on top)
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
