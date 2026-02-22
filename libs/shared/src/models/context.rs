use serde::{Deserialize, Serialize};

/// Shared API limits for caller-provided context payloads.
pub const MAX_CALLER_CONTEXT_ITEMS: usize = 32;
pub const MAX_CALLER_CONTEXT_NAME_CHARS: usize = 256;
pub const MAX_CALLER_CONTEXT_CONTENT_CHARS: usize = 50_000;
pub const MAX_CALLER_CONTEXT_TOTAL_CHARS: usize = 500_000;

/// Structured caller-provided context injected into server session runs.
///
/// Used by HTTP clients (gateway/watch) and server request parsing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallerContextInput {
    pub name: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
}

/// Validate a batch of caller context inputs against shared limits.
///
/// Used by both the server routes and the gateway client to enforce
/// consistent constraints at the API boundary.
pub fn validate_caller_context(inputs: &[CallerContextInput]) -> Result<(), String> {
    if inputs.len() > MAX_CALLER_CONTEXT_ITEMS {
        return Err(format!(
            "context can include at most {} entries",
            MAX_CALLER_CONTEXT_ITEMS
        ));
    }

    let mut total_content_chars: usize = 0;

    for input in inputs {
        let raw_name_len = input.name.chars().count();
        if raw_name_len > MAX_CALLER_CONTEXT_NAME_CHARS {
            return Err(format!(
                "context.name exceeds {} characters",
                MAX_CALLER_CONTEXT_NAME_CHARS
            ));
        }

        let raw_content_len = input.content.chars().count();
        if raw_content_len > MAX_CALLER_CONTEXT_CONTENT_CHARS {
            return Err(format!(
                "context.content exceeds {} characters",
                MAX_CALLER_CONTEXT_CONTENT_CHARS
            ));
        }

        total_content_chars = total_content_chars.saturating_add(raw_content_len);
        if total_content_chars > MAX_CALLER_CONTEXT_TOTAL_CHARS {
            return Err(format!(
                "total context exceeds {} characters",
                MAX_CALLER_CONTEXT_TOTAL_CHARS
            ));
        }

        let trimmed_name = input.name.trim();
        let trimmed_content = input.content.trim();
        if trimmed_name.is_empty() || trimmed_content.is_empty() {
            continue; // empty values are silently dropped downstream
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_within_limits() {
        let input = CallerContextInput {
            name: "n".repeat(MAX_CALLER_CONTEXT_NAME_CHARS),
            content: "x".repeat(MAX_CALLER_CONTEXT_CONTENT_CHARS),
            priority: Some("high".to_string()),
        };
        assert!(validate_caller_context(&[input]).is_ok());
    }

    #[test]
    fn validate_rejects_too_many_items() {
        let inputs: Vec<_> = (0..MAX_CALLER_CONTEXT_ITEMS + 1)
            .map(|i| CallerContextInput {
                name: format!("ctx-{i}"),
                content: "value".to_string(),
                priority: None,
            })
            .collect();
        assert!(validate_caller_context(&inputs).is_err());
    }

    #[test]
    fn validate_rejects_oversized_name() {
        let input = CallerContextInput {
            name: "n".repeat(MAX_CALLER_CONTEXT_NAME_CHARS + 1),
            content: "value".to_string(),
            priority: None,
        };
        assert!(validate_caller_context(&[input]).is_err());
    }

    #[test]
    fn validate_rejects_oversized_content() {
        let input = CallerContextInput {
            name: "ctx".to_string(),
            content: "x".repeat(MAX_CALLER_CONTEXT_CONTENT_CHARS + 1),
            priority: None,
        };
        assert!(validate_caller_context(&[input]).is_err());
    }

    #[test]
    fn validate_rejects_total_content_over_limit() {
        let inputs: Vec<_> = (0..11)
            .map(|i| CallerContextInput {
                name: format!("ctx-{i}"),
                content: "x".repeat(MAX_CALLER_CONTEXT_CONTENT_CHARS),
                priority: None,
            })
            .collect();

        assert!(validate_caller_context(&inputs).is_err());
    }

    #[test]
    fn validate_rejects_oversized_whitespace_only_name() {
        let input = CallerContextInput {
            name: " ".repeat(MAX_CALLER_CONTEXT_NAME_CHARS + 1),
            content: "value".to_string(),
            priority: None,
        };
        assert!(
            validate_caller_context(&[input]).is_err(),
            "raw name length must be enforced even if trimmed name is empty"
        );
    }

    #[test]
    fn validate_rejects_oversized_whitespace_only_content() {
        let input = CallerContextInput {
            name: "ctx".to_string(),
            content: " ".repeat(MAX_CALLER_CONTEXT_CONTENT_CHARS + 1),
            priority: None,
        };
        assert!(
            validate_caller_context(&[input]).is_err(),
            "raw content length must be enforced even if trimmed content is empty"
        );
    }

    #[test]
    fn validate_skips_small_whitespace_only_content() {
        let input = CallerContextInput {
            name: "ctx".to_string(),
            content: "   ".to_string(),
            priority: None,
        };
        assert!(
            validate_caller_context(&[input]).is_ok(),
            "small whitespace-only content is skipped downstream"
        );
    }
}
