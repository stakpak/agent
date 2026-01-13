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
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error body".to_string());

            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&error_body) {
                if let Ok(api_error) = serde_json::from_value::<ApiError>(json.clone()) {
                    if api_error.error.key == "EXCEEDED_API_LIMIT" {
                        return Err(format!(
                            "{}.\n\nPlease top up your account at https://stakpak.dev/settings/billing to keep Stakpaking.",
                            api_error.error.message
                        ));
                    } else {
                        return Err(api_error.error.message);
                    }
                }

                if let Some(error_obj) = json.get("error") {
                    let error_message =
                        if let Some(message) = error_obj.get("message").and_then(|m| m.as_str()) {
                            message.to_string()
                        } else if let Some(code) = error_obj.get("code").and_then(|c| c.as_str()) {
                            format!("API error: {}", code)
                        } else if let Some(key) = error_obj.get("key").and_then(|k| k.as_str()) {
                            format!("API error: {}", key)
                        } else {
                            serde_json::to_string(error_obj)
                                .unwrap_or_else(|_| "Unknown API error".to_string())
                        };
                    return Err(error_message);
                }
            }

            Err(error_body)
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
            Err(_) => {
                // eprintln!("Failed to deserialize response: {}", e);
                // eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
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

    async fn get_billing_info(
        &self,
        account_username: &str,
    ) -> Result<stakpak_shared::models::billing::BillingResponse, String> {
        // Billing endpoint is v2 and requires account username in path

        let base = self.base_url.trim_end_matches("/v1");
        let url = format!("{}/v2/{}/billing", base, account_username);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<stakpak_shared::models::billing::BillingResponse>(
            value.clone(),
        ) {
            Ok(response) => Ok(response),
            Err(e) => {
                let error_msg = format!("Failed to deserialize billing response: {}", e);
                Err(error_msg)
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

        let model_string = model.to_string();
        let input = ChatCompletionRequest::new(model_string.clone(), messages, tools, None);

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        if let Some(error_obj) = value.get("error") {
            let error_message = if let Some(message) =
                error_obj.get("message").and_then(|m| m.as_str())
            {
                message.to_string()
            } else if let Some(code) = error_obj.get("code").and_then(|c| c.as_str()) {
                format!("API error: {}", code)
            } else if let Some(key) = error_obj.get("key").and_then(|k| k.as_str()) {
                format!("API error: {}", key)
            } else {
                serde_json::to_string(error_obj).unwrap_or_else(|_| "Unknown API error".to_string())
            };
            return Err(error_message);
        }

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

        let model_string = model.to_string();
        let input = ChatCompletionRequest::new(model_string.clone(), messages, tools, Some(true));

        let response = self
            .client
            .post(&url)
            .headers(headers.unwrap_or_default())
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        // Check content-type before processing
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown");

        // Extract x-request-id from headers
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // If content-type is not event-stream, it's likely an error message
        if !content_type.contains("event-stream") && !content_type.contains("text/event-stream") {
            let status = response.status();
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error body".to_string());

            let error_message =
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&error_body) {
                    // Try ApiError format first (Stakpak API format)
                    if let Ok(api_error) = serde_json::from_value::<ApiError>(json.clone()) {
                        api_error.error.message
                    } else if let Some(error_obj) = json.get("error") {
                        // Generic error format
                        if let Some(message) = error_obj.get("message").and_then(|m| m.as_str()) {
                            message.to_string()
                        } else if let Some(code) = error_obj.get("code").and_then(|c| c.as_str()) {
                            format!("API error: {}", code)
                        } else {
                            error_body
                        }
                    } else {
                        error_body
                    }
                } else {
                    error_body
                };

            return Err(format!(
                "Server returned non-stream response ({}): {}",
                status, error_message
            ));
        }

        let response = self.handle_response_error(response).await?;
        let stream = response.bytes_stream().eventsource().map(move |event| {
            event
                .map_err(|_| ApiStreamError::Unknown("Failed to read response".to_string()))
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

    // async fn build_code_index(
    //     &self,
    //     input: &BuildCodeIndexInput,
    // ) -> Result<BuildCodeIndexOutput, String> {
    //     let url = format!("{}/commands/build_code_index", self.base_url,);

    //     let response = self
    //         .client
    //         .post(&url)
    //         .json(&input)
    //         .send()
    //         .await
    //         .map_err(|e: ReqwestError| e.to_string())?;

    //     let response = self.handle_response_error(response).await?;

    //     let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    //     match serde_json::from_value::<BuildCodeIndexOutput>(value.clone()) {
    //         Ok(response) => Ok(response),
    //         Err(e) => {
    //             eprintln!("Failed to deserialize response: {}", e);
    //             eprintln!("Raw response: {}", value);
    //             Err("Failed to deserialize response:".into())
    //         }
    //     }
    // }

    async fn search_docs(&self, input: &SearchDocsRequest) -> Result<Vec<Content>, String> {
        self.call_mcp_tool(&ToolsCallParams {
            name: "search_docs".to_string(),
            arguments: serde_json::to_value(input).map_err(|e| e.to_string())?,
        })
        .await
    }

    async fn search_memory(&self, input: &SearchMemoryRequest) -> Result<Vec<Content>, String> {
        self.call_mcp_tool(&ToolsCallParams {
            name: "search_memory".to_string(),
            arguments: serde_json::to_value(input).map_err(|e| e.to_string())?,
        })
        .await
    }

    async fn slack_read_messages(
        &self,
        input: &SlackReadMessagesRequest,
    ) -> Result<Vec<Content>, String> {
        self.call_mcp_tool(&ToolsCallParams {
            name: "slack_read_messages".to_string(),
            arguments: serde_json::to_value(input).map_err(|e| e.to_string())?,
        })
        .await
    }

    async fn slack_read_replies(
        &self,
        input: &SlackReadRepliesRequest,
    ) -> Result<Vec<Content>, String> {
        self.call_mcp_tool(&ToolsCallParams {
            name: "slack_read_replies".to_string(),
            arguments: serde_json::to_value(input).map_err(|e| e.to_string())?,
        })
        .await
    }

    async fn slack_send_message(
        &self,
        input: &SlackSendMessageRequest,
    ) -> Result<Vec<Content>, String> {
        // Note: The remote tool expects "markdown_text" but the struct has "mrkdwn_text".
        // We need to map this correctly. The struct in models.rs has mrkdwn_text.
        // The remote tool likely expects what was previously passed.
        // In slack.rs, it was mapping "mrkdwn_text" to "markdown_text".
        // So we should construct the arguments manually or use a custom serializer if we want to match exactly.
        // However, since we are sending `input` which is `SlackSendMessageRequest`, let's check its definition.
        // It has `mrkdwn_text`.
        // The previous implementation in slack.rs did:
        // arguments: json!({
        //     "channel": channel,
        //     "markdown_text": mrkdwn_text,
        //     "thread_ts": thread_ts,
        // }),
        // So we need to replicate this mapping.

        let arguments = json!({
            "channel": input.channel,
            "markdown_text": input.mrkdwn_text,
            "thread_ts": input.thread_ts,
        });

        self.call_mcp_tool(&ToolsCallParams {
            name: "slack_send_message".to_string(),
            arguments,
        })
        .await
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
