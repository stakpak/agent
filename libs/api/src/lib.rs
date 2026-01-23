use async_trait::async_trait;
use futures_util::Stream;
use models::*;
use reqwest::header::HeaderMap;
use rmcp::model::Content;
use stakpak_shared::models::integrations::openai::{
    ChatCompletionResponse, ChatCompletionStreamResponse, ChatMessage, Tool,
};
use uuid::Uuid;

pub mod client;
pub mod error;
pub mod models;
pub mod stakpak;

// Internal modules (not re-exported directly)
pub(crate) mod local;

// Re-export unified AgentClient as the primary client
pub use client::{
    AgentClient, AgentClientConfig, DEFAULT_STAKPAK_ENDPOINT, ModelOptions, StakpakConfig,
};

// Re-export Model types from stakai
pub use stakai::{Model, ModelCost, ModelLimit};

/// Find a model by ID string
///
/// Parses the model string and searches the catalog:
/// - If format is "provider/model_id", searches within that provider
/// - If no provider prefix, searches all providers by model ID
///
/// When `use_stakpak` is true, the returned model will have provider set to "stakpak"
/// for routing through the Stakpak API.
///
/// Returns None if the model is not found in any catalog.
pub fn find_model(model_str: &str, use_stakpak: bool) -> Option<Model> {
    use stakai::providers::{anthropic, gemini, openai};

    // Split on first '/' to check for provider prefix
    let (provider_prefix, model_id) = if let Some(idx) = model_str.find('/') {
        let (prefix, rest) = model_str.split_at(idx);
        (Some(prefix), &rest[1..]) // Skip the '/'
    } else {
        (None, model_str)
    };

    // Search for the model
    let found_model = match provider_prefix {
        Some("anthropic") => anthropic::models::get_model(model_id),
        Some("openai") => openai::models::get_model(model_id),
        Some("google") | Some("gemini") => gemini::models::get_model(model_id),
        Some(_) => None, // Unknown provider prefix, will search all below
        None => None,
    };

    // If not found with prefix, or no prefix given, search all providers
    let found_model = found_model.or_else(|| {
        anthropic::models::get_model(model_id)
            .or_else(|| openai::models::get_model(model_id))
            .or_else(|| gemini::models::get_model(model_id))
    });

    // Adjust the model for Stakpak routing if needed
    found_model.map(|mut m| {
        if use_stakpak {
            // Prefix the ID with the original provider for Stakpak routing
            let provider_for_id = match m.provider.as_str() {
                "anthropic" => "anthropic",
                "openai" => "openai",
                "google" => "google",
                _ => &m.provider,
            };
            m.id = format!("{}/{}", provider_for_id, m.id);
            m.provider = "stakpak".into();
        }
        m
    })
}

#[async_trait]
pub trait AgentProvider: Send + Sync {
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

    // Agent Sessions
    async fn list_agent_sessions(&self) -> Result<Vec<AgentSession>, String>;
    async fn get_agent_session(&self, session_id: Uuid) -> Result<AgentSession, String>;
    async fn get_agent_session_stats(&self, session_id: Uuid) -> Result<AgentSessionStats, String>;
    async fn get_agent_checkpoint(&self, checkpoint_id: Uuid) -> Result<RunAgentOutput, String>;
    async fn get_agent_session_latest_checkpoint(
        &self,
        session_id: Uuid,
    ) -> Result<RunAgentOutput, String>;

    // Chat
    async fn chat_completion(
        &self,
        model: Model,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
    ) -> Result<ChatCompletionResponse, String>;
    async fn chat_completion_stream(
        &self,
        model: Model,
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

    // Models
    async fn list_models(&self) -> Vec<Model>;
}
