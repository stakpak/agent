//! Adapter layer between CLI LLM types and StakAI SDK
//!
//! This module provides conversion functions and a wrapper client to use the StakAI SDK
//! with the CLI's existing LLM types, enabling BYOM (Bring Your Own Model) functionality.

use crate::models::error::{AgentError, BadRequestErrorMessage};
use crate::models::llm::{
    GenerationDelta, GenerationDeltaToolUse, LLMChoice, LLMCompletionResponse, LLMInput,
    LLMMessage, LLMMessageContent, LLMMessageImageSource, LLMMessageTypedContent, LLMModel,
    LLMProviderConfig, LLMProviderOptions, LLMStreamInput, LLMTokenUsage, LLMTool, ProviderConfig,
};
use futures::StreamExt;
use stakai::{
    AnthropicOptions, ContentPart, FinishReason, GenerateOptions, GenerateRequest,
    GenerateResponse, GoogleOptions, Headers, Inference, InferenceConfig, Message, MessageContent,
    OpenAIOptions, ProviderOptions, ReasoningEffort, Role, StreamEvent, ThinkingOptions, Tool,
    ToolFunction, Usage, providers::anthropic::AnthropicConfig as StakaiAnthropicConfig,
    registry::ProviderRegistry,
};

/// Convert CLI LLMMessage to StakAI Message
pub fn to_stakai_message(msg: &LLMMessage) -> Message {
    let role = match msg.role.as_str() {
        "system" => Role::System,
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    };

    let content: MessageContent = match &msg.content {
        LLMMessageContent::String(s) => s.clone().into(),
        LLMMessageContent::List(parts) => {
            let content_parts: Vec<ContentPart> =
                parts.iter().map(to_stakai_content_part).collect();
            content_parts.into()
        }
    };

    Message::new(role, content)
}

/// Convert CLI LLMMessageTypedContent to StakAI ContentPart
fn to_stakai_content_part(part: &LLMMessageTypedContent) -> ContentPart {
    match part {
        LLMMessageTypedContent::Text { text } => ContentPart::text(text),
        LLMMessageTypedContent::Image { source } => {
            // Convert base64 image to data URI
            ContentPart::image(format!("data:{};base64,{}", source.media_type, source.data))
        }
        LLMMessageTypedContent::ToolCall { id, name, args } => {
            ContentPart::tool_call(id, name, args.clone())
        }
        LLMMessageTypedContent::ToolResult {
            tool_use_id,
            content,
        } => ContentPart::tool_result(tool_use_id, serde_json::Value::String(content.clone())),
    }
}

/// Convert StakAI Message to CLI LLMMessage
pub fn from_stakai_message(msg: &Message) -> LLMMessage {
    let role = match msg.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
    .to_string();

    let content = match &msg.content {
        MessageContent::Text(s) => LLMMessageContent::String(s.clone()),
        MessageContent::Parts(parts) => {
            LLMMessageContent::List(parts.iter().map(from_stakai_content_part).collect())
        }
    };

    LLMMessage { role, content }
}

/// Convert StakAI ContentPart to CLI LLMMessageTypedContent
fn from_stakai_content_part(part: &ContentPart) -> LLMMessageTypedContent {
    match part {
        ContentPart::Text { text, .. } => LLMMessageTypedContent::Text { text: text.clone() },
        ContentPart::Image { url, .. } => {
            // Parse data URI back to base64
            let (media_type, data) = if url.starts_with("data:") {
                let parts: Vec<&str> = url.splitn(2, ',').collect();
                if parts.len() == 2 {
                    let media = parts[0]
                        .strip_prefix("data:")
                        .unwrap_or("image/png")
                        .strip_suffix(";base64")
                        .unwrap_or("image/png");
                    (media.to_string(), parts[1].to_string())
                } else {
                    ("image/png".to_string(), url.clone())
                }
            } else {
                ("image/png".to_string(), url.clone())
            };

            LLMMessageTypedContent::Image {
                source: LLMMessageImageSource {
                    r#type: "base64".to_string(),
                    media_type,
                    data,
                },
            }
        }
        ContentPart::ToolCall {
            id,
            name,
            arguments,
            ..
        } => LLMMessageTypedContent::ToolCall {
            id: id.clone(),
            name: name.clone(),
            args: arguments.clone(),
        },
        ContentPart::ToolResult {
            tool_call_id,
            content,
            ..
        } => LLMMessageTypedContent::ToolResult {
            tool_use_id: tool_call_id.clone(),
            content: match content {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            },
        },
    }
}

/// Convert CLI LLMTool to StakAI Tool
pub fn to_stakai_tool(tool: &LLMTool) -> Tool {
    Tool {
        tool_type: "function".to_string(),
        function: ToolFunction {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.input_schema.clone(),
        },
        provider_options: None,
    }
}

/// Convert StakAI Tool to CLI LLMTool
pub fn from_stakai_tool(tool: &Tool) -> LLMTool {
    LLMTool {
        name: tool.function.name.clone(),
        description: tool.function.description.clone(),
        input_schema: tool.function.parameters.clone(),
    }
}

/// Convert StakAI StreamEvent to CLI GenerationDelta
pub fn from_stakai_stream_event(event: &StreamEvent) -> Option<GenerationDelta> {
    match event {
        StreamEvent::TextDelta { delta, .. } => Some(GenerationDelta::Content {
            content: delta.clone(),
        }),
        StreamEvent::ReasoningDelta { delta, .. } => Some(GenerationDelta::Thinking {
            thinking: delta.clone(),
        }),
        StreamEvent::ToolCallStart { id, name } => Some(GenerationDelta::ToolUse {
            tool_use: GenerationDeltaToolUse {
                id: Some(id.clone()),
                name: Some(name.clone()),
                input: None,
                index: 0,
            },
        }),
        StreamEvent::ToolCallDelta { id, delta } => Some(GenerationDelta::ToolUse {
            tool_use: GenerationDeltaToolUse {
                id: Some(id.clone()),
                name: None,
                input: Some(delta.clone()),
                index: 0,
            },
        }),
        StreamEvent::ToolCallEnd { id, name, .. } => {
            // ToolCallEnd signals completion - don't emit arguments here as they
            // were already accumulated via ToolCallDelta events. Including them
            // would cause doubling for providers like Anthropic that stream deltas.
            // We emit name to ensure it's set (for providers like Gemini that only send ToolCallEnd).
            Some(GenerationDelta::ToolUse {
                tool_use: GenerationDeltaToolUse {
                    id: Some(id.clone()),
                    name: Some(name.clone()),
                    input: None,
                    index: 0,
                },
            })
        }
        StreamEvent::Finish { usage, .. } => Some(GenerationDelta::Usage {
            usage: from_stakai_usage(usage),
        }),
        StreamEvent::Start { .. } | StreamEvent::Error { .. } => None,
    }
}

/// Convert StakAI Usage to CLI LLMTokenUsage
pub fn from_stakai_usage(usage: &Usage) -> LLMTokenUsage {
    use crate::models::llm::PromptTokensDetails;

    // Convert StakAI input_token_details to CLI PromptTokensDetails
    let prompt_tokens_details = usage.input_token_details.as_ref().map(|details| {
        PromptTokensDetails {
            input_tokens: details.no_cache,
            output_tokens: None, // Output tokens are tracked separately
            cache_read_input_tokens: details.cache_read,
            cache_write_input_tokens: details.cache_write,
        }
    });

    LLMTokenUsage {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
        prompt_tokens_details,
    }
}

/// Convert StakAI FinishReason to string
pub fn finish_reason_to_string(reason: &FinishReason) -> String {
    // If raw value is available, use it; otherwise use the unified reason
    if let Some(raw) = &reason.raw {
        raw.clone()
    } else {
        match reason.unified {
            stakai::FinishReasonKind::Stop => "stop".to_string(),
            stakai::FinishReasonKind::Length => "length".to_string(),
            stakai::FinishReasonKind::ContentFilter => "content_filter".to_string(),
            stakai::FinishReasonKind::ToolCalls => "tool_calls".to_string(),
            stakai::FinishReasonKind::Error => "error".to_string(),
            stakai::FinishReasonKind::Other => "other".to_string(),
        }
    }
}

/// Convert CLI LLMProviderOptions to StakAI ProviderOptions
pub fn to_stakai_provider_options(
    opts: &LLMProviderOptions,
    model: &LLMModel,
) -> Option<ProviderOptions> {
    // Determine provider from model and options
    match model {
        LLMModel::Anthropic(_) => {
            if let Some(anthropic) = &opts.anthropic {
                let thinking = anthropic
                    .thinking
                    .as_ref()
                    .map(|t| ThinkingOptions::new(t.budget_tokens));

                Some(ProviderOptions::Anthropic(AnthropicOptions {
                    thinking,
                    effort: None,
                }))
            } else {
                None
            }
        }
        LLMModel::OpenAI(_) => {
            if let Some(openai) = &opts.openai {
                let reasoning_effort = openai.reasoning_effort.as_ref().and_then(|e| {
                    match e.to_lowercase().as_str() {
                        "low" => Some(ReasoningEffort::Low),
                        "medium" => Some(ReasoningEffort::Medium),
                        "high" => Some(ReasoningEffort::High),
                        _ => None,
                    }
                });

                Some(ProviderOptions::OpenAI(OpenAIOptions {
                    reasoning_effort,
                    reasoning_summary: None,
                    system_message_mode: None,
                    store: None,
                    user: None,
                    prompt_cache_key: None,
                    prompt_cache_retention: None,
                }))
            } else {
                None
            }
        }
        LLMModel::Gemini(_) => opts.google.as_ref().map(|google| {
            ProviderOptions::Google(GoogleOptions {
                thinking_budget: google.thinking_budget,
                cached_content: None,
            })
        }),
        LLMModel::Custom { .. } => {
            // For custom models, try to infer from which options are set
            if let Some(anthropic) = &opts.anthropic {
                let thinking = anthropic
                    .thinking
                    .as_ref()
                    .map(|t| ThinkingOptions::new(t.budget_tokens));
                Some(ProviderOptions::Anthropic(AnthropicOptions {
                    thinking,
                    effort: None,
                }))
            } else if let Some(openai) = &opts.openai {
                let reasoning_effort = openai.reasoning_effort.as_ref().and_then(|e| {
                    match e.to_lowercase().as_str() {
                        "low" => Some(ReasoningEffort::Low),
                        "medium" => Some(ReasoningEffort::Medium),
                        "high" => Some(ReasoningEffort::High),
                        _ => None,
                    }
                });
                Some(ProviderOptions::OpenAI(OpenAIOptions {
                    reasoning_effort,
                    reasoning_summary: None,
                    system_message_mode: None,
                    store: None,
                    user: None,
                    prompt_cache_key: None,
                    prompt_cache_retention: None,
                }))
            } else {
                opts.google.as_ref().map(|google| {
                    ProviderOptions::Google(GoogleOptions {
                        thinking_budget: google.thinking_budget,
                        cached_content: None,
                    })
                })
            }
        }
    }
}

/// Convert StakAI GenerateResponse to CLI LLMCompletionResponse
pub fn from_stakai_response(response: GenerateResponse, model: &str) -> LLMCompletionResponse {
    let mut content_parts: Vec<LLMMessageTypedContent> = Vec::new();

    for content in &response.content {
        match content {
            stakai::ResponseContent::Text { text } => {
                content_parts.push(LLMMessageTypedContent::Text { text: text.clone() });
            }
            stakai::ResponseContent::Reasoning { reasoning } => {
                // Include reasoning as a text block with a prefix for visibility
                // This matches how Anthropic's thinking is typically displayed
                content_parts.push(LLMMessageTypedContent::Text {
                    text: format!("[Reasoning: {}]", reasoning),
                });
            }
            stakai::ResponseContent::ToolCall(tool_call) => {
                content_parts.push(LLMMessageTypedContent::ToolCall {
                    id: tool_call.id.clone(),
                    name: tool_call.name.clone(),
                    args: tool_call.arguments.clone(),
                });
            }
        }
    }

    let message_content = if content_parts.len() == 1 {
        if let LLMMessageTypedContent::Text { text } = &content_parts[0] {
            LLMMessageContent::String(text.clone())
        } else {
            LLMMessageContent::List(content_parts)
        }
    } else {
        LLMMessageContent::List(content_parts)
    };

    LLMCompletionResponse {
        id: uuid::Uuid::new_v4().to_string(),
        model: model.to_string(),
        object: "chat.completion".to_string(),
        choices: vec![LLMChoice {
            finish_reason: Some(finish_reason_to_string(&response.finish_reason)),
            index: 0,
            message: LLMMessage {
                role: "assistant".to_string(),
                content: message_content,
            },
        }],
        created: chrono::Utc::now().timestamp_millis() as u64,
        usage: Some(from_stakai_usage(&response.usage)),
    }
}

/// Build StakAI InferenceConfig from CLI LLMProviderConfig
pub fn build_inference_config(config: &LLMProviderConfig) -> Result<InferenceConfig, String> {
    let mut inference_config = InferenceConfig::new();

    for (name, provider_config) in &config.providers {
        match provider_config {
            ProviderConfig::OpenAI {
                api_key,
                api_endpoint,
            } => {
                if let Some(api_key) = api_key {
                    inference_config =
                        inference_config.openai(api_key.clone(), api_endpoint.clone());
                }
            }
            ProviderConfig::Anthropic {
                api_key,
                api_endpoint,
                access_token,
            } => {
                let stakai_config = if let Some(token) = access_token {
                    // OAuth authentication - uses Bearer token header
                    let mut cfg = StakaiAnthropicConfig::with_oauth(token);
                    if let Some(endpoint) = api_endpoint {
                        cfg = cfg.with_base_url(endpoint);
                    }
                    Some(cfg)
                } else if let Some(key) = api_key {
                    // API key authentication - uses x-api-key header
                    let mut cfg = StakaiAnthropicConfig::new(key);
                    if let Some(endpoint) = api_endpoint {
                        cfg = cfg.with_base_url(endpoint);
                    }
                    Some(cfg)
                } else {
                    None
                };

                if let Some(cfg) = stakai_config {
                    inference_config = inference_config.anthropic_config(cfg);
                }
            }
            ProviderConfig::Gemini {
                api_key,
                api_endpoint,
            } => {
                if let Some(api_key) = api_key {
                    inference_config =
                        inference_config.gemini(api_key.clone(), api_endpoint.clone());
                }
            }
            ProviderConfig::Stakpak {
                api_key,
                api_endpoint,
            } => {
                // Skip if api_key is empty - stakpak is optional
                if !api_key.is_empty() {
                    inference_config =
                        inference_config.stakpak(api_key.clone(), api_endpoint.clone());
                }
            }
            ProviderConfig::Custom { .. } => {
                // Custom providers are handled by build_provider_registry_direct
                // InferenceConfig doesn't support custom providers directly
                let _ = name; // Suppress unused warning
            }
        }
    }

    Ok(inference_config)
}

/// Build a ProviderRegistry directly with all providers including custom ones
fn build_provider_registry_direct(config: &LLMProviderConfig) -> Result<ProviderRegistry, String> {
    use stakai::providers::anthropic::{
        AnthropicConfig as StakaiAnthropicConfig, AnthropicProvider,
    };
    use stakai::providers::gemini::{GeminiConfig as StakaiGeminiConfig, GeminiProvider};
    use stakai::providers::openai::{OpenAIConfig as StakaiOpenAIConfig, OpenAIProvider};
    use stakai::providers::stakpak::{StakpakProvider, StakpakProviderConfig};

    let mut registry = ProviderRegistry::new();

    for (name, provider_config) in &config.providers {
        match provider_config {
            ProviderConfig::OpenAI {
                api_key,
                api_endpoint,
            } => {
                if let Some(api_key) = api_key {
                    let mut openai_config = StakaiOpenAIConfig::new(api_key.clone());
                    if let Some(endpoint) = api_endpoint {
                        openai_config = openai_config.with_base_url(endpoint.clone());
                    }
                    let provider = OpenAIProvider::new(openai_config)
                        .map_err(|e| format!("Failed to create OpenAI provider: {}", e))?;
                    registry = registry.register("openai", provider);
                }
            }
            ProviderConfig::Anthropic {
                api_key,
                api_endpoint,
                access_token,
            } => {
                let stakai_config = if let Some(token) = access_token {
                    let mut cfg = StakaiAnthropicConfig::with_oauth(token);
                    if let Some(endpoint) = api_endpoint {
                        cfg = cfg.with_base_url(endpoint);
                    }
                    Some(cfg)
                } else if let Some(key) = api_key {
                    let mut cfg = StakaiAnthropicConfig::new(key);
                    if let Some(endpoint) = api_endpoint {
                        cfg = cfg.with_base_url(endpoint);
                    }
                    Some(cfg)
                } else {
                    None
                };

                if let Some(cfg) = stakai_config {
                    let provider = AnthropicProvider::new(cfg)
                        .map_err(|e| format!("Failed to create Anthropic provider: {}", e))?;
                    registry = registry.register("anthropic", provider);
                }
            }
            ProviderConfig::Gemini {
                api_key,
                api_endpoint,
            } => {
                if let Some(api_key) = api_key {
                    let mut gemini_config = StakaiGeminiConfig::new(api_key.clone());
                    if let Some(endpoint) = api_endpoint {
                        gemini_config = gemini_config.with_base_url(endpoint.clone());
                    }
                    let provider = GeminiProvider::new(gemini_config)
                        .map_err(|e| format!("Failed to create Gemini provider: {}", e))?;
                    registry = registry.register("google", provider);
                }
            }
            ProviderConfig::Stakpak {
                api_key,
                api_endpoint,
            } => {
                // Skip if api_key is empty - stakpak is optional
                if api_key.is_empty() {
                    continue;
                }
                let mut stakpak_config = StakpakProviderConfig::new(api_key.clone())
                    .with_user_agent(format!("Stakpak/{}", env!("CARGO_PKG_VERSION")));
                if let Some(endpoint) = api_endpoint {
                    stakpak_config = stakpak_config.with_base_url(endpoint.clone());
                }
                let provider = StakpakProvider::new(stakpak_config)
                    .map_err(|e| format!("Failed to create Stakpak provider: {}", e))?;
                registry = registry.register("stakpak", provider);
            }
            ProviderConfig::Custom {
                api_key,
                api_endpoint,
            } => {
                // Custom providers are registered as OpenAI-compatible providers.
                // The provider is registered with the config key (e.g., "litellm") as its ID.
                // When a model like "litellm/anthropic/claude-opus" is used:
                // 1. "litellm" is matched to this provider
                // 2. "anthropic/claude-opus" is sent as the model name to the API
                let key = api_key.clone().unwrap_or_default();
                let openai_config =
                    StakaiOpenAIConfig::new(key).with_base_url(api_endpoint.clone());

                let provider = OpenAIProvider::new(openai_config)
                    .map_err(|e| format!("Failed to create custom provider '{}': {}", name, e))?;

                // Register with the config key as provider ID (e.g., "litellm", "ollama")
                registry = registry.register(name, provider);
            }
        }
    }

    Ok(registry)
}

/// Get model string with provider prefix for StakAI
pub fn get_stakai_model_string(model: &LLMModel) -> String {
    match model {
        LLMModel::Anthropic(m) => format!("anthropic/{}", m),
        LLMModel::Gemini(m) => format!("google/{}", m),
        LLMModel::OpenAI(m) => format!("openai/{}", m),
        LLMModel::Custom {
            provider, model, ..
        } => format!("{}/{}", provider, model),
    }
}

/// Wrapper around StakAI Inference for CLI usage
#[derive(Clone)]
pub struct StakAIClient {
    inference: Inference,
}

impl StakAIClient {
    /// Create a new StakAI client from CLI provider config
    pub fn new(config: &LLMProviderConfig) -> Result<Self, AgentError> {
        // Build registry with all providers including custom ones
        let registry = build_provider_registry_direct(config)
            .map_err(|e| AgentError::BadRequest(BadRequestErrorMessage::InvalidAgentInput(e)))?;

        let inference = Inference::builder()
            .with_registry(registry)
            .build()
            .map_err(|e| {
                AgentError::BadRequest(BadRequestErrorMessage::InvalidAgentInput(e.to_string()))
            })?;

        Ok(Self { inference })
    }

    /// Create a new StakAI client with custom provider registry
    pub fn with_registry(registry: ProviderRegistry) -> Result<Self, AgentError> {
        let inference = Inference::builder()
            .with_registry(registry)
            .build()
            .map_err(|e| {
                AgentError::BadRequest(BadRequestErrorMessage::InvalidAgentInput(e.to_string()))
            })?;

        Ok(Self { inference })
    }

    /// Non-streaming chat completion
    pub async fn chat(&self, input: LLMInput) -> Result<LLMCompletionResponse, AgentError> {
        let model_string = get_stakai_model_string(&input.model);
        let messages: Vec<Message> = input.messages.iter().map(to_stakai_message).collect();

        let mut options = GenerateOptions::new().max_tokens(input.max_tokens);

        if let Some(tools) = &input.tools {
            for tool in tools {
                options = options.add_tool(to_stakai_tool(tool));
            }
        }

        // Add custom headers if present
        if let Some(headers) = &input.headers {
            let mut stakai_headers = Headers::new();
            for (key, value) in headers {
                stakai_headers.insert(key, value);
            }
            options = options.headers(stakai_headers);
        }

        // Convert provider options if present
        let provider_options = input
            .provider_options
            .as_ref()
            .and_then(|opts| to_stakai_provider_options(opts, &input.model));

        let request = GenerateRequest {
            model: model_string.clone(),
            messages,
            options,
            provider_options,
            telemetry_metadata: None,
        };

        let response = self.inference.generate(&request).await.map_err(|e| {
            AgentError::BadRequest(BadRequestErrorMessage::InvalidAgentInput(e.to_string()))
        })?;

        Ok(from_stakai_response(response, &model_string))
    }

    /// Streaming chat completion
    pub async fn chat_stream(
        &self,
        input: LLMStreamInput,
    ) -> Result<LLMCompletionResponse, AgentError> {
        let model_string = get_stakai_model_string(&input.model);
        let messages: Vec<Message> = input.messages.iter().map(to_stakai_message).collect();

        let mut options = GenerateOptions::new().max_tokens(input.max_tokens);

        if let Some(tools) = &input.tools {
            for tool in tools {
                options = options.add_tool(to_stakai_tool(tool));
            }
        }

        // Add custom headers if present
        if let Some(headers) = &input.headers {
            let mut stakai_headers = Headers::new();
            for (key, value) in headers {
                stakai_headers.insert(key, value);
            }
            options = options.headers(stakai_headers);
        }

        // Convert provider options if present
        let provider_options = input
            .provider_options
            .as_ref()
            .and_then(|opts| to_stakai_provider_options(opts, &input.model));

        let request = GenerateRequest {
            model: model_string.clone(),
            messages,
            options,
            provider_options,
            telemetry_metadata: None,
        };

        let mut stream = self.inference.stream(&request).await.map_err(|e| {
            AgentError::BadRequest(BadRequestErrorMessage::InvalidAgentInput(e.to_string()))
        })?;

        let tx = input.stream_channel_tx;
        let mut accumulated_text = String::new();
        let mut accumulated_tool_calls: Vec<LLMMessageTypedContent> = Vec::new();
        let mut final_usage = LLMTokenUsage::default();
        let mut finish_reason = "stop".to_string();

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => {
                    // Forward event to channel
                    if let Some(delta) = from_stakai_stream_event(&event) {
                        // Accumulate content for final response
                        match &delta {
                            GenerationDelta::Content { content } => {
                                accumulated_text.push_str(content);
                            }
                            GenerationDelta::ToolUse { tool_use } => {
                                if let Some(id) = &tool_use.id {
                                    // Find existing tool call by id
                                    let existing = accumulated_tool_calls.iter_mut().find(|tc| {
                                        matches!(tc, LLMMessageTypedContent::ToolCall { id: tc_id, .. } if tc_id == id)
                                    });

                                    match existing {
                                        Some(LLMMessageTypedContent::ToolCall {
                                            args,
                                            name: existing_name,
                                            ..
                                        }) => {
                                            // Update existing tool call
                                            // Update name if provided and current is empty
                                            if let Some(new_name) = &tool_use.name
                                                && existing_name.is_empty()
                                            {
                                                *existing_name = new_name.clone();
                                            }
                                            // Append arguments if provided
                                            if let Some(input) = &tool_use.input {
                                                // Accumulate as string first, parse later
                                                if let serde_json::Value::String(s) = args {
                                                    s.push_str(input);
                                                } else {
                                                    *args =
                                                        serde_json::Value::String(input.clone());
                                                }
                                            }
                                        }
                                        _ => {
                                            // Create new tool call
                                            let name = tool_use.name.clone().unwrap_or_default();
                                            let args = tool_use
                                                .input
                                                .clone()
                                                .map(serde_json::Value::String)
                                                .unwrap_or_else(|| {
                                                    serde_json::Value::String(String::new())
                                                });
                                            accumulated_tool_calls.push(
                                                LLMMessageTypedContent::ToolCall {
                                                    id: id.clone(),
                                                    name,
                                                    args,
                                                },
                                            );
                                        }
                                    }
                                }
                            }
                            GenerationDelta::Usage { usage } => {
                                final_usage = usage.clone();
                            }
                            _ => {}
                        }

                        // Send to channel (ignore errors if receiver dropped)
                        let _ = tx.send(delta).await;
                    }

                    // Check for finish
                    if let StreamEvent::Finish { reason, usage } = event {
                        finish_reason = finish_reason_to_string(&reason);
                        final_usage = from_stakai_usage(&usage);
                    }
                }
                Err(e) => {
                    return Err(AgentError::BadRequest(
                        BadRequestErrorMessage::InvalidAgentInput(e.to_string()),
                    ));
                }
            }
        }

        // Build final response
        // Parse accumulated JSON string arguments into proper JSON values
        let parsed_tool_calls: Vec<LLMMessageTypedContent> = accumulated_tool_calls
            .into_iter()
            .map(|tc| {
                if let LLMMessageTypedContent::ToolCall { id, name, args } = tc {
                    let parsed_args = match args {
                        serde_json::Value::String(s) if !s.is_empty() => {
                            serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s))
                        }
                        other => other,
                    };
                    LLMMessageTypedContent::ToolCall {
                        id,
                        name,
                        args: parsed_args,
                    }
                } else {
                    tc
                }
            })
            .collect();

        let message_content = if parsed_tool_calls.is_empty() {
            LLMMessageContent::String(accumulated_text)
        } else {
            let mut parts = vec![LLMMessageTypedContent::Text {
                text: accumulated_text,
            }];
            parts.extend(parsed_tool_calls);
            LLMMessageContent::List(parts)
        };

        Ok(LLMCompletionResponse {
            id: uuid::Uuid::new_v4().to_string(),
            model: model_string,
            object: "chat.completion".to_string(),
            choices: vec![LLMChoice {
                finish_reason: Some(finish_reason),
                index: 0,
                message: LLMMessage {
                    role: "assistant".to_string(),
                    content: message_content,
                },
            }],
            created: chrono::Utc::now().timestamp_millis() as u64,
            usage: Some(final_usage),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::integrations::anthropic::AnthropicModel;
    use crate::models::integrations::gemini::GeminiModel;
    use crate::models::integrations::openai::OpenAIModel;

    // ==================== Role Conversion Tests ====================

    #[test]
    fn test_role_conversion_user() {
        let msg = LLMMessage {
            role: "user".to_string(),
            content: LLMMessageContent::String("Hello".to_string()),
        };

        let stakai_msg = to_stakai_message(&msg);
        assert!(matches!(stakai_msg.role, Role::User));

        let back = from_stakai_message(&stakai_msg);
        assert_eq!(back.role, "user");
    }

    #[test]
    fn test_role_conversion_assistant() {
        let msg = LLMMessage {
            role: "assistant".to_string(),
            content: LLMMessageContent::String("Hi there!".to_string()),
        };

        let stakai_msg = to_stakai_message(&msg);
        assert!(matches!(stakai_msg.role, Role::Assistant));

        let back = from_stakai_message(&stakai_msg);
        assert_eq!(back.role, "assistant");
    }

    #[test]
    fn test_role_conversion_system() {
        let msg = LLMMessage {
            role: "system".to_string(),
            content: LLMMessageContent::String("You are a helpful assistant.".to_string()),
        };

        let stakai_msg = to_stakai_message(&msg);
        assert!(matches!(stakai_msg.role, Role::System));

        let back = from_stakai_message(&stakai_msg);
        assert_eq!(back.role, "system");
    }

    #[test]
    fn test_role_conversion_tool() {
        let msg = LLMMessage {
            role: "tool".to_string(),
            content: LLMMessageContent::String("Tool result".to_string()),
        };

        let stakai_msg = to_stakai_message(&msg);
        assert!(matches!(stakai_msg.role, Role::Tool));

        let back = from_stakai_message(&stakai_msg);
        assert_eq!(back.role, "tool");
    }

    #[test]
    fn test_role_conversion_unknown_defaults_to_user() {
        let msg = LLMMessage {
            role: "unknown_role".to_string(),
            content: LLMMessageContent::String("Test".to_string()),
        };

        let stakai_msg = to_stakai_message(&msg);
        assert!(matches!(stakai_msg.role, Role::User));
    }

    // ==================== Content Conversion Tests ====================

    #[test]
    fn test_string_content_conversion() {
        let msg = LLMMessage {
            role: "user".to_string(),
            content: LLMMessageContent::String("Simple text message".to_string()),
        };

        let stakai_msg = to_stakai_message(&msg);
        assert_eq!(stakai_msg.text(), Some("Simple text message".to_string()));

        let back = from_stakai_message(&stakai_msg);
        if let LLMMessageContent::String(text) = back.content {
            assert_eq!(text, "Simple text message");
        } else {
            panic!("Expected String content");
        }
    }

    #[test]
    fn test_list_content_with_text() {
        let msg = LLMMessage {
            role: "assistant".to_string(),
            content: LLMMessageContent::List(vec![LLMMessageTypedContent::Text {
                text: "Hello world".to_string(),
            }]),
        };

        let stakai_msg = to_stakai_message(&msg);
        let parts = stakai_msg.parts();
        assert_eq!(parts.len(), 1);

        let back = from_stakai_message(&stakai_msg);
        if let LLMMessageContent::List(parts) = back.content {
            assert_eq!(parts.len(), 1);
            assert!(
                matches!(&parts[0], LLMMessageTypedContent::Text { text } if text == "Hello world")
            );
        } else {
            panic!("Expected List content");
        }
    }

    #[test]
    fn test_list_content_with_tool_call() {
        let msg = LLMMessage {
            role: "assistant".to_string(),
            content: LLMMessageContent::List(vec![
                LLMMessageTypedContent::Text {
                    text: "Let me check the weather.".to_string(),
                },
                LLMMessageTypedContent::ToolCall {
                    id: "call_abc123".to_string(),
                    name: "get_weather".to_string(),
                    args: serde_json::json!({"location": "New York", "unit": "celsius"}),
                },
            ]),
        };

        let stakai_msg = to_stakai_message(&msg);
        let parts = stakai_msg.parts();
        assert_eq!(parts.len(), 2);

        let back = from_stakai_message(&stakai_msg);
        if let LLMMessageContent::List(parts) = back.content {
            assert_eq!(parts.len(), 2);

            // Check text part
            assert!(
                matches!(&parts[0], LLMMessageTypedContent::Text { text } if text == "Let me check the weather.")
            );

            // Check tool call part
            if let LLMMessageTypedContent::ToolCall { id, name, args } = &parts[1] {
                assert_eq!(id, "call_abc123");
                assert_eq!(name, "get_weather");
                assert_eq!(args["location"], "New York");
                assert_eq!(args["unit"], "celsius");
            } else {
                panic!("Expected ToolCall content");
            }
        } else {
            panic!("Expected List content");
        }
    }

    #[test]
    fn test_list_content_with_tool_result() {
        let msg = LLMMessage {
            role: "tool".to_string(),
            content: LLMMessageContent::List(vec![LLMMessageTypedContent::ToolResult {
                tool_use_id: "call_abc123".to_string(),
                content: "Temperature: 22°C, Sunny".to_string(),
            }]),
        };

        let stakai_msg = to_stakai_message(&msg);
        let parts = stakai_msg.parts();
        assert_eq!(parts.len(), 1);

        let back = from_stakai_message(&stakai_msg);
        if let LLMMessageContent::List(parts) = back.content {
            assert_eq!(parts.len(), 1);
            if let LLMMessageTypedContent::ToolResult {
                tool_use_id,
                content,
            } = &parts[0]
            {
                assert_eq!(tool_use_id, "call_abc123");
                assert_eq!(content, "Temperature: 22°C, Sunny");
            } else {
                panic!("Expected ToolResult content");
            }
        } else {
            panic!("Expected List content");
        }
    }

    #[test]
    fn test_image_content_conversion() {
        let msg = LLMMessage {
            role: "user".to_string(),
            content: LLMMessageContent::List(vec![LLMMessageTypedContent::Image {
                source: LLMMessageImageSource {
                    r#type: "base64".to_string(),
                    media_type: "image/png".to_string(),
                    data: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==".to_string(),
                },
            }]),
        };

        let stakai_msg = to_stakai_message(&msg);
        let parts = stakai_msg.parts();
        assert_eq!(parts.len(), 1);

        // Verify it's converted to a data URI
        if let ContentPart::Image { url, .. } = &parts[0] {
            assert!(url.starts_with("data:image/png;base64,"));
        } else {
            panic!("Expected Image content part");
        }

        let back = from_stakai_message(&stakai_msg);
        if let LLMMessageContent::List(parts) = back.content {
            if let LLMMessageTypedContent::Image { source } = &parts[0] {
                assert_eq!(source.media_type, "image/png");
                assert!(!source.data.is_empty());
            } else {
                panic!("Expected Image content");
            }
        } else {
            panic!("Expected List content");
        }
    }

    // ==================== Tool Conversion Tests ====================

    #[test]
    fn test_tool_conversion_basic() {
        let tool = LLMTool {
            name: "get_weather".to_string(),
            description: "Get weather for a location".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                }
            }),
        };

        let stakai_tool = to_stakai_tool(&tool);
        assert_eq!(stakai_tool.tool_type, "function");
        assert_eq!(stakai_tool.function.name, "get_weather");
        assert_eq!(
            stakai_tool.function.description,
            "Get weather for a location"
        );
        assert_eq!(stakai_tool.function.parameters["type"], "object");

        let back = from_stakai_tool(&stakai_tool);
        assert_eq!(back.name, "get_weather");
        assert_eq!(back.description, "Get weather for a location");
        assert_eq!(back.input_schema["type"], "object");
    }

    #[test]
    fn test_tool_conversion_complex_schema() {
        let tool = LLMTool {
            name: "search_database".to_string(),
            description: "Search a database with filters".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "filters": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "field": {"type": "string"},
                                "value": {"type": "string"}
                            }
                        }
                    },
                    "limit": {
                        "type": "integer",
                        "default": 10
                    }
                },
                "required": ["query"]
            }),
        };

        let stakai_tool = to_stakai_tool(&tool);
        let back = from_stakai_tool(&stakai_tool);

        assert_eq!(back.name, "search_database");
        assert_eq!(back.input_schema["properties"]["query"]["type"], "string");
        assert_eq!(back.input_schema["properties"]["filters"]["type"], "array");
        assert_eq!(back.input_schema["required"][0], "query");
    }

    // ==================== Stream Event Conversion Tests ====================

    #[test]
    fn test_stream_event_text_delta() {
        let event = StreamEvent::TextDelta {
            id: "gen_123".to_string(),
            delta: "Hello ".to_string(),
        };

        let delta = from_stakai_stream_event(&event);
        assert!(delta.is_some());

        if let Some(GenerationDelta::Content { content }) = delta {
            assert_eq!(content, "Hello ");
        } else {
            panic!("Expected Content delta");
        }
    }

    #[test]
    fn test_stream_event_reasoning_delta() {
        let event = StreamEvent::ReasoningDelta {
            id: "gen_123".to_string(),
            delta: "Let me think about this...".to_string(),
        };

        let delta = from_stakai_stream_event(&event);
        assert!(delta.is_some());

        if let Some(GenerationDelta::Thinking { thinking }) = delta {
            assert_eq!(thinking, "Let me think about this...");
        } else {
            panic!("Expected Thinking delta");
        }
    }

    #[test]
    fn test_stream_event_tool_call_start() {
        let event = StreamEvent::ToolCallStart {
            id: "call_xyz".to_string(),
            name: "run_command".to_string(),
        };

        let delta = from_stakai_stream_event(&event);
        assert!(delta.is_some());

        if let Some(GenerationDelta::ToolUse { tool_use }) = delta {
            assert_eq!(tool_use.id, Some("call_xyz".to_string()));
            assert_eq!(tool_use.name, Some("run_command".to_string()));
            assert!(tool_use.input.is_none());
        } else {
            panic!("Expected ToolUse delta");
        }
    }

    #[test]
    fn test_stream_event_tool_call_delta() {
        let event = StreamEvent::ToolCallDelta {
            id: "call_xyz".to_string(),
            delta: r#"{"command": "ls"#.to_string(),
        };

        let delta = from_stakai_stream_event(&event);
        assert!(delta.is_some());

        if let Some(GenerationDelta::ToolUse { tool_use }) = delta {
            assert_eq!(tool_use.id, Some("call_xyz".to_string()));
            assert!(tool_use.name.is_none());
            assert_eq!(tool_use.input, Some(r#"{"command": "ls"#.to_string()));
        } else {
            panic!("Expected ToolUse delta");
        }
    }

    #[test]
    fn test_stream_event_tool_call_end() {
        let event = StreamEvent::ToolCallEnd {
            id: "call_xyz".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command": "ls -la"}),
        };

        let delta = from_stakai_stream_event(&event);
        assert!(delta.is_some());

        if let Some(GenerationDelta::ToolUse { tool_use }) = delta {
            assert_eq!(tool_use.id, Some("call_xyz".to_string()));
            // ToolCallEnd emits name (for providers like Gemini) but NOT input
            // to avoid doubling arguments that were already accumulated via ToolCallDelta
            assert_eq!(tool_use.name, Some("run_command".to_string()));
            assert!(tool_use.input.is_none());
        } else {
            panic!("Expected ToolUse delta");
        }
    }

    #[test]
    fn test_stream_event_finish() {
        let event = StreamEvent::Finish {
            usage: Usage::new(100, 50),
            reason: FinishReason::stop(),
        };

        let delta = from_stakai_stream_event(&event);
        assert!(delta.is_some());

        if let Some(GenerationDelta::Usage { usage }) = delta {
            assert_eq!(usage.prompt_tokens, 100);
            assert_eq!(usage.completion_tokens, 50);
            assert_eq!(usage.total_tokens, 150);
        } else {
            panic!("Expected Usage delta");
        }
    }

    #[test]
    fn test_stream_event_start_returns_none() {
        let event = StreamEvent::Start {
            id: "gen_123".to_string(),
        };

        let delta = from_stakai_stream_event(&event);
        assert!(delta.is_none());
    }

    #[test]
    fn test_stream_event_error_returns_none() {
        let event = StreamEvent::Error {
            message: "Something went wrong".to_string(),
        };

        let delta = from_stakai_stream_event(&event);
        assert!(delta.is_none());
    }

    // ==================== Usage Conversion Tests ====================

    #[test]
    fn test_usage_conversion() {
        let usage = Usage::new(500, 200);

        let llm_usage = from_stakai_usage(&usage);
        assert_eq!(llm_usage.prompt_tokens, 500);
        assert_eq!(llm_usage.completion_tokens, 200);
        assert_eq!(llm_usage.total_tokens, 700);
        assert!(llm_usage.prompt_tokens_details.is_none());
    }

    // ==================== Finish Reason Tests ====================

    #[test]
    fn test_finish_reason_conversion() {
        assert_eq!(finish_reason_to_string(&FinishReason::stop()), "stop");
        assert_eq!(finish_reason_to_string(&FinishReason::length()), "length");
        assert_eq!(
            finish_reason_to_string(&FinishReason::content_filter()),
            "content_filter"
        );
        assert_eq!(
            finish_reason_to_string(&FinishReason::tool_calls()),
            "tool_calls"
        );
        assert_eq!(finish_reason_to_string(&FinishReason::other()), "other");
        assert_eq!(finish_reason_to_string(&FinishReason::error()), "error");

        // Test with raw values - should return the raw value
        use stakai::FinishReasonKind;
        let reason = FinishReason::with_raw(FinishReasonKind::Stop, "end_turn");
        assert_eq!(finish_reason_to_string(&reason), "end_turn");
    }

    // ==================== Model String Tests ====================

    #[test]
    fn test_model_string_anthropic() {
        let model = LLMModel::Anthropic(AnthropicModel::Claude45Sonnet);
        let model_str = get_stakai_model_string(&model);
        assert!(model_str.starts_with("anthropic/"));
        assert!(model_str.contains("claude"));
    }

    #[test]
    fn test_model_string_openai() {
        let model = LLMModel::OpenAI(OpenAIModel::GPT5);
        let model_str = get_stakai_model_string(&model);
        assert!(model_str.starts_with("openai/"));
        assert!(model_str.contains("gpt"));
    }

    #[test]
    fn test_model_string_gemini() {
        let model = LLMModel::Gemini(GeminiModel::Gemini25Flash);
        let model_str = get_stakai_model_string(&model);
        assert!(model_str.starts_with("google/"));
        assert!(model_str.contains("gemini"));
    }

    #[test]
    fn test_model_string_custom() {
        let model = LLMModel::Custom {
            provider: "litellm".to_string(),
            model: "claude-opus-4-5".to_string(),
            name: None,
        };
        let model_str = get_stakai_model_string(&model);
        assert_eq!(model_str, "litellm/claude-opus-4-5");
    }

    // ==================== Response Conversion Tests ====================

    #[test]
    fn test_response_conversion_text_only() {
        let response = GenerateResponse {
            content: vec![stakai::ResponseContent::Text {
                text: "Hello, how can I help?".to_string(),
            }],
            usage: Usage::new(10, 5),
            finish_reason: FinishReason::stop(),
            metadata: None,
            warnings: None,
        };

        let llm_response = from_stakai_response(response, "gpt-4");

        assert_eq!(llm_response.model, "gpt-4");
        assert_eq!(llm_response.object, "chat.completion");
        assert_eq!(llm_response.choices.len(), 1);
        assert_eq!(
            llm_response.choices[0].finish_reason,
            Some("stop".to_string())
        );
        assert_eq!(llm_response.choices[0].message.role, "assistant");

        if let LLMMessageContent::String(text) = &llm_response.choices[0].message.content {
            assert_eq!(text, "Hello, how can I help?");
        } else {
            panic!("Expected String content");
        }

        let usage = llm_response.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
    }

    #[test]
    fn test_response_conversion_with_tool_calls() {
        let response = GenerateResponse {
            content: vec![
                stakai::ResponseContent::Text {
                    text: "I'll check the weather for you.".to_string(),
                },
                stakai::ResponseContent::ToolCall(stakai::ToolCall {
                    id: "call_123".to_string(),
                    name: "get_weather".to_string(),
                    arguments: serde_json::json!({"location": "NYC"}),
                }),
            ],
            usage: Usage::new(20, 15),
            finish_reason: FinishReason::tool_calls(),
            metadata: None,
            warnings: None,
        };

        let llm_response = from_stakai_response(response, "claude-3");

        assert_eq!(
            llm_response.choices[0].finish_reason,
            Some("tool_calls".to_string())
        );

        if let LLMMessageContent::List(parts) = &llm_response.choices[0].message.content {
            assert_eq!(parts.len(), 2);

            // Check text part
            assert!(
                matches!(&parts[0], LLMMessageTypedContent::Text { text } if text == "I'll check the weather for you.")
            );

            // Check tool call part
            if let LLMMessageTypedContent::ToolCall { id, name, args } = &parts[1] {
                assert_eq!(id, "call_123");
                assert_eq!(name, "get_weather");
                assert_eq!(args["location"], "NYC");
            } else {
                panic!("Expected ToolCall");
            }
        } else {
            panic!("Expected List content");
        }
    }

    // ==================== Provider Options Conversion Tests ====================

    #[test]
    fn test_provider_options_anthropic_thinking() {
        use crate::models::llm::{LLMAnthropicOptions, LLMProviderOptions, LLMThinkingOptions};

        let opts = LLMProviderOptions {
            anthropic: Some(LLMAnthropicOptions {
                thinking: Some(LLMThinkingOptions::new(8000)),
            }),
            openai: None,
            google: None,
        };

        let model = LLMModel::Anthropic(AnthropicModel::Claude45Sonnet);
        let result = to_stakai_provider_options(&opts, &model);

        assert!(result.is_some());
        if let Some(ProviderOptions::Anthropic(anthropic)) = result {
            assert!(anthropic.thinking.is_some());
            assert_eq!(anthropic.thinking.unwrap().budget_tokens, 8000);
        } else {
            panic!("Expected Anthropic provider options");
        }
    }

    #[test]
    fn test_provider_options_openai_reasoning() {
        use crate::models::llm::{LLMOpenAIOptions, LLMProviderOptions};

        let opts = LLMProviderOptions {
            anthropic: None,
            openai: Some(LLMOpenAIOptions {
                reasoning_effort: Some("high".to_string()),
            }),
            google: None,
        };

        let model = LLMModel::OpenAI(OpenAIModel::GPT5);
        let result = to_stakai_provider_options(&opts, &model);

        assert!(result.is_some());
        if let Some(ProviderOptions::OpenAI(openai)) = result {
            assert_eq!(openai.reasoning_effort, Some(ReasoningEffort::High));
        } else {
            panic!("Expected OpenAI provider options");
        }
    }

    #[test]
    fn test_provider_options_google_thinking() {
        use crate::models::llm::{LLMGoogleOptions, LLMProviderOptions};

        let opts = LLMProviderOptions {
            anthropic: None,
            openai: None,
            google: Some(LLMGoogleOptions {
                thinking_budget: Some(5000),
            }),
        };

        let model = LLMModel::Gemini(GeminiModel::Gemini25Flash);
        let result = to_stakai_provider_options(&opts, &model);

        assert!(result.is_some());
        if let Some(ProviderOptions::Google(google)) = result {
            assert_eq!(google.thinking_budget, Some(5000));
        } else {
            panic!("Expected Google provider options");
        }
    }

    #[test]
    fn test_provider_options_none_when_empty() {
        use crate::models::llm::LLMProviderOptions;

        let opts = LLMProviderOptions::default();

        let model = LLMModel::Anthropic(AnthropicModel::Claude45Sonnet);
        let result = to_stakai_provider_options(&opts, &model);

        assert!(result.is_none());
    }

    // ==================== Config Building Tests ====================

    #[test]
    fn test_build_inference_config_empty() {
        let config = LLMProviderConfig::new();

        let result = build_inference_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_inference_config_with_openai() {
        use crate::models::llm::ProviderConfig;

        let mut config = LLMProviderConfig::new();
        config.add_provider(
            "openai",
            ProviderConfig::OpenAI {
                api_key: Some("sk-test-key".to_string()),
                api_endpoint: Some("https://api.openai.com/v1".to_string()),
            },
        );

        let result = build_inference_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_inference_config_with_anthropic() {
        use crate::models::llm::ProviderConfig;

        let mut config = LLMProviderConfig::new();
        config.add_provider(
            "anthropic",
            ProviderConfig::Anthropic {
                api_key: Some("sk-ant-test".to_string()),
                api_endpoint: None,
                access_token: None,
            },
        );

        let result = build_inference_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_inference_config_with_gemini() {
        use crate::models::llm::ProviderConfig;

        let mut config = LLMProviderConfig::new();
        config.add_provider(
            "gemini",
            ProviderConfig::Gemini {
                api_key: Some("gemini-test-key".to_string()),
                api_endpoint: None,
            },
        );

        let result = build_inference_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_inference_config_all_providers() {
        use crate::models::llm::ProviderConfig;

        let mut config = LLMProviderConfig::new();
        config.add_provider(
            "anthropic",
            ProviderConfig::Anthropic {
                api_key: Some("sk-ant-test".to_string()),
                api_endpoint: None,
                access_token: None,
            },
        );
        config.add_provider(
            "openai",
            ProviderConfig::OpenAI {
                api_key: Some("sk-openai-test".to_string()),
                api_endpoint: None,
            },
        );
        config.add_provider(
            "gemini",
            ProviderConfig::Gemini {
                api_key: Some("gemini-test".to_string()),
                api_endpoint: None,
            },
        );

        let result = build_inference_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_provider_registry_with_custom_providers() {
        use crate::models::llm::ProviderConfig;

        let mut config = LLMProviderConfig::new();
        config.add_provider(
            "litellm",
            ProviderConfig::Custom {
                api_endpoint: "http://localhost:4000".to_string(),
                api_key: Some("sk-1234".to_string()),
            },
        );
        config.add_provider(
            "ollama",
            ProviderConfig::Custom {
                api_endpoint: "http://localhost:11434/v1".to_string(),
                api_key: None,
            },
        );

        let result = build_provider_registry_direct(&config);
        assert!(
            result.is_ok(),
            "Failed to build registry: {:?}",
            result.err()
        );

        let registry = result.unwrap();
        assert!(registry.has_provider("litellm"));
        assert!(registry.has_provider("ollama"));
    }

    // ==================== Round-trip Tests ====================

    #[test]
    fn test_message_roundtrip_simple() {
        let original = LLMMessage {
            role: "user".to_string(),
            content: LLMMessageContent::String("What is 2+2?".to_string()),
        };

        let stakai_msg = to_stakai_message(&original);
        let back = from_stakai_message(&stakai_msg);

        assert_eq!(back.role, original.role);
        if let (LLMMessageContent::String(orig), LLMMessageContent::String(converted)) =
            (&original.content, &back.content)
        {
            assert_eq!(orig, converted);
        } else {
            panic!("Content type mismatch");
        }
    }

    #[test]
    fn test_message_roundtrip_complex() {
        let original = LLMMessage {
            role: "assistant".to_string(),
            content: LLMMessageContent::List(vec![
                LLMMessageTypedContent::Text {
                    text: "Here's the result:".to_string(),
                },
                LLMMessageTypedContent::ToolCall {
                    id: "call_001".to_string(),
                    name: "calculator".to_string(),
                    args: serde_json::json!({"expression": "2+2"}),
                },
            ]),
        };

        let stakai_msg = to_stakai_message(&original);
        let back = from_stakai_message(&stakai_msg);

        assert_eq!(back.role, original.role);
        if let LLMMessageContent::List(parts) = back.content {
            assert_eq!(parts.len(), 2);
        } else {
            panic!("Expected List content");
        }
    }

    #[test]
    fn test_tool_roundtrip() {
        let original = LLMTool {
            name: "file_reader".to_string(),
            description: "Read contents of a file".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to read"
                    }
                },
                "required": ["path"]
            }),
        };

        let stakai_tool = to_stakai_tool(&original);
        let back = from_stakai_tool(&stakai_tool);

        assert_eq!(back.name, original.name);
        assert_eq!(back.description, original.description);
        assert_eq!(back.input_schema, original.input_schema);
    }
}
