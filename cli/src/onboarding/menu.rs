//! Interactive menu selection utilities

use crate::onboarding::navigation::NavResult;
use crate::onboarding::styled_output::{self, Colors, StepStatus};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::{self, Write, stdout};
use std::time::{Duration, Instant};

/// Internal helper: Select from a list of options with search capability
/// Returns NavResult to support back navigation
fn select_option_internal<T: Clone>(
    options: &[(T, &str, bool)], // (value, display_name, is_recommended)
    can_go_back: bool,
    header_height: usize,
) -> NavResult<T> {
    let mut selected = 0;
    let mut search_input = String::new();

    if enable_raw_mode().is_err() {
        return NavResult::Cancel;
    }

    let mut previous_height = 0;

    loop {
        // If we previously rendered content, move cursor up and clear from there
        if previous_height > 0 {
            print!("\x1b[{}A", previous_height);
            print!("\x1b[0J");
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
                    print!("\x1b[{}A", previous_height + header_height);
                    print!("\x1b[0J");
                    let _ = stdout().flush();
                    disable_raw_mode().ok();
                    std::process::exit(130);
                }
                KeyCode::Enter => {
                    if !filtered.is_empty() {
                        print!("\x1b[{}A", previous_height + header_height);
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
                    print!("\x1b[{}A", previous_height + header_height);
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

/// Select from a list of options with search capability
/// Returns NavResult to support back navigation
pub fn select_option<T: Clone>(
    title: &str,
    options: &[(T, &str, bool)], // (value, display_name, is_recommended)
    current_step: usize,
    total_steps: usize,
    can_go_back: bool,
) -> NavResult<T> {
    // Render header: title and step indicators
    styled_output::render_title(title);
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

    // Header height: title (1) + steps (1) + empty line (1) = 3
    select_option_internal(options, can_go_back, 3)
}

/// Select from a list of options without rendering title and step indicators
/// Used for sub-steps within a larger step (e.g., hybrid provider configuration)
/// Returns NavResult to support back navigation
pub fn select_option_no_header<T: Clone>(
    options: &[(T, &str, bool)], // (value, display_name, is_recommended)
    can_go_back: bool,
) -> NavResult<T> {
    // No header, so header_height is 0
    select_option_internal(options, can_go_back, 0)
}

/// Validate profile name (alphanumeric and underscores only, no spaces or special chars)
pub fn validate_profile_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Profile name cannot be empty".to_string());
    }
    if name == "all" {
        return Err("Cannot use 'all' as a profile name. It's reserved for defaults.".to_string());
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("Profile name can only contain letters, numbers, and underscores".to_string());
    }
    Ok(())
}

/// Prompt for profile name with validation
/// Returns NavResult to support back navigation
pub fn prompt_profile_name(config_path: Option<&str>) -> NavResult<Option<String>> {
    use crate::config::AppConfig;
    use std::path::PathBuf;

    let mut input = String::new();
    let mut error_message: Option<String> = None;

    if enable_raw_mode().is_err() {
        return NavResult::Cancel;
    }

    loop {
        // Clear the line and re-render prompt
        print!("\r\x1b[K"); // Clear current line
        print!("{}▲ {}Enter profile name: ", Colors::YELLOW, Colors::CYAN);

        // Show error if any
        if let Some(ref error) = error_message {
            print!("{}({}){} ", Colors::YELLOW, error, Colors::RESET);
        }

        // Show current input (RESET color to match question)
        print!("{}{}", Colors::RESET, input);
        let _ = io::stdout().flush();

        match event::read() {
            Ok(Event::Paste(pasted_text)) => {
                input.push_str(&pasted_text);
                error_message = None;
            }
            Ok(Event::Key(KeyEvent {
                code,
                kind: KeyEventKind::Press,
                modifiers,
                ..
            })) => {
                match code {
                    KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                        disable_raw_mode().ok();
                        std::process::exit(130);
                    }
                    KeyCode::Enter => {
                        let trimmed = input.trim();
                        if trimmed.is_empty() {
                            error_message = Some("Profile name cannot be empty".to_string());
                            input.clear();
                            continue;
                        }

                        // Validate format
                        if let Err(e) = validate_profile_name(trimmed) {
                            error_message = Some(e);
                            input.clear();
                            continue;
                        }

                        // Check if profile exists
                        let custom_path = config_path.map(PathBuf::from);
                        if let Ok(existing_profiles) =
                            AppConfig::list_available_profiles(custom_path.as_deref())
                            && existing_profiles.contains(&trimmed.to_string())
                        {
                            error_message = Some(format!("Profile '{}' already exists", trimmed));
                            input.clear();
                            continue;
                        }

                        // Clear the prompt line before returning
                        print!("\r\x1b[K"); // Clear current line
                        print!("\r\n");
                        disable_raw_mode().ok();
                        return NavResult::Forward(Some(trimmed.to_string()));
                    }
                    KeyCode::Esc => {
                        print!("\r\n");
                        disable_raw_mode().ok();
                        return NavResult::Back;
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        error_message = None;
                    }
                    KeyCode::Char(c) => {
                        input.push(c);
                        error_message = None;
                    }
                    _ => {}
                }
            }
            _ => {}
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

        match event::read() {
            Ok(Event::Paste(pasted_text)) => {
                // Handle paste event - add all characters at once
                input.push_str(&pasted_text);
            }
            Ok(Event::Key(KeyEvent {
                code,
                kind: KeyEventKind::Press,
                modifiers,
                ..
            })) => {
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
            _ => {}
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

    // Enable bracketed paste mode for better paste handling
    let _ = execute!(stdout(), EnableBracketedPaste);

    // Buffer for rapid character input (paste detection)
    let mut paste_buffer = String::new();
    let mut last_char_time = Instant::now();
    const PASTE_TIMEOUT_MS: u64 = 50; // Characters arriving within 50ms are considered a paste

    loop {
        // Get terminal width for wrapping calculation
        let terminal_width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(80)
            .max(40);

        // Calculate what we're about to print
        let prompt_text = format!("▲ {}: ", prompt);
        let required_text = if show_required && required {
            " (Required) "
        } else {
            ""
        };
        let prefix_len = prompt_text.len() + required_text.len();
        let display_len = password.len() + paste_buffer.len();

        let available_width = terminal_width.saturating_sub(prefix_len);

        // Show asterisks for password (including buffered paste)
        // Cap the number of stars to available width to prevent wrapping issues
        let stars_to_show = display_len.min(available_width.saturating_sub(1));

        // Move up to the start of the prompt if we've printed before
        // Since we force single line now, we only need to handle the single line case
        // But to be safe against edge cases where we might have wrapped previously (before this fix running)
        // or just standard redraw:
        // Actually, with single line enforcement, `\r` is sufficient to return to start of line,
        // and `\x1b[K` clears it.
        // The previous logic tried to handle multi-line. We simplify it.

        print!("\r\x1b[K"); // Clear current line

        // Render prompt
        print!("{}▲ {}{}: ", Colors::YELLOW, Colors::CYAN, prompt);
        if show_required && required {
            print!("{}(Required){} ", Colors::YELLOW, Colors::RESET);
        }

        // Show asterisks
        print!("{}", Colors::CYAN);
        for _ in 0..stars_to_show {
            print!("*");
        }
        // If truncated, maybe show a hint? No, simpler is better for now to fix the bug.
        print!("{}", Colors::RESET);

        // previous_lines is no longer needed for multi-line tracking,
        // but we keep the variable or logic compatible if we want.
        // Actually, I should remove the complex movement logic above this block too.

        let _ = io::stdout().flush();

        match event::read() {
            Ok(Event::Paste(pasted_text)) => {
                // Handle paste event - add all characters at once
                password.push_str(&pasted_text);
                paste_buffer.clear();
            }
            Ok(Event::Key(KeyEvent {
                code,
                kind: KeyEventKind::Press,
                modifiers,
                ..
            })) => {
                let now = Instant::now();
                let time_since_last = now.duration_since(last_char_time);

                match code {
                    KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                        let _ = execute!(stdout(), DisableBracketedPaste);
                        disable_raw_mode().ok();
                        std::process::exit(130);
                    }
                    KeyCode::Enter => {
                        // Flush any pending paste buffer
                        if !paste_buffer.is_empty() {
                            password.push_str(&paste_buffer);
                            paste_buffer.clear();
                        }
                        let trimmed = password.trim();
                        if required && trimmed.is_empty() {
                            show_required = true;
                            password.clear();
                            // Don't disable raw mode - continue the loop
                            continue;
                        }
                        print!("\r\n");
                        let _ = execute!(stdout(), DisableBracketedPaste);
                        disable_raw_mode().ok();
                        return NavResult::Forward(if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.to_string())
                        });
                    }
                    KeyCode::Esc => {
                        print!("\r\n");
                        let _ = execute!(stdout(), DisableBracketedPaste);
                        disable_raw_mode().ok();
                        return NavResult::Back;
                    }
                    KeyCode::Backspace => {
                        if !paste_buffer.is_empty() {
                            paste_buffer.pop();
                        } else {
                            password.pop();
                        }
                        last_char_time = now;
                    }
                    KeyCode::Char(c) => {
                        // If characters are arriving rapidly, collect them as a paste
                        if time_since_last.as_millis() < PASTE_TIMEOUT_MS as u128 {
                            paste_buffer.push(c);
                            last_char_time = now;

                            // Try to read all remaining rapid characters
                            while let Ok(true) = event::poll(Duration::from_millis(10)) {
                                match event::read() {
                                    Ok(Event::Key(KeyEvent {
                                        code: KeyCode::Char(ch),
                                        kind: KeyEventKind::Press,
                                        ..
                                    })) => {
                                        paste_buffer.push(ch);
                                        last_char_time = Instant::now();
                                    }
                                    _ => break,
                                }
                            }

                            // Flush buffer to password
                            password.push_str(&paste_buffer);
                            paste_buffer.clear();
                        } else {
                            // Flush any pending paste buffer
                            if !paste_buffer.is_empty() {
                                password.push_str(&paste_buffer);
                                paste_buffer.clear();
                            }
                            password.push(c);
                            last_char_time = now;
                        }
                    }
                    _ => {}
                }
            }
            _ => {
                // Flush any pending paste buffer on other events
                if !paste_buffer.is_empty() {
                    password.push_str(&paste_buffer);
                    paste_buffer.clear();
                }
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

/// Select profile interactively with scrolling (max 7 visible)
/// Returns Some(profile_name) or None if cancelled
/// Special return value "CREATE_NEW_PROFILE" indicates user wants to create new profile
pub async fn select_profile_interactive(config_path: Option<&std::path::Path>) -> Option<String> {
    use crate::config::AppConfig;

    // Get available profiles
    let profiles = AppConfig::list_available_profiles(config_path).unwrap_or_default();

    // Build options: "Create a new profile" first, then profiles
    let mut options: Vec<(String, &str, bool)> = vec![(
        "CREATE_NEW_PROFILE".to_string(),
        "Create a new profile",
        false,
    )];

    for profile in &profiles {
        options.push((profile.clone(), profile.as_str(), false));
    }

    if options.len() == 1 {
        // Only "Create a new profile" option, return it directly
        return Some("CREATE_NEW_PROFILE".to_string());
    }

    // Use select_option but we need to customize it for scrolling
    // For now, let's create a simplified version
    select_profile_with_scrolling("Stakpak profiles", &options, config_path).await
}

/// Select profile with scrolling support (max 7 visible)
async fn select_profile_with_scrolling(
    title: &str,
    options: &[(String, &str, bool)],
    _config_path: Option<&std::path::Path>,
) -> Option<String> {
    let _ = _config_path; // Suppress unused parameter warning
    use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
    use std::io::{Write, stdout};

    let mut selected = 0;
    let mut search_input = String::new();
    let mut scroll_offset = 0;
    const MAX_VISIBLE: usize = 7;

    if enable_raw_mode().is_err() {
        return None;
    }

    styled_output::render_title(title);
    print!("\r\n");

    let mut previous_height = 0;

    loop {
        if previous_height > 0 {
            print!("\x1b[{}A", previous_height);
            print!("\x1b[0J");
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

        // Filter options
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

            // Calculate scroll window
            let total = filtered.len();
            let visible_start = if total <= MAX_VISIBLE {
                0
            } else if selected < scroll_offset {
                selected.max(0)
            } else if selected >= scroll_offset + MAX_VISIBLE {
                selected.saturating_sub(MAX_VISIBLE - 1)
            } else {
                scroll_offset
            };

            let visible_end = (visible_start + MAX_VISIBLE).min(total);
            let visible_items = &filtered[visible_start..visible_end];

            // Show ▲ if there are items above
            if visible_start > 0 {
                let hidden_above = visible_start;
                print!(
                    "  {}▲ {} more above{}\r\n",
                    Colors::GRAY,
                    hidden_above,
                    Colors::RESET
                );
                current_height += 1;
            }

            // Render visible items with radio button circles
            for (idx, (_, name, _)) in visible_items.iter().enumerate() {
                let global_idx = visible_start + idx;
                let is_selected = global_idx == selected;

                if is_selected {
                    // Selected: green filled circle + white text
                    print!(
                        "  {}●{} {}{}\r\n",
                        Colors::GREEN,
                        Colors::RESET,
                        Colors::WHITE,
                        name
                    );
                    print!("{}", Colors::RESET);
                } else {
                    // Unselected: gray circle border + gray text
                    print!(
                        "  {}○{} {}{}\r\n",
                        Colors::GRAY,
                        Colors::RESET,
                        Colors::GRAY,
                        name
                    );
                    print!("{}", Colors::RESET);
                }
                current_height += 1;
            }

            // Show ▼ if there are items below
            if visible_end < total {
                let hidden_below = total - visible_end;
                print!(
                    "  {}▼ {} more below{}\r\n",
                    Colors::GRAY,
                    hidden_below,
                    Colors::RESET
                );
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
                    print!("\x1b[{}A", previous_height + 2);
                    print!("\x1b[0J");
                    let _ = stdout().flush();
                    disable_raw_mode().ok();
                    std::process::exit(130);
                }
                KeyCode::Enter => {
                    if !filtered.is_empty() {
                        print!("\x1b[{}A", previous_height + 2);
                        print!("\x1b[0J");
                        let _ = stdout().flush();
                        disable_raw_mode().ok();
                        return Some(filtered[selected].0.clone());
                    }
                }
                KeyCode::Up => {
                    if selected > 0 {
                        selected -= 1;
                        // Update scroll if needed
                        if selected < scroll_offset {
                            scroll_offset = selected;
                        }
                    }
                }
                KeyCode::Down => {
                    if selected < filtered.len().saturating_sub(1) {
                        selected += 1;
                        // Update scroll if needed
                        if selected >= scroll_offset + MAX_VISIBLE {
                            scroll_offset = selected.saturating_sub(MAX_VISIBLE - 1);
                        }
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
                    print!("\x1b[{}A", previous_height + 2);
                    print!("\x1b[0J");
                    let _ = stdout().flush();
                    disable_raw_mode().ok();
                    return None;
                }
                _ => {}
            }
        }
    }
}
