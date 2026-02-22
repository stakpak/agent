//! Terminal theme detection (light/dark background)
//!
//! Provides a single source of truth for detecting the terminal's color scheme.
//! Used by both CLI and TUI crates to ensure consistent theme detection.

use std::sync::OnceLock;

/// Terminal color theme (light or dark background)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/// Global theme state - detected once at startup and cached
static CURRENT_THEME: OnceLock<Theme> = OnceLock::new();

/// Initialize the theme detection. Call this once at startup.
/// If `override_theme` is Some, use that instead of auto-detection.
pub fn init_theme(override_theme: Option<Theme>) {
    let theme = override_theme.unwrap_or_else(detect_theme);
    // OnceLock::set returns Err if already set, which we ignore
    let _ = CURRENT_THEME.set(theme);
}

/// Get the current theme. Returns Dark if not initialized.
pub fn current_theme() -> Theme {
    *CURRENT_THEME.get_or_init(detect_theme)
}

/// Check if we're in light mode
pub fn is_light_mode() -> bool {
    current_theme() == Theme::Light
}

/// Detect terminal theme using terminal-light crate
/// Falls back to Dark if detection fails
fn detect_theme() -> Theme {
    // First check environment variable override
    if let Ok(theme_env) = std::env::var("STAKPAK_THEME") {
        match theme_env.to_lowercase().as_str() {
            "light" => return Theme::Light,
            "dark" => return Theme::Dark,
            _ => {} // Fall through to detection
        }
    }

    // Use terminal-light for detection (only on unix, Windows falls back)
    #[cfg(unix)]
    {
        // Use a thread with timeout to avoid blocking on slow/unresponsive terminals
        // (e.g., SSH connections, terminals that don't respond to OSC queries)
        use std::sync::mpsc;
        use std::time::Duration;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(terminal_light::luma());
        });

        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(luma)) if luma > 0.5 => return Theme::Light,
            Ok(Ok(_)) => return Theme::Dark,
            Ok(Err(_)) | Err(_) => {
                // Detection failed or timed out - try COLORFGBG fallback
            }
        }
    }

    // Fallback: COLORFGBG environment variable
    detect_theme_from_colorfgbg()
}

/// Fallback theme detection using COLORFGBG environment variable
fn detect_theme_from_colorfgbg() -> Theme {
    // COLORFGBG format: "fg;bg" where bg is ANSI color code
    // 0 = black (dark), 15 = white (light)
    if let Ok(colorfgbg) = std::env::var("COLORFGBG")
        && let Some(bg_str) = colorfgbg.split(';').next_back()
        && let Ok(bg) = bg_str.trim().parse::<u8>()
    {
        // ANSI colors: 0-7 are dark variants, 8-15 are light variants
        // White (15) and light gray (7) typically indicate light background
        if bg == 15 || bg == 7 {
            return Theme::Light;
        }
    }
    Theme::Dark // Default to dark
}
