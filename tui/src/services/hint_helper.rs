use crate::services::detect_term::{detect_terminal, should_use_rgb_colors};
use crate::services::shell_mode::SHELL_PROMPT_PREFIX;
use crate::{app::AppState, services::detect_term::AdaptiveColors};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use stakpak_shared::models::integrations::openai::AgentModel;
use stakpak_shared::models::model_pricing::ContextAware;

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
            "Shell mode is on   Esc to exit",
            Style::default().fg(AdaptiveColors::dark_magenta()),
        ));
        f.render_widget(hint, area);
        return;
    }

    if state.show_shortcuts && state.input().is_empty() {
        let shortcuts = vec![
            Line::from("ctrl+p palette . @ files . / commands . ctrl+s shortcuts"),
            Line::from(format!(
                "{} shell mode . ↵ submit . ctrl+c quit . ctrl+f profile . ctrl+k rulebooks . ctrl+s shortcuts",
                SHELL_PROMPT_PREFIX.trim()
            )),
        ];
        let shortcuts_widget = Paragraph::new(shortcuts).style(Style::default().fg(Color::Cyan));
        f.render_widget(shortcuts_widget, area);
    } else if !state.show_sessions_dialog && !state.is_dialog_open && state.input().is_empty() {
        let context_info = state
            .llm_model
            .as_ref()
            .map(|model| model.context_info())
            .unwrap_or_default();
        let max_tokens = context_info.max_tokens as u32;
        let high_cost_warning =
            state.total_session_usage.total_tokens >= (max_tokens as f64 * 0.9) as u32;
        let approaching_max = (state.total_session_usage.total_tokens as f64 / max_tokens as f64)
            >= context_info.approach_warning_threshold;

        if state.latest_tool_call.is_some() && !high_cost_warning && !approaching_max {
            // Create a line with both hints - shortcuts on left, retry on right
            let shortcuts_text = "ctrl+p palette . @ files . / commands . ctrl+s shortcuts";
            let retry_text = "ctrl+r to retry last command in shell mode";

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
                "Fn/Option/Shift + drag to select"
            } else {
                ""
            };

            // Create spans for left and right alignment on first line
            let left_text = "ctrl+p palette . @ files . / commands . ctrl+s shortcuts";

            // Calculate spacing to align profile info to the right

            let right_text_content = {
                let mut s = String::new();

                // Add model info
                s.push_str("model ");
                s.push_str(&state.agent_model.to_string());

                // Add profile info only if side panel is hidden
                if !state.show_side_panel {
                    s.push_str(" | profile ");
                    s.push_str(&state.current_profile_name);
                }

                // Add rulebooks info
                s.push_str(" | ctrl+k rulebooks");
                s.push_str(" | ctrl+k rulebooks");
                s
            };

            let right_style = Style::default().fg(Color::DarkGray);

            // Left side content
            let left_spans = vec![Span::styled(left_text, Style::default().fg(Color::Cyan))];

            // Right side content
            let mut right_spans = vec![];

            if high_cost_warning || approaching_max {
                right_spans.push(Span::styled(right_text_content, right_style));
            } else {
                right_spans.push(Span::styled("model ", Style::default().fg(Color::DarkGray)));
                match state.agent_model {
                    AgentModel::Smart => {
                        right_spans.push(Span::styled("smart", Style::default().fg(Color::Cyan)));
                    }
                    AgentModel::Eco => {
                        right_spans
                            .push(Span::styled("eco", Style::default().fg(Color::LightGreen)));
                    }
                    AgentModel::Recovery => {
                        right_spans.push(Span::styled(
                            "recovery",
                            Style::default().fg(Color::LightBlue),
                        ));
                    }
                }

                // Show profile info only if side panel is hidden
                if !state.show_side_panel {
                    right_spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
                    right_spans.push(Span::styled(
                        "profile ",
                        Style::default().fg(Color::DarkGray),
                    ));
                    right_spans.push(Span::styled(
                        state.current_profile_name.clone(),
                        Style::default().fg(Color::Reset),
                    ));
                }

                right_spans.push(Span::styled(
                    " | ctrl+k rulebooks",
                    Style::default().fg(Color::DarkGray),
                ));
            }

            // Render both aligned to opposite sides
            let left_widget =
                Paragraph::new(Line::from(left_spans)).alignment(ratatui::layout::Alignment::Left);
            let right_widget = Paragraph::new(Line::from(right_spans))
                .alignment(ratatui::layout::Alignment::Right);

            f.render_widget(left_widget, area);
            f.render_widget(right_widget, area);

            // Add second line with select hint if available (Unix only)
            #[cfg(unix)]
            if !select_hint.is_empty() {
                // Render on next line (assuming area height > 1)
                // We need to create a new area or just rely on Paragraph handling?
                // Actually, if we use the same area but with a newline in content, it works.
                // But left_widget uses `Line::from(left_spans)`.

                // Let's create a NEW paragraph for the second line and render it.
                // We'll calculate a sub-area for the second line.
                if area.height > 1 {
                    let second_line_area = Rect {
                        x: area.x,
                        y: area.y + 1,
                        width: area.width,
                        height: 1, // Only 1 line
                    };

                    let select_hint_widget =
                        Paragraph::new(Span::styled(select_hint, Style::default().fg(Color::Cyan)));
                    f.render_widget(select_hint_widget, second_line_area);
                }
            }
        }
    } else if !state.show_sessions_dialog && !state.is_dialog_open {
        // Show auto-approve status
        let auto_approve_status = if state.auto_approve_manager.is_enabled() {
            "auto-approve is ON"
        } else {
            "auto-approve is OFF"
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

        // detect if terminal is vscode
        let terminal_info = detect_terminal();
        let terminal_name = terminal_info.emulator;
        let is_iterm2 = terminal_name == "iTerm2";
        let new_line_hint = if !is_iterm2 { "ctrl+j" } else { "shift+enter" };
        let hint = Paragraph::new(Span::styled(
            format!(
                "{} new line | {} | ctrl+o toggle auto-approve",
                new_line_hint, auto_approve_status
            ),
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
        spans_vec.push(Span::styled("ctrl+o", Style::default().fg(Color::Cyan)));
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
