//! OpenAI provider configuration and chat message types
//!
//! This module contains:
//! - Configuration types for OpenAI provider
//! - OpenAI model enums with pricing info
//! - Chat message types used throughout the TUI
//! - Tool call types for agent interactions
//!
//! Note: Low-level API request/response types are in `libs/ai/src/providers/openai/`.

use crate::models::llm::{
    GenerationDelta, LLMMessage, LLMMessageContent, LLMMessageImageSource, LLMMessageTypedContent,
    LLMTokenUsage, LLMTool,
};
use crate::models::model_pricing::{ContextAware, ContextPricingTier, ModelContextInfo};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

// =============================================================================
// Provider Configuration
// =============================================================================

/// Configuration for OpenAI provider
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct OpenAIConfig {
    pub api_endpoint: Option<String>,
    pub api_key: Option<String>,
}

impl OpenAIConfig {
    /// Create config with API key
    pub fn with_api_key(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Some(api_key.into()),
            api_endpoint: None,
        }
    }

    /// Create config from ProviderAuth (only supports API key for OpenAI)
    pub fn from_provider_auth(auth: &crate::models::auth::ProviderAuth) -> Option<Self> {
        match auth {
            crate::models::auth::ProviderAuth::Api { key } => Some(Self::with_api_key(key)),
            crate::models::auth::ProviderAuth::OAuth { .. } => None, // OpenAI doesn't support OAuth
        }
    }

    /// Merge with credentials from ProviderAuth, preserving existing endpoint
    pub fn with_provider_auth(mut self, auth: &crate::models::auth::ProviderAuth) -> Option<Self> {
        match auth {
            crate::models::auth::ProviderAuth::Api { key } => {
                self.api_key = Some(key.clone());
                Some(self)
            }
            crate::models::auth::ProviderAuth::OAuth { .. } => None, // OpenAI doesn't support OAuth
        }
    }
}

// =============================================================================
// Model Definitions
// =============================================================================

/// OpenAI model identifiers
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
    #[serde(rename = "gpt-5.1-2025-11-13")]
    GPT51,
    #[serde(rename = "gpt-5-mini-2025-08-07")]
    GPT5Mini,
    #[serde(rename = "gpt-5-nano-2025-08-07")]
    GPT5Nano,

    Custom(String),
}

impl OpenAIModel {
    pub fn from_string(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.to_string()))
            .map_err(|_| "Failed to deserialize OpenAI model".to_string())
    }

    /// Default smart model for OpenAI
    pub const DEFAULT_SMART_MODEL: OpenAIModel = OpenAIModel::GPT5;

    /// Default eco model for OpenAI
    pub const DEFAULT_ECO_MODEL: OpenAIModel = OpenAIModel::GPT5Mini;

    /// Default recovery model for OpenAI
    pub const DEFAULT_RECOVERY_MODEL: OpenAIModel = OpenAIModel::GPT5Mini;

    /// Get default smart model as string
    pub fn default_smart_model() -> String {
        Self::DEFAULT_SMART_MODEL.to_string()
    }

    /// Get default eco model as string
    pub fn default_eco_model() -> String {
        Self::DEFAULT_ECO_MODEL.to_string()
    }

    /// Get default recovery model as string
    pub fn default_recovery_model() -> String {
        Self::DEFAULT_RECOVERY_MODEL.to_string()
    }
}

impl ContextAware for OpenAIModel {
    fn context_info(&self) -> ModelContextInfo {
        let model_name = self.to_string();

        if model_name.starts_with("o3") {
            return ModelContextInfo {
                max_tokens: 200_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 2.0,
                    output_cost_per_million: 8.0,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        if model_name.starts_with("o4-mini") {
            return ModelContextInfo {
                max_tokens: 200_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 1.10,
                    output_cost_per_million: 4.40,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        if model_name.starts_with("gpt-5-mini") {
            return ModelContextInfo {
                max_tokens: 400_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 0.25,
                    output_cost_per_million: 2.0,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        if model_name.starts_with("gpt-5-nano") {
            return ModelContextInfo {
                max_tokens: 400_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 0.05,
                    output_cost_per_million: 0.40,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        if model_name.starts_with("gpt-5") {
            return ModelContextInfo {
                max_tokens: 400_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 1.25,
                    output_cost_per_million: 10.0,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        ModelContextInfo::default()
    }

    fn model_name(&self) -> String {
        match self {
            OpenAIModel::O3 => "O3".to_string(),
            OpenAIModel::O4Mini => "O4-mini".to_string(),
            OpenAIModel::GPT5 => "GPT-5".to_string(),
            OpenAIModel::GPT51 => "GPT-5.1".to_string(),
            OpenAIModel::GPT5Mini => "GPT-5 Mini".to_string(),
            OpenAIModel::GPT5Nano => "GPT-5 Nano".to_string(),
            OpenAIModel::Custom(name) => format!("Custom ({})", name),
        }
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
            OpenAIModel::GPT51 => write!(f, "gpt-5.1-2025-11-13"),
            OpenAIModel::Custom(model_name) => write!(f, "{}", model_name),
        }
    }
}

/// Agent model type (smart/eco/recovery)
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

// =============================================================================
// Message Types (used by TUI)
// =============================================================================

/// Message role
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    Developer,
    User,
    #[default]
    Assistant,
    Tool,
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

/// Chat message
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

/// Message content (string or array of parts)
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

/// Content part (text or image)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ContentPart {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<ImageUrl>,
}

/// Image URL with optional detail level
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// =============================================================================
// Tool Types (used by TUI)
// =============================================================================

/// Tool definition
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Tool {
    pub r#type: String,
    pub function: FunctionDefinition,
}

/// Function definition for tools
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: Option<String>,
    pub parameters: serde_json::Value,
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

/// Tool choice configuration
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

/// Tool call from assistant
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String,
    pub function: FunctionCall,
}

/// Function call details
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Tool call result status
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum ToolCallResultStatus {
    Success,
    Error,
    Cancelled,
}

/// Tool call result
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ToolCallResult {
    pub call: ToolCall,
    pub result: String,
    pub status: ToolCallResultStatus,
}

/// Tool call result progress update
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallResultProgress {
    pub id: Uuid,
    pub message: String,
}

// =============================================================================
// Chat Completion Types (used by TUI)
// =============================================================================

/// Chat completion request
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionContext {
    pub scratchpad: Option<Value>,
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

/// Chat completion response
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

// =============================================================================
// Streaming Types
// =============================================================================

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

// =============================================================================
// Conversions
// =============================================================================

impl From<LLMMessage> for ChatMessage {
    fn from(llm_message: LLMMessage) -> Self {
        let role = match llm_message.role.as_str() {
            "system" => Role::System,
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "tool" => Role::Tool,
            "developer" => Role::Developer,
            _ => Role::User,
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
                        LLMMessageTypedContent::Image { source } => {
                            text_parts.push(ContentPart {
                                r#type: "image_url".to_string(),
                                text: None,
                                image_url: Some(ImageUrl {
                                    url: format!(
                                        "data:{};base64,{}",
                                        source.media_type, source.data
                                    ),
                                    detail: None,
                                }),
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
            name: None,
            tool_calls,
            tool_call_id: None,
            usage: None,
        }
    }
}

impl From<ChatMessage> for LLMMessage {
    fn from(chat_message: ChatMessage) -> Self {
        let mut content_parts = Vec::new();

        match chat_message.content {
            Some(MessageContent::String(s)) => {
                if !s.is_empty() {
                    content_parts.push(LLMMessageTypedContent::Text { text: s });
                }
            }
            Some(MessageContent::Array(parts)) => {
                for part in parts {
                    if let Some(text) = part.text {
                        content_parts.push(LLMMessageTypedContent::Text { text });
                    } else if let Some(image_url) = part.image_url {
                        let (media_type, data) = if image_url.url.starts_with("data:") {
                            let parts: Vec<&str> = image_url.url.splitn(2, ',').collect();
                            if parts.len() == 2 {
                                let meta = parts[0];
                                let data = parts[1];
                                let media_type = meta
                                    .trim_start_matches("data:")
                                    .trim_end_matches(";base64")
                                    .to_string();
                                (media_type, data.to_string())
                            } else {
                                ("image/jpeg".to_string(), image_url.url)
                            }
                        } else {
                            ("image/jpeg".to_string(), image_url.url)
                        };

                        content_parts.push(LLMMessageTypedContent::Image {
                            source: LLMMessageImageSource {
                                r#type: "base64".to_string(),
                                media_type,
                                data,
                            },
                        });
                    }
                }
            }
            None => {}
        }

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
    }

    #[test]
    fn test_llm_message_to_chat_message() {
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
    }
}
