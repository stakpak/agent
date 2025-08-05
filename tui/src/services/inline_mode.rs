use std::io::Stdout;

use crate::{
    services::message::{Message, get_wrapped_message_lines},
    view::render_processed_lines,
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    widgets::{Paragraph, Widget},
};

pub fn push_inline_message(
    message: &Message,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> std::io::Result<()> {
    let width = terminal.size()?.width as usize;
    let lines = get_wrapped_message_lines(&[message.clone()], width);
    terminal.insert_before(lines.len() as u16, |buf| {
        let lines_vec = render_processed_lines(width, lines);
        Paragraph::new(lines_vec).render(buf.area, buf);
    })?;
    Ok(())
}

pub fn push_inline_history_messages(
    messages: &[Message],
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> std::io::Result<()> {
    let width = terminal.size()?.width as usize;
    let lines = get_wrapped_message_lines(messages, width);

    terminal.insert_before(lines.len() as u16, |buf| {
        let lines_vec = render_processed_lines(width, lines);
        Paragraph::new(lines_vec).render(buf.area, buf);
    })?;
    Ok(())
}
