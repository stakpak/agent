use crate::models::llm::{
    LLMChoice, LLMCompletionResponse, LLMMessage, LLMMessageContent, LLMMessageTypedContent,
    LLMTokenUsage, LLMTool,
};
use crate::models::model_pricing::{ContextAware, ContextPricingTier, ModelContextInfo};
use serde::{Deserialize, Serialize};

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
}

impl std::fmt::Display for GeminiModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeminiModel::Gemini3Pro => write!(f, "gemini-3-pro-preview"),
            GeminiModel::Gemini25Pro => write!(f, "gemini-2.5-pro"),
            GeminiModel::Gemini25Flash => write!(f, "gemini-2.5-flash"),
            GeminiModel::Gemini25FlashLite => write!(f, "gemini-2.5-flash-lite"),
        }
    }
}

impl GeminiModel {
    pub fn from_string(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.to_string()))
            .map_err(|_| "Failed to deserialize Gemini model".to_string())
    }

    /// Default smart model for Gemini
    pub const DEFAULT_SMART_MODEL: GeminiModel = GeminiModel::Gemini3Pro;

    /// Default eco model for Gemini
    pub const DEFAULT_ECO_MODEL: GeminiModel = GeminiModel::Gemini25Flash;

    /// Default recovery model for Gemini
    pub const DEFAULT_RECOVERY_MODEL: GeminiModel = GeminiModel::Gemini25Flash;

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

impl ContextAware for GeminiModel {
    fn context_info(&self) -> ModelContextInfo {
        match self {
            GeminiModel::Gemini3Pro => ModelContextInfo {
                max_tokens: 1_000_000,
                pricing_tiers: vec![
                    ContextPricingTier {
                        label: "<200k tokens".to_string(),
                        input_cost_per_million: 2.0,
                        output_cost_per_million: 12.0,
                        upper_bound: Some(200_000),
                    },
                    ContextPricingTier {
                        label: ">200k tokens".to_string(),
                        input_cost_per_million: 4.0,
                        output_cost_per_million: 18.0,
                        upper_bound: None,
                    },
                ],
                approach_warning_threshold: 0.8,
            },
            GeminiModel::Gemini25Pro => ModelContextInfo {
                max_tokens: 1_000_000,
                pricing_tiers: vec![
                    ContextPricingTier {
                        label: "<200k tokens".to_string(),
                        input_cost_per_million: 1.25,
                        output_cost_per_million: 10.0,
                        upper_bound: Some(200_000),
                    },
                    ContextPricingTier {
                        label: ">200k tokens".to_string(),
                        input_cost_per_million: 2.50,
                        output_cost_per_million: 15.0,
                        upper_bound: None,
                    },
                ],
                approach_warning_threshold: 0.8,
            },
            GeminiModel::Gemini25Flash => ModelContextInfo {
                max_tokens: 1_000_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 0.30,
                    output_cost_per_million: 2.50,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            },
            GeminiModel::Gemini25FlashLite => ModelContextInfo {
                max_tokens: 1_000_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 0.1,
                    output_cost_per_million: 0.4,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            },
        }
    }

    fn model_name(&self) -> String {
        match self {
            GeminiModel::Gemini3Pro => "Gemini 3 Pro".to_string(),
            GeminiModel::Gemini25Pro => "Gemini 2.5 Pro".to_string(),
            GeminiModel::Gemini25Flash => "Gemini 2.5 Flash".to_string(),
            GeminiModel::Gemini25FlashLite => "Gemini 2.5 Flash Lite".to_string(),
        }
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
#[serde(rename_all = "camelCase")]
pub struct GeminiResponse {
    pub candidates: Option<Vec<GeminiCandidate>>,
    pub usage_metadata: Option<GeminiUsageMetadata>,
    pub model_version: Option<String>,
    pub response_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCandidate {
    pub content: Option<GeminiContent>,
    pub finish_reason: Option<String>,
    pub index: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
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
