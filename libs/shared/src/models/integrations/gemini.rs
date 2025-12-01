use crate::models::error::{AgentError, BadRequestErrorMessage};
use crate::models::llm::{
    GenerationDelta, GenerationDeltaToolUse, LLMChoice, LLMCompletionResponse, LLMMessage,
    LLMMessageContent, LLMMessageTypedContent, LLMTokenUsage, LLMTool,
};
use futures_util::StreamExt;
use reqwest_middleware::ClientBuilder;
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct GeminiConfig {
    pub api_endpoint: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub enum GeminiModel {
    #[default]
    #[serde(rename = "gemini-3-pro-preview")]
    Gemini3Pro,
    #[serde(rename = "gemini-2.5-pro")]
    Gemini25Pro,
    #[serde(rename = "gemini-2.5-flash")]
    Gemini25Flash,
    #[serde(rename = "gemini-2.5-flash-lite")]
    Gemini25FlashLite,
    #[serde(rename = "gemini-2.0-flash")]
    Gemini20Flash,
    #[serde(rename = "gemini-2.0-flash-lite")]
    Gemini20FlashLite,
}

impl std::fmt::Display for GeminiModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeminiModel::Gemini3Pro => write!(f, "gemini-3-pro-preview"),
            GeminiModel::Gemini25Pro => write!(f, "gemini-2.5-pro"),
            GeminiModel::Gemini25Flash => write!(f, "gemini-2.5-flash"),
            GeminiModel::Gemini25FlashLite => write!(f, "gemini-2.5-flash-lite"),
            GeminiModel::Gemini20Flash => write!(f, "gemini-2.0-flash"),
            GeminiModel::Gemini20FlashLite => write!(f, "gemini-2.0-flash-lite"),
        }
    }
}

impl GeminiModel {
    pub fn from_string(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.to_string()))
            .map_err(|_| "Failed to deserialize Gemini model".to_string())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GeminiInput {
    pub model: GeminiModel,
    pub messages: Vec<LLMMessage>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<LLMTool>>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GeminiRequest {
    pub contents: Vec<GeminiContent>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiTool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiSystemInstruction>, // checked

    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>, // checked
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum GeminiRole {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "model")]
    Model,
}

impl std::fmt::Display for GeminiRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeminiRole::User => write!(f, "user"),
            GeminiRole::Model => write!(f, "model"),
        }
    }
}

impl GeminiRole {
    pub fn from_string(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.to_string()))
            .map_err(|_| "Failed to deserialize Gemini role".to_string())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeminiContent {
    pub role: GeminiRole,
    #[serde(default)]
    pub parts: Vec<GeminiPart>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum GeminiPart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
    InlineData {
        #[serde(rename = "inlineData")]
        inline_data: GeminiInlineData,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeminiFunctionCall {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeminiFunctionResponse {
    pub id: String,
    pub name: String,
    pub response: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeminiInlineData {
    pub mime_type: String,
    pub data: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GeminiSystemInstruction {
    pub parts: Vec<GeminiPart>, // checked
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GeminiTool {
    pub function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GeminiFunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters_json_schema: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GeminiGenerationConfig {
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub candidate_count: Option<u32>,
}

// Gemini API Response Structs

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeminiResponse {
    pub candidates: Option<Vec<GeminiCandidate>>,
    pub usage_metadata: Option<GeminiUsageMetadata>,
    pub model_version: Option<String>,
    pub response_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeminiCandidate {
    pub content: Option<GeminiContent>,
    pub finish_reason: Option<String>,
    pub index: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeminiUsageMetadata {
    pub prompt_token_count: Option<u32>,
    pub cached_content_token_count: Option<u32>,
    pub candidates_token_count: Option<u32>,
    pub tool_use_prompt_token_count: Option<u32>,
    pub thoughts_token_count: Option<u32>,
    pub total_token_count: Option<u32>,
}

impl From<LLMMessage> for GeminiContent {
    fn from(message: LLMMessage) -> Self {
        let role = match message.role.as_str() {
            "assistant" | "model" => GeminiRole::Model,
            "user" | "tool" => GeminiRole::User,
            _ => GeminiRole::User,
        };

        let parts = match message.content {
            LLMMessageContent::String(text) => vec![GeminiPart::Text { text }],
            LLMMessageContent::List(items) => items
                .into_iter()
                .map(|item| match item {
                    LLMMessageTypedContent::Text { text } => GeminiPart::Text { text },

                    LLMMessageTypedContent::ToolCall { id, name, args } => {
                        GeminiPart::FunctionCall {
                            function_call: GeminiFunctionCall {
                                id: Some(id),
                                name,
                                args,
                            },
                        }
                    }

                    LLMMessageTypedContent::ToolResult { content, .. } => {
                        GeminiPart::Text { text: content }
                    }

                    LLMMessageTypedContent::Image { source } => GeminiPart::InlineData {
                        inline_data: GeminiInlineData {
                            mime_type: source.media_type,
                            data: source.data,
                        },
                    },
                })
                .collect(),
        };

        GeminiContent { role, parts }
    }
}

// Conversion from GeminiContent to LLMMessage
impl From<GeminiContent> for LLMMessage {
    fn from(content: GeminiContent) -> Self {
        let role = content.role.to_string();
        let mut message_content = Vec::new();

        for part in content.parts {
            match part {
                GeminiPart::Text { text } => {
                    message_content.push(LLMMessageTypedContent::Text { text });
                }
                GeminiPart::FunctionCall { function_call } => {
                    message_content.push(LLMMessageTypedContent::ToolCall {
                        id: function_call.id.unwrap_or_else(|| "".to_string()),
                        name: function_call.name,
                        args: function_call.args,
                    });
                }
                GeminiPart::FunctionResponse { function_response } => {
                    message_content.push(LLMMessageTypedContent::ToolResult {
                        tool_use_id: function_response.id,
                        content: function_response.response.to_string(),
                    });
                }
                //TODO: Add Image support
                _ => {}
            }
        }

        let content = if message_content.is_empty() {
            LLMMessageContent::String(String::new())
        } else if message_content.len() == 1 {
            match &message_content[0] {
                LLMMessageTypedContent::Text { text } => LLMMessageContent::String(text.clone()),
                _ => LLMMessageContent::List(message_content),
            }
        } else {
            LLMMessageContent::List(message_content)
        };

        LLMMessage { role, content }
    }
}

impl From<LLMTool> for GeminiFunctionDeclaration {
    fn from(tool: LLMTool) -> Self {
        GeminiFunctionDeclaration {
            name: tool.name,
            description: tool.description,
            parameters_json_schema: Some(tool.input_schema),
        }
    }
}

impl From<Vec<LLMTool>> for GeminiTool {
    fn from(tools: Vec<LLMTool>) -> Self {
        GeminiTool {
            function_declarations: tools.into_iter().map(|t| t.into()).collect(),
        }
    }
}

impl From<GeminiResponse> for LLMCompletionResponse {
    fn from(response: GeminiResponse) -> Self {
        let usage = response.usage_metadata.map(|u| LLMTokenUsage {
            prompt_tokens: u.prompt_token_count.unwrap_or(0),
            completion_tokens: u.candidates_token_count.unwrap_or(0),
            total_tokens: u.total_token_count.unwrap_or(0),
            prompt_tokens_details: None,
        });

        let choices = response
            .candidates
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, candidate)| {
                let message = candidate
                    .content
                    .map(|c| c.into())
                    .unwrap_or_else(|| LLMMessage {
                        role: "model".to_string(),
                        content: LLMMessageContent::String(String::new()),
                    });

                let has_tool_calls = match &message.content {
                    LLMMessageContent::List(items) => items
                        .iter()
                        .any(|item| matches!(item, LLMMessageTypedContent::ToolCall { .. })),
                    _ => false,
                };

                let finish_reason = if has_tool_calls {
                    Some("tool_calls".to_string())
                } else {
                    candidate.finish_reason.map(|s| s.to_lowercase())
                };

                LLMChoice {
                    finish_reason,
                    index: index as u32,
                    message,
                }
            })
            .collect();

        LLMCompletionResponse {
            // Use model_version from the response, with fallback
            model: response
                .model_version
                .unwrap_or_else(|| "gemini".to_string()),
            object: "chat.completion".to_string(),
            choices,
            created: chrono::Utc::now().timestamp_millis() as u64,
            usage,
            id: response
                .response_id
                .unwrap_or_else(|| "unknown".to_string()),
        }
    }
}

pub struct Gemini {}

impl Gemini {
    pub async fn chat(
        config: &GeminiConfig,
        input: GeminiInput,
    ) -> Result<LLMCompletionResponse, AgentError> {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        let (contents, system_instruction) = convert_messages_to_gemini(input.messages)?;

        let tools = input.tools.map(|t| vec![t.into()]);

        let payload = GeminiRequest {
            contents,
            tools,
            system_instruction,
            generation_config: Some(GeminiGenerationConfig {
                max_output_tokens: Some(input.max_tokens),
                temperature: Some(0.0),
                candidate_count: Some(1),
            }),
        };

        let api_endpoint = config.api_endpoint.as_ref().map_or(DEFAULT_BASE_URL, |v| v);
        let api_key = config.api_key.as_ref().map_or("", |v| v);

        let url = format!(
            "{}/models/{}:generateContent?key={}",
            api_endpoint, input.model, api_key
        );

        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| AgentError::BadRequest(BadRequestErrorMessage::ApiError(e.to_string())))?;

        if !response.status().is_success() {
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                format!(
                    "{}: {}",
                    response.status(),
                    response.text().await.unwrap_or_default()
                ),
            )));
        }

        // Log response body before attempting to decode
        let response_text = response.text().await.map_err(|e| {
            AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                "Failed to read response body: {}",
                e
            )))
        })?;

        let gemini_response: GeminiResponse =
            serde_json::from_str(&response_text).map_err(|e| {
                AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                    "Failed to deserialize Gemini response: {}. Response body: {}",
                    e, response_text
                )))
            })?;

        Ok(gemini_response.into())
    }

    pub async fn chat_stream(
        config: &GeminiConfig,
        stream_channel_tx: tokio::sync::mpsc::Sender<GenerationDelta>,
        input: GeminiInput,
    ) -> Result<LLMCompletionResponse, AgentError> {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        let (contents, system_instruction) = convert_messages_to_gemini(input.messages)?;

        let tools = input.tools.map(|t| vec![t.into()]);

        let payload = GeminiRequest {
            contents,
            tools,
            system_instruction,
            generation_config: Some(GeminiGenerationConfig {
                max_output_tokens: Some(input.max_tokens),
                temperature: Some(0.0),
                candidate_count: Some(1),
            }),
        };

        let api_endpoint = config.api_endpoint.as_ref().map_or(DEFAULT_BASE_URL, |v| v);
        let api_key = config.api_key.as_ref().map_or("", |v| v);

        let url = format!(
            "{}/models/{}:streamGenerateContent?key={}",
            api_endpoint, input.model, api_key
        );

        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| AgentError::BadRequest(BadRequestErrorMessage::ApiError(e.to_string())))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                format!("{}: {}", status, error_body),
            )));
        }

        process_gemini_stream(response, input.model.to_string(), stream_channel_tx).await
    }
}

fn convert_messages_to_gemini(
    messages: Vec<LLMMessage>,
) -> Result<(Vec<GeminiContent>, Option<GeminiSystemInstruction>), AgentError> {
    let mut contents = Vec::new();
    let mut system_parts = Vec::new();
    let mut tool_id_to_name = std::collections::HashMap::new();

    for message in messages {
        match message.role.as_str() {
            "system" => {
                if let LLMMessageContent::String(text) = message.content {
                    system_parts.push(GeminiPart::Text { text });
                }
            }
            _ => {
                let role = match message.role.as_str() {
                    "assistant" | "model" => GeminiRole::Model,
                    "user" | "tool" => GeminiRole::User,
                    _ => GeminiRole::User,
                };

                let mut parts = Vec::new();

                match message.content {
                    LLMMessageContent::String(text) => {
                        parts.push(GeminiPart::Text { text });
                    }
                    LLMMessageContent::List(items) => {
                        for item in items {
                            match item {
                                LLMMessageTypedContent::Text { text } => {
                                    parts.push(GeminiPart::Text { text });
                                }
                                LLMMessageTypedContent::ToolCall { id, name, args } => {
                                    tool_id_to_name.insert(id.clone(), name.clone());
                                    parts.push(GeminiPart::FunctionCall {
                                        function_call: GeminiFunctionCall {
                                            id: Some(id),
                                            name,
                                            args,
                                        },
                                    });
                                }
                                LLMMessageTypedContent::ToolResult {
                                    tool_use_id,
                                    content,
                                } => {
                                    let name = tool_id_to_name
                                        .get(&tool_use_id)
                                        .cloned()
                                        .unwrap_or_else(|| "unknown".to_string());

                                    // Gemini expects a JSON object for the response
                                    let response_json = serde_json::json!({ "result": content });

                                    parts.push(GeminiPart::FunctionResponse {
                                        function_response: GeminiFunctionResponse {
                                            id: tool_use_id,
                                            name,
                                            response: response_json,
                                        },
                                    });
                                }
                                LLMMessageTypedContent::Image { source } => {
                                    parts.push(GeminiPart::InlineData {
                                        inline_data: GeminiInlineData {
                                            mime_type: source.media_type,
                                            data: source.data,
                                        },
                                    });
                                }
                            }
                        }
                    }
                }

                contents.push(GeminiContent { role, parts });
            }
        }
    }

    let system_instruction = if system_parts.is_empty() {
        None
    } else {
        Some(GeminiSystemInstruction {
            parts: system_parts,
        })
    };

    Ok((contents, system_instruction))
}

async fn process_gemini_stream(
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
    let mut line_buffer = String::new();
    let mut json_accumulator = String::new();
    let mut brace_depth = 0;
    let mut in_object = false;
    let mut finish_reason = None;
    let mut message_content: Vec<LLMMessageTypedContent> = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                "Failed to read stream chunk: {}",
                e
            )))
        })?;

        let text = std::str::from_utf8(&chunk).map_err(|e| {
            AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                "Failed to parse UTF-8: {}",
                e
            )))
        })?;

        line_buffer.push_str(text);

        // Process complete lines from buffer
        while let Some(line_end) = line_buffer.find('\n') {
            let line = line_buffer[..line_end].trim().to_string();
            line_buffer = line_buffer[line_end + 1..].to_string();

            // Skip empty lines and array delimiters
            if line.is_empty() || line == "[" || line == "]" {
                continue;
            }

            // Track braces to detect complete JSON objects
            for ch in line.chars() {
                match ch {
                    '{' => {
                        brace_depth += 1;
                        in_object = true;
                    }
                    '}' => {
                        brace_depth -= 1;
                    }
                    _ => {}
                }
            }

            // Accumulate JSON lines
            if in_object {
                if !json_accumulator.is_empty() {
                    json_accumulator.push('\n');
                }
                json_accumulator.push_str(&line);
            }

            // When we reach depth 0, we have a complete JSON object
            if in_object && brace_depth == 0 {
                let mut json_str = json_accumulator.trim();
                if json_str.starts_with('[') {
                    json_str = json_str[1..].trim();
                }
                if json_str.ends_with(']') {
                    json_str = json_str[..json_str.len() - 1].trim();
                }
                let json_str = json_str.trim_matches(',').trim();

                // Try to parse the complete JSON object
                match serde_json::from_str::<GeminiResponse>(json_str) {
                    Ok(gemini_response) => {
                        // Process candidates
                        if let Some(candidates) = gemini_response.candidates {
                            for candidate in candidates {
                                if let Some(reason) = candidate.finish_reason {
                                    finish_reason = Some(reason);
                                }
                                if let Some(content) = candidate.content {
                                    for part in content.parts {
                                        match part {
                                            GeminiPart::Text { text } => {
                                                stream_channel_tx
                                                    .send(GenerationDelta::Content {
                                                        content: text.clone(),
                                                    })
                                                    .await
                                                    .map_err(|e| {
                                                        AgentError::BadRequest(
                                                            BadRequestErrorMessage::ApiError(
                                                                e.to_string(),
                                                            ),
                                                        )
                                                    })?;
                                                message_content
                                                    .push(LLMMessageTypedContent::Text { text });
                                            }
                                            GeminiPart::FunctionCall { function_call } => {
                                                let GeminiFunctionCall { id, name, args } =
                                                    function_call;

                                                let id = id
                                                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                                                let name_clone = name.clone();
                                                let args_clone = args.clone();
                                                stream_channel_tx
                                                    .send(GenerationDelta::ToolUse {
                                                        tool_use: GenerationDeltaToolUse {
                                                            id: Some(id.clone()),
                                                            name: Some(name_clone),
                                                            input: Some(args_clone.to_string()),
                                                            index: 0,
                                                        },
                                                    })
                                                    .await
                                                    .map_err(|e| {
                                                        AgentError::BadRequest(
                                                            BadRequestErrorMessage::ApiError(
                                                                e.to_string(),
                                                            ),
                                                        )
                                                    })?;
                                                message_content.push(
                                                    LLMMessageTypedContent::ToolCall {
                                                        id,
                                                        name,
                                                        args,
                                                    },
                                                );
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }

                        // Update usage metadata
                        if let Some(usage) = gemini_response.usage_metadata {
                            let token_usage = LLMTokenUsage {
                                prompt_tokens: usage.prompt_token_count.unwrap_or(0),
                                completion_tokens: usage.candidates_token_count.unwrap_or(0),
                                total_tokens: usage.total_token_count.unwrap_or(0),
                                prompt_tokens_details: None,
                            };
                            stream_channel_tx
                                .send(GenerationDelta::Usage {
                                    usage: token_usage.clone(),
                                })
                                .await
                                .map_err(|e| {
                                    AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                                        e.to_string(),
                                    ))
                                })?;
                            completion_response.usage = Some(token_usage);
                        }

                        // Update response ID if available
                        if let Some(response_id) = gemini_response.response_id {
                            completion_response.id = response_id;
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to parse JSON object: {}. Error: {}", json_str, e);
                    }
                }

                // Reset for next object
                json_accumulator.clear();
                in_object = false;
            }
        }
    }

    let has_tool_calls = message_content
        .iter()
        .any(|c| matches!(c, LLMMessageTypedContent::ToolCall { .. }));

    let final_finish_reason = if has_tool_calls {
        Some("tool_calls".to_string())
    } else {
        finish_reason.map(|s| s.to_lowercase())
    };

    // Build final message content
    completion_response.choices = vec![LLMChoice {
        finish_reason: final_finish_reason,
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

    eprint!("{:?}", completion_response);

    Ok(completion_response)
}
