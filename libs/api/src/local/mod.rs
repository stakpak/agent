use crate::local::context_managers::scratchpad_context_manager::{
    ScratchpadContextManager, ScratchpadContextManagerOptions,
};
use crate::local::hooks::scratchpad_context_hook::{ContextHook, ContextHookOptions};
use crate::{AgentProvider, ApiStreamError, GetMyAccountResponse};
use crate::{ListRuleBook, models::*};
use async_trait::async_trait;
use futures_util::Stream;
use libsql::{Builder, Connection};
use reqwest::Error as ReqwestError;
use reqwest::header::HeaderMap;
use rmcp::model::Content;
use stakpak_shared::hooks::{HookContext, HookRegistry, LifecycleEvent};
use stakpak_shared::models::integrations::anthropic::{AnthropicConfig, AnthropicModel};
use stakpak_shared::models::integrations::gemini::{GeminiConfig, GeminiModel};
use stakpak_shared::models::integrations::openai::{
    AgentModel, ChatCompletionChoice, ChatCompletionResponse, ChatCompletionStreamChoice,
    ChatCompletionStreamResponse, ChatMessage, FinishReason, MessageContent, OpenAIConfig,
    OpenAIModel, Tool,
};
use stakpak_shared::models::integrations::searchpak::*;
use stakpak_shared::models::llm::{
    GenerationDelta, LLMInput, LLMMessage, LLMMessageContent, LLMModel, LLMProviderConfig,
    LLMStreamInput, chat, chat_stream,
};
use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

mod context_managers;
mod db;
mod hooks;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub struct LocalClient {
    pub db: Connection,
    pub stakpak_base_url: Option<String>,
    pub anthropic_config: Option<AnthropicConfig>,
    pub openai_config: Option<OpenAIConfig>,
    pub gemini_config: Option<GeminiConfig>,
    pub smart_model: LLMModel,
    pub eco_model: LLMModel,
    pub recovery_model: LLMModel,
    pub hook_registry: Option<Arc<HookRegistry<AgentState>>>,
    pub _search_services_guard: Option<Arc<SearchServicesGuard>>,
}

pub struct LocalClientConfig {
    pub stakpak_base_url: Option<String>,
    pub store_path: Option<String>,
    pub anthropic_config: Option<AnthropicConfig>,
    pub openai_config: Option<OpenAIConfig>,
    pub gemini_config: Option<GeminiConfig>,
    pub smart_model: Option<String>,
    pub eco_model: Option<String>,
    pub recovery_model: Option<String>,
    pub hook_registry: Option<HookRegistry<AgentState>>,
}

#[derive(Debug)]
enum StreamMessage {
    Delta(GenerationDelta),
    Ctx(Box<HookContext<AgentState>>),
}

const DEFAULT_STORE_PATH: &str = ".stakpak/data/local.db";
const SYSTEM_PROMPT: &str = include_str!("./prompts/agent.v1.txt");
const TITLE_GENERATOR_PROMPT: &str = include_str!("./prompts/session_title_generator.v1.txt");

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

        let smart_model = config
            .smart_model
            .map(LLMModel::from)
            .unwrap_or(LLMModel::Anthropic(AnthropicModel::Claude45Sonnet));

        let eco_model = config
            .eco_model
            .map(LLMModel::from)
            .unwrap_or(LLMModel::Anthropic(AnthropicModel::Claude45Haiku));

        let recovery_model = config
            .recovery_model
            .map(LLMModel::from)
            .unwrap_or(LLMModel::OpenAI(OpenAIModel::GPT5));

        // Add hooks
        let mut hook_registry = config.hook_registry.unwrap_or_default();
        hook_registry.register(
            LifecycleEvent::BeforeInference,
            Box::new(ContextHook::new(ContextHookOptions {
                context_manager: Box::new(ScratchpadContextManager::new(
                    ScratchpadContextManagerOptions {
                        history_action_message_size_limit: 100,
                        history_action_message_keep_last_n: 1,
                        history_action_result_keep_last_n: 50,
                    },
                )),
                smart_model: (smart_model.clone(), SYSTEM_PROMPT.to_string()),
                eco_model: (eco_model.clone(), SYSTEM_PROMPT.to_string()),
                recovery_model: (recovery_model.clone(), SYSTEM_PROMPT.to_string()),
            })),
        );

        Ok(Self {
            db: conn,
            stakpak_base_url: config.stakpak_base_url.map(|url| url + "/v1"),
            anthropic_config: config.anthropic_config,
            gemini_config: config.gemini_config,
            openai_config: config.openai_config,
            smart_model,
            eco_model,
            recovery_model,
            hook_registry: Some(Arc::new(hook_registry)),
            _search_services_guard: Some(Arc::new(SearchServicesGuard)),
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
        if self.stakpak_base_url.is_none() {
            return Ok(vec![]);
        }

        let stakpak_base_url = self
            .stakpak_base_url
            .as_ref()
            .ok_or("Stakpak base URL not set")?;

        let url = format!("{}/rules", stakpak_base_url);

        let client = create_tls_client(
            TlsClientConfig::default().with_timeout(std::time::Duration::from_secs(300)),
        )?;

        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        match serde_json::from_value::<ListRulebooksResponse>(value.clone()) {
            Ok(response) => Ok(response.results),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    async fn get_rulebook_by_uri(&self, uri: &str) -> Result<RuleBook, String> {
        let stakpak_base_url = self
            .stakpak_base_url
            .as_ref()
            .ok_or("Stakpak base URL not set")?;

        let encoded_uri = urlencoding::encode(uri);

        let url = format!("{}/rules/{}", stakpak_base_url, encoded_uri);

        let client = create_tls_client(
            TlsClientConfig::default().with_timeout(std::time::Duration::from_secs(300)),
        )?;

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        match serde_json::from_value::<RuleBook>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
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

    async fn search_docs(&self, input: &SearchDocsRequest) -> Result<Vec<Content>, String> {
        let config = start_search_services().await.map_err(|e| e.to_string())?;

        let api_url = format!("http://localhost:{}", config.api_port);
        let search_client = SearchPakClient::new(api_url);

        let initial_query = if let Some(exclude) = &input.exclude_keywords {
            format!("{} -{}", input.keywords, exclude)
        } else {
            input.keywords.clone()
        };

        let llm_config = self.get_llm_config();
        let search_model = get_search_model(self.eco_model.clone());
        let analysis = analyze_search_query(&llm_config, &search_model, &initial_query).await?;
        let required_documentation = analysis.required_documentation;
        let mut current_query = analysis.reformulated_query;
        let mut previous_queries = Vec::new();
        let mut final_valid_docs = Vec::new();
        let mut accumulated_needed_urls = Vec::new();

        const MAX_ITERATIONS: usize = 3;

        for _iteration in 0..MAX_ITERATIONS {
            previous_queries.push(current_query.clone());

            let search_results = search_client
                .search_and_scrape(current_query.clone())
                .await
                .map_err(|e| e.to_string())?;

            if search_results.is_empty() {
                break;
            }

            let validation_result = validate_search_docs(
                &llm_config,
                &search_model,
                &search_results,
                &current_query,
                &required_documentation,
                &previous_queries,
                &accumulated_needed_urls,
            )
            .await?;

            for url in &validation_result.needed_urls {
                if !accumulated_needed_urls.contains(url) {
                    accumulated_needed_urls.push(url.clone());
                }
            }

            for doc in validation_result.valid_docs.into_iter() {
                let is_duplicate = final_valid_docs
                    .iter()
                    .any(|existing_doc: &ScrapedContent| existing_doc.url == doc.url);

                if !is_duplicate {
                    final_valid_docs.push(doc);
                }
            }

            if validation_result.is_satisfied {
                break;
            }

            if let Some(new_query) = validation_result.new_query {
                if new_query != current_query && !previous_queries.contains(&new_query) {
                    current_query = new_query;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if final_valid_docs.is_empty() {
            return Ok(Vec::new());
        }

        let contents: Vec<Content> = final_valid_docs
            .into_iter()
            .map(|result| {
                Content::text(format!(
                    "Title: {}\nURL: {}\nContent: {}",
                    result.title.unwrap_or_default(),
                    result.url,
                    result.content.unwrap_or_default(),
                ))
            })
            .collect();

        Ok(contents)
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
    fn get_llm_config(&self) -> LLMProviderConfig {
        LLMProviderConfig {
            anthropic_config: self.anthropic_config.clone(),
            openai_config: self.openai_config.clone(),
            gemini_config: self.gemini_config.clone(),
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
            return Err(
                "Run agent completion: LLM input not found, make sure to register a context hook before inference"
                    .to_string(),
            );
        };

        let llm_config = self.get_llm_config();

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
                chat_stream(&llm_config, input)
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
            let response = chat(&llm_config, input).await.map_err(|e| e.to_string())?;
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
        let llm_config = self.get_llm_config();
        let llm_model = self.eco_model.clone();

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
            model: llm_model,
            messages,
            max_tokens: 100,
            tools: None,
        };

        let response = chat(&llm_config, input).await.map_err(|e| e.to_string())?;

        Ok(response.choices[0].message.content.to_string())
    }
}

async fn analyze_search_query(
    llm_config: &LLMProviderConfig,
    model: &LLMModel,
    query: &str,
) -> Result<AnalysisResult, String> {
    let system_prompt = r#"You are a query analyzer. Analyze the user's search query and output JSON only:
{
  "required_documentation": ["list of document types/topics needed"],
  "reformulated_query": "optimized search query"
}"#;

    let user_prompt = format!(
        "Analyze this search query and determine what documentation is needed: '{}'",
        query
    );

    let response = chat(
        llm_config,
        LLMInput {
            model: model.clone(),
            messages: vec![
                LLMMessage {
                    role: "system".into(),
                    content: LLMMessageContent::String(system_prompt.to_string()),
                },
                LLMMessage {
                    role: "user".into(),
                    content: LLMMessageContent::String(user_prompt.to_string()),
                },
            ],
            max_tokens: 2000,
            tools: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    let content = response.choices[0].message.content.to_string();

    serde_json::from_str::<AnalysisResult>(&content)
        .map_err(|e| format!("Failed to parse analysis: {}", e))
}

async fn validate_search_docs(
    llm_config: &LLMProviderConfig,
    model: &LLMModel,
    docs: &[ScrapedContent],
    query: &str,
    required_documentation: &[String],
    previous_queries: &[String],
    accumulated_needed_urls: &[String],
) -> Result<ValidationResult, String> {
    let docs_preview = docs
        .iter()
        .take(5)
        .map(|r| format!("- {} ({})", r.title.clone().unwrap_or_default(), r.url))
        .collect::<Vec<_>>()
        .join("\n");

    let system_prompt = r#"You are a search result validator. Output JSON only:
{
  "is_satisfied": bool,
  "valid_docs": [{"url": "..."}],
  "needed_urls": ["urls still needed"],
  "new_query": "refined query if not satisfied",
  "reason": "explanation"
}"#;

    let user_prompt = format!(
        "Original Query: '{}'\nRequired Documentation: {:?}\nPrevious Queries: {:?}\nAccumulated Needed URLs: {:?}\nCurrent Results:\n{}\n\nAre these results sufficient? Which are valid? What's still needed?",
        query, required_documentation, previous_queries, accumulated_needed_urls, docs_preview
    );

    let response = chat(
        llm_config,
        LLMInput {
            model: model.clone(),
            messages: vec![
                LLMMessage {
                    role: "system".into(),
                    content: LLMMessageContent::String(system_prompt.to_string()),
                },
                LLMMessage {
                    role: "user".into(),
                    content: LLMMessageContent::String(user_prompt.to_string()),
                },
            ],
            max_tokens: 4000,
            tools: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    let content = response.choices[0].message.content.to_string();

    let mut validation: ValidationResult =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse validation: {}", e))?;

    // Enrich valid_docs with full content from search results
    validation.valid_docs = validation
        .valid_docs
        .into_iter()
        .filter_map(|valid_doc| {
            docs.iter()
                .find(|result| result.url == valid_doc.url)
                .cloned()
        })
        .collect();

    Ok(validation)
}

fn get_search_model(model: LLMModel) -> LLMModel {
    match model {
        LLMModel::Anthropic(_) => LLMModel::Anthropic(AnthropicModel::Claude45Haiku),
        LLMModel::OpenAI(_) => LLMModel::OpenAI(OpenAIModel::O4Mini),
        LLMModel::Gemini(_) => LLMModel::Gemini(GeminiModel::Gemini3Flash),
        _ => model,
    }
}
