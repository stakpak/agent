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
    if !state.input.is_empty()
        && !state.show_shell_mode
        && !state.show_sessions_dialog
        && !state.is_dialog_open
    {
        let hint = Paragraph::new(Span::styled(
            "Press Enter to send",
            Style::default().fg(Color::DarkGray),
        ));
        f.render_widget(hint, area);
        return;
    }
    if state.is_pasting {
        let hint = Paragraph::new(Span::styled(
            "Pasting text...",
            Style::default().fg(Color::DarkGray),
        ));
        f.render_widget(hint, area);
        return;
    }
    if state.ctrl_c_pressed_once && state.ctrl_c_timer.is_some() {
        let hint = Paragraph::new(Span::styled(
            "Press Ctrl+C again to exit Stakpak",
            Style::default().fg(Color::DarkGray),
        ));
        f.render_widget(hint, area);
        return;
    }

    if state.show_shell_mode && !state.is_dialog_open && !state.show_sessions_dialog {
        let hint = Paragraph::new(Span::styled(
            "Shell mode is on     '$' to undo shell mode",
            Style::default().fg(Color::Rgb(160, 92, 158)),
        ));
        f.render_widget(hint, area);
        return;
    }

    if state.show_shortcuts && state.input.is_empty() {
        let shortcuts = vec![
            Line::from(
                "/ for commands      PageUp/Down(Fn + ↑/↓) for fast scroll      shift + enter or ctrl + j to insert newline",
            ),
            Line::from(format!(
                "{} for shell mode    ↵ to send message    ctrl + c to quit    ctrl + r to retry",
                SHELL_PROMPT_PREFIX.trim()
            )),
        ];
        let shortcuts_widget = Paragraph::new(shortcuts).style(Style::default().fg(Color::Cyan));
        f.render_widget(shortcuts_widget, area);
    } else if !state.show_sessions_dialog && !state.is_dialog_open && state.input.is_empty() {
        // Show both hints when appropriate
        if state.latest_tool_call.is_some() {
            // Create a line with both hints - shortcuts on left, retry on right
            let shortcuts_text = "? for shortcuts";
            let retry_text = "Ctrl+R to retry last command in shell mode";

            // Calculate spacing to align retry hint to the right
            let total_width = area.width as usize;
            let shortcuts_len = shortcuts_text.len();
            let retry_len = retry_text.len();
            let spacing = total_width.saturating_sub(shortcuts_len + retry_len);

            let spans = vec![
                Span::styled(shortcuts_text, Style::default().fg(Color::DarkGray)),
                Span::styled(" ".repeat(spacing), Style::default()),
                Span::styled(retry_text, Style::default().fg(Color::Yellow)),
            ];

            let hint = Paragraph::new(Line::from(spans));
            f.render_widget(hint, area);
        } else {
            let hint = Paragraph::new(Span::styled(
                "? for shortcuts",
                Style::default().fg(Color::DarkGray),
            ));
            f.render_widget(hint, area);
        }
    }
}
