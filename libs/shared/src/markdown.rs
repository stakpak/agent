//! Markdown utilities.

/// Extract first H1 heading (# Title) from content.
pub fn extract_markdown_title(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let title = line.trim().strip_prefix("# ")?.trim().to_string();
        (!title.is_empty()).then_some(title)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_markdown_title() {
        assert_eq!(
            extract_markdown_title("# Security Review\n\nContent"),
            Some("Security Review".to_string())
        );
        assert_eq!(extract_markdown_title("No heading"), None);
        assert_eq!(extract_markdown_title("## Not H1"), None);
    }
}
