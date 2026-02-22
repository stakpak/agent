//! Styled output utilities for inline terminal output matching Stakpak TUI style guide

use crate::utils::cli_colors::CliColors;

/// ANSI color codes matching Stakpak TUI style
/// Now uses theme-aware colors that adapt to light/dark terminal backgrounds
pub struct Colors;

impl Colors {
    /// Yellow - for titles and active items
    pub fn yellow() -> &'static str {
        CliColors::yellow()
    }
    /// Cyan - for borders, accents, selected items
    pub fn cyan() -> &'static str {
        CliColors::cyan()
    }
    /// Green - for success, completed steps
    pub fn green() -> &'static str {
        CliColors::green()
    }
    /// White - for default text
    pub fn white() -> &'static str {
        CliColors::text()
    }
    /// Gray - for inactive/secondary text
    pub fn gray() -> &'static str {
        CliColors::gray()
    }
    /// Magenta - for info messages
    pub fn magenta() -> &'static str {
        CliColors::magenta()
    }
    /// Red - for errors
    pub fn red() -> &'static str {
        CliColors::red()
    }
    /// Reset color
    pub fn reset() -> &'static str {
        CliColors::reset()
    }
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
            parts.push(format!("{} {} ", Colors::gray(), "|"));
        }

        let (indicator, color) = match status {
            StepStatus::Active => ("◆", Colors::yellow()),
            StepStatus::Completed => ("◆", Colors::green()),
            StepStatus::Pending => ("", Colors::gray()),
        };

        let text_color = match status {
            StepStatus::Active => Colors::green(),
            StepStatus::Completed => Colors::green(),
            StepStatus::Pending => Colors::white(),
        };

        if !indicator.is_empty() {
            parts.push(format!("{}{}{} ", color, indicator, Colors::reset()));
        }
        parts.push(format!("{}{}{}", text_color, step, Colors::reset()));
    }

    print!("{}\r\n", parts.join(""));
}

/// Render a title (yellow, bold)
pub fn render_title(title: &str) {
    print!("{}{}{}\r\n", Colors::yellow(), title, Colors::reset());
}

/// Render a subtitle (cyan)
pub fn render_subtitle(subtitle: &str) {
    print!("{}{}{}\r\n", Colors::cyan(), subtitle, Colors::reset());
}

/// Render a success message (green)
pub fn render_success(message: &str) {
    print!("{}{}{}\r\n", Colors::green(), message, Colors::reset());
}

/// Render an option in a menu
pub fn render_option(option: &str, is_selected: bool, _is_recommended: bool) {
    let indicator = if is_selected {
        format!("{}●{}", Colors::green(), Colors::reset())
    } else {
        format!("{}○{}", Colors::gray(), Colors::reset())
    };

    let text_color = if is_selected {
        Colors::white()
    } else {
        Colors::gray()
    };

    print!(
        "  {} {}{}{}\r\n",
        indicator,
        text_color,
        option,
        Colors::reset()
    );
}

/// Render a config preview (TOML syntax highlighting)
pub fn render_config_preview(config: &str) {
    print!("\r\n");
    print!(
        "{}Configuration preview:{}\r\n",
        Colors::cyan(),
        Colors::reset()
    );
    print!("{}\r\n", Colors::gray());
    for line in config.lines() {
        if line.trim().starts_with('[') {
            // Section headers
            print!("{}{}{}\r\n", Colors::yellow(), line, Colors::reset());
        } else if line.contains('=') {
            // Key-value pairs
            if let Some((key, value)) = line.split_once('=') {
                print!(
                    "{}{}{}={}{}{}\r\n",
                    Colors::cyan(),
                    key.trim(),
                    Colors::reset(),
                    Colors::gray(),
                    value.trim(),
                    Colors::reset()
                );
            } else {
                print!("{}\r\n", line);
            }
        } else {
            print!("{}\r\n", line);
        }
    }
    print!("{}\r\n", Colors::reset());
}

/// Render colorized keyboard shortcuts footer (like TUI style)
pub fn render_footer_shortcuts() {
    // Green for Enter
    let enter = format!("{}{}{}", Colors::green(), "Enter", Colors::gray());
    // Yellow for arrows (↑/↓)
    let arrows = format!("{}{}{}", Colors::yellow(), " ↑/↓", Colors::gray());
    // Light blue/cyan for Type
    let type_key = format!("{}{}{}", Colors::cyan(), " Type", Colors::gray());
    // Red for Esc
    let esc = format!("{}{}{}", Colors::red(), " Esc", Colors::gray());
    // Gray for action words
    let gray = Colors::gray();

    print!(
        "{}{} confirm {}{} select {}{} search {}{} cancel\r\n",
        enter, gray, arrows, gray, type_key, gray, esc, gray
    );
}

/// Render an error message (red)
pub fn render_error(message: &str) {
    eprint!("{}{}{}\r\n", Colors::red(), message, Colors::reset());
}

/// Render a warning message (yellow)
pub fn render_warning(message: &str) {
    print!("{}⚠️  {}{}\r\n", Colors::yellow(), message, Colors::reset());
}

/// Render an info message (magenta)
pub fn render_info(message: &str) {
    print!("{}{}{}\r\n", Colors::magenta(), message, Colors::reset());
}

/// Render profile name display (Profile in GRAY, name in RESET)
pub fn render_profile_name(profile_name: &str) {
    print!(
        "{}Profile{} {}{}\r\n",
        Colors::gray(),
        Colors::reset(),
        profile_name,
        Colors::reset()
    );
}

/// Render default model for a provider in neutral colors (white/gray)
pub fn render_default_model(model: &str) {
    print!(
        "{}Default model: {}{}\r\n",
        Colors::GRAY,
        Colors::WHITE,
        model
    );
    print!("\r\n");
}

/// Render a styled telemetry disclaimer box for local providers
pub fn render_telemetry_disclaimer() {
    let border = Colors::cyan();
    let r = Colors::reset();

    print!("\r\n");
    print!(
        "{}╭──────────────────────────────────────────────────────────╮{}\r\n",
        border, r
    );
    print!(
        "{}│{}  {}Anonymous Telemetry{}                                     {}│{}\r\n",
        border,
        r,
        Colors::green(),
        r,
        border,
        r
    );
    print!(
        "{}│{}  we collect anonymous telemetry to improve stakpak.      {}│{}\r\n",
        border, r, border, r
    );
    print!(
        "{}│{}  no prompts, code, or personal data is collected.        {}│{}\r\n",
        border, r, border, r
    );
    print!(
        "{}│{}  opt-out: set {}collect_telemetry = false{} in config        {}│{}\r\n",
        border,
        r,
        Colors::yellow(),
        r,
        border,
        r
    );
    print!(
        "{}╰──────────────────────────────────────────────────────────╯{}\r\n",
        border, r
    );
    print!("\r\n");
}
