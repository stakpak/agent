// Shared layout utilities for TUI popup and overlay positioning.
// All popup rendering modules should import from here instead of defining their own copies.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Return a centered rectangle of the given percentage dimensions within `r`.
///
/// Both `percent_x` and `percent_y` are clamped to [0, 100]. The returned
/// `Rect` is the inner cell of a 3×3 percentage-based grid, so the popup
/// floats centred over whatever area is passed in.
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let percent_x = percent_x.min(100);
    let percent_y = percent_y.min(100);

    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
