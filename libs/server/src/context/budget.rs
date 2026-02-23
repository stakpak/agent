use crate::context::project::{ContextFile, ContextPriority};

const DEFAULT_HEAD_RATIO: f64 = 0.7;
const DEFAULT_TAIL_RATIO: f64 = 0.2;
const MIN_FILE_ALLOCATION_CHARS: usize = 64;

#[derive(Debug, Clone)]
pub struct ContextBudget {
    pub system_prompt_max_chars: usize,
    pub per_file_max_chars: usize,
    pub total_context_max_chars: usize,
    pub head_ratio: f64,
    pub tail_ratio: f64,
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            system_prompt_max_chars: 32_000,
            per_file_max_chars: 20_000,
            total_context_max_chars: 100_000,
            head_ratio: DEFAULT_HEAD_RATIO,
            tail_ratio: DEFAULT_TAIL_RATIO,
        }
    }
}

pub fn truncate_with_marker(content: &str, max_chars: usize, name: &str) -> (String, bool) {
    truncate_with_marker_and_ratio(
        content,
        max_chars,
        name,
        DEFAULT_HEAD_RATIO,
        DEFAULT_TAIL_RATIO,
    )
}

pub fn apply_budget(files: &mut Vec<ContextFile>, budget: &ContextBudget) {
    for file in files.iter_mut() {
        let (content, truncated) = truncate_with_marker_and_ratio(
            &file.content,
            budget.per_file_max_chars,
            &file.name,
            budget.head_ratio,
            budget.tail_ratio,
        );
        file.content = content;
        file.truncated |= truncated;
    }

    let mut prioritized = prioritized_files(files);
    let mut remaining = budget.total_context_max_chars;
    let mut kept = Vec::new();

    for mut file in prioritized.drain(..) {
        let file_chars = file.content.chars().count();
        if file_chars <= remaining {
            remaining -= file_chars;
            kept.push(file);
            continue;
        }

        let should_keep_as_truncated =
            file.priority == ContextPriority::Critical || remaining >= MIN_FILE_ALLOCATION_CHARS;

        if !should_keep_as_truncated {
            continue;
        }

        if remaining == 0 {
            continue;
        }

        let (content, truncated) = truncate_with_marker_and_ratio(
            &file.content,
            remaining,
            &file.name,
            budget.head_ratio,
            budget.tail_ratio,
        );
        file.content = content;
        file.truncated |= truncated;
        remaining = 0;
        kept.push(file);
    }

    *files = kept;
}

fn prioritized_files(files: &[ContextFile]) -> Vec<ContextFile> {
    let mut prioritized = Vec::new();

    for priority in [
        ContextPriority::Critical,
        ContextPriority::High,
        ContextPriority::Normal,
        ContextPriority::CallerSupplied,
    ] {
        for file in files {
            if file.priority == priority {
                prioritized.push(file.clone());
            }
        }
    }

    prioritized
}

fn truncate_with_marker_and_ratio(
    content: &str,
    max_chars: usize,
    name: &str,
    head_ratio: f64,
    tail_ratio: f64,
) -> (String, bool) {
    if max_chars == 0 {
        return (String::new(), !content.is_empty());
    }

    let chars: Vec<char> = content.chars().collect();
    if chars.len() <= max_chars {
        return (content.to_string(), false);
    }

    let marker = format!("\n[... truncated {name}; read file for full content ...]\n");
    let marker_len = marker.chars().count();

    if marker_len >= max_chars {
        let truncated: String = chars.into_iter().take(max_chars).collect();
        return (truncated, true);
    }

    let available = max_chars - marker_len;
    let mut head_count = ((available as f64) * head_ratio).floor() as usize;
    let mut tail_count = ((available as f64) * tail_ratio).floor() as usize;

    if head_count + tail_count > available {
        tail_count = tail_count.min(available.saturating_sub(head_count));
    }

    let used = head_count + tail_count;
    if used < available {
        head_count += available - used;
    }

    let head: String = chars.iter().take(head_count).collect();
    let tail: String = chars
        .iter()
        .rev()
        .take(tail_count)
        .copied()
        .collect::<Vec<char>>()
        .into_iter()
        .rev()
        .collect();

    (format!("{head}{marker}{tail}"), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_with_marker() {
        let content = "x".repeat(1_000);
        let (truncated, changed) = truncate_with_marker(&content, 120, "AGENTS.md");

        assert!(changed);
        assert!(truncated.contains("truncated AGENTS.md"));
        assert!(truncated.chars().count() <= 120);
    }

    #[test]
    fn budget_prioritizes_critical_files() {
        let mut files = vec![
            ContextFile::new(
                "notes",
                "/tmp/notes",
                "x".repeat(500),
                ContextPriority::Normal,
            ),
            ContextFile::new(
                "AGENTS.md",
                "/tmp/AGENTS.md",
                "y".repeat(500),
                ContextPriority::Critical,
            ),
        ];

        apply_budget(
            &mut files,
            &ContextBudget {
                system_prompt_max_chars: 1_000,
                per_file_max_chars: 1_000,
                total_context_max_chars: 300,
                head_ratio: 0.7,
                tail_ratio: 0.2,
            },
        );

        assert!(!files.is_empty());
        assert_eq!(files[0].name, "AGENTS.md");
    }
}
