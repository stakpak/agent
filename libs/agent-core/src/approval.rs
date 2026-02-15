use crate::types::{
    AgentCommand, ProposedToolCall, ToolApprovalAction, ToolApprovalPolicy, ToolDecision,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ApprovalEntryState {
    PendingUserDecision,
    Ready(ToolDecision),
    Dispatched,
}

#[derive(Debug, Clone, PartialEq)]
struct ApprovalEntry {
    tool_call: ProposedToolCall,
    state: ApprovalEntryState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedToolCall {
    pub tool_call: ProposedToolCall,
    pub decision: ToolDecision,
}

#[derive(Debug, Clone)]
pub struct ApprovalStateMachine {
    entries: Vec<ApprovalEntry>,
    next_index: usize,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApprovalError {
    #[error("unknown tool_call_id: {tool_call_id}")]
    UnknownToolCallId { tool_call_id: String },

    #[error("tool_call_id {tool_call_id} is already resolved")]
    AlreadyResolved { tool_call_id: String },

    #[error("invalid approval command for state machine")]
    InvalidCommand,
}

impl ApprovalStateMachine {
    pub fn new(tool_calls: Vec<ProposedToolCall>, policy: &ToolApprovalPolicy) -> Self {
        let entries = tool_calls
            .into_iter()
            .map(|tool_call| {
                let initial_state = match policy.action_for(&tool_call.name) {
                    ToolApprovalAction::Approve => ApprovalEntryState::Ready(ToolDecision::Accept),
                    ToolApprovalAction::Deny => ApprovalEntryState::Ready(ToolDecision::Reject),
                    ToolApprovalAction::Ask => ApprovalEntryState::PendingUserDecision,
                };

                ApprovalEntry {
                    tool_call,
                    state: initial_state,
                }
            })
            .collect();

        Self {
            entries,
            next_index: 0,
        }
    }

    pub fn pending_tool_call_ids(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter_map(|entry| {
                if matches!(entry.state, ApprovalEntryState::PendingUserDecision) {
                    Some(entry.tool_call.id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn is_waiting_for_user(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry.state, ApprovalEntryState::PendingUserDecision))
    }

    pub fn is_complete(&self) -> bool {
        self.next_index >= self.entries.len()
    }

    pub fn apply_command(&mut self, command: AgentCommand) -> Result<(), ApprovalError> {
        match command {
            AgentCommand::ResolveTool {
                tool_call_id,
                decision,
            } => self.resolve_tool(&tool_call_id, decision),
            AgentCommand::ResolveTools { decisions } => {
                for (tool_call_id, decision) in decisions {
                    self.resolve_tool(&tool_call_id, decision)?;
                }
                Ok(())
            }
            _ => Err(ApprovalError::InvalidCommand),
        }
    }

    pub fn resolve_tool(
        &mut self,
        tool_call_id: &str,
        decision: ToolDecision,
    ) -> Result<(), ApprovalError> {
        let maybe_entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.tool_call.id == tool_call_id);

        let Some(entry) = maybe_entry else {
            return Err(ApprovalError::UnknownToolCallId {
                tool_call_id: tool_call_id.to_string(),
            });
        };

        match &entry.state {
            ApprovalEntryState::PendingUserDecision => {
                entry.state = ApprovalEntryState::Ready(decision);
                Ok(())
            }
            ApprovalEntryState::Ready(existing) if *existing == decision => Ok(()),
            ApprovalEntryState::Ready(_) | ApprovalEntryState::Dispatched => {
                Err(ApprovalError::AlreadyResolved {
                    tool_call_id: tool_call_id.to_string(),
                })
            }
        }
    }

    pub fn next_ready(&mut self) -> Option<ResolvedToolCall> {
        while self.next_index < self.entries.len() {
            let entry = self.entries.get_mut(self.next_index)?;

            match &entry.state {
                ApprovalEntryState::PendingUserDecision => return None,
                ApprovalEntryState::Ready(decision) => {
                    let resolved = ResolvedToolCall {
                        tool_call: entry.tool_call.clone(),
                        decision: decision.clone(),
                    };
                    entry.state = ApprovalEntryState::Dispatched;
                    self.next_index += 1;
                    return Some(resolved);
                }
                ApprovalEntryState::Dispatched => {
                    self.next_index += 1;
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolApprovalAction;
    use serde_json::json;
    use std::collections::HashMap;

    fn tool_call(id: &str, name: &str) -> ProposedToolCall {
        ProposedToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: json!({"input": id}),
            metadata: None,
        }
    }

    #[test]
    fn incremental_decisions_buffer_until_prior_call_is_resolved() {
        let calls = vec![tool_call("tc_1", "tool_a"), tool_call("tc_2", "tool_b")];
        let mut machine = ApprovalStateMachine::new(calls, &ToolApprovalPolicy::None);

        let out_of_order = machine.resolve_tool("tc_2", ToolDecision::Accept);
        assert!(out_of_order.is_ok());

        assert!(machine.next_ready().is_none());

        let first_resolution = machine.resolve_tool("tc_1", ToolDecision::Reject);
        assert!(first_resolution.is_ok());

        let first = machine.next_ready();
        assert_eq!(
            first,
            Some(ResolvedToolCall {
                tool_call: tool_call("tc_1", "tool_a"),
                decision: ToolDecision::Reject,
            })
        );

        let second = machine.next_ready();
        assert_eq!(
            second,
            Some(ResolvedToolCall {
                tool_call: tool_call("tc_2", "tool_b"),
                decision: ToolDecision::Accept,
            })
        );

        assert!(machine.is_complete());
    }

    #[test]
    fn bulk_resolution_resolves_multiple_calls() {
        let calls = vec![
            tool_call("tc_1", "tool_a"),
            tool_call("tc_2", "tool_b"),
            tool_call("tc_3", "tool_c"),
        ];
        let mut machine = ApprovalStateMachine::new(calls, &ToolApprovalPolicy::None);

        let mut decisions = HashMap::new();
        decisions.insert("tc_1".to_string(), ToolDecision::Accept);
        decisions.insert("tc_2".to_string(), ToolDecision::Reject);

        let command_result = machine.apply_command(AgentCommand::ResolveTools { decisions });
        assert!(command_result.is_ok());

        assert_eq!(
            machine.next_ready(),
            Some(ResolvedToolCall {
                tool_call: tool_call("tc_1", "tool_a"),
                decision: ToolDecision::Accept,
            })
        );

        assert_eq!(
            machine.next_ready(),
            Some(ResolvedToolCall {
                tool_call: tool_call("tc_2", "tool_b"),
                decision: ToolDecision::Reject,
            })
        );

        assert!(machine.next_ready().is_none());
        assert_eq!(machine.pending_tool_call_ids(), vec!["tc_3".to_string()]);
    }

    #[test]
    fn policy_applies_auto_approve_and_auto_deny() {
        let calls = vec![
            tool_call("tc_1", "safe_tool"),
            tool_call("tc_2", "danger_tool"),
            tool_call("tc_3", "unknown_tool"),
        ];

        let mut rules = HashMap::new();
        rules.insert("safe_tool".to_string(), ToolApprovalAction::Approve);
        rules.insert("danger_tool".to_string(), ToolApprovalAction::Deny);

        let policy = ToolApprovalPolicy::Custom {
            rules,
            default: ToolApprovalAction::Ask,
        };

        let mut machine = ApprovalStateMachine::new(calls, &policy);

        assert_eq!(
            machine.next_ready(),
            Some(ResolvedToolCall {
                tool_call: tool_call("tc_1", "safe_tool"),
                decision: ToolDecision::Accept,
            })
        );

        assert_eq!(
            machine.next_ready(),
            Some(ResolvedToolCall {
                tool_call: tool_call("tc_2", "danger_tool"),
                decision: ToolDecision::Reject,
            })
        );

        assert!(machine.next_ready().is_none());
        assert_eq!(machine.pending_tool_call_ids(), vec!["tc_3".to_string()]);
    }

    #[test]
    fn resolve_unknown_tool_call_returns_error() {
        let calls = vec![tool_call("tc_1", "tool_a")];
        let mut machine = ApprovalStateMachine::new(calls, &ToolApprovalPolicy::None);

        let error = machine.resolve_tool("tc_missing", ToolDecision::Accept);

        assert_eq!(
            error,
            Err(ApprovalError::UnknownToolCallId {
                tool_call_id: "tc_missing".to_string(),
            })
        );
    }

    #[test]
    fn resolve_same_decision_is_idempotent() {
        let calls = vec![tool_call("tc_1", "tool_a")];
        let mut machine = ApprovalStateMachine::new(calls, &ToolApprovalPolicy::None);

        assert!(machine.resolve_tool("tc_1", ToolDecision::Accept).is_ok());
        assert!(machine.resolve_tool("tc_1", ToolDecision::Accept).is_ok());

        assert_eq!(
            machine.next_ready(),
            Some(ResolvedToolCall {
                tool_call: tool_call("tc_1", "tool_a"),
                decision: ToolDecision::Accept,
            })
        );
    }
}
