use crate::local::integrations::anthropic::{AnthropicConfig, AnthropicModel};
use crate::local::integrations::models::{
    generation::GenerationDelta,
    llm::{LLMMessage, LLMMessageContent, LLMMessageTypedContent, LLMTool},
};
use crate::local::integrations::openai::{OpenAIConfig, OpenAIModel};
use crate::local::integrations::{InferenceConfig, InferenceModel};
use crate::{AgentProvider, ApiStreamError, GetMyAccountResponse};
use crate::{ListRuleBook, models::*};
use async_trait::async_trait;
use futures_util::Stream;
use libsql::{Builder, Connection};
use reqwest::header::HeaderMap;
use rmcp::model::Content;
use stakpak_shared::models::integrations::openai::{
    AgentModel, ChatCompletionChoice, ChatCompletionResponse, ChatCompletionStreamChoice,
    ChatCompletionStreamResponse, ChatMessage, ChatMessageDelta, FinishReason, FunctionCall,
    FunctionCallDelta, MessageContent, Role, Tool, ToolCall, ToolCallDelta, Usage,
};
use stakpak_shared::models::llm::LLMTokenUsage;
use std::pin::Pin;
use uuid::Uuid;

mod context_manager;
mod db;
pub mod integrations;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub struct LocalClient {
    pub db: Connection,
    pub anthropic_config: Option<AnthropicConfig>,
    pub openai_config: Option<OpenAIConfig>,
    pub smart_model: InferenceModel,
    pub eco_model: InferenceModel,
    pub recovery_model: InferenceModel,
}

pub struct LocalClientConfig {
    pub store_path: Option<String>,
    pub anthropic_config: Option<AnthropicConfig>,
    pub openai_config: Option<OpenAIConfig>,
    pub smart_model: Option<String>,
    pub eco_model: Option<String>,
    pub recovery_model: Option<String>,
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

        let db = Builder::new_local(&default_store_path.display().to_string())
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
                .map(InferenceModel::from)
                .unwrap_or(InferenceModel::Anthropic(AnthropicModel::Claude45Sonnet)),
            eco_model: config
                .eco_model
                .map(InferenceModel::from)
                .unwrap_or(InferenceModel::Anthropic(AnthropicModel::Claude45Haiku)),
            recovery_model: config
                .recovery_model
                .map(InferenceModel::from)
                .unwrap_or(InferenceModel::OpenAI(OpenAIModel::GPT5)),
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
        let current_checkpoint = self.initialize_session(&messages).await?;

        let (new_message, usage) = self
            .run_agent_completion(model.clone(), messages.clone(), tools, None)
            .await?;

        let mut new_messages = messages;
        new_messages.push(new_message);

        let result = self
            .update_session(&current_checkpoint, new_messages)
            .await?;

        Ok(ChatCompletionResponse {
            id: result.checkpoint.id.to_string(),
            object: "chat.completion".to_string(),
            created: result.checkpoint.created_at.timestamp() as u64,
            model: self.get_inference_model(model).to_string(),
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: match result.output {
                    AgentOutput::PabloV1 { messages, .. } => messages.last().cloned().unwrap(),
                },
                logprobs: None,
                finish_reason: FinishReason::Stop,
            }],
            usage: usage
                .map(|u| Usage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                    prompt_tokens_details: None,
                })
                .unwrap_or(Usage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                    prompt_tokens_details: None,
                }),
            system_fingerprint: None,
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
        let current_checkpoint = self.initialize_session(&messages).await?;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<GenerationDelta, String>>(100);

        let client = self.clone();
        let model_clone = model.clone();
        let messages_clone = messages.clone();
        let tools_clone = tools.clone();

        // Send initial checkpoint ID
        let _ = tx
            .send(Ok(GenerationDelta::Content {
                content: format!(
                    "\n<checkpoint_id>{}</checkpoint_id>\n",
                    current_checkpoint.checkpoint.id
                ),
            }))
            .await;

        let self_clone = self.clone();
        tokio::spawn(async move {
            let result = client
                .run_agent_completion(model_clone, messages_clone, tools_clone, Some(tx.clone()))
                .await;

            match result {
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                }
                Ok((new_message, _usage)) => {
                    let mut new_messages = messages.clone();
                    new_messages.push(new_message);

                    let output = self_clone
                        .update_session(&current_checkpoint, new_messages)
                        .await;

                    match output {
                        Err(e) => {
                            let _ = tx.send(Err(e)).await;
                        }
                        Ok(output) => {
                            let _ = tx
                                .send(Ok(GenerationDelta::Content {
                                    content: format!(
                                        "\n<checkpoint_id>{}</checkpoint_id>\n",
                                        output.checkpoint.id
                                    ),
                                }))
                                .await;
                        }
                    }
                }
            }
        });

        let model_name = self.get_inference_model(model.clone()).to_string();
        let stream = async_stream::stream! {
            let completion_id = Uuid::new_v4().to_string();
            while let Some(delta_result) = rx.recv().await {
                match delta_result {
                    Ok(delta) => {
                        let usage = if let GenerationDelta::Usage { usage } = &delta {
                            Some(Usage {
                                prompt_tokens: usage.prompt_tokens,
                                completion_tokens: usage.completion_tokens,
                                total_tokens: usage.total_tokens,
                                prompt_tokens_details: None,
                            })
                        } else {
                            None
                        };

                        yield Ok(ChatCompletionStreamResponse {
                            id: completion_id.clone(),
                            object: "chat.completion.chunk".to_string(),
                            created: chrono::Utc::now().timestamp() as u64,
                            model: model_name.to_owned(),
                            choices: vec![ChatCompletionStreamChoice {
                                index: 0,
                                delta: delta.into(),
                                finish_reason: None,
                            }],
                            usage,
                        })
                    }
                    Err(e) => yield Err(ApiStreamError::Unknown(e)),
                }
            }
        };

        Ok((Box::pin(stream), None))
    }

    async fn cancel_stream(&self, _request_id: String) -> Result<(), String> {
        Ok(())
    }

    async fn build_code_index(
        &self,
        _input: &BuildCodeIndexInput,
    ) -> Result<BuildCodeIndexOutput, String> {
        // TODO: Implement build code index
        Ok(BuildCodeIndexOutput {
            blocks: Vec::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
        })
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
    fn get_inference_model(&self, model: AgentModel) -> InferenceModel {
        match model {
            AgentModel::Smart => self.smart_model.clone(),
            AgentModel::Eco => self.eco_model.clone(),
        }
    }

    fn get_inference_config(&self) -> InferenceConfig {
        InferenceConfig {
            anthropic_config: self.anthropic_config.clone(),
            openai_config: self.openai_config.clone(),
        }
    }

    async fn run_agent_completion(
        &self,
        model: AgentModel,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        stream_channel_tx: Option<tokio::sync::mpsc::Sender<Result<GenerationDelta, String>>>,
    ) -> Result<(ChatMessage, Option<LLMTokenUsage>), String> {
        let inference_config = self.get_inference_config();
        let inference_model = self.get_inference_model(model);

        let llm_messages = vec![
            LLMMessage {
                role: Role::System.to_string(),
                content: LLMMessageContent::String(SYSTEM_PROMPT.into()),
            },
            LLMMessage {
                role: Role::User.to_string(),
                content: LLMMessageContent::String(context_manager::project_messages(messages)),
            },
        ];

        let llm_tools = tools.map(|t| t.into_iter().map(Into::into).collect());

        let (response_message, usage) = if let Some(tx) = stream_channel_tx {
            let (internal_tx, mut internal_rx) = tokio::sync::mpsc::channel::<GenerationDelta>(100);

            let input = crate::local::integrations::InferenceStreamInput {
                model: inference_model,
                messages: llm_messages,
                max_tokens: 16000,
                stream_channel_tx: internal_tx,
                tools: llm_tools,
            };

            let chat_future = async move {
                integrations::chat_stream(&inference_config, input)
                    .await
                    .map_err(|e| e.to_string())
            };

            let receive_future = async move {
                while let Some(delta) = internal_rx.recv().await {
                    if tx.send(Ok(delta)).await.is_err() {
                        break;
                    }
                }
            };

            let (chat_result, _) = tokio::join!(chat_future, receive_future);
            let response = chat_result?;
            (response.choices[0].message.clone(), response.usage)
        } else {
            let input = crate::local::integrations::InferenceInput {
                model: inference_model,
                messages: llm_messages,
                max_tokens: 16000,
                tools: llm_tools,
            };
            let response = integrations::chat(&inference_config, input)
                .await
                .map_err(|e| e.to_string())?;
            (response.choices[0].message.clone(), response.usage)
        };

        let message_content_string = match &response_message.content {
            LLMMessageContent::String(s) => s.clone(),
            LLMMessageContent::List(l) => l
                .iter()
                .map(|c| match c {
                    LLMMessageTypedContent::Text { text } => text.clone(),
                    LLMMessageTypedContent::ToolCall { .. } => String::new(),
                    LLMMessageTypedContent::ToolResult { content, .. } => content.clone(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        };
        let tool_calls = if let LLMMessageContent::List(items) = &response_message.content {
            let calls: Vec<ToolCall> = items
                .iter()
                .filter_map(|item| {
                    if let LLMMessageTypedContent::ToolCall { id, name, args } = item {
                        Some(ToolCall {
                            id: id.clone(),
                            r#type: "function".to_string(),
                            function: FunctionCall {
                                name: name.clone(),
                                arguments: args.to_string(),
                            },
                        })
                    } else {
                        None
                    }
                })
                .collect();

            if calls.is_empty() { None } else { Some(calls) }
        } else {
            None
        };

        Ok((
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(message_content_string)),
                name: None,
                tool_calls,
                tool_call_id: None,
            },
            usage,
        ))
    }

    async fn initialize_session(&self, messages: &[ChatMessage]) -> Result<RunAgentOutput, String> {
        // 1. Validate input
        if messages.is_empty() {
            return Err("At least one message is required".to_string());
        }

        // 2. Extract session/checkpoint ID or create new session
        let checkpoint_id = ChatMessage::last_server_message(&messages).and_then(|message| {
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
        // 6. Create "Complete" checkpoint
        let complete_checkpoint_id = Uuid::new_v4();
        let complete_checkpoint = AgentCheckpointListItem {
            id: complete_checkpoint_id,
            status: AgentStatus::Complete,
            execution_depth: checkpoint_info.checkpoint.execution_depth + 1,
            parent: Some(AgentParentCheckpoint {
                id: checkpoint_info.checkpoint.id,
            }),
            created_at: now,
            updated_at: now,
        };

        // Update state with the new assistant message
        let mut new_state = checkpoint_info.output.clone();
        new_state.set_messages(new_messages);

        db::create_checkpoint(
            &self.db,
            checkpoint_info.session.id,
            &complete_checkpoint,
            &new_state,
        )
        .await?;

        // Return the new checkpoint info
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

        let input = crate::local::integrations::InferenceInput {
            model: inference_model,
            messages,
            max_tokens: 100,
            tools: None,
        };

        let response = integrations::chat(&inference_config, input)
            .await
            .map_err(|e| e.to_string())?;

        Ok(response.choices[0].message.content.to_string())
    }
}

impl From<Tool> for LLMTool {
    fn from(tool: Tool) -> Self {
        LLMTool {
            name: tool.function.name,
            description: tool.function.description.unwrap_or_default(),
            input_schema: tool.function.parameters,
        }
    }
}

impl From<integrations::models::generation::GenerationDelta> for ChatMessageDelta {
    fn from(delta: integrations::models::generation::GenerationDelta) -> Self {
        match delta {
            integrations::models::generation::GenerationDelta::Content { content } => {
                ChatMessageDelta {
                    role: Some(Role::Assistant),
                    content: Some(content),
                    tool_calls: None,
                }
            }
            integrations::models::generation::GenerationDelta::Thinking { thinking: _ } => {
                ChatMessageDelta {
                    role: Some(Role::Assistant),
                    content: None,
                    tool_calls: None,
                }
            }
            integrations::models::generation::GenerationDelta::ToolUse { tool_use } => {
                ChatMessageDelta {
                    role: Some(Role::Assistant),
                    content: None,
                    tool_calls: Some(vec![ToolCallDelta {
                        index: tool_use.index,
                        id: tool_use.id,
                        r#type: Some("function".to_string()),
                        function: Some(FunctionCallDelta {
                            name: tool_use.name,
                            arguments: tool_use.input,
                        }),
                    }]),
                }
            }
            _ => ChatMessageDelta {
                role: Some(Role::Assistant),
                content: None,
                tool_calls: None,
            },
        }
    }
}
