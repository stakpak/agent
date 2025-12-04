use crate::app::AppState;
use crate::services::helper_block::format_number_with_separator;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Paragraph},
};
use stakpak_shared::models::{
    integrations::openai::AgentModel,
    llm::LLMModel,
    model_pricing::{ContextAware, ModelContextInfo},
};

/// Get context info from llm_model, or fallback to agent_model mapping
fn get_context_info(state: &AppState) -> ModelContextInfo {
    if let Some(model) = &state.llm_model {
        let info = model.context_info();
        // If we have a model but no pricing tiers, try to infer from agent_model
        if info.pricing_tiers.is_empty() {
            // Map agent_model to actual model string and get context_info
            let model_string = match state.agent_model {
                AgentModel::Smart => "claude-sonnet-4-5",
                AgentModel::Eco => "claude-haiku-4-5",
                AgentModel::Recovery => "gpt-5",
            };
            let mapped_model: LLMModel = model_string.to_string().into();
            mapped_model.context_info()
        } else {
            info
        }
    } else {
        // No llm_model set, use agent_model to determine model
        let model_string = match state.agent_model {
            AgentModel::Smart => "claude-sonnet-4-5",
            AgentModel::Eco => "claude-haiku-4-5",
            AgentModel::Recovery => "gpt-5",
        };
        let mapped_model: LLMModel = model_string.to_string().into();
        mapped_model.context_info()
    }
}

pub fn render_context_popup(f: &mut Frame, state: &AppState) {
    let screen = f.area();
    if screen.width < 30 || screen.height < 10 {
        return;
    }

    let available_width = screen.width.saturating_sub(2);
    let desired_width = 55;
    let min_width = 40;
    let popup_width = if available_width == 0 {
        0
    } else {
        desired_width
            .min(available_width)
            .max(min_width.min(available_width))
    };

    let available_height = screen.height.saturating_sub(2);
    let desired_height = 17;
    let min_height = 17;
    let popup_height = if available_height == 0 {
        0
    } else {
        desired_height
            .min(available_height)
            .max(min_height.min(available_height))
    };

    let right_edge = screen.x.saturating_add(screen.width);
    let popup_x = right_edge.saturating_sub(popup_width).max(screen.x + 1);

    let anchor_offset: u16 = 5;
    let popup_y = screen.y.saturating_add(
        screen
            .height
            .saturating_sub(popup_height.saturating_add(anchor_offset)),
    );

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Line::from(vec![
            Span::styled(
                "Context Utilization",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " · {}",
                    state
                        .llm_model
                        .as_ref()
                        .map(|model| model.model_name())
                        .unwrap_or("Unknown".to_string())
                ),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(" · ctrl+g", Style::default().fg(Color::DarkGray)),
        ]));
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // top padding
            Constraint::Length(3), // usage summary (allows wrapping + IO line)
            Constraint::Length(1), // gauge
            Constraint::Length(2), // markers
            Constraint::Length(7), // pricing table
            Constraint::Min(1),    // footer
        ])
        .split(inner);

    f.render_widget(Paragraph::new(""), layout[0]);
    render_usage_summary(f, state, layout[1]);
    render_usage_gauge(f, state, layout[2]);
    render_markers(f, state, layout[3]);
    render_pricing_table(f, state, layout[4]);
    render_footer(f, state, layout[5]);
}

fn render_usage_summary(f: &mut Frame, state: &AppState, area: Rect) {
    let usage = &state.current_message_usage;
    let total_tokens = usage.total_tokens;
    let formatted_total = format_number_with_separator(total_tokens);

    let context_info = get_context_info(state);
    let max_tokens = context_info.max_tokens;
    let formatted_max = format_number_with_separator(max_tokens as u32);

    let summary_lines = vec![
        Line::from(vec![
            Span::styled("Total: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} tokens", formatted_total),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ({}% of {})", state.context_usage_percent, formatted_max),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("Input: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format_number_with_separator(usage.prompt_tokens),
                Style::default().fg(Color::Reset),
            ),
            Span::styled("  ·  Output: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format_number_with_separator(usage.completion_tokens),
                Style::default().fg(Color::Reset),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(summary_lines);
    f.render_widget(paragraph, area);
}

fn render_usage_gauge(f: &mut Frame, state: &AppState, area: Rect) {
    let usage = &state.current_message_usage;
    let total_tokens = usage.total_tokens as f64;

    let context_info = get_context_info(state);
    let max_tokens = context_info.max_tokens;

    let ratio = (total_tokens / max_tokens as f64).clamp(0.0, 1.0);

    // Determine gauge color based on usage and thresholds
    let gauge_color = if ratio >= 0.9 {
        Color::Red
    } else if ratio >= context_info.approach_warning_threshold {
        Color::Yellow
    } else {
        Color::Green
    };

    let gauge = Gauge::default()
        .gauge_style(
            Style::default()
                .fg(gauge_color)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .label(Span::styled(
            format!("{}%", state.context_usage_percent),
            Style::default()
                .fg(Color::Black)
                .bg(gauge_color)
                .add_modifier(Modifier::BOLD),
        ))
        .ratio(ratio);

    f.render_widget(gauge, area);
}

fn render_markers(f: &mut Frame, state: &AppState, area: Rect) {
    let marker_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(area);

    let zero = Paragraph::new(Line::from("0")).alignment(Alignment::Left);

    let context_info = get_context_info(state);
    let max_tokens = context_info.max_tokens;

    // Find the first threshold that is not 0 cost, if any, to show as a marker
    let cost_marker_text = context_info
        .pricing_tiers
        .iter()
        .find(|tier| {
            tier.upper_bound.is_some()
                && (tier.input_cost_per_million > 0.0 || tier.output_cost_per_million > 0.0)
        })
        .and_then(|tier| tier.upper_bound)
        .map(|bound| format_number_with_separator(bound as u32).to_string())
        .unwrap_or_default();

    let cost_marker = Paragraph::new(Line::from(cost_marker_text))
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Yellow));

    let limit_marker = Paragraph::new(Line::from(format!(
        "{} max",
        format_number_with_separator(max_tokens as u32)
    )))
    .alignment(Alignment::Right)
    .style(Style::default().fg(Color::DarkGray));

    f.render_widget(zero, marker_layout[0]);
    f.render_widget(cost_marker, marker_layout[1]);
    f.render_widget(limit_marker, marker_layout[2]);
}

fn render_pricing_table(f: &mut Frame, state: &AppState, area: Rect) {
    if area.width < 20 {
        return;
    }

    if state.llm_model.is_none() {
        return;
    }

    let context_info = get_context_info(state);
    let pricing_table = &context_info.pricing_tiers;
    let table_len = pricing_table.len();

    let headers = ["Price Tier", "Input", "Output"];
    let mut min_widths = headers.map(|h| h.len() + 2);

    // Pre-calculate formatted strings to determine widths
    let formatted_rows: Vec<(String, String, String)> = pricing_table
        .iter()
        .map(|tier| {
            (
                tier.label.clone(),
                format!("${:.2}/1M", tier.input_cost_per_million),
                format!("${:.2}/1M", tier.output_cost_per_million),
            )
        })
        .collect();

    for (label, input, output) in &formatted_rows {
        min_widths[0] = min_widths[0].max(label.len() + 2);
        min_widths[1] = min_widths[1].max(input.len() + 2);
        min_widths[2] = min_widths[2].max(output.len() + 2);
    }

    let ratios = [4, 3, 3];
    let total_width = area.width as usize;
    let column_count = headers.len();
    let separators = column_count + 1;

    let total_tokens = state.current_message_usage.total_tokens as u64;
    let mut current_lower_bound = 0;

    if total_width < min_widths.iter().sum::<usize>() + separators {
        // Fallback: render a simplified list if space is too tight
        let mut lines = Vec::new();
        for (idx, tier) in pricing_table.iter().enumerate() {
            let is_active = match tier.upper_bound {
                Some(upper) => total_tokens >= current_lower_bound && total_tokens < upper,
                None => total_tokens >= current_lower_bound,
            };

            if let Some(upper) = tier.upper_bound {
                current_lower_bound = upper;
            }

            let bullet = if is_active { ">" } else { "-" };

            let style = if is_active {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let (label, input, output) = &formatted_rows[idx];

            lines.push(Line::from(vec![Span::styled(
                format!("{} {} · {} / {}", bullet, label, input, output),
                style,
            )]));
            if idx < table_len - 1 {
                lines.push(Line::from(""));
            }
        }
        let paragraph = Paragraph::new(lines).alignment(Alignment::Left);
        f.render_widget(paragraph, area);
        return;
    }

    let mut widths = min_widths.to_vec();
    let current_total = widths.iter().sum::<usize>();
    let remaining = total_width.saturating_sub(current_total + separators);

    if remaining > 0 {
        let ratio_sum: usize = ratios.iter().sum();
        for (i, ratio) in ratios.iter().enumerate() {
            let additional = remaining * ratio / ratio_sum;
            widths[i] += additional;
        }
        let mut distributed = widths.iter().sum::<usize>() + separators;
        while distributed < total_width {
            for width in widths.iter_mut() {
                if distributed >= total_width {
                    break;
                }
                *width += 1;
                distributed += 1;
            }
        }
    }

    let border_color = Color::DarkGray;
    let header_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let mut lines = Vec::new();
    lines.push(border_line('┌', '┬', '┐', '─', &widths, border_color));
    lines.push(build_row_line(headers, &widths, header_style, border_color));
    lines.push(border_line('├', '┼', '┤', '─', &widths, border_color));

    // Reset for main table loop
    current_lower_bound = 0;

    for (idx, tier) in pricing_table.iter().enumerate() {
        let is_active = match tier.upper_bound {
            Some(upper) => total_tokens >= current_lower_bound && total_tokens < upper,
            None => total_tokens >= current_lower_bound,
        };

        if let Some(upper) = tier.upper_bound {
            current_lower_bound = upper;
        }

        let row_style = if is_active {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let (label, input, output) = &formatted_rows[idx];

        lines.push(build_row_line(
            [label.as_str(), input.as_str(), output.as_str()],
            &widths,
            row_style,
            border_color,
        ));

        if idx == table_len - 1 {
            lines.push(border_line('└', '┴', '┘', '─', &widths, border_color));
        } else {
            lines.push(border_line('├', '┼', '┤', '─', &widths, border_color));
        }
    }

    let paragraph = Paragraph::new(lines)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .alignment(Alignment::Left);
    f.render_widget(paragraph, area);
}

fn border_line(
    left: char,
    mid: char,
    right: char,
    fill: char,
    widths: &[usize],
    border_color: Color,
) -> Line<'static> {
    let mut line = String::new();
    line.push(left);
    for (idx, width) in widths.iter().enumerate() {
        for _ in 0..*width {
            line.push(fill);
        }
        if idx == widths.len() - 1 {
            line.push(right);
        } else {
            line.push(mid);
        }
    }
    Line::from(vec![Span::styled(line, Style::default().fg(border_color))])
}

fn build_row_line(
    cells: [&str; 3],
    widths: &[usize],
    text_style: Style,
    border_color: Color,
) -> Line<'static> {
    let border_style = Style::default().fg(border_color);
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled("│".to_string(), border_style));

    for (cell, width) in cells.iter().zip(widths.iter()) {
        let inner_width = width.saturating_sub(2);
        let truncated: String = cell.chars().take(inner_width).collect();
        let padded = if inner_width > 0 {
            format!("{:<width$}", truncated, width = inner_width)
        } else {
            String::new()
        };

        let mut cell_text = String::new();
        cell_text.push(' ');
        if inner_width > 0 {
            cell_text.push_str(&padded);
        }
        cell_text.push(' ');

        spans.push(Span::styled(cell_text, text_style));
        spans.push(Span::styled("│".to_string(), border_style));
    }

    Line::from(spans)
}

fn render_footer(f: &mut Frame, state: &AppState, area: Rect) {
    let total_tokens = state.current_message_usage.total_tokens;

    let context_info = get_context_info(state);
    let max_tokens = context_info.max_tokens;
    let ratio = (total_tokens as f64 / max_tokens as f64).clamp(0.0, 1.0);

    let message = if ratio >= context_info.approach_warning_threshold {
        format!(
            "Approaching the {} token limit. Try /summarize or /model.",
            format_number_with_separator(max_tokens as u32)
        )
    } else {
        "".to_string()
    };

    let paragraph = Paragraph::new(Line::from(message))
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    f.render_widget(paragraph, area);
}
