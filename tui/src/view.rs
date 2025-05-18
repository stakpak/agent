use crate::app::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn view(f: &mut Frame, state: &AppState) {
    let input_height = 3;
    let margin_height = 2;
    let dropdown_showing = state.show_helper_dropdown
        && !state.filtered_helpers.is_empty()
        && state.input.starts_with('/');
    let dropdown_height = if dropdown_showing {
        state.filtered_helpers.len() as u16
    } else {
        0
    };
    let hint_height = if dropdown_showing { 0 } else { margin_height };

    let outer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(input_height as u16),
            Constraint::Length(dropdown_height),
            Constraint::Length(hint_height),
        ])
        .split(f.size());

    let message_area = outer_chunks[0];
    let input_area = outer_chunks[1];
    let dropdown_area = outer_chunks[2];
    let hint_area = outer_chunks[3];
    let message_area_width = message_area.width as usize;
    let message_area_height = message_area.height as usize;

    render_messages(
        f,
        state,
        message_area,
        message_area_width,
        message_area_height,
    );
    render_input(f, &state.input, input_area, state.cursor_position);
    render_helper_dropdown(f, state, dropdown_area);
    if !dropdown_showing {
        render_hint_or_shortcuts(f, state, hint_area);
    }
}

fn render_messages(f: &mut Frame, state: &AppState, area: Rect, width: usize, height: usize) {
    let mut all_lines: Vec<(Line, Style)> = Vec::new();
    for msg in &state.messages {
        for line in msg.text.lines() {
            let mut current = line;
            while !current.is_empty() {
                let take = current
                    .char_indices()
                    .scan(0, |acc, (i, c)| {
                        *acc += unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                        Some((i, *acc))
                    })
                    .take_while(|&(_i, w)| w <= width)
                    .last()
                    .map(|(i, _w)| i + 1)
                    .unwrap_or(current.len());
                if take == 0 {
                    // fallback: push the first char and advance
                    let ch_len = current.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                    let (part, rest) = current.split_at(ch_len);
                    all_lines.push((Line::from(vec![Span::styled(part, msg.style)]), msg.style));
                    current = rest;
                } else {
                    let (part, rest) = current.split_at(take);
                    all_lines.push((Line::from(vec![Span::styled(part, msg.style)]), msg.style));
                    current = rest;
                }
            }
        }
        all_lines.push((Line::from(""), msg.style));
    }
    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(height);
    let scroll = state.scroll.min(max_scroll);
    let mut visible_lines = Vec::new();
    for i in 0..height {
        if let Some((line, _)) = all_lines.get(scroll + i) {
            visible_lines.push(line.clone());
        } else {
            visible_lines.push(Line::from(""));
        }
    }
    let message_widget = Paragraph::new(visible_lines).wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(message_widget, area);
}

fn render_input(f: &mut Frame, input: &str, area: Rect, cursor_position: usize) {
    let mut spans = vec![Span::raw("> ")];
    let pos = cursor_position.min(input.len());
    let (before, after) = input.split_at(pos);
    spans.push(Span::raw(before));
    // Render a block cursor or caret
    let cursor_char = after.chars().next().unwrap_or(' ');
    spans.push(Span::styled(
        cursor_char.to_string(),
        Style::default()
            .bg(Color::Cyan)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
    ));
    if !after.is_empty() {
        let char_len = cursor_char.len_utf8();
        if after.len() > char_len {
            spans.push(Span::raw(&after[char_len..]));
        }
    }
    let input_widget = Paragraph::new(vec![Line::from(spans)])
        .style(Style::default())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(input_widget, area);
}

fn render_helper_dropdown(f: &mut Frame, state: &AppState, dropdown_area: Rect) {
    if state.show_helper_dropdown
        && !state.filtered_helpers.is_empty()
        && state.input.starts_with('/')
    {
        use ratatui::widgets::{List, ListItem, ListState};
        let item_style = Style::default().bg(Color::Black);
        let items: Vec<ListItem> = if state.input == "/" {
            state
                .helpers
                .iter()
                .map(|h| {
                    ListItem::new(Line::from(vec![Span::raw(format!("  {}  ", h))]))
                        .style(item_style)
                })
                .collect()
        } else {
            state
                .filtered_helpers
                .iter()
                .map(|h| {
                    ListItem::new(Line::from(vec![Span::raw(format!("  {}  ", h))]))
                        .style(item_style)
                })
                .collect()
        };
        let bg_block = Block::default().style(Style::default().bg(Color::Black));
        f.render_widget(bg_block, dropdown_area);
        let mut list_state = ListState::default();
        list_state.select(Some(
            state.helper_selected.min(items.len().saturating_sub(1)),
        ));
        let dropdown_widget = List::new(items)
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .bg(Color::DarkGray),
            )
            .block(Block::default());
        f.render_stateful_widget(dropdown_widget, dropdown_area, &mut list_state);
    }
}

fn render_hint_or_shortcuts(f: &mut Frame, state: &AppState, area: Rect) {
    if state.show_shortcuts {
        let shortcuts = vec![
            Line::from(
                "! for bash mode    double tap esc to undo    / for commands     # to memorize",
            ),
            Line::from(
                "↵ for newline      @ for file paths          shift + tab to auto-accept edits",
            ),
        ];
        let shortcuts_widget = Paragraph::new(shortcuts).style(Style::default().fg(Color::Cyan));
        f.render_widget(shortcuts_widget, area);
    } else {
        let hint = Paragraph::new(Span::styled(
            "? for shortcuts",
            Style::default().fg(Color::Cyan),
        ));
        f.render_widget(hint, area);
    }
}
