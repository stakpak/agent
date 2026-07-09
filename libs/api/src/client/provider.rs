//! AgentProvider trait implementation for AgentClient
//!
//! Implements the unified provider interface with:
//! - Stakpak-first routing when API key is present
//! - Local fallback when Stakpak is unavailable
//! - Hook registry integration for lifecycle events

use crate::AgentProvider;
use crate::models::*;
use crate::storage::{
    CreateCheckpointRequest as StorageCreateCheckpointRequest,
    CreateSessionRequest as StorageCreateSessionRequest,
    UpdateSessionRequest as StorageUpdateSessionRequest,
};
use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use reqwest::header::HeaderMap;
use rmcp::model::Content;
use stakai::{ContentPart, Message, Model, ResponseContent, StreamEvent};
use stakpak_shared::hooks::{HookContext, LifecycleEvent};
use std::pin::Pin;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Lightweight session info returned by initialize_session / save_checkpoint
#[derive(Debug, Clone)]
pub(crate) struct SessionInfo {
    session_id: Uuid,
    checkpoint_id: Uuid,
    checkpoint_created_at: chrono::DateTime<chrono::Utc>,
}

use super::AgentClient;

// =============================================================================
// Internal Message Types
// =============================================================================

#[derive(Debug)]
pub(crate) enum StreamMessage {
    Event(StreamEvent),
    Ctx(Box<HookContext<AgentState>>),
}

// =============================================================================
// AgentProvider Implementation
// =============================================================================

#[async_trait]
impl AgentProvider for AgentClient {
    // =========================================================================
    // Account
    // =========================================================================

    async fn get_my_account(&self) -> Result<GetMyAccountResponse, String> {
        if let Some(api) = &self.stakpak_api {
            api.get_account().await
        } else {
            // Local stub
            Ok(GetMyAccountResponse {
                username: "local".to_string(),
                id: "local".to_string(),
                first_name: "local".to_string(),
                last_name: "local".to_string(),
                email: "local@stakpak.dev".to_string(),
                scope: None,
            })
        }
    }

    async fn get_billing_info(
        &self,
        account_username: &str,
    ) -> Result<stakpak_shared::models::billing::BillingResponse, String> {
        if let Some(api) = &self.stakpak_api {
            api.get_billing(account_username).await
        } else {
            Err("Billing info not available without Stakpak API key".to_string())
        }
    }

    // =========================================================================
    // Rulebooks
    // =========================================================================

    async fn list_rulebooks(&self) -> Result<Vec<ListRuleBook>, String> {
        if let Some(api) = &self.stakpak_api {
            api.list_rulebooks().await
        } else {
            // Try to fetch public rulebooks via unauthenticated request
            let client = stakpak_shared::tls_client::create_tls_client(
                stakpak_shared::tls_client::TlsClientConfig::default()
                    .with_timeout(std::time::Duration::from_secs(30)),
            )?;

            let url = format!("{}/v1/rules", self.get_stakpak_api_endpoint());
            let response = client.get(&url).send().await.map_err(|e| e.to_string())?;

            if response.status().is_success() {
                let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
                match serde_json::from_value::<ListRulebooksResponse>(value) {
                    Ok(resp) => Ok(resp.results),
                    Err(_) => Ok(vec![]),
                }
            } else {
                Ok(vec![])
            }
        }
    }

    async fn get_rulebook_by_uri(&self, uri: &str) -> Result<RuleBook, String> {
        if let Some(api) = &self.stakpak_api {
            api.get_rulebook_by_uri(uri).await
        } else {
            // Try to fetch public rulebook via unauthenticated request
            let client = stakpak_shared::tls_client::create_tls_client(
                stakpak_shared::tls_client::TlsClientConfig::default()
                    .with_timeout(std::time::Duration::from_secs(30)),
            )?;

            let encoded_uri = urlencoding::encode(uri);
            let url = format!(
                "{}/v1/rules/{}",
                self.get_stakpak_api_endpoint(),
                encoded_uri
            );
            let response = client.get(&url).send().await.map_err(|e| e.to_string())?;

            if response.status().is_success() {
                response.json().await.map_err(|e| e.to_string())
            } else {
                Err("Rulebook not found".to_string())
            }
        }
    }

    async fn create_rulebook(
        &self,
        uri: &str,
        description: &str,
        content: &str,
        tags: Vec<String>,
        visibility: Option<RuleBookVisibility>,
    ) -> Result<CreateRuleBookResponse, String> {
        if let Some(api) = &self.stakpak_api {
            api.create_rulebook(&CreateRuleBookInput {
                uri: uri.to_string(),
                description: description.to_string(),
                content: content.to_string(),
                tags,
                visibility,
            })
            .await
        } else {
            Err("Creating rulebooks requires Stakpak API key".to_string())
        }
    }

    async fn delete_rulebook(&self, uri: &str) -> Result<(), String> {
        if let Some(api) = &self.stakpak_api {
            api.delete_rulebook(uri).await
        } else {
            Err("Deleting rulebooks requires Stakpak API key".to_string())
        }
    }

    // =========================================================================
    // Chat Completion
    // =========================================================================

    async fn chat_completion(
        &self,
        model: Model,
        messages: Vec<Message>,
        tools: Option<Vec<stakai::Tool>>,
        session_id: Option<Uuid>,
        metadata: Option<serde_json::Value>,
    ) -> Result<CompletionResponse, String> {
        let mut ctx = HookContext::new(
            session_id,
            AgentState::new(model, messages, tools, metadata),
        );

        // Execute before request hooks
        self.hook_registry
            .execute_hooks(&mut ctx, &LifecycleEvent::BeforeRequest)
            .await
            .map_err(|e| e.to_string())?
            .ok()?;

        // Initialize or resume session
        let current_session = self.initialize_session(&ctx).await?;
        ctx.set_session_id(current_session.session_id);

        // Run completion
        let new_message = self.run_agent_completion(&mut ctx, None).await?;
        ctx.state.append_new_message(new_message.clone());

        // Save checkpoint
        let result = self
            .save_checkpoint(
                &current_session,
                ctx.state.messages.clone(),
                ctx.state.metadata.clone(),
            )
            .await?;
        let checkpoint_created_at = result.checkpoint_created_at.timestamp() as u64;
        ctx.set_new_checkpoint_id(result.checkpoint_id);

        // Execute after request hooks
        self.hook_registry
            .execute_hooks(&mut ctx, &LifecycleEvent::AfterRequest)
            .await
            .map_err(|e| e.to_string())?
            .ok()?;

        let mut meta = serde_json::Map::new();
        if let Some(session_id) = ctx.session_id {
            meta.insert(
                "session_id".to_string(),
                serde_json::Value::String(session_id.to_string()),
            );
        }
        if let Some(checkpoint_id) = ctx.new_checkpoint_id {
            meta.insert(
                "checkpoint_id".to_string(),
                serde_json::Value::String(checkpoint_id.to_string()),
            );
        }
        if let Some(state_metadata) = &ctx.state.metadata {
            meta.insert("state_metadata".to_string(), state_metadata.clone());
        }

        Ok(CompletionResponse {
            id: ctx.new_checkpoint_id.unwrap().to_string(),
            created: checkpoint_created_at,
            model: ctx
                .state
                .llm_input
                .as_ref()
                .map(|llm_input| llm_input.model.id.clone())
                .unwrap_or_default(),
            message: ctx
                .state
                .messages
                .last()
                .cloned()
                .unwrap_or_else(|| Message::new(stakai::Role::Assistant, "")),
            usage: ctx
                .state
                .llm_output
                .as_ref()
                .map(|u| u.usage.clone())
                .unwrap_or_default(),
            metadata: if meta.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(meta))
            },
        })
    }

    async fn chat_completion_stream(
        &self,
        model: Model,
        messages: Vec<Message>,
        tools: Option<Vec<stakai::Tool>>,
        _headers: Option<HeaderMap>,
        session_id: Option<Uuid>,
        metadata: Option<serde_json::Value>,
    ) -> Result<
        (
            Pin<Box<dyn Stream<Item = Result<AgentStreamEvent, ApiStreamError>> + Send>>,
            Option<String>,
        ),
        String,
    > {
        let mut ctx = HookContext::new(
            session_id,
            AgentState::new(model, messages, tools, metadata),
        );

        // Execute before request hooks
        self.hook_registry
            .execute_hooks(&mut ctx, &LifecycleEvent::BeforeRequest)
            .await
            .map_err(|e| e.to_string())?
            .ok()?;

        // Initialize session
        let current_session = self.initialize_session(&ctx).await?;
        ctx.set_session_id(current_session.session_id);

        let (tx, mut rx) = mpsc::channel::<Result<StreamMessage, String>>(100);

        // Clone what we need for the spawned task
        let client = self.clone();
        let mut ctx_clone = ctx.clone();

        // Spawn the completion task with proper shutdown handling
        // The task checks if the channel is closed before each expensive operation
        // to support graceful shutdown when the stream consumer is dropped
        tokio::spawn(async move {
            // Check if consumer is still listening before starting
            if tx.is_closed() {
                return;
            }

            let result = client
                .run_agent_completion(&mut ctx_clone, Some(tx.clone()))
                .await;

            match result {
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                }
                Ok(new_message) => {
                    // Check if consumer is still listening before continuing
                    if tx.is_closed() {
                        return;
                    }

                    ctx_clone.state.append_new_message(new_message.clone());
                    if tx
                        .send(Ok(StreamMessage::Ctx(Box::new(ctx_clone.clone()))))
                        .await
                        .is_err()
                    {
                        // Consumer dropped, exit gracefully
                        return;
                    }

                    // Check again before expensive session update
                    if tx.is_closed() {
                        return;
                    }

                    let result = client
                        .save_checkpoint(
                            &current_session,
                            ctx_clone.state.messages.clone(),
                            ctx_clone.state.metadata.clone(),
                        )
                        .await;

                    match result {
                        Err(e) => {
                            let _ = tx.send(Err(e)).await;
                        }
                        Ok(updated) => {
                            ctx_clone.set_new_checkpoint_id(updated.checkpoint_id);
                            let _ = tx.send(Ok(StreamMessage::Ctx(Box::new(ctx_clone)))).await;
                        }
                    }
                }
            }
        });

        let hook_registry = self.hook_registry.clone();
        let stream = async_stream::stream! {
            yield Ok(AgentStreamEvent::Model(ctx.state.active_model.clone()));

            while let Some(delta_result) = rx.recv().await {
                match delta_result {
                    Ok(delta) => match delta {
                        StreamMessage::Ctx(updated_ctx) => {
                            ctx = *updated_ctx;
                            // Emit session metadata so callers can track session_id
                            if let Some(session_id) = ctx.session_id {
                                let mut meta = serde_json::Map::new();
                                meta.insert("session_id".to_string(), serde_json::Value::String(session_id.to_string()));
                                if let Some(checkpoint_id) = ctx.new_checkpoint_id {
                                    meta.insert("checkpoint_id".to_string(), serde_json::Value::String(checkpoint_id.to_string()));
                                }
                                if let Some(state_metadata) = &ctx.state.metadata {
                                    meta.insert("state_metadata".to_string(), state_metadata.clone());
                                }
                                yield Ok(AgentStreamEvent::Metadata(serde_json::Value::Object(meta)));
                            }
                        }
                        StreamMessage::Event(event) => {
                            yield Ok(AgentStreamEvent::Event(event))
                        }
                    }
                    Err(e) => yield Err(ApiStreamError::Unknown(e)),
                }
            }

            // Execute after request hooks
            hook_registry
                .execute_hooks(&mut ctx, &LifecycleEvent::AfterRequest)
                .await
                .map_err(|e| e.to_string())?
                .ok()?;
        };

        Ok((Box::pin(stream), None))
    }

    async fn cancel_stream(&self, request_id: String) -> Result<(), String> {
        if let Some(api) = &self.stakpak_api {
            api.cancel_request(&request_id).await
        } else {
            // Local mode doesn't support cancellation yet
            Ok(())
        }
    }

    // =========================================================================
    // Search Docs
    // =========================================================================

    async fn search_docs(&self, input: &SearchDocsRequest) -> Result<Vec<Content>, String> {
        if let Some(api) = &self.stakpak_api {
            api.search_docs(&crate::stakpak::SearchDocsRequest {
                keywords: input.keywords.clone(),
                exclude_keywords: input.exclude_keywords.clone(),
                limit: input.limit,
            })
            .await
        } else {
            // Fallback to local search service
            use stakpak_shared::models::integrations::search_service::*;

            let config = SearchServicesOrchestrator::start()
                .await
                .map_err(|e| e.to_string())?;

            let api_url = format!("http://localhost:{}", config.api_port);
            let search_client = SearchClient::new(api_url);

            let search_results = search_client
                .search_and_scrape(input.keywords.clone(), None)
                .await
                .map_err(|e| e.to_string())?;

            if search_results.is_empty() {
                return Ok(vec![Content::text("No results found".to_string())]);
            }

            Ok(search_results
                .into_iter()
                .map(|result| {
                    let content = result.content.unwrap_or_default();
                    Content::text(format!("URL: {}\nContent: {}", result.url, content))
                })
                .collect())
        }
    }

    // =========================================================================
    // Memory
    // =========================================================================

    async fn memorize_session(&self, checkpoint_id: Uuid) -> Result<(), String> {
        if let Some(api) = &self.stakpak_api {
            api.memorize_session(checkpoint_id).await
        } else {
            // No-op in local mode
            Ok(())
        }
    }

    async fn search_memory(&self, input: &SearchMemoryRequest) -> Result<Vec<Content>, String> {
        if let Some(api) = &self.stakpak_api {
            api.search_memory(&crate::stakpak::SearchMemoryRequest {
                keywords: input.keywords.clone(),
                start_time: input.start_time,
                end_time: input.end_time,
            })
            .await
        } else {
            // Empty results in local mode
            Ok(vec![])
        }
    }

    // =========================================================================
    // Slack
    // =========================================================================

    async fn slack_read_messages(
        &self,
        input: &SlackReadMessagesRequest,
    ) -> Result<Vec<Content>, String> {
        if let Some(api) = &self.stakpak_api {
            api.slack_read_messages(&crate::stakpak::SlackReadMessagesRequest {
                channel: input.channel.clone(),
                limit: input.limit,
            })
            .await
        } else {
            Err("Slack integration requires Stakpak API key".to_string())
        }
    }

    async fn slack_read_replies(
        &self,
        input: &SlackReadRepliesRequest,
    ) -> Result<Vec<Content>, String> {
        if let Some(api) = &self.stakpak_api {
            api.slack_read_replies(&crate::stakpak::SlackReadRepliesRequest {
                channel: input.channel.clone(),
                ts: input.ts.clone(),
            })
            .await
        } else {
            Err("Slack integration requires Stakpak API key".to_string())
        }
    }

    async fn slack_send_message(
        &self,
        input: &SlackSendMessageRequest,
    ) -> Result<Vec<Content>, String> {
        if let Some(api) = &self.stakpak_api {
            api.slack_send_message(&crate::stakpak::SlackSendMessageRequest {
                channel: input.channel.clone(),
                markdown_text: input.markdown_text.clone(),
                thread_ts: input.thread_ts.clone(),
            })
            .await
        } else {
            Err("Slack integration requires Stakpak API key".to_string())
        }
    }

    // =========================================================================
    // Models
    // =========================================================================

    async fn list_models(&self) -> Vec<stakai::Model> {
        // Use the provider registry which only contains providers with configured API keys.
        // This ensures we only list models for providers the user actually has access to.
        // Aggregate per provider so one failing provider does not hide all others.
        let registry = self.stakai.registry();
        let mut all_models = Vec::new();

        for provider_id in registry.list_providers() {
            if let Ok(mut models) = registry.models_for_provider(&provider_id).await {
                all_models.append(&mut models);
            }
        }

        sort_models_by_recency(&mut all_models);
        all_models
    }
}

/// Sort models by release_date descending (newest first)
fn sort_models_by_recency(models: &mut [stakai::Model]) {
    models.sort_by(|a, b| {
        match (&b.release_date, &a.release_date) {
            (Some(b_date), Some(a_date)) => b_date.cmp(a_date),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b.id.cmp(&a.id), // Fallback to ID descending
        }
    });
}

// =============================================================================
// SessionStorage implementation (delegates to inner session_storage)
// =============================================================================

#[async_trait]
impl crate::storage::SessionStorage for super::AgentClient {
    fn backend_info(&self) -> crate::storage::BackendInfo {
        self.session_storage.backend_info()
    }

    async fn list_sessions(
        &self,
        query: &crate::storage::ListSessionsQuery,
    ) -> Result<crate::storage::ListSessionsResult, crate::storage::StorageError> {
        self.session_storage.list_sessions(query).await
    }

    async fn get_session(
        &self,
        session_id: Uuid,
    ) -> Result<crate::storage::Session, crate::storage::StorageError> {
        self.session_storage.get_session(session_id).await
    }

    async fn create_session(
        &self,
        request: &crate::storage::CreateSessionRequest,
    ) -> Result<crate::storage::CreateSessionResult, crate::storage::StorageError> {
        self.session_storage.create_session(request).await
    }

    async fn update_session(
        &self,
        session_id: Uuid,
        request: &crate::storage::UpdateSessionRequest,
    ) -> Result<crate::storage::Session, crate::storage::StorageError> {
        self.session_storage
            .update_session(session_id, request)
            .await
    }

    async fn delete_session(&self, session_id: Uuid) -> Result<(), crate::storage::StorageError> {
        self.session_storage.delete_session(session_id).await
    }

    async fn list_checkpoints(
        &self,
        session_id: Uuid,
        query: &crate::storage::ListCheckpointsQuery,
    ) -> Result<crate::storage::ListCheckpointsResult, crate::storage::StorageError> {
        self.session_storage
            .list_checkpoints(session_id, query)
            .await
    }

    async fn get_checkpoint(
        &self,
        checkpoint_id: Uuid,
    ) -> Result<crate::storage::Checkpoint, crate::storage::StorageError> {
        self.session_storage.get_checkpoint(checkpoint_id).await
    }

    async fn create_checkpoint(
        &self,
        session_id: Uuid,
        request: &crate::storage::CreateCheckpointRequest,
    ) -> Result<crate::storage::Checkpoint, crate::storage::StorageError> {
        self.session_storage
            .create_checkpoint(session_id, request)
            .await
    }

    async fn get_active_checkpoint(
        &self,
        session_id: Uuid,
    ) -> Result<crate::storage::Checkpoint, crate::storage::StorageError> {
        self.session_storage.get_active_checkpoint(session_id).await
    }

    async fn get_session_stats(
        &self,
        session_id: Uuid,
    ) -> Result<crate::storage::SessionStats, crate::storage::StorageError> {
        self.session_storage.get_session_stats(session_id).await
    }
}

// =============================================================================
// Helper Methods
// =============================================================================

fn response_to_message(response: &stakai::GenerateResponse) -> Message {
    let mut parts = Vec::new();

    for content in &response.content {
        match content {
            ResponseContent::Text { text } => parts.push(ContentPart::text(text.clone())),
            ResponseContent::Reasoning { .. } => {}
            ResponseContent::ToolCall(tool_call) => {
                let mut part = ContentPart::tool_call(
                    tool_call.id.clone(),
                    tool_call.name.clone(),
                    tool_call.arguments.clone(),
                );
                if let ContentPart::ToolCall { metadata, .. } = &mut part {
                    *metadata = tool_call.metadata.clone();
                }
                parts.push(part);
            }
        }
    }

    if parts.is_empty() {
        return Message::new(stakai::Role::Assistant, "");
    }

    if parts.len() == 1
        && let ContentPart::Text { text, .. } = &parts[0]
    {
        return Message::new(stakai::Role::Assistant, text.clone());
    }

    Message::new(stakai::Role::Assistant, parts)
}

fn accumulate_stream_tool_event(parts: &mut Vec<ContentPart>, event: &StreamEvent) {
    let (id, event_name, event_arguments, event_delta, event_metadata) = match event {
        StreamEvent::ToolCallStart { id, name } => (id, Some(name), None, None, None),
        StreamEvent::ToolCallDelta { id, delta } => (id, None, None, Some(delta), None),
        StreamEvent::ToolCallEnd {
            id,
            name,
            arguments,
            metadata,
        } => (id, Some(name), Some(arguments), None, metadata.as_ref()),
        _ => return,
    };

    let existing = parts
        .iter_mut()
        .find(|part| matches!(part, ContentPart::ToolCall { id: part_id, .. } if part_id == id));

    match existing {
        Some(ContentPart::ToolCall {
            name,
            arguments,
            metadata,
            ..
        }) => {
            if let Some(new_name) = event_name
                && name.is_empty()
            {
                *name = new_name.clone();
            }
            if let Some(arguments_value) = event_arguments {
                *arguments = arguments_value.clone();
            }
            if let Some(delta) = event_delta {
                if let serde_json::Value::String(current) = arguments {
                    current.push_str(delta);
                } else {
                    *arguments = serde_json::Value::String(delta.clone());
                }
            }
            if let Some(new_metadata) = event_metadata {
                *metadata = Some(new_metadata.clone());
            }
        }
        _ => {
            parts.push(ContentPart::tool_call(
                id.clone(),
                event_name.cloned().unwrap_or_default(),
                event_arguments
                    .cloned()
                    .or_else(|| event_delta.cloned().map(serde_json::Value::String))
                    .unwrap_or_else(|| serde_json::Value::String(String::new())),
            ));
            if let Some(new_metadata) = event_metadata
                && let Some(ContentPart::ToolCall { metadata, .. }) = parts.last_mut()
            {
                *metadata = Some(new_metadata.clone());
            }
        }
    }
}

const TITLE_GENERATOR_PROMPT: &str = include_str!("../prompts/session_title_generator.v1.txt");

impl AgentClient {
    /// Initialize or resume a session based on context
    ///
    /// If `ctx.session_id` is set, we resume that session directly.
    /// Otherwise, we create a new session.
    pub(crate) async fn initialize_session(
        &self,
        ctx: &HookContext<AgentState>,
    ) -> Result<SessionInfo, String> {
        let messages = &ctx.state.messages;

        if messages.is_empty() {
            return Err("At least one message is required".to_string());
        }

        // If session_id is set in context, resume that session directly
        if let Some(session_id) = ctx.session_id {
            let session = self
                .session_storage
                .get_session(session_id)
                .await
                .map_err(|e| e.to_string())?;

            let checkpoint = session
                .active_checkpoint
                .ok_or_else(|| format!("Session {} has no active checkpoint", session_id))?;

            // If the session still has the default title, generate a better one in the background.
            if session.title.trim().is_empty() || session.title == "New Session" {
                let client = self.clone();
                let messages_for_title = messages.to_vec();
                let session_id = session.id;
                let existing_title = session.title.clone();
                tokio::spawn(async move {
                    if let Ok(title) = client.generate_session_title(&messages_for_title).await {
                        let trimmed = title.trim();
                        if !trimmed.is_empty() && trimmed != existing_title {
                            let request =
                                StorageUpdateSessionRequest::new().with_title(trimmed.to_string());
                            let _ = client
                                .session_storage
                                .update_session(session_id, &request)
                                .await;
                        }
                    }
                });
            }

            return Ok(SessionInfo {
                session_id: session.id,
                checkpoint_id: checkpoint.id,
                checkpoint_created_at: checkpoint.created_at,
            });
        }

        // Create new session with a fast local title.
        let fallback_title = Self::fallback_session_title(messages);

        // Get current working directory
        let cwd = std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string());

        // Create session via storage trait
        let mut session_request =
            StorageCreateSessionRequest::new(fallback_title.clone(), messages.to_vec());
        if let Some(cwd) = cwd {
            session_request = session_request.with_cwd(cwd);
        }

        let result = self
            .session_storage
            .create_session(&session_request)
            .await
            .map_err(|e| e.to_string())?;

        // Generate a better title asynchronously and update the session when ready.
        let client = self.clone();
        let messages_for_title = messages.to_vec();
        let session_id = result.session_id;
        tokio::spawn(async move {
            if let Ok(title) = client.generate_session_title(&messages_for_title).await {
                let trimmed = title.trim();
                if !trimmed.is_empty() && trimmed != fallback_title {
                    let request =
                        StorageUpdateSessionRequest::new().with_title(trimmed.to_string());
                    let _ = client
                        .session_storage
                        .update_session(session_id, &request)
                        .await;
                }
            }
        });

        Ok(SessionInfo {
            session_id: result.session_id,
            checkpoint_id: result.checkpoint.id,
            checkpoint_created_at: result.checkpoint.created_at,
        })
    }

    fn fallback_session_title(messages: &[Message]) -> String {
        messages
            .iter()
            .find(|m| m.role == stakai::Role::User)
            .into_iter()
            .filter_map(Message::text)
            .next()
            .map(|text| {
                text.split_whitespace()
                    .take(5)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| "New Session".to_string())
    }

    /// Save a new checkpoint for the current session
    pub(crate) async fn save_checkpoint(
        &self,
        current: &SessionInfo,
        messages: Vec<Message>,
        metadata: Option<serde_json::Value>,
    ) -> Result<SessionInfo, String> {
        let mut checkpoint_request =
            StorageCreateCheckpointRequest::new(messages).with_parent(current.checkpoint_id);

        if let Some(meta) = metadata {
            checkpoint_request = checkpoint_request.with_metadata(meta);
        }

        let checkpoint = self
            .session_storage
            .create_checkpoint(current.session_id, &checkpoint_request)
            .await
            .map_err(|e| e.to_string())?;

        Ok(SessionInfo {
            session_id: current.session_id,
            checkpoint_id: checkpoint.id,
            checkpoint_created_at: checkpoint.created_at,
        })
    }

    /// Run agent completion (inference)
    pub(crate) async fn run_agent_completion(
        &self,
        ctx: &mut HookContext<AgentState>,
        stream_channel_tx: Option<mpsc::Sender<Result<StreamMessage, String>>>,
    ) -> Result<Message, String> {
        // Execute before inference hooks
        self.hook_registry
            .execute_hooks(ctx, &LifecycleEvent::BeforeInference)
            .await
            .map_err(|e| e.to_string())?
            .ok()?;

        let mut input = if let Some(llm_input) = ctx.state.llm_input.clone() {
            llm_input
        } else {
            return Err(
                "LLM input not found, make sure to register a context hook before inference"
                    .to_string(),
            );
        };

        // Inject session_id header if available
        if let Some(session_id) = ctx.session_id {
            input
                .options
                .headers
                .get_or_insert_with(stakai::Headers::new)
                .insert("X-Session-Id", session_id.to_string());
        }

        let (response_message, usage) = if let Some(tx) = stream_channel_tx {
            // Streaming mode
            let mut stream = self
                .stakai
                .stream(&input)
                .await
                .map_err(|e| e.to_string())?;
            let mut text = String::new();
            let mut tool_calls: Vec<ContentPart> = Vec::new();
            let mut usage = stakai::Usage::default();

            while let Some(event_result) = stream.next().await {
                let event = event_result.map_err(|e| e.to_string())?;
                match &event {
                    StreamEvent::TextDelta { delta, .. } => text.push_str(delta),
                    StreamEvent::ToolCallStart { .. }
                    | StreamEvent::ToolCallDelta { .. }
                    | StreamEvent::ToolCallEnd { .. } => {
                        accumulate_stream_tool_event(&mut tool_calls, &event);
                    }
                    StreamEvent::Finish {
                        usage: final_usage, ..
                    } => {
                        usage = final_usage.clone();
                    }
                    StreamEvent::Error { message } => {
                        let _ = tx.send(Err(message.clone())).await;
                        return Err(message.clone());
                    }
                    StreamEvent::Start { .. } | StreamEvent::ReasoningDelta { .. } => {}
                }
                if tx.send(Ok(StreamMessage::Event(event))).await.is_err() {
                    break;
                }
            }

            let message = if tool_calls.is_empty() {
                Message::new(stakai::Role::Assistant, text)
            } else {
                let mut parts: Vec<ContentPart> = Vec::new();
                if !text.is_empty() {
                    parts.push(ContentPart::text(text));
                }
                parts.extend(tool_calls);
                Message::new(stakai::Role::Assistant, parts)
            };
            (message, usage)
        } else {
            // Non-streaming mode
            let response = self
                .stakai
                .generate(&input)
                .await
                .map_err(|e| e.to_string())?;
            (response_to_message(&response), response.usage)
        };

        ctx.state.set_llm_output(response_message, usage);

        // Execute after inference hooks
        self.hook_registry
            .execute_hooks(ctx, &LifecycleEvent::AfterInference)
            .await
            .map_err(|e| e.to_string())?
            .ok()?;

        let llm_output = ctx
            .state
            .llm_output
            .as_ref()
            .ok_or_else(|| "LLM output is missing from state".to_string())?;

        Ok(llm_output.new_message.clone())
    }

    /// Generate a title for a new session
    async fn generate_session_title(&self, messages: &[Message]) -> Result<String, String> {
        // Pick a cheap model from the user's configured providers
        let use_stakpak = self.stakpak.is_some();
        let providers = self.stakai.registry().list_providers();
        let cheap_models: &[(&str, &str)] = &[
            ("stakpak", "claude-haiku-4-5"),
            ("anthropic", "claude-haiku-4-5"),
            ("amazon-bedrock", "claude-haiku-4-5"),
            ("openai", "gpt-4.1-mini"),
            ("google", "gemini-2.5-flash"),
        ];
        let model = cheap_models
            .iter()
            .find_map(|(provider, model_id)| {
                if providers.contains(&provider.to_string()) {
                    crate::find_model(model_id, use_stakpak)
                } else {
                    None
                }
            })
            .ok_or_else(|| "No model available for title generation".to_string())?;

        let title_messages = vec![
            Message::new(stakai::Role::System, TITLE_GENERATOR_PROMPT),
            Message::new(
                stakai::Role::User,
                messages
                    .iter()
                    .filter_map(Message::text)
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ];

        let input = stakai::GenerateRequest {
            model,
            messages: title_messages,
            options: stakai::GenerateOptions {
                max_tokens: Some(100),
                ..Default::default()
            },
            provider_options: None,
            telemetry_metadata: None,
        };

        let response = self
            .stakai
            .generate(&input)
            .await
            .map_err(|e| e.to_string())?;

        Ok(response.text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::stakai::StakAIClient;
    use crate::local::storage::LocalStorage;
    use futures_util::stream;
    use stakai::provider::Provider;
    use stakai::{
        FinishReason, GenerateRequest, GenerateResponse, GenerateStream, Headers, MessageContent,
        ProviderRegistry, Role, Usage,
    };
    use stakpak_shared::hooks::HookContext;
    use std::sync::Arc;

    #[derive(Clone)]
    struct FakeProvider {
        events: Vec<StreamEvent>,
        response: GenerateResponse,
    }

    #[async_trait::async_trait]
    impl Provider for FakeProvider {
        fn provider_id(&self) -> &str {
            "fake"
        }

        fn build_headers(&self, _custom_headers: Option<&Headers>) -> Headers {
            Headers::new()
        }

        async fn generate(&self, _request: GenerateRequest) -> stakai::Result<GenerateResponse> {
            Ok(self.response.clone())
        }

        async fn stream(&self, _request: GenerateRequest) -> stakai::Result<GenerateStream> {
            let events = self.events.clone().into_iter().map(Ok::<_, stakai::Error>);
            Ok(GenerateStream::new(Box::pin(stream::iter(events))))
        }
    }

    fn generate_response(content: Vec<ResponseContent>) -> GenerateResponse {
        GenerateResponse {
            content,
            usage: Usage::default(),
            finish_reason: FinishReason::stop(),
            metadata: None,
            warnings: None,
        }
    }

    async fn agent_client_with_fake_provider(
        events: Vec<StreamEvent>,
        response: GenerateResponse,
    ) -> AgentClient {
        let registry = ProviderRegistry::new().register("fake", FakeProvider { events, response });
        let stakai = StakAIClient::with_registry(registry).expect("fake stakai client");
        let session_storage = Arc::new(
            LocalStorage::new(":memory:")
                .await
                .expect("in-memory local storage"),
        );

        AgentClient {
            stakai,
            stakpak_api: None,
            session_storage,
            hook_registry: Arc::new(stakpak_shared::hooks::HookRegistry::<AgentState>::default()),
            stakpak: None,
        }
    }

    async fn run_completion_with_stream_events(
        events: Vec<StreamEvent>,
    ) -> Result<Message, String> {
        let client = agent_client_with_fake_provider(events, generate_response(Vec::new())).await;
        let model = Model::custom("fake-model", "fake");
        let messages = vec![Message::new(Role::User, "hello")];
        let mut state = AgentState::new(model.clone(), messages.clone(), None, None);
        state.set_llm_input(Some(GenerateRequest::new(model, messages)));
        let mut ctx = HookContext::new(None, state);
        let (tx, _rx) = mpsc::channel(16);

        client.run_agent_completion(&mut ctx, Some(tx)).await
    }

    #[test]
    fn response_to_message_drops_reasoning_content() {
        let response = generate_response(vec![
            ResponseContent::Text {
                text: "visible".to_string(),
            },
            ResponseContent::Reasoning {
                reasoning: "hidden chain of thought".to_string(),
            },
        ]);

        let message = response_to_message(&response);

        assert_eq!(message.text().as_deref(), Some("visible"));
    }

    #[test]
    fn response_to_message_drops_reasoning_only_content() {
        let response = generate_response(vec![ResponseContent::Reasoning {
            reasoning: "hidden chain of thought".to_string(),
        }]);

        let message = response_to_message(&response);

        assert_eq!(message.text().as_deref(), Some(""));
    }

    #[tokio::test]
    async fn streaming_completion_treats_stream_error_event_as_failure() {
        let result = run_completion_with_stream_events(vec![StreamEvent::Error {
            message: "provider overloaded".to_string(),
        }])
        .await;

        assert!(result.is_err());
        assert!(
            result
                .err()
                .is_some_and(|err| err.contains("provider overloaded"))
        );
    }

    #[tokio::test]
    async fn streaming_completion_omits_empty_text_part_for_tool_only_messages() {
        let message = run_completion_with_stream_events(vec![StreamEvent::ToolCallEnd {
            id: "tc_1".to_string(),
            name: "test_tool".to_string(),
            arguments: serde_json::json!({"path": "README.md"}),
            metadata: None,
        }])
        .await
        .expect("stream completion");

        let MessageContent::Parts(parts) = message.content else {
            panic!("expected parts content");
        };

        assert_eq!(parts.len(), 1);
        assert!(matches!(parts[0], ContentPart::ToolCall { .. }));
    }
}
