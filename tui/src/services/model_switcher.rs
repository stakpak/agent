//! Model Switcher UI Component
//!
//! Provides a popup UI for switching between available AI models.
//! Accessible via Ctrl+M or the /model command.
//!
//! Features:
//! - Search input for filtering by model name or provider
//! - Models grouped by provider with headers

use crate::app::{AppState, ModelSwitcherMode};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use stakai::Model;

/// Filter models based on mode and search query
/// Returns indices into the original available_models vec that match the filter
pub fn filter_models(models: &[Model], mode: ModelSwitcherMode, search: &str) -> Vec<usize> {
    let search_lower = search.to_lowercase();

    models
        .iter()
        .enumerate()
        .filter(|(_, model)| {
            // Apply mode filter
            let mode_match = match mode {
                ModelSwitcherMode::All => true,
                ModelSwitcherMode::Reasoning => model.reasoning,
            };

            if !mode_match {
                return false;
            }

            // Apply search filter
            if search.is_empty() {
                return true;
            }

            model.name.to_lowercase().contains(&search_lower)
                || model.provider.to_lowercase().contains(&search_lower)
                || model.id.to_lowercase().contains(&search_lower)
        })
        .map(|(idx, _)| idx)
        .collect()
}

/// Render the model switcher popup
pub fn render_model_switcher_popup(f: &mut Frame, state: &AppState) {
    // Calculate popup size (50% width to fit model names and costs, 70% height)
    let area = centered_rect(50, 70, f.area());

    // Clear background
    f.render_widget(ratatui::widgets::Clear, area);

    // Create the main block with border
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Show loading message if no models are available yet
    if state.available_models.is_empty() {
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

        f.render_widget(block.clone().title(" Switch Model "), area);
        let inner = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width - 2,
            height: area.height - 2,
        };
        f.render_widget(loading, inner);
        return;
    }

    // Split area for title, tabs, search, list, and help text inside the block
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
            Constraint::Length(3), // Search (with spacing above and below)
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

    // Render search input
    let search_prompt = ">";
    let cursor = "|";
    let placeholder = "Type to filter";

    let search_spans = if state.model_switcher_search.is_empty() {
        vec![
            Span::raw(" "),
            Span::styled(search_prompt, Style::default().fg(Color::Magenta)),
            Span::raw(" "),
            Span::styled(cursor, Style::default().fg(Color::Cyan)),
            Span::styled(placeholder, Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
        ]
    } else {
        vec![
            Span::raw(" "),
            Span::styled(search_prompt, Style::default().fg(Color::Magenta)),
            Span::raw(" "),
            Span::styled(
                &state.model_switcher_search,
                Style::default()
                    .fg(Color::Reset)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(cursor, Style::default().fg(Color::Cyan)),
        ]
    };

    let search_text = ratatui::text::Text::from(vec![Line::from(""), Line::from(search_spans)]);
    let search_paragraph = Paragraph::new(search_text);
    f.render_widget(search_paragraph, chunks[1]);

    // Get filtered model indices
    let filtered_indices = filter_models(
        &state.available_models,
        state.model_switcher_mode,
        &state.model_switcher_search,
    );

    // Render model list
    render_model_list(f, state, &filtered_indices, chunks[2]);

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
            Span::styled(" = reasoning", Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled("$in/$out", Style::default().fg(Color::DarkGray)),
            Span::styled(" = cost/1M tokens", Style::default().fg(Color::DarkGray)),
        ]),
    ]);

    let help_area = Rect {
        x: chunks[3].x + 1,
        y: chunks[3].y,
        width: chunks[3].width.saturating_sub(2),
        height: chunks[3].height,
    };
    f.render_widget(help, help_area);

    // Render the border last (so it's on top)
    f.render_widget(block, area);
}

/// Render the model list with provider headers
fn render_model_list(f: &mut Frame, state: &AppState, filtered_indices: &[usize], list_area: Rect) {
    // Show empty state if no models match
    if filtered_indices.is_empty() {
        let empty_message = if state.model_switcher_search.is_empty() {
            match state.model_switcher_mode {
                ModelSwitcherMode::All => " No models available",
                ModelSwitcherMode::Reasoning => " No reasoning models available",
            }
        } else {
            " No models match your search"
        };
        let empty_widget = Paragraph::new(Line::from(vec![Span::styled(
            empty_message,
            Style::default().fg(Color::DarkGray),
        )]));
        f.render_widget(empty_widget, list_area);
        return;
    }

    // Group filtered models by provider
    let mut models_by_provider: std::collections::HashMap<&str, Vec<usize>> =
        std::collections::HashMap::new();
    for &idx in filtered_indices {
        let model = &state.available_models[idx];
        models_by_provider
            .entry(model.provider.as_str())
            .or_default()
            .push(idx);
    }

    // Sort providers for consistent ordering, with "stakpak" always first
    let mut providers: Vec<_> = models_by_provider.keys().collect();
    providers.sort_by(|a, b| match (**a == "stakpak", **b == "stakpak") {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    });

    // Build display lines
    let mut lines: Vec<Line> = Vec::new();
    let mut line_to_model_idx: Vec<Option<usize>> = Vec::new(); // Maps line index to model index

    for provider in providers {
        let model_indices = &models_by_provider[provider];

        // Provider header
        let provider_name = get_provider_display_name(provider);
        lines.push(Line::from(vec![Span::styled(
            format!(" {} ", provider_name),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
        line_to_model_idx.push(None); // Header is not selectable

        // Model items
        for &model_idx in model_indices {
            let model = &state.available_models[model_idx];
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

            // Model name with selection/current styling
            let name_style = if is_current {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            // Build the full model line
            let mut model_text = model.name.clone();

            // Reasoning indicator
            if model.reasoning {
                model_text.push_str(" [R]");
            }

            // Cost if available
            if let Some(cost) = &model.cost {
                model_text.push_str(&format!(" ${:.2}/${:.2}", cost.input, cost.output));
            }

            if is_selected && !is_current {
                // For selected item, apply background to entire line
                // Calculate padding to fill the width
                let padding_len = list_area.width.saturating_sub(model_text.len() as u16 + 4);
                let padded_text = format!("{}{}", model_text, " ".repeat(padding_len as usize));
                spans.push(Span::styled(padded_text, name_style));
            } else {
                // Normal rendering with separate colors for parts
                spans.push(Span::styled(model.name.clone(), name_style));

                if model.reasoning {
                    let reasoning_style = if is_current {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Magenta)
                    };
                    spans.push(Span::styled(" [R]", reasoning_style));
                }

                if let Some(cost) = &model.cost {
                    let cost_style = if is_current {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    spans.push(Span::styled(
                        format!(" ${:.2}/${:.2}", cost.input, cost.output),
                        cost_style,
                    ));
                }
            }

            lines.push(Line::from(spans));
            line_to_model_idx.push(Some(model_idx));
        }
    }

    // Calculate scroll position based on selected item
    let height = list_area.height as usize;
    let selected_line = line_to_model_idx
        .iter()
        .position(|idx| *idx == Some(state.model_switcher_selected))
        .unwrap_or(0);

    let scroll = if selected_line >= height {
        selected_line.saturating_sub(height / 2)
    } else {
        0
    };

    // Create visible lines with scroll
    let visible_lines: Vec<Line> = lines.into_iter().skip(scroll).take(height).collect();

    let content = Paragraph::new(visible_lines);
    f.render_widget(content, list_area);
}

/// Get display name for a provider
fn get_provider_display_name(provider: &str) -> &str {
    match provider {
        "anthropic" => "Anthropic",
        "openai" => "OpenAI",
        "google" => "Google",
        "gemini" => "Google Gemini",
        "amazon-bedrock" => "Amazon Bedrock",
        "stakpak" => "Stakpak",
        other => other,
    }
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
