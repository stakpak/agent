use crate::app::AppState;
use crate::services::detect_term::{ThemeColors, detect_terminal};
use crate::services::shell_mode::SHELL_PROMPT_PREFIX;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

pub fn render_hint_or_shortcuts(f: &mut Frame, state: &AppState, area: Rect) {
    if state.input_state.is_pasting {
        let hint = Paragraph::new(Span::styled(
            "Pasting text...",
            Style::default().fg(ThemeColors::dark_gray()),
        ));
        f.render_widget(hint, area);
        return;
    }
    if state.ctrl_c_pressed_once && state.ctrl_c_timer.is_some() {
        let hint = Paragraph::new(Span::styled(
            "Press Ctrl+C again to exit Stakpak",
            Style::default().fg(ThemeColors::dark_gray()),
        ));
        f.render_widget(hint, area);
        return;
    }

    if state.shell_popup_state.show_shell_mode && !state.dialog_approval_state.is_dialog_open {
        let hint = Paragraph::new(Span::styled(
            "Shell mode is on   Esc to exit",
            Style::default().fg(ThemeColors::magenta()),
        ));
        f.render_widget(hint, area);
        return;
    }

    if state.dialog_approval_state.show_shortcuts && state.input().is_empty() {
        let shortcuts = vec![
            Line::from("$ shell . / commands . ctrl+s shortcuts"),
            Line::from(format!(
                "{} shell mode . ↵ submit . ctrl+c quit . ctrl+f profile . ctrl+k rulebooks . ctrl+s shortcuts",
                SHELL_PROMPT_PREFIX.trim()
            )),
        ];
        let shortcuts_widget =
            Paragraph::new(shortcuts).style(Style::default().fg(ThemeColors::dark_gray()));
        f.render_widget(shortcuts_widget, area);
    } else if !state.dialog_approval_state.is_dialog_open && state.input().is_empty() {
        // Use current_model if set (from streaming), otherwise use default model
        let active_model = state.current_model.as_ref().unwrap_or(&state.model);
        let max_tokens = active_model.limit.context as u32;
        // Use current message's prompt_tokens for context window warnings
        // (prompt_tokens represents the actual context size, not accumulated across messages)
        let current_context = state.current_message_usage.prompt_tokens;
        let high_cost_warning = current_context >= (max_tokens as f64 * 0.9) as u32;
        let approaching_max = (current_context as f64 / max_tokens as f64) >= 0.8; // Default threshold

        {
            #[cfg(unix)]
            let select_hint = if state.mouse_capture_enabled {
                "Fn/Option/Shift + drag to select"
            } else {
                ""
            };

            // Helper text for the right side
            let helper_text = "$ shell | / commands | ctrl+s shortcuts";

            // Left side: loader if loading, otherwise empty
            let mut left_spans = Vec::new();
            if state.loading_state.is_loading {
                let spinner_chars = ["▄▀", "▐▌", "▀▄", "▐▌"];
                let spinner = spinner_chars[state.loading_state.spinner_frame % spinner_chars.len()];
                let spinner_text = if state.loading_state.loading_type == crate::app::LoadingType::Sessions {
                    "Loading sessions..."
                } else {
                    "Stakpaking..."
                };

                left_spans.push(Span::styled(
                    format!("{} {}", spinner, spinner_text),
                    Style::default()
                        .fg(ThemeColors::orange())
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ));

                if state.loading_state.loading_type == crate::app::LoadingType::Llm {
                    left_spans.push(Span::styled(
                        " - esc cancel",
                        Style::default().fg(ThemeColors::dark_gray()),
                    ));
                }
            }

            // Right side: helper text (always on right), plus profile info if side panel hidden
            let mut right_spans = vec![Span::styled(
                helper_text,
                Style::default().fg(ThemeColors::dark_gray()),
            )];

            // Add profile info only if side panel is hidden and not loading heavily
            if !state.show_side_panel && !high_cost_warning && !approaching_max {
                right_spans.push(Span::styled(
                    " | profile ",
                    Style::default().fg(ThemeColors::dark_gray()),
                ));
                right_spans.push(Span::styled(
                    state.current_profile_name.clone(),
                    Style::default().fg(Color::Reset),
                ));
                right_spans.push(Span::styled(
                    " | ctrl+y side panel",
                    Style::default().fg(ThemeColors::dark_gray()),
                ));
            }

            // Render left (loader) and right (helper text) aligned to opposite sides
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

                    let select_hint_widget = Paragraph::new(Span::styled(
                        select_hint,
                        Style::default().fg(ThemeColors::cyan()),
                    ));
                    f.render_widget(select_hint_widget, second_line_area);
                }
            }
        }
    } else if state.dialog_approval_state.approval_bar.is_visible() {
        // Show approval bar hints
        let spans_vec = if state.dialog_approval_state.approval_bar.is_esc_pending() {
            // After first ESC: show confirmation hint
            vec![
                Span::styled("Enter", Style::default().fg(ThemeColors::green())),
                Span::styled(
                    " show approval bar . ",
                    Style::default().fg(ThemeColors::dark_gray()),
                ),
                Span::styled("Esc", Style::default().fg(ThemeColors::red())),
                Span::styled(" reject all", Style::default().fg(ThemeColors::dark_gray())),
            ]
        } else {
            // Normal approval bar hints
            vec![
                Span::styled("←→", Style::default().fg(ThemeColors::cyan())),
                Span::styled(
                    " navigate . ",
                    Style::default().fg(ThemeColors::dark_gray()),
                ),
                Span::styled("Space", Style::default().fg(ThemeColors::cyan())),
                Span::styled(" toggle . ", Style::default().fg(ThemeColors::dark_gray())),
                Span::styled("Enter", Style::default().fg(ThemeColors::green())),
                Span::styled(
                    " accept all . ",
                    Style::default().fg(ThemeColors::dark_gray()),
                ),
                Span::styled("Esc", Style::default().fg(ThemeColors::red())),
                Span::styled(" reject all", Style::default().fg(ThemeColors::dark_gray())),
            ]
        };
        let hint =
            Paragraph::new(Line::from(spans_vec)).alignment(ratatui::layout::Alignment::Right);
        f.render_widget(hint, area);
    } else if !state.dialog_approval_state.is_dialog_open {
        let status_color = ThemeColors::dark_gray();

        // detect if terminal is vscode
        let terminal_info = detect_terminal();
        let terminal_name = terminal_info.emulator;
        let is_iterm2 = terminal_name == "iTerm2";
        let new_line_hint = if !is_iterm2 { "ctrl+j" } else { "shift+enter" };
        let hint = Paragraph::new(Span::styled(
            format!("{} new line | @ files", new_line_hint),
            Style::default().fg(status_color),
        ));
        f.render_widget(hint, area);
    } else if state.dialog_approval_state.is_dialog_open {
        let mut spans_vec = vec![];
        if !state.dialog_approval_state.approval_bar.is_visible() && state.dialog_approval_state.message_tool_calls.is_some() {
            spans_vec.push(Span::styled(
                "Enter",
                Style::default().fg(ThemeColors::cyan()),
            ));
            spans_vec.push(Span::styled(
                " show approval bar . ",
                Style::default().fg(Color::Reset),
            ));
            spans_vec.push(Span::styled("Esc", Style::default().fg(ThemeColors::red())));
            spans_vec.push(Span::styled(
                " reject all . ",
                Style::default().fg(Color::Reset),
            ));
        }
        spans_vec.push(Span::styled(
            "ctrl+o",
            Style::default().fg(ThemeColors::cyan()),
        ));
        spans_vec.push(Span::styled(
            " toggle auto-approve",
            Style::default().fg(ThemeColors::dark_gray()),
        ));
        // Show focus information when dialog is open
        let hint =
            Paragraph::new(Line::from(spans_vec)).alignment(ratatui::layout::Alignment::Right);
        f.render_widget(hint, area);
    }
}
