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
pub mod local;
pub mod models;
pub mod stakpak;
pub mod storage;

// Re-export unified AgentClient as the primary client
pub use client::{
    AgentClient, AgentClientConfig, DEFAULT_STAKPAK_ENDPOINT, ModelOptions, StakpakConfig,
};

// Re-export Model types from stakai
pub use stakai::{Model, ModelCost, ModelLimit};

// Re-export storage types
pub use storage::{
    BoxedSessionStorage, Checkpoint, CheckpointState, CheckpointSummary, CreateCheckpointRequest,
    CreateSessionRequest as StorageCreateSessionRequest, CreateSessionResult, ListCheckpointsQuery,
    ListCheckpointsResult, ListSessionsQuery, ListSessionsResult, LocalStorage, Session,
    SessionStats, SessionStatus, SessionStorage, SessionSummary, SessionVisibility, StakpakStorage,
    StorageError, UpdateSessionRequest as StorageUpdateSessionRequest,
};

/// Find a model by ID string
///
/// Parses the model string and searches the model cache:
/// - Format "provider/model_id" searches within that specific provider
/// - Plain "model_id" searches all providers
///
/// When `use_stakpak` is true, the model is transformed for Stakpak API routing.
pub fn find_model(model_str: &str, use_stakpak: bool) -> Option<Model> {
    const PROVIDERS: &[&str] = &["anthropic", "openai", "google"];

    let (provider_hint, model_id) = parse_model_string(model_str);

    // Search with provider hint first, then fall back to searching all
    let model = provider_hint
        .and_then(|p| find_in_provider(p, model_id))
        .or_else(|| {
            PROVIDERS
                .iter()
                .find_map(|&p| find_in_provider(p, model_id))
        })?;

    Some(if use_stakpak {
        transform_for_stakpak(model)
    } else {
        model
    })
}

/// Parse "provider/model_id" or plain "model_id"
#[allow(clippy::string_slice)] // idx from find('/') on same string, '/' is ASCII
fn parse_model_string(s: &str) -> (Option<&str>, &str) {
    match s.find('/') {
        Some(idx) => {
            let provider = &s[..idx];
            let model_id = &s[idx + 1..];
            let normalized = match provider {
                "gemini" => "google",
                p => p,
            };
            (Some(normalized), model_id)
        }
        None => (None, s),
    }
}

/// Find a model by ID within a specific provider
fn find_in_provider(provider_id: &str, model_id: &str) -> Option<Model> {
    let models = stakai::load_models_for_provider(provider_id).ok()?;

    // Try exact match first
    if let Some(model) = models.iter().find(|m| m.id == model_id) {
        return Some(model.clone());
    }

    // Try prefix match (e.g., "gpt-5.2-2026-01-15" matches catalog's "gpt-5.2")
    // Find the longest matching prefix
    let mut best_match: Option<&Model> = None;
    let mut best_len = 0;

    for model in &models {
        if model_id.starts_with(&model.id) && model.id.len() > best_len {
            best_match = Some(model);
            best_len = model.id.len();
        }
    }

    best_match.cloned()
}

/// Transform a model for Stakpak API routing
fn transform_for_stakpak(model: Model) -> Model {
    Model {
        id: format!("{}/{}", model.provider, model.id),
        provider: "stakpak".into(),
        name: model.name,
        reasoning: model.reasoning,
        cost: model.cost,
        limit: model.limit,
        release_date: model.release_date,
    }
}

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
        model: Model,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        session_id: Option<Uuid>,
        metadata: Option<serde_json::Value>,
    ) -> Result<ChatCompletionResponse, String>;
    async fn chat_completion_stream(
        &self,
        model: Model,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        headers: Option<HeaderMap>,
        session_id: Option<Uuid>,
        metadata: Option<serde_json::Value>,
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
