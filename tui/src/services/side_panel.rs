//! Side Panel UI rendering
//!
//! This module handles rendering the side panel with its four sections:
//! - Context: Token usage, credits, session time, model
//! - Todos: Task list parsed from task.md or agent-generated
//! - Changeset: Files modified with edit history
//! - Todos: Task list parsed from task.md or agent-generated

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
const LEFT_PADDING: &str = " ";

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

    // Add horizontal padding - shift content right by 1 char
    let padded_area = Rect {
        x: inner_area.x + 1,
        y: inner_area.y,
        width: inner_area.width.saturating_sub(1),
        height: inner_area.height,
    };

    // Calculate section heights
    let context_height = 4; // Fixed height for context (header, tokens, model)
    let collapsed_height = 1; // Height when collapsed (just header)
    let footer_height = 6; // For version+profile, cwd, empty line, shortcuts (2 lines)

    // All sections are expanded by default (no collapsing)
    let todos_collapsed = state
        .side_panel_section_collapsed
        .get(&SidePanelSection::Todos)
        .copied()
        .unwrap_or(false);
    let changeset_collapsed = state
        .side_panel_section_collapsed
        .get(&SidePanelSection::Changeset)
        .copied()
        .unwrap_or(false);

    let todos_height = if todos_collapsed {
        collapsed_height
    } else {
        (state.todos.len().max(1) + 2).min(8) as u16 // +2 for header, max 8
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
            Constraint::Length(todos_height),
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
        .insert(SidePanelSection::Todos, chunks[1]);
    state
        .side_panel_areas
        .insert(SidePanelSection::Changeset, chunks[2]);

    render_context_section(f, state, chunks[0]);
    render_todos_section(f, state, chunks[1], todos_collapsed);
    render_changeset_section(f, state, chunks[2], changeset_collapsed);
    render_footer_section(f, state, chunks[4]);
}

/// Render the Context section (always visible)
fn render_context_section(f: &mut Frame, state: &AppState, area: Rect) {
    let focused = state.side_panel_focus == SidePanelSection::Context;
    let header_style = section_header_style(focused);

    // Header with caret like other sections
    let header = Line::from(Span::styled(
        format!("{}▾ Context", LEFT_PADDING),
        header_style,
    ));

    let mut lines = vec![header];

    // Helper for right-aligned value row
    // label (Left) ................ value (Right)
    let make_row = |label: &str, value: String, value_color: Color| -> Line {
        // Indent label by 2 spaces to align with "No tasks"
        let label_span = Span::styled(
            format!("{}  {} ", LEFT_PADDING, label),
            Style::default().fg(Color::DarkGray),
        );
        // "   " (3 chars) + label + ":"
        let label_len = 3 + label.len() + 1;
        let value_len = value.len();

        let available_width = area.width as usize;
        let spacing = available_width.saturating_sub(label_len + value_len);

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

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

/// Render the Todos section
fn render_todos_section(f: &mut Frame, state: &AppState, area: Rect, collapsed: bool) {
    let focused = state.side_panel_focus == SidePanelSection::Todos;
    let header_style = section_header_style(focused);

    let collapse_indicator = if collapsed { "▸" } else { "▾" };
    let count = if state.todos.is_empty() {
        String::new()
    } else {
        format!(" ({})", state.todos.len())
    };

    let header = Line::from(Span::styled(
        format!("{}{} Todos{}", LEFT_PADDING, collapse_indicator, count),
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
        for todo in &state.todos {
            let (symbol, color) = match todo.status {
                TodoStatus::Done => ("[x]", Color::Green),
                TodoStatus::InProgress => ("[/]", Color::Yellow),
                TodoStatus::Pending => ("[ ]", Color::DarkGray),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}  {} ", LEFT_PADDING, symbol),
                    Style::default().fg(color),
                ),
                Span::styled(
                    truncate_string(&todo.text, area.width as usize - 10),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
    }

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
        format!(" {} files", count)
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
            let prefix_visual_len = 5;

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
                format!("{}  Ctrl+e to show all files", LEFT_PADDING),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

/// Render the footer section with version, profile, and cwd
fn render_footer_section(f: &mut Frame, state: &AppState, area: Rect) {
    let mut lines = Vec::new();

    // Get current working directory
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let version = env!("CARGO_PKG_VERSION");
    let profile = &state.current_profile_name;
    let available_width = area.width as usize;

    // Line 1: Version (left) and Profile (right)
    let left_part = format!("{}v{}", LEFT_PADDING, version);
    let right_part = format!("profile {}", profile);
    let total_content = left_part.len() + right_part.len();
    let spacing = available_width.saturating_sub(total_content).max(1);

    lines.push(Line::from(vec![
        Span::styled(
            format!("{}v{}", LEFT_PADDING, version),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(" ".repeat(spacing)),
        Span::styled("profile ", Style::default().fg(Color::DarkGray)),
        Span::styled(profile, Style::default().fg(Color::Cyan)),
    ]));

    // Line 2: CWD - truncated to fit width
    let cwd_prefix = LEFT_PADDING.to_string();
    let max_cwd_len = available_width.saturating_sub(cwd_prefix.len());
    let truncated_cwd = truncate_string(&cwd, max_cwd_len);
    lines.push(Line::from(vec![
        Span::styled(cwd_prefix, Style::default()),
        Span::styled(truncated_cwd, Style::default().fg(Color::DarkGray)),
    ]));

    // Empty line between CWD and shortcuts
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
        Span::styled("Tab:", Style::default().fg(Color::Cyan)),
        Span::styled(" Select", Style::default().fg(Color::Reset)),
        Span::raw("  "),
        Span::styled("Enter:", Style::default().fg(Color::LightMagenta)),
        Span::styled(" toggle", Style::default().fg(Color::Reset)),
    ]));

    lines.push(Line::from(vec![
        left_padding_span,
        Span::styled("Ctrl+y:", Style::default().fg(Color::Yellow)),
        Span::styled(" Hide", Style::default().fg(Color::Reset)),
        Span::raw("  "),
        Span::styled("Ctrl+e:", Style::default().fg(Color::Green)),
        Span::styled(" Changes", Style::default().fg(Color::Reset)),
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
