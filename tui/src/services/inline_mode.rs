use std::io::Stdout;

use crate::{
    services::message::{Message, get_wrapped_message_lines},
    view::render_processed_lines,
};
use ratatui::{
    backend::CrosstermBackend, layout::Rect, widgets::{Paragraph, Widget}, Terminal
};

pub fn push_inline_message(
    message: &Message,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> std::io::Result<()> {
    render_terminal_inline(terminal, &[message.clone()])
}

pub fn push_inline_history_messages(
    messages: &[Message],
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> std::io::Result<()> {
    render_terminal_inline(terminal, messages)
}


fn render_terminal_inline(terminal: &mut Terminal<CrosstermBackend<Stdout>>, lines: &[Message]) -> std::io::Result<()> {
    let width = terminal.size()?.width as usize; // 4 for padding
    let lines = get_wrapped_message_lines(lines, width);
    terminal.insert_before(lines.len() as u16, |buf| {
        let lines_vec = render_processed_lines(width, lines);
        let area = buf.area;
        let area = Rect {
            x: area.x + 2,
            y: area.y,
            width: area.width.saturating_sub(4),
            height: area.height,
        };
        Paragraph::new(lines_vec).render(area, buf);
    })?;
    Ok(())
}