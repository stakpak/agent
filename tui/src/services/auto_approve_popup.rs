use crate::app::AppState;
use crate::services::auto_approve::AutoApprovePolicy;
use crate::services::detect_term::ThemeColors;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

const VISIBLE_ROWS: usize = 12;

fn policy_label(policy: &AutoApprovePolicy) -> &'static str {
    match policy {
        AutoApprovePolicy::Prompt => "Always Ask",
        AutoApprovePolicy::Auto => "Auto Approve",
        AutoApprovePolicy::Never => "Always Reject",
    }
}

fn policy_color(policy: &AutoApprovePolicy) -> Color {
    match policy {
        AutoApprovePolicy::Prompt => ThemeColors::yellow(),
        AutoApprovePolicy::Auto => ThemeColors::green(),
        AutoApprovePolicy::Never => ThemeColors::red(),
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut out: String = value.chars().take(max_chars).collect();
    if value.chars().count() > max_chars {
        out.push('…');
    }
    out
}

pub fn render_auto_approve_popup(f: &mut Frame, state: &AppState) {
    let area = {
        let terminal_area = f.area();
        let width = (terminal_area.width * 90 / 100)
            .max(110)
            .min(terminal_area.width);
        // borders(2) + title(1) + filter(1) + status(1) + header(1) + rows + footer(1)
        let desired_height = (VISIBLE_ROWS + 7) as u16;
        let height = desired_height.clamp(9, terminal_area.height);
        let x = terminal_area.width.saturating_sub(width) / 2;
        let y = terminal_area.height.saturating_sub(height) / 2;
        Rect::new(x, y, width, height)
    };

    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ThemeColors::cyan()));
    f.render_widget(block, area);

    let inner_area = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    // Layout: title | filter | status | header | rows (expands) | footer (pinned)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                // 0: title
            Constraint::Length(1),                // 1: filter input
            Constraint::Length(1),                // 2: status
            Constraint::Length(1),                // 3: column headers
            Constraint::Min(VISIBLE_ROWS as u16), // 4: rows (fills remaining space)
            Constraint::Length(1),                // 5: footer (pinned to bottom)
        ])
        .split(inner_area);

    // --- Title ---
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            " Tool Approval Settings ",
            Style::default()
                .fg(ThemeColors::yellow())
                .add_modifier(Modifier::BOLD),
        )])),
        chunks[0],
    );

    // --- Filter input ---
    let filter_text = &state.tool_approval_popup_state.filter_text;
    let filter_line = if filter_text.is_empty() {
        Line::from(vec![
            Span::styled(" Search: ", Style::default().fg(ThemeColors::dark_gray())),
            Span::styled("_", Style::default().fg(ThemeColors::dark_gray())),
        ])
    } else {
        Line::from(vec![
            Span::styled(" Search: ", Style::default().fg(ThemeColors::dark_gray())),
            Span::styled(
                filter_text.as_str(),
                Style::default()
                    .fg(ThemeColors::cyan())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("▌", Style::default().fg(ThemeColors::cyan())),
        ])
    };
    f.render_widget(Paragraph::new(filter_line), chunks[1]);

    // --- Status ---
    let changed_count = state
        .tool_approval_popup_state
        .rows
        .iter()
        .filter(|row| row.policy != row.original_policy)
        .count();
    let visible_count = state.tool_approval_popup_state.visible_count();
    let total_count = state.tool_approval_popup_state.rows.len();
    let status_line = if changed_count > 0 {
        Line::from(vec![
            Span::styled(
                " Pending changes: ",
                Style::default().fg(ThemeColors::dark_gray()),
            ),
            Span::styled(
                changed_count.to_string(),
                Style::default()
                    .fg(ThemeColors::accent())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · ", Style::default().fg(ThemeColors::dark_gray())),
            Span::styled(
                format!("{}/{} tools", visible_count, total_count),
                Style::default().fg(ThemeColors::cyan()),
            ),
        ])
    } else {
        Line::from(vec![Span::styled(
            format!(" {}/{} tools", visible_count, total_count),
            Style::default().fg(ThemeColors::dark_gray()),
        )])
    };
    f.render_widget(Paragraph::new(status_line), chunks[2]);

    // --- Column headers ---
    let policy_columns = [
        AutoApprovePolicy::Prompt,
        AutoApprovePolicy::Auto,
        AutoApprovePolicy::Never,
    ];
    let policy_width = 17usize;
    let header_total_width = chunks[3].width as usize;
    let header_policy_width = policy_columns.len() * policy_width;
    let header_spacer = header_total_width
        .saturating_sub(6 + header_policy_width)
        .max(1);

    let mut header_spans = vec![Span::styled(
        " Tool",
        Style::default()
            .fg(ThemeColors::dark_gray())
            .add_modifier(Modifier::BOLD),
    )];
    header_spans.push(Span::raw(" ".repeat(header_spacer)));
    for policy in &policy_columns {
        header_spans.push(Span::styled(
            format!(" {:^17}", policy_label(policy)),
            Style::default()
                .fg(ThemeColors::dark_gray())
                .add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(header_spans)), chunks[3]);

    // --- Rows ---
    let total = state.tool_approval_popup_state.visible_count();
    let start = state.tool_approval_popup_state.scroll.min(total);
    let end = (start + VISIBLE_ROWS).min(total);

    let visible_indices: Vec<usize> = (start..end)
        .filter_map(|i| state.tool_approval_popup_state.get_row_index(i))
        .collect();

    let mut lines = Vec::new();
    for (idx, &row_idx) in visible_indices.iter().enumerate() {
        let row = &state.tool_approval_popup_state.rows[row_idx];
        let is_selected = (start + idx) == state.tool_approval_popup_state.row_selected;
        let is_changed = row.policy != row.original_policy;

        let marker = if is_changed { "*" } else { " " };
        let row_bg = if is_selected {
            ThemeColors::unselected_bg()
        } else {
            Color::Reset
        };

        let total_width = chunks[4].width as usize;
        let policy_area_width = policy_columns.len() * policy_width;
        let reserved_prefix = marker.len() + 1;
        let tool_max_width = total_width
            .saturating_sub(reserved_prefix + 2 + policy_area_width)
            .max(8);
        let tool_name = truncate_chars(&row.tool_name, tool_max_width);

        let mut spans = vec![
            Span::styled(
                format!("{} ", marker),
                Style::default()
                    .fg(if is_selected {
                        ThemeColors::accent()
                    } else {
                        ThemeColors::dark_gray()
                    })
                    .bg(row_bg),
            ),
            Span::styled(
                tool_name.clone(),
                Style::default()
                    .fg(if is_selected {
                        ThemeColors::highlight_fg()
                    } else {
                        ThemeColors::text()
                    })
                    .add_modifier(if is_selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    })
                    .bg(row_bg),
            ),
        ];

        let left_used = marker.len() + 1 + tool_name.chars().count();
        let spacer = total_width
            .saturating_sub(left_used + policy_area_width)
            .max(1);
        spans.push(Span::styled(
            " ".repeat(spacer),
            Style::default().bg(row_bg),
        ));

        for (i, policy) in policy_columns.iter().enumerate() {
            let selected_policy = row.policy == *policy;
            let indicator = if selected_policy { "●" } else { "·" };
            let margin = if i + 1 == policy_columns.len() {
                " "
            } else {
                ""
            };
            let fg = if selected_policy {
                policy_color(policy)
            } else {
                ThemeColors::dark_gray()
            };
            spans.push(Span::styled(
                format!("{}{:^17}", margin, indicator),
                Style::default()
                    .fg(fg)
                    .bg(row_bg)
                    .add_modifier(if selected_policy {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ));
        }

        lines.push(Line::from(spans));
    }

    while lines.len() < VISIBLE_ROWS {
        lines.push(Line::from(""));
    }
    f.render_widget(Paragraph::new(lines), chunks[4]);

    // --- Footer ---
    let footer = Paragraph::new(Line::from(vec![
        Span::styled("↑/↓", Style::default().fg(ThemeColors::cyan())),
        Span::styled(" tool  ", Style::default().fg(ThemeColors::dark_gray())),
        Span::styled("←/→", Style::default().fg(ThemeColors::cyan())),
        Span::styled(" policy  ", Style::default().fg(ThemeColors::dark_gray())),
        Span::styled("type", Style::default().fg(ThemeColors::cyan())),
        Span::styled(" filter  ", Style::default().fg(ThemeColors::dark_gray())),
        Span::styled("enter", Style::default().fg(ThemeColors::cyan())),
        Span::styled(" apply  ", Style::default().fg(ThemeColors::dark_gray())),
        Span::styled("esc", Style::default().fg(ThemeColors::cyan())),
        Span::styled(" cancel", Style::default().fg(ThemeColors::dark_gray())),
    ]));
    f.render_widget(footer, chunks[5]);
}
