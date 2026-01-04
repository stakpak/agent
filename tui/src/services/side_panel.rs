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
    let context_height = 5; // Fixed height for context
    let collapsed_height = 1; // Height when collapsed (just header)
    let footer_height = 4; // For STAKPAK+profile, cwd (with wrapping)

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

    // Session duration
    let duration = state.session_start_time.elapsed();
    let mins = duration.as_secs() / 60;
    let secs = duration.as_secs() % 60;
    lines.push(make_row(
        "Session",
        format!("{}m {}s", mins, secs),
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

    if state.changeset.file_count() == 0 {
        lines.push(Line::from(Span::styled(
            format!("{}  No changes", LEFT_PADDING),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
    } else {
        for (i, file) in state.changeset.files_in_order().iter().enumerate() {
            let is_selected = i == state.changeset.selected_index && focused;
            // Prefix: "  ▸ " (4 chars)
            let prefix = if file.is_expanded { "▾" } else { "▸" };
            let deleted_marker = if file.is_deleted { " [del]" } else { "" };

            let added = file.total_lines_added();
            let removed = file.total_lines_removed();
            let stats = format!("+{} -{}", added, removed);

            // File Name Style: DarkGray unless selected or deleted
            let name_style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::White)
            } else if file.is_deleted {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let display_name = file.display_name();
            // Calculate available width for name
            // Total = Width

            // Prefix part: " " (1) + "  " (2) + "▸" (1) + " " (1) = 5 chars
            // Note: "▸" is 3 bytes, but visually 1 char (width 1).
            // We use visual length for spacing calculation.
            let prefix_part = format!("{}  {} ", LEFT_PADDING, prefix);
            let prefix_visual_len = 5;

            let combined_name = format!("{}{}", display_name, deleted_marker);

            // Check if file is reverted
            let (stats_spans, stats_len) = if file.is_reverted {
                // Show "REVERTED" in dark gray instead of stats
                let reverted_text = "REVERTED";
                (vec![
                    Span::styled(reverted_text, Style::default().fg(Color::DarkGray)),
                ], reverted_text.len())
            } else {
                // Show normal stats
                let added = file.total_lines_added();
                let removed = file.total_lines_removed();
                let stats = format!("+{} -{}", added, removed);
                let stats_len_calc = stats.len();
                (vec![
                    Span::styled(format!("+{}", added), Style::default().fg(Color::Green)),
                    Span::raw(" "),
                    Span::styled(format!("-{}", removed), Style::default().fg(Color::Red)),
                ], stats_len_calc)
            };

            let available_width = area.width as usize;
            let space_for_name = available_width.saturating_sub(prefix_visual_len + stats_len + 1); // +1 min padding

            let truncated_name = truncate_string(&combined_name, space_for_name);

            // Calculate spacing using visual lengths
            let spacing = available_width.saturating_sub(prefix_visual_len + truncated_name.len() + stats_len);

            let mut line_spans = vec![
                Span::styled(prefix_part, Style::default().fg(Color::DarkGray)),
                Span::styled(truncated_name, name_style),
                Span::raw(" ".repeat(spacing)),
            ];
            line_spans.extend(stats_spans);

            lines.push(Line::from(line_spans));

            // Show edits if expanded (keep left aligned or indented)
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

    // Line 1: STAKPAK label + Version (left) and Profile (right) on same line
    let stakpak_label = "STAKPAK";
    let version = env!("CARGO_PKG_VERSION");
    let profile = &state.current_profile_name;

    // Calculate spacing to push profile to the right
    // left_part visual spacing approximation
    let left_combined = format!("{}{} v{}", LEFT_PADDING, stakpak_label, version);
    let right_part = format!("profile {}", profile);
    let total_content = left_combined.len() + right_part.len();
    let available_width = area.width as usize;
    let spacing = if available_width > total_content {
        available_width - total_content
    } else {
        1
    };

    lines.push(Line::from(vec![
        Span::styled(
            format!("{}{}", LEFT_PADDING, stakpak_label),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" v{}", version),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(" ".repeat(spacing)),
        Span::styled("profile ", Style::default().fg(Color::Reset)),
        Span::styled(profile, Style::default().fg(Color::DarkGray)),
    ]));

    // Line 2+: CWD without label, wrapping
    lines.push(Line::from(vec![
        Span::styled(
            format!("{}", LEFT_PADDING),
            Style::default().fg(Color::Reset),
        ),
        Span::styled(&cwd, Style::default().fg(Color::DarkGray)),
    ]));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

/// Get the style for a section header - Reset color
fn section_header_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Reset)
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
