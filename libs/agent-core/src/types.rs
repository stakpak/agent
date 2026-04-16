use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

pub type TokenUsage = stakai::Usage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunContext {
    pub run_id: Uuid,
    pub session_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub model: stakai::Model,
    pub system_prompt: String,
    pub max_turns: usize,
    pub max_output_tokens: u32,
    pub provider_options: Option<stakai::ProviderOptions>,
    pub tool_approval: ToolApprovalPolicy,
    pub retry: RetryConfig,
    pub compaction: CompactionConfig,
    pub tools: Vec<stakai::Tool>,
}

#[derive(Debug, Clone)]
pub struct ContextConfig {
    pub keep_last_messages: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            keep_last_messages: 64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: usize,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff_ms: 2_000,
            max_backoff_ms: 30_000,
            multiplier: 2.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompactionConfig {
    pub enabled: bool,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolApprovalPolicy {
    None,
    All,
    Custom {
        rules: HashMap<String, ToolApprovalAction>,
        default: ToolApprovalAction,
    },
}

const SHELL_TOOLS: &[&str] = &[
    "run_command",
    "run_command_task",
    "run_remote_command",
    "run_remote_command_task",
];

const BASE_SHELL_TOOL: &str = "run_command";

/// Read-only tools that are safe to auto-approve by default.
const DEFAULT_AUTO_APPROVE_TOOLS: &[&str] = &[
    "view",
    "generate_password",
    "search_docs",
    "search_memory",
    "load_skill",
    "local_code_search",
    "get_all_tasks",
    "get_task_details",
    "wait_for_tasks",
];

/// Safe autopilot tools used when no explicit profile allowlist is configured.
pub const SAFE_AUTOPILOT_TOOLS: &[&str] = DEFAULT_AUTO_APPROVE_TOOLS;

/// Mutating tools that require explicit approval by default.
const DEFAULT_ASK_TOOLS: &[&str] = &[
    "create",
    "str_replace",
    "generate_code",
    "run_command",
    "run_command_task",
    "run_remote_command",
    "run_remote_command_task",
    "subagent_task",
    "dynamic_subagent_task",
    "cancel_task",
    "remove",
];

impl ToolApprovalPolicy {
    /// Build a policy with sane defaults: read-only tools auto-approved,
    /// mutating tools require approval, unknown tools require approval.
    pub fn with_defaults() -> Self {
        let mut rules = HashMap::new();

        for name in DEFAULT_AUTO_APPROVE_TOOLS {
            rules.insert((*name).to_string(), ToolApprovalAction::Approve);
        }
        for name in DEFAULT_ASK_TOOLS {
            rules.insert((*name).to_string(), ToolApprovalAction::Ask);
        }

        Self::Custom {
            rules,
            default: ToolApprovalAction::Ask,
        }
    }

    /// Build an unattended policy from an explicit allowlist.
    ///
    /// Listed tools are approved. Everything else is denied.
    pub fn from_allowlist(tools: &[String]) -> Self {
        let mut rules = HashMap::new();

        for name in tools {
            let normalized = strip_tool_prefix(name.trim());
            if normalized.is_empty() {
                continue;
            }
            rules.insert(normalized.to_string(), ToolApprovalAction::Approve);
        }

        Self::Custom {
            rules,
            default: ToolApprovalAction::Deny,
        }
    }

    /// Layer overrides on top of an existing policy.
    /// Only meaningful for `Custom` — returns `self` unchanged for `None`/`All`.
    pub fn with_overrides(
        self,
        overrides: impl IntoIterator<Item = (String, ToolApprovalAction)>,
    ) -> Self {
        match self {
            Self::Custom { mut rules, default } => {
                for (name, action) in overrides {
                    rules.insert(name, action);
                }
                Self::Custom { rules, default }
            }
            other => other,
        }
    }

    /// Determine the approval action for a tool call.
    ///
    /// `tool_arguments` is `Some` when the raw JSON arguments are available
    /// (used for hierarchical shell command resolution on `run_command`).
    /// Pass `None` for tools that have no arguments or when shell inspection
    /// is not needed.
    pub fn action_for(
        &self,
        tool_name: &str,
        tool_arguments: Option<&Value>,
    ) -> ToolApprovalAction {
        let stripped = strip_tool_prefix(tool_name);

        match self {
            Self::None => ToolApprovalAction::Ask,
            Self::All => ToolApprovalAction::Approve,
            Self::Custom { rules, default } => {
                if SHELL_TOOLS.contains(&stripped)
                    && let Some(args) = tool_arguments
                    && let Some(command_str) = args.get("command").and_then(|v| v.as_str())
                {
                    let fallback_scopes = if SHELL_TOOLS
                        .iter()
                        .any(|&c| c == stripped && c != BASE_SHELL_TOOL)
                    {
                        vec![BASE_SHELL_TOOL]
                    } else {
                        Vec::new()
                    };

                    match stakpak_shell_tool_approvals::resolve_hierarchical_policy(
                        command_str,
                        stripped,
                        &fallback_scopes,
                        rules,
                        *default,
                    ) {
                        Ok(Some(action)) => return action,
                        Ok(None) => {}
                        Err(_) => {
                            return conservative_shell_parse_fallback(stripped, rules, *default)
                                .max(ToolApprovalAction::Ask);
                        }
                    }
                }

                rules.get(stripped).copied().unwrap_or(*default)
            }
        }
    }
}

fn conservative_shell_parse_fallback(
    tool_scope: &str,
    rules: &HashMap<String, ToolApprovalAction>,
    default: ToolApprovalAction,
) -> ToolApprovalAction {
    rules
        .get(tool_scope)
        .copied()
        .or_else(|| {
            if SHELL_TOOLS.contains(&tool_scope) && tool_scope != BASE_SHELL_TOOL {
                rules.get(BASE_SHELL_TOOL).copied()
            } else {
                None
            }
        })
        .unwrap_or(default)
}

/// Strip MCP server prefix from tool name (e.g. "stakpak__run_command" -> "run_command").
#[allow(clippy::string_slice)] // pos from find("__") on same string, "__" is ASCII
pub fn strip_tool_prefix(name: &str) -> &str {
    if let Some(pos) = name.find("__")
        && pos + 2 < name.len()
    {
        return &name[pos + 2..];
    }
    name
}

/// Security ordering: `Approve < Ask < Deny`.
///
/// The ordering is enforced via a manual `Ord` implementation so that
/// reordering variants in the future cannot silently break the
/// "most-restrictive wins" aggregation in `action_for`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolApprovalAction {
    /// Least restrictive — auto-approve.
    Approve,
    /// Prompt the user.
    Ask,
    /// Most restrictive — hard reject.
    Deny,
}

impl ToolApprovalAction {
    fn restrictiveness(self) -> u8 {
        match self {
            Self::Approve => 0,
            Self::Ask => 1,
            Self::Deny => 2,
        }
    }
}

impl PartialOrd for ToolApprovalAction {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ToolApprovalAction {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.restrictiveness().cmp(&other.restrictiveness())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentCommand {
    ResolveTool {
        tool_call_id: String,
        decision: ToolDecision,
    },
    ResolveTools {
        decisions: HashMap<String, ToolDecision>,
    },
    Steering(String),
    FollowUp(String),
    SwitchModel(stakai::Model),
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ToolDecision {
    Accept,
    Reject,
    CustomResult { content: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnFinishReason {
    Stop,
    ToolCalls,
    MaxOutputTokens,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Completed,
    Cancelled,
    MaxTurns,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentEvent {
    RunStarted {
        run_id: Uuid,
    },
    TurnStarted {
        run_id: Uuid,
        turn: usize,
    },
    TurnCompleted {
        run_id: Uuid,
        turn: usize,
        finish_reason: TurnFinishReason,
    },
    RunCompleted {
        run_id: Uuid,
        total_turns: usize,
        total_usage: TokenUsage,
        stop_reason: StopReason,
    },
    RunError {
        run_id: Uuid,
        error: String,
        retryable: bool,
    },

    TextDelta {
        run_id: Uuid,
        delta: String,
    },
    ThinkingDelta {
        run_id: Uuid,
        delta: String,
    },
    TextComplete {
        run_id: Uuid,
        text: String,
    },

    ToolCallsProposed {
        run_id: Uuid,
        tool_calls: Vec<ProposedToolCall>,
    },
    WaitingForToolApproval {
        run_id: Uuid,
        pending_tool_call_ids: Vec<String>,
    },
    ToolExecutionStarted {
        run_id: Uuid,
        tool_call_id: String,
        tool_name: String,
    },
    ToolExecutionProgress {
        run_id: Uuid,
        tool_call_id: String,
        message: String,
    },
    ToolExecutionCompleted {
        run_id: Uuid,
        tool_call_id: String,
        tool_name: String,
        result: String,
        is_error: bool,
    },
    ToolRejected {
        run_id: Uuid,
        tool_call_id: String,
        tool_name: String,
        reason: String,
    },

    RetryAttempt {
        run_id: Uuid,
        attempt: usize,
        delay_ms: u64,
        reason: String,
    },
    CompactionStarted {
        run_id: Uuid,
        reason: String,
    },
    CompactionCompleted {
        run_id: Uuid,
        tokens_before: usize,
        tokens_after: usize,
        truncated: bool,
    },

    UsageReport {
        run_id: Uuid,
        turn: usize,
        usage: TokenUsage,
    },
}

#[derive(Debug, Clone)]
pub struct AgentLoopResult {
    pub run_id: Uuid,
    pub total_turns: usize,
    pub total_usage: TokenUsage,
    pub stop_reason: StopReason,
    pub messages: Vec<stakai::Message>,
    pub metadata: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_defaults_auto_approves_readonly_tools() {
        let policy = ToolApprovalPolicy::with_defaults();
        for tool in DEFAULT_AUTO_APPROVE_TOOLS {
            assert_eq!(
                policy.action_for(tool, None),
                ToolApprovalAction::Approve,
                "{tool} should be auto-approved"
            );
        }
    }

    #[test]
    fn with_defaults_asks_for_mutating_tools() {
        let policy = ToolApprovalPolicy::with_defaults();
        for tool in DEFAULT_ASK_TOOLS {
            assert_eq!(
                policy.action_for(tool, None),
                ToolApprovalAction::Ask,
                "{tool} should require approval"
            );
        }
    }

    #[test]
    fn with_defaults_asks_for_unknown_tools() {
        let policy = ToolApprovalPolicy::with_defaults();
        assert_eq!(
            policy.action_for("some_unknown_tool", None),
            ToolApprovalAction::Ask
        );
    }

    #[test]
    fn from_allowlist_approves_listed() {
        let tools = vec!["view".to_string()];
        let policy = ToolApprovalPolicy::from_allowlist(&tools);
        assert_eq!(policy.action_for("view", None), ToolApprovalAction::Approve);
    }

    #[test]
    fn from_allowlist_denies_unlisted() {
        let tools = vec!["view".to_string()];
        let policy = ToolApprovalPolicy::from_allowlist(&tools);
        assert_eq!(
            policy.action_for("run_command", None),
            ToolApprovalAction::Deny
        );
    }

    #[test]
    fn from_allowlist_denies_unknown() {
        let tools = vec!["view".to_string()];
        let policy = ToolApprovalPolicy::from_allowlist(&tools);
        assert_eq!(
            policy.action_for("some_future_tool", None),
            ToolApprovalAction::Deny
        );
    }

    #[test]
    fn from_allowlist_handles_prefix() {
        let tools = vec!["view".to_string()];
        let policy = ToolApprovalPolicy::from_allowlist(&tools);

        assert_eq!(
            policy.action_for("stakpak__view", None),
            ToolApprovalAction::Approve
        );
        assert_eq!(
            policy.action_for("stakpak__run_command", None),
            ToolApprovalAction::Deny
        );
    }

    #[test]
    fn from_allowlist_with_overrides() {
        let tools = vec!["view".to_string()];
        let policy = ToolApprovalPolicy::from_allowlist(&tools)
            .with_overrides([("run_command".to_string(), ToolApprovalAction::Approve)]);

        assert_eq!(
            policy.action_for("run_command", None),
            ToolApprovalAction::Approve
        );
    }

    #[test]
    fn safe_autopilot_tools_is_complete() {
        assert_eq!(SAFE_AUTOPILOT_TOOLS, DEFAULT_AUTO_APPROVE_TOOLS);
    }

    #[test]
    fn with_overrides_promotes_tool_to_approve() {
        let policy = ToolApprovalPolicy::with_defaults()
            .with_overrides([("run_command".to_string(), ToolApprovalAction::Approve)]);
        assert_eq!(
            policy.action_for("run_command", None),
            ToolApprovalAction::Approve
        );
        // Other mutating tools unchanged
        assert_eq!(policy.action_for("create", None), ToolApprovalAction::Ask);
    }

    #[test]
    fn with_overrides_can_deny_tool() {
        let policy = ToolApprovalPolicy::with_defaults()
            .with_overrides([("remove".to_string(), ToolApprovalAction::Deny)]);
        assert_eq!(policy.action_for("remove", None), ToolApprovalAction::Deny);
    }

    #[test]
    fn with_overrides_noop_on_none_and_all() {
        let none = ToolApprovalPolicy::None
            .with_overrides([("view".to_string(), ToolApprovalAction::Approve)]);
        assert_eq!(none.action_for("view", None), ToolApprovalAction::Ask);

        let all = ToolApprovalPolicy::All
            .with_overrides([("view".to_string(), ToolApprovalAction::Deny)]);
        assert_eq!(all.action_for("view", None), ToolApprovalAction::Approve);
    }

    #[test]
    fn action_for_strips_mcp_prefix() {
        let policy = ToolApprovalPolicy::with_defaults();
        assert_eq!(
            policy.action_for("stakpak__view", None),
            ToolApprovalAction::Approve
        );
        assert_eq!(
            policy.action_for("stakpak__run_command", None),
            ToolApprovalAction::Ask
        );
    }

    #[test]
    fn action_for_handles_edge_case_prefixes() {
        let policy = ToolApprovalPolicy::with_defaults();
        // No prefix — works as-is
        assert_eq!(policy.action_for("view", None), ToolApprovalAction::Approve);
        // Double-underscore at end — no stripping (nothing after __)
        assert_eq!(policy.action_for("view__", None), ToolApprovalAction::Ask);
        // Prefix with unknown tool
        assert_eq!(
            policy.action_for("other__unknown_tool", None),
            ToolApprovalAction::Ask
        );
    }

    #[test]
    fn strip_tool_prefix_cases() {
        assert_eq!(strip_tool_prefix("stakpak__run_command"), "run_command");
        assert_eq!(strip_tool_prefix("run_command"), "run_command");
        assert_eq!(strip_tool_prefix("view"), "view");
        assert_eq!(strip_tool_prefix("prefix__tool"), "tool");
        // Edge: __ at end with nothing after — returns original
        assert_eq!(strip_tool_prefix("bad__"), "bad__");
        // Edge: starts with __
        assert_eq!(strip_tool_prefix("__tool"), "tool");
    }

    #[test]
    fn e2e_command_level_rule_approves_git_status() {
        let mut rules = HashMap::new();
        rules.insert("run_command::git".to_string(), ToolApprovalAction::Approve);
        let policy = ToolApprovalPolicy::Custom {
            rules,
            default: ToolApprovalAction::Ask,
        };
        assert_eq!(
            policy.action_for(
                "run_command",
                Some(&serde_json::json!({"command": "git status"}))
            ),
            ToolApprovalAction::Approve
        );
    }

    #[test]
    fn e2e_argument_level_rule_denies_git_push() {
        let mut rules = HashMap::new();
        rules.insert("run_command::git".to_string(), ToolApprovalAction::Approve);
        rules.insert(
            "run_command::git::push".to_string(),
            ToolApprovalAction::Deny,
        );
        let policy = ToolApprovalPolicy::Custom {
            rules,
            default: ToolApprovalAction::Ask,
        };
        assert_eq!(
            policy.action_for(
                "run_command",
                Some(&serde_json::json!({"command": "git push origin main"}))
            ),
            ToolApprovalAction::Deny
        );
    }

    #[test]
    fn e2e_pipeline_most_restrictive_wins() {
        let mut rules = HashMap::new();
        rules.insert("run_command::git".to_string(), ToolApprovalAction::Approve);
        rules.insert(
            "run_command::git::push".to_string(),
            ToolApprovalAction::Deny,
        );
        let policy = ToolApprovalPolicy::Custom {
            rules,
            default: ToolApprovalAction::Ask,
        };
        // "git log" → Approve, "git push" → Deny; max = Deny
        assert_eq!(
            policy.action_for(
                "run_command",
                Some(&serde_json::json!({"command": "git log && git push origin main"}))
            ),
            ToolApprovalAction::Deny
        );
    }

    #[test]
    fn e2e_unknown_command_falls_back_to_default() {
        let mut rules = HashMap::new();
        rules.insert("run_command::git".to_string(), ToolApprovalAction::Approve);
        let policy = ToolApprovalPolicy::Custom {
            rules,
            default: ToolApprovalAction::Ask,
        };
        // "rm" not in rules → default (Ask)
        assert_eq!(
            policy.action_for(
                "run_command",
                Some(&serde_json::json!({"command": "rm -rf /tmp/test"}))
            ),
            ToolApprovalAction::Ask
        );
    }

    #[test]
    fn e2e_glob_pattern_in_argument_rule() {
        let mut rules = HashMap::new();
        rules.insert("run_command::curl".to_string(), ToolApprovalAction::Approve);
        rules.insert(
            "run_command::curl::*.prod.*".to_string(),
            ToolApprovalAction::Deny,
        );
        let policy = ToolApprovalPolicy::Custom {
            rules,
            default: ToolApprovalAction::Ask,
        };
        // non-prod URL → Approve
        assert_eq!(
            policy.action_for(
                "run_command",
                Some(&serde_json::json!({"command": "curl https://api.staging.example.com"}))
            ),
            ToolApprovalAction::Approve
        );
        // prod URL → Deny (glob *.prod.* matches)
        assert_eq!(
            policy.action_for(
                "run_command",
                Some(&serde_json::json!({"command": "curl https://api.prod.example.com"}))
            ),
            ToolApprovalAction::Deny
        );
    }

    #[test]
    fn e2e_nested_sh_c_resolves_inner_commands() {
        let mut rules = HashMap::new();
        rules.insert("run_command::rm".to_string(), ToolApprovalAction::Ask);
        let policy = ToolApprovalPolicy::Custom {
            rules,
            default: ToolApprovalAction::Approve,
        };
        // The outer command is "sh", but the inner script contains "rm"
        // stakpak_shell_tool_approvals recursively extracts inner commands from "sh -c '...'"
        assert_eq!(
            policy.action_for(
                "run_command",
                Some(&serde_json::json!({"command": "sh -c 'rm -rf /tmp/old'"}))
            ),
            ToolApprovalAction::Ask
        );
    }

    #[test]
    fn e2e_argument_level_rule_can_relax_default_when_more_specific() {
        let mut rules = HashMap::new();
        rules.insert(
            "run_command::git::status".to_string(),
            ToolApprovalAction::Approve,
        );
        let policy = ToolApprovalPolicy::Custom {
            rules,
            default: ToolApprovalAction::Ask,
        };

        assert_eq!(
            policy.action_for(
                "run_command",
                Some(&serde_json::json!({"command": "git status"}))
            ),
            ToolApprovalAction::Approve
        );
    }

    #[test]
    fn e2e_run_command_task_specific_rule_overrides_shared_run_command_scope() {
        let mut rules = HashMap::new();
        rules.insert("run_command_task".to_string(), ToolApprovalAction::Deny);
        rules.insert("run_command::git".to_string(), ToolApprovalAction::Approve);
        let policy = ToolApprovalPolicy::Custom {
            rules,
            default: ToolApprovalAction::Ask,
        };

        assert_eq!(
            policy.action_for(
                "run_command_task",
                Some(&serde_json::json!({"command": "git status"}))
            ),
            ToolApprovalAction::Deny
        );
    }

    #[test]
    fn e2e_run_command_task_can_fallback_to_shared_run_command_scope() {
        let mut rules = HashMap::new();
        rules.insert("run_command::git".to_string(), ToolApprovalAction::Approve);
        let policy = ToolApprovalPolicy::Custom {
            rules,
            default: ToolApprovalAction::Ask,
        };

        assert_eq!(
            policy.action_for(
                "run_command_task",
                Some(&serde_json::json!({"command": "git status"}))
            ),
            ToolApprovalAction::Approve
        );
    }

    #[test]
    fn e2e_parse_error_fails_closed_without_losing_explicit_deny() {
        let mut rules = HashMap::new();
        rules.insert("run_command_task".to_string(), ToolApprovalAction::Deny);
        let policy = ToolApprovalPolicy::Custom {
            rules,
            default: ToolApprovalAction::Approve,
        };

        let mut command = "echo deeply nested".to_string();
        for _ in 0..=6 {
            command = format!("sh -c '{}'", command.replace('\'', "'\\''"));
        }

        assert_eq!(
            policy.action_for(
                "run_command_task",
                Some(&serde_json::json!({"command": command}))
            ),
            ToolApprovalAction::Deny
        );
    }
}
