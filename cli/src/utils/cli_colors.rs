//! Theme-aware CLI colors for terminal output
//!
//! Provides ANSI escape codes that adapt to light/dark terminal backgrounds.
//! Delegates theme detection to stakpak_shared::terminal_theme for consistency
//! with the TUI crate.

/// Check if terminal is in light mode (delegates to shared detection)
pub fn is_light_mode() -> bool {
    stakpak_shared::terminal_theme::is_light_mode()
}

/// Theme-aware ANSI color codes for CLI output
pub struct CliColors;

impl CliColors {
    // ==========================================================================
    // Primary colors - adapt based on terminal background
    // ==========================================================================

    /// Yellow - for titles, warnings, active items
    /// Dark mode: bright yellow, Light mode: dark gold/orange
    pub fn yellow() -> &'static str {
        if is_light_mode() {
            "\x1b[38;5;136m" // Dark gold (ANSI 256 color 136)
        } else {
            "\x1b[1;33m" // Bright yellow
        }
    }

    /// Cyan - for borders, accents, selected items
    /// Dark mode: bright cyan, Light mode: dark cyan/teal
    pub fn cyan() -> &'static str {
        if is_light_mode() {
            "\x1b[38;5;30m" // Dark cyan (ANSI 256 color 30)
        } else {
            "\x1b[1;36m" // Bright cyan
        }
    }

    /// Green - for success, completed steps
    /// Dark mode: bright green, Light mode: dark green
    pub fn green() -> &'static str {
        if is_light_mode() {
            "\x1b[38;5;28m" // Dark green (ANSI 256 color 28)
        } else {
            "\x1b[1;32m" // Bright green
        }
    }

    /// Red - for errors
    /// Dark mode: bright red, Light mode: dark red
    pub fn red() -> &'static str {
        if is_light_mode() {
            "\x1b[38;5;160m" // Dark red (ANSI 256 color 160)
        } else {
            "\x1b[1;31m" // Bright red
        }
    }

    /// Magenta - for info messages, highlights
    /// Dark mode: bright magenta, Light mode: dark magenta
    pub fn magenta() -> &'static str {
        if is_light_mode() {
            "\x1b[38;5;127m" // Dark magenta (ANSI 256 color 127)
        } else {
            "\x1b[1;35m" // Bright magenta
        }
    }

    /// Blue - for links, info
    /// Dark mode: bright blue, Light mode: dark blue
    pub fn blue() -> &'static str {
        if is_light_mode() {
            "\x1b[38;5;25m" // Dark blue (ANSI 256 color 25)
        } else {
            "\x1b[1;34m" // Bright blue
        }
    }

    /// White/primary text - main content
    /// Dark mode: bright white, Light mode: dark gray
    pub fn text() -> &'static str {
        if is_light_mode() {
            "\x1b[38;5;235m" // Very dark gray (ANSI 256 color 235)
        } else {
            "\x1b[1;37m" // Bright white
        }
    }

    /// Gray - for secondary/inactive text
    /// Dark mode: dark gray, Light mode: medium gray
    pub fn gray() -> &'static str {
        if is_light_mode() {
            "\x1b[38;5;243m" // Medium gray (ANSI 256 color 243)
        } else {
            "\x1b[90m" // Dark gray
        }
    }

    /// Orange - for special highlights
    /// Dark mode: bright orange, Light mode: dark orange
    pub fn orange() -> &'static str {
        if is_light_mode() {
            "\x1b[38;5;166m" // Dark orange (ANSI 256 color 166)
        } else {
            "\x1b[38;5;214m" // Bright orange (ANSI 256 color 214)
        }
    }

    /// Reset - return to default terminal colors
    pub fn reset() -> &'static str {
        "\x1b[0m"
    }

    /// Bold modifier (for future use)
    #[allow(dead_code)]
    pub fn bold() -> &'static str {
        "\x1b[1m"
    }
}

/// Crossterm Color equivalents for use with crossterm::style
pub mod crossterm_colors {
    use crossterm::style::Color;

    /// Check if terminal is in light mode
    pub fn is_light_mode() -> bool {
        super::is_light_mode()
    }

    /// Theme-aware cyan color
    pub fn cyan() -> Color {
        if is_light_mode() {
            Color::AnsiValue(30) // Dark cyan
        } else {
            Color::Cyan
        }
    }

    /// Theme-aware green color
    pub fn green() -> Color {
        if is_light_mode() {
            Color::AnsiValue(28) // Dark green
        } else {
            Color::Green
        }
    }

    /// Theme-aware yellow color
    pub fn yellow() -> Color {
        if is_light_mode() {
            Color::AnsiValue(136) // Dark gold
        } else {
            Color::Yellow
        }
    }

    /// Theme-aware magenta color
    pub fn magenta() -> Color {
        if is_light_mode() {
            Color::AnsiValue(127) // Dark magenta
        } else {
            Color::Magenta
        }
    }

    /// Theme-aware white/text color
    pub fn white() -> Color {
        if is_light_mode() {
            Color::AnsiValue(235) // Very dark gray
        } else {
            Color::White
        }
    }

    /// Theme-aware gray color
    pub fn gray() -> Color {
        if is_light_mode() {
            Color::AnsiValue(243) // Medium gray
        } else {
            Color::DarkGrey
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colors_return_valid_ansi() {
        // All color functions should return valid ANSI escape sequences
        assert!(CliColors::yellow().starts_with("\x1b["));
        assert!(CliColors::cyan().starts_with("\x1b["));
        assert!(CliColors::green().starts_with("\x1b["));
        assert!(CliColors::red().starts_with("\x1b["));
        assert!(CliColors::magenta().starts_with("\x1b["));
        assert!(CliColors::blue().starts_with("\x1b["));
        assert!(CliColors::text().starts_with("\x1b["));
        assert!(CliColors::gray().starts_with("\x1b["));
        assert!(CliColors::reset().starts_with("\x1b["));
    }

    #[test]
    fn test_reset_code() {
        assert_eq!(CliColors::reset(), "\x1b[0m");
    }
}
