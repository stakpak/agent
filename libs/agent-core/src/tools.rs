use crate::{error::AgentError, types::AgentRunContext, types::ProposedToolCall};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolExecutionResult {
    Completed { result: String, is_error: bool },
    Cancelled,
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute_tool_call(
        &self,
        run: &AgentRunContext,
        tool_call: &ProposedToolCall,
        cancel: &CancellationToken,
    ) -> Result<ToolExecutionResult, AgentError>;
}
