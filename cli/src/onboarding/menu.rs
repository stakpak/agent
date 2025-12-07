//! Interactive menu selection utilities

use crate::onboarding::navigation::NavResult;
use crate::onboarding::styled_output::{self, Colors, StepStatus};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::{self, Write, stdout};

/// Select from a list of options with search capability
/// Returns NavResult to support back navigation
pub fn select_option<T: Clone>(
    title: &str,
    options: &[(T, &str, bool)], // (value, display_name, is_recommended)
    current_step: usize,
    total_steps: usize,
    can_go_back: bool,
) -> NavResult<T> {
    let mut selected = 0;
    let mut search_input = String::new();

    if enable_raw_mode().is_err() {
        return NavResult::Cancel;
    }

    // Note: Caller should have already cleared the step content area
    // We just render this step's content fresh
    print!("\r\n");
    styled_output::render_title(title);
    print!("\r\n");

    // Render progress steps on one line
    let steps: Vec<_> = (0..total_steps)
        .map(|i| {
            let status = if i < current_step {
                StepStatus::Completed
            } else if i == current_step {
                StepStatus::Active
            } else {
                StepStatus::Pending
            };
            (format!("Step {}", i + 1), status)
        })
        .collect();
    styled_output::render_steps(&steps);
    print!("\r\n");
    print!("\r\n");

    // Initial render of the interactive area
    // We track how many lines we printed so we can move back up
    let mut previous_height = 0;

    loop {
        // If we previously rendered content, move cursor up and clear from there
        if previous_height > 0 {
            print!("\x1b[{}A", previous_height); // Move up N lines
            print!("\x1b[0J"); // Clear from cursor down
        }

        let mut current_height = 0;

        // Render search line
        print!(
            "  {}Search: {}{}\r\n",
            Colors::GRAY,
            search_input,
            Colors::RESET
        );
        current_height += 1;

        // Filter options based on search
        let filtered: Vec<_> = if search_input.is_empty() {
            options.iter().collect()
        } else {
            options
                .iter()
                .filter(|(_, name, _)| name.to_lowercase().contains(&search_input.to_lowercase()))
                .collect()
        };

        if !filtered.is_empty() {
            if selected >= filtered.len() {
                selected = filtered.len().saturating_sub(1);
            }

            // Render options
            for (idx, (_, name, is_recommended)) in filtered.iter().enumerate() {
                styled_output::render_option(name, idx == selected, *is_recommended);
                current_height += 1;
            }
        } else {
            print!("  {}No matches found{}\r\n", Colors::GRAY, Colors::RESET);
            current_height += 1;
        }

        print!("\r\n");
        current_height += 1;

        styled_output::render_footer_shortcuts();
        current_height += 1;

        let _ = stdout().flush();
        previous_height = current_height;

        if let Ok(Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            modifiers,
            ..
        })) = event::read()
        {
            match code {
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    // Clear interactive area before exiting
                    print!("\x1b[{}A", previous_height);
                    print!("\x1b[0J");
                    let _ = stdout().flush();
                    disable_raw_mode().ok();
                    std::process::exit(130);
                }
                KeyCode::Enter => {
                    if !filtered.is_empty() {
                        // Clear only the interactive area
                        print!("\x1b[{}A", previous_height);
                        print!("\x1b[0J");
                        let _ = stdout().flush();
                        disable_raw_mode().ok();
                        return NavResult::Forward(filtered[selected].0.clone());
                    }
                }
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                }
                KeyCode::Down => {
                    if selected < filtered.len().saturating_sub(1) {
                        selected += 1;
                    }
                }
                KeyCode::Char(c) => {
                    search_input.push(c);
                    selected = 0;
                }
                KeyCode::Backspace => {
                    search_input.pop();
                    selected = 0;
                }
                KeyCode::Esc => {
                    // Clear interactive area
                    print!("\x1b[{}A", previous_height);
                    print!("\x1b[0J");
                    let _ = stdout().flush();
                    disable_raw_mode().ok();
                    if can_go_back {
                        return NavResult::Back;
                    } else {
                        return NavResult::Cancel;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Prompt for text input
/// Returns NavResult to support back navigation
pub fn prompt_text(prompt: &str, required: bool) -> NavResult<Option<String>> {
    let mut input = String::new();
    let mut show_required = false;

    if enable_raw_mode().is_err() {
        return NavResult::Cancel;
    }

    loop {
        // Clear the line and re-render prompt
        print!("\r\x1b[K"); // Clear current line
        print!("{}▲ {}{}: ", Colors::YELLOW, Colors::CYAN, prompt);

        if show_required && required {
            print!("{}(Required){} ", Colors::YELLOW, Colors::RESET);
        }

        // Show current input
        print!("{}{}", Colors::CYAN, input);
        let _ = io::stdout().flush();

        if let Ok(Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            modifiers,
            ..
        })) = event::read()
        {
            match code {
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    disable_raw_mode().ok();
                    std::process::exit(130);
                }
                KeyCode::Enter => {
                    let trimmed = input.trim();
                    if required && trimmed.is_empty() {
                        show_required = true;
                        input.clear();
                        // Don't disable raw mode - continue the loop
                        continue;
                    }
                    print!("\r\n");
                    disable_raw_mode().ok();
                    return NavResult::Forward(if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    });
                }
                KeyCode::Esc => {
                    print!("\r\n");
                    disable_raw_mode().ok();
                    return NavResult::Back;
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) => {
                    input.push(c);
                }
                _ => {}
            }
        }
    }
}

/// Prompt for password/API key (hidden input)
/// Returns NavResult to support back navigation
pub fn prompt_password(prompt: &str, required: bool) -> NavResult<Option<String>> {
    let mut password = String::new();
    let mut show_required = false;

    if enable_raw_mode().is_err() {
        return NavResult::Cancel;
    }

    loop {
        // Clear the line and re-render prompt
        print!("\r\x1b[K"); // Clear current line
        print!("{}▲ {}{}: ", Colors::YELLOW, Colors::CYAN, prompt);

        if show_required && required {
            print!("{}(Required){} ", Colors::YELLOW, Colors::RESET);
        }

        // Show asterisks for password
        print!("{}", Colors::CYAN);
        for _ in 0..password.len() {
            print!("*");
        }
        let _ = io::stdout().flush();

        if let Ok(Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            modifiers,
            ..
        })) = event::read()
        {
            match code {
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    disable_raw_mode().ok();
                    std::process::exit(130);
                }
                KeyCode::Enter => {
                    let trimmed = password.trim();
                    if required && trimmed.is_empty() {
                        show_required = true;
                        password.clear();
                        // Don't disable raw mode - continue the loop
                        continue;
                    }
                    print!("\r\n");
                    disable_raw_mode().ok();
                    return NavResult::Forward(if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    });
                }
                KeyCode::Esc => {
                    print!("\r\n");
                    disable_raw_mode().ok();
                    return NavResult::Back;
                }
                KeyCode::Backspace => {
                    password.pop();
                }
                KeyCode::Char(c) => {
                    password.push(c);
                }
                _ => {}
            }
        }
    }
}

/// Prompt for yes/no confirmation
/// Returns NavResult to support back navigation
pub fn prompt_yes_no(prompt: &str, default: bool) -> NavResult<Option<bool>> {
    let default_text = if default { "Y/n" } else { "y/N" };
    let mut input = String::new();

    if enable_raw_mode().is_err() {
        return NavResult::Cancel;
    }

    loop {
        // Clear the line and re-render prompt
        print!("\r\x1b[K"); // Clear current line
        print!(
            "{}▲ {}{} ({}): ",
            Colors::YELLOW,
            Colors::CYAN,
            prompt,
            default_text
        );
        print!("{}{}", Colors::CYAN, input);
        let _ = io::stdout().flush();

        if let Ok(Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            modifiers,
            ..
        })) = event::read()
        {
            match code {
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    disable_raw_mode().ok();
                    std::process::exit(130);
                }
                KeyCode::Enter => {
                    print!("\r\n");
                    disable_raw_mode().ok();
                    let trimmed = input.trim().to_lowercase();
                    let result = match trimmed.as_str() {
                        "y" | "yes" => Some(true),
                        "n" | "no" => Some(false),
                        "" => None, // Use default
                        _ => None,  // Use default for invalid input
                    };
                    return NavResult::Forward(result);
                }
                KeyCode::Esc => {
                    print!("\r\n");
                    disable_raw_mode().ok();
                    return NavResult::Back;
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) => {
                    input.push(c);
                }
                _ => {}
            }
        }
    }
}
