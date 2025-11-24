use crate::local::integrations::anthropic::{
    Anthropic, AnthropicConfig, AnthropicInput, AnthropicModel,
};
use crate::local::integrations::models::{
    generation::GenerationDelta,
    llm::{LLMMessage, LLMMessageContent, LLMMessageTypedContent, LLMTool},
};
use crate::local::integrations::openai::{OpenAIConfig, OpenAIModel};
use crate::{AgentProvider, GetMyAccountResponse};
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
use std::collections::BTreeMap;
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
}

pub struct LocalClientConfig {
    pub store_path: Option<String>,
    pub anthropic_config: Option<AnthropicConfig>,
    pub openai_config: Option<OpenAIConfig>,
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
        })
    }
}

impl From<AgentModel> for AnthropicModel {
    fn from(model: AgentModel) -> Self {
        match model {
            AgentModel::Smart => AnthropicModel::Claude45Sonnet,
            AgentModel::Eco => AnthropicModel::Claude45Haiku,
        }
    }
}

impl From<AgentModel> for OpenAIModel {
    fn from(model: AgentModel) -> Self {
        match model {
            AgentModel::Smart => OpenAIModel::GPT5,
            AgentModel::Eco => OpenAIModel::GPT5Mini,
        }
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

        let new_message = self
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
            model: AnthropicModel::from(model).to_string(),
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: match result.output {
                    AgentOutput::PabloV1 { messages, .. } => messages.last().cloned().unwrap(),
                },
                logprobs: None,
                finish_reason: FinishReason::Stop,
            }],
            usage: Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                prompt_tokens_details: None,
            },
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
                Ok(new_message) => {
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

        let stream = async_stream::stream! {
            let completion_id = Uuid::new_v4().to_string();
            while let Some(delta_result) = rx.recv().await {
                match delta_result {
                    Ok(delta) => yield Ok(ChatCompletionStreamResponse {
                        id: completion_id.clone(),
                        object: "chat.completion.chunk".to_string(),
                        created: chrono::Utc::now().timestamp() as u64,
                        model: AnthropicModel::from(model.clone()).to_string(),
                        choices: vec![ChatCompletionStreamChoice {
                            index: 0,
                            delta: delta.into(),
                            finish_reason: None,
                        }],
                        usage: None,
                    }),
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
    async fn run_agent_completion(
        &self,
        model: AgentModel,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        stream_channel_tx: Option<tokio::sync::mpsc::Sender<Result<GenerationDelta, String>>>,
    ) -> Result<ChatMessage, String> {
        let (response_content, tool_calls) = if let Some(config) = &self.anthropic_config {
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

            let model_anthropic: AnthropicModel = model.into();

            if let Some(tx) = stream_channel_tx {
                let input = AnthropicInput {
                    model: model_anthropic.clone(),
                    messages: llm_messages.clone(),
                    grammar: None,
                    max_tokens: 16000,
                    stop_sequences: None,
                    tools: tools
                        .clone()
                        .map(|t| t.into_iter().map(Into::into).collect()),
                    thinking: Default::default(),
                };

                let (internal_tx, mut internal_rx) =
                    tokio::sync::mpsc::channel::<GenerationDelta>(100);
                let config_clone = config.clone();

                let chat_future = async move {
                    Anthropic::chat_stream(&config_clone, internal_tx, input)
                        .await
                        .map_err(|e| e.to_string())
                };

                let receive_future = async move {
                    let mut full_content = String::new();
                    let mut tool_calls_acc: BTreeMap<
                        usize,
                        (Option<String>, Option<String>, String),
                    > = BTreeMap::new();

                    while let Some(delta) = internal_rx.recv().await {
                        if tx.send(Ok(delta.clone())).await.is_err() {
                            break;
                        }
                        match delta {
                            GenerationDelta::Content { content } => full_content.push_str(&content),
                            GenerationDelta::ToolUse { tool_use } => {
                                let entry = tool_calls_acc.entry(tool_use.index).or_insert((
                                    None,
                                    None,
                                    String::new(),
                                ));
                                if let Some(id) = tool_use.id {
                                    entry.0 = Some(id);
                                }
                                if let Some(name) = tool_use.name {
                                    entry.1 = Some(name);
                                }
                                if let Some(input) = tool_use.input {
                                    entry.2.push_str(&input);
                                }
                            }
                            _ => {}
                        }
                    }
                    (full_content, tool_calls_acc)
                };

                let (chat_result, (full_content, tool_calls_acc)) =
                    tokio::join!(chat_future, receive_future);

                chat_result?;

                let tool_calls = if tool_calls_acc.is_empty() {
                    None
                } else {
                    Some(
                        tool_calls_acc
                            .into_values()
                            .map(|(id, name, input)| ToolCall {
                                id: id.unwrap_or_default(),
                                r#type: "function".to_string(),
                                function: FunctionCall {
                                    name: name.unwrap_or_default(),
                                    arguments: input,
                                },
                            })
                            .collect(),
                    )
                };
                (full_content, tool_calls)
            } else {
                let input = AnthropicInput {
                    model: model_anthropic.clone(),
                    messages: llm_messages,
                    grammar: None,
                    max_tokens: 16000,
                    stop_sequences: None,
                    tools: tools
                        .clone()
                        .map(|t| t.into_iter().map(Into::into).collect()),
                    thinking: Default::default(),
                };
                let response = Anthropic::chat(config, input)
                    .await
                    .map_err(|e| e.to_string())?;

                let message_content = response.choices[0].message.content.clone();
                let text_content = message_content.to_string();

                let tool_calls = if let LLMMessageContent::List(items) = message_content {
                    let calls: Vec<ToolCall> = items
                        .into_iter()
                        .filter_map(|item| {
                            if let LLMMessageTypedContent::ToolCall { id, name, args } = item {
                                Some(ToolCall {
                                    id,
                                    r#type: "function".to_string(),
                                    function: FunctionCall {
                                        name,
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

                (text_content, tool_calls)
            }
        } else {
            return Err("Local LLM provider not configured.".to_string());
        };

        Ok(ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String(response_content.clone())),
            name: None,
            tool_calls: tool_calls.clone(),
            tool_call_id: None,
        })
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
        if let Some(config) = &self.anthropic_config {
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
            let response = Anthropic::chat(
                config,
                AnthropicInput {
                    model: AnthropicModel::Claude45Haiku,
                    messages,
                    grammar: None,
                    max_tokens: 100,
                    stop_sequences: None,
                    tools: None,
                    thinking: Default::default(),
                },
            )
            .await
            .map_err(|e| e.to_string())?;

            Ok(response.choices[0].message.content.to_string())
        } else {
            Err("Failed to generate session title".to_string())
        }
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
