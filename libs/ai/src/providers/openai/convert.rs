//! Conversion between SDK types and OpenAI types

use super::types::*;
use crate::error::{Error, Result};
use crate::types::{
    ContentPart, FinishReason, FinishReasonKind, GenerateRequest, GenerateResponse, ImageDetail,
    InputTokenDetails, Message, OpenAIApiConfig, OutputTokenDetails, ProviderOptions,
    ReasoningEffort, ResponseContent, ResponsesConfig, Role, SystemMessageMode, ToolCall, Usage,
};
use serde_json::json;

/// Check if a model is a reasoning model (o1, o3, o4, gpt-5)
fn is_reasoning_model(model: &str) -> bool {
    let model_lower = model.to_lowercase();
    model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("o4")
        || model_lower.starts_with("gpt-5")
}

/// Convert SDK request to OpenAI request
pub fn to_openai_request(req: &GenerateRequest, stream: bool) -> ChatCompletionRequest {
    // Convert tools to OpenAI format
    let tools = req.options.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": tool.tool_type,
                    "function": {
                        "name": tool.function.name,
                        "description": tool.function.description,
                        "parameters": tool.function.parameters,
                    }
                })
            })
            .collect::<Vec<_>>()
    });

    // Convert tool_choice to OpenAI format
    let tool_choice = req.options.tool_choice.as_ref().map(|choice| match choice {
        crate::types::ToolChoice::Auto => json!("auto"),
        crate::types::ToolChoice::None => json!("none"),
        crate::types::ToolChoice::Required { name } => json!({
            "type": "function",
            "function": { "name": name }
        }),
    });

    // Determine system message mode
    // Default: for reasoning models, convert system to developer; otherwise keep as system
    let system_message_mode = req
        .provider_options
        .as_ref()
        .and_then(|opts| {
            if let ProviderOptions::OpenAI(openai_opts) = opts {
                openai_opts.system_message_mode
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            if is_reasoning_model(&req.model.id) {
                SystemMessageMode::Developer
            } else {
                SystemMessageMode::System
            }
        });

    // Convert messages with system message mode handling
    let messages: Vec<ChatMessage> = req
        .messages
        .iter()
        .filter_map(|msg| to_openai_message_with_mode(msg, system_message_mode))
        .collect();

    let temp = match is_reasoning_model(&req.model.id) {
        false => Some(0.0),
        true => None,
    };

    // Include usage in streaming responses
    let stream_options = if stream {
        Some(super::types::StreamOptions {
            include_usage: true,
        })
    } else {
        None
    };

    ChatCompletionRequest {
        model: req.model.id.clone(),
        messages,
        temperature: temp,
        max_completion_tokens: req.options.max_tokens,
        top_p: req.options.top_p,
        stop: req.options.stop_sequences.clone(),
        stream: Some(stream),
        stream_options,
        tools,
        tool_choice,
    }
}

/// Convert SDK message to OpenAI message with system message mode handling
fn to_openai_message_with_mode(msg: &Message, mode: SystemMessageMode) -> Option<ChatMessage> {
    let role = match msg.role {
        Role::System => {
            match mode {
                SystemMessageMode::System => "system",
                SystemMessageMode::Developer => "developer",
                SystemMessageMode::Remove => return None, // Skip system messages
            }
        }
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };

    // Get content parts from the message
    let parts = msg.parts();

    // Check if this is a tool result message
    let tool_call_id = parts.iter().find_map(|part| match part {
        ContentPart::ToolResult { tool_call_id, .. } => Some(tool_call_id.clone()),
        _ => None,
    });

    // Check if this message contains tool calls
    let tool_calls = parts
        .iter()
        .filter_map(|part| match part {
            ContentPart::ToolCall {
                id,
                name,
                arguments,
                ..
            } => Some(OpenAIToolCall {
                id: id.clone(),
                type_: "function".to_string(),
                function: OpenAIFunctionCall {
                    name: name.clone(),
                    arguments: arguments.to_string(),
                },
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    let tool_calls = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };

    let content = if parts.len() == 1 {
        // Single content part - use string format
        match &parts[0] {
            ContentPart::Text { text, .. } => Some(json!(text)),
            ContentPart::Image { url, detail, .. } => Some(json!([{
                "type": "image_url",
                "image_url": {
                    "url": url,
                    "detail": detail.map(|d| match d {
                        ImageDetail::Low => "low",
                        ImageDetail::High => "high",
                        ImageDetail::Auto => "auto",
                    })
                }
            }])),
            ContentPart::ToolCall { .. } => None, // Handled via tool_calls field
            ContentPart::ToolResult { content, .. } => Some(content.clone()),
        }
    } else {
        // Multiple content parts - use array format
        Some(json!(
            parts
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text { text, .. } => Some(json!({
                        "type": "text",
                        "text": text
                    })),
                    ContentPart::Image { url, detail, .. } => Some(json!({
                        "type": "image_url",
                        "image_url": {
                            "url": url,
                            "detail": detail.map(|d| match d {
                                ImageDetail::Low => "low",
                                ImageDetail::High => "high",
                                ImageDetail::Auto => "auto",
                            })
                        }
                    })),
                    ContentPart::ToolCall { .. } => None, // Handled via tool_calls field
                    ContentPart::ToolResult { .. } => None, // Handled separately via tool_call_id
                })
                .collect::<Vec<_>>()
        ))
    };

    Some(ChatMessage {
        role: role.to_string(),
        content,
        name: msg.name.clone(),
        tool_calls,
        tool_call_id,
    })
}

/// Convert OpenAI response to SDK response
pub fn from_openai_response(resp: ChatCompletionResponse) -> Result<GenerateResponse> {
    let choice = resp
        .choices
        .first()
        .ok_or_else(|| Error::invalid_response("No choices in response"))?;

    let content = parse_message_content(&choice.message)?;

    let finish_reason = parse_openai_finish_reason(choice.finish_reason.as_deref());

    // OpenAI: prompt_tokens_details.cached_tokens -> cacheRead (OpenAI doesn't report cacheWrite)
    let prompt_tokens = resp.usage.prompt_tokens;
    let completion_tokens = resp.usage.completion_tokens;

    let cached_tokens = resp
        .usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|d| d.cached_tokens)
        .unwrap_or(0);

    let reasoning_tokens = resp
        .usage
        .completion_tokens_details
        .as_ref()
        .and_then(|d| d.reasoning_tokens);

    let usage = Usage::with_details(
        InputTokenDetails {
            total: Some(prompt_tokens),
            no_cache: Some(prompt_tokens.saturating_sub(cached_tokens)),
            cache_read: if cached_tokens > 0 {
                Some(cached_tokens)
            } else {
                None
            },
            cache_write: None, // OpenAI doesn't report cache writes
        },
        OutputTokenDetails {
            total: Some(completion_tokens),
            text: reasoning_tokens.map(|r| completion_tokens.saturating_sub(r)),
            reasoning: reasoning_tokens,
        },
        Some(serde_json::to_value(&resp.usage).unwrap_or_default()),
    );

    Ok(GenerateResponse {
        content,
        usage,
        finish_reason,
        metadata: Some(json!({
            "id": resp.id,
            "model": resp.model,
            "created": resp.created,
            "object": resp.object,
        })),
        warnings: None, // OpenAI caching is automatic, no SDK-level validation warnings
    })
}

/// Parse OpenAI finish reason to unified finish reason
fn parse_openai_finish_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("stop") => FinishReason::with_raw(FinishReasonKind::Stop, "stop"),
        Some("length") => FinishReason::with_raw(FinishReasonKind::Length, "length"),
        Some("content_filter") => {
            FinishReason::with_raw(FinishReasonKind::ContentFilter, "content_filter")
        }
        Some("tool_calls") => FinishReason::with_raw(FinishReasonKind::ToolCalls, "tool_calls"),
        Some("function_call") => {
            FinishReason::with_raw(FinishReasonKind::ToolCalls, "function_call")
        }
        Some(raw) => FinishReason::with_raw(FinishReasonKind::Other, raw),
        None => FinishReason::other(),
    }
}

/// Parse message content from OpenAI format
fn parse_message_content(msg: &ChatMessage) -> Result<Vec<ResponseContent>> {
    let mut content = Vec::new();

    // Handle string content
    if let Some(content_value) = &msg.content
        && let Some(text) = content_value.as_str()
        && !text.is_empty()
    {
        content.push(ResponseContent::Text {
            text: text.to_string(),
        });
    }

    // Handle tool calls
    if let Some(tool_calls) = &msg.tool_calls {
        for tc in tool_calls {
            content.push(ResponseContent::ToolCall(ToolCall {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                arguments: serde_json::from_str(&tc.function.arguments)
                    .unwrap_or_else(|_| json!({})),
                metadata: None,
            }));
        }
    }

    Ok(content)
}

// ============================================================================
// Responses API Conversion
// ============================================================================

/// Get the API config from request, defaulting to Completions
pub fn get_api_config(req: &GenerateRequest) -> OpenAIApiConfig {
    if let Some(ProviderOptions::OpenAI(opts)) = &req.provider_options
        && let Some(api_config) = &opts.api_config
    {
        return api_config.clone();
    }
    // Default: Completions API
    OpenAIApiConfig::Completions(Default::default())
}

/// Check if using Responses API
pub fn is_responses_api(req: &GenerateRequest) -> bool {
    matches!(get_api_config(req), OpenAIApiConfig::Responses(_))
}

/// Convert SDK request to OpenAI Responses API request
pub fn to_responses_request(req: &GenerateRequest, stream: bool) -> ResponsesRequest {
    // Get Responses config
    let responses_config = match get_api_config(req) {
        OpenAIApiConfig::Responses(config) => config,
        _ => ResponsesConfig::default(),
    };

    // Convert tools to OpenAI Responses API format
    // Responses API uses flat format: { type, name, description, parameters }
    // No "strict" field or "function" wrapper
    let tools = req.options.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "name": tool.function.name,
                    "description": tool.function.description,
                    "parameters": tool.function.parameters,
                })
            })
            .collect::<Vec<_>>()
    });

    // Convert tool_choice to OpenAI format
    let tool_choice = req.options.tool_choice.as_ref().map(|choice| match choice {
        crate::types::ToolChoice::Auto => json!("auto"),
        crate::types::ToolChoice::None => json!("none"),
        crate::types::ToolChoice::Required { name } => json!({
            "type": "function",
            "function": { "name": name }
        }),
    });

    // Determine if this is a reasoning model
    let is_reasoning = is_reasoning_model(&req.model.id);

    // Build input items
    let mut input: Vec<serde_json::Value> = Vec::new();

    for (msg_index, msg) in req.messages.iter().enumerate() {
        let parts = msg.parts();

        match msg.role {
            Role::System => {
                // System messages use "developer" role for reasoning models, "system" otherwise
                // Content is a string, not array
                if let Some(text) = msg.text() {
                    let role = if is_reasoning { "developer" } else { "system" };
                    input.push(json!({
                        "role": role,
                        "content": text
                    }));
                }
            }
            Role::User => {
                // User messages have content as array of input_text/input_image items
                let content: Vec<serde_json::Value> = parts
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::Text { text, .. } => Some(json!({
                            "type": "input_text",
                            "text": text
                        })),
                        ContentPart::Image { url, .. } => Some(json!({
                            "type": "input_image",
                            "detail": "auto",
                            "image_url": url
                        })),
                        _ => None,
                    })
                    .collect();

                if !content.is_empty() {
                    input.push(json!({
                        "role": "user",
                        "content": content
                    }));
                }
            }
            Role::Assistant => {
                // Assistant content becomes individual items in the input array
                for part in parts.iter() {
                    match part {
                        ContentPart::Text { text, .. } => {
                            if !text.is_empty() {
                                input.push(json!({
                                    "type": "message",
                                    "role": "assistant",
                                    "content": [{
                                        "type": "output_text",
                                        "text": text,
                                        "annotations": []
                                    }],
                                    "status": "completed",
                                    "id": format!("msg_{}", msg_index)
                                }));
                            }
                        }
                        ContentPart::ToolCall {
                            id,
                            name,
                            arguments,
                            ..
                        } => {
                            // Tool call IDs use format: call_id|item_id
                            let call_id = if id.contains('|') {
                                id.split('|').next().unwrap_or(id).to_string()
                            } else {
                                id.clone()
                            };

                            // Omit the `id` field — OpenAI pairs function_call ids with
                            // reasoning item ids.  Without the matching reasoning item
                            // (which requires encrypted_content round-tripping), including
                            // the id causes a 400 "provided without its required reasoning
                            // item" error.
                            let fc = json!({
                                "type": "function_call",
                                "call_id": call_id,
                                "name": name,
                                "arguments": arguments.to_string()
                            });

                            input.push(fc);
                        }
                        _ => {}
                    }
                }
            }
            Role::Tool => {
                // Tool results use function_call_output format
                for part in parts.iter() {
                    if let ContentPart::ToolResult {
                        tool_call_id,
                        content,
                        ..
                    } = part
                    {
                        // Extract call_id (first part before |)
                        let call_id = if tool_call_id.contains('|') {
                            tool_call_id
                                .split('|')
                                .next()
                                .unwrap_or(tool_call_id)
                                .to_string()
                        } else {
                            tool_call_id.clone()
                        };

                        let output_text = if let Some(text) = content.as_str() {
                            text.to_string()
                        } else {
                            content.to_string()
                        };

                        input.push(json!({
                            "type": "function_call_output",
                            "call_id": call_id,
                            "output": output_text
                        }));
                    }
                }
            }
        }
    }

    // Build reasoning config for reasoning models (only if effort or summary is set)
    let reasoning = if is_reasoning {
        if responses_config.reasoning_effort.is_some()
            || responses_config.reasoning_summary.is_some()
        {
            let effort = responses_config
                .reasoning_effort
                .map(|e| match e {
                    ReasoningEffort::Low => "low",
                    ReasoningEffort::Medium => "medium",
                    ReasoningEffort::High => "high",
                })
                .unwrap_or("medium")
                .to_string();

            let summary = responses_config
                .reasoning_summary
                .map(|s| match s {
                    crate::types::ReasoningSummary::Auto => "auto",
                    crate::types::ReasoningSummary::Detailed => "detailed",
                })
                .unwrap_or("auto")
                .to_string();

            Some(ReasoningConfig {
                effort,
                summary: Some(summary),
            })
        } else {
            None
        }
    } else {
        None
    };

    // Include reasoning encrypted content when reasoning is enabled
    let include = if reasoning.is_some() {
        Some(vec!["reasoning.encrypted_content".to_string()])
    } else {
        None
    };

    // Reasoning models don't support temperature or top_p
    let temperature = if is_reasoning {
        None
    } else {
        req.options.temperature
    };
    let top_p = if is_reasoning {
        None
    } else {
        req.options.top_p
    };

    ResponsesRequest {
        model: req.model.id.clone(),
        input,
        instructions: None, // System message is in input array
        max_output_tokens: req.options.max_tokens,
        temperature,
        top_p,
        stream: Some(stream),
        tools,
        tool_choice,
        reasoning,
        include,
        prompt_cache_key: responses_config.session_id,
        prompt_cache_retention: responses_config.cache_retention,
        service_tier: responses_config.service_tier,
    }
}

/// Convert OpenAI Responses API response to SDK response
pub fn from_responses_response(resp: ResponsesResponse) -> Result<GenerateResponse> {
    let mut content = Vec::new();

    for output_item in &resp.output {
        match output_item {
            ResponsesOutputItem::Message {
                content: msg_content,
                ..
            } => {
                for item in msg_content {
                    let ResponsesOutputContent::Text { text } = item;
                    if !text.is_empty() {
                        content.push(ResponseContent::Text { text: text.clone() });
                    }
                }
            }
            ResponsesOutputItem::FunctionCall {
                call_id,
                name,
                arguments,
                ..
            } => {
                content.push(ResponseContent::ToolCall(ToolCall {
                    id: call_id.clone(),
                    name: name.clone(),
                    arguments: serde_json::from_str(arguments).unwrap_or_else(|_| json!({})),
                    metadata: None,
                }));
            }
            ResponsesOutputItem::Reasoning { .. } => {
                // Reasoning content can be exposed via metadata if needed
            }
        }
    }

    let finish_reason = match resp.status.as_str() {
        "completed" => FinishReason::with_raw(FinishReasonKind::Stop, "completed"),
        "incomplete" => FinishReason::with_raw(FinishReasonKind::Length, "incomplete"),
        "failed" => FinishReason::with_raw(FinishReasonKind::Other, "failed"),
        raw => FinishReason::with_raw(FinishReasonKind::Other, raw),
    };

    let cached_tokens = resp
        .usage
        .input_tokens_details
        .as_ref()
        .map(|d| d.cached_tokens)
        .unwrap_or(0);

    let reasoning_tokens = resp
        .usage
        .output_tokens_details
        .as_ref()
        .map(|d| d.reasoning_tokens);

    let usage = Usage::with_details(
        InputTokenDetails {
            total: Some(resp.usage.input_tokens),
            no_cache: Some(resp.usage.input_tokens.saturating_sub(cached_tokens)),
            cache_read: if cached_tokens > 0 {
                Some(cached_tokens)
            } else {
                None
            },
            cache_write: None,
        },
        OutputTokenDetails {
            total: Some(resp.usage.output_tokens),
            text: reasoning_tokens.map(|r| resp.usage.output_tokens.saturating_sub(r)),
            reasoning: reasoning_tokens,
        },
        Some(serde_json::to_value(&resp.usage).unwrap_or_default()),
    );

    Ok(GenerateResponse {
        content,
        usage,
        finish_reason,
        metadata: Some(json!({
            "id": resp.id,
            "model": resp.model,
            "created_at": resp.created_at,
            "object": resp.object,
            "status": resp.status,
        })),
        warnings: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CompletionsConfig, Model, OpenAIOptions};

    fn make_request(model_id: &str, provider_options: Option<ProviderOptions>) -> GenerateRequest {
        let mut req = GenerateRequest::new(
            Model::custom(model_id, "openai"),
            vec![Message::new(Role::User, "Hello")],
        );
        req.provider_options = provider_options;
        req
    }

    // =========================================================================
    // API Config Tests
    // =========================================================================

    #[test]
    fn test_default_is_completions_api() {
        let req = make_request("gpt-4o", None);
        assert!(!is_responses_api(&req));
    }

    #[test]
    fn test_explicit_completions_api() {
        let req = make_request(
            "gpt-4o",
            Some(ProviderOptions::OpenAI(OpenAIOptions {
                api_config: Some(OpenAIApiConfig::Completions(CompletionsConfig::default())),
                ..Default::default()
            })),
        );
        assert!(!is_responses_api(&req));
    }

    #[test]
    fn test_explicit_responses_api() {
        let req = make_request(
            "gpt-4o",
            Some(ProviderOptions::OpenAI(OpenAIOptions {
                api_config: Some(OpenAIApiConfig::Responses(ResponsesConfig::default())),
                ..Default::default()
            })),
        );
        assert!(is_responses_api(&req));
    }

    #[test]
    fn test_responses_api_with_reasoning_effort() {
        let req = make_request(
            "o3",
            Some(ProviderOptions::OpenAI(OpenAIOptions {
                api_config: Some(OpenAIApiConfig::Responses(ResponsesConfig {
                    reasoning_effort: Some(ReasoningEffort::High),
                    ..Default::default()
                })),
                ..Default::default()
            })),
        );
        assert!(is_responses_api(&req));
    }

    #[test]
    fn test_openai_options_completions_helper() {
        let opts = OpenAIOptions::completions();
        assert!(matches!(
            opts.api_config,
            Some(OpenAIApiConfig::Completions(_))
        ));
    }

    #[test]
    fn test_openai_options_responses_helper() {
        let opts = OpenAIOptions::responses();
        assert!(matches!(
            opts.api_config,
            Some(OpenAIApiConfig::Responses(_))
        ));
    }

    #[test]
    fn test_openai_options_responses_with_reasoning_helper() {
        let opts = OpenAIOptions::responses_with_reasoning(ReasoningEffort::High);
        if let Some(OpenAIApiConfig::Responses(config)) = opts.api_config {
            assert_eq!(config.reasoning_effort, Some(ReasoningEffort::High));
        } else {
            panic!("Expected Responses config");
        }
    }

    // =========================================================================
    // Reasoning Model Detection Tests
    // =========================================================================

    #[test]
    fn test_is_reasoning_model_o1() {
        assert!(is_reasoning_model("o1"));
        assert!(is_reasoning_model("o1-preview"));
        assert!(is_reasoning_model("o1-mini"));
        assert!(is_reasoning_model("O1-Preview")); // case insensitive
    }

    #[test]
    fn test_is_reasoning_model_o3() {
        assert!(is_reasoning_model("o3"));
        assert!(is_reasoning_model("o3-mini"));
    }

    #[test]
    fn test_is_reasoning_model_o4() {
        assert!(is_reasoning_model("o4"));
        assert!(is_reasoning_model("o4-mini"));
    }

    #[test]
    fn test_is_reasoning_model_gpt5() {
        assert!(is_reasoning_model("gpt-5"));
        assert!(is_reasoning_model("gpt-5-turbo"));
        assert!(is_reasoning_model("GPT-5")); // case insensitive
    }

    #[test]
    fn test_is_not_reasoning_model() {
        assert!(!is_reasoning_model("gpt-4"));
        assert!(!is_reasoning_model("gpt-4o"));
        assert!(!is_reasoning_model("gpt-4-turbo"));
        assert!(!is_reasoning_model("gpt-3.5-turbo"));
    }

    // =========================================================================
    // Request Conversion Tests
    // =========================================================================

    #[test]
    fn test_to_openai_request_basic() {
        let req = make_request("gpt-4o", None);
        let openai_req = to_openai_request(&req, false);

        assert_eq!(openai_req.model, "gpt-4o");
        assert_eq!(openai_req.stream, Some(false));
        assert_eq!(openai_req.messages.len(), 1);
        assert_eq!(openai_req.messages[0].role, "user");
    }

    #[test]
    fn test_to_openai_request_streaming() {
        let req = make_request("gpt-4o", None);
        let openai_req = to_openai_request(&req, true);

        assert_eq!(openai_req.stream, Some(true));
    }

    #[test]
    fn test_to_responses_request_basic() {
        let req = make_request(
            "gpt-4o",
            Some(ProviderOptions::OpenAI(OpenAIOptions::responses())),
        );
        let responses_req = to_responses_request(&req, false);

        assert_eq!(responses_req.model, "gpt-4o");
        assert_eq!(responses_req.stream, Some(false));
        assert_eq!(responses_req.input.len(), 1);
    }

    #[test]
    fn test_to_responses_request_with_reasoning() {
        let req = make_request(
            "o3",
            Some(ProviderOptions::OpenAI(OpenAIOptions {
                api_config: Some(OpenAIApiConfig::Responses(ResponsesConfig {
                    reasoning_effort: Some(ReasoningEffort::High),
                    ..Default::default()
                })),
                ..Default::default()
            })),
        );
        let responses_req = to_responses_request(&req, false);

        assert!(responses_req.reasoning.is_some());
        let reasoning = responses_req.reasoning.unwrap();
        assert_eq!(reasoning.effort, "high");
    }

    #[test]
    fn test_to_responses_request_with_service_tier() {
        let req = make_request(
            "gpt-4o",
            Some(ProviderOptions::OpenAI(OpenAIOptions {
                api_config: Some(OpenAIApiConfig::Responses(ResponsesConfig {
                    service_tier: Some("flex".to_string()),
                    ..Default::default()
                })),
                ..Default::default()
            })),
        );
        let responses_req = to_responses_request(&req, false);

        assert_eq!(responses_req.service_tier, Some("flex".to_string()));
    }

    #[test]
    fn test_to_responses_request_with_session_id() {
        let req = make_request(
            "gpt-4o",
            Some(ProviderOptions::OpenAI(OpenAIOptions {
                api_config: Some(OpenAIApiConfig::Responses(ResponsesConfig {
                    session_id: Some("my-session-123".to_string()),
                    ..Default::default()
                })),
                ..Default::default()
            })),
        );
        let responses_req = to_responses_request(&req, false);

        assert_eq!(
            responses_req.prompt_cache_key,
            Some("my-session-123".to_string())
        );
    }

    // =========================================================================
    // System Message Handling Tests
    // =========================================================================

    #[test]
    fn test_system_message_in_responses_api() {
        let mut req = GenerateRequest::new(
            Model::custom("gpt-4o", "openai"),
            vec![
                Message::new(Role::System, "You are a helpful assistant"),
                Message::new(Role::User, "Hello"),
            ],
        );
        req.provider_options = Some(ProviderOptions::OpenAI(OpenAIOptions::responses()));

        let responses_req = to_responses_request(&req, false);

        // System message should be in input (as system role for non-reasoning)
        assert_eq!(responses_req.input.len(), 2);

        // First item should be system message
        let first = &responses_req.input[0];
        assert_eq!(first["role"], "system");
    }

    #[test]
    fn test_system_message_as_developer_for_reasoning() {
        let mut req = GenerateRequest::new(
            Model::custom("o3", "openai"),
            vec![
                Message::new(Role::System, "You are a helpful assistant"),
                Message::new(Role::User, "Hello"),
            ],
        );
        req.provider_options = Some(ProviderOptions::OpenAI(OpenAIOptions::responses()));

        let responses_req = to_responses_request(&req, false);

        // System message should be developer role for reasoning models
        let first = &responses_req.input[0];
        assert_eq!(first["role"], "developer");
    }

    // =========================================================================
    // Response Conversion Tests
    // =========================================================================

    #[test]
    fn test_from_responses_response_completed() {
        let resp = ResponsesResponse {
            id: "resp_123".to_string(),
            object: "response".to_string(),
            created_at: 1234567890,
            model: "gpt-4o".to_string(),
            output: vec![ResponsesOutputItem::Message {
                id: "msg_123".to_string(),
                role: "assistant".to_string(),
                content: vec![ResponsesOutputContent::Text {
                    text: "Hello!".to_string(),
                }],
            }],
            usage: ResponsesUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                input_tokens_details: None,
                output_tokens_details: None,
            },
            status: "completed".to_string(),
        };

        let result = from_responses_response(resp).unwrap();

        assert_eq!(result.content.len(), 1);
        if let ResponseContent::Text { text } = &result.content[0] {
            assert_eq!(text, "Hello!");
        } else {
            panic!("Expected text content");
        }
        assert_eq!(result.finish_reason.unified, FinishReasonKind::Stop);
    }

    #[test]
    fn test_from_responses_response_with_tool_call() {
        let resp = ResponsesResponse {
            id: "resp_123".to_string(),
            object: "response".to_string(),
            created_at: 1234567890,
            model: "gpt-4o".to_string(),
            output: vec![ResponsesOutputItem::FunctionCall {
                id: "fc_123".to_string(),
                call_id: "call_456".to_string(),
                name: "get_weather".to_string(),
                arguments: r#"{"location":"NYC"}"#.to_string(),
            }],
            usage: ResponsesUsage::default(),
            status: "completed".to_string(),
        };

        let result = from_responses_response(resp).unwrap();

        assert_eq!(result.content.len(), 1);
        if let ResponseContent::ToolCall(tc) = &result.content[0] {
            assert_eq!(tc.id, "call_456");
            assert_eq!(tc.name, "get_weather");
            assert_eq!(tc.arguments["location"], "NYC");
        } else {
            panic!("Expected tool call content");
        }
    }

    #[test]
    fn test_from_responses_response_incomplete() {
        let resp = ResponsesResponse {
            id: "resp_123".to_string(),
            object: "response".to_string(),
            created_at: 1234567890,
            model: "gpt-4o".to_string(),
            output: vec![],
            usage: ResponsesUsage::default(),
            status: "incomplete".to_string(),
        };

        let result = from_responses_response(resp).unwrap();
        assert_eq!(result.finish_reason.unified, FinishReasonKind::Length);
    }

    // =========================================================================
    // Input Format Tests
    // =========================================================================

    #[test]
    fn test_user_message_format() {
        let req = make_request(
            "gpt-4o",
            Some(ProviderOptions::OpenAI(OpenAIOptions::responses())),
        );
        let responses_req = to_responses_request(&req, false);

        let user_msg = &responses_req.input[0];
        assert_eq!(user_msg["role"], "user");
        assert!(user_msg["content"].is_array());
        assert_eq!(user_msg["content"][0]["type"], "input_text");
        assert_eq!(user_msg["content"][0]["text"], "Hello");
    }

    #[test]
    fn test_tool_result_format() {
        use crate::types::MessageContent;

        // Create a tool result message
        let tool_result_content =
            ContentPart::tool_result("call_123", json!({"result": "success"}));
        let tool_msg = Message {
            role: Role::Tool,
            content: MessageContent::Parts(vec![tool_result_content]),
            name: None,
            provider_options: None,
        };

        let mut req = GenerateRequest::new(
            Model::custom("gpt-4o", "openai"),
            vec![Message::new(Role::User, "Hello"), tool_msg],
        );
        req.provider_options = Some(ProviderOptions::OpenAI(OpenAIOptions::responses()));

        let responses_req = to_responses_request(&req, false);

        // Find the function_call_output item
        let tool_result = responses_req
            .input
            .iter()
            .find(|item| item["type"] == "function_call_output");

        assert!(tool_result.is_some());
        let tool_result = tool_result.unwrap();
        assert_eq!(tool_result["call_id"], "call_123");
    }

    // =========================================================================
    // Temperature / Top-P Filtering Tests
    // =========================================================================

    #[test]
    fn test_responses_request_strips_temperature_for_reasoning_model() {
        let mut req = make_request(
            "gpt-5.2-2025-12-11",
            Some(ProviderOptions::OpenAI(OpenAIOptions::responses())),
        );
        req.options.temperature = Some(0.7);
        req.options.top_p = Some(0.9);

        let responses_req = to_responses_request(&req, false);

        // Reasoning models must not send temperature or top_p
        assert!(responses_req.temperature.is_none());
        assert!(responses_req.top_p.is_none());
    }

    #[test]
    fn test_responses_request_keeps_temperature_for_standard_model() {
        let mut req = make_request(
            "gpt-4o",
            Some(ProviderOptions::OpenAI(OpenAIOptions::responses())),
        );
        req.options.temperature = Some(0.7);
        req.options.top_p = Some(0.9);

        let responses_req = to_responses_request(&req, false);

        assert_eq!(responses_req.temperature, Some(0.7));
        assert_eq!(responses_req.top_p, Some(0.9));
    }
}
