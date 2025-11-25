use crate::{
    error::{AgentError, BadRequestErrorMessage},
    local::integrations::{
        anthropic::{Anthropic, AnthropicConfig, AnthropicInput, AnthropicModel},
        models::{
            generation::GenerationDelta,
            llm::{LLMCompletionResponse, LLMMessage, LLMTool},
        },
        openai::{OpenAI, OpenAIConfig, OpenAIInput, OpenAIModel},
    },
};
use std::fmt::Display;

pub mod anthropic;
pub mod models;
pub mod openai;

#[derive(Clone, Debug)]
pub enum InferenceModel {
    Anthropic(AnthropicModel),
    OpenAI(OpenAIModel),
    Custom(String),
}

pub struct InferenceConfig {
    pub anthropic_config: Option<AnthropicConfig>,
    pub openai_config: Option<OpenAIConfig>,
}

impl From<String> for InferenceModel {
    fn from(value: String) -> Self {
        match value.as_str() {
            "claude-haiku-4-5" => InferenceModel::Anthropic(AnthropicModel::Claude45Haiku),
            "claude-sonnet-4-5" => InferenceModel::Anthropic(AnthropicModel::Claude45Sonnet),
            "claude-opus-4-5" => InferenceModel::Anthropic(AnthropicModel::Claude45Opus),
            "gpt-5" => InferenceModel::OpenAI(OpenAIModel::GPT5),
            "gpt-5-mini" => InferenceModel::OpenAI(OpenAIModel::GPT5Mini),
            _ => InferenceModel::Custom(value),
        }
    }
}

impl Display for InferenceModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InferenceModel::Anthropic(model) => write!(f, "{}", model),
            InferenceModel::OpenAI(model) => write!(f, "{}", model),
            InferenceModel::Custom(model) => write!(f, "{}", model),
        }
    }
}

pub struct InferenceInput {
    pub model: InferenceModel,
    pub messages: Vec<LLMMessage>,
    pub max_tokens: u32,
    pub tools: Option<Vec<LLMTool>>,
}

pub struct InferenceStreamInput {
    pub model: InferenceModel,
    pub messages: Vec<LLMMessage>,
    pub max_tokens: u32,
    pub stream_channel_tx: tokio::sync::mpsc::Sender<GenerationDelta>,
    pub tools: Option<Vec<LLMTool>>,
}

pub async fn chat(
    config: &InferenceConfig,
    input: InferenceInput,
) -> Result<LLMCompletionResponse, AgentError> {
    match input.model {
        InferenceModel::Anthropic(model) => {
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
        InferenceModel::OpenAI(model) => {
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
        InferenceModel::Custom(_) => Err(AgentError::BadRequest(
            BadRequestErrorMessage::InvalidAgentInput("Custom model not supported".to_string()),
        )),
    }
}

pub async fn chat_stream(
    config: &InferenceConfig,
    input: InferenceStreamInput,
) -> Result<LLMCompletionResponse, AgentError> {
    match input.model {
        InferenceModel::Anthropic(model) => {
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
        InferenceModel::OpenAI(model) => {
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
        InferenceModel::Custom(_) => Err(AgentError::BadRequest(
            BadRequestErrorMessage::InvalidAgentInput("Custom model not supported".to_string()),
        )),
    }
}
