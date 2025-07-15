use crate::app::AppState;
use crate::services::shell_mode::SHELL_PROMPT_PREFIX;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

pub fn render_hint_or_shortcuts(f: &mut Frame, state: &AppState, area: Rect) {
    if state.show_shell_mode {
        let hint = Paragraph::new(Span::styled(
            "Shell mode is on     '$' to undo shell mode",
            Style::default().fg(Color::Rgb(160, 92, 158)),
        ));
        f.render_widget(hint, area);
        return;
    }

    if state.show_shortcuts {
        let shortcuts = vec![
            Line::from("/ for commands      PageUp/Down(Fn + ↑/↓) for fast scroll      shift + enter or ctrl + j to insert newline"),
            Line::from(format!(
                "{}for shell mode     ↵ to send message    ctrl + c to quit",
                SHELL_PROMPT_PREFIX.trim()
            )),
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
