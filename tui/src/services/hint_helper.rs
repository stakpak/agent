use crate::services::detect_term::should_use_rgb_colors;
use crate::services::shell_mode::SHELL_PROMPT_PREFIX;
use crate::{app::AppState, services::detect_term::AdaptiveColors};
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

    if state.show_shell_mode && !state.is_dialog_open && !state.show_sessions_dialog {
        let hint = Paragraph::new(Span::styled(
            "Shell mode is on     '$' to undo shell mode",
            Style::default().fg(AdaptiveColors::dark_magenta()),
        ));
        f.render_widget(hint, area);
        return;
    }

    if state.show_shortcuts && state.input().is_empty() {
        let shortcuts = vec![
            Line::from(
                "/ for commands      PageUp/Down(Fn + ↑/↓) for fast scroll      shift + enter or ctrl + j to insert newline",
            ),
            Line::from(format!(
                "{} for shell mode    ↵ to send message    ctrl + c to quit    ctrl + r to retry    ctrl + p to switch profile    ctrl + k for rulebooks",
                SHELL_PROMPT_PREFIX.trim()
            )),
        ];
        let shortcuts_widget = Paragraph::new(shortcuts).style(Style::default().fg(Color::Cyan));
        f.render_widget(shortcuts_widget, area);
    } else if !state.show_sessions_dialog && !state.is_dialog_open && state.input().is_empty() {
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

            let retry_color = if should_use_rgb_colors() {
                Color::Yellow
            } else {
                Color::Cyan
            };
            let spans = vec![
                Span::styled(shortcuts_text, Style::default().fg(Color::Cyan)),
                Span::styled(" ".repeat(spacing), Style::default()),
                Span::styled(retry_text, Style::default().fg(retry_color)),
            ];

            let hint = Paragraph::new(Line::from(spans));
            f.render_widget(hint, area);
        } else {
            #[cfg(unix)]
            let select_hint = if state.mouse_capture_enabled {
                " . Fn/Option/Shift + drag to select text"
            } else {
                ""
            };

            // Create spans for left and right alignment
            #[cfg(unix)]
            let left_text = format!(
                "? for shortcuts . @ for files . / for commands{}",
                select_hint
            );
            #[cfg(not(unix))]
            let left_text = format!("? for shortcuts . @ for files . / for commands");

            let right_text = format!("profile {}", state.current_profile_name);

            // Calculate spacing to align profile info to the right
            let total_width = area.width as usize;
            let left_len = left_text.len();
            let right_len = profile_text.len() + rulebooks_text.len();
            let spacing = total_width.saturating_sub(left_len + right_len);

            let spans = vec![
                Span::styled(left_text, Style::default().fg(Color::Cyan)),
                Span::styled(" ".repeat(spacing), Style::default()),
                Span::styled("profile ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    state.current_profile_name.clone(),
                    Style::default().fg(Color::Reset),
                ),
                Span::styled(rulebooks_text, Style::default().fg(Color::DarkGray)),
            ];

            let hint = Paragraph::new(Line::from(spans));
            f.render_widget(hint, area);
        }
    } else if !state.show_sessions_dialog && !state.is_dialog_open {
        // Show auto-approve status
        let auto_approve_status = if state.auto_approve_manager.is_enabled() {
            "🔓 Auto-approve ON"
        } else {
            "🔒 Auto-approve OFF"
        };
        let status_color = if state.auto_approve_manager.is_enabled() {
            if should_use_rgb_colors() {
                Color::LightYellow
            } else {
                Color::Cyan
            }
        } else {
            Color::DarkGray
        };

        let hint = Paragraph::new(Span::styled(
            format!("{} | Ctrl+o: toggle auto-approve", auto_approve_status),
            Style::default().fg(status_color),
        ));
        f.render_widget(hint, area);
    } else if state.is_dialog_open {
        let mut spans_vec = vec![];
        if !state.approval_popup.is_visible() && state.message_tool_calls.is_some() {
            spans_vec.push(Span::styled("Enter", Style::default().fg(Color::Cyan)));
            spans_vec.push(Span::styled(
                " show approval popup . ",
                Style::default().fg(Color::Reset),
            ));
            spans_vec.push(Span::styled("Esc", Style::default().fg(Color::Red)));
            spans_vec.push(Span::styled(
                " reject all . ",
                Style::default().fg(Color::Reset),
            ));
        }
        spans_vec.push(Span::styled("Ctrl+o", Style::default().fg(Color::DarkGray)));
        spans_vec.push(Span::styled(
            " toggle auto-approve",
            Style::default().fg(Color::DarkGray),
        ));
        // Show focus information when dialog is open
        let hint =
            Paragraph::new(Line::from(spans_vec)).alignment(ratatui::layout::Alignment::Right);
        f.render_widget(hint, area);
    }
}
