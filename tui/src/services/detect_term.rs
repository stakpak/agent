use ratatui::style::Color;
use std::env;
use std::process::Command;

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

    // Check if we're in a basic Windows console by checking for common indicators
    if env::var("PROMPT").is_ok() || env::var("ComSpec").is_ok() {
        "Windows Console".to_string()
    } else {
        // Fallback for unknown Windows environment
        "Windows Console".to_string()
    }
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

/// Simple function to check if RGB colors should be used
#[allow(dead_code)]
pub fn should_use_rgb_colors() -> bool {
    let terminal_info = detect_terminal();
    terminal_info.supports_rgb_colors
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
}
