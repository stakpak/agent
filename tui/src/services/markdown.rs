use ratatui::text::{Line, Span};

pub fn render_markdown_to_lines(markdown: &str, _width: usize) -> Vec<Line<'static>> {
    let text = tui_markdown::from_str(markdown);

    text.lines
        .into_iter()
        .map(|line| {
            Line::from(
                line.spans
                    .into_iter()
                    .map(|span| Span::styled(span.content.into_owned(), span.style))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}
