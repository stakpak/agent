use crate::{ParseError, ParsedCommand, matches_pattern, parse_with_status};
use std::collections::HashMap;

/// Resolve a shell command string against hierarchical approval scopes.
///
/// Scope lookup happens within `primary_scope` first. If no rule in that scope
/// matches a parsed command, fallback scopes are consulted in order.
///
/// Within a single scope, the most specific rule wins:
/// 1. `scope::<command>::<pattern>` (most specific; if multiple match, most restrictive wins)
/// 2. `scope::<command>`
/// 3. `scope`
///
/// Across multiple parsed commands in the same shell script, the most
/// restrictive action wins.
pub fn resolve_hierarchical_policy<T: Clone + Ord>(
    command_str: &str,
    primary_scope: &str,
    fallback_scopes: &[&str],
    rules: &HashMap<String, T>,
    default: T,
) -> Result<Option<T>, ParseError> {
    let parsed_commands = parse_with_status(command_str)?;
    if parsed_commands.is_empty() {
        return Ok(None);
    }

    let scope_chain: Vec<&str> = std::iter::once(primary_scope)
        .chain(fallback_scopes.iter().copied())
        .collect();

    let action = parsed_commands
        .iter()
        .map(|cmd| {
            scope_chain
                .iter()
                .find_map(|scope| resolve_command_in_scope(cmd, scope, rules))
                .unwrap_or_else(|| default.clone())
        })
        .max();

    Ok(action)
}

fn resolve_command_in_scope<T: Clone + Ord>(
    cmd: &ParsedCommand,
    scope: &str,
    rules: &HashMap<String, T>,
) -> Option<T> {
    let Some(name) = &cmd.name else {
        return rules.get(scope).cloned();
    };

    let arg_prefix = format!("{scope}::{name}::");
    let arg_match = rules.iter().filter_map(|(key, action)| {
        let remainder = key.strip_prefix(&arg_prefix)?;
        let segments: Vec<&str> = remainder.split("::").collect();
        if segments.len() > cmd.args.len() {
            return None;
        }
        let matched = segments
            .iter()
            .zip(cmd.args.iter())
            .all(|(segment, arg)| matches_pattern(segment, arg));
        matched.then_some(action.clone())
    });

    if let Some(action) = arg_match.max() {
        return Some(action);
    }

    let command_key = format!("{scope}::{name}");
    if let Some(action) = rules.get(&command_key).cloned() {
        return Some(action);
    }

    rules.get(scope).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Action {
        Approve,
        Ask,
        Deny,
    }

    #[test]
    fn argument_level_rule_can_relax_default_when_more_specific() {
        let mut rules = HashMap::new();
        rules.insert("run_command::git::status".to_string(), Action::Approve);

        let resolved =
            resolve_hierarchical_policy("git status", "run_command", &[], &rules, Action::Ask);

        assert_eq!(resolved, Ok(Some(Action::Approve)));
    }

    #[test]
    fn arg_single_segment_matches_args_first() {
        let mut rules = HashMap::new();
        rules.insert("run_command::git::status".to_string(), Action::Approve);

        let resolved =
            resolve_hierarchical_policy("git status", "run_command", &[], &rules, Action::Ask);

        assert_eq!(resolved, Ok(Some(Action::Approve)));
    }

    #[test]
    fn arg_single_segment_does_not_match_later_positions() {
        let mut rules = HashMap::new();
        rules.insert("run_command::stakpak::ak".to_string(), Action::Approve);

        let resolved = resolve_hierarchical_policy(
            "stakpak browser ak visit example.com",
            "run_command",
            &[],
            &rules,
            Action::Ask,
        );

        assert_eq!(resolved, Ok(Some(Action::Ask)));
    }

    #[test]
    fn arg_multi_segment_matches_exact_path() {
        let mut rules = HashMap::new();
        rules.insert(
            "run_command::stakpak::config::show".to_string(),
            Action::Approve,
        );

        let resolved = resolve_hierarchical_policy(
            "stakpak config show",
            "run_command",
            &[],
            &rules,
            Action::Ask,
        );

        assert_eq!(resolved, Ok(Some(Action::Approve)));
    }

    #[test]
    fn arg_multi_segment_does_not_match_short_command() {
        let mut rules = HashMap::new();
        rules.insert(
            "run_command::stakpak::config::show".to_string(),
            Action::Approve,
        );

        let resolved =
            resolve_hierarchical_policy("stakpak config", "run_command", &[], &rules, Action::Ask);

        assert_eq!(resolved, Ok(Some(Action::Ask)));
    }

    #[test]
    fn arg_multi_segment_does_not_match_different_middle() {
        let mut rules = HashMap::new();
        rules.insert(
            "run_command::stakpak::config::show".to_string(),
            Action::Approve,
        );

        let resolved = resolve_hierarchical_policy(
            "stakpak config list",
            "run_command",
            &[],
            &rules,
            Action::Ask,
        );

        assert_eq!(resolved, Ok(Some(Action::Ask)));
    }

    #[test]
    fn arg_namespace_rule_matches_trailing_args() {
        let mut rules = HashMap::new();
        rules.insert("run_command::stakpak::ak".to_string(), Action::Approve);

        let resolved = resolve_hierarchical_policy(
            "stakpak ak write notes.md",
            "run_command",
            &[],
            &rules,
            Action::Ask,
        );

        assert_eq!(resolved, Ok(Some(Action::Approve)));
    }

    #[test]
    fn arg_namespace_and_sub_rule_both_match_most_restrictive_wins() {
        let mut rules = HashMap::new();
        rules.insert("run_command::stakpak::ak".to_string(), Action::Approve);
        rules.insert("run_command::stakpak::ak::write".to_string(), Action::Ask);

        let resolved = resolve_hierarchical_policy(
            "stakpak ak write notes.md",
            "run_command",
            &[],
            &rules,
            Action::Deny,
        );

        assert_eq!(resolved, Ok(Some(Action::Ask)));
    }

    #[test]
    fn arg_empty_args_command_falls_through_to_command_level() {
        let mut rules = HashMap::new();
        rules.insert("run_command::ls".to_string(), Action::Approve);

        let resolved = resolve_hierarchical_policy("ls", "run_command", &[], &rules, Action::Ask);

        assert_eq!(resolved, Ok(Some(Action::Approve)));
    }

    #[test]
    fn most_restrictive_wins_across_multiple_commands() {
        let mut rules = HashMap::new();
        rules.insert("run_command::git".to_string(), Action::Approve);
        rules.insert("run_command::rm".to_string(), Action::Deny);

        let resolved = resolve_hierarchical_policy(
            "git status && rm -rf /tmp/test",
            "run_command",
            &[],
            &rules,
            Action::Ask,
        );

        assert_eq!(resolved, Ok(Some(Action::Deny)));
    }

    #[test]
    fn primary_scope_wins_over_shared_fallback_scope() {
        let mut rules = HashMap::new();
        rules.insert("run_command_task".to_string(), Action::Deny);
        rules.insert("run_command::git".to_string(), Action::Approve);

        let resolved = resolve_hierarchical_policy(
            "git status",
            "run_command_task",
            &["run_command"],
            &rules,
            Action::Ask,
        );

        assert_eq!(resolved, Ok(Some(Action::Deny)));
    }

    #[test]
    fn shared_fallback_scope_applies_when_primary_scope_has_no_match() {
        let mut rules = HashMap::new();
        rules.insert("run_command::git".to_string(), Action::Approve);

        let resolved = resolve_hierarchical_policy(
            "git status",
            "run_command_task",
            &["run_command"],
            &rules,
            Action::Ask,
        );

        assert_eq!(resolved, Ok(Some(Action::Approve)));
    }
}
