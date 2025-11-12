use chrono::{DateTime, Utc};
use eventsource_stream::Eventsource;
use reqwest::header::HeaderMap;
use reqwest::{Client as ReqwestClient, Error as ReqwestError, Response, header};
use rmcp::model::Content;
use rmcp::model::JsonRpcResponse;
use serde::{Deserialize, Serialize};
use stakpak_shared::tls_client::TlsClientConfig;
use stakpak_shared::tls_client::create_tls_client;
pub mod models;
use futures_util::Stream;
use futures_util::StreamExt;
use models::*;
pub use models::{RecoveryMode, RecoveryOption, RecoveryOptionsResponse};
use serde_json::Value;
use serde_json::json;
use stakpak_shared::models::integrations::openai::{
    ChatCompletionRequest, ChatCompletionResponse, ChatCompletionStreamResponse, ChatMessage, Tool,
};
use uuid::Uuid;

#[derive(Clone, Debug)]

pub struct Client {
    client: ReqwestClient,
    base_url: String,
}

#[derive(Clone, Debug)]

pub struct ClientConfig {
    pub api_key: Option<String>,
    pub api_endpoint: String,
}

#[derive(Deserialize)]
struct ApiError {
    error: ApiErrorDetail,
}

#[derive(Deserialize)]
struct ApiErrorDetail {
    key: String,
    message: String,
}

impl Client {
    async fn handle_response_error(&self, response: Response) -> Result<Response, String> {
        if response.status().is_success() {
            Ok(response)
        } else {
            match response.json::<ApiError>().await {
                Ok(response) => {
                    if response.error.key == "EXCEEDED_API_LIMIT" {
                        Err(format!(
                            "{}.\n\nPlease top up your account at https://stakpak.dev/settings/billing to keep Stakpaking.",
                            response.error.message
                        ))
                    } else {
                        Err(response.error.message)
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        }
    }

    pub fn new(config: &ClientConfig) -> Result<Self, String> {
        if config.api_key.is_none() {
            return Err("API Key not found, please login".into());
        }

        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", config.api_key.clone().unwrap()))
                .expect("Invalid API key format"),
        );
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_str(&format!("Stakpak/{}", env!("CARGO_PKG_VERSION")))
                .expect("Invalid user agent format"),
        );

        let client = create_tls_client(
            TlsClientConfig::default()
                .with_headers(headers)
                .with_timeout(std::time::Duration::from_secs(300)),
        )?;

        Ok(Self {
            client,
            base_url: config.api_endpoint.clone() + "/v1",
        })
    }

    pub async fn get_my_account(&self) -> Result<GetMyAccountResponse, String> {
        let url = format!("{}/account", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<GetMyAccountResponse>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn list_rulebooks(&self) -> Result<Vec<ListRuleBook>, String> {
        let url = format!("{}/rules", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

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

    pub async fn get_rulebook_by_uri(&self, uri: &str) -> Result<RuleBook, String> {
        // URL encode the URI to handle special characters
        let encoded_uri = urlencoding::encode(uri);
        let url = format!("{}/rules/{}", self.base_url, encoded_uri);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

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

    pub async fn create_rulebook(
        &self,
        uri: &str,
        description: &str,
        content: &str,
        tags: Vec<String>,
        visibility: Option<RuleBookVisibility>,
    ) -> Result<CreateRuleBookResponse, String> {
        let url = format!("{}/rules", self.base_url);

        let input = CreateRuleBookInput {
            uri: uri.to_string(),
            description: description.to_string(),
            content: content.to_string(),
            tags,
            visibility,
        };

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        // Check status before consuming body
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("API error ({}): {}", status, error_text));
        }

        // Get response as text first to handle non-JSON responses
        let response_text = response.text().await.map_err(|e| e.to_string())?;

        // Try to parse as JSON first
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&response_text) {
            match serde_json::from_value::<CreateRuleBookResponse>(value.clone()) {
                Ok(response) => return Ok(response),
                Err(e) => {
                    eprintln!("Failed to deserialize JSON response: {}", e);
                    eprintln!("Raw response: {}", value);
                }
            }
        }

        // If JSON parsing failed, try to parse as plain text "id: <uuid>"
        if response_text.starts_with("id: ") {
            let id = response_text.trim_start_matches("id: ").trim().to_string();
            return Ok(CreateRuleBookResponse { id });
        }

        Err(format!("Unexpected response format: {}", response_text))
    }

    pub async fn delete_rulebook(&self, uri: &str) -> Result<(), String> {
        let encoded_uri = urlencoding::encode(uri);
        let url = format!("{}/rules/{}", self.base_url, encoded_uri);

        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let _response = self.handle_response_error(response).await?;

        Ok(())
    }

    pub async fn list_agent_sessions(&self) -> Result<Vec<AgentSession>, String> {
        let url = format!("{}/agents/sessions", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<Vec<AgentSession>>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn get_agent_session(&self, session_id: Uuid) -> Result<AgentSession, String> {
        let url = format!("{}/agents/sessions/{}", self.base_url, session_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        match serde_json::from_value::<AgentSession>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn get_agent_session_stats(
        &self,
        session_id: Uuid,
    ) -> Result<AgentSessionStats, String> {
        let url = format!("{}/agents/sessions/{}/stats", self.base_url, session_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        match serde_json::from_value::<AgentSessionStats>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn create_agent_session(
        &self,
        agent_id: AgentID,
        visibility: AgentSessionVisibility,
        input: Option<AgentInput>,
    ) -> Result<AgentSession, String> {
        let url = format!("{}/agents/sessions", self.base_url);

        let input = serde_json::json!({
            "agent_id": agent_id,
            "visibility": visibility,
            "input": input,
        });

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<AgentSession>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn run_agent(&self, input: &RunAgentInput) -> Result<RunAgentOutput, String> {
        let url = format!("{}/agents/run", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<RunAgentOutput>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn get_agent_checkpoint(
        &self,
        checkpoint_id: Uuid,
    ) -> Result<RunAgentOutput, String> {
        let url = format!("{}/agents/checkpoints/{}", self.base_url, checkpoint_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<RunAgentOutput>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn get_agent_session_latest_checkpoint(
        &self,
        session_id: Uuid,
    ) -> Result<RunAgentOutput, String> {
        let url = format!(
            "{}/agents/sessions/{}/checkpoints/latest",
            self.base_url, session_id
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<RunAgentOutput>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn get_recovery_options(
        &self,
        session_id: Uuid,
        status: Option<&str>,
    ) -> Result<RecoveryOptionsResponse, String> {
        let url = format!(
            "{}/recovery/sessions/{}/recoveries",
            self.base_url, session_id
        );

        let status = status.unwrap_or("pending");

        let response = self
            .client
            .get(&url)
            .query(&[("status", status)])
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        eprintln!("Recovery options response: {:?}", response);

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        if value.is_null() {
            return Ok(RecoveryOptionsResponse {
                recovery_options: Vec::new(),
                id: None,
            });
        }

        if let Some(result) = value.get("result") {
            if result.is_null() {
                return Ok(RecoveryOptionsResponse {
                    recovery_options: Vec::new(),
                    id: None,
                });
            }

            match serde_json::from_value::<RecoveryOptionsResponse>(result.clone()) {
                Ok(response) => return Ok(response),
                Err(e) => {
                    eprintln!("Failed to deserialize response: {}", e);
                    eprintln!("Raw response: {}", result);
                }
            }
        }

        if let Some(recovery_options) = value.get("recovery_options")
            && recovery_options.is_null()
        {
            return Ok(RecoveryOptionsResponse {
                recovery_options: Vec::new(),
                id: None,
            });
        }

        match serde_json::from_value::<RecoveryOptionsResponse>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn submit_recovery_action(
        &self,
        session_id: Uuid,
        recovery_id: &str,
        action: RecoveryActionType,
        selected_option_id: Option<Uuid>,
    ) -> Result<(), String> {
        let url = format!(
            "{}/recovery/sessions/{}/recovery/{}/action",
            self.base_url, session_id, recovery_id
        );

        let payload = RecoveryActionRequest {
            action,
            selected_option_id,
        };

        eprintln!("Submitting recovery action: {:?}", payload);

        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        self.handle_response_error(response).await?;
        Ok(())
    }

    pub async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
    ) -> Result<ChatCompletionResponse, String> {
        let url = format!("{}/agents/openai/v1/chat/completions", self.base_url);

        let input = ChatCompletionRequest::new(messages, tools, None);

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        match serde_json::from_value::<ChatCompletionResponse>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn chat_completion_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        headers: Option<HeaderMap>,
    ) -> Result<
        (
            impl Stream<Item = Result<ChatCompletionStreamResponse, ApiStreamError>>,
            Option<String>,
        ),
        String,
    > {
        let url = format!("{}/agents/openai/v1/chat/completions", self.base_url);

        let input = ChatCompletionRequest::new(messages, tools, Some(true));

        let response = self
            .client
            .post(&url)
            .headers(headers.unwrap_or_default())
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        // Extract x-request-id from headers
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let response = self.handle_response_error(response).await?;
        let stream = response.bytes_stream().eventsource().map(|event| {
            event
                .map_err(|err| {
                    eprintln!("stream: failed to read response: {:?}", err);
                    ApiStreamError::Unknown("Failed to read response".to_string())
                })
                .and_then(|event| match event.event.as_str() {
                    "error" => Err(ApiStreamError::from(event.data)),
                    _ => serde_json::from_str::<ChatCompletionStreamResponse>(&event.data).map_err(
                        |_| {
                            ApiStreamError::Unknown(
                                "Failed to parse JSON from Anthropic response".to_string(),
                            )
                        },
                    ),
                })
        });

        Ok((stream, request_id))
    }

    pub async fn cancel_stream(&self, request_id: String) -> Result<(), String> {
        let url = format!("{}/agents/requests/{}/cancel", self.base_url, request_id);
        self.client
            .post(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        Ok(())
    }

    pub async fn build_code_index(
        &self,
        input: &BuildCodeIndexInput,
    ) -> Result<BuildCodeIndexOutput, String> {
        let url = format!("{}/commands/build_code_index", self.base_url,);

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<BuildCodeIndexOutput>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn call_mcp_tool(&self, input: &ToolsCallParams) -> Result<Vec<Content>, String> {
        let url = format!("{}/mcp", self.base_url);

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": input.name,
                "arguments": input.arguments,
            },
            "id": Uuid::new_v4().to_string(),
        });

        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        match serde_json::from_value::<JsonRpcResponse<ToolsCallResponse>>(value.clone()) {
            Ok(response) => Ok(response.result.content),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn memorize_session(&self, checkpoint_id: Uuid) -> Result<(), String> {
        let url = format!(
            "{}/agents/sessions/checkpoints/{}/extract-memory",
            self.base_url, checkpoint_id
        );

        let response = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let _ = self.handle_response_error(response).await?;
        Ok(())
    }
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GetMyAccountResponse {
    pub username: String,
    pub id: String,
    pub first_name: String,
    pub last_name: String,
}

impl GetMyAccountResponse {
    pub fn to_text(&self) -> String {
        format!(
            "ID: {}\nUsername: {}\nName: {} {}",
            self.id, self.username, self.first_name, self.last_name
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum RuleBookVisibility {
    #[default]
    Public,
    Private,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ListRuleBook {
    pub id: String,
    pub uri: String,
    pub description: String,
    pub visibility: RuleBookVisibility,
    pub tags: Vec<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ListRulebooksResponse {
    pub results: Vec<ListRuleBook>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RuleBook {
    pub id: String,
    pub uri: String,
    pub description: String,
    pub content: String,
    pub visibility: RuleBookVisibility,
    pub tags: Vec<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateRuleBookInput {
    pub uri: String,
    pub description: String,
    pub content: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<RuleBookVisibility>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateRuleBookResponse {
    pub id: String,
}

impl ListRuleBook {
    pub fn to_text(&self) -> String {
        format!(
            "URI: {}\nDescription: {}\nTags: {}\n",
            self.uri,
            self.description,
            self.tags.join(", ")
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SimpleLLMMessage {
    #[serde(rename = "role")]
    pub role: SimpleLLMRole,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SimpleLLMRole {
    User,
    Assistant,
}

impl std::fmt::Display for SimpleLLMRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimpleLLMRole::User => write!(f, "user"),
            SimpleLLMRole::Assistant => write!(f, "assistant"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ToolsCallParams {
    pub name: String,
    pub arguments: Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ToolsCallResponse {
    pub content: Vec<Content>,
}
