//! Conversion between unified types and Gemini types

use super::types::{
    GeminiContent, GeminiFunctionCall, GeminiFunctionDeclaration, GeminiFunctionResponse,
    GeminiGenerationConfig, GeminiInlineData, GeminiPart, GeminiRequest, GeminiResponse,
    GeminiSystemInstruction, GeminiThinkingConfig, GeminiTool,
};
use crate::error::{Error, Result};
use crate::types::{
    ContentPart, FinishReason, FinishReasonKind, GenerateRequest, GenerateResponse,
    InputTokenDetails, Message, OutputTokenDetails, ProviderOptions, ResponseContent, Role, Usage,
};
use serde_json::json;

/// Convert unified request to Gemini request
pub fn to_gemini_request(req: &GenerateRequest) -> Result<GeminiRequest> {
    use serde_json::json;

    // Gemini has separate system instructions
    let (system_instruction, contents) = convert_messages(&req.messages)?;

    // Extract Google options if present
    let google_opts = if let Some(ProviderOptions::Google(opts)) = &req.provider_options {
        Some(opts)
    } else {
        None
    };

    let generation_config = Some(GeminiGenerationConfig {
        temperature: req.options.temperature,
        top_p: req.options.top_p,
        top_k: None, // Gemini-specific, not in unified options
        max_output_tokens: req.options.max_tokens,
        stop_sequences: req.options.stop_sequences.clone(),
        response_mime_type: None,
        candidate_count: None,
        seed: None,
        presence_penalty: None,
        frequency_penalty: None,
        response_logprobs: None,
        logprobs: None,
        enable_enhanced_civic_answers: None,
        thinking_config: google_opts.and_then(|opts| {
            opts.thinking_budget.map(|budget| GeminiThinkingConfig {
                include_thoughts: Some(true),
                thinking_budget: Some(budget),
            })
        }),
        speech_config: None,
        media_resolution: None,
        response_modalities: None,
    });

    // Convert tools to Gemini format
    let tools = req.options.tools.as_ref().map(|tools| {
        vec![GeminiTool {
            function_declarations: tools
                .iter()
                .map(|tool| GeminiFunctionDeclaration {
                    name: tool.function.name.clone(),
                    description: tool.function.description.clone(),
                    parameters_json_schema: Some(tool.function.parameters.clone()),
                })
                .collect::<Vec<_>>(),
        }]
    });

    // Convert tool_choice to Gemini format
    let tool_config = req.options.tool_choice.as_ref().map(|choice| {
        let mode = match choice {
            crate::types::ToolChoice::Auto => "AUTO",
            crate::types::ToolChoice::None => "NONE",
            crate::types::ToolChoice::Required { .. } => "ANY",
        };
        json!({
            "function_calling_config": {
                "mode": mode
            }
        })
    });

    // Extract cached_content from Google options
    let cached_content = google_opts.and_then(|opts| opts.cached_content.clone());

    Ok(GeminiRequest {
        contents,
        generation_config,
        safety_settings: None, // Could be added to options later
        tools,
        system_instruction,
        tool_config,
        cached_content,
    })
}

/// Convert messages to Gemini format, separating system instructions
fn convert_messages(
    messages: &[Message],
) -> Result<(Option<GeminiSystemInstruction>, Vec<GeminiContent>)> {
    let mut result = Vec::new();
    let mut system_parts = Vec::new();

    // Collect system messages
    for msg in messages {
        if msg.role == Role::System {
            let content = to_gemini_content(msg)?;
            system_parts.extend(content.parts);
        }
    }

    let system_instruction = if system_parts.is_empty() {
        None
    } else {
        Some(GeminiSystemInstruction {
            parts: system_parts,
        })
    };

    // Convert non-system messages
    for msg in messages {
        if msg.role == Role::System {
            continue; // Already handled
        }

        let content = to_gemini_content(msg)?;
        result.push(content);
    }

    Ok((system_instruction, result))
}

/// Convert unified message to Gemini content
fn to_gemini_content(msg: &Message) -> Result<GeminiContent> {
    let role = match msg.role {
        Role::User | Role::System => "user",
        Role::Assistant => "model", // Gemini uses "model" instead of "assistant"
        Role::Tool => "function",
    };

    let content_parts = msg.parts();
    let parts: Vec<GeminiPart> = content_parts
        .iter()
        .map(|part| match part {
            ContentPart::Text { text, .. } => GeminiPart {
                text: Some(text.clone()),
                inline_data: None,
                function_call: None,
                function_response: None,
                thought_signature: None,
            },
            ContentPart::Image { url, .. } => {
                // Parse image data
                match parse_image_data(url) {
                    Ok(inline_data) => GeminiPart {
                        text: None,
                        inline_data: Some(inline_data),
                        function_call: None,
                        function_response: None,
                        thought_signature: None,
                    },
                    Err(_) => GeminiPart {
                        text: Some(format!("[Image: {}]", url)),
                        inline_data: None,
                        function_call: None,
                        function_response: None,
                        thought_signature: None,
                    },
                }
            }
            ContentPart::ToolCall {
                id,
                name,
                arguments,
                metadata,
                ..
            } => {
                // Extract thought_signature from metadata to place on the Part level
                let thought_signature = metadata.as_ref().and_then(|m| {
                    m.get("thought_signature")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });

                GeminiPart {
                    text: None,
                    inline_data: None,
                    function_call: Some(GeminiFunctionCall {
                        id: Some(id.clone()),
                        name: name.clone(),
                        args: arguments.clone(),
                    }),
                    function_response: None,
                    thought_signature,
                }
            }
            ContentPart::ToolResult {
                tool_call_id,
                content,
                ..
            } => {
                // Gemini function response
                // We'll extract the function name from the content if possible,
                // but Gemini now supports ID-based matching in some versions.
                // For now, we'll try to get the name from the content if it's an object,
                // otherwise use "unknown".
                let name = if let Some(obj) = content.as_object() {
                    obj.get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string()
                } else {
                    "unknown".to_string()
                };

                // Gemini requires function_response.response to be a JSON object
                // (google.protobuf.Struct). Wrap non-object values in {"result": ...}.
                let response = if content.is_object() {
                    content.clone()
                } else {
                    json!({ "result": content })
                };

                GeminiPart {
                    text: None,
                    inline_data: None,
                    function_call: None,
                    function_response: Some(GeminiFunctionResponse {
                        id: tool_call_id.clone(),
                        name,
                        response,
                    }),
                    thought_signature: None,
                }
            }
        })
        .collect();

    Ok(GeminiContent {
        role: role.to_string(),
        parts,
    })
}

/// Parse image URL to Gemini inline data format
fn parse_image_data(url: &str) -> Result<GeminiInlineData> {
    if url.starts_with("data:") {
        // Data URL format: data:image/png;base64,iVBORw0KG...
        let parts: Vec<&str> = url.splitn(2, ',').collect();
        if parts.len() != 2 {
            return Err(Error::invalid_response("Invalid data URL format"));
        }

        let mime_type = parts[0]
            .strip_prefix("data:")
            .and_then(|s| s.strip_suffix(";base64"))
            .ok_or_else(|| Error::invalid_response("Invalid data URL media type"))?;

        Ok(GeminiInlineData {
            mime_type: mime_type.to_string(),
            data: parts[1].to_string(),
        })
    } else {
        // URL format (Gemini doesn't support direct URLs)
        Err(Error::invalid_response(
            "Gemini requires base64-encoded images, not URLs",
        ))
    }
}

/// Convert Gemini response to unified response
pub fn from_gemini_response(resp: GeminiResponse) -> Result<GenerateResponse> {
    use crate::types::ToolCall;

    let candidate = resp.candidates.as_ref().and_then(|c| c.first());

    let mut content: Vec<ResponseContent> = Vec::new();
    let mut has_tool_calls = false;

    if let Some(candidate) = candidate
        && let Some(c) = &candidate.content
    {
        for part in &c.parts {
            if let Some(text) = &part.text {
                content.push(ResponseContent::Text { text: text.clone() });
            }

            if let Some(function_call) = &part.function_call {
                has_tool_calls = true;

                // Preserve thought_signature from the Part level in metadata
                // so it can be echoed back in subsequent requests
                let metadata = part
                    .thought_signature
                    .as_ref()
                    .map(|sig| json!({ "thought_signature": sig }));

                content.push(ResponseContent::ToolCall(ToolCall {
                    id: function_call
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4())),
                    name: function_call.name.clone(),
                    arguments: function_call.args.clone(),
                    metadata,
                }));
            }
        }
    }

    // Ensure at least one content item (matches OpenAI/Gemini behavior)
    if content.is_empty() {
        content.push(ResponseContent::Text {
            text: String::new(),
        });
    }

    // Gemini: cachedContentTokenCount -> cacheRead (Gemini doesn't report cacheWrite)
    let usage = resp
        .usage_metadata
        .map(|u| {
            let prompt_tokens = u.prompt_token_count.unwrap_or(0);
            let completion_tokens = u.candidates_token_count.unwrap_or(0);
            let cached_tokens = u.cached_content_token_count.unwrap_or(0);

            let reasoning_tokens = u.thoughts_token_count;

            Usage::with_details(
                InputTokenDetails {
                    total: Some(prompt_tokens),
                    no_cache: Some(prompt_tokens.saturating_sub(cached_tokens)),
                    cache_read: if cached_tokens > 0 {
                        Some(cached_tokens)
                    } else {
                        None
                    },
                    cache_write: None, // Gemini doesn't report cache writes
                },
                OutputTokenDetails {
                    total: Some(completion_tokens),
                    text: reasoning_tokens.map(|r| completion_tokens.saturating_sub(r)),
                    reasoning: reasoning_tokens,
                },
                Some(serde_json::to_value(&u).unwrap_or_default()),
            )
        })
        .unwrap_or_default();

    let finish_reason = if has_tool_calls {
        FinishReason::with_raw(FinishReasonKind::ToolCalls, "TOOL_CALLS")
    } else {
        candidate
            .and_then(|c| c.finish_reason.as_deref())
            .map(parse_finish_reason)
            .unwrap_or_else(FinishReason::other)
    };

    Ok(GenerateResponse {
        content,
        usage,
        finish_reason,
        metadata: Some(json!({
            "model_version": resp.model_version,
            "response_id": resp.response_id,
        })),
        warnings: None, // Gemini doesn't have SDK-level cache validation warnings
    })
}

/// Parse Gemini finish reason to unified finish reason
pub(super) fn parse_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "STOP" => FinishReason::with_raw(FinishReasonKind::Stop, "STOP"),
        "MAX_TOKENS" => FinishReason::with_raw(FinishReasonKind::Length, "MAX_TOKENS"),
        "SAFETY" => FinishReason::with_raw(FinishReasonKind::ContentFilter, "SAFETY"),
        "RECITATION" => FinishReason::with_raw(FinishReasonKind::ContentFilter, "RECITATION"),
        "OTHER" => FinishReason::with_raw(FinishReasonKind::Other, "OTHER"),
        raw => FinishReason::with_raw(FinishReasonKind::Other, raw),
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::GeminiCandidate;
    use super::*;

    #[test]
    fn test_to_gemini_content_tool_result() {
        let msg = Message {
            role: Role::Tool,
            content: crate::types::MessageContent::Parts(vec![ContentPart::ToolResult {
                tool_call_id: "call_123".to_string(),
                content: serde_json::json!({"temp": 22, "name": "get_weather"}),
                provider_options: None,
            }]),
            name: None,
            provider_options: None,
        };

        let result = to_gemini_content(&msg).unwrap();
        assert_eq!(result.role, "function");
        assert_eq!(result.parts.len(), 1);
        let part = &result.parts[0];
        assert!(part.function_response.is_some());
        let resp = part.function_response.as_ref().unwrap();
        assert_eq!(resp.id, "call_123");
        assert_eq!(resp.name, "get_weather");
        assert_eq!(resp.response["temp"], 22);
    }

    #[test]
    fn test_to_gemini_content_tool_result_string_wrapped() {
        // Gemini requires function_response.response to be a JSON object (Struct).
        // When tool result content is a string, it must be wrapped in {"result": ...}.
        let msg = Message {
            role: Role::Tool,
            content: crate::types::MessageContent::Parts(vec![ContentPart::ToolResult {
                tool_call_id: "call_456".to_string(),
                content: serde_json::json!("File: README.md\n  1: # Hello"),
                provider_options: None,
            }]),
            name: None,
            provider_options: None,
        };

        let result = to_gemini_content(&msg).unwrap();
        let part = &result.parts[0];
        let resp = part.function_response.as_ref().unwrap();
        assert_eq!(resp.id, "call_456");
        // String should be wrapped in an object
        assert!(
            resp.response.is_object(),
            "response must be a JSON object for Gemini, got: {:?}",
            resp.response
        );
        assert_eq!(resp.response["result"], "File: README.md\n  1: # Hello");
    }

    #[test]
    fn test_from_gemini_response_tool_call() {
        let resp = GeminiResponse {
            candidates: Some(vec![GeminiCandidate {
                content: Some(GeminiContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart {
                        text: None,
                        inline_data: None,
                        function_call: Some(GeminiFunctionCall {
                            id: Some("call_123".to_string()),
                            name: "get_weather".to_string(),
                            args: serde_json::json!({"location": "London"}),
                        }),
                        function_response: None,
                        thought_signature: None,
                    }],
                }),
                finish_reason: Some("STOP".to_string()),
                safety_ratings: None,
            }]),
            usage_metadata: None,
            model_version: None,
            response_id: None,
        };

        let result = from_gemini_response(resp).unwrap();
        assert_eq!(result.content.len(), 1);
        if let ResponseContent::ToolCall(call) = &result.content[0] {
            assert_eq!(call.id, "call_123");
            assert_eq!(call.name, "get_weather");
            assert_eq!(call.arguments["location"], "London");
        } else {
            panic!("Expected ToolCall");
        }
    }

    #[test]
    fn test_convert_messages_system_instruction() {
        let messages = vec![
            Message::new(Role::System, "You are a helpful assistant."),
            Message::new(Role::User, "Hello!"),
        ];

        let (system_instruction, contents) = convert_messages(&messages).unwrap();
        assert!(system_instruction.is_some());
        let si = system_instruction.unwrap();
        assert_eq!(si.parts.len(), 1);
        assert_eq!(
            si.parts[0].text,
            Some("You are a helpful assistant.".to_string())
        );

        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0].role, "user");
        assert_eq!(contents[0].parts[0].text, Some("Hello!".to_string()));
    }
}
