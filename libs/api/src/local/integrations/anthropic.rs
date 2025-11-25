use std::collections::HashMap;

use super::models::{
    generation::{GenerationDelta, GenerationDeltaToolUse},
    llm::{
        LLMChoice, LLMCompletionResponse, LLMMessage, LLMMessageContent, LLMMessageTypedContent,
        LLMTool,
    },
};
use crate::error::{AgentError, BadRequestErrorMessage};
use futures_util::StreamExt;
use itertools::Itertools;
use reqwest::Response;
use reqwest_middleware::ClientBuilder;
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use stakpak_shared::models::llm::{LLMTokenUsage, PromptTokensDetails};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum AnthropicModel {
    #[serde(rename = "claude-haiku-4-5-20251001")]
    Claude45Haiku,
    #[serde(rename = "claude-sonnet-4-5-20250929")]
    Claude45Sonnet,
    #[serde(rename = "claude-opus-4-5-20251101")]
    Claude45Opus,
}
impl std::fmt::Display for AnthropicModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnthropicModel::Claude45Haiku => write!(f, "claude-haiku-4-5-20251001"),
            AnthropicModel::Claude45Sonnet => write!(f, "claude-sonnet-4-5-20250929"),
            AnthropicModel::Claude45Opus => write!(f, "claude-opus-4-5-20251101"),
        }
    }
}

impl AnthropicModel {
    pub fn from_string(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.to_string()))
            .map_err(|_| "Failed to deserialize Anthropic model".to_string())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AnthropicInput {
    pub model: AnthropicModel,
    pub messages: Vec<LLMMessage>,
    pub grammar: Option<String>,
    pub max_tokens: u32,
    pub stop_sequences: Option<Vec<String>>,
    pub tools: Option<Vec<LLMTool>>,
    pub thinking: ThinkingInput,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ThinkingInput {
    pub r#type: ThinkingType,
    // Must be ≥1024 and less than max_tokens
    pub budget_tokens: u32,
}

impl Default for ThinkingInput {
    fn default() -> Self {
        Self {
            r#type: ThinkingType::default(),
            budget_tokens: 1024,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingType {
    Enabled,
    #[default]
    Disabled,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AnthropicOutputUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AnthropicOutput {
    pub id: String,
    pub r#type: String,
    pub role: String,
    pub content: LLMMessageContent,
    pub model: String,
    pub stop_reason: String,
    pub usage: AnthropicOutputUsage,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AnthropicErrorOutput {
    pub r#type: String,
    pub error: AnthropicError,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AnthropicError {
    pub message: String,
    pub r#type: String,
}

impl From<AnthropicOutput> for LLMCompletionResponse {
    fn from(val: AnthropicOutput) -> Self {
        let choices = vec![LLMChoice {
            finish_reason: Some(val.stop_reason.clone()),
            index: 0,
            message: LLMMessage {
                role: val.role.clone(),
                content: val.content,
            },
        }];

        LLMCompletionResponse {
            id: val.id,
            model: val.model,
            object: val.r#type,
            choices,
            created: chrono::Utc::now().timestamp_millis() as u64,
            usage: Some(val.usage.into()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    pub event: String,
    #[serde(flatten)]
    pub data: AnthropicStreamEventData,
}

impl From<AnthropicOutputUsage> for LLMTokenUsage {
    fn from(usage: AnthropicOutputUsage) -> Self {
        let input_tokens = usage.input_tokens
            + usage.cache_creation_input_tokens.unwrap_or(0)
            + usage.cache_read_input_tokens.unwrap_or(0);
        let output_tokens = usage.output_tokens;
        Self {
            completion_tokens: output_tokens,
            prompt_tokens: input_tokens,
            total_tokens: input_tokens + output_tokens,
            prompt_tokens_details: Some(PromptTokensDetails {
                input_tokens: usage.input_tokens,
                output_tokens,
                cache_read_input_tokens: usage.cache_read_input_tokens.unwrap_or(0),
                cache_write_input_tokens: usage.cache_creation_input_tokens.unwrap_or(0),
            }),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AnthropicStreamOutput {
    pub id: String,
    pub r#type: String,
    pub role: String,
    pub content: LLMMessageContent,
    pub model: String,
    pub stop_reason: Option<String>,
    pub usage: AnthropicOutputUsage,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum AnthropicStreamEventData {
    MessageStart {
        message: AnthropicStreamOutput,
    },
    ContentBlockStart {
        index: usize,
        content_block: ContentBlock,
    },
    ContentBlockDelta {
        index: usize,
        delta: ContentDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: MessageDelta,
        usage: Option<AnthropicOutputUsage>,
    },
    MessageStop {},
    Ping {},
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum ContentDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MessageDelta {
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct AnthropicConfig {
    pub api_endpoint: Option<String>,
    pub api_key: Option<String>,
}

pub struct Anthropic {}

impl Anthropic {
    pub async fn chat(
        config: &AnthropicConfig,
        input: AnthropicInput,
    ) -> Result<LLMCompletionResponse, AgentError> {
        let model = serde_json::to_string(&input.model)
            .map_err(|e| AgentError::BadRequest(BadRequestErrorMessage::ApiError(e.to_string())))?
            .trim_matches('"')
            .to_string();

        let mut payload = json!({
            "model": model,
            "system": input.messages.iter().find(|mess| mess.role == "system").map(|mess| mess.content.clone()),
            "messages": input.messages.into_iter().filter(|message| message.role!= "system").collect::<Vec<LLMMessage>>(),
            "max_tokens": input.max_tokens,
            "temperature": 0,
            "stream": false,
        });

        if let Some(tools) = input.tools {
            payload["tools"] = json!(tools);
        }

        if let Some(stop_sequences) = input.stop_sequences {
            payload["stop_sequences"] = json!(stop_sequences);
        }

        // Setup retry with exponential backoff
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        let api_endpoint = config
            .api_endpoint
            .as_ref()
            .map_or("https://api.anthropic.com/v1/messages", |v| v);
        let api_key = config.api_key.as_ref().map_or("", |v| v);

        // Send the POST request
        let response = client
            .post(api_endpoint)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .await;

        let response = match response {
            Ok(resp) => resp,
            Err(e) => {
                let error_message = format!("Anthropic API request error: {e}");
                return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                    error_message,
                )));
            }
        };

        // Check for HTTP status errors and extract error details if present
        if !response.status().is_success() {
            let status = response.status();
            let error_body = match response.text().await {
                Ok(body) => body,
                Err(_) => "Unable to read error response".to_string(),
            };

            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                format!(
                    "Anthropic API returned error status: {}, body: {}",
                    status, error_body
                ),
            )));
        }

        match response.json::<Value>().await {
            Ok(json) => {
                // I have to copy this here to print the original response in case we find an error
                let pretty_json = serde_json::to_string_pretty(&json).unwrap_or_default();
                match serde_json::from_value::<AnthropicOutput>(json) {
                    Ok(json_response) => Ok(json_response.into()),
                    Err(e) => Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                        format!(
                            "Error deserializing JSON: {:?}\nOriginal JSON: {}",
                            e, pretty_json
                        ),
                    ))),
                }
            }
            Err(e) => Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                format!("Failed to decode Anthropic JSON response:: {:?}", e),
            ))),
        }
    }

    pub async fn chat_stream(
        config: &AnthropicConfig,
        stream_channel_tx: tokio::sync::mpsc::Sender<GenerationDelta>,
        input: AnthropicInput,
    ) -> Result<LLMCompletionResponse, AgentError> {
        let model = serde_json::to_string(&input.model)
            .map_err(|e| AgentError::BadRequest(BadRequestErrorMessage::ApiError(e.to_string())))?
            .trim_matches('"')
            .to_string();

        let mut payload = json!({
            "model": model,
            "system": input.messages.iter().find(|mess| mess.role == "system").map(|mess| json!([
                {
                    "type": "text",
                    "text": mess.content.clone(),
                    "cache_control": {"type": "ephemeral", "ttl": "5m"}
                }
            ])),
            "messages": input.messages.into_iter().filter(|message| message.role != "system").collect::<Vec<LLMMessage>>(),
            "max_tokens": input.max_tokens,
            "temperature": 0,
            "stream": true,
        });

        if let Some(tools) = input.tools {
            payload["tools"] = json!(
                tools
                    .iter()
                    .map(|tool| {
                        let mut tool_json = json!(tool);
                        if let Some(last_tool) = tools.last() {
                            if tool == last_tool {
                                tool_json["cache_control"] =
                                    json!({"type": "ephemeral", "ttl": "1h"});
                            }
                        }
                        tool_json
                    })
                    .collect::<Vec<serde_json::Value>>()
            );
        }

        if let Some(stop_sequences) = input.stop_sequences {
            payload["stop_sequences"] = json!(stop_sequences);
        }

        // Setup retry with exponential backoff
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        let api_endpoint = config
            .api_endpoint
            .as_deref()
            .unwrap_or("https://api.anthropic.com/v1/messages");

        let api_key = config.api_key.as_ref().map_or("", |v| v);

        // Send the POST request
        let response = client
            .post(api_endpoint)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header(
                "anthropic-beta",
                "extended-cache-ttl-2025-04-11,context-1m-2025-08-07",
            )
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .await;

        let response = match response {
            Ok(resp) => resp,
            Err(e) => {
                return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                    e.to_string(),
                )));
            }
        };

        if !response.status().is_success() {
            let error_body = match response.json::<AnthropicErrorOutput>().await {
                Ok(body) => body,
                Err(_) => AnthropicErrorOutput {
                    r#type: "error".to_string(),
                    error: AnthropicError {
                        message: "Unable to read error response".to_string(),
                        r#type: "error".to_string(),
                    },
                },
            };

            match error_body.error.r#type.as_str() {
                "invalid_request_error" => {
                    return Err(AgentError::BadRequest(
                        BadRequestErrorMessage::InvalidAgentInput(error_body.error.message),
                    ));
                }
                _ => {
                    return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                        error_body.error.message,
                    )));
                }
            }
        }

        let completion_response =
            process_stream(response, model.clone(), stream_channel_tx).await?;

        Ok(completion_response)
    }
}

pub async fn process_stream(
    response: Response,
    model: String,
    stream_channel_tx: tokio::sync::mpsc::Sender<GenerationDelta>,
) -> Result<LLMCompletionResponse, AgentError> {
    let mut completion_response = LLMCompletionResponse {
        id: "".to_string(),
        model: model.clone(),
        object: "chat.completion".to_string(),
        choices: vec![],
        created: chrono::Utc::now().timestamp_millis() as u64,
        usage: None,
    };

    let mut choices: HashMap<usize, LLMChoice> = HashMap::from([(
        0,
        LLMChoice {
            finish_reason: None,
            index: 0,
            message: LLMMessage {
                role: "assistant".to_string(),
                content: LLMMessageContent::List(vec![]),
            },
        },
    )]);
    let mut contents: Vec<LLMMessageTypedContent> = vec![];
    let mut stream = response.bytes_stream();
    let mut unparsed_data = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            let error_message = format!("Failed to read stream chunk from Anthropic API: {e}");
            AgentError::BadRequest(BadRequestErrorMessage::ApiError(error_message))
        })?;

        let text = std::str::from_utf8(&chunk).map_err(|e| {
            let error_message = format!("Failed to parse UTF-8 from Anthropic response: {e}");
            AgentError::BadRequest(BadRequestErrorMessage::ApiError(error_message))
        })?;

        unparsed_data.push_str(text);

        while let Some(event_end) = unparsed_data.find("\n\n") {
            let event_str = unparsed_data[..event_end].to_string();
            unparsed_data = unparsed_data[event_end + 2..].to_string();

            if !event_str.starts_with("event: ") {
                continue;
            }

            let json_str = &event_str[event_str.find("data: ").map(|i| i + 6).unwrap_or(6)..];
            if json_str == "[DONE]" {
                continue;
            }

            match serde_json::from_str::<AnthropicStreamEventData>(json_str) {
                Ok(data) => {
                    match data {
                        AnthropicStreamEventData::MessageStart { message } => {
                            completion_response.id = message.id;
                            completion_response.model = message.model;
                            completion_response.object = message.r#type;
                            completion_response.usage = Some(message.usage.into());
                        }
                        AnthropicStreamEventData::ContentBlockStart {
                            content_block,
                            index,
                        } => match content_block {
                            ContentBlock::Text { text } => {
                                stream_channel_tx
                                    .send(GenerationDelta::Content {
                                        // if this will be rendered as markdown, we need to escape the < and >
                                        content: text.clone(), //.replace("<", "\\<").replace(">", "\\>"),
                                    })
                                    .await
                                    .map_err(|e| {
                                        AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                                            e.to_string(),
                                        ))
                                    })?;
                                contents.push(LLMMessageTypedContent::Text { text: text.clone() });
                            }
                            ContentBlock::Thinking { thinking } => {
                                stream_channel_tx
                                    .send(GenerationDelta::Thinking {
                                        thinking: thinking.clone(),
                                    })
                                    .await
                                    .map_err(|e| {
                                        AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                                            e.to_string(),
                                        ))
                                    })?;
                                contents.push(LLMMessageTypedContent::Text {
                                    text: thinking.clone(),
                                });
                            }
                            ContentBlock::ToolUse { id, name, input: _ } => {
                                stream_channel_tx
                                    .send(GenerationDelta::ToolUse {
                                        tool_use: GenerationDeltaToolUse {
                                            id: Some(id.clone()),
                                            name: Some(name.clone()),
                                            input: Some(String::new()),
                                            index,
                                        },
                                    })
                                    .await
                                    .map_err(|e| {
                                        AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                                            e.to_string(),
                                        ))
                                    })?;
                                // Initialize with empty string since we'll accumulate via InputJsonDelta events
                                contents.push(LLMMessageTypedContent::ToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    args: serde_json::Value::String(String::new()),
                                });
                            }
                        },
                        AnthropicStreamEventData::ContentBlockDelta { delta, index } => {
                            if let Some(content) = contents.get_mut(index) {
                                match delta {
                                    ContentDelta::TextDelta { text } => {
                                        stream_channel_tx
                                            .send(GenerationDelta::Content {
                                                // if this will be rendered as markdown, we need to escape the < and >
                                                content: text.clone(), //.replace("<", "\\<").replace(">", "\\>"),
                                            })
                                            .await
                                            .map_err(|e| {
                                                AgentError::BadRequest(
                                                    BadRequestErrorMessage::ApiError(e.to_string()),
                                                )
                                            })?;
                                        let delta_text = text.clone();
                                        if let LLMMessageTypedContent::Text { text } = content {
                                            text.push_str(&delta_text);
                                        }
                                    }
                                    ContentDelta::ThinkingDelta { thinking } => {
                                        stream_channel_tx
                                            .send(GenerationDelta::Thinking {
                                                thinking: thinking.clone(),
                                            })
                                            .await
                                            .map_err(|e| {
                                                AgentError::BadRequest(
                                                    BadRequestErrorMessage::ApiError(e.to_string()),
                                                )
                                            })?;
                                        if let LLMMessageTypedContent::Text { text } = content {
                                            text.push_str(&thinking);
                                        }
                                    }
                                    ContentDelta::InputJsonDelta { partial_json } => {
                                        stream_channel_tx
                                            .send(GenerationDelta::ToolUse {
                                                tool_use: GenerationDeltaToolUse {
                                                    id: None,
                                                    name: None,
                                                    input: Some(partial_json.clone()),
                                                    index,
                                                },
                                            })
                                            .await
                                            .map_err(|e| {
                                                AgentError::BadRequest(
                                                    BadRequestErrorMessage::ApiError(e.to_string()),
                                                )
                                            })?;
                                        if let Some(LLMMessageTypedContent::ToolCall {
                                            args: serde_json::Value::String(accumulated_json),
                                            ..
                                        }) = contents.get_mut(index)
                                        {
                                            accumulated_json.push_str(&partial_json);
                                        }
                                    }
                                }
                            }
                        }
                        AnthropicStreamEventData::ContentBlockStop { index } => {
                            if let Some(LLMMessageTypedContent::ToolCall { args, .. }) =
                                contents.get_mut(index)
                            {
                                if let serde_json::Value::String(json_str) = args {
                                    // Try to parse the accumulated JSON string
                                    *args = serde_json::from_str(json_str).unwrap_or_else(|_| {
                                        // If parsing fails, keep as string
                                        serde_json::Value::String(json_str.clone())
                                    });
                                }
                            }
                        }
                        AnthropicStreamEventData::MessageDelta { delta, usage } => {
                            //write message delta to file as json

                            if let Some(stop_reason) = delta.stop_reason {
                                for choice in choices.values_mut() {
                                    if choice.finish_reason.is_none() {
                                        choice.finish_reason = Some(stop_reason.clone());
                                    }
                                }
                            }
                            if let Some(usage) = usage {
                                let usage = LLMTokenUsage {
                                    prompt_tokens: usage.input_tokens,
                                    completion_tokens: usage.output_tokens,
                                    total_tokens: usage.input_tokens + usage.output_tokens,
                                    prompt_tokens_details: Some(PromptTokensDetails {
                                        input_tokens: usage.input_tokens,
                                        output_tokens: usage.output_tokens,
                                        cache_read_input_tokens: usage
                                            .cache_read_input_tokens
                                            .unwrap_or(0),
                                        cache_write_input_tokens: usage
                                            .cache_creation_input_tokens
                                            .unwrap_or(0),
                                    }),
                                };

                                stream_channel_tx
                                    .send(GenerationDelta::Usage {
                                        usage: usage.clone(),
                                    })
                                    .await
                                    .map_err(|e| {
                                        AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                                            e.to_string(),
                                        ))
                                    })?;
                                completion_response.usage = Some(usage);
                            }
                        }

                        _ => {}
                    }
                }
                Err(_) => {
                    // We don't want to fail the entire stream if we can't parse one message
                    // Just log the error and continue
                }
            }
        }
    }

    if let Some(choice) = choices.get_mut(&0) {
        choice.message.content = LLMMessageContent::List(contents);
    }

    completion_response.choices = choices
        .into_iter()
        .sorted_by(|(index, _), (other_index, _)| index.cmp(other_index))
        .map(|(_, choice)| choice)
        .collect();

    Ok(completion_response)
}
