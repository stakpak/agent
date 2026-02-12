use serde::{Deserialize, Serialize};
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
    pub auto_approve: AutoApprovePolicy,
    pub context: ContextConfig,
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
pub enum AutoApprovePolicy {
    None,
    All,
    Custom {
        rules: HashMap<String, ToolApprovalAction>,
        default: ToolApprovalAction,
    },
}

impl AutoApprovePolicy {
    pub fn action_for(&self, tool_name: &str) -> ToolApprovalAction {
        match self {
            Self::None => ToolApprovalAction::Ask,
            Self::All => ToolApprovalAction::Approve,
            Self::Custom { rules, default } => rules.get(tool_name).copied().unwrap_or(*default),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolApprovalAction {
    Approve,
    Ask,
    Deny,
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
}
