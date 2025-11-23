use async_trait::async_trait;
use futures_util::Stream;
use models::*;
use reqwest::header::HeaderMap;
use rmcp::model::Content;
use stakpak_shared::models::integrations::openai::{
    AgentModel, ChatCompletionResponse, ChatCompletionStreamResponse, ChatMessage, Tool,
};
use uuid::Uuid;

pub mod local;
pub mod models;
pub mod remote;

#[async_trait]
pub trait AgentProvider: Send + Sync {
    async fn get_my_account(&self) -> Result<GetMyAccountResponse, String>;
    async fn list_rulebooks(&self) -> Result<Vec<ListRuleBook>, String>;
    async fn get_rulebook_by_uri(&self, uri: &str) -> Result<RuleBook, String>;
    async fn create_rulebook(
        &self,
        uri: &str,
        description: &str,
        content: &str,
        tags: Vec<String>,
        visibility: Option<RuleBookVisibility>,
    ) -> Result<CreateRuleBookResponse, String>;
    async fn delete_rulebook(&self, uri: &str) -> Result<(), String>;
    async fn list_agent_sessions(&self) -> Result<Vec<AgentSession>, String>;
    async fn get_agent_session(&self, session_id: Uuid) -> Result<AgentSession, String>;
    async fn get_agent_session_stats(&self, session_id: Uuid) -> Result<AgentSessionStats, String>;
    async fn get_agent_checkpoint(&self, checkpoint_id: Uuid) -> Result<RunAgentOutput, String>;
    async fn get_agent_session_latest_checkpoint(
        &self,
        session_id: Uuid,
    ) -> Result<RunAgentOutput, String>;
    async fn chat_completion(
        &self,
        model: AgentModel,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
    ) -> Result<ChatCompletionResponse, String>;
    async fn chat_completion_stream(
        &self,
        model: AgentModel,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        headers: Option<HeaderMap>,
    ) -> Result<
        (
            std::pin::Pin<
                Box<dyn Stream<Item = Result<ChatCompletionStreamResponse, ApiStreamError>> + Send>,
            >,
            Option<String>,
        ),
        String,
    >;
    async fn cancel_stream(&self, request_id: String) -> Result<(), String>;
    async fn build_code_index(
        &self,
        input: &BuildCodeIndexInput,
    ) -> Result<BuildCodeIndexOutput, String>;
    async fn search_docs(&self, input: &SearchDocsRequest) -> Result<Vec<Content>, String>;
    async fn search_memory(&self, input: &SearchMemoryRequest) -> Result<Vec<Content>, String>;
    async fn slack_read_messages(
        &self,
        input: &SlackReadMessagesRequest,
    ) -> Result<Vec<Content>, String>;
    async fn slack_read_replies(
        &self,
        input: &SlackReadRepliesRequest,
    ) -> Result<Vec<Content>, String>;
    async fn slack_send_message(
        &self,
        input: &SlackSendMessageRequest,
    ) -> Result<Vec<Content>, String>;
    async fn memorize_session(&self, checkpoint_id: Uuid) -> Result<(), String>;
}
