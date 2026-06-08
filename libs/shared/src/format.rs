//! Small formatting helpers shared across crates.
//!
//! Display-oriented utilities for human-readable terminal output.

/// Render a byte count as a short, fixed-precision human string.
///
/// Uses binary units (KB = 1024 B) to match `du -h` and similar tools.
/// One decimal place above the byte threshold for compact output.
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else if bytes < GB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    }
}

/// Git-style short hash — first 8 chars of `hash`, or the full string if
/// it's shorter than 8 chars (defensive for non-hex inputs).
///
/// Safe on UTF-8 because SHA-256 hex is ASCII. The `min` guard handles
/// callers that pass shorter strings
pub fn short_hash(hash: &str) -> &str {
    let end = hash.len().min(8);
    hash.get(..end).unwrap_or(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_renders_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
        // 2.5 MB exact
        assert_eq!(format_size(1024 * 1024 * 5 / 2), "2.5 MB");
    }

    #[test]
    fn short_hash_truncates_and_handles_short_input() {
        let full = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(short_hash(full), "ba7816bf");
        // Shorter-than-8 input falls back to the full string.
        assert_eq!(short_hash("abc"), "abc");
        assert_eq!(short_hash(""), "");
    }
}
