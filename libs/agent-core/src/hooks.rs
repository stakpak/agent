use crate::{error::AgentError, types::AgentRunContext, types::ProposedToolCall};
use async_trait::async_trait;
use stakai::{Message, Model};

#[async_trait]
pub trait AgentHook: Send + Sync {
    async fn before_inference(
        &self,
        _run: &AgentRunContext,
        _messages: &[Message],
        _model: &Model,
    ) -> Result<(), AgentError> {
        Ok(())
    }

    async fn after_inference(
        &self,
        _run: &AgentRunContext,
        _messages: &[Message],
        _model: &Model,
    ) -> Result<(), AgentError> {
        Ok(())
    }

    async fn before_tool_execution(
        &self,
        _run: &AgentRunContext,
        _tool_call: &ProposedToolCall,
        _messages: &[Message],
    ) -> Result<(), AgentError> {
        Ok(())
    }

    async fn after_tool_execution(
        &self,
        _run: &AgentRunContext,
        _tool_call: &ProposedToolCall,
        _messages: &[Message],
    ) -> Result<(), AgentError> {
        Ok(())
    }

    async fn on_error(
        &self,
        _run: &AgentRunContext,
        _error: &AgentError,
        _messages: &[Message],
    ) -> Result<(), AgentError> {
        Ok(())
    }
}
