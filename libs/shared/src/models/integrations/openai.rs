use crate::models::error::{AgentError, BadRequestErrorMessage};
use crate::models::llm::{
    GenerationDelta, GenerationDeltaToolUse, LLMChoice, LLMCompletionResponse,
    LLMCompletionStreamResponse, LLMMessage, LLMMessageContent, LLMMessageTypedContent,
    LLMTokenUsage, LLMTool,
};
use futures_util::{Stream, StreamExt};
use reqwest_middleware::ClientBuilder;
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;
use uuid::Uuid;

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1/chat/completions";

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct OpenAIConfig {
    pub api_endpoint: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub enum OpenAIModel {
    // Reasoning Models
    #[serde(rename = "o3-2025-04-16")]
    O3,
    #[serde(rename = "o4-mini-2025-04-16")]
    O4Mini,

    #[default]
    #[serde(rename = "gpt-5-2025-08-07")]
    GPT5,
    #[serde(rename = "gpt-5-mini-2025-08-07")]
    GPT5Mini,
    #[serde(rename = "gpt-5-nano-2025-08-07")]
    GPT5Nano,
}

impl OpenAIModel {
    pub fn from_string(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.to_string()))
            .map_err(|_| "Failed to deserialize OpenAI model".to_string())
    }
}

impl std::fmt::Display for OpenAIModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenAIModel::O3 => write!(f, "o3-2025-04-16"),

            OpenAIModel::O4Mini => write!(f, "o4-mini-2025-04-16"),

            OpenAIModel::GPT5Nano => write!(f, "gpt-5-nano-2025-08-07"),
            OpenAIModel::GPT5Mini => write!(f, "gpt-5-mini-2025-08-07"),
            OpenAIModel::GPT5 => write!(f, "gpt-5-2025-08-07"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OpenAIInput {
    pub model: OpenAIModel,
    pub messages: Vec<LLMMessage>,
    pub max_tokens: u32,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<serde_json::Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<LLMTool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<OpenAIReasoningEffort>,
}

impl OpenAIInput {
    pub fn is_reasoning_model(&self) -> bool {
        matches!(self.model, |OpenAIModel::O3| OpenAIModel::O4Mini
            | OpenAIModel::GPT5
            | OpenAIModel::GPT5Mini
            | OpenAIModel::GPT5Nano)
    }

    pub fn is_standard_model(&self) -> bool {
        !self.is_reasoning_model()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub enum OpenAIReasoningEffort {
    #[serde(rename = "minimal")]
    Minimal,
    #[serde(rename = "low")]
    Low,
    #[default]
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OpenAITool {
    pub r#type: String,
    pub function: OpenAIToolFunction,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OpenAIToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl From<LLMTool> for OpenAITool {
    fn from(tool: LLMTool) -> Self {
        OpenAITool {
            r#type: "function".to_string(),
            function: OpenAIToolFunction {
                name: tool.name,
                description: tool.description,
                parameters: tool.input_schema,
            },
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAIOutput {
    pub model: String,
    pub object: String,
    pub choices: Vec<OpenAILLMChoice>,
    pub created: u64,
    pub usage: Option<LLMTokenUsage>,
    pub id: String,
}

impl From<OpenAIOutput> for LLMCompletionResponse {
    fn from(val: OpenAIOutput) -> Self {
        LLMCompletionResponse {
            model: val.model,
            object: val.object,
            choices: val.choices.into_iter().map(OpenAILLMChoice::into).collect(),
            created: val.created,
            usage: val.usage,
            id: val.id,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAILLMChoice {
    pub finish_reason: Option<String>,
    pub index: u32,
    pub message: OpenAILLMMessage,
}

impl From<OpenAILLMChoice> for LLMChoice {
    fn from(val: OpenAILLMChoice) -> Self {
        LLMChoice {
            finish_reason: val.finish_reason,
            index: val.index,
            message: val.message.into(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAILLMMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<OpenAILLMMessageToolCall>>,
}
impl From<OpenAILLMMessage> for LLMMessage {
    fn from(val: OpenAILLMMessage) -> Self {
        LLMMessage {
            role: val.role,
            content: match val.tool_calls {
                None => LLMMessageContent::String(val.content.unwrap_or_default()),
                Some(tool_calls) => LLMMessageContent::List(
                    std::iter::once(LLMMessageTypedContent::Text {
                        text: val.content.unwrap_or_default(),
                    })
                    .chain(tool_calls.into_iter().map(|tool_call| {
                        LLMMessageTypedContent::ToolCall {
                            id: tool_call.id,
                            name: tool_call.function.name,
                            args: match serde_json::from_str(&tool_call.function.arguments) {
                                Ok(args) => args,
                                Err(_) => {
                                    return LLMMessageTypedContent::Text {
                                        text: String::from("Error parsing tool call arguments"),
                                    };
                                }
                            },
                        }
                    }))
                    .collect(),
                ),
            },
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAILLMMessageToolCall {
    pub id: String,
    pub r#type: String,
    pub function: OpenAILLMMessageToolCallFunction,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAILLMMessageToolCallFunction {
    pub arguments: String,
    pub name: String,
}

pub struct OpenAI {}

impl OpenAI {
    pub async fn chat_completions_stream(
        config: &OpenAIConfig,
        input: OpenAIInput,
    ) -> Result<impl Stream<Item = Result<Vec<LLMCompletionStreamResponse>, AgentError>>, AgentError>
    {
        let model = serde_json::to_string(&input.model)
            .map_err(|_| AgentError::InternalError)?
            .trim_matches('"')
            .to_string();

        let mut payload = json!({
            "model": model,
            "messages": input.messages,
            "max_completion_tokens": input.max_tokens,
            "stream": true,
            "stream_options":{
                "include_usage": true
            }
        });

        if input.is_reasoning_model() {
            if let Some(reasoning_effort) = input.reasoning_effort {
                payload["reasoning_effort"] = json!(reasoning_effort);
            } else {
                payload["reasoning_effort"] = json!(OpenAIReasoningEffort::Medium);
            }
        }

        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        let api_endpoint = config.api_endpoint.as_ref().map_or(DEFAULT_BASE_URL, |v| v);
        let api_key = config.api_key.as_ref().map_or("", |v| v);

        // Send the POST request
        let response = client
            .post(api_endpoint)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&payload).map_err(|_| AgentError::InternalError)?)
            .send()
            .await;

        if response.is_err() {
            return Err(AgentError::InternalError);
        }

        let response = response.map_err(|_| AgentError::InternalError)?;

        if !response.status().is_success() {
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                response.status().to_string(),
            )));
        }

        let stream = response.bytes_stream().map(|chunk| {
            chunk
                .map_err(|_| AgentError::InternalError)
                .and_then(|bytes| {
                    std::str::from_utf8(&bytes)
                        .map_err(|_| AgentError::InternalError)
                        .map(|text| {
                            text.split("\n\n")
                                .filter(|event| event.starts_with("data: "))
                                .filter_map(|event| {
                                    event.strip_prefix("data: ").and_then(|json_str| {
                                        serde_json::from_str::<LLMCompletionStreamResponse>(
                                            json_str,
                                        )
                                        .ok()
                                    })
                                })
                                .collect::<Vec<LLMCompletionStreamResponse>>()
                        })
                })
        });

        Ok(stream)
    }

    pub async fn chat(
        config: &OpenAIConfig,
        input: OpenAIInput,
    ) -> Result<LLMCompletionResponse, AgentError> {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        let model = serde_json::to_string(&input.model)
            .map_err(|_| AgentError::InternalError)?
            .trim_matches('"')
            .to_string();

        // Replace deprecated max_tokens with max_completion_tokens
        let mut payload = json!({
            "model": model,
            "messages": input.messages,
            "max_completion_tokens": input.max_tokens,
            "stream": false,
        });

        if input.is_reasoning_model() {
            if let Some(reasoning_effort) = input.reasoning_effort {
                payload["reasoning_effort"] = json!(reasoning_effort);
            } else {
                payload["reasoning_effort"] = json!(OpenAIReasoningEffort::Medium);
            }
        } else {
            payload["temperature"] = json!(0);
        }

        if let Some(tools) = input.tools {
            let openai_tools: Vec<OpenAITool> = tools.into_iter().map(|t| t.into()).collect();
            payload["tools"] = json!(openai_tools);
        }

        if let Some(schema) = input.json {
            payload["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {
                    "strict": true,
                    "schema": schema,
                    "name": "my-schema"
                }
            });
        }

        let api_endpoint = config.api_endpoint.as_ref().map_or(DEFAULT_BASE_URL, |v| v);
        let api_key = config.api_key.as_ref().map_or("", |v| v);

        // Send the POST request
        let response = client
            .post(api_endpoint)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&payload).map_err(|e| {
                AgentError::BadRequest(BadRequestErrorMessage::ApiError(e.to_string()))
            })?)
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

        match response.json::<Value>().await {
            Ok(json) => match serde_json::from_value::<OpenAIOutput>(json.clone()) {
                Ok(json_response) => Ok(json_response.into()),
                Err(e) => Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                    e.to_string(),
                ))),
            },
            Err(e) => Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                e.to_string(),
            ))),
        }
    }

    pub async fn chat_stream_v2(
        config: &OpenAIConfig,
        stream_channel_tx: tokio::sync::mpsc::Sender<GenerationDelta>,
        input: OpenAIInput,
    ) -> Result<LLMCompletionResponse, AgentError> {
        let model = serde_json::to_string(&input.model)
            .map_err(|_| AgentError::InternalError)?
            .trim_matches('"')
            .to_string();

        // Replace deprecated max_tokens with max_completion_tokens
        let mut payload = json!({
            "model": model,
            "messages": input.messages,
            "max_completion_tokens": input.max_tokens,
            "stream": true,
            "stream_options":{
                "include_usage": true
            }
        });

        if input.is_reasoning_model() {
            if let Some(reasoning_effort) = input.reasoning_effort {
                payload["reasoning_effort"] = json!(reasoning_effort);
            } else {
                payload["reasoning_effort"] = json!(OpenAIReasoningEffort::Medium);
            }
        } else {
            payload["temperature"] = json!(0);
        }

        if let Some(tools) = input.tools {
            let openai_tools: Vec<OpenAITool> = tools.into_iter().map(|t| t.into()).collect();
            payload["tools"] = json!(openai_tools);
        }

        if let Some(schema) = input.json {
            payload["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {
                    "strict": true,
                    "schema": schema,
                    "name": "my-schema"
                }
            });
        }

        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        let api_endpoint = config.api_endpoint.as_ref().map_or(DEFAULT_BASE_URL, |v| v);
        let api_key = config.api_key.as_ref().map_or("", |v| v);

        // Send the POST request
        let response = client
            .post(api_endpoint)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
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
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                response.status().to_string(),
            )));
        }

        // Process the stream and convert to GenerationDelta
        process_openai_stream(response, model.clone(), stream_channel_tx)
            .await
            .map_err(|e| AgentError::BadRequest(BadRequestErrorMessage::ApiError(e.to_string())))
    }
}

/// Process OpenAI stream and convert to GenerationDelta format
async fn process_openai_stream(
    response: reqwest::Response,
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

    let mut stream = response.bytes_stream();
    let mut unparsed_data = String::new();
    let mut current_tool_calls: std::collections::HashMap<usize, (String, String, String)> =
        std::collections::HashMap::new();
    let mut accumulated_content = String::new();
    let mut finish_reason: Option<String> = None;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            let error_message = format!("Failed to read stream chunk from OpenAI API: {e}");
            AgentError::BadRequest(BadRequestErrorMessage::ApiError(error_message))
        })?;

        let text = std::str::from_utf8(&chunk).map_err(|e| {
            let error_message = format!("Failed to parse UTF-8 from OpenAI response: {e}");
            AgentError::BadRequest(BadRequestErrorMessage::ApiError(error_message))
        })?;

        unparsed_data.push_str(text);

        while let Some(line_end) = unparsed_data.find('\n') {
            let line = unparsed_data[..line_end].to_string();
            unparsed_data = unparsed_data[line_end + 1..].to_string();

            if line.trim().is_empty() {
                continue;
            }

            if !line.starts_with("data: ") {
                continue;
            }

            let json_str = &line[6..];
            if json_str == "[DONE]" {
                continue;
            }

            match serde_json::from_str::<ChatCompletionStreamResponse>(json_str) {
                Ok(stream_response) => {
                    // Update completion response metadata
                    if completion_response.id.is_empty() {
                        completion_response.id = stream_response.id.clone();
                        completion_response.model = stream_response.model.clone();
                        completion_response.object = stream_response.object.clone();
                        completion_response.created = stream_response.created;
                    }

                    // Process choices
                    for choice in &stream_response.choices {
                        if let Some(content) = &choice.delta.content {
                            // Send content delta
                            stream_channel_tx
                                .send(GenerationDelta::Content {
                                    content: content.clone(),
                                })
                                .await
                                .map_err(|e| {
                                    AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                                        e.to_string(),
                                    ))
                                })?;
                            accumulated_content.push_str(content);
                        }

                        // Handle tool calls
                        if let Some(tool_calls) = &choice.delta.tool_calls {
                            for tool_call in tool_calls {
                                let index = tool_call.index;

                                // Initialize or update tool call
                                let entry = current_tool_calls.entry(index).or_insert((
                                    String::new(),
                                    String::new(),
                                    String::new(),
                                ));

                                if let Some(id) = &tool_call.id {
                                    entry.0 = id.clone();
                                }
                                if let Some(function) = &tool_call.function {
                                    if let Some(name) = &function.name {
                                        entry.1 = name.clone();
                                    }
                                    if let Some(args) = &function.arguments {
                                        entry.2.push_str(args);
                                    }
                                }

                                // Send tool use delta
                                stream_channel_tx
                                    .send(GenerationDelta::ToolUse {
                                        tool_use: GenerationDeltaToolUse {
                                            id: tool_call.id.clone(),
                                            name: tool_call
                                                .function
                                                .as_ref()
                                                .and_then(|f| f.name.clone())
                                                .and_then(|n| {
                                                    if n.is_empty() { None } else { Some(n) }
                                                }),
                                            input: tool_call
                                                .function
                                                .as_ref()
                                                .and_then(|f| f.arguments.clone()),
                                            index,
                                        },
                                    })
                                    .await
                                    .map_err(|e| {
                                        AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                                            e.to_string(),
                                        ))
                                    })?;
                            }
                        }

                        if let Some(reason) = &choice.finish_reason {
                            finish_reason = Some(match reason {
                                FinishReason::Stop => "stop".to_string(),
                                FinishReason::Length => "length".to_string(),
                                FinishReason::ContentFilter => "content_filter".to_string(),
                                FinishReason::ToolCalls => "tool_calls".to_string(),
                            });
                        }
                    }

                    // Update usage if available
                    if let Some(usage) = &stream_response.usage {
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
                        completion_response.usage = Some(usage.clone());
                    }
                }
                Err(e) => {
                    eprintln!("Error parsing response: {}", e);
                }
            }
        }
    }

    // Build final response
    let mut message_content = vec![];

    if !accumulated_content.is_empty() {
        message_content.push(LLMMessageTypedContent::Text {
            text: accumulated_content,
        });
    }

    for (_, (id, name, args)) in current_tool_calls {
        if let Ok(parsed_args) = serde_json::from_str(&args) {
            message_content.push(LLMMessageTypedContent::ToolCall {
                id,
                name,
                args: parsed_args,
            });
        }
    }

    completion_response.choices = vec![LLMChoice {
        finish_reason,
        index: 0,
        message: LLMMessage {
            role: "assistant".to_string(),
            content: if message_content.is_empty() {
                LLMMessageContent::String(String::new())
            } else if message_content.len() == 1
                && matches!(&message_content[0], LLMMessageTypedContent::Text { .. })
            {
                if let LLMMessageTypedContent::Text { text } = &message_content[0] {
                    LLMMessageContent::String(text.clone())
                } else {
                    LLMMessageContent::List(message_content)
                }
            } else {
                LLMMessageContent::List(message_content)
            },
        },
    }];

    Ok(completion_response)
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    Developer,
    User,
    #[default]
    Assistant,
    Tool,
    // Function,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::System => write!(f, "system"),
            Role::Developer => write!(f, "developer"),
            Role::User => write!(f, "user"),
            Role::Assistant => write!(f, "assistant"),
            Role::Tool => write!(f, "tool"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<StopSequence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ChatCompletionContext>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionContext {
    pub scratchpad: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: Option<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    // TODO: check if this is needed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<LLMTokenUsage>,
}

impl ChatMessage {
    pub fn last_server_message(messages: &[ChatMessage]) -> Option<&ChatMessage> {
        messages
            .iter()
            .rev()
            .find(|message| message.role != Role::User && message.role != Role::Tool)
    }

    pub fn to_xml(&self) -> String {
        match &self.content {
            Some(MessageContent::String(s)) => {
                format!("<message role=\"{}\">{}</message>", self.role, s)
            }
            Some(MessageContent::Array(parts)) => parts
                .iter()
                .map(|part| {
                    format!(
                        "<message role=\"{}\" type=\"{}\">{}</message>",
                        self.role,
                        part.r#type,
                        part.text.clone().unwrap_or_default()
                    )
                })
                .collect::<Vec<String>>()
                .join("\n"),
            None => String::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum MessageContent {
    String(String),
    Array(Vec<ContentPart>),
}

impl MessageContent {
    pub fn inject_checkpoint_id(&self, checkpoint_id: Uuid) -> Self {
        match self {
            MessageContent::String(s) => MessageContent::String(format!(
                "<checkpoint_id>{checkpoint_id}</checkpoint_id>\n{s}"
            )),
            MessageContent::Array(parts) => MessageContent::Array(
                std::iter::once(ContentPart {
                    r#type: "text".to_string(),
                    text: Some(format!("<checkpoint_id>{checkpoint_id}</checkpoint_id>")),
                    image_url: None,
                })
                .chain(parts.iter().cloned())
                .collect(),
            ),
        }
    }

    pub fn extract_checkpoint_id(&self) -> Option<Uuid> {
        match self {
            MessageContent::String(s) => s
                .rfind("<checkpoint_id>")
                .and_then(|start| {
                    s[start..]
                        .find("</checkpoint_id>")
                        .map(|end| (start + "<checkpoint_id>".len(), start + end))
                })
                .and_then(|(start, end)| Uuid::parse_str(&s[start..end]).ok()),
            MessageContent::Array(parts) => parts.iter().rev().find_map(|part| {
                part.text.as_deref().and_then(|text| {
                    text.rfind("<checkpoint_id>")
                        .and_then(|start| {
                            text[start..]
                                .find("</checkpoint_id>")
                                .map(|end| (start + "<checkpoint_id>".len(), start + end))
                        })
                        .and_then(|(start, end)| Uuid::parse_str(&text[start..end]).ok())
                })
            }),
        }
    }
}

impl std::fmt::Display for MessageContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageContent::String(s) => write!(f, "{s}"),
            MessageContent::Array(parts) => {
                let text_parts: Vec<String> =
                    parts.iter().filter_map(|part| part.text.clone()).collect();
                write!(f, "{}", text_parts.join("\n"))
            }
        }
    }
}
impl Default for MessageContent {
    fn default() -> Self {
        MessageContent::String(String::new())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ContentPart {
    pub r#type: String,
    pub text: Option<String>,
    pub image_url: Option<ImageUrl>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ResponseFormat {
    pub r#type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum StopSequence {
    String(String),
    Array(Vec<String>),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Tool {
    pub r#type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolChoice {
    Auto,
    Required,
    Object(ToolChoiceObject),
}

impl Serialize for ToolChoice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ToolChoice::Auto => serializer.serialize_str("auto"),
            ToolChoice::Required => serializer.serialize_str("required"),
            ToolChoice::Object(obj) => obj.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ToolChoice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ToolChoiceVisitor;

        impl<'de> serde::de::Visitor<'de> for ToolChoiceVisitor {
            type Value = ToolChoice;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("string or object")
            }

            fn visit_str<E>(self, value: &str) -> Result<ToolChoice, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "auto" => Ok(ToolChoice::Auto),
                    "required" => Ok(ToolChoice::Required),
                    _ => Err(serde::de::Error::unknown_variant(
                        value,
                        &["auto", "required"],
                    )),
                }
            }

            fn visit_map<M>(self, map: M) -> Result<ToolChoice, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let obj = ToolChoiceObject::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                Ok(ToolChoice::Object(obj))
            }
        }

        deserializer.deserialize_any(ToolChoiceVisitor)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ToolChoiceObject {
    pub r#type: String,
    pub function: FunctionChoice,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FunctionChoice {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionChoice>,
    pub usage: LLMTokenUsage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionChoice {
    pub index: usize,
    pub message: ChatMessage,
    pub logprobs: Option<LogProbs>,
    pub finish_reason: FinishReason,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ContentFilter,
    ToolCalls,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LogProbs {
    pub content: Option<Vec<LogProbContent>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LogProbContent {
    pub token: String,
    pub logprob: f32,
    pub bytes: Option<Vec<u8>>,
    pub top_logprobs: Option<Vec<TokenLogprob>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TokenLogprob {
    pub token: String,
    pub logprob: f32,
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionStreamResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionStreamChoice>,
    pub usage: Option<LLMTokenUsage>,
    pub metadata: Option<serde_json::Value>,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionStreamChoice {
    pub index: usize,
    pub delta: ChatMessageDelta,
    pub finish_reason: Option<FinishReason>,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatMessageDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub r#type: Option<String>,
    pub function: Option<FunctionCallDelta>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FunctionCallDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

impl From<LLMMessage> for ChatMessage {
    fn from(llm_message: LLMMessage) -> Self {
        let role = match llm_message.role.as_str() {
            "system" => Role::System,
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "tool" => Role::Tool,
            // "function" => Role::Function,
            "developer" => Role::Developer,
            _ => Role::User, // Default to user for unknown roles
        };

        let (content, tool_calls) = match llm_message.content {
            LLMMessageContent::String(text) => (Some(MessageContent::String(text)), None),
            LLMMessageContent::List(items) => {
                let mut text_parts = Vec::new();
                let mut tool_call_parts = Vec::new();

                for item in items {
                    match item {
                        LLMMessageTypedContent::Text { text } => {
                            text_parts.push(ContentPart {
                                r#type: "text".to_string(),
                                text: Some(text),
                                image_url: None,
                            });
                        }
                        LLMMessageTypedContent::ToolCall { id, name, args } => {
                            tool_call_parts.push(ToolCall {
                                id,
                                r#type: "function".to_string(),
                                function: FunctionCall {
                                    name,
                                    arguments: args.to_string(),
                                },
                            });
                        }
                        LLMMessageTypedContent::ToolResult { content, .. } => {
                            text_parts.push(ContentPart {
                                r#type: "text".to_string(),
                                text: Some(content),
                                image_url: None,
                            });
                        }
                    }
                }

                let content = if !text_parts.is_empty() {
                    Some(MessageContent::Array(text_parts))
                } else {
                    None
                };

                let tool_calls = if !tool_call_parts.is_empty() {
                    Some(tool_call_parts)
                } else {
                    None
                };

                (content, tool_calls)
            }
        };

        ChatMessage {
            role,
            content,
            name: None, // LLMMessage doesn't have a name field
            tool_calls,
            tool_call_id: None, // LLMMessage doesn't have a tool_call_id field
            usage: None,
        }
    }
}

impl From<ChatMessage> for LLMMessage {
    fn from(chat_message: ChatMessage) -> Self {
        let mut content_parts = Vec::new();

        // Handle text content
        match chat_message.content {
            Some(MessageContent::String(s)) => {
                if !s.is_empty() {
                    content_parts.push(LLMMessageTypedContent::Text { text: s });
                }
            }
            Some(MessageContent::Array(parts)) => {
                for part in parts {
                    content_parts.push(LLMMessageTypedContent::Text {
                        text: part.text.unwrap_or_default(),
                    });
                }
            }
            None => {}
        }

        // Handle tool calls
        if let Some(tool_calls) = chat_message.tool_calls {
            for tool_call in tool_calls {
                let args = serde_json::from_str(&tool_call.function.arguments).unwrap_or(json!({}));
                content_parts.push(LLMMessageTypedContent::ToolCall {
                    id: tool_call.id,
                    name: tool_call.function.name,
                    args,
                });
            }
        }

        LLMMessage {
            role: chat_message.role.to_string(),
            content: if content_parts.is_empty() {
                LLMMessageContent::String(String::new())
            } else if content_parts.len() == 1 {
                match &content_parts[0] {
                    LLMMessageTypedContent::Text { text } => {
                        LLMMessageContent::String(text.clone())
                    }
                    _ => LLMMessageContent::List(content_parts),
                }
            } else {
                LLMMessageContent::List(content_parts)
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub enum AgentModel {
    #[serde(rename = "smart")]
    #[default]
    Smart,
    #[serde(rename = "eco")]
    Eco,
    #[serde(rename = "recovery")]
    Recovery,
}

impl std::fmt::Display for AgentModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentModel::Smart => write!(f, "smart"),
            AgentModel::Eco => write!(f, "eco"),
            AgentModel::Recovery => write!(f, "recovery"),
        }
    }
}

impl From<String> for AgentModel {
    fn from(value: String) -> Self {
        match value.as_str() {
            "eco" => AgentModel::Eco,
            "recovery" => AgentModel::Recovery,
            _ => AgentModel::Smart,
        }
    }
}

impl ChatCompletionRequest {
    pub fn new(
        model: String,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        stream: Option<bool>,
    ) -> Self {
        Self {
            model,
            messages,
            frequency_penalty: None,
            logit_bias: None,
            logprobs: None,
            max_tokens: None,
            n: None,
            presence_penalty: None,
            response_format: None,
            seed: None,
            stop: None,
            stream,
            temperature: None,
            top_p: None,
            tools,
            tool_choice: None,
            user: None,
            context: None,
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum ToolCallResultStatus {
    Success,
    Error,
    Cancelled,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ToolCallResult {
    pub call: ToolCall,
    pub result: String,
    pub status: ToolCallResultStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallResultProgress {
    pub id: Uuid,
    pub message: String,
}

impl From<GenerationDelta> for ChatMessageDelta {
    fn from(delta: GenerationDelta) -> Self {
        match delta {
            GenerationDelta::Content { content } => ChatMessageDelta {
                role: Some(Role::Assistant),
                content: Some(content),
                tool_calls: None,
            },
            GenerationDelta::Thinking { thinking: _ } => ChatMessageDelta {
                role: Some(Role::Assistant),
                content: None,
                tool_calls: None,
            },
            GenerationDelta::ToolUse { tool_use } => ChatMessageDelta {
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
            },
            _ => ChatMessageDelta {
                role: Some(Role::Assistant),
                content: None,
                tool_calls: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_basic_request() {
        let request = ChatCompletionRequest {
            model: AgentModel::Smart.to_string(),
            messages: vec![
                ChatMessage {
                    role: Role::System,
                    content: Some(MessageContent::String(
                        "You are a helpful assistant.".to_string(),
                    )),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    usage: None,
                },
                ChatMessage {
                    role: Role::User,
                    content: Some(MessageContent::String("Hello!".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    usage: None,
                },
            ],
            frequency_penalty: None,
            logit_bias: None,
            logprobs: None,
            max_tokens: Some(100),
            n: None,
            presence_penalty: None,
            response_format: None,
            seed: None,
            stop: None,
            stream: None,
            temperature: Some(0.7),
            top_p: None,
            tools: None,
            tool_choice: None,
            user: None,
            context: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"model\":\"smart\""));
        assert!(json.contains("\"messages\":["));
        assert!(json.contains("\"role\":\"system\""));
        assert!(json.contains("\"content\":\"You are a helpful assistant.\""));
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"Hello!\""));
        assert!(json.contains("\"max_tokens\":100"));
        assert!(json.contains("\"temperature\":0.7"));
    }

    #[test]
    fn test_deserialize_response() {
        let json = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "smart",
            "system_fingerprint": "fp_123abc",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help you today?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 9,
                "completion_tokens": 12,
                "total_tokens": 21
            }
        }"#;

        let response: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.id, "chatcmpl-123");
        assert_eq!(response.object, "chat.completion");
        assert_eq!(response.created, 1677652288);
        assert_eq!(response.model, AgentModel::Smart.to_string());
        assert_eq!(response.system_fingerprint, Some("fp_123abc".to_string()));

        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].index, 0);
        assert_eq!(response.choices[0].message.role, Role::Assistant);

        match &response.choices[0].message.content {
            Some(MessageContent::String(content)) => {
                assert_eq!(content, "Hello! How can I help you today?");
            }
            _ => panic!("Expected string content"),
        }

        assert_eq!(response.choices[0].finish_reason, FinishReason::Stop);
        assert_eq!(response.usage.prompt_tokens, 9);
        assert_eq!(response.usage.completion_tokens, 12);
        assert_eq!(response.usage.total_tokens, 21);
    }

    #[test]
    fn test_tool_calls_request_response() {
        // Test a request with tools
        let tools_request = ChatCompletionRequest {
            model: AgentModel::Smart.to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(
                    "What's the weather in San Francisco?".to_string(),
                )),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                usage: None,
            }],
            tools: Some(vec![Tool {
                r#type: "function".to_string(),
                function: FunctionDefinition {
                    name: "get_weather".to_string(),
                    description: Some("Get the current weather in a given location".to_string()),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "location": {
                                "type": "string",
                                "description": "The city and state, e.g. San Francisco, CA"
                            }
                        },
                        "required": ["location"]
                    }),
                },
            }]),
            tool_choice: Some(ToolChoice::Auto),
            max_tokens: Some(100),
            temperature: Some(0.7),
            frequency_penalty: None,
            logit_bias: None,
            logprobs: None,
            n: None,
            presence_penalty: None,
            response_format: None,
            seed: None,
            stop: None,
            stream: None,
            top_p: None,
            user: None,
            context: None,
        };

        let json = serde_json::to_string(&tools_request).unwrap();
        println!("Tool request JSON: {}", json);

        assert!(json.contains("\"tools\":["));
        assert!(json.contains("\"type\":\"function\""));
        assert!(json.contains("\"name\":\"get_weather\""));
        // Auto should be serialized as "auto" (string)
        assert!(json.contains("\"tool_choice\":\"auto\""));

        // Test response with tool calls
        let tool_response_json = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "eco",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_abc123",
                            "type": "function",
                            "function": {
                                "name": "get_weather",
                                "arguments": "{\"location\":\"San Francisco, CA\"}"
                            }
                        }
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 82,
                "completion_tokens": 17,
                "total_tokens": 99
            }
        }"#;

        let tool_response: ChatCompletionResponse =
            serde_json::from_str(tool_response_json).unwrap();
        assert_eq!(tool_response.choices[0].message.role, Role::Assistant);
        assert_eq!(tool_response.choices[0].message.content, None);
        assert!(tool_response.choices[0].message.tool_calls.is_some());

        let tool_calls = tool_response.choices[0]
            .message
            .tool_calls
            .as_ref()
            .unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_abc123");
        assert_eq!(tool_calls[0].r#type, "function");
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert_eq!(
            tool_calls[0].function.arguments,
            "{\"location\":\"San Francisco, CA\"}"
        );

        assert_eq!(
            tool_response.choices[0].finish_reason,
            FinishReason::ToolCalls,
        );
    }

    #[test]
    fn test_content_with_image() {
        let message_with_image = ChatMessage {
            role: Role::User,
            content: Some(MessageContent::Array(vec![
                ContentPart {
                    r#type: "text".to_string(),
                    text: Some("What's in this image?".to_string()),
                    image_url: None,
                },
                ContentPart {
                    r#type: "image_url".to_string(),
                    text: None,
                    image_url: Some(ImageUrl {
                        url: "data:image/jpeg;base64,/9j/4AAQSkZ...".to_string(),
                        detail: Some("low".to_string()),
                    }),
                },
            ])),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            usage: None,
        };

        let json = serde_json::to_string(&message_with_image).unwrap();
        println!("Serialized JSON: {}", json);

        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"What's in this image?\""));
        assert!(json.contains("\"type\":\"image_url\""));
        assert!(json.contains("\"url\":\"data:image/jpeg;base64,/9j/4AAQSkZ...\""));
        assert!(json.contains("\"detail\":\"low\""));
    }

    #[test]
    fn test_response_format() {
        let json_format_request = ChatCompletionRequest {
            model: AgentModel::Smart.to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(
                    "Generate a JSON object with name and age fields".to_string(),
                )),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                usage: None,
            }],
            response_format: Some(ResponseFormat {
                r#type: "json_object".to_string(),
            }),
            max_tokens: Some(100),
            temperature: None,
            frequency_penalty: None,
            logit_bias: None,
            logprobs: None,
            n: None,
            presence_penalty: None,
            seed: None,
            stop: None,
            stream: None,
            top_p: None,
            tools: None,
            tool_choice: None,
            user: None,
            context: None,
        };

        let json = serde_json::to_string(&json_format_request).unwrap();
        assert!(json.contains("\"response_format\":{\"type\":\"json_object\"}"));
    }

    #[test]
    fn test_llm_message_to_chat_message() {
        // Test simple string content
        let llm_message = LLMMessage {
            role: "user".to_string(),
            content: LLMMessageContent::String("Hello, world!".to_string()),
        };

        let chat_message = ChatMessage::from(llm_message);
        assert_eq!(chat_message.role, Role::User);
        match &chat_message.content {
            Some(MessageContent::String(text)) => assert_eq!(text, "Hello, world!"),
            _ => panic!("Expected string content"),
        }
        assert_eq!(chat_message.tool_calls, None);

        // Test tool call conversion
        let llm_message_with_tool = LLMMessage {
            role: "assistant".to_string(),
            content: LLMMessageContent::List(vec![LLMMessageTypedContent::ToolCall {
                id: "call_123".to_string(),
                name: "get_weather".to_string(),
                args: serde_json::json!({"location": "San Francisco"}),
            }]),
        };

        let chat_message = ChatMessage::from(llm_message_with_tool);
        assert_eq!(chat_message.role, Role::Assistant);
        assert_eq!(chat_message.content, None); // No text content
        assert!(chat_message.tool_calls.is_some());

        let tool_calls = chat_message.tool_calls.unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_123");
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert!(tool_calls[0].function.arguments.contains("San Francisco"));

        // Test mixed content
        let llm_message_mixed = LLMMessage {
            role: "assistant".to_string(),
            content: LLMMessageContent::List(vec![
                LLMMessageTypedContent::Text {
                    text: "The weather is:".to_string(),
                },
                LLMMessageTypedContent::ToolCall {
                    id: "call_456".to_string(),
                    name: "get_weather".to_string(),
                    args: serde_json::json!({"location": "New York"}),
                },
            ]),
        };

        let chat_message = ChatMessage::from(llm_message_mixed);
        assert_eq!(chat_message.role, Role::Assistant);

        match &chat_message.content {
            Some(MessageContent::Array(parts)) => {
                assert_eq!(parts.len(), 1);
                assert_eq!(parts[0].r#type, "text");
                assert_eq!(parts[0].text, Some("The weather is:".to_string()));
            }
            _ => panic!("Expected array content"),
        }

        let tool_calls = chat_message.tool_calls.unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_456");
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert!(tool_calls[0].function.arguments.contains("New York"));
    }
}
