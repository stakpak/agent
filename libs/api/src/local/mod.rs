use crate::{AgentProvider, GetMyAccountResponse};
use crate::{ListRuleBook, models::*};
use async_trait::async_trait;
use futures_util::Stream;
use libsql::{Builder, Connection};
use reqwest::header::HeaderMap;
use rmcp::model::Content;
use stakpak_shared::models::integrations::openai::{
    AgentModel, ChatCompletionResponse, ChatCompletionStreamResponse, ChatMessage, MessageContent,
    Tool,
};
use uuid::Uuid;

mod db;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub struct LocalClient {
    pub db: Connection,
}

pub struct LocalClientConfig {
    pub store_path: Option<String>,
}

const DEFAULT_STORE_PATH: &str = ".stakpak/data";

impl LocalClient {
    pub async fn new(config: LocalClientConfig) -> Result<Self, String> {
        let default_store_path = std::env::home_dir()
            .unwrap_or_default()
            .join(DEFAULT_STORE_PATH);

        let db = Builder::new_local(
            config
                .store_path
                .unwrap_or(default_store_path.display().to_string()),
        )
        .build()
        .await
        .map_err(|e| e.to_string())?;

        let conn = db.connect().map_err(|e| e.to_string())?;

        // Initialize database schema
        db::init_schema(&conn).await?;

        Ok(Self { db: conn })
    }
}

#[async_trait]
impl AgentProvider for LocalClient {
    async fn get_my_account(&self) -> Result<GetMyAccountResponse, String> {
        Err("Local provider does not support account management yet".to_string())
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
        Err("Local provider does not support rulebooks yet".to_string())
    }

    async fn delete_rulebook(&self, _uri: &str) -> Result<(), String> {
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
        Err("Local provider does not support session stats yet".to_string())
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
        _tools: Option<Vec<Tool>>,
    ) -> Result<ChatCompletionResponse, String> {
        let (mut parent_checkpoint, running_checkpoint, _) =
            self.initialize_session(messages.clone()).await?;

        // 4. Placeholder LLM Invocation
        // TODO: Integrate with actual LLM provider (OpenAI/Anthropic/Local)
        let response_content = format!(
            "Echo: {}",
            messages
                .last()
                .and_then(|m| m.content.as_ref().map(|c| c.to_string()))
                .unwrap_or_default()
        );

        let (response_message, complete_checkpoint_id, now) = self
            .update_session(
                &mut parent_checkpoint,
                &running_checkpoint,
                response_content,
            )
            .await?;

        Ok(ChatCompletionResponse {
            id: complete_checkpoint_id.to_string(),
            object: "chat.completion".to_string(),
            created: now.timestamp() as u64,
            model: model.to_string(),
            choices: vec![
                stakpak_shared::models::integrations::openai::ChatCompletionChoice {
                    index: 0,
                    message: response_message,
                    logprobs: None,
                    finish_reason: stakpak_shared::models::integrations::openai::FinishReason::Stop,
                },
            ],
            usage: stakpak_shared::models::integrations::openai::Usage {
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
        _tools: Option<Vec<Tool>>,
        _headers: Option<HeaderMap>,
    ) -> Result<
        (
            std::pin::Pin<
                Box<dyn Stream<Item = Result<ChatCompletionStreamResponse, ApiStreamError>> + Send>,
            >,
            Option<String>,
        ),
        String,
    > {
        let (mut parent_checkpoint, running_checkpoint, _) =
            self.initialize_session(messages.clone()).await?;

        // Placeholder LLM Invocation
        let response_content = format!(
            "Echo: {}",
            messages
                .last()
                .and_then(|m| m.content.as_ref().map(|c| c.to_string()))
                .unwrap_or_default()
        );

        let (_response_message, complete_checkpoint_id, now) = self
            .update_session(
                &mut parent_checkpoint,
                &running_checkpoint,
                response_content.clone(),
            )
            .await?;

        // Create a stream that yields the response in a single chunk (simulated streaming)
        // In a real implementation, this would yield multiple chunks
        let stream = async_stream::stream! {
            yield Ok(ChatCompletionStreamResponse {
                id: complete_checkpoint_id.to_string(),
                object: "chat.completion.chunk".to_string(),
                created: now.timestamp() as u64,
                model: model.to_string(),
                choices: vec![
                    stakpak_shared::models::integrations::openai::ChatCompletionStreamChoice {
                        index: 0,
                        delta: stakpak_shared::models::integrations::openai::ChatMessageDelta {
                            role: Some(stakpak_shared::models::integrations::openai::Role::Assistant),
                            content: Some(response_content),
                            tool_calls: None,
                        },
                        finish_reason: Some(stakpak_shared::models::integrations::openai::FinishReason::Stop),
                    },
                ],
                usage: None,
            });
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
        Err("Local provider does not support code indexing yet".to_string())
    }

    async fn search_docs(&self, _input: &SearchDocsRequest) -> Result<Vec<Content>, String> {
        Err("Local provider does not support search docs yet".to_string())
    }

    async fn search_memory(&self, _input: &SearchMemoryRequest) -> Result<Vec<Content>, String> {
        Err("Local provider does not support search memory yet".to_string())
    }

    async fn slack_read_messages(
        &self,
        _input: &SlackReadMessagesRequest,
    ) -> Result<Vec<Content>, String> {
        Err("Local provider does not support slack read messages yet".to_string())
    }

    async fn slack_read_replies(
        &self,
        _input: &SlackReadRepliesRequest,
    ) -> Result<Vec<Content>, String> {
        Err("Local provider does not support slack read replies yet".to_string())
    }

    async fn slack_send_message(
        &self,
        _input: &SlackSendMessageRequest,
    ) -> Result<Vec<Content>, String> {
        Err("Local provider does not support slack send message yet".to_string())
    }

    async fn memorize_session(&self, _checkpoint_id: Uuid) -> Result<(), String> {
        Ok(())
    }
}

impl LocalClient {
    async fn initialize_session(
        &self,
        messages: Vec<ChatMessage>,
    ) -> Result<(RunAgentOutput, AgentCheckpointListItem, Uuid), String> {
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

        let parent_checkpoint = if let Some(checkpoint_id) = checkpoint_id {
            db::get_checkpoint(&self.db, checkpoint_id).await?
        } else {
            // Create new session
            let session_id = Uuid::new_v4();
            let now = chrono::Utc::now();
            let session = AgentSession {
                id: session_id,
                title: "New Session".to_string(),
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
            // Initial state is just the messages
            let initial_state = AgentOutput::PabloV1 {
                messages: messages.clone(),
                node_states: serde_json::json!({}),
            };
            db::create_checkpoint(&self.db, session_id, &checkpoint, &initial_state).await?;

            db::get_checkpoint(&self.db, checkpoint_id).await?
        };

        // 3. Create a new "Running" checkpoint
        let current_checkpoint_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let running_checkpoint = AgentCheckpointListItem {
            id: current_checkpoint_id,
            status: AgentStatus::Running,
            execution_depth: parent_checkpoint.checkpoint.execution_depth + 1,
            parent: Some(AgentParentCheckpoint {
                id: parent_checkpoint.checkpoint.id,
            }),
            created_at: now,
            updated_at: now,
        };

        // For now, just copy the previous state
        db::create_checkpoint(
            &self.db,
            parent_checkpoint.session.id,
            &running_checkpoint,
            &parent_checkpoint.output,
        )
        .await?;

        Ok((parent_checkpoint, running_checkpoint, current_checkpoint_id))
    }

    async fn update_session(
        &self,
        parent_checkpoint: &mut RunAgentOutput,
        running_checkpoint: &AgentCheckpointListItem,
        response_content: String,
    ) -> Result<(ChatMessage, Uuid, chrono::DateTime<chrono::Utc>), String> {
        let now = chrono::Utc::now();
        // 6. Create "Complete" checkpoint
        let complete_checkpoint_id = Uuid::new_v4();
        let complete_checkpoint = AgentCheckpointListItem {
            id: complete_checkpoint_id,
            status: AgentStatus::Complete,
            execution_depth: running_checkpoint.execution_depth + 1,
            parent: Some(AgentParentCheckpoint {
                id: running_checkpoint.id,
            }),
            created_at: now,
            updated_at: now,
        };

        // Update state with the new assistant message
        let mut new_state = parent_checkpoint.output.clone();
        match &mut new_state {
            AgentOutput::PabloV1 { messages, .. } => {
                messages.push(ChatMessage {
                    role: stakpak_shared::models::integrations::openai::Role::Assistant,
                    content: Some(MessageContent::String(response_content.clone())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }

        db::create_checkpoint(
            &self.db,
            parent_checkpoint.session.id,
            &complete_checkpoint,
            &new_state,
        )
        .await?;

        // 7. Construct Response
        let response_message = ChatMessage {
            role: stakpak_shared::models::integrations::openai::Role::Assistant,
            content: Some(
                MessageContent::String(response_content)
                    .inject_checkpoint_id(complete_checkpoint_id),
            ),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        };

        Ok((response_message, complete_checkpoint_id, now))
    }
}
