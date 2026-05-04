use crate::app::AppState;
use crate::services::detect_term::ThemeColors;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

const OPTIONS: &[(&str, &str)] = &[
    ("Save to Profile", "~/.stakpak/config.toml"),
    ("Save to Project", ".stakpak/session/auto_approve.json"),
    ("Discard", "Revert changes"),
];

pub fn render_policy_persistence_popup(f: &mut Frame, state: &AppState) {
    let terminal_area = f.area();
    let width = (terminal_area.width * 55 / 100)
        .max(70)
        .min(terminal_area.width);
    // border(2) + title(1) + description(1) + separator(1) + options(3×2) + separator(1) + footer(1)
    let height: u16 = 12;
    let height = height.min(terminal_area.height);
    let x = terminal_area.width.saturating_sub(width) / 2;
    let y = terminal_area.height.saturating_sub(height) / 2;
    let area = Rect::new(x, y, width, height);

    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ThemeColors::yellow()));
    f.render_widget(block, area);

    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title
            Constraint::Length(1), // description
            Constraint::Length(1), // separator
            Constraint::Length(6), // options (3 × 2 lines each)
            Constraint::Length(1), // separator
            Constraint::Length(1), // footer
        ])
        .split(inner);

    // Title
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            " Unsaved Policy Changes ",
            Style::default()
                .fg(ThemeColors::yellow())
                .add_modifier(Modifier::BOLD),
        )])),
        chunks[0],
    );

    // Description
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            " Where would you like to save them?",
            Style::default().fg(ThemeColors::text()),
        )])),
        chunks[1],
    );

    // Separator
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "─".repeat(inner.width as usize),
            Style::default().fg(ThemeColors::dark_gray()),
        )])),
        chunks[2],
    );

    // Options
    let selected = state.approval_settings_persistence_state.selected;
    let mut option_lines: Vec<Line> = Vec::new();
    for (i, (title, path)) in OPTIONS.iter().enumerate() {
        let is_selected = i == selected;
        let marker = if is_selected { " ● " } else { "   " };

        let title_style = if is_selected {
            Style::default()
                .fg(ThemeColors::accent())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(ThemeColors::text())
        };

        let path_style = if is_selected {
            Style::default().fg(ThemeColors::cyan())
        } else {
            Style::default().fg(ThemeColors::dark_gray())
        };

        let bg = if is_selected {
            ThemeColors::unselected_bg()
        } else {
            ratatui::style::Color::Reset
        };

        option_lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(ThemeColors::accent()).bg(bg)),
            Span::styled(title.to_string(), title_style.bg(bg)),
        ]));
        option_lines.push(Line::from(vec![
            Span::raw("   "),
            Span::styled(format!("→ {}", path), path_style),
        ]));
    }

    f.render_widget(Paragraph::new(option_lines), chunks[3]);

    // Separator
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "─".repeat(inner.width as usize),
            Style::default().fg(ThemeColors::dark_gray()),
        )])),
        chunks[4],
    );

    // Footer
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("↑/↓", Style::default().fg(ThemeColors::cyan())),
            Span::styled(" select  ", Style::default().fg(ThemeColors::dark_gray())),
            Span::styled("enter", Style::default().fg(ThemeColors::cyan())),
            Span::styled(" confirm  ", Style::default().fg(ThemeColors::dark_gray())),
            Span::styled("1-3", Style::default().fg(ThemeColors::cyan())),
            Span::styled(
                " quick pick  ",
                Style::default().fg(ThemeColors::dark_gray()),
            ),
            Span::styled("esc", Style::default().fg(ThemeColors::cyan())),
            Span::styled(" cancel", Style::default().fg(ThemeColors::dark_gray())),
        ])),
        chunks[5],
    );
}
