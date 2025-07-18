use crate::app::AppState;
use crate::services::confirmation_dialog::render_confirmation_dialog;
use crate::services::helper_block::render_loading_spinner;
use crate::services::helper_dropdown::{render_autocomplete_dropdown, render_helper_dropdown};
use crate::services::hint_helper::render_hint_or_shortcuts;
use crate::services::message::get_wrapped_message_lines;
use crate::services::message_pattern::{
    process_agent_mode_patterns, process_checkpoint_patterns, process_section_title_patterns,
    spans_to_string,
};
use crate::services::sessions_dialog::render_sessions_dialog;
use crate::services::shell_mode::SHELL_PROMPT_PREFIX;
use edtui::{EditorTheme};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

const DROPDOWN_MAX_HEIGHT: usize = 8;

pub fn view(f: &mut Frame, state: &mut AppState) {
    // Calculate the required height for the input area based on content
    let input_area_width = f.area().width.saturating_sub(4) as usize;
    let input_lines = calculate_input_lines(&state.input, input_area_width); // -4 for borders and padding
    let input_height = (input_lines + 2) as u16;
    let margin_height = 2;
    let dropdown_showing = state.show_helper_dropdown
        && ((state.autocomplete.is_active() && !state.autocomplete.filtered_files.is_empty())
            || (!state.autocomplete.is_active()
                && !state.filtered_helpers.is_empty()
                && state.input.starts_with('/')));
    let dropdown_height = if dropdown_showing {
        if state.autocomplete.is_active() {
            DROPDOWN_MAX_HEIGHT as u16
        } else {
            state.filtered_helpers.len() as u16
        }
    } else {
        0
    };
    let hint_height = if dropdown_showing { 0 } else { margin_height };

    let dialog_height = if state.show_sessions_dialog { 11 } else { 0 };
    let dialog_margin = if state.show_sessions_dialog { 1 } else { 0 };

    // Layout: [messages][dialog_margin][dialog][input][dropdown][hint]
    let mut constraints = vec![
        Constraint::Min(1), // messages
        Constraint::Length(dialog_margin),
        Constraint::Length(dialog_height),
    ];
    if !state.show_sessions_dialog {
        constraints.push(Constraint::Length(input_height));
        constraints.push(Constraint::Length(dropdown_height));
        constraints.push(Constraint::Length(hint_height));
    }
    let chunks = ratatui::layout::Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(f.area());

    let message_area = chunks[0];
    let mut input_area = Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };
    let mut dropdown_area = Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };
    let mut hint_area = Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };

    if !state.show_sessions_dialog {
        input_area = chunks[3];
        dropdown_area = chunks.get(4).copied().unwrap_or(input_area);
        hint_area = chunks.get(5).copied().unwrap_or(input_area);
    }

    let message_area_width = message_area.width as usize;
    let message_area_height = message_area.height as usize;

    render_messages(
        f,
        state,
        message_area,
        message_area_width,
        message_area_height,
    );

    if state.show_sessions_dialog {
        render_sessions_dialog(f, state);
        return;
    }
    if state.is_dialog_open {
        render_confirmation_dialog(f, state);
        return;
    }

    if state.show_file_picker {
        render_file_picker(f, state, f.area());
        return;
    }

    if state.show_editor {
        render_editor(f, state, f.area());
        return;
    }

    if !state.is_dialog_open {
        render_multiline_input(f, state, input_area);
        render_helper_dropdown(f, state, dropdown_area);
        render_autocomplete_dropdown(f, state, dropdown_area);
        if !dropdown_showing {
            render_hint_or_shortcuts(f, state, hint_area);
        }
    }
}

fn render_file_picker(f: &mut Frame, state: &mut AppState, area: Rect) {
    use ratatui::style::Modifier;
    use ratatui::widgets::{Block, Borders, List, ListItem};

    // Clear the entire area first
    f.render_widget(ratatui::widgets::Clear, area);

    // Create a block with background
    let block = Block::default()
        .title(" File Picker - Press Enter to open, Esc to cancel ")
        .title_style(
            Style::default()
                .fg(ratatui::style::Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ratatui::style::Color::Green))
        .style(Style::default().bg(ratatui::style::Color::Rgb(30, 30, 46)));

    let inner_area = block.inner(area);

    // Render the background block first
    f.render_widget(block, area);

    // Create list items
    let items: Vec<ListItem> = state
        .file_picker_files
        .iter()
        .map(|file| ListItem::new(file.clone()))
        .collect();

    let list = List::new(items)
        .block(Block::default())
        .highlight_style(
            Style::default()
                .bg(ratatui::style::Color::Blue)
                .fg(ratatui::style::Color::White),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(
        list,
        inner_area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(state.file_picker_selected)),
    );
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
            let word = words.next().unwrap_or_default();
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
    f.render_widget(ratatui::widgets::Clear, area);
    let mut all_lines: Vec<(Line, Style)> = get_wrapped_message_lines(&state.messages, width);
    if state.loading {
        let loading_line = render_loading_spinner(state);
        all_lines.push((loading_line, Style::default()));
    }

    // Pre-process ALL lines completely and consistently
    let mut processed_lines: Vec<Line> = Vec::new();

    for (i, (line, _style)) in all_lines.iter().enumerate() {
        let line_text = spans_to_string(line);
        let mut should_add_spacing = false;

        // Check if we need spacing before this line (but not for the first line)
        if i > 0 {
            if line_text.contains("<checkpoint_id>") || line_text.contains("<agent_mode>") {
                should_add_spacing = true;
            } else {
                let section_tags = [
                    "planning",
                    "reasoning",
                    "notes",
                    "progress",
                    "local_context",
                    "todo",
                    "application_analysis",
                    "scratchpad",
                    "report",
                    "current_context",
                    "rulebooks",
                    "current_analysis",
                ];

                for tag in &section_tags {
                    if line_text.contains(&format!("<{}>", tag)) {
                        should_add_spacing = true;
                        break;
                    }
                }
            }
        }

        // Add spacing before the line if needed
        if should_add_spacing {
            processed_lines.push(Line::from(""));
        }

        // Process the line and add all resulting lines
        if line_text.contains("<checkpoint_id>") {
            let processed = process_checkpoint_patterns(
                &[(line.clone(), Style::default())],
                f.area().width as usize,
            );
            for (processed_line, _) in processed {
                processed_lines.push(processed_line);
            }
        } else if line_text.contains("<agent_mode>") {
            let processed = process_agent_mode_patterns(&[(line.clone(), Style::default())]);
            for (processed_line, _) in processed {
                processed_lines.push(processed_line);
            }
        } else {
            let section_tags = [
                "planning",
                "reasoning",
                "notes",
                "progress",
                "local_context",
                "todo",
                "application_analysis",
                "scratchpad",
                "report",
                "current_context",
                "rulebooks",
                "current_analysis",
            ];
            let mut found = false;

            for tag in &section_tags {
                let closing_tag = format!("</{}>", tag);
                if line_text.trim() == closing_tag {
                    processed_lines.push(Line::from(""));
                    found = true;
                    break;
                }
                if line_text.contains(&format!("<{}>", tag)) {
                    let processed =
                        process_section_title_patterns(&[(line.clone(), Style::default())], tag);
                    for (processed_line, _) in processed {
                        processed_lines.push(processed_line);
                    }
                    found = true;
                    break;
                }
            }

            if !found {
                if line_text.trim() == "SPACING_MARKER" {
                    processed_lines.push(Line::from(""));
                } else {
                    processed_lines.push(line.clone());
                }
            }
        }
    }

    let total_lines = processed_lines.len();

    // Handle edge case where we have no content
    if total_lines == 0 {
        let message_widget =
            Paragraph::new(Vec::<Line>::new()).wrap(ratatui::widgets::Wrap { trim: false });
        f.render_widget(message_widget, area);
        return;
    }

    let max_scroll = total_lines.saturating_sub(height);

    // Prevent snapping by adjusting scroll relative to the processed content
    let original_total = all_lines.len();
    let processed_total = total_lines;

    let scroll = if state.stay_at_bottom {
        max_scroll
    } else {
        // Scale the scroll position to account for processed content size difference
        let adjusted_scroll = if original_total > 0 {
            (state.scroll * processed_total) / original_total
        } else {
            state.scroll
        };
        adjusted_scroll.min(max_scroll)
    };

    // Create visible lines with simple, consistent indexing
    let mut visible_lines = Vec::new();

    for i in 0..height {
        let line_index = scroll + i;
        if line_index < processed_lines.len() {
            visible_lines.push(processed_lines[line_index].clone());
        } else {
            visible_lines.push(Line::from(""));
        }
    }

    let message_widget = Paragraph::new(visible_lines).wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(message_widget, area);
}

fn render_multiline_input(f: &mut Frame, state: &AppState, area: Rect) {
    // Mask input if in shell mode and waiting for shell input (password)
    let input = if state.show_shell_mode && state.waiting_for_shell_input {
        "*".repeat(state.input.chars().count())
    } else {
        state.input.clone()
    };
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
        // Handle empty segments (blank lines)
        if segment.is_empty() {
            let mut empty_line = Vec::new();

            // Add prompt only for first line
            if segment_idx == 0 {
                if state.show_shell_mode {
                    empty_line.push(Span::styled(
                        " ".to_string() + SHELL_PROMPT_PREFIX,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    empty_line.push(Span::raw("> "));
                }
            }

            // Check if cursor is at the end of this empty segment
            if current_pos == cursor_pos && !cursor_rendered {
                empty_line.push(Span::styled(
                    "█",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
                cursor_rendered = true;
            }

            lines.push(Line::from(empty_line));
            current_pos += 1; // +1 for the newline
            continue;
        }

        // For non-empty segments, wrap text properly but preserve spaces
        let prompt_text = if state.show_shell_mode {
            format!(" {}", SHELL_PROMPT_PREFIX)
        } else if segment_idx == 0 {
            "> ".to_string()
        } else {
            String::new()
        };

        let prompt_width = prompt_text.chars().count();
        let content_width = available_width.saturating_sub(prompt_width);

        // Process the segment preserving all whitespace
        let mut current_line_content = String::new();
        let mut current_line_width = 0;
        let mut line_start_pos = current_pos;

        // Split by words but preserve spaces
        let mut word_positions = Vec::new();
        let mut word_start = 0;
        let mut in_word = false;

        for (i, ch) in segment.char_indices() {
            if ch.is_whitespace() {
                if in_word {
                    // End of word
                    word_positions.push((word_start, i, false)); // false = word
                    in_word = false;
                }
                if !in_word {
                    word_start = i;
                }
            } else if !in_word {
                if word_start < i {
                    // There were spaces before this word
                    word_positions.push((word_start, i, true)); // true = whitespace
                }
                word_start = i;
                in_word = true;
            }
        }

        // Handle final word or whitespace
        if in_word && word_start < segment.len() {
            word_positions.push((word_start, segment.len(), false));
        } else if !in_word && word_start < segment.len() {
            word_positions.push((word_start, segment.len(), true));
        }

        // If no word positions found, treat entire segment as one unit
        if word_positions.is_empty() {
            word_positions.push((0, segment.len(), false));
        }

        for (start, end, _is_whitespace) in word_positions {
            let text = &segment[start..end];
            let text_width = text.chars().count();

            // Check if this text fits on current line
            if current_line_width + text_width > content_width && !current_line_content.is_empty() {
                // Current line is full, render it
                let mut line_spans = Vec::new();

                // Add prompt for first line of this segment
                if segment_idx == 0 && lines.is_empty() {
                    if state.show_shell_mode {
                        line_spans.push(Span::styled(
                            prompt_text.clone(),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ));
                    } else {
                        line_spans.push(Span::raw(prompt_text.clone()));
                    }
                }

                // Handle cursor in current line content
                let line_end_pos = line_start_pos + current_line_content.len();
                if cursor_pos >= line_start_pos && cursor_pos <= line_end_pos && !cursor_rendered {
                    let cursor_offset = cursor_pos - line_start_pos;

                    if cursor_offset < current_line_content.len() {
                        let before_cursor = &current_line_content[..cursor_offset];
                        let at_cursor_char = current_line_content
                            .chars()
                            .nth(cursor_offset)
                            .unwrap_or(' ');
                        let at_cursor = if at_cursor_char == ' ' {
                            "█"
                        } else {
                            &at_cursor_char.to_string()
                        };
                        let after_cursor_start = cursor_offset + at_cursor_char.len_utf8();
                        let after_cursor = if after_cursor_start < current_line_content.len() {
                            &current_line_content[after_cursor_start..]
                        } else {
                            ""
                        };

                        if !before_cursor.is_empty() {
                            line_spans.push(Span::raw(before_cursor.to_string()));
                        }
                        line_spans.push(Span::styled(
                            at_cursor.to_string(),
                            Style::default()
                                .bg(Color::Cyan)
                                .fg(Color::Black)
                                .add_modifier(Modifier::BOLD),
                        ));
                        if !after_cursor.is_empty() {
                            line_spans.push(Span::raw(after_cursor.to_string()));
                        }
                        cursor_rendered = true;
                    } else {
                        // Cursor at end of line
                        line_spans.push(Span::raw(current_line_content.clone()));
                        line_spans.push(Span::styled(
                            "█",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ));
                        cursor_rendered = true;
                    }
                } else {
                    line_spans.push(Span::raw(current_line_content.clone()));
                }

                lines.push(Line::from(line_spans));

                // Start new line
                current_line_content.clear();
                current_line_width = 0;
                line_start_pos = current_pos + start;
            }

            // Add text to current line
            current_line_content.push_str(text);
            current_line_width += text_width;
        }

        // Render remaining content
        if !current_line_content.is_empty() {
            let mut line_spans = Vec::new();

            // Add prompt for first line of this segment
            if segment_idx == 0 && lines.is_empty() {
                if state.show_shell_mode {
                    line_spans.push(Span::styled(
                        prompt_text.clone(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    line_spans.push(Span::raw(prompt_text.clone()));
                }
            }

            // Handle cursor in the remaining content
            let line_end_pos = line_start_pos + current_line_content.len();

            if cursor_pos >= line_start_pos && cursor_pos <= line_end_pos && !cursor_rendered {
                let cursor_offset = cursor_pos - line_start_pos;

                if cursor_offset < current_line_content.len() {
                    let before_cursor = &current_line_content[..cursor_offset];
                    let at_cursor_char = current_line_content
                        .chars()
                        .nth(cursor_offset)
                        .unwrap_or(' ');
                    let at_cursor = if at_cursor_char == ' ' {
                        "█"
                    } else {
                        &at_cursor_char.to_string()
                    };
                    let after_cursor_start = cursor_offset + at_cursor_char.len_utf8();
                    let after_cursor = if after_cursor_start < current_line_content.len() {
                        &current_line_content[after_cursor_start..]
                    } else {
                        ""
                    };

                    if !before_cursor.is_empty() {
                        line_spans.push(Span::raw(before_cursor.to_string()));
                    }
                    line_spans.push(Span::styled(
                        at_cursor.to_string(),
                        Style::default()
                            .bg(Color::Cyan)
                            .fg(Color::Black)
                            .add_modifier(Modifier::BOLD),
                    ));
                    if !after_cursor.is_empty() {
                        line_spans.push(Span::raw(after_cursor.to_string()));
                    }
                    cursor_rendered = true;
                } else {
                    // Cursor at end of content
                    line_spans.push(Span::raw(current_line_content.clone()));
                    line_spans.push(Span::styled(
                        "█",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ));
                    cursor_rendered = true;
                }
            } else {
                line_spans.push(Span::raw(current_line_content.clone()));
            }

            lines.push(Line::from(line_spans));
        }

        // Move to next segment
        current_pos += segment.len() + 1; // +1 for newline
    }

    // Handle cursor at the very end of input
    if cursor_pos == input.len() && !cursor_rendered {
        if let Some(last_line) = lines.last_mut() {
            last_line.spans.push(Span::styled(
                "█",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            // Create a new line with prompt and cursor
            let mut cursor_line = Vec::new();
            if state.show_shell_mode {
                cursor_line.push(Span::styled(
                    " ".to_string() + SHELL_PROMPT_PREFIX,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                cursor_line.push(Span::raw("> "));
            }
            cursor_line.push(Span::styled(
                "█",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::from(cursor_line));
        }
    }

    // Ensure we have at least one line
    if lines.is_empty() {
        let mut default_line = Vec::new();
        if state.show_shell_mode {
            default_line.push(Span::styled(
                " ".to_string() + SHELL_PROMPT_PREFIX,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            default_line.push(Span::raw("> "));
        }
        default_line.push(Span::styled(
            "█",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::from(default_line));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if state.show_shell_mode {
            Style::default().fg(Color::Rgb(160, 92, 158))
        } else {
            Style::default().fg(Color::DarkGray)
        });

    // Render the input widget
    let input_widget = Paragraph::new(lines)
        .style(Style::default())
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(input_widget, area);
}

fn render_editor(f: &mut Frame, state: &mut AppState, area: Rect) {
    use edtui::{EditorView, SyntaxHighlighter};
    use ratatui::style::Modifier;
    use ratatui::widgets::{Block, Borders};

    // Initialize editor state if not already done
    let mut editor_state_initialized = false;
    crate::EDITOR_STATE.with(|editor_state| {
        if editor_state.borrow().is_none() {
            *editor_state.borrow_mut() = Some(edtui::EditorState::new(edtui::Lines::from(
                &state.editor_content,
            )));
            editor_state_initialized = true;
        }
    });

    if editor_state_initialized {
        crate::EDITOR_EVENT_HANDLER.with(|editor_event_handler| {
            if editor_event_handler.borrow().is_none() {
                *editor_event_handler.borrow_mut() = Some(edtui::EditorEventHandler::default());
            }
        });
    }

    // Clear the entire area first to ensure no content shows through
    f.render_widget(ratatui::widgets::Clear, area);

    // Fill the entire area with the desired background color
    let background_rect = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height,
    };
    let background_widget = ratatui::widgets::Block::default()
        .style(Style::default().bg(ratatui::style::Color::Rgb(30, 30, 46)));
    f.render_widget(background_widget, background_rect);

    // Create a block with background to cover the content behind
    let title = if let Some(file_path) = &state.editor_file_path {
        format!(" Editor - {}", file_path)
    } else {
        " Editor ".to_string()
    };

    // Get the current editor mode from the editor state
    let editor_mode = crate::EDITOR_STATE.with(|editor_state| {
        if let Some(state_ref) = editor_state.borrow().as_ref() {
            format!("{:?}", state_ref.mode)
        } else {
            "Normal".to_string()
        }
    });
    
    // Create status line with mode on left and commands on right
    let status_line = format!(" {}", editor_mode);
    let commands = "Ctrl+S: Save | Ctrl+E: Exit | Ctrl+P: File Picker ";
    let status_line_len = status_line.len();

    let block = Block::default()
        .title(title)
        .title_style(
            Style::default()
                .fg(ratatui::style::Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ratatui::style::Color::Blue))
        .style(Style::default().bg(ratatui::style::Color::Rgb(30, 30, 46)));

    // Calculate the inner area first (reserve space for status line)
    let mut inner_area = block.inner(area);
    inner_area.height = inner_area.height.saturating_sub(1); // Reserve 1 line for status

    // Render the background block first
    f.render_widget(block, area);

    // Create the editor view widget with syntax highlighting
    crate::EDITOR_STATE.with(|editor_state| {
        if let Some(state_ref) = editor_state.borrow_mut().as_mut() {
            // Determine file extension for syntax highlighting
            let file_extension = if let Some(file_path) = &state.editor_file_path {
                file_path.split('.').next_back().unwrap_or("txt")
            } else {
                "txt"
            };

            // Create syntax highlighter based on file extension
            let syntax_highlighter = match file_extension {
                "rs" => Some(SyntaxHighlighter::new("dracula", "rs")),
                "toml" => Some(SyntaxHighlighter::new("dracula", "toml")),
                "md" => Some(SyntaxHighlighter::new("dracula", "markdown")),
                "json" => Some(SyntaxHighlighter::new("dracula", "json")),
                "yaml" | "yml" => Some(SyntaxHighlighter::new("dracula", "yaml")),
                "py" => Some(SyntaxHighlighter::new("dracula", "python")),
                "js" | "ts" => Some(SyntaxHighlighter::new("dracula", "javascript")),
                "html" | "htm" => Some(SyntaxHighlighter::new("dracula", "html")),
                "css" => Some(SyntaxHighlighter::new("dracula", "css")),
                "sh" | "bash" => Some(SyntaxHighlighter::new("dracula", "bash")),
                _ => None,
            };

            // Render a background paragraph to fill the area with the desired color
            let background_paragraph = Paragraph::new("")
                .style(Style::default().bg(ratatui::style::Color::Rgb(30, 30, 46)));
            f.render_widget(background_paragraph, inner_area);

            // Create a custom theme with the desired background color
            let custom_theme = EditorTheme {
                base: Style::default().bg(ratatui::style::Color::Rgb(30, 30, 46)),
                cursor_style: Style::default().bg(ratatui::style::Color::White),
                selection_style: Style::default()
                    .bg(ratatui::style::Color::Blue)
                    .fg(ratatui::style::Color::White),
                block: None,
                status_line: None,
            };

            let mut editor_view = EditorView::new(state_ref).theme(custom_theme).wrap(true);

            // Add syntax highlighting if available
            if let Some(highlighter) = syntax_highlighter {
                editor_view = editor_view.syntax_highlighter(Some(highlighter));
            }

            f.render_widget(editor_view, inner_area);
        }
    });

    // Render status line at the bottom
    let status_area = Rect {
        x: area.x + 1,
        y: area.y + area.height - 2,
        width: area.width - 2,
        height: 1,
    };

    let status_text = ratatui::text::Line::from(vec![
        ratatui::text::Span::styled(
            status_line,
            Style::default()
                .fg(ratatui::style::Color::Cyan)
                .bg(ratatui::style::Color::Rgb(30, 30, 46)),
        ),
        ratatui::text::Span::styled(
            format!("{:>width$}", commands, width = (area.width - status_line_len as u16 - 2) as usize),
            Style::default()
                .fg(ratatui::style::Color::Rgb(160, 92, 158))
                .bg(ratatui::style::Color::Rgb(30, 30, 46))
               
        ),
    ]);

    f.render_widget(
        ratatui::widgets::Paragraph::new(status_text)
            .style(Style::default().bg(ratatui::style::Color::Rgb(30, 30, 46))),
        status_area,
    );
}
