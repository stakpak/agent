//! AgentProvider trait implementation for AgentClient
//!
//! Implements the unified provider interface with:
//! - Stakpak-first routing when API key is present
//! - Local fallback when Stakpak is unavailable
//! - Hook registry integration for lifecycle events

use crate::AgentProvider;
use crate::local::db;
use crate::models::*;
use crate::stakpak::{
    CheckpointState, CreateCheckpointRequest, CreateSessionRequest, ListCheckpointsQuery,
    ListSessionsQuery,
};
use async_trait::async_trait;
use futures_util::Stream;
use reqwest::header::HeaderMap;
use rmcp::model::Content;
use stakai::Model;
use stakpak_shared::hooks::{HookContext, LifecycleEvent};
use stakpak_shared::models::integrations::openai::{
    ChatCompletionChoice, ChatCompletionResponse, ChatCompletionStreamChoice,
    ChatCompletionStreamResponse, ChatMessage, FinishReason, MessageContent, Role, Tool,
};
use stakpak_shared::models::llm::{
    GenerationDelta, LLMInput, LLMMessage, LLMMessageContent, LLMStreamInput,
};
use std::pin::Pin;
use tokio::sync::mpsc;
use uuid::Uuid;

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
    // Agent Sessions
    // =========================================================================

    async fn list_agent_sessions(&self) -> Result<Vec<AgentSession>, String> {
        if let Some(api) = &self.stakpak_api {
            // Use Stakpak API
            let response = api.list_sessions(&ListSessionsQuery::default()).await?;
            Ok(response
                .sessions
                .into_iter()
                .map(|s| AgentSession {
                    id: s.id,
                    title: s.title,
                    agent_id: AgentID::PabloV1,
                    visibility: match s.visibility {
                        crate::stakpak::SessionVisibility::Public => AgentSessionVisibility::Public,
                        crate::stakpak::SessionVisibility::Private => {
                            AgentSessionVisibility::Private
                        }
                    },
                    checkpoints: vec![], // Summary doesn't include full checkpoints
                    created_at: s.created_at,
                    updated_at: s.updated_at,
                })
                .collect())
        } else {
            // Fallback to local DB
            db::list_sessions(&self.local_db).await
        }
    }

    async fn get_agent_session(&self, session_id: Uuid) -> Result<AgentSession, String> {
        if let Some(api) = &self.stakpak_api {
            let response = api.get_session(session_id).await?;
            let s = response.session;

            // Get checkpoints for this session
            let checkpoints_response = api
                .list_checkpoints(session_id, &ListCheckpointsQuery::default())
                .await?;

            Ok(AgentSession {
                id: s.id,
                title: s.title,
                agent_id: AgentID::PabloV1,
                visibility: match s.visibility {
                    crate::stakpak::SessionVisibility::Public => AgentSessionVisibility::Public,
                    crate::stakpak::SessionVisibility::Private => AgentSessionVisibility::Private,
                },
                checkpoints: checkpoints_response
                    .checkpoints
                    .into_iter()
                    .enumerate()
                    .map(|(i, c)| AgentCheckpointListItem {
                        id: c.id,
                        status: AgentStatus::Complete,
                        execution_depth: i,
                        parent: c.parent_id.map(|id| AgentParentCheckpoint { id }),
                        created_at: c.created_at,
                        updated_at: c.updated_at,
                    })
                    .collect(),
                created_at: s.created_at,
                updated_at: s.updated_at,
            })
        } else {
            db::get_session(&self.local_db, session_id).await
        }
    }

    async fn get_agent_session_stats(
        &self,
        _session_id: Uuid,
    ) -> Result<AgentSessionStats, String> {
        // TODO: Implement session stats via Stakpak API when available
        Ok(AgentSessionStats::default())
    }

    async fn get_agent_checkpoint(&self, checkpoint_id: Uuid) -> Result<RunAgentOutput, String> {
        if let Some(api) = &self.stakpak_api {
            let response = api.get_checkpoint(checkpoint_id).await?;
            let c = response.checkpoint;

            // Get session info
            let session_response = api.get_session(c.session_id).await?;
            let s = session_response.session;

            Ok(RunAgentOutput {
                checkpoint: AgentCheckpointListItem {
                    id: c.id,
                    status: AgentStatus::Complete,
                    execution_depth: 0, // We don't track depth in new API
                    parent: c.parent_id.map(|id| AgentParentCheckpoint { id }),
                    created_at: c.created_at,
                    updated_at: c.updated_at,
                },
                session: AgentSessionListItem {
                    id: s.id,
                    agent_id: AgentID::PabloV1,
                    visibility: match s.visibility {
                        crate::stakpak::SessionVisibility::Public => AgentSessionVisibility::Public,
                        crate::stakpak::SessionVisibility::Private => {
                            AgentSessionVisibility::Private
                        }
                    },
                    created_at: s.created_at,
                    updated_at: s.updated_at,
                },
                output: AgentOutput::PabloV1 {
                    messages: c.state.messages,
                    node_states: serde_json::json!({}),
                },
            })
        } else {
            db::get_checkpoint(&self.local_db, checkpoint_id).await
        }
    }

    async fn get_agent_session_latest_checkpoint(
        &self,
        session_id: Uuid,
    ) -> Result<RunAgentOutput, String> {
        if let Some(api) = &self.stakpak_api {
            // Get session with active checkpoint
            let session_response = api.get_session(session_id).await?;
            let s = session_response.session;

            if let Some(active_checkpoint) = s.active_checkpoint {
                Ok(RunAgentOutput {
                    checkpoint: AgentCheckpointListItem {
                        id: active_checkpoint.id,
                        status: AgentStatus::Complete,
                        execution_depth: 0,
                        parent: active_checkpoint
                            .parent_id
                            .map(|id| AgentParentCheckpoint { id }),
                        created_at: active_checkpoint.created_at,
                        updated_at: active_checkpoint.updated_at,
                    },
                    session: AgentSessionListItem {
                        id: s.id,
                        agent_id: AgentID::PabloV1,
                        visibility: match s.visibility {
                            crate::stakpak::SessionVisibility::Public => {
                                AgentSessionVisibility::Public
                            }
                            crate::stakpak::SessionVisibility::Private => {
                                AgentSessionVisibility::Private
                            }
                        },
                        created_at: s.created_at,
                        updated_at: s.updated_at,
                    },
                    output: AgentOutput::PabloV1 {
                        messages: active_checkpoint.state.messages,
                        node_states: serde_json::json!({}),
                    },
                })
            } else {
                Err("Session has no active checkpoint".to_string())
            }
        } else {
            db::get_latest_checkpoint(&self.local_db, session_id).await
        }
    }

    // =========================================================================
    // Chat Completion
    // =========================================================================

    async fn chat_completion(
        &self,
        model: Model,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
    ) -> Result<ChatCompletionResponse, String> {
        let mut ctx = HookContext::new(None, AgentState::new(model, messages, tools));

        // Execute before request hooks
        self.hook_registry
            .execute_hooks(&mut ctx, &LifecycleEvent::BeforeRequest)
            .await
            .map_err(|e| e.to_string())?
            .ok()?;

        // Initialize or resume session
        let current_checkpoint = self.initialize_session(&ctx.state.messages).await?;
        ctx.set_session_id(current_checkpoint.session.id);

        // Run completion
        let new_message = self.run_agent_completion(&mut ctx, None).await?;
        ctx.state.append_new_message(new_message.clone());

        // Save checkpoint
        let result = self
            .update_session(&current_checkpoint, ctx.state.messages.clone())
            .await?;
        let checkpoint_created_at = result.checkpoint.created_at.timestamp() as u64;
        ctx.set_new_checkpoint_id(result.checkpoint.id);

        // Execute after request hooks
        self.hook_registry
            .execute_hooks(&mut ctx, &LifecycleEvent::AfterRequest)
            .await
            .map_err(|e| e.to_string())?
            .ok()?;

        Ok(ChatCompletionResponse {
            id: ctx.new_checkpoint_id.unwrap().to_string(),
            object: "chat.completion".to_string(),
            created: checkpoint_created_at,
            model: ctx
                .state
                .llm_input
                .as_ref()
                .map(|llm_input| llm_input.model.id.clone())
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
            metadata: None,
        })
    }

    async fn chat_completion_stream(
        &self,
        model: Model,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        _headers: Option<HeaderMap>,
    ) -> Result<
        (
            Pin<
                Box<dyn Stream<Item = Result<ChatCompletionStreamResponse, ApiStreamError>> + Send>,
            >,
            Option<String>,
        ),
        String,
    > {
        let mut ctx = HookContext::new(None, AgentState::new(model, messages, tools));

        // Execute before request hooks
        self.hook_registry
            .execute_hooks(&mut ctx, &LifecycleEvent::BeforeRequest)
            .await
            .map_err(|e| e.to_string())?
            .ok()?;

        // Initialize session
        let current_checkpoint = self.initialize_session(&ctx.state.messages).await?;
        ctx.set_session_id(current_checkpoint.session.id);

        let (tx, mut rx) = mpsc::channel::<Result<StreamMessage, String>>(100);

        // Send initial checkpoint ID
        let _ = tx
            .send(Ok(StreamMessage::Delta(GenerationDelta::Content {
                content: format!(
                    "\n<checkpoint_id>{}</checkpoint_id>\n",
                    current_checkpoint.checkpoint.id
                ),
            })))
            .await;

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

                    let output = client
                        .update_session(&current_checkpoint, ctx_clone.state.messages.clone())
                        .await;

                    match output {
                        Err(e) => {
                            let _ = tx.send(Err(e)).await;
                        }
                        Ok(output) => {
                            ctx_clone.set_new_checkpoint_id(output.checkpoint.id);
                            let _ = tx.send(Ok(StreamMessage::Ctx(Box::new(ctx_clone)))).await;
                            let _ = tx
                                .send(Ok(StreamMessage::Delta(GenerationDelta::Content {
                                    content: format!(
                                        "\n<checkpoint_id>{}</checkpoint_id>\n",
                                        output.checkpoint.id
                                    ),
                                })))
                                .await;
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
                        }
                        StreamMessage::Delta(delta) => {
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
                                usage: ctx.state.llm_output.as_ref().map(|u| u.usage.clone()),
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
                mrkdwn_text: input.mrkdwn_text.clone(),
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
        // Return all known static models directly
        // No network calls - this should always be fast
        use stakai::providers::{anthropic, gemini, openai};
        
        let mut models = Vec::new();
        
        // When using Stakpak API, models are routed through Stakpak
        if self.has_stakpak() {
            // Add all models with stakpak routing prefix
            for model in anthropic::models::models() {
                models.push(stakai::Model {
                    id: format!("anthropic/{}", model.id),
                    name: model.name,
                    provider: "stakpak".into(),
                    reasoning: model.reasoning,
                    cost: model.cost,
                    limit: model.limit,
                });
            }
            for model in openai::models::models() {
                models.push(stakai::Model {
                    id: format!("openai/{}", model.id),
                    name: model.name,
                    provider: "stakpak".into(),
                    reasoning: model.reasoning,
                    cost: model.cost,
                    limit: model.limit,
                });
            }
            for model in gemini::models::models() {
                models.push(stakai::Model {
                    id: format!("google/{}", model.id),
                    name: model.name,
                    provider: "stakpak".into(),
                    reasoning: model.reasoning,
                    cost: model.cost,
                    limit: model.limit,
                });
            }
        } else {
            // Direct provider access - return models grouped by provider
            models.extend(anthropic::models::models());
            models.extend(openai::models::models());
            models.extend(gemini::models::models());
        }
        
        models
    }
}

// =============================================================================
// Helper Methods
// =============================================================================

const TITLE_GENERATOR_PROMPT: &str =
    include_str!("../local/prompts/session_title_generator.v1.txt");

impl AgentClient {
    /// Initialize or resume a session based on messages
    pub(crate) async fn initialize_session(
        &self,
        messages: &[ChatMessage],
    ) -> Result<RunAgentOutput, String> {
        if messages.is_empty() {
            return Err("At least one message is required".to_string());
        }

        // Check if we have an existing checkpoint ID in messages
        let checkpoint_id = ChatMessage::last_server_message(messages).and_then(|message| {
            message
                .content
                .as_ref()
                .and_then(|content| content.extract_checkpoint_id())
        });

        if let Some(checkpoint_id) = checkpoint_id {
            // Resume existing session
            return self.get_agent_checkpoint(checkpoint_id).await;
        }

        // Create new session
        // Generate title with fallback - don't fail session creation if title generation fails
        let title = match self.generate_session_title(messages).await {
            Ok(title) => title,
            Err(_) => {
                // Extract first few words from user message as fallback title
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
        };

        // Get current working directory
        let cwd = std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string());

        if let Some(api) = &self.stakpak_api {
            // Create session via Stakpak API (includes initial checkpoint)
            let mut session_request = CreateSessionRequest::new(
                title,
                CheckpointState {
                    messages: messages.to_vec(),
                },
            );
            if let Some(cwd) = cwd {
                session_request = session_request.with_cwd(cwd);
            }
            let response = api.create_session(&session_request).await?;

            Ok(RunAgentOutput {
                checkpoint: AgentCheckpointListItem {
                    id: response.checkpoint.id,
                    status: AgentStatus::Complete,
                    execution_depth: 0,
                    parent: response
                        .checkpoint
                        .parent_id
                        .map(|id| AgentParentCheckpoint { id }),
                    created_at: response.checkpoint.created_at,
                    updated_at: response.checkpoint.updated_at,
                },
                session: AgentSessionListItem {
                    id: response.session_id,
                    agent_id: AgentID::PabloV1,
                    visibility: AgentSessionVisibility::Private,
                    created_at: response.checkpoint.created_at,
                    updated_at: response.checkpoint.updated_at,
                },
                output: AgentOutput::PabloV1 {
                    messages: messages.to_vec(),
                    node_states: serde_json::json!({}),
                },
            })
        } else {
            // Create locally
            let now = chrono::Utc::now();
            let session_id = Uuid::new_v4();
            let session = AgentSession {
                id: session_id,
                title,
                agent_id: AgentID::PabloV1,
                visibility: AgentSessionVisibility::Private,
                created_at: now,
                updated_at: now,
                checkpoints: vec![],
            };
            db::create_session(&self.local_db, &session).await?;

            let checkpoint_id = Uuid::new_v4();
            let checkpoint = AgentCheckpointListItem {
                id: checkpoint_id,
                status: AgentStatus::Complete,
                execution_depth: 0,
                parent: None,
                created_at: now,
                updated_at: now,
            };
            let initial_state = AgentOutput::PabloV1 {
                messages: messages.to_vec(),
                node_states: serde_json::json!({}),
            };
            db::create_checkpoint(&self.local_db, session_id, &checkpoint, &initial_state).await?;

            db::get_checkpoint(&self.local_db, checkpoint_id).await
        }
    }

    /// Update session with new messages
    pub(crate) async fn update_session(
        &self,
        checkpoint_info: &RunAgentOutput,
        new_messages: Vec<ChatMessage>,
    ) -> Result<RunAgentOutput, String> {
        if let Some(api) = &self.stakpak_api {
            // Add checkpoint via Stakpak API
            let checkpoint_request = CreateCheckpointRequest::new(CheckpointState {
                messages: new_messages.clone(),
            })
            .with_parent(checkpoint_info.checkpoint.id);

            let response = api
                .create_checkpoint(checkpoint_info.session.id, &checkpoint_request)
                .await?;

            Ok(RunAgentOutput {
                checkpoint: AgentCheckpointListItem {
                    id: response.checkpoint.id,
                    status: AgentStatus::Complete,
                    execution_depth: checkpoint_info.checkpoint.execution_depth + 1,
                    parent: Some(AgentParentCheckpoint {
                        id: checkpoint_info.checkpoint.id,
                    }),
                    created_at: response.checkpoint.created_at,
                    updated_at: response.checkpoint.updated_at,
                },
                session: checkpoint_info.session.clone(),
                output: AgentOutput::PabloV1 {
                    messages: new_messages,
                    node_states: serde_json::json!({}),
                },
            })
        } else {
            // Create checkpoint locally
            let now = chrono::Utc::now();
            let complete_checkpoint = AgentCheckpointListItem {
                id: Uuid::new_v4(),
                status: AgentStatus::Complete,
                execution_depth: checkpoint_info.checkpoint.execution_depth + 1,
                parent: Some(AgentParentCheckpoint {
                    id: checkpoint_info.checkpoint.id,
                }),
                created_at: now,
                updated_at: now,
            };

            let new_state = AgentOutput::PabloV1 {
                messages: new_messages.clone(),
                node_states: serde_json::json!({}),
            };

            db::create_checkpoint(
                &self.local_db,
                checkpoint_info.session.id,
                &complete_checkpoint,
                &new_state,
            )
            .await?;

            Ok(RunAgentOutput {
                checkpoint: complete_checkpoint,
                session: checkpoint_info.session.clone(),
                output: new_state,
            })
        }
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
        // Use a default haiku model for title generation
        let model = Model::new(
            "claude-haiku-4-5-20250929",
            "Claude Haiku 4.5",
            "anthropic",
            false,
            None,
            stakai::ModelLimit::default(),
        );

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
