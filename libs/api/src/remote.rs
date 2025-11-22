use crate::AgentProvider;
use crate::models::*;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::Stream;
use futures_util::StreamExt;
use reqwest::header::HeaderMap;
use reqwest::{Client as ReqwestClient, Error as ReqwestError, Response, header};
use rmcp::model::Content;
use rmcp::model::JsonRpcResponse;
use serde::Deserialize;
use serde_json::json;
use stakpak_shared::models::integrations::openai::{
    AgentModel, ChatCompletionRequest, ChatCompletionResponse, ChatCompletionStreamResponse,
    ChatMessage, Tool,
};
use stakpak_shared::tls_client::TlsClientConfig;
use stakpak_shared::tls_client::create_tls_client;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct RemoteClient {
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

impl RemoteClient {
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
}

#[async_trait]
impl AgentProvider for RemoteClient {
    async fn get_my_account(&self) -> Result<GetMyAccountResponse, String> {
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

    async fn list_rulebooks(&self) -> Result<Vec<ListRuleBook>, String> {
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

    async fn get_rulebook_by_uri(&self, uri: &str) -> Result<RuleBook, String> {
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

    async fn create_rulebook(
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

    async fn delete_rulebook(&self, uri: &str) -> Result<(), String> {
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

    async fn list_agent_sessions(&self) -> Result<Vec<AgentSession>, String> {
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

    async fn get_agent_session(&self, session_id: Uuid) -> Result<AgentSession, String> {
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

    async fn get_agent_session_stats(&self, session_id: Uuid) -> Result<AgentSessionStats, String> {
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

    async fn create_agent_session(
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

    async fn get_agent_checkpoint(&self, checkpoint_id: Uuid) -> Result<RunAgentOutput, String> {
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

    async fn get_agent_session_latest_checkpoint(
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

    async fn chat_completion(
        &self,
        model: AgentModel,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
    ) -> Result<ChatCompletionResponse, String> {
        let url = format!("{}/agents/openai/v1/chat/completions", self.base_url);

        let input = ChatCompletionRequest::new(model, messages, tools, None);

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

    async fn chat_completion_stream(
        &self,
        model: AgentModel,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        headers: Option<HeaderMap>,
    ) -> Result<
        (
            std::pin::Pin<
                Box<dyn Stream<Item = Result<ChatCompletionStreamResponse, ApiStreamError>> + Send>,
            >,
            Option<String>,
        ),
        String,
    > {
        let url = format!("{}/agents/openai/v1/chat/completions", self.base_url);

        let input = ChatCompletionRequest::new(model, messages, tools, Some(true));

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

        Ok((Box::pin(stream), request_id))
    }

    async fn cancel_stream(&self, request_id: String) -> Result<(), String> {
        let url = format!("{}/agents/requests/{}/cancel", self.base_url, request_id);
        self.client
            .post(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        Ok(())
    }

    async fn build_code_index(
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

    async fn call_mcp_tool(&self, input: &ToolsCallParams) -> Result<Vec<Content>, String> {
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

    async fn memorize_session(&self, checkpoint_id: Uuid) -> Result<(), String> {
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
