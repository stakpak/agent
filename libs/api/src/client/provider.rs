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
use futures_util::Stream;
use reqwest::header::HeaderMap;
use rmcp::model::Content;
use stakpak_shared::hooks::{HookContext, LifecycleEvent};
use stakpak_shared::models::integrations::anthropic::AnthropicModel;
use stakpak_shared::models::integrations::openai::{
    AgentModel, ChatCompletionChoice, ChatCompletionResponse, ChatCompletionStreamChoice,
    ChatCompletionStreamResponse, ChatMessage, FinishReason, MessageContent, Role, Tool,
};
use stakpak_shared::models::llm::{
    GenerationDelta, LLMInput, LLMMessage, LLMMessageContent, LLMModel, LLMStreamInput,
};
use stakpak_shared::models::stakai_adapter::get_stakai_model_string;
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
    Delta(GenerationDelta),
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
        model: AgentModel,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        session_id: Option<Uuid>,
    ) -> Result<ChatCompletionResponse, String> {
        let mut ctx = HookContext::new(session_id, AgentState::new(model, messages, tools));

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
            .save_checkpoint(&current_session, ctx.state.messages.clone())
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

        Ok(ChatCompletionResponse {
            id: ctx.new_checkpoint_id.unwrap().to_string(),
            object: "chat.completion".to_string(),
            created: checkpoint_created_at,
            model: ctx
                .state
                .llm_input
                .as_ref()
                .map(|llm_input| llm_input.model.clone().to_string())
                .unwrap_or_default(),
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: ctx.state.messages.last().cloned().unwrap(),
                logprobs: None,
                finish_reason: FinishReason::Stop,
            }],
            usage: ctx
                .state
                .llm_output
                .as_ref()
                .map(|u| u.usage.clone())
                .unwrap_or_default(),
            system_fingerprint: None,
            metadata: if meta.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(meta))
            },
        })
    }

    async fn chat_completion_stream(
        &self,
        model: AgentModel,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        _headers: Option<HeaderMap>,
        session_id: Option<Uuid>,
    ) -> Result<
        (
            Pin<
                Box<dyn Stream<Item = Result<ChatCompletionStreamResponse, ApiStreamError>> + Send>,
            >,
            Option<String>,
        ),
        String,
    > {
        let mut ctx = HookContext::new(session_id, AgentState::new(model, messages, tools));

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
                        .save_checkpoint(&current_session, ctx_clone.state.messages.clone())
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
                                yield Ok(ChatCompletionStreamResponse {
                                    id: ctx.request_id.to_string(),
                                    object: "chat.completion.chunk".to_string(),
                                    created: chrono::Utc::now().timestamp() as u64,
                                    model: String::new(),
                                    choices: vec![],
                                    usage: None,
                                    metadata: Some(serde_json::Value::Object(meta)),
                                });
                            }
                        }
                        StreamMessage::Delta(delta) => {
                            // Extract usage from Usage delta variant
                            let usage = if let GenerationDelta::Usage { usage } = &delta {
                                Some(usage.clone())
                            } else {
                                None
                            };

                            yield Ok(ChatCompletionStreamResponse {
                                id: ctx.request_id.to_string(),
                                object: "chat.completion.chunk".to_string(),
                                created: chrono::Utc::now().timestamp() as u64,
                                model: ctx.state.llm_input.as_ref().map(|llm_input| llm_input.model.clone().to_string()).unwrap_or_default(),
                                choices: vec![ChatCompletionStreamChoice {
                                    index: 0,
                                    delta: delta.into(),
                                    finish_reason: None,
                                }],
                                usage,
                                metadata: None,
                            })
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
}

// =============================================================================
// SessionStorage implementation (delegates to inner session_storage)
// =============================================================================

#[async_trait]
impl crate::storage::SessionStorage for super::AgentClient {
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

const TITLE_GENERATOR_PROMPT: &str =
    include_str!("../local/prompts/session_title_generator.v1.txt");

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

    fn fallback_session_title(messages: &[ChatMessage]) -> String {
        messages
            .iter()
            .find(|m| m.role == Role::User)
            .and_then(|m| m.content.as_ref())
            .map(|c| {
                let text = c.to_string();
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
        messages: Vec<ChatMessage>,
    ) -> Result<SessionInfo, String> {
        let checkpoint_request =
            StorageCreateCheckpointRequest::new(messages).with_parent(current.checkpoint_id);

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
    ) -> Result<ChatMessage, String> {
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
            let headers = input
                .headers
                .get_or_insert_with(std::collections::HashMap::new);
            headers.insert("X-Session-Id".to_string(), session_id.to_string());
        }

        let (response_message, usage) = if let Some(tx) = stream_channel_tx {
            // Streaming mode
            let (internal_tx, mut internal_rx) = mpsc::channel::<GenerationDelta>(100);
            let stream_input = LLMStreamInput {
                model: input.model,
                messages: input.messages,
                max_tokens: input.max_tokens,
                tools: input.tools,
                stream_channel_tx: internal_tx,
                provider_options: input.provider_options,
                headers: input.headers,
            };

            let stakai = self.stakai.clone();
            let chat_future = async move {
                stakai
                    .chat_stream(stream_input)
                    .await
                    .map_err(|e| e.to_string())
            };

            let receive_future = async move {
                while let Some(delta) = internal_rx.recv().await {
                    if tx.send(Ok(StreamMessage::Delta(delta))).await.is_err() {
                        break;
                    }
                }
            };

            let (chat_result, _) = tokio::join!(chat_future, receive_future);
            let response = chat_result?;
            (response.choices[0].message.clone(), response.usage)
        } else {
            // Non-streaming mode
            let response = self.stakai.chat(input).await.map_err(|e| e.to_string())?;
            (response.choices[0].message.clone(), response.usage)
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

        Ok(ChatMessage::from(llm_output))
    }

    /// Generate a title for a new session
    async fn generate_session_title(&self, messages: &[ChatMessage]) -> Result<String, String> {
        let llm_model = if let Some(eco_model) = &self.model_options.eco_model {
            eco_model.clone()
        } else {
            // Try to find a suitable model
            LLMModel::Anthropic(AnthropicModel::Claude45Haiku)
        };

        // If Stakpak is available, route through it
        let model = if self.has_stakpak() {
            // Get properly formatted model string with provider prefix (e.g., "anthropic/claude-haiku-4-5")
            let model_str = get_stakai_model_string(&llm_model);
            // Extract display name from the last segment for UI
            let display_name = model_str
                .rsplit('/')
                .next()
                .unwrap_or(&model_str)
                .to_string();
            LLMModel::Custom {
                provider: "stakpak".to_string(),
                model: model_str,
                name: Some(display_name),
            }
        } else {
            llm_model
        };

        let llm_messages = vec![
            LLMMessage {
                role: Role::System.to_string(),
                content: LLMMessageContent::String(TITLE_GENERATOR_PROMPT.to_string()),
            },
            LLMMessage {
                role: Role::User.to_string(),
                content: LLMMessageContent::String(
                    messages
                        .iter()
                        .map(|msg| {
                            msg.content
                                .as_ref()
                                .unwrap_or(&MessageContent::String("".to_string()))
                                .to_string()
                        })
                        .collect(),
                ),
            },
        ];

        let input = LLMInput {
            model,
            messages: llm_messages,
            max_tokens: 100,
            tools: None,
            provider_options: None,
            headers: None,
        };

        let response = self.stakai.chat(input).await.map_err(|e| e.to_string())?;

        Ok(response.choices[0].message.content.to_string())
    }
}
