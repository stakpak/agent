use crate::app::AppState;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

pub fn render_hint_or_shortcuts(f: &mut Frame, state: &AppState, area: Rect) {
    if state.show_shortcuts {
        let shortcuts = vec![
            Line::from("/ for commands       shift + enter or ctrl + j to insert newline"),
            Line::from("↵ to send message    ctrl + c to quit"),
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
