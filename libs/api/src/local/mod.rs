use crate::local::context_manager::{ContextManager, SimpleContextManager};
use crate::{AgentProvider, ApiStreamError, GetMyAccountResponse};
use crate::{ListRuleBook, models::*};
use async_trait::async_trait;
use futures_util::Stream;
use libsql::{Builder, Connection};
use reqwest::header::HeaderMap;
use rmcp::model::Content;
use stakpak_shared::hooks::{HookContext, HookRegistry, LifecycleEvent};
use stakpak_shared::models::integrations::anthropic::{AnthropicConfig, AnthropicModel};
use stakpak_shared::models::integrations::openai::{
    AgentModel, ChatCompletionChoice, ChatCompletionResponse, ChatCompletionStreamChoice,
    ChatCompletionStreamResponse, ChatMessage, FinishReason, MessageContent, OpenAIConfig,
    OpenAIModel, Role, Tool,
};
use stakpak_shared::models::llm::{
    GenerationDelta, LLMInput, LLMMessage, LLMMessageContent, LLMModel, LLMProviderConfig,
    LLMStreamInput, chat, chat_stream,
};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

mod context_manager;
mod db;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub struct LocalClient {
    pub db: Connection,
    pub anthropic_config: Option<AnthropicConfig>,
    pub openai_config: Option<OpenAIConfig>,
    pub smart_model: LLMModel,
    pub eco_model: LLMModel,
    pub recovery_model: LLMModel,
    pub hook_registry: Option<Arc<HookRegistry<AgentState>>>,
}

pub struct LocalClientConfig {
    pub store_path: Option<String>,
    pub anthropic_config: Option<AnthropicConfig>,
    pub openai_config: Option<OpenAIConfig>,
    pub smart_model: Option<String>,
    pub eco_model: Option<String>,
    pub recovery_model: Option<String>,
    pub hook_registry: Option<Arc<HookRegistry<AgentState>>>,
}

#[derive(Debug)]
enum StreamMessage {
    Delta(GenerationDelta),
    Ctx(Box<HookContext<AgentState>>),
}

const DEFAULT_STORE_PATH: &str = ".stakpak/data/local.db";
const SYSTEM_PROMPT: &str = include_str!("./prompts/main.txt");
const TITLE_GENERATOR_PROMPT: &str = include_str!("./prompts/title_generator.txt");

impl LocalClient {
    pub async fn new(config: LocalClientConfig) -> Result<Self, String> {
        let default_store_path = std::env::home_dir()
            .unwrap_or_default()
            .join(DEFAULT_STORE_PATH);

        if let Some(parent) = default_store_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create database directory: {}", e))?;
        }

        let db = Builder::new_local(default_store_path.display().to_string())
            .build()
            .await
            .map_err(|e| e.to_string())?;

        let conn = db.connect().map_err(|e| e.to_string())?;

        // Initialize database schema
        db::init_schema(&conn).await?;

        Ok(Self {
            db: conn,
            anthropic_config: config.anthropic_config,
            openai_config: config.openai_config,
            smart_model: config
                .smart_model
                .map(LLMModel::from)
                .unwrap_or(LLMModel::Anthropic(AnthropicModel::Claude45Sonnet)),
            eco_model: config
                .eco_model
                .map(LLMModel::from)
                .unwrap_or(LLMModel::Anthropic(AnthropicModel::Claude45Haiku)),
            recovery_model: config
                .recovery_model
                .map(LLMModel::from)
                .unwrap_or(LLMModel::OpenAI(OpenAIModel::GPT5)),
            hook_registry: config.hook_registry,
        })
    }
}

#[async_trait]
impl AgentProvider for LocalClient {
    async fn get_my_account(&self) -> Result<GetMyAccountResponse, String> {
        Ok(GetMyAccountResponse {
            username: "local".to_string(),
            id: "local".to_string(),
            first_name: "local".to_string(),
            last_name: "local".to_string(),
        })
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
        // TODO: Implement create rulebook
        Err("Local provider does not support rulebooks yet".to_string())
    }

    async fn delete_rulebook(&self, _uri: &str) -> Result<(), String> {
        // TODO: Implement delete rulebook
        Err("Local provider does not support rulebooks yet".to_string())
    }

    async fn list_agent_sessions(&self) -> Result<Vec<AgentSession>, String> {
        db::list_sessions(&self.db).await
    }

    async fn get_agent_session(&self, session_id: Uuid) -> Result<AgentSession, String> {
        db::get_session(&self.db, session_id).await
    }

    async fn get_agent_session_stats(
        &self,
        _session_id: Uuid,
    ) -> Result<AgentSessionStats, String> {
        // TODO: Implement get agent session stats
        Ok(AgentSessionStats::default())
    }

    async fn get_agent_checkpoint(&self, checkpoint_id: Uuid) -> Result<RunAgentOutput, String> {
        db::get_checkpoint(&self.db, checkpoint_id).await
    }

    async fn get_agent_session_latest_checkpoint(
        &self,
        session_id: Uuid,
    ) -> Result<RunAgentOutput, String> {
        db::get_latest_checkpoint(&self.db, session_id).await
    }

    async fn chat_completion(
        &self,
        model: AgentModel,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
    ) -> Result<ChatCompletionResponse, String> {
        let mut ctx = HookContext::new(None, AgentState::new(model, messages, tools));

        if let Some(hook_registry) = &self.hook_registry {
            hook_registry
                .execute_hooks(&mut ctx, &LifecycleEvent::BeforeRequest)
                .await
                .map_err(|e| e.to_string())?
                .ok()?;
        }

        let current_checkpoint = self.initialize_session(&ctx.state.messages).await?;
        ctx.set_session_id(current_checkpoint.session.id);

        let new_message = self.run_agent_completion(&mut ctx, None).await?;
        ctx.state.append_new_message(new_message.clone());

        let result = self
            .update_session(&current_checkpoint, ctx.state.messages.clone())
            .await?;
        let checkpoint_created_at = result.checkpoint.created_at.timestamp() as u64;
        ctx.set_new_checkpoint_id(result.checkpoint.id);

        if let Some(hook_registry) = &self.hook_registry {
            hook_registry
                .execute_hooks(&mut ctx, &LifecycleEvent::AfterRequest)
                .await
                .map_err(|e| e.to_string())?
                .ok()?;
        }

        Ok(ChatCompletionResponse {
            id: ctx.new_checkpoint_id.unwrap().to_string(),
            object: "chat.completion".to_string(),
            created: checkpoint_created_at,
            model: self
                .get_inference_model(ctx.state.agent_model.clone())
                .to_string(),
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
        model: AgentModel,
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

        if let Some(hook_registry) = &self.hook_registry {
            hook_registry
                .execute_hooks(&mut ctx, &LifecycleEvent::BeforeRequest)
                .await
                .map_err(|e| e.to_string())?
                .ok()?;
        }

        let current_checkpoint = self.initialize_session(&ctx.state.messages).await?;
        ctx.set_session_id(current_checkpoint.session.id);

        let (tx, mut rx) = mpsc::channel::<Result<StreamMessage, String>>(100);

        let _ = tx
            .send(Ok(StreamMessage::Delta(GenerationDelta::Content {
                content: format!(
                    "\n<checkpoint_id>{}</checkpoint_id>\n",
                    current_checkpoint.checkpoint.id
                ),
            })))
            .await;

        let client = self.clone();
        let self_clone = self.clone();
        let mut ctx_clone = ctx.clone();
        tokio::spawn(async move {
            let result = client
                .run_agent_completion(&mut ctx_clone, Some(tx.clone()))
                .await;

            match result {
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                }
                Ok(new_message) => {
                    ctx_clone.state.append_new_message(new_message.clone());
                    let _ = tx
                        .send(Ok(StreamMessage::Ctx(Box::new(ctx_clone.clone()))))
                        .await;

                    let output = self_clone
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
        let model_name = self
            .get_inference_model(ctx.state.agent_model.clone())
            .to_string();
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
                                    model: model_name.to_owned(),
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

            if let Some(hook_registry) = hook_registry {
                hook_registry
                    .execute_hooks(&mut ctx, &LifecycleEvent::AfterRequest)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok()?;
            }
        };

        Ok((Box::pin(stream), None))
    }

    async fn cancel_stream(&self, _request_id: String) -> Result<(), String> {
        Ok(())
    }

    async fn search_docs(&self, _input: &SearchDocsRequest) -> Result<Vec<Content>, String> {
        // TODO: Implement search docs
        Ok(Vec::new())
    }

    async fn search_memory(&self, _input: &SearchMemoryRequest) -> Result<Vec<Content>, String> {
        // TODO: Implement search memory
        Ok(Vec::new())
    }

    async fn slack_read_messages(
        &self,
        _input: &SlackReadMessagesRequest,
    ) -> Result<Vec<Content>, String> {
        // TODO: Implement slack read messages
        Ok(Vec::new())
    }

    async fn slack_read_replies(
        &self,
        _input: &SlackReadRepliesRequest,
    ) -> Result<Vec<Content>, String> {
        // TODO: Implement slack read replies
        Ok(Vec::new())
    }

    async fn slack_send_message(
        &self,
        _input: &SlackSendMessageRequest,
    ) -> Result<Vec<Content>, String> {
        // TODO: Implement slack send message
        Ok(Vec::new())
    }

    async fn memorize_session(&self, _checkpoint_id: Uuid) -> Result<(), String> {
        // TODO: Implement memorize session
        Ok(())
    }
}

impl LocalClient {
    fn get_inference_model(&self, model: AgentModel) -> LLMModel {
        match model {
            AgentModel::Smart => self.smart_model.clone(),
            AgentModel::Eco => self.eco_model.clone(),
            AgentModel::Recovery => self.recovery_model.clone(),
        }
    }

    fn get_inference_config(&self) -> LLMProviderConfig {
        LLMProviderConfig {
            anthropic_config: self.anthropic_config.clone(),
            openai_config: self.openai_config.clone(),
        }
    }

    async fn run_agent_completion(
        &self,
        ctx: &mut HookContext<AgentState>,
        stream_channel_tx: Option<mpsc::Sender<Result<StreamMessage, String>>>,
    ) -> Result<ChatMessage, String> {
        if let Some(hook_registry) = &self.hook_registry {
            hook_registry
                .execute_hooks(ctx, &LifecycleEvent::BeforeInference)
                .await
                .map_err(|e| e.to_string())?
                .ok()?;
        }

        let input = if let Some(llm_input) = ctx.state.llm_input.clone() {
            llm_input
        } else {
            let inference_model = self.get_inference_model(ctx.state.agent_model.clone());
            let context_manager = SimpleContextManager;
            let mut llm_messages = vec![LLMMessage {
                role: Role::System.to_string(),
                content: LLMMessageContent::String(SYSTEM_PROMPT.into()),
            }];
            llm_messages.extend(context_manager.reduce_context(ctx.state.messages.clone()));

            let llm_tools = ctx
                .state
                .tools
                .clone()
                .map(|t| t.into_iter().map(Into::into).collect());

            LLMInput {
                model: inference_model,
                messages: llm_messages,
                max_tokens: 16000,
                tools: llm_tools,
            }
        };

        let inference_config = self.get_inference_config();

        let (response_message, usage) = if let Some(tx) = stream_channel_tx {
            let (internal_tx, mut internal_rx) = mpsc::channel::<GenerationDelta>(100);
            let input = LLMStreamInput {
                model: input.model,
                messages: input.messages,
                max_tokens: input.max_tokens,
                tools: input.tools,
                stream_channel_tx: internal_tx,
            };

            let chat_future = async move {
                chat_stream(&inference_config, input)
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
            let response = chat(&inference_config, input)
                .await
                .map_err(|e| e.to_string())?;
            (response.choices[0].message.clone(), response.usage)
        };

        ctx.state.set_llm_output(response_message, usage);

        if let Some(hook_registry) = &self.hook_registry {
            hook_registry
                .execute_hooks(ctx, &LifecycleEvent::AfterInference)
                .await
                .map_err(|e| e.to_string())?
                .ok()?;
        }

        let llm_output = ctx
            .state
            .llm_output
            .as_ref()
            .ok_or_else(|| "LLM output is missing from state".to_string())?;

        Ok(ChatMessage::from(llm_output))
    }

    async fn initialize_session(&self, messages: &[ChatMessage]) -> Result<RunAgentOutput, String> {
        // 1. Validate input
        if messages.is_empty() {
            return Err("At least one message is required".to_string());
        }

        // 2. Extract session/checkpoint ID or create new session
        let checkpoint_id = ChatMessage::last_server_message(messages).and_then(|message| {
            message
                .content
                .as_ref()
                .and_then(|content| content.extract_checkpoint_id())
        });

        let current_checkpoint = if let Some(checkpoint_id) = checkpoint_id {
            db::get_checkpoint(&self.db, checkpoint_id).await?
        } else {
            let title = self.generate_session_title(messages).await?;

            // Create new session
            let session_id = Uuid::new_v4();
            let now = chrono::Utc::now();
            let session = AgentSession {
                id: session_id,
                title,
                agent_id: AgentID::PabloV1,
                visibility: AgentSessionVisibility::Private,
                created_at: now,
                updated_at: now,
                checkpoints: vec![],
            };
            db::create_session(&self.db, &session).await?;

            // Create initial checkpoint (root)
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
            db::create_checkpoint(&self.db, session_id, &checkpoint, &initial_state).await?;

            db::get_checkpoint(&self.db, checkpoint_id).await?
        };

        Ok(current_checkpoint)
    }

    async fn update_session(
        &self,
        checkpoint_info: &RunAgentOutput,
        new_messages: Vec<ChatMessage>,
    ) -> Result<RunAgentOutput, String> {
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

        let mut new_state = checkpoint_info.output.clone();
        new_state.set_messages(new_messages);

        db::create_checkpoint(
            &self.db,
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

    async fn generate_session_title(&self, messages: &[ChatMessage]) -> Result<String, String> {
        let inference_config = self.get_inference_config();
        // Use eco model for title generation
        let inference_model = self.eco_model.clone();

        let messages = vec![
            LLMMessage {
                role: "system".to_string(),
                content: LLMMessageContent::String(TITLE_GENERATOR_PROMPT.into()),
            },
            LLMMessage {
                role: "user".to_string(),
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
            model: inference_model,
            messages,
            max_tokens: 100,
            tools: None,
        };

        let response = chat(&inference_config, input)
            .await
            .map_err(|e| e.to_string())?;

        Ok(response.choices[0].message.content.to_string())
    }
}
