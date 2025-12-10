//! Styled output utilities for inline terminal output matching Stakpak TUI style guide

/// ANSI color codes matching Stakpak TUI style
pub struct Colors;

impl Colors {
    /// Yellow - for titles and active items
    pub const YELLOW: &'static str = "\x1b[1;33m";
    /// Cyan - for borders, accents, selected items
    pub const CYAN: &'static str = "\x1b[1;36m";
    /// Green - for success, completed steps
    pub const GREEN: &'static str = "\x1b[1;32m";
    /// White - for default text
    pub const WHITE: &'static str = "\x1b[1;37m";
    /// Gray - for inactive/secondary text
    pub const GRAY: &'static str = "\x1b[90m";
    /// Magenta - for info messages
    pub const MAGENTA: &'static str = "\x1b[1;35m";
    /// Reset color
    pub const RESET: &'static str = "\x1b[0m";
}

/// Step status for rendering
#[derive(Clone, Copy, PartialEq)]
pub enum StepStatus {
    Active,
    Completed,
    Pending,
}

/// Render step indicators on a single line (horizontal progress)
pub fn render_steps(steps: &[(String, StepStatus)]) {
    let mut parts = Vec::new();

    for (i, (step, status)) in steps.iter().enumerate() {
        if i > 0 {
            parts.push(format!("{} {} ", Colors::GRAY, "|"));
        }

        let (indicator, color) = match status {
            StepStatus::Active => ("▲", Colors::YELLOW),
            StepStatus::Completed => ("◆", Colors::GREEN),
            StepStatus::Pending => ("", Colors::GRAY),
        };

        let text_color = match status {
            StepStatus::Active => Colors::GREEN,
            StepStatus::Completed => Colors::GREEN,
            StepStatus::Pending => Colors::WHITE,
        };

        if !indicator.is_empty() {
            parts.push(format!("{}{}{} ", color, indicator, Colors::RESET));
        }
        parts.push(format!("{}{}{}", text_color, step, Colors::RESET));
    }

    print!("{}\r\n", parts.join(""));
}

/// Render a title (yellow, bold)
pub fn render_title(title: &str) {
    print!("{}{}{}\r\n", Colors::YELLOW, title, Colors::RESET);
}

/// Render a subtitle (cyan)
pub fn render_subtitle(subtitle: &str) {
    print!("{}{}{}\r\n", Colors::CYAN, subtitle, Colors::RESET);
}

/// Render a success message (green)
pub fn render_success(message: &str) {
    print!("{}{}{}\r\n", Colors::GREEN, message, Colors::RESET);
}

/// Render an option in a menu
pub fn render_option(option: &str, is_selected: bool, _is_recommended: bool) {
    let indicator = if is_selected {
        format!("{}●{}", Colors::GREEN, Colors::RESET)
    } else {
        format!("{}○{}", Colors::GRAY, Colors::RESET)
    };

    let text_color = if is_selected {
        Colors::WHITE
    } else {
        Colors::GRAY
    };

    print!(
        "  {} {}{}{}\r\n",
        indicator,
        text_color,
        option,
        Colors::RESET
    );
}

/// Render a config preview (TOML syntax highlighting)
pub fn render_config_preview(config: &str) {
    print!("\r\n");
    print!(
        "{}Configuration preview:{}\r\n",
        Colors::CYAN,
        Colors::RESET
    );
    print!("{}\r\n", Colors::GRAY);
    for line in config.lines() {
        if line.trim().starts_with('[') {
            // Section headers
            print!("{}{}{}\r\n", Colors::YELLOW, line, Colors::RESET);
        } else if line.contains('=') {
            // Key-value pairs
            if let Some((key, value)) = line.split_once('=') {
                print!(
                    "{}{}{}={}{}{}\r\n",
                    Colors::CYAN,
                    key.trim(),
                    Colors::RESET,
                    Colors::GRAY,
                    value.trim(),
                    Colors::RESET
                );
            } else {
                print!("{}\r\n", line);
            }
        } else {
            print!("{}\r\n", line);
        }
    }
    print!("{}\r\n", Colors::RESET);
}

/// Render colorized keyboard shortcuts footer (like TUI style)
pub fn render_footer_shortcuts() {
    // Green for Enter
    let enter = format!("{}{}{}", Colors::GREEN, "Enter", Colors::GRAY);
    // Yellow for arrows (↑/↓)
    let arrows = format!("{}{}{}", Colors::YELLOW, " ↑/↓", Colors::GRAY);
    // Light blue/cyan for Type
    let type_key = format!("{}{}{}", Colors::CYAN, " Type", Colors::GRAY);
    // Red for Esc
    let esc = format!("{}\x1b[1;31m{}{}", Colors::RESET, " Esc", Colors::GRAY);
    // White for action words
    let gray = Colors::GRAY;

    print!(
        "{}{} confirm {}{} select {}{} search {}{} cancel\r\n",
        enter, gray, arrows, gray, type_key, gray, esc, gray
    );
}

/// Render an error message (red)
pub fn render_error(message: &str) {
    eprint!("\x1b[1;31m{}\x1b[0m\r\n", message);
}

/// Render a warning message (yellow)
pub fn render_warning(message: &str) {
    print!("{}⚠️  {}{}\r\n", Colors::YELLOW, message, Colors::RESET);
}

/// Render an info message (magenta)
pub fn render_info(message: &str) {
    print!("{}{}{}\r\n", Colors::MAGENTA, message, Colors::RESET);
}

/// Render profile name display (Profile in GRAY, name in RESET)
pub fn render_profile_name(profile_name: &str) {
    print!(
        "{}Profile{} {}{}\r\n",
        Colors::GRAY,
        Colors::RESET,
        profile_name,
        Colors::RESET
    );
}

/// Render default models for a provider in neutral colors (white/gray)
pub fn render_default_models(smart_model: &str, eco_model: &str, recovery_model: Option<&str>) {
    print!("{}Default models:{}\r\n", Colors::WHITE, Colors::RESET);
    print!(
        "  {}Smart: {}{}\r\n",
        Colors::GRAY,
        Colors::WHITE,
        smart_model
    );
    print!("  {}Eco: {}{}\r\n", Colors::GRAY, Colors::WHITE, eco_model);
    if let Some(recovery) = recovery_model {
        print!(
            "  {}Recovery: {}{}\r\n",
            Colors::GRAY,
            Colors::WHITE,
            recovery
        );
    }
    print!("\r\n");
}
