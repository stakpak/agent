pub mod agent;
pub mod approval;
pub mod checkpoint;
pub mod compaction;
pub mod context;
pub mod error;
pub mod hooks;
pub mod retry;
pub mod stream;
pub mod tools;
pub mod types;

pub use agent::run_agent;
pub use approval::{ApprovalError, ApprovalStateMachine, ResolvedToolCall};
pub use checkpoint::{
    CHECKPOINT_FORMAT_V1, CHECKPOINT_VERSION_V1, CheckpointEnvelopeV1, CheckpointError,
    deserialize_checkpoint, serialize_checkpoint,
};
pub use compaction::{CompactionEngine, CompactionResult, PassthroughCompactionEngine};
pub use context::{
    dedup_tool_results, merge_consecutive_same_role, reduce_context, remove_orphaned_tool_results,
    truncate_old_assistant_messages, truncate_old_tool_results,
};
pub use error::AgentError;
pub use hooks::AgentHook;
pub use retry::{
    RetryDelay, RetryDelaySource, exponential_backoff_ms, parse_retry_delay_from_headers,
    resolve_retry_delay_ms,
};
pub use stream::{
    IndexedStreamEvent, OrderedContentPart, StreamAssemblyError, assemble_ordered_content,
};
pub use tools::{ToolExecutionResult, ToolExecutor};
pub use types::{
    AgentCommand, AgentConfig, AgentEvent, AgentLoopResult, AgentRunContext, CompactionConfig,
    ContextConfig, ProposedToolCall, RetryConfig, StopReason, TokenUsage, ToolApprovalAction,
    ToolApprovalPolicy, ToolDecision, TurnFinishReason,
};
