fn trim_line_endings(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

fn frontmatter_sections(content: &str) -> Option<(serde_yaml::Value, &str, &str)> {
    let mut segments = content.split_inclusive('\n');
    let first = segments.next()?;
    if trim_line_endings(first) != "---" {
        return None;
    }

    let frontmatter_start = first.len();
    let mut cursor = first.len();

    for segment in segments {
        let segment_start = cursor;
        cursor += segment.len();

        if trim_line_endings(segment) == "---" {
            let yaml = content.get(frontmatter_start..segment_start)?;
            let raw = content.get(..cursor)?;
            let body = content.get(cursor..)?;
            let frontmatter = serde_yaml::from_str::<serde_yaml::Value>(yaml).ok()?;
            return Some((frontmatter, raw, body));
        }
    }

    None
}

fn first_paragraph(content: &str) -> String {
    let mut lines = Vec::new();
    let mut started = false;

    for line in content.lines() {
        if line.trim().is_empty() {
            if started {
                break;
            }
            continue;
        }

        started = true;
        lines.push(line);
    }

    lines.join("\n")
}

pub fn parse_frontmatter(content: &str) -> (Option<serde_yaml::Value>, &str) {
    match frontmatter_sections(content) {
        Some((frontmatter, _raw, body)) => (Some(frontmatter), body),
        None => (None, content),
    }
}

pub fn extract_description(content: &str) -> Option<String> {
    let (frontmatter, body) = parse_frontmatter(content);

    if let Some(frontmatter) = frontmatter
        && let Some(description) = frontmatter
            .get("description")
            .and_then(serde_yaml::Value::as_str)
            .map(str::trim)
        && !description.is_empty()
    {
        return Some(description.to_string());
    }

    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
}

pub fn extract_peek(content: &str) -> String {
    if let Some((_frontmatter, raw, body)) = frontmatter_sections(content) {
        let paragraph = first_paragraph(body);
        if paragraph.is_empty() {
            raw.trim_end_matches(['\r', '\n']).to_string()
        } else {
            format!("{}\n{}", raw.trim_end_matches(['\r', '\n']), paragraph)
        }
    } else {
        first_paragraph(content)
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_description, extract_peek, parse_frontmatter};

    #[test]
    fn parse_frontmatter_returns_yaml_and_body_for_valid_frontmatter() {
        let content = "---\ndescription: API rate limits\ntags:\n  - api\n---\nBody text\n";

        let (frontmatter, body) = parse_frontmatter(content);

        let frontmatter = frontmatter.expect("frontmatter should parse");
        assert_eq!(frontmatter["description"].as_str(), Some("API rate limits"));
        assert_eq!(body, "Body text\n");
    }

    #[test]
    fn parse_frontmatter_handles_missing_frontmatter() {
        let content = "Body text\n";

        let (frontmatter, body) = parse_frontmatter(content);

        assert!(frontmatter.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn parse_frontmatter_treats_invalid_yaml_as_body() {
        let content = "---\ndescription: [oops\n---\nBody text\n";

        let (frontmatter, body) = parse_frontmatter(content);

        assert!(frontmatter.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn extract_description_prefers_frontmatter_description() {
        let content =
            "---\ndescription: API rate limits\n---\nFirst body line\n\nSecond paragraph\n";

        assert_eq!(
            extract_description(content),
            Some("API rate limits".to_string())
        );
    }

    #[test]
    fn extract_description_falls_back_to_first_non_empty_body_line() {
        let content = "\n\nThe auth service uses OAuth2 PKCE\n\nDetails\n";

        assert_eq!(
            extract_description(content),
            Some("The auth service uses OAuth2 PKCE".to_string())
        );
    }

    #[test]
    fn extract_peek_returns_frontmatter_and_first_paragraph() {
        let content = "---\ndescription: API rate limits\n---\nThe auth service rate limits at 1000 req/min.\nAfter hitting the limit, responses return 429.\n\nSecond paragraph.\n";

        assert_eq!(
            extract_peek(content),
            "---\ndescription: API rate limits\n---\nThe auth service rate limits at 1000 req/min.\nAfter hitting the limit, responses return 429."
        );
    }

    #[test]
    fn extract_peek_returns_first_paragraph_without_frontmatter() {
        let content = "First paragraph line one.\nStill first paragraph.\n\nSecond paragraph.\n";

        assert_eq!(
            extract_peek(content),
            "First paragraph line one.\nStill first paragraph."
        );
    }
}
