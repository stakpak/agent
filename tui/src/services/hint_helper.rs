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

    if state.show_shell_mode {
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
                "{} for shell mode     ↵ to send message    ctrl + c to quit",
                SHELL_PROMPT_PREFIX.trim()
            )),
        ];
        let shortcuts_widget = Paragraph::new(shortcuts).style(Style::default().fg(Color::Cyan));
        f.render_widget(shortcuts_widget, area);
    } else if !state.show_sessions_dialog && !state.is_dialog_open && state.input.is_empty() {
        let hint = Paragraph::new(Span::styled(
            "? for shortcuts",
            Style::default().fg(Color::Cyan),
        ));
        f.render_widget(hint, area);
    } else if !state.show_sessions_dialog && !state.is_dialog_open {
        // Show auto-approve status
        let auto_approve_status = if state.auto_approve_manager.is_enabled() {
            "🔓 Auto-approve ON"
        } else {
            "🔒 Auto-approve OFF"
        };
        let status_color = if state.auto_approve_manager.is_enabled() {
            Color::Green
        } else {
            Color::Red
        };

        let hint = Paragraph::new(Span::styled(
            format!("{} | Ctrl+O: toggle auto-approve", auto_approve_status),
            Style::default().fg(status_color),
        ));
        f.render_widget(hint, area);
    } else if state.is_dialog_open {
        // Show focus information when dialog is open
        let focus_text = if state.dialog_focused {
            "Press Tab to focus Chat view. Dialog focused | Ctrl+O: toggle auto-approve | Ctrl+Y: auto-approve this tool"
        } else {
            "Press Tab to focus Dialog. Chat view focused | Ctrl+O: toggle auto-approve | Ctrl+Y: auto-approve this tool"
        };

        let hint = Paragraph::new(Span::styled(
            focus_text,
            Style::default().fg(Color::DarkGray),
        ))
        .alignment(ratatui::layout::Alignment::Right);
        f.render_widget(hint, area);
    }
}
