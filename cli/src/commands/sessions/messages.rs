//! Message filtering for `stakpak sessions show`.
//!
//! Applies `--role`, `--limit`, and `--last` flags to a checkpoint's messages.

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

/// Apply `--role`, `--last`, and `--limit` to a checkpoint's messages.
///
/// Ordering: filter by role first, then `--last` narrows to the final message,
/// then `--limit` keeps at most N most recent messages (preserving chronological order).
pub fn filter_messages(
    messages: Vec<ChatMessage>,
    role: Option<RoleFilter>,
    last: bool,
    limit: Option<u32>,
) -> Vec<ChatMessage> {
    if last {
        return messages
            .into_iter()
            .rev()
            .find(|m| role.map(|f| f.matches(&m.role)).unwrap_or(true))
            .map(|m| vec![m])
            .unwrap_or_default();
    }

    let mut filtered: Vec<ChatMessage> = match role {
        Some(filter) => messages
            .into_iter()
            .filter(|m| filter.matches(&m.role))
            .collect(),
        None => messages,
    };

    if let Some(n) = limit {
        let n = n as usize;
        if filtered.len() > n {
            let start = filtered.len() - n;
            filtered = filtered.split_off(start);
        }
    }

    filtered
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

    #[test]
    fn role_filter_user_keeps_only_user_messages() {
        let out = filter_messages(sample(), Some(RoleFilter::User), false, None);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|m| m.role == Role::User));
    }

    #[test]
    fn role_filter_assistant_keeps_only_assistant_messages() {
        let out = filter_messages(sample(), Some(RoleFilter::Assistant), false, None);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|m| m.role == Role::Assistant));
    }

    #[test]
    fn role_filter_system_includes_developer() {
        let mut msgs = sample();
        msgs.push(msg(Role::Developer, "dev"));
        let out = filter_messages(msgs, Some(RoleFilter::System), false, None);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn last_returns_single_most_recent_message_of_any_role() {
        let out = filter_messages(sample(), None, true, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].role, Role::Assistant);
        if let Some(MessageContent::String(s)) = &out[0].content {
            assert_eq!(s, "a2");
        } else {
            panic!("unexpected content");
        }
    }

    #[test]
    fn last_combined_with_role_returns_last_of_that_role() {
        let out = filter_messages(sample(), Some(RoleFilter::User), true, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].role, Role::User);
        if let Some(MessageContent::String(s)) = &out[0].content {
            assert_eq!(s, "u2");
        } else {
            panic!("unexpected content");
        }
    }

    #[test]
    fn limit_keeps_most_recent_n_messages_in_order() {
        let out = filter_messages(sample(), None, false, Some(3));
        assert_eq!(out.len(), 3);
        // Should be the last 3 in chronological order: t1, u2, a2
        assert_eq!(out[0].role, Role::Tool);
        assert_eq!(out[1].role, Role::User);
        assert_eq!(out[2].role, Role::Assistant);
    }

    #[test]
    fn limit_larger_than_len_returns_all() {
        let out = filter_messages(sample(), None, false, Some(100));
        assert_eq!(out.len(), 6);
    }

    #[test]
    fn last_takes_precedence_over_limit() {
        let out = filter_messages(sample(), None, true, Some(3));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].role, Role::Assistant);
    }

    #[test]
    fn last_on_empty_returns_empty() {
        let out = filter_messages(vec![], None, true, None);
        assert!(out.is_empty());
    }

    #[test]
    fn role_filter_with_no_matches_returns_empty() {
        let msgs = vec![msg(Role::User, "u1"), msg(Role::User, "u2")];
        let out = filter_messages(msgs, Some(RoleFilter::Assistant), false, None);
        assert!(out.is_empty());
    }

    #[test]
    fn from_str_parses_valid_roles() {
        assert_eq!("user".parse::<RoleFilter>().unwrap(), RoleFilter::User);
        assert_eq!(
            "Assistant".parse::<RoleFilter>().unwrap(),
            RoleFilter::Assistant
        );
        assert_eq!("TOOL".parse::<RoleFilter>().unwrap(), RoleFilter::Tool);
        assert_eq!("system".parse::<RoleFilter>().unwrap(), RoleFilter::System);
    }

    #[test]
    fn from_str_rejects_unknown_role() {
        assert!("robot".parse::<RoleFilter>().is_err());
    }
}
