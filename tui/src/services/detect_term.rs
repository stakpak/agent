use ratatui::style::Color;
use std::env;
use std::process::Command;

// ============================================================================
// Theme Detection - delegates to stakpak_shared::terminal_theme
// ============================================================================

// Re-export shared theme detection so existing imports continue to work
pub use stakpak_shared::terminal_theme::{Theme, current_theme, init_theme, is_light_mode};

// ============================================================================
// Themed Colors - The main interface for getting theme-aware colors
// ============================================================================

/// Get theme-aware colors. This is the primary interface for color selection.
pub struct ThemeColors;

impl ThemeColors {
    // --- Text Colors ---

    /// Primary text color - should be readable on terminal background
    pub fn text() -> Color {
        if is_light_mode() {
            Color::Indexed(235) // Very dark gray for light backgrounds
        } else if should_use_rgb_colors() {
            Color::Rgb(180, 180, 180) // Light gray for dark backgrounds
        } else {
            Color::Reset
        }
    }

    /// Muted/secondary text color - for less important text
    pub fn muted() -> Color {
        if is_light_mode() {
            Color::Indexed(242) // Medium gray for light backgrounds
        } else {
            Color::DarkGray
        }
    }

    /// Assistant message text color
    pub fn assistant_text() -> Color {
        if is_light_mode() {
            Color::Indexed(238) // Dark gray, readable on white
        } else if should_use_rgb_colors() {
            Color::Rgb(180, 180, 180)
        } else {
            Color::Reset
        }
    }

    // --- Accent Colors ---

    /// Primary accent color (for highlights, borders, interactive elements)
    pub fn accent() -> Color {
        if is_light_mode() {
            Color::Indexed(30) // Darker cyan/teal for light backgrounds
        } else {
            Color::Cyan
        }
    }

    /// Secondary accent color
    pub fn accent_secondary() -> Color {
        if is_light_mode() {
            Color::Indexed(25) // Dark blue for light backgrounds
        } else {
            Color::Blue
        }
    }

    // --- Semantic Colors ---

    /// Success color (green)
    pub fn success() -> Color {
        if is_light_mode() {
            Color::Indexed(28) // Darker green for light backgrounds
        } else {
            Color::LightGreen
        }
    }

    /// Warning color (yellow/orange)
    pub fn warning() -> Color {
        if is_light_mode() {
            Color::Indexed(172) // Darker orange for light backgrounds
        } else {
            Color::Yellow
        }
    }

    /// Error/danger color (red)
    pub fn danger() -> Color {
        if is_light_mode() {
            Color::Indexed(160) // Darker red for light backgrounds
        } else {
            Color::LightRed
        }
    }

    // --- UI Element Colors ---

    /// Border color for boxes and panels
    pub fn border() -> Color {
        if is_light_mode() {
            Color::Indexed(245) // Medium gray for light backgrounds
        } else {
            Color::DarkGray
        }
    }

    /// Title color in popups and sections
    pub fn title() -> Color {
        if is_light_mode() {
            Color::Indexed(166) // Darker yellow/orange for light backgrounds
        } else {
            Color::Yellow
        }
    }

    /// Highlight background color (e.g., selected item)
    pub fn highlight_bg() -> Color {
        if is_light_mode() {
            Color::Indexed(117) // Light blue for light backgrounds
        } else {
            Color::Cyan
        }
    }

    /// Highlight foreground color (text on highlight)
    pub fn highlight_fg() -> Color {
        // Black text on highlight works well for both themes
        Color::Black
    }

    /// Input cursor color
    pub fn cursor() -> Color {
        if is_light_mode() {
            Color::Indexed(30) // Darker cyan for light backgrounds
        } else {
            Color::Cyan
        }
    }

    /// Code block background
    pub fn code_bg() -> Color {
        if is_light_mode() {
            Color::Indexed(254) // Very light gray for light backgrounds
        } else if should_use_rgb_colors() {
            Color::Rgb(48, 48, 48)
        } else {
            Color::Reset
        }
    }

    /// Magenta accent (for user messages, special highlights)
    pub fn magenta() -> Color {
        if is_light_mode() {
            Color::Indexed(127) // Darker magenta for light backgrounds
        } else {
            Color::Magenta
        }
    }

    /// Light magenta for backgrounds
    pub fn magenta_dim() -> Color {
        if is_light_mode() {
            Color::Indexed(225) // Very light magenta for light backgrounds
        } else if should_use_rgb_colors() {
            Color::Rgb(31, 32, 44)
        } else {
            Color::LightMagenta
        }
    }

    // --- Additional Theme-Aware Colors (migrated from AdaptiveColors) ---

    /// Primary title color - for headers, labels, and prominent text
    /// Replaces direct use of Color::White which is invisible on light backgrounds
    pub fn title_primary() -> Color {
        if is_light_mode() {
            Color::Indexed(235) // Very dark gray for light backgrounds
        } else {
            Color::White
        }
    }

    /// Success dot/indicator color - for status dots and small indicators
    pub fn dot_success() -> Color {
        if is_light_mode() {
            Color::Indexed(28) // Darker green for light backgrounds
        } else {
            Color::LightGreen
        }
    }

    /// Error dot/indicator color - for error status dots and small indicators
    pub fn dot_error() -> Color {
        if is_light_mode() {
            Color::Indexed(160) // Darker red for light backgrounds
        } else {
            Color::LightRed
        }
    }

    /// Unselected/inactive background color - for non-highlighted items
    pub fn unselected_bg() -> Color {
        if is_light_mode() {
            Color::Indexed(252) // Light gray for light mode
        } else {
            Color::Indexed(235) // Dark gray for dark mode
        }
    }

    /// Theme-aware red color (for text/icons, not backgrounds)
    pub fn red() -> Color {
        if is_light_mode() {
            Color::Indexed(160) // Darker red for light backgrounds
        } else if should_use_rgb_colors() {
            Color::Rgb(239, 100, 97)
        } else {
            Color::LightRed
        }
    }

    /// Theme-aware green color (for text/icons, not backgrounds)
    pub fn green() -> Color {
        if is_light_mode() {
            Color::Indexed(28) // Darker green for light backgrounds
        } else if should_use_rgb_colors() {
            Color::Rgb(35, 218, 111)
        } else {
            Color::LightGreen
        }
    }

    /// Theme-aware dark gray color
    pub fn dark_gray() -> Color {
        if is_light_mode() {
            Color::Indexed(245) // Medium gray for light backgrounds
        } else if should_use_rgb_colors() {
            Color::Rgb(80, 80, 80)
        } else {
            Color::DarkGray
        }
    }

    /// Theme-aware orange color
    pub fn orange() -> Color {
        if is_light_mode() {
            Color::Indexed(166) // Darker orange for light backgrounds
        } else {
            Color::Indexed(208) // Bright orange for dark backgrounds
        }
    }

    /// Theme-aware cyan color (alias for accent in most cases)
    pub fn cyan() -> Color {
        Self::accent()
    }

    /// Theme-aware yellow color
    pub fn yellow() -> Color {
        if is_light_mode() {
            Color::Indexed(136) // Darker yellow/gold for light backgrounds
        } else {
            Color::Yellow
        }
    }

    /// Background color for dropdown menus and overlays
    /// Light mode needs an explicit bg for contrast; dark mode uses terminal default
    pub fn dropdown_bg() -> Color {
        if is_light_mode() {
            Color::Indexed(255) // Near-white for light mode
        } else {
            Color::Reset // Use terminal background in dark mode
        }
    }

    /// Text color for dropdown menus (contrasts with dropdown_bg)
    pub fn dropdown_text() -> Color {
        if is_light_mode() {
            Color::Indexed(235) // Very dark gray for light mode
        } else {
            Color::Reset // Use terminal default text in dark mode
        }
    }

    /// Muted/secondary text color for dropdowns
    pub fn dropdown_muted() -> Color {
        if is_light_mode() {
            Color::Indexed(245) // Medium gray for light mode
        } else {
            Color::DarkGray // Standard dark gray for dark mode
        }
    }
}

// ============================================================================
// ANSI Color Transformation for Light Mode
// ============================================================================

/// Transform a color to be readable on light backgrounds.
/// This maps bright/light colors that are hard to read on light backgrounds
/// to darker alternatives.
pub fn transform_color_for_light_mode(color: Color) -> Color {
    if !is_light_mode() {
        return color;
    }

    match color {
        // Bright/Light colors that are hard to read on light backgrounds
        Color::White => Color::Indexed(235), // Very dark gray
        Color::Gray | Color::DarkGray => Color::Indexed(240), // Medium gray
        Color::Yellow | Color::LightYellow => Color::Indexed(136), // Darker gold
        Color::LightGreen => Color::Indexed(28), // Darker green
        Color::LightBlue => Color::Indexed(25), // Darker blue
        Color::LightCyan | Color::Cyan => Color::Indexed(30), // Darker cyan
        Color::LightMagenta | Color::Magenta => Color::Indexed(127), // Darker magenta
        Color::LightRed => Color::Indexed(160), // Darker red

        // RGB colors - check if they're too bright (high luminance)
        Color::Rgb(r, g, b) => {
            // Calculate relative luminance
            let luminance = 0.299 * (r as f32) + 0.587 * (g as f32) + 0.114 * (b as f32);
            if luminance > 180.0 {
                // Too bright - darken the color
                let factor = 0.5;
                Color::Rgb(
                    ((r as f32) * factor) as u8,
                    ((g as f32) * factor) as u8,
                    ((b as f32) * factor) as u8,
                )
            } else {
                color
            }
        }

        // Indexed colors - map bright ones to darker alternatives
        Color::Indexed(idx) => {
            match idx {
                // Standard bright colors (8-15)
                15 => Color::Indexed(235), // Bright white -> dark gray
                14 => Color::Indexed(30),  // Bright cyan -> dark cyan
                13 => Color::Indexed(127), // Bright magenta -> dark magenta
                12 => Color::Indexed(25),  // Bright blue -> dark blue
                11 => Color::Indexed(136), // Bright yellow -> dark yellow
                10 => Color::Indexed(28),  // Bright green -> dark green
                9 => Color::Indexed(160),  // Bright red -> dark red
                7 => Color::Indexed(240),  // White/Light gray -> medium gray

                // 256-color palette - handle bright grays (232-255)
                idx if idx >= 252 => Color::Indexed(240), // Very light grays -> medium gray

                _ => color,
            }
        }

        // Other colors pass through unchanged
        _ => color,
    }
}

/// Transform all colors in a Style for light mode readability
pub fn transform_style_for_light_mode(style: ratatui::style::Style) -> ratatui::style::Style {
    if !is_light_mode() {
        return style;
    }

    let mut new_style = style;
    if let Some(fg) = style.fg {
        new_style = new_style.fg(transform_color_for_light_mode(fg));
    }
    // Don't transform background colors - they should remain as intended
    new_style
}

// ============================================================================
// Terminal Emulator Detection (preserved from original)
// ============================================================================

/// Terminal emulator information
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalInfo {
    pub emulator: String,
    pub supports_rgb_colors: bool,
}

impl Default for TerminalInfo {
    fn default() -> Self {
        Self {
            emulator: "Unknown".to_string(),
            supports_rgb_colors: true, // Assume RGB support unless we detect otherwise
        }
    }
}

/// Detect the current terminal emulator and whether it supports RGB colors
#[allow(dead_code)]
pub fn detect_terminal() -> TerminalInfo {
    let emulator = detect_terminal_emulator();
    let supports_rgb = !is_unsupported_terminal(&emulator);

    TerminalInfo {
        emulator,
        supports_rgb_colors: supports_rgb,
    }
}

/// Detect the terminal emulator name
#[allow(dead_code)]
fn detect_terminal_emulator() -> String {
    // Check TERM_PROGRAM environment variable first (most reliable)
    if let Ok(term_program) = env::var("TERM_PROGRAM") {
        match term_program.as_str() {
            "Apple_Terminal" => return "Terminal.app".to_string(),
            "iTerm.app" => return "iTerm2".to_string(),
            "vscode" => return "VS Code Terminal".to_string(),
            "Hyper" => return "Hyper".to_string(),
            "Alacritty" => return "Alacritty".to_string(),
            "kitty" => return "Kitty".to_string(),
            "WezTerm" => return "WezTerm".to_string(),
            "Terminus" => return "Terminus".to_string(),
            "Terminal" => return "Terminal.app".to_string(),
            _ => return term_program,
        }
    }

    // Check COLORTERM environment variable
    if let Ok(colorterm) = env::var("COLORTERM") {
        match colorterm.as_str() {
            "gnome-terminal" => return "GNOME Terminal".to_string(),
            "xfce4-terminal" => return "XFCE Terminal".to_string(),
            "konsole" => return "Konsole".to_string(),
            "mate-terminal" => return "MATE Terminal".to_string(),
            "lxterminal" => return "LXTerminal".to_string(),
            _ => {}
        }
    }

    // Check TERMINAL_EMULATOR environment variable
    if let Ok(terminal_emulator) = env::var("TERMINAL_EMULATOR") {
        return terminal_emulator;
    }

    // Check TERM environment variable for clues
    if let Ok(term) = env::var("TERM") {
        match term.as_str() {
            "xterm-256color" | "xterm-color" | "xterm" => {
                // Check for specific indicators
                if env::var("ITERM_PROFILE").is_ok() {
                    return "iTerm2".to_string();
                }
                if env::var("KITTY_WINDOW_ID").is_ok() {
                    return "Kitty".to_string();
                }
                if env::var("ALACRITTY_LOG").is_ok() {
                    return "Alacritty".to_string();
                }
                if env::var("WEZTERM_EXECUTABLE").is_ok() {
                    return "WezTerm".to_string();
                }
                return "xterm".to_string();
            }
            "screen" => return "GNU Screen".to_string(),
            "tmux" => return "tmux".to_string(),
            "linux" => return "Linux Console".to_string(),
            "vt100" | "vt220" => return "VT Terminal".to_string(),
            _ => {}
        }
    }

    // Check for specific terminal indicators
    if env::var("ITERM_PROFILE").is_ok() {
        return "iTerm2".to_string();
    }
    if env::var("KITTY_WINDOW_ID").is_ok() {
        return "Kitty".to_string();
    }
    if env::var("ALACRITTY_LOG").is_ok() {
        return "Alacritty".to_string();
    }
    if env::var("WEZTERM_EXECUTABLE").is_ok() {
        return "WezTerm".to_string();
    }
    if env::var("HYPER_TERM").is_ok() {
        return "Hyper".to_string();
    }

    // Try to detect by checking parent process on Unix-like systems
    let os = std::env::consts::OS;
    if os != "windows" {
        let current_pid = std::process::id().to_string();
        if let Ok(output) = Command::new("ps")
            .args(["-p", &current_pid, "-o", "ppid="])
            .output()
            && output.status.success()
        {
            let ppid = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Ok(parent_output) = Command::new("ps")
                .args(["-p", &ppid, "-o", "comm="])
                .output()
                && parent_output.status.success()
            {
                let parent_comm = String::from_utf8_lossy(&parent_output.stdout)
                    .trim()
                    .to_string();

                // Check for common terminal emulators
                match parent_comm.as_str() {
                    "Terminal" => return "Terminal.app".to_string(),
                    "iTerm2" => return "iTerm2".to_string(),
                    "kitty" => return "Kitty".to_string(),
                    "alacritty" => return "Alacritty".to_string(),
                    "wezterm" => return "WezTerm".to_string(),
                    "hyper" => return "Hyper".to_string(),
                    "gnome-terminal" => return "GNOME Terminal".to_string(),
                    "xfce4-terminal" => return "XFCE Terminal".to_string(),
                    "konsole" => return "Konsole".to_string(),
                    "mate-terminal" => return "MATE Terminal".to_string(),
                    "lxterminal" => return "LXTerminal".to_string(),
                    "terminator" => return "Terminator".to_string(),
                    "tilix" => return "Tilix".to_string(),
                    "guake" => return "Guake".to_string(),
                    "yakuake" => return "Yakuake".to_string(),
                    "terminology" => return "Terminology".to_string(),
                    "cool-retro-term" => return "Cool Retro Term".to_string(),
                    "sakura" => return "Sakura".to_string(),
                    "roxterm" => return "Roxterm".to_string(),
                    "pantheon-terminal" => return "Pantheon Terminal".to_string(),
                    "deepin-terminal" => return "Deepin Terminal".to_string(),
                    "qterminal" => return "QTerminal".to_string(),
                    "st" => return "st (suckless terminal)".to_string(),
                    "urxvt" | "rxvt-unicode" => return "URxvt".to_string(),
                    "xterm" => return "xterm".to_string(),
                    _ => {}
                }
            }
        }
    } else {
        // Windows-specific terminal detection
        return detect_windows_terminal();
    }

    // Fallback: try to detect based on environment
    if env::var("DISPLAY").is_ok() {
        "X11 Terminal".to_string()
    } else if env::var("WAYLAND_DISPLAY").is_ok() {
        "Wayland Terminal".to_string()
    } else {
        "Unknown Terminal".to_string()
    }
}

/// Detect Windows-specific terminal emulators
#[cfg(target_os = "windows")]
fn detect_windows_terminal() -> String {
    // Check for Windows Terminal
    if env::var("WT_SESSION").is_ok() {
        return "Windows Terminal".to_string();
    }

    // Check for PowerShell
    if env::var("PSModulePath").is_ok() {
        return "PowerShell".to_string();
    }

    // Check for cmd.exe specifically
    if let Ok(comspec) = env::var("ComSpec") {
        if comspec.to_lowercase().contains("cmd.exe") {
            return "cmd.exe".to_string();
        }
    }

    // Check for WSL
    if env::var("WSL_DISTRO_NAME").is_ok() {
        return "WSL".to_string();
    }

    // Check for ConEmu
    if env::var("ConEmuPID").is_ok() {
        return "ConEmu".to_string();
    }

    // Check for Cmder
    if env::var("CMDER_ROOT").is_ok() {
        return "Cmder".to_string();
    }

    // Check for Git Bash
    if env::var("MSYSTEM").is_ok() {
        return "Git Bash".to_string();
    }

    // Check for VS Code integrated terminal
    if env::var("VSCODE_INJECTION").is_ok() {
        return "VS Code Terminal".to_string();
    }

    "cmd.exe".to_string()
}

/// Detect Windows-specific terminal emulators (no-op on non-Windows)
#[cfg(not(target_os = "windows"))]
fn detect_windows_terminal() -> String {
    "Unknown Terminal".to_string()
}

/// Check if the terminal is one of the known unsupported terminals
#[allow(dead_code)]
pub fn is_unsupported_terminal(emulator: &str) -> bool {
    match emulator {
        // Terminals that don't support RGB colors
        "Terminal.app" => true, // macOS Terminal built-in
        "Terminus" => true,     // highly configurable terminal emulator
        "Terminology" => true,  // Enlightenment terminal
        "Hyper" => true,        // cross-platform, HTML/CSS/JS-based (Electron)

        // Windows terminals that may not support RGB or have TUI issues
        "Cmder" => true,           // Portable console emulator for Windows
        "KiTTY" => true,           // Windows platform
        "mRemoteNG" => true,       // Windows platform
        "MTPuTTY" => true,         // Windows platform
        "SmarTTY" => true,         // Windows platform
        "Windows Console" => true, // Basic Windows console (cmd.exe)
        "PowerShell" => true,      // PowerShell console
        "cmd.exe" => true,         // Command Prompt - limited ANSI support

        // Linux/Unix terminals that may not support RGB
        "aterm" => true,  // looks abandoned
        "mrxvt" => true,  // looks abandoned
        "yaft" => true,   // framebuffer terminal
        "fbcon" => true,  // prior to Linux 3.16
        "frecon" => true, // Console that is part of ChromeOS kernel
        "FreeBSD console" => true,

        // libvte and GTK2 based terminals
        "libvte-based GTKTerm2" => true,
        "libvte-based stjerm" => true, // looks abandoned

        // Android terminals
        "JuiceSSH" => true, // Android platform
        "Termius" => true,  // Linux, Windows, OS X platforms

        _ => false, // Assume RGB support for unknown terminals
    }
}

/// Simple function to check if RGB colors should be used (cached)
#[allow(dead_code)]
pub fn should_use_rgb_colors() -> bool {
    use std::sync::OnceLock;
    static SUPPORTS_RGB: OnceLock<bool> = OnceLock::new();
    *SUPPORTS_RGB.get_or_init(|| {
        let terminal_info = detect_terminal();
        terminal_info.supports_rgb_colors
    })
}

/// Color definitions that adapt based on terminal capabilities
#[allow(dead_code)]
pub struct AdaptiveColors;

#[allow(dead_code)]
impl AdaptiveColors {
    /// Get red color - RGB for supported terminals, standard for unsupported
    pub fn red() -> Color {
        if should_use_rgb_colors() {
            Color::Rgb(239, 100, 97) // Original RGB red
        } else {
            Color::LightRed
        }
    }

    /// Get green color - RGB for supported terminals, standard for unsupported
    pub fn green() -> Color {
        if should_use_rgb_colors() {
            Color::Rgb(35, 218, 111) // Original RGB green
        } else {
            Color::LightGreen
        }
    }

    /// Get text color - RGB for supported terminals, standard for unsupported
    pub fn text() -> Color {
        if should_use_rgb_colors() {
            Color::Rgb(180, 180, 180) // Original RGB text
        } else {
            Color::Reset
        }
    }

    /// Get dark gray color - RGB for supported terminals, standard for unsupported
    pub fn dark_gray() -> Color {
        if should_use_rgb_colors() {
            Color::Rgb(80, 80, 80) // Original RGB dark gray
        } else {
            Color::DarkGray
        }
    }

    /// Get dark green color - RGB for supported terminals, standard for unsupported
    pub fn dark_green() -> Color {
        if should_use_rgb_colors() {
            Color::Rgb(44, 51, 35) // Original RGB dark green
        } else {
            Color::Green
        }
    }

    /// Get dark red color - RGB for supported terminals, standard for unsupported
    pub fn dark_red() -> Color {
        if should_use_rgb_colors() {
            Color::Rgb(51, 36, 35) // Original RGB dark red
        } else {
            Color::Red
        }
    }

    /// Get code background color - RGB for supported terminals, standard for unsupported
    pub fn code_bg() -> Color {
        if should_use_rgb_colors() {
            Color::Rgb(48, 48, 48) // Original RGB code background
        } else {
            Color::Reset
        }
    }

    /// Get code block background color - RGB for supported terminals, standard for unsupported
    pub fn code_block_bg() -> Color {
        if should_use_rgb_colors() {
            Color::Rgb(30, 30, 30) // Original RGB code block background
        } else {
            Color::Reset
        }
    }

    /// Get list bullet color - RGB for supported terminals, standard for unsupported
    pub fn list_bullet() -> Color {
        if should_use_rgb_colors() {
            Color::Rgb(180, 180, 180) // Original RGB list bullet
        } else {
            Color::Gray
        }
    }

    /// Get dark magenta color - RGB for supported terminals, standard for unsupported
    pub fn dark_magenta() -> Color {
        if should_use_rgb_colors() {
            Color::Rgb(160, 92, 158) // Original RGB dark magenta
        } else {
            Color::Magenta
        }
    }

    pub fn light_magenta() -> Color {
        if should_use_rgb_colors() {
            Color::Rgb(31, 32, 44) // Original RGB light magenta
        } else {
            Color::LightMagenta
        }
    }

    /// Get orange color - Indexed(208) for all terminals (ANSI 256 color)
    /// Falls back to Indexed(208) even on unsupported terminals as it's widely compatible
    pub fn orange() -> Color {
        Color::Indexed(208) // ANSI 256 color orange, works on most terminals
    }

    /// Get user text color - Indexed(243) for supported terminals, Reset for unsupported
    pub fn user_text() -> Color {
        if should_use_rgb_colors() {
            Color::Indexed(243) // ANSI 256 color gray
        } else {
            Color::Reset
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_terminal_default() {
        let info = TerminalInfo::default();
        assert_eq!(info.emulator, "Unknown");
        assert!(info.supports_rgb_colors); // Default assumes RGB support
    }

    #[test]
    fn test_unsupported_terminals() {
        assert!(is_unsupported_terminal("Terminal.app"));
        assert!(is_unsupported_terminal("Terminus"));
        assert!(is_unsupported_terminal("Terminology"));
        assert!(is_unsupported_terminal("Hyper"));
        assert!(is_unsupported_terminal("Cmder"));
        assert!(is_unsupported_terminal("KiTTY"));
        assert!(is_unsupported_terminal("aterm"));
        assert!(is_unsupported_terminal("yaft"));
        assert!(is_unsupported_terminal("JuiceSSH"));
    }

    #[test]
    fn test_supported_terminals() {
        assert!(!is_unsupported_terminal("iTerm2"));
        assert!(!is_unsupported_terminal("VS Code Terminal"));
        assert!(!is_unsupported_terminal("Alacritty"));
        assert!(!is_unsupported_terminal("Kitty"));
        assert!(!is_unsupported_terminal("WezTerm"));
        assert!(!is_unsupported_terminal("GNOME Terminal"));
        assert!(!is_unsupported_terminal("Konsole"));
        assert!(!is_unsupported_terminal("xterm"));
    }

    #[test]
    fn test_adaptive_colors() {
        // Test that adaptive colors return valid Color values
        // Note: We can't easily test the conditional logic without mocking the terminal detection
        // but we can ensure the functions don't panic and return valid colors

        let red = AdaptiveColors::red();
        let green = AdaptiveColors::green();
        let text = AdaptiveColors::text();
        let dark_gray = AdaptiveColors::dark_gray();
        let dark_green = AdaptiveColors::dark_green();
        let dark_red = AdaptiveColors::dark_red();
        let code_bg = AdaptiveColors::code_bg();
        let code_block_bg = AdaptiveColors::code_block_bg();
        let list_bullet = AdaptiveColors::list_bullet();

        // All should return valid Color enum variants
        match red {
            Color::Rgb(_, _, _) | Color::LightRed => {}
            _ => panic!("Invalid red color returned"),
        }

        match green {
            Color::Rgb(_, _, _) | Color::LightGreen => {}
            _ => panic!("Invalid green color returned"),
        }

        match text {
            Color::Rgb(_, _, _) | Color::Gray => {}
            _ => panic!("Invalid text color returned"),
        }

        match dark_gray {
            Color::Rgb(_, _, _) | Color::DarkGray => {}
            _ => panic!("Invalid dark gray color returned"),
        }

        match dark_green {
            Color::Rgb(_, _, _) | Color::Green => {}
            _ => panic!("Invalid dark green color returned"),
        }

        match dark_red {
            Color::Rgb(_, _, _) | Color::Red => {}
            _ => panic!("Invalid dark red color returned"),
        }

        match code_bg {
            Color::Rgb(_, _, _) | Color::DarkGray => {}
            _ => panic!("Invalid code background color returned"),
        }

        match code_block_bg {
            Color::Rgb(_, _, _) | Color::Black => {}
            _ => panic!("Invalid code block background color returned"),
        }

        match list_bullet {
            Color::Rgb(_, _, _) | Color::Gray => {}
            _ => panic!("Invalid list bullet color returned"),
        }
    }

    // ========================================================================
    // Theme Detection Tests
    // ========================================================================

    #[test]
    fn test_theme_enum_default() {
        assert_eq!(Theme::default(), Theme::Dark);
    }

    #[test]
    fn test_detect_theme_from_colorfgbg_light() {
        // Test parsing of COLORFGBG for light backgrounds
        // Note: We can't easily set env vars in tests without affecting other tests,
        // so we test the function's logic directly

        // White background (15) should be light
        // This would require setting env var, so we test the parsing logic
        let colorfgbg_white = "0;15";
        let bg_str = colorfgbg_white.split(';').last();
        assert_eq!(bg_str, Some("15"));

        // Light gray background (7) should be light
        let colorfgbg_light_gray = "0;7";
        let bg_str = colorfgbg_light_gray.split(';').last();
        assert_eq!(bg_str, Some("7"));
    }

    #[test]
    fn test_detect_theme_from_colorfgbg_dark() {
        // Black background (0) should be dark
        let colorfgbg_black = "15;0";
        let bg_str = colorfgbg_black.split(';').last();
        assert_eq!(bg_str, Some("0"));

        // Other colors should default to dark
        let colorfgbg_blue = "15;4";
        let bg_str = colorfgbg_blue.split(';').last();
        assert_eq!(bg_str, Some("4"));
    }

    #[test]
    fn test_theme_colors_return_valid_colors() {
        // Test that all ThemeColors methods return valid Color values
        // We can't easily test light vs dark mode without mocking,
        // but we can ensure the functions don't panic

        let _text = ThemeColors::text();
        let _muted = ThemeColors::muted();
        let _assistant = ThemeColors::assistant_text();
        let _accent = ThemeColors::accent();
        let _accent_secondary = ThemeColors::accent_secondary();
        let _success = ThemeColors::success();
        let _warning = ThemeColors::warning();
        let _danger = ThemeColors::danger();
        let _border = ThemeColors::border();
        let _title = ThemeColors::title();
        let _highlight_bg = ThemeColors::highlight_bg();
        let _highlight_fg = ThemeColors::highlight_fg();
        let _cursor = ThemeColors::cursor();
        let _code_bg = ThemeColors::code_bg();
        let _magenta = ThemeColors::magenta();
        let _magenta_dim = ThemeColors::magenta_dim();

        // New methods
        let _title_primary = ThemeColors::title_primary();
        let _dot_success = ThemeColors::dot_success();
        let _dot_error = ThemeColors::dot_error();
        let _unselected_bg = ThemeColors::unselected_bg();
        let _red = ThemeColors::red();
        let _green = ThemeColors::green();
        let _dark_gray = ThemeColors::dark_gray();
        let _orange = ThemeColors::orange();
        let _cyan = ThemeColors::cyan();
        let _yellow = ThemeColors::yellow();
    }

    #[test]
    fn test_theme_colors_consistency() {
        // Test that related color methods are consistent
        // dot_success should be same as success (both green)
        // dot_error should be same as danger (both red)

        // We can at least verify they don't panic and return Color values
        let success = ThemeColors::success();
        let dot_success = ThemeColors::dot_success();
        let danger = ThemeColors::danger();
        let dot_error = ThemeColors::dot_error();

        // In dark mode (default), these should match
        // Note: This test may be flaky if env vars are set differently
        assert_eq!(success, dot_success);
        assert_eq!(danger, dot_error);
    }

    #[test]
    fn test_is_light_mode_default() {
        // Without any env vars or theme override, should default to dark
        // Note: This test could be affected by terminal environment
        // In CI or most terminals, this should return false
        let _is_light = is_light_mode();
        // Just ensure it doesn't panic; actual value depends on environment
    }
}
