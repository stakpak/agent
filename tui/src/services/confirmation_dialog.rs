use crate::app::AppState;
use crate::services::auto_approve::AutoApprovePolicy;
use crate::services::message::get_wrapped_message_lines;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

pub fn render_confirmation_dialog(f: &mut Frame, state: &AppState) {
    let screen = f.area();
    let message_lines = get_wrapped_message_lines(&state.messages, screen.width as usize);
    let mut last_message_y = message_lines.len() as u16 + 1; // +1 for a gap

    // Calculate dynamic dialog height based on content
    let dialog_height = calculate_dialog_height(state);

    // Clamp so dialog fits on screen
    if last_message_y + dialog_height > screen.height {
        last_message_y = screen.height.saturating_sub(dialog_height + 3);
    }

    let area = Rect {
        x: 1,
        y: last_message_y,
        width: screen.width - 2,
        height: dialog_height,
    };

    if let Some(dialog_command) = state.dialog_command.as_ref() {
        render_enhanced_confirmation_dialog(f, state, dialog_command, area);
    } else {
        render_simple_confirmation_dialog(f, area);
    }
}

pub fn calculate_dialog_height(state: &AppState) -> u16 {
    let mut height = 3; // Base height for borders and basic content

    if let Some(dialog_command) = &state.dialog_command {
        // Add height for risk level and policy info
        height += 1;

        // Add height for command description
        height += 1;

        // Add height for options list (minimum 3 options)
        height += 3;

        // Add extra height for multi-line titles
        let title = get_command_title(dialog_command);
        let title_lines = (title.len() as f32 / 60.0).ceil() as u16; // Approximate line wrapping
        if title_lines > 1 {
            height += title_lines - 1;
        }
    }

    height
}

fn get_command_title(tool_call: &stakpak_shared::models::integrations::openai::ToolCall) -> String {
    let tool_name = &tool_call.function.name;
    let command = extract_command_preview(tool_call);

    if command.is_empty() {
        tool_name.to_string()
    } else {
        format!("{}: {}", tool_name, command)
    }
}

fn extract_command_preview(
    tool_call: &stakpak_shared::models::integrations::openai::ToolCall,
) -> String {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments) {
        if let Some(cmd) = json.get("command").and_then(|v| v.as_str()) {
            // Truncate long commands for display
            if cmd.len() > 50 {
                return format!("{}...", &cmd[..47]);
            }
            return cmd.to_string();
        }
    }

    // Fallback to old parsing method
    tool_call
        .function
        .arguments
        .split("\"command\": \"")
        .nth(1)
        .and_then(|s| s.split('\"').next())
        .unwrap_or("")
        .to_string()
}

fn render_enhanced_confirmation_dialog(
    f: &mut Frame,
    state: &AppState,
    tool_call: &stakpak_shared::models::integrations::openai::ToolCall,
    area: Rect,
) {
    // Create a proper dialog box with borders
    let border_color = if state.dialog_focused {
        Color::LightYellow
    } else {
        Color::DarkGray
    };

    let dialog_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title("Confirmation");

    let inner_area = dialog_block.inner(area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Risk/Policy info
            Constraint::Length(1), // Command description
            Constraint::Min(3),    // Options list
        ])
        .split(inner_area);

    // Risk level and policy info
    let policy = state.auto_approve_manager.get_policy_for_tool(tool_call);

    let policy_text = match policy {
        AutoApprovePolicy::Auto => "Auto-approve",
        AutoApprovePolicy::Prompt => "Requires confirmation",
        AutoApprovePolicy::Smart => "Smart approval",
        AutoApprovePolicy::Never => "Always blocked",
    };

    let risk_info = Line::from(vec![
        Span::styled("Policy: ", Style::default().fg(Color::White)),
        Span::styled(policy_text, Style::default().fg(Color::Cyan)),
    ]);

    let risk_widget = Paragraph::new(risk_info).alignment(Alignment::Center);
    f.render_widget(risk_widget, chunks[0]);

    // Command description
    let command = extract_command_preview(tool_call);
    let description = if command.is_empty() {
        "Execute this tool call"
    } else {
        "Do you want to proceed?"
    };

    let desc_widget = Paragraph::new(description).alignment(Alignment::Center);
    f.render_widget(desc_widget, chunks[1]);

    // Options list
    let options = create_options_list(state, tool_call);
    let list_widget = List::new(options)
        .block(Block::default().borders(Borders::NONE))
        .style(Style::default().fg(Color::White));
    f.render_widget(list_widget, chunks[2]);
    // Render the dialog block
    f.render_widget(dialog_block, area);
}

fn create_options_list(
    state: &AppState,
    tool_call: &stakpak_shared::models::integrations::openai::ToolCall,
) -> Vec<ListItem<'static>> {
    let mut options = Vec::new();

    // Option 1: Yes
    let option1 = if state.dialog_selected == 0 {
        ListItem::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::LightYellow)),
            Span::styled("Yes", Style::default().fg(Color::LightYellow)),
        ]))
    } else {
        ListItem::new(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("Yes", Style::default().fg(Color::DarkGray)),
        ]))
    };
    options.push(option1);

    // Option 2: Yes, and don't ask again for this tool
    let tool_name = &tool_call.function.name;
    let option2 = if state.dialog_selected == 1 {
        ListItem::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::LightYellow)),
            Span::styled(
                format!("Yes, and don't ask again for {} commands", tool_name),
                Style::default().fg(Color::LightYellow),
            ),
        ]))
    } else {
        ListItem::new(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("Yes, and don't ask again for {} commands", tool_name),
                Style::default().fg(Color::DarkGray),
            ),
        ]))
    };
    options.push(option2);

    // Option 3: No, and tell Stakpak what to do differently
    let option3 = if state.dialog_selected == 2 {
        ListItem::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::LightYellow)),
            Span::styled(
                "No, and tell Stakpak what to do differently (esc)",
                Style::default().fg(Color::LightYellow),
            ),
        ]))
    } else {
        ListItem::new(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                "No, and tell Stakpak what to do differently (esc)",
                Style::default().fg(Color::DarkGray),
            ),
        ]))
    };
    options.push(option3);

    options
}

fn render_simple_confirmation_dialog(f: &mut Frame, area: Rect) {
    let message = "Press Enter to continue or Esc to cancel and reprompt";
    let dialog = Paragraph::new(message)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightYellow))
                .title("Confirmation"),
        )
        .alignment(Alignment::Center);
    f.render_widget(dialog, area);
}
