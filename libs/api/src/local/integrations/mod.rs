use crate::{
    error::{AgentError, BadRequestErrorMessage},
    local::integrations::{
        anthropic::{Anthropic, AnthropicConfig, AnthropicInput, AnthropicModel},
        openai::{OpenAI, OpenAIConfig, OpenAIInput, OpenAIModel},
    },
};
use serde::Serialize;
use stakpak_shared::models::llm::{GenerationDelta, LLMCompletionResponse, LLMMessage, LLMTool};
use std::fmt::Display;

pub mod anthropic;
pub mod openai;

#[derive(Clone, Debug, Serialize)]
pub enum LLMModel {
    Anthropic(AnthropicModel),
    OpenAI(OpenAIModel),
    Custom(String),
}

pub struct LLMProviderConfig {
    pub anthropic_config: Option<AnthropicConfig>,
    pub openai_config: Option<OpenAIConfig>,
}

impl From<String> for LLMModel {
    fn from(value: String) -> Self {
        match value.as_str() {
            "claude-haiku-4-5" => LLMModel::Anthropic(AnthropicModel::Claude45Haiku),
            "claude-sonnet-4-5" => LLMModel::Anthropic(AnthropicModel::Claude45Sonnet),
            "claude-opus-4-5" => LLMModel::Anthropic(AnthropicModel::Claude45Opus),
            "gpt-5" => LLMModel::OpenAI(OpenAIModel::GPT5),
            "gpt-5-mini" => LLMModel::OpenAI(OpenAIModel::GPT5Mini),
            _ => LLMModel::Custom(value),
        }
    }
}

impl Display for LLMModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LLMModel::Anthropic(model) => write!(f, "{}", model),
            LLMModel::OpenAI(model) => write!(f, "{}", model),
            LLMModel::Custom(model) => write!(f, "{}", model),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct LLMInput {
    pub model: LLMModel,
    pub messages: Vec<LLMMessage>,
    pub max_tokens: u32,
    pub tools: Option<Vec<LLMTool>>,
}

pub struct LLMStreamInput {
    pub model: LLMModel,
    pub messages: Vec<LLMMessage>,
    pub max_tokens: u32,
    pub stream_channel_tx: tokio::sync::mpsc::Sender<GenerationDelta>,
    pub tools: Option<Vec<LLMTool>>,
}

impl From<&LLMStreamInput> for LLMInput {
    fn from(value: &LLMStreamInput) -> Self {
        LLMInput {
            model: value.model.clone(),
            messages: value.messages.clone(),
            max_tokens: value.max_tokens,
            tools: value.tools.clone(),
        }
    }
}

pub async fn chat(
    config: &LLMProviderConfig,
    input: LLMInput,
) -> Result<LLMCompletionResponse, AgentError> {
    match input.model {
        LLMModel::Anthropic(model) => {
            if let Some(anthropic_config) = &config.anthropic_config {
                let anthropic_input = AnthropicInput {
                    model,
                    messages: input.messages,
                    grammar: None,
                    max_tokens: input.max_tokens,
                    stop_sequences: None,
                    tools: input.tools,
                    thinking: Default::default(),
                };
                Anthropic::chat(anthropic_config, anthropic_input).await
            } else {
                Err(AgentError::BadRequest(
                    BadRequestErrorMessage::InvalidAgentInput(
                        "Anthropic config not found".to_string(),
                    ),
                ))
            }
        }
        LLMModel::OpenAI(model) => {
            if let Some(openai_config) = &config.openai_config {
                let openai_input = OpenAIInput {
                    model,
                    messages: input.messages,
                    max_tokens: input.max_tokens,
                    json: None,
                    tools: input.tools,
                    reasoning_effort: None,
                };
                OpenAI::chat(openai_config, openai_input).await
            } else {
                Err(AgentError::BadRequest(
                    BadRequestErrorMessage::InvalidAgentInput(
                        "OpenAI config not found".to_string(),
                    ),
                ))
            }
        }
        LLMModel::Custom(_) => Err(AgentError::BadRequest(
            BadRequestErrorMessage::InvalidAgentInput("Custom model not supported".to_string()),
        )),
    }
}

pub async fn chat_stream(
    config: &LLMProviderConfig,
    input: LLMStreamInput,
) -> Result<LLMCompletionResponse, AgentError> {
    match input.model {
        LLMModel::Anthropic(model) => {
            if let Some(anthropic_config) = &config.anthropic_config {
                let anthropic_input = AnthropicInput {
                    model,
                    messages: input.messages,
                    grammar: None,
                    max_tokens: input.max_tokens,
                    stop_sequences: None,
                    tools: input.tools,
                    thinking: Default::default(),
                };
                Anthropic::chat_stream(anthropic_config, input.stream_channel_tx, anthropic_input)
                    .await
            } else {
                Err(AgentError::BadRequest(
                    BadRequestErrorMessage::InvalidAgentInput(
                        "Anthropic config not found".to_string(),
                    ),
                ))
            }
        }
        LLMModel::OpenAI(model) => {
            if let Some(openai_config) = &config.openai_config {
                let openai_input = OpenAIInput {
                    model,
                    messages: input.messages,
                    max_tokens: input.max_tokens,
                    json: None,
                    tools: input.tools,
                    reasoning_effort: None,
                };
                OpenAI::chat_stream_v2(openai_config, input.stream_channel_tx, openai_input).await
            } else {
                Err(AgentError::BadRequest(
                    BadRequestErrorMessage::InvalidAgentInput(
                        "OpenAI config not found".to_string(),
                    ),
                ))
            }
        }
        LLMModel::Custom(_) => Err(AgentError::BadRequest(
            BadRequestErrorMessage::InvalidAgentInput("Custom model not supported".to_string()),
        )),
    }
}
