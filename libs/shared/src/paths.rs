use std::path::PathBuf;

/// Returns the Stakpak home directory: `~/.stakpak/`
pub fn stakpak_home_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".stakpak")
}
