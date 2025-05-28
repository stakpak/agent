use crate::app::AppState;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Block,
};

pub fn render_helper_dropdown(f: &mut Frame, state: &AppState, dropdown_area: Rect) {
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
