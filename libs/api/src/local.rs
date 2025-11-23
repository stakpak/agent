use crate::{AgentProvider, GetMyAccountResponse};
use crate::{ListRuleBook, models::*};
use async_trait::async_trait;
use futures_util::Stream;
use reqwest::header::HeaderMap;
use rmcp::model::Content;
use stakpak_shared::models::integrations::openai::{
    AgentModel, ChatCompletionResponse, ChatCompletionStreamResponse, ChatMessage, Tool,
};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct LocalClient;

#[async_trait]
impl AgentProvider for LocalClient {
    async fn get_my_account(&self) -> Result<GetMyAccountResponse, String> {
        Err("Local provider does not support account management yet".to_string())
    }

    async fn list_rulebooks(&self) -> Result<Vec<ListRuleBook>, String> {
        Ok(vec![])
    }

    async fn get_rulebook_by_uri(&self, _uri: &str) -> Result<RuleBook, String> {
        Err("Local provider does not support rulebooks yet".to_string())
    }

    async fn create_rulebook(
        &self,
        _uri: &str,
        _description: &str,
        _content: &str,
        _tags: Vec<String>,
        _visibility: Option<RuleBookVisibility>,
    ) -> Result<CreateRuleBookResponse, String> {
        Err("Local provider does not support rulebooks yet".to_string())
    }

    async fn delete_rulebook(&self, _uri: &str) -> Result<(), String> {
        Err("Local provider does not support rulebooks yet".to_string())
    }

    async fn list_agent_sessions(&self) -> Result<Vec<AgentSession>, String> {
        Ok(vec![])
    }

    async fn get_agent_session(&self, _session_id: Uuid) -> Result<AgentSession, String> {
        Err("Local provider does not support sessions yet".to_string())
    }

    async fn get_agent_session_stats(
        &self,
        _session_id: Uuid,
    ) -> Result<AgentSessionStats, String> {
        Err("Local provider does not support sessions yet".to_string())
    }

    async fn create_agent_session(
        &self,
        _agent_id: AgentID,
        _visibility: AgentSessionVisibility,
        _input: Option<AgentInput>,
    ) -> Result<AgentSession, String> {
        Err("Local provider does not support sessions yet".to_string())
    }

    async fn get_agent_checkpoint(&self, _checkpoint_id: Uuid) -> Result<RunAgentOutput, String> {
        Err("Local provider does not support checkpoints yet".to_string())
    }

    async fn get_agent_session_latest_checkpoint(
        &self,
        _session_id: Uuid,
    ) -> Result<RunAgentOutput, String> {
        Err("Local provider does not support checkpoints yet".to_string())
    }

    async fn chat_completion(
        &self,
        _model: AgentModel,
        _messages: Vec<ChatMessage>,
        _tools: Option<Vec<Tool>>,
    ) -> Result<ChatCompletionResponse, String> {
        Err("Local provider does not support chat completion yet".to_string())
    }

    async fn chat_completion_stream(
        &self,
        _model: AgentModel,
        _messages: Vec<ChatMessage>,
        _tools: Option<Vec<Tool>>,
        _headers: Option<HeaderMap>,
    ) -> Result<
        (
            std::pin::Pin<
                Box<dyn Stream<Item = Result<ChatCompletionStreamResponse, ApiStreamError>> + Send>,
            >,
            Option<String>,
        ),
        String,
    > {
        Err("Local provider does not support chat completion yet".to_string())
    }

    async fn cancel_stream(&self, _request_id: String) -> Result<(), String> {
        Ok(())
    }

    async fn build_code_index(
        &self,
        _input: &BuildCodeIndexInput,
    ) -> Result<BuildCodeIndexOutput, String> {
        Err("Local provider does not support code indexing yet".to_string())
    }

    async fn search_docs(&self, _input: &SearchDocsRequest) -> Result<Vec<Content>, String> {
        Err("Local provider does not support search docs yet".to_string())
    }

    async fn search_memory(&self, _input: &SearchMemoryRequest) -> Result<Vec<Content>, String> {
        Err("Local provider does not support search memory yet".to_string())
    }

    async fn slack_read_messages(
        &self,
        _input: &SlackReadMessagesRequest,
    ) -> Result<Vec<Content>, String> {
        Err("Local provider does not support slack read messages yet".to_string())
    }

    async fn slack_read_replies(
        &self,
        _input: &SlackReadRepliesRequest,
    ) -> Result<Vec<Content>, String> {
        Err("Local provider does not support slack read replies yet".to_string())
    }

    async fn slack_send_message(
        &self,
        _input: &SlackSendMessageRequest,
    ) -> Result<Vec<Content>, String> {
        Err("Local provider does not support slack send message yet".to_string())
    }

    async fn memorize_session(&self, _checkpoint_id: Uuid) -> Result<(), String> {
        Ok(())
    }
}
