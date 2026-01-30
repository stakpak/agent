use async_trait::async_trait;
use futures_util::Stream;
use models::*;
use reqwest::header::HeaderMap;
use rmcp::model::Content;
use stakpak_shared::models::integrations::openai::{
    AgentModel, ChatCompletionResponse, ChatCompletionStreamResponse, ChatMessage, Tool,
};
use uuid::Uuid;

pub mod client;
pub mod error;
pub mod local;
pub mod models;
pub mod stakpak;
pub mod storage;

// Re-export unified AgentClient as the primary client
pub use client::{
    AgentClient, AgentClientConfig, DEFAULT_STAKPAK_ENDPOINT, ModelOptions, StakpakConfig,
};

// Re-export storage types
pub use storage::{
    BoxedSessionStorage, Checkpoint, CheckpointState, CheckpointSummary, CreateCheckpointRequest,
    CreateSessionRequest as StorageCreateSessionRequest, CreateSessionResult, ListCheckpointsQuery,
    ListCheckpointsResult, ListSessionsQuery, ListSessionsResult, LocalStorage, Session,
    SessionStats, SessionStatus, SessionStorage, SessionSummary, SessionVisibility, StakpakStorage,
    StorageError, UpdateSessionRequest as StorageUpdateSessionRequest,
};

/// Unified agent provider trait.
///
/// Extends `SessionStorage` so that any `AgentProvider` can also manage
/// sessions and checkpoints.  This avoids passing two separate trait
/// objects through the CLI call-chain.
#[async_trait]
pub trait AgentProvider: SessionStorage + Send + Sync {
    // Account
    async fn get_my_account(&self) -> Result<GetMyAccountResponse, String>;
    async fn get_billing_info(
        &self,
        account_username: &str,
    ) -> Result<stakpak_shared::models::billing::BillingResponse, String>;

    // Rulebooks
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

    // Chat
    async fn chat_completion(
        &self,
        model: AgentModel,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        session_id: Option<Uuid>,
    ) -> Result<ChatCompletionResponse, String>;
    async fn chat_completion_stream(
        &self,
        model: AgentModel,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        headers: Option<HeaderMap>,
        session_id: Option<Uuid>,
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

    // Search Docs
    async fn search_docs(&self, input: &SearchDocsRequest) -> Result<Vec<Content>, String>;

    // Memory
    async fn memorize_session(&self, checkpoint_id: Uuid) -> Result<(), String>;
    async fn search_memory(&self, input: &SearchMemoryRequest) -> Result<Vec<Content>, String>;

    // Slack
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
}
