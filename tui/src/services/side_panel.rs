//! Side Panel UI rendering
//!
//! This module handles rendering the side panel with its four sections:
//! - Context: Token usage, credits, session time, model
//! - Tasks: Task list from agent-board cards
//! - Changeset: Files modified with edit history

use crate::app::AppState;
use crate::services::changeset::{SidePanelSection, TodoStatus};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use stakpak_shared::models::model_pricing::ContextAware;

/// Left padding for content inside the side panel
const LEFT_PADDING: &str = "  ";

/// Render the complete side panel
pub fn render_side_panel(f: &mut Frame, state: &mut AppState, area: Rect) {
    // Clear the area first
    f.render_widget(ratatui::widgets::Clear, area);

    // Create a block for the side panel with a subtle border
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    // Add padding: 1 line on top and bottom (left uses LEFT_PADDING in content)
    let padded_area = Rect {
        x: inner_area.x,
        y: inner_area.y.saturating_add(1),
        width: inner_area.width,
        height: inner_area.height.saturating_sub(2),
    };

    // Calculate section heights
    let collapsed_height = 1; // Height when collapsed (just header)
    let footer_height = 4; // For version+profile, empty line, shortcuts (2 lines)

    // All sections are expanded by default (no collapsing)
    let context_collapsed = state
        .side_panel_section_collapsed
        .get(&SidePanelSection::Context)
        .copied()
        .unwrap_or(false);
    let billing_collapsed = state
        .side_panel_section_collapsed
        .get(&SidePanelSection::Billing)
        .copied()
        .unwrap_or(false);
    let tasks_collapsed = state
        .side_panel_section_collapsed
        .get(&SidePanelSection::Tasks)
        .copied()
        .unwrap_or(false);
    let changeset_collapsed = state
        .side_panel_section_collapsed
        .get(&SidePanelSection::Changeset)
        .copied()
        .unwrap_or(false);

    let context_height = if context_collapsed {
        collapsed_height
    } else {
        6 // Header + Tokens + Model + Provider + Profile
    };

    // Billing section is hidden when billing_info is None (local mode)
    let billing_height = if state.billing_info.is_none() {
        0
    } else if billing_collapsed {
        collapsed_height
    } else {
        4 // Header + Plan + Credits
    };

    // Calculate task content width for wrapping
    let task_content_width = padded_area.width.saturating_sub(10) as usize; // Accounts for LEFT_PADDING + symbol + spacing

    let tasks_height = if tasks_collapsed {
        collapsed_height
    } else if state.todos.is_empty() {
        3 // Header + "No tasks" + blank line
    } else {
        // Calculate total lines needed including wrapped lines
        let mut total_lines = 1; // Header
        for todo in &state.todos {
            let wrapped_lines = wrap_text(&todo.text, task_content_width);
            total_lines += wrapped_lines.len().max(1);
        }
        total_lines += 1; // blank line spacing
        (total_lines as u16).min(30) // Allow more items to be visible
    };

    let changeset_height = if changeset_collapsed {
        collapsed_height
    } else {
        (state.changeset.file_count().max(1) + 2).min(10) as u16 // +2 for header, max 10
    };

    // Layout the sections vertically
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(context_height),
            Constraint::Length(billing_height),
            Constraint::Length(tasks_height),
            Constraint::Length(changeset_height),
            Constraint::Min(0),                // Remaining space
            Constraint::Length(footer_height), // Footer
        ])
        .split(padded_area);

    // Store areas for mouse handling
    state.side_panel_areas.clear();
    state
        .side_panel_areas
        .insert(SidePanelSection::Context, chunks[0]);
    state
        .side_panel_areas
        .insert(SidePanelSection::Billing, chunks[1]);
    state
        .side_panel_areas
        .insert(SidePanelSection::Tasks, chunks[2]);
    state
        .side_panel_areas
        .insert(SidePanelSection::Changeset, chunks[3]);

    render_context_section(f, state, chunks[0], context_collapsed);
    render_billing_section(f, state, chunks[1], billing_collapsed);
    render_tasks_section(f, state, chunks[2], tasks_collapsed);
    render_changeset_section(f, state, chunks[3], changeset_collapsed);
    render_footer_section(f, state, chunks[5]);
}

/// Render the Context section
fn render_context_section(f: &mut Frame, state: &AppState, area: Rect, collapsed: bool) {
    let focused = state.side_panel_focus == SidePanelSection::Context;
    let header_style = section_header_style(focused);

    let collapse_indicator = if collapsed { "▸" } else { "▾" };

    let header = Line::from(Span::styled(
        format!("{}{} Context", LEFT_PADDING, collapse_indicator),
        header_style,
    ));

    if collapsed {
        let paragraph = Paragraph::new(vec![header]);
        f.render_widget(paragraph, area);
        return;
    }

    let mut lines = vec![header];

    // Helper for right-aligned value row
    // label (Left) ................ value (Right)
    let make_row = |label: &str, value: String, value_color: Color| -> Line {
        // Indent label by 2 spaces to align with "No tasks"
        let label_span = Span::styled(
            format!("{}  {} ", LEFT_PADDING, label),
            Style::default().fg(Color::DarkGray),
        );
        // LEFT_PADDING (2) + "  " (2 indent) + label
        let label_len = LEFT_PADDING.len() + 2 + label.len();
        let value_len = value.len();
        let right_padding = 2; // Reserve space at right edge

        let available_width = area.width as usize;
        let spacing = available_width.saturating_sub(label_len + value_len + right_padding);

        Line::from(vec![
            label_span,
            Span::raw(" ".repeat(spacing)),
            Span::styled(value, Style::default().fg(value_color)),
        ])
    };

    // Token usage
    let tokens = state.current_message_usage.total_tokens;
    let context_info = state
        .llm_model
        .as_ref()
        .map(|m| m.context_info())
        .unwrap_or_default();
    let max_tokens = context_info.max_tokens as u32;

    // Show N/A when no content yet (tokens == 0)
    if tokens == 0 {
        lines.push(make_row("Tokens", "N/A".to_string(), Color::DarkGray));
        lines.push(make_row("Model", "N/A".to_string(), Color::DarkGray));
    } else {
        let percentage = if max_tokens > 0 {
            ((tokens as f64 / max_tokens as f64) * 100.0).round() as u32
        } else {
            0
        };

        lines.push(make_row(
            "Tokens",
            format!(
                "{} / {}K ({}%)",
                format_tokens(tokens),
                max_tokens / 1000,
                percentage
            ),
            Color::White,
        ));

        // Model name
        let model_name = state
            .llm_model
            .as_ref()
            .map(|m| m.model_name())
            .unwrap_or_else(|| state.agent_model.to_string());

        // Truncate model name if needed, assuming label len ~10 ("   Model:")
        let avail_for_model = area.width as usize - 10;
        let truncated_model = truncate_string(&model_name, avail_for_model);

        lines.push(make_row("Model", truncated_model, Color::Cyan));
    }

    // Provider - show subscription, auth provider, or config provider
    let provider_value = match &state.auth_display_info {
        (_, Some(_), Some(subscription)) => subscription.clone(),
        (_, Some(auth_provider), None) => auth_provider.clone(),
        (Some(config_provider), None, None) => config_provider.clone(),
        _ => "Remote".to_string(),
    };
    lines.push(make_row("Provider", provider_value, Color::DarkGray));

    // Profile
    lines.push(make_row(
        "Profile",
        state.current_profile_name.clone(),
        Color::DarkGray,
    ));

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

/// Render the Billing section
fn render_billing_section(f: &mut Frame, state: &AppState, area: Rect, collapsed: bool) {
    let focused = state.side_panel_focus == SidePanelSection::Billing;
    let header_style = section_header_style(focused);

    let collapse_indicator = if collapsed { "▸" } else { "▾" };

    let header = Line::from(Span::styled(
        format!("{}{} Billing", LEFT_PADDING, collapse_indicator),
        header_style,
    ));

    if collapsed {
        let paragraph = Paragraph::new(vec![header]);
        f.render_widget(paragraph, area);
        return;
    }

    let mut lines = vec![header];

    // Helper for right-aligned value row
    let make_row = |label: &str, value: String, value_color: Color| -> Line {
        let label_span = Span::styled(
            format!("{}  {} ", LEFT_PADDING, label),
            Style::default().fg(Color::DarkGray),
        );
        // LEFT_PADDING (2) + "  " (2 indent) + label
        let label_len = LEFT_PADDING.len() + 2 + label.len();
        let value_len = value.len();
        let right_padding = 2; // Reserve space at right edge

        let available_width = area.width as usize;
        let spacing = available_width.saturating_sub(label_len + value_len + right_padding);

        Line::from(vec![
            label_span,
            Span::raw(" ".repeat(spacing)),
            Span::styled(value, Style::default().fg(value_color)),
        ])
    };

    if let Some(info) = &state.billing_info {
        // Get plan name from first active product
        let plan_name = info
            .products
            .iter()
            .find(|p| p.status == "active")
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "-".to_string());
        lines.push(make_row("Plan", plan_name, Color::Cyan));

        let credits = info.features.get("credits");
        if let Some(credit_feature) = credits {
            let balance = credit_feature.balance.unwrap_or(0.0);
            lines.push(make_row("Balance", format!("${:.2}", balance), Color::Cyan));
        } else {
            lines.push(make_row("Balance", "-".to_string(), Color::DarkGray));
        }
    } else {
        lines.push(Line::from(Span::styled(
            format!("{}  Loading...", LEFT_PADDING),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

/// Render the Tasks section
fn render_tasks_section(f: &mut Frame, state: &AppState, area: Rect, collapsed: bool) {
    let focused = state.side_panel_focus == SidePanelSection::Tasks;
    let header_style = section_header_style(focused);

    let collapse_indicator = if collapsed { "▸" } else { "▾" };
    let progress = if let Some(ref p) = state.task_progress {
        format!(" ({}/{})", p.completed, p.total)
    } else if state.todos.is_empty() {
        String::new()
    } else {
        format!(" ({})", state.todos.len())
    };

    let header = Line::from(Span::styled(
        format!("{}{} Tasks{}", LEFT_PADDING, collapse_indicator, progress),
        header_style,
    ));

    if collapsed {
        let paragraph = Paragraph::new(vec![header]);
        f.render_widget(paragraph, area);
        return;
    }

    let mut lines = vec![header];

    if state.todos.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("{}  No tasks", LEFT_PADDING),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
    } else {
        use crate::services::changeset::TodoItemType;

        // Calculate available width for todo text
        let card_prefix_width = LEFT_PADDING.len() + 6; // "  [x] " = 6 chars
        let checklist_prefix_width = LEFT_PADDING.len() + 9; // "     └ [x] " = 9 chars
        let card_content_width = (area.width as usize).saturating_sub(card_prefix_width + 2);
        let checklist_content_width =
            (area.width as usize).saturating_sub(checklist_prefix_width + 2);

        for todo in &state.todos {
            let (symbol, symbol_color, text_color) = match todo.status {
                TodoStatus::Done => ("✓", Color::Green, Color::DarkGray),
                TodoStatus::InProgress => ("◐", Color::Yellow, Color::Reset),
                TodoStatus::Pending => ("○", Color::DarkGray, Color::DarkGray),
            };

            match todo.item_type {
                TodoItemType::Card => {
                    // Card: bold with status symbol
                    let wrapped_lines = wrap_text(&todo.text, card_content_width);

                    for (i, line_text) in wrapped_lines.iter().enumerate() {
                        if i == 0 {
                            lines.push(Line::from(vec![
                                Span::styled(
                                    format!("{}  {} ", LEFT_PADDING, symbol),
                                    Style::default().fg(symbol_color),
                                ),
                                Span::styled(
                                    line_text.clone(),
                                    Style::default().fg(text_color).add_modifier(Modifier::BOLD),
                                ),
                            ]));
                        } else {
                            lines.push(Line::from(vec![
                                Span::styled(format!("{}    ", LEFT_PADDING), Style::default()),
                                Span::styled(
                                    line_text.clone(),
                                    Style::default().fg(text_color).add_modifier(Modifier::BOLD),
                                ),
                            ]));
                        }
                    }
                }
                TodoItemType::ChecklistItem => {
                    // Checklist item: indented with tree connector
                    let wrapped_lines = wrap_text(&todo.text, checklist_content_width);

                    for (i, line_text) in wrapped_lines.iter().enumerate() {
                        if i == 0 {
                            lines.push(Line::from(vec![
                                Span::styled(
                                    format!("{}     └ ", LEFT_PADDING),
                                    Style::default().fg(Color::DarkGray),
                                ),
                                Span::styled(
                                    format!("{} ", symbol),
                                    Style::default().fg(symbol_color),
                                ),
                                Span::styled(line_text.clone(), Style::default().fg(text_color)),
                            ]));
                        } else {
                            lines.push(Line::from(vec![
                                Span::styled(
                                    format!("{}         ", LEFT_PADDING),
                                    Style::default(),
                                ),
                                Span::styled(line_text.clone(), Style::default().fg(text_color)),
                            ]));
                        }
                    }
                }
                TodoItemType::CollapsedIndicator => {
                    // Collapsed indicator: italic, dimmed, shows count
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{}     ⋮ ", LEFT_PADDING),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(
                            todo.text.clone(),
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::ITALIC),
                        ),
                    ]));
                }
            }
        }
    }
    // Add blank line for spacing before Changeset section
    lines.push(Line::from(""));

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

/// Render the Changeset section
fn render_changeset_section(f: &mut Frame, state: &AppState, area: Rect, collapsed: bool) {
    let focused = state.side_panel_focus == SidePanelSection::Changeset;
    let header_style = section_header_style(focused);

    let collapse_indicator = if collapsed { "▸" } else { "▾" };
    let count = state.changeset.file_count();

    // Show "n files changed" on the right if there are files
    // User requested "numbers of edits/deletion move them to the far right"
    // "file label on left and numbers on far right also make file names into DarkGray"

    // Header remains same
    let count_label = if count > 0 {
        format!(" ({})", count)
    } else {
        String::new()
    };

    let header = Line::from(Span::styled(
        format!(
            "{}{} Changeset{}",
            LEFT_PADDING, collapse_indicator, count_label
        ),
        header_style,
    ));

    if collapsed {
        let paragraph = Paragraph::new(vec![header]);
        f.render_widget(paragraph, area);
        return;
    }

    let mut lines = vec![header];

    // Import FileState
    use crate::services::changeset::FileState;

    if state.changeset.file_count() == 0 {
        lines.push(Line::from(Span::styled(
            format!("{}  No changes", LEFT_PADDING),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
    } else {
        // Show all files including reverted/deleted ones so user can see history
        // The file_count() filter might need adjustment if we want to hide them totally
        // But for "Removed" files we definitely want to show them

        let files = state.changeset.files_in_order();
        let total_files = files.len();
        let max_display = 5;

        for (i, file) in files.iter().take(max_display).enumerate() {
            let is_selected = i == state.changeset.selected_index && focused;
            // Prefix: "  ▸ " (4 chars)
            let prefix = if file.is_expanded { "▾" } else { "▸" };

            // Determine state label and color
            let state_label = file.state.label();
            let state_color = match file.state {
                FileState::Created => Color::Green,
                FileState::Modified => Color::Blue,
                FileState::Removed => Color::Red,
                FileState::Reverted => Color::DarkGray,
                FileState::Deleted => Color::DarkGray,
            };

            // File Name Style
            let name_style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                match file.state {
                    FileState::Removed => Style::default().fg(Color::Red),
                    FileState::Reverted | FileState::Deleted => Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::CROSSED_OUT),
                    _ => Style::default().fg(Color::DarkGray),
                }
            };

            let display_name = file.display_name();

            // Prefix part: " " (1) + "  " (2) + "▸" (1) + " " (1) = 5 chars
            let prefix_part = format!("{}  {} ", LEFT_PADDING, prefix);
            let prefix_visual_len = 6;

            // Stats or State Label
            let (stats_spans, stats_len) = match file.state {
                FileState::Reverted => (
                    vec![Span::styled(
                        "REVERTED",
                        Style::default().fg(Color::DarkGray),
                    )],
                    8,
                ),
                FileState::Deleted => (
                    vec![Span::styled(
                        "DELETED",
                        Style::default().fg(Color::DarkGray),
                    )],
                    7,
                ),
                FileState::Removed => (
                    vec![Span::styled("REMOVED", Style::default().fg(Color::Red))],
                    7,
                ),
                _ => {
                    let added = file.total_lines_added();
                    let removed = file.total_lines_removed();
                    (
                        vec![
                            Span::styled(format!("+{}", added), Style::default().fg(Color::Green)),
                            Span::raw(" "),
                            Span::styled(format!("-{}", removed), Style::default().fg(Color::Red)),
                        ],
                        format!("+{} -{}", added, removed).len(),
                    )
                }
            };

            // Calculate available width
            let available_width = area.width as usize;

            // Format: [PREFIX] [STATE_LABEL] [NAME] ... [STATS]
            // We want the state label to be next to the name

            let label_span = Span::styled(
                format!("{} ", state_label),
                Style::default().fg(state_color),
            );
            let label_len = state_label.len() + 1;

            let space_for_name =
                available_width.saturating_sub(prefix_visual_len + label_len + stats_len + 1); // +1 padding

            let truncated_name = truncate_string(display_name, space_for_name);

            let spacing = available_width
                .saturating_sub(prefix_visual_len + label_len + truncated_name.len() + stats_len);

            let mut line_spans = vec![
                Span::styled(prefix_part, Style::default().fg(Color::DarkGray)),
                label_span,
                Span::styled(truncated_name, name_style),
                Span::raw(" ".repeat(spacing)),
            ];
            line_spans.extend(stats_spans);

            lines.push(Line::from(line_spans));

            // Show edits if expanded
            if file.is_expanded {
                for (j, edit) in file.edits.iter().enumerate().rev().take(5) {
                    let time = edit.timestamp.format("%H:%M").to_string();
                    let edit_selected = is_selected && j == file.selected_edit;
                    let edit_style = if edit_selected {
                        Style::default().fg(Color::Black).bg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    lines.push(Line::from(Span::styled(
                        format!(
                            "{}    {} {}",
                            LEFT_PADDING,
                            time,
                            truncate_string(&edit.summary, area.width as usize - 14)
                        ),
                        edit_style,
                    )));
                }
            }
        }

        // Show hint if there are more files than displayed
        if total_files > max_display {
            lines.push(Line::from(Span::styled(
                format!("{}  ctrl+g to show all files", LEFT_PADDING),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

/// Render the footer section with version and shortcuts
fn render_footer_section(f: &mut Frame, _state: &AppState, area: Rect) {
    let mut lines = Vec::new();

    let version = env!("CARGO_PKG_VERSION");

    // Line 1: Version (left)
    lines.push(Line::from(vec![Span::styled(
        format!("{}v{}", LEFT_PADDING, version),
        Style::default().fg(Color::DarkGray),
    )]));

    // Empty line between version/profile and shortcuts
    lines.push(Line::from(""));

    // Shortcuts split into lines with colors:
    // Tab: Select (Cyan)
    // Enter: toggle (LightMagenta)
    // Ctrl+b: Hide (Yellow)

    let left_padding_span = Span::styled(
        LEFT_PADDING.to_string(),
        Style::default().fg(Color::DarkGray),
    );

    lines.push(Line::from(vec![
        left_padding_span.clone(),
        Span::styled("tab", Style::default().fg(Color::DarkGray)),
        Span::styled(" select", Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled("enter", Style::default().fg(Color::DarkGray)),
        Span::styled(" toggle", Style::default().fg(Color::Cyan)),
    ]));

    lines.push(Line::from(vec![
        left_padding_span,
        Span::styled("ctrl+y", Style::default().fg(Color::DarkGray)),
        Span::styled(" hide", Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled("ctrl+g", Style::default().fg(Color::DarkGray)),
        Span::styled(" changes", Style::default().fg(Color::Cyan)),
    ]));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

/// Get the style for a section header - magenta when focused for better visibility
fn section_header_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Reset)
    }
}

/// Format token count with separators
fn format_tokens(tokens: u32) -> String {
    if tokens >= 1000 {
        format!("{}K", tokens / 1000)
    } else {
        tokens.to_string()
    }
}

/// Truncate a string to fit within a given width
fn truncate_string(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        s.to_string()
    } else if max_width > 3 {
        format!("{}...", &s[..max_width - 3])
    } else {
        s[..max_width].to_string()
    }
}

/// Wrap text to fit within a given width, returning multiple lines
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    // Handle edge cases - always return at least the original text
    if text.is_empty() {
        return vec![String::new()];
    }
    if max_width == 0 || max_width < 5 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = unicode_width::UnicodeWidthStr::width(word);

        if current_line.is_empty() {
            // First word on the line
            current_line = word.to_string();
            current_width = word_width;
        } else if current_width + 1 + word_width <= max_width {
            // Word fits on current line with a space
            current_line.push(' ');
            current_line.push_str(word);
            current_width += 1 + word_width;
        } else {
            // Word doesn't fit, start a new line
            lines.push(current_line);
            current_line = word.to_string();
            current_width = word_width;
        }
    }

    // Don't forget the last line
    if !current_line.is_empty() {
        lines.push(current_line);
    }

    // Ensure we always return at least one line
    if lines.is_empty() {
        vec![text.to_string()]
    } else {
        lines
    }
}
