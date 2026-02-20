#[derive(Debug, Clone)]
struct Segment {
    text: String,
    fenced: bool,
}

/// Split text into chunks respecting a character limit.
/// Prefers paragraph/newline/space boundaries for plain text,
/// and never splits inside fenced code blocks (``` ... ```).
pub fn chunk_text(text: &str, limit: usize) -> Vec<String> {
    if text.is_empty() || limit == 0 {
        return Vec::new();
    }

    let segments = split_by_fenced_code(text);
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for segment in segments {
        if segment.fenced {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }

            chunks.push(segment.text);
            continue;
        }

        for piece in split_plain_segment(&segment.text, limit) {
            if current.is_empty() {
                current = piece;
                continue;
            }

            if current.chars().count() + piece.chars().count() <= limit {
                current.push_str(&piece);
            } else {
                chunks.push(std::mem::take(&mut current));
                current = piece;
            }
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

fn split_by_fenced_code(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_fence = false;

    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let is_fence_line = trimmed.starts_with("```");

        if is_fence_line {
            if in_fence {
                current.push_str(line);
                segments.push(Segment {
                    text: std::mem::take(&mut current),
                    fenced: true,
                });
                in_fence = false;
            } else {
                if !current.is_empty() {
                    segments.push(Segment {
                        text: std::mem::take(&mut current),
                        fenced: false,
                    });
                }
                current.push_str(line);
                in_fence = true;
            }
        } else {
            current.push_str(line);
        }
    }

    if !current.is_empty() {
        segments.push(Segment {
            text: current,
            fenced: in_fence,
        });
    }

    segments
}

/// All split indices from char_indices()/rfind() — always valid char boundaries
#[allow(clippy::string_slice)]
fn split_plain_segment(text: &str, limit: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    if text.chars().count() <= limit {
        return vec![text.to_string()];
    }

    let mut remaining = text.to_string();
    let mut chunks = Vec::new();

    while remaining.chars().count() > limit {
        let split_at = find_preferred_split(&remaining, limit)
            .or_else(|| find_char_boundary_at_or_before(&remaining, limit))
            .unwrap_or(remaining.len());

        let head = remaining[..split_at].to_string();
        let tail = remaining[split_at..].to_string();

        if !head.is_empty() {
            chunks.push(head);
        }

        remaining = tail;
    }

    if !remaining.is_empty() {
        chunks.push(remaining);
    }

    chunks
}

fn find_preferred_split(text: &str, limit_chars: usize) -> Option<usize> {
    let prefix = prefix_by_chars(text, limit_chars);

    ["\n\n", "\n", " "]
        .iter()
        .find_map(|separator| prefix.rfind(separator).map(|idx| idx + separator.len()))
        .filter(|idx| *idx > 0)
}

/// idx from char_indices().nth() — always a valid char boundary
#[allow(clippy::string_slice)]
fn prefix_by_chars(text: &str, max_chars: usize) -> &str {
    if text.chars().count() <= max_chars {
        return text;
    }

    if let Some((idx, _)) = text.char_indices().nth(max_chars) {
        &text[..idx]
    } else {
        text
    }
}

fn find_char_boundary_at_or_before(text: &str, limit_chars: usize) -> Option<usize> {
    if limit_chars == 0 {
        return Some(0);
    }

    text.char_indices()
        .nth(limit_chars)
        .map(|(idx, _)| idx)
        .or(if text.is_empty() {
            None
        } else {
            Some(text.len())
        })
}

#[cfg(test)]
mod tests {
    use super::chunk_text;

    #[test]
    fn empty_input_returns_empty_chunks() {
        let chunks = chunk_text("", 10);
        assert!(chunks.is_empty());
    }

    #[test]
    fn under_limit_returns_single_chunk() {
        let chunks = chunk_text("hello", 10);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn exact_limit_returns_single_chunk() {
        let chunks = chunk_text("hello", 5);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn prefers_paragraph_boundaries() {
        let text = "alpha\n\nbeta\n\ngamma";
        let chunks = chunk_text(text, 8);

        assert_eq!(chunks, vec!["alpha\n\n", "beta\n\n", "gamma"]);
    }

    #[test]
    fn falls_back_to_space_boundaries() {
        let text = "alpha beta gamma";
        let chunks = chunk_text(text, 10);

        assert_eq!(chunks, vec!["alpha ", "beta gamma"]);
    }

    #[test]
    fn hard_splits_when_no_breakpoints_exist() {
        let text = "abcdefghij";
        let chunks = chunk_text(text, 3);

        assert_eq!(chunks, vec!["abc", "def", "ghi", "j"]);
    }

    #[test]
    fn does_not_split_inside_code_fence() {
        let text = "before\n```\nvery long code block\n```\nafter";
        let chunks = chunk_text(text, 8);

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], "before\n");
        assert_eq!(chunks[1], "```\nvery long code block\n```\n");
        assert_eq!(chunks[2], "after");
    }

    #[test]
    fn preserves_unicode_boundaries() {
        let text = "🙂🙂🙂🙂";
        let chunks = chunk_text(text, 3);

        assert_eq!(chunks, vec!["🙂🙂🙂", "🙂"]);
    }
}
