//! Message filtering for `stakpak sessions show`.
//!
//! Applies `--role`, `--limit`, and `--offset` flags to a checkpoint's messages.

use stakpak_shared::models::integrations::openai::{ChatMessage, Role};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleFilter {
    User,
    Assistant,
    Tool,
    System,
}

impl RoleFilter {
    pub fn matches(&self, role: &Role) -> bool {
        match self {
            RoleFilter::User => matches!(role, Role::User),
            RoleFilter::Assistant => matches!(role, Role::Assistant),
            RoleFilter::Tool => matches!(role, Role::Tool),
            RoleFilter::System => matches!(role, Role::System | Role::Developer),
        }
    }
}

impl std::str::FromStr for RoleFilter {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "user" => Ok(RoleFilter::User),
            "assistant" => Ok(RoleFilter::Assistant),
            "tool" => Ok(RoleFilter::Tool),
            "system" => Ok(RoleFilter::System),
            other => Err(format!(
                "invalid role '{}' (expected one of: user, assistant, tool, system)",
                other
            )),
        }
    }
}

/// Apply `--role`, `--limit`, and `--offset` to a checkpoint's messages.
///
/// Ordering: filter by role first, then compute a chronological window anchored at
/// the newest end of the filtered list.
pub fn filter_messages(
    messages: Vec<ChatMessage>,
    role: Option<RoleFilter>,
    limit: Option<u32>,
    offset: u32,
) -> (Vec<ChatMessage>, u32) {
    let filtered: Vec<ChatMessage> = match role {
        Some(filter) => messages
            .into_iter()
            .filter(|m| filter.matches(&m.role))
            .collect(),
        None => messages,
    };

    let total = filtered.len() as u32;
    if offset >= total {
        return (Vec::new(), total);
    }

    let end = total - offset;
    let start = match limit {
        Some(count) => end.saturating_sub(count),
        None => 0,
    };

    let window = filtered
        .into_iter()
        .skip(start as usize)
        .take((end - start) as usize)
        .collect();

    (window, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stakpak_shared::models::integrations::openai::MessageContent;

    fn msg(role: Role, text: &str) -> ChatMessage {
        ChatMessage {
            role,
            content: Some(MessageContent::String(text.to_string())),
            ..Default::default()
        }
    }

    fn sample() -> Vec<ChatMessage> {
        vec![
            msg(Role::System, "sys"),
            msg(Role::User, "u1"),
            msg(Role::Assistant, "a1"),
            msg(Role::Tool, "t1"),
            msg(Role::User, "u2"),
            msg(Role::Assistant, "a2"),
        ]
    }

    fn contents(messages: &[ChatMessage]) -> Vec<&str> {
        messages
            .iter()
            .map(|message| match &message.content {
                Some(MessageContent::String(content)) => content.as_str(),
                other => panic!("expected string content, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn role_filter_user_keeps_only_user_messages_and_reports_total() {
        let (out, total) = filter_messages(sample(), Some(RoleFilter::User), None, 0);
        assert_eq!(contents(&out), vec!["u1", "u2"]);
        assert_eq!(total, 2);
    }

    #[test]
    fn role_filter_assistant_keeps_only_assistant_messages_and_reports_total() {
        let (out, total) = filter_messages(sample(), Some(RoleFilter::Assistant), None, 0);
        assert_eq!(contents(&out), vec!["a1", "a2"]);
        assert_eq!(total, 2);
    }

    #[test]
    fn role_filter_system_includes_developer_and_reports_total() {
        let mut msgs = sample();
        msgs.push(msg(Role::Developer, "dev"));
        let (out, total) = filter_messages(msgs, Some(RoleFilter::System), None, 0);
        assert_eq!(contents(&out), vec!["sys", "dev"]);
        assert_eq!(total, 2);
    }

    #[test]
    fn limit_only_keeps_most_recent_n_messages_in_chronological_order() {
        let (out, total) = filter_messages(sample(), None, Some(3), 0);
        assert_eq!(contents(&out), vec!["t1", "u2", "a2"]);
        assert_eq!(total, 6);
    }

    #[test]
    fn offset_only_drops_messages_from_newest_end_and_returns_remaining_prefix() {
        let (out, total) = filter_messages(sample(), None, None, 2);
        assert_eq!(contents(&out), vec!["sys", "u1", "a1", "t1"]);
        assert_eq!(total, 6);
    }

    #[test]
    fn limit_and_offset_return_mid_range_window() {
        let (out, total) = filter_messages(sample(), None, Some(2), 2);
        assert_eq!(contents(&out), vec!["a1", "t1"]);
        assert_eq!(total, 6);
    }

    #[test]
    fn offset_past_end_returns_empty_and_preserves_total() {
        let (out, total) = filter_messages(sample(), None, Some(2), 99);
        assert!(out.is_empty());
        assert_eq!(total, 6);
    }

    #[test]
    fn limit_larger_than_remaining_returns_all_remaining_messages() {
        let (out, total) = filter_messages(sample(), None, Some(100), 2);
        assert_eq!(contents(&out), vec!["sys", "u1", "a1", "t1"]);
        assert_eq!(total, 6);
    }

    #[test]
    fn role_filter_applies_before_offset_and_limit() {
        let messages = vec![
            msg(Role::User, "u1"),
            msg(Role::Assistant, "a1"),
            msg(Role::User, "u2"),
            msg(Role::User, "u3"),
            msg(Role::Assistant, "a2"),
            msg(Role::User, "u4"),
        ];

        let (out, total) = filter_messages(messages, Some(RoleFilter::Assistant), Some(1), 0);
        assert_eq!(contents(&out), vec!["a2"]);
        assert_eq!(total, 2);
    }

    #[test]
    fn role_filter_with_offset_and_limit_uses_filtered_list_for_windowing() {
        let messages = vec![
            msg(Role::Assistant, "a1"),
            msg(Role::User, "u1"),
            msg(Role::Assistant, "a2"),
            msg(Role::User, "u2"),
            msg(Role::Assistant, "a3"),
            msg(Role::User, "u3"),
            msg(Role::Assistant, "a4"),
        ];

        let (out, total) = filter_messages(messages, Some(RoleFilter::Assistant), Some(2), 1);
        assert_eq!(contents(&out), vec!["a2", "a3"]);
        assert_eq!(total, 4);
    }

    #[test]
    fn empty_input_returns_empty_window_and_zero_total() {
        let (out, total) = filter_messages(vec![], None, Some(10), 0);
        assert!(out.is_empty());
        assert_eq!(total, 0);
    }

    #[test]
    fn role_filter_with_no_matches_returns_empty_and_zero_total() {
        let msgs = vec![msg(Role::User, "u1"), msg(Role::User, "u2")];
        let (out, total) = filter_messages(msgs, Some(RoleFilter::Assistant), Some(1), 0);
        assert!(out.is_empty());
        assert_eq!(total, 0);
    }

    #[test]
    fn from_str_parses_valid_roles() {
        assert_eq!(
            "user".parse::<RoleFilter>().expect("user role"),
            RoleFilter::User
        );
        assert_eq!(
            "Assistant".parse::<RoleFilter>().expect("assistant role"),
            RoleFilter::Assistant
        );
        assert_eq!(
            "TOOL".parse::<RoleFilter>().expect("tool role"),
            RoleFilter::Tool
        );
        assert_eq!(
            "system".parse::<RoleFilter>().expect("system role"),
            RoleFilter::System
        );
    }

    #[test]
    fn from_str_rejects_unknown_role() {
        assert!("robot".parse::<RoleFilter>().is_err());
    }
}
