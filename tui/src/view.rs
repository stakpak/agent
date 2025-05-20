use crate::app::{AppState, MessageContent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use serde_json::Value;

pub fn view(f: &mut Frame, state: &AppState) {
    // Calculate the required height for the input area based on content
    let input_area_width = f.size().width.saturating_sub(4) as usize;
    let input_lines = calculate_input_lines(&state.input, input_area_width); // -4 for borders and padding
    let input_height = (input_lines + 2) as u16; // +2 for border

    let margin_height = 2;
    let dropdown_showing = state.show_helper_dropdown
        && !state.filtered_helpers.is_empty()
        && state.input.starts_with('/');
    let dropdown_height = if dropdown_showing {
        state.filtered_helpers.len() as u16
    } else {
        0
    };
    let hint_height = if dropdown_showing { 0 } else { margin_height };

    let dialog_height = if state.is_dialog_open { 9 } else { 0 }; // adjust as needed
    let dialog_margin = if state.is_dialog_open { 1 } else { 0 };

    // Layout: [messages][dialog_margin][dialog][input][dropdown][hint]
    let chunks = ratatui::layout::Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1), // messages
            Constraint::Length(dialog_margin),
            Constraint::Length(dialog_height),
            Constraint::Length(input_height),
            Constraint::Length(dropdown_height),
            Constraint::Length(hint_height),
        ])
        .split(f.size());

    let message_area = chunks[0];
    let input_area = chunks[3];
    let dropdown_area = chunks[4];
    let hint_area = chunks[5];
    let message_area_width = message_area.width as usize;
    let message_area_height = message_area.height as usize;

    render_messages(
        f,
        state,
        message_area,
        message_area_width,
        message_area_height,
    );
    // Only reserve and render dialog if open
    if state.is_dialog_open {
        render_confirmation_dialog(f, state);
    }
    // Only render input, dropdown, and hint if dialog is not open
    if !state.is_dialog_open {
        render_multiline_input(f, state, input_area);
        render_helper_dropdown(f, state, dropdown_area);
        if !dropdown_showing {
            render_hint_or_shortcuts(f, state, hint_area);
        }
    }
    // Loader: still as a message at the end of the message list
}

// Calculate how many lines the input will take up when wrapped
fn calculate_input_lines(input: &str, width: usize) -> usize {
    if input.is_empty() {
        return 1; // At least one line
    }

    let prompt_width = 2; // "> " prefix
    let first_line_width = width.saturating_sub(prompt_width);
    let available_width = width;
    if available_width <= 1 {
        return input.len(); // Fallback if width is too small
    }

    // Split by explicit newlines first
    let mut total_lines = 0;
    for line in input.split('\n') {
        // For each line segment after splitting by newlines
        let mut words = line.split_whitespace().peekable();
        let mut current_width = 0;
        let mut is_first_line_in_segment = true;

        while words.peek().is_some() {
            let word = words.next().unwrap();
            let word_width = word
                .chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
                .sum::<usize>();

            // Determine available width for this line
            let line_width_limit = if is_first_line_in_segment && total_lines == 0 {
                first_line_width
            } else {
                available_width
            };

            // Add space before word (except at start of line)
            if current_width > 0 {
                current_width += 1; // Space width
            }

            // Check if word fits on current line
            if current_width + word_width <= line_width_limit {
                current_width += word_width;
            } else {
                // Word doesn't fit, start new line
                total_lines += 1;
                current_width = word_width;
                is_first_line_in_segment = false;
            }
        }

        total_lines += 1;
    }

    total_lines
}

fn render_messages(f: &mut Frame, state: &AppState, area: Rect, width: usize, height: usize) {
    let mut all_lines: Vec<(Line, Style)> = Vec::new();
    for msg in &state.messages {
        match &msg.content {
            MessageContent::Plain(text, style) => {
                for line in text.lines() {
                    let mut current = line;
                    while !current.is_empty() {
                        let take = current
                            .char_indices()
                            .scan(0, |acc, (i, c)| {
                                *acc += unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                                Some((i, *acc))
                            })
                            .take_while(|&(_i, w)| w <= width)
                            .last()
                            .map(|(i, _w)| i + 1)
                            .unwrap_or(current.len());
                        if take == 0 {
                            let ch_len = current.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                            let (part, rest) = current.split_at(ch_len);
                            all_lines.push((Line::from(vec![Span::styled(part, *style)]), *style));
                            current = rest;
                        } else {
                            let (part, rest) = current.split_at(take);
                            all_lines.push((Line::from(vec![Span::styled(part, *style)]), *style));
                            current = rest;
                        }
                    }
                }
                all_lines.push((Line::from(""), *style));
            }
            MessageContent::Styled(line) => {
                all_lines.push((line.clone(), Style::default()));
                all_lines.push((Line::from(""), Style::default()));
            }
            MessageContent::StyledBlock(lines) => {
                for line in lines {
                    all_lines.push((line.clone(), Style::default()));
                }
            }
        }
    }
    // Add loader as a new message line if loading
    if state.loading {
        let spinner_chars = ["|", "/", "-", "\\"];
        let spinner = spinner_chars[state.spinner_frame % spinner_chars.len()];
        let loading_line = Line::from(vec![Span::styled(
            format!("{} Stakpaking...", spinner),
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        )]);
        all_lines.push((loading_line, Style::default()));
    }
    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(height);
    // If stay_at_bottom, always scroll to the bottom (show last messages above dialog if open)
    let scroll = if state.stay_at_bottom {
        max_scroll
    } else {
        state.scroll.min(max_scroll)
    };
    let mut visible_lines = Vec::new();
    for i in 0..height {
        if let Some((line, _)) = all_lines.get(scroll + i) {
            visible_lines.push(line.clone());
        } else {
            visible_lines.push(Line::from(""));
        }
    }
    let message_widget = Paragraph::new(visible_lines).wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(message_widget, area);
}

fn render_multiline_input(f: &mut Frame, state: &AppState, area: Rect) {
    // Make a copy of input to avoid borrowing issues
    let input = state.input.clone();
    let available_width = area.width.saturating_sub(4) as usize; // -4 for borders and padding

    // Ensure the cursor position is valid
    let cursor_pos = state.cursor_position.min(input.len());

    // Split the input by newlines first
    let line_segments: Vec<&str> = input.split('\n').collect();

    let mut lines = Vec::new();
    let mut cursor_rendered = false;

    // Track position in the input string (in bytes)
    let mut current_pos = 0;

    for (segment_idx, segment) in line_segments.iter().enumerate() {
        let mut current_line = Vec::new();
        // Add prompt to first line only
        let prompt = if segment_idx == 0 { "> " } else { "" };
        let prompt_width = prompt.len();
        current_line.push(Span::raw(prompt));

        let mut current_width = prompt_width;

        // Process this line segment
        let mut word_segments = Vec::new();
        let mut current_word = String::new();
        let mut in_word = false;

        // Split segment into words and spaces, preserving exact positions
        for (i, c) in segment.char_indices() {
            let byte_pos = current_pos + i;

            // Render cursor if it's at this exact position
            if byte_pos == cursor_pos && !cursor_rendered {
                if in_word {
                    // End current word before cursor
                    if !current_word.is_empty() {
                        word_segments.push((current_word.clone(), false));
                        current_word.clear();
                    }
                }

                // Add the cursor
                word_segments.push((c.to_string(), true));
                cursor_rendered = true;
                in_word = !c.is_whitespace();

                if in_word {
                    current_word.push(c);
                }
            } else if c.is_whitespace() {
                // End current word if any
                if in_word && !current_word.is_empty() {
                    word_segments.push((current_word.clone(), false));
                    current_word.clear();
                    in_word = false;
                }

                // Add the whitespace
                word_segments.push((c.to_string(), false));
            } else {
                // Part of a word
                current_word.push(c);
                in_word = true;
            }
        }

        // Add any remaining word
        if in_word && !current_word.is_empty() {
            word_segments.push((current_word, false));
        }

        // If cursor is at the end of this segment
        if current_pos + segment.len() == cursor_pos && !cursor_rendered {
            word_segments.push((" ".to_string(), true));
            cursor_rendered = true;
        }

        // Render the word segments with proper wrapping
        for (text, is_cursor) in word_segments {
            let text_width = text
                .chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
                .sum::<usize>();

            // Check if this segment would exceed line width
            let needs_wrap = !text.trim().is_empty()
                && current_width > prompt_width
                && current_width + text_width > available_width;

            if needs_wrap {
                // Add current line and start a new one
                lines.push(Line::from(std::mem::take(&mut current_line)));
                current_line = Vec::new();
                current_width = 0;
            }

            // Add the segment (with or without cursor highlighting)
            if is_cursor {
                current_line.push(Span::styled(
                    text,
                    Style::default()
                        .bg(Color::Cyan)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                current_line.push(Span::raw(text));
            }

            current_width += text_width;
        }

        // Add this line
        lines.push(Line::from(std::mem::take(&mut current_line)));

        // Move to next segment
        current_pos += segment.len() + 1; // +1 for newline
    }

    // If cursor is at the very end and we haven't rendered it yet
    if cursor_pos == input.len() && !cursor_rendered {
        // If the last line is empty, add cursor there
        if let Some(last_line) = lines.last_mut() {
            last_line.spans.push(Span::styled(
                " ",
                Style::default()
                    .bg(Color::Cyan)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            // Create a new line with prompt and cursor
            lines.push(Line::from(vec![
                Span::raw("> "),
                Span::styled(
                    " ",
                    Style::default()
                        .bg(Color::Cyan)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
    }

    // Ensure we have at least one line
    if lines.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("> "),
            Span::styled(
                " ",
                Style::default()
                    .bg(Color::Cyan)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // Render the input widget
    let input_widget = Paragraph::new(lines)
        .style(Style::default())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(input_widget, area);
}

fn render_helper_dropdown(f: &mut Frame, state: &AppState, dropdown_area: Rect) {
    if state.show_helper_dropdown
        && !state.filtered_helpers.is_empty()
        && state.input.starts_with('/')
    {
        use ratatui::widgets::{List, ListItem, ListState};
        let item_style = Style::default().bg(Color::Black);
        let items: Vec<ListItem> = if state.input == "/" {
            state
                .helpers
                .iter()
                .map(|h| {
                    ListItem::new(Line::from(vec![Span::raw(format!("  {}  ", h))]))
                        .style(item_style)
                })
                .collect()
        } else {
            state
                .filtered_helpers
                .iter()
                .map(|h| {
                    ListItem::new(Line::from(vec![Span::raw(format!("  {}  ", h))]))
                        .style(item_style)
                })
                .collect()
        };
        let bg_block = Block::default().style(Style::default().bg(Color::Black));
        f.render_widget(bg_block, dropdown_area);
        let mut list_state = ListState::default();
        list_state.select(Some(
            state.helper_selected.min(items.len().saturating_sub(1)),
        ));
        let dropdown_widget = List::new(items)
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .bg(Color::DarkGray),
            )
            .block(Block::default());
        f.render_stateful_widget(dropdown_widget, dropdown_area, &mut list_state);
    }
}

fn render_hint_or_shortcuts(f: &mut Frame, state: &AppState, area: Rect) {
    if state.show_shortcuts {
        let shortcuts = vec![
            Line::from(
                "! for bash mode    double tap esc to undo    / for commands     # to memorize",
            ),
            Line::from(
                "↵ for newline      @ for file paths          shift + tab to auto-accept edits",
            ),
        ];
        let shortcuts_widget = Paragraph::new(shortcuts).style(Style::default().fg(Color::Cyan));
        f.render_widget(shortcuts_widget, area);
    } else {
        let hint = Paragraph::new(Span::styled(
            "? for shortcuts",
            Style::default().fg(Color::Cyan),
        ));
        f.render_widget(hint, area);
    }
}

fn render_confirmation_dialog(f: &mut Frame, state: &AppState) {
    use ratatui::layout::Alignment;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Paragraph};
    let screen = f.size();
    // Calculate the number of visible message lines
    let message_lines =
        crate::app::get_wrapped_message_lines(&state.messages, screen.width as usize);
    let mut last_message_y = message_lines.len() as u16 + 1; // +1 for a gap
    // Clamp so dialog fits on screen
    let dialog_height = 9;
    if last_message_y + dialog_height > screen.height {
        last_message_y = screen.height.saturating_sub(dialog_height + 5);
    }
    let area = ratatui::layout::Rect {
        x: 1,
        y: last_message_y,
        width: screen.width - 2,
        height: dialog_height as u16,
    };

    let command_name = serde_json::from_str::<Value>(&state.dialog_command.as_ref().unwrap().function.arguments)
    .ok()
    .and_then(|v| v.get("command").and_then(|c| c.as_str()).map(|s| s.to_string()))
    .unwrap_or_else(|| "?".to_string());
   

    let max_title_width = area.width.saturating_sub(12) as usize; 
    let mut title_lines = vec![];
    let mut current = command_name.as_str();
    while !current.is_empty() {
        let take = current
            .char_indices()
            .scan(0, |acc, (i, c)| {
                *acc += unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                Some((i, *acc))
            })
            .take_while(|&(_i, w)| w <= max_title_width)
            .last()
            .map(|(i, _w)| i + 1)
            .unwrap_or(current.len());
        let (part, rest) = current.split_at(take);
        title_lines.push(part.trim());
        current = rest;
    }

    let pad = "  "; // 2 spaces of padding
    let mut lines = vec![];
    for (i, part) in title_lines.iter().enumerate() {
        let is_last = i == title_lines.len() - 1;
        let line = if i == 0 {
            if is_last {
                // Only one line: put everything on it
                format!("{pad}Bash({part})...{pad}")
            } else {
                format!("{pad}Bash({part}")
            }
        } else if is_last {
            format!("{pad}  {part})...{pad}")
        } else {
            format!("{pad}  {part}")
        };
        lines.push(Line::from(vec![Span::styled(
            line,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]));
    }
    // Dynamically adjust dialog height
    let base_height = 9;
    let dialog_height = base_height + title_lines.len().saturating_sub(1);
    let area = ratatui::layout::Rect {
        x: 1,
        y: last_message_y,
        width: screen.width - 2,
        height: dialog_height as u16,
    };
    let desc = ""; // TODO: make this dynamic
    let options = ["Yes", "No, and tell Stapak what to do differently (esc)"];
    lines.push(Line::from(vec![Span::styled(
        format!("{pad}{}{pad}", desc),
        Style::default().fg(Color::Gray),
    )]));
    lines.push(Line::from(format!("{pad}{pad}")));
    lines.push(Line::from(format!("{pad}Do you want to proceed?{pad}")));
    lines.push(Line::from(format!("{pad}{pad}")));
    for (i, opt) in options.iter().enumerate() {
        let style = if state.dialog_selected == i {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(vec![Span::styled(
            format!("{pad}{}. {}{pad}", i + 1, opt),
            style,
        )]));
    }
    let dialog = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightYellow))
                .title("Bash command"),
        )
        .alignment(Alignment::Left);
    f.render_widget(dialog, area);
}


