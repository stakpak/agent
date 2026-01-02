use crate::models::llm::{
    LLMChoice, LLMCompletionResponse, LLMMessage, LLMMessageContent, LLMTokenUsage, LLMTool,
    PromptTokensDetails,
};
use crate::models::model_pricing::{ContextAware, ContextPricingTier, ModelContextInfo};
use serde::{Deserialize, Serialize};

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

    /// Default smart model for Anthropic
    pub const DEFAULT_SMART_MODEL: AnthropicModel = AnthropicModel::Claude45Opus;

    /// Default eco model for Anthropic
    pub const DEFAULT_ECO_MODEL: AnthropicModel = AnthropicModel::Claude45Haiku;

    /// Default recovery model for Anthropic
    pub const DEFAULT_RECOVERY_MODEL: AnthropicModel = AnthropicModel::Claude45Haiku;

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

impl ContextAware for AnthropicModel {
    fn context_info(&self) -> ModelContextInfo {
        let model_name = self.to_string();

        if model_name.starts_with("claude-haiku") {
            return ModelContextInfo {
                max_tokens: 200_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 1.0,
                    output_cost_per_million: 5.0,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        if model_name.starts_with("claude-sonnet") {
            return ModelContextInfo {
                max_tokens: 1_000_000,
                pricing_tiers: vec![
                    ContextPricingTier {
                        label: "<200K tokens".to_string(),
                        input_cost_per_million: 3.0,
                        output_cost_per_million: 15.0,
                        upper_bound: Some(200_000),
                    },
                    ContextPricingTier {
                        label: ">200K tokens".to_string(),
                        input_cost_per_million: 6.0,
                        output_cost_per_million: 22.5,
                        upper_bound: None,
                    },
                ],
                approach_warning_threshold: 0.8,
            };
        }

        if model_name.starts_with("claude-opus") {
            return ModelContextInfo {
                max_tokens: 200_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 5.0,
                    output_cost_per_million: 25.0,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        panic!("Unknown model: {}", model_name);
    }

    fn model_name(&self) -> String {
        match self {
            AnthropicModel::Claude45Sonnet => "Claude Sonnet 4.5".to_string(),
            AnthropicModel::Claude45Haiku => "Claude Haiku 4.5".to_string(),
            AnthropicModel::Claude45Opus => "Claude Opus 4.5".to_string(),
        }
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
                input_tokens: Some(input_tokens),
                output_tokens: Some(output_tokens),
                cache_read_input_tokens: usage.cache_read_input_tokens,
                cache_write_input_tokens: usage.cache_creation_input_tokens,
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
