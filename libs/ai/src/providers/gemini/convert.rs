//! Conversion between unified types and Gemini types

use super::types::{
    GeminiContent, GeminiFunctionCall, GeminiFunctionDeclaration,
    GeminiFunctionResponse, GeminiGenerationConfig, GeminiInlineData, GeminiPart, GeminiRequest,
    GeminiResponse, GeminiSystemInstruction, GeminiThinkingConfig, GeminiTool,
};
use crate::error::{Error, Result};
use crate::types::{
    ContentPart, FinishReason, GenerateRequest, GenerateResponse, Message,
    ProviderOptions, ResponseContent, Role, Usage,
};

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

    Ok(GeminiRequest {
        contents,
        generation_config,
        safety_settings: None, // Could be added to options later
        tools,
        system_instruction,
        tool_config,
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
            ContentPart::Text { text } => GeminiPart {
                text: Some(text.clone()),
                inline_data: None,
                function_call: None,
                function_response: None,
            },
            ContentPart::Image { url, detail: _ } => {
                // Parse image data
                match parse_image_data(url) {
                    Ok(inline_data) => GeminiPart {
                        text: None,
                        inline_data: Some(inline_data),
                        function_call: None,
                        function_response: None,
                    },
                    Err(_) => GeminiPart {
                        text: Some(format!("[Image: {}]", url)),
                        inline_data: None,
                        function_call: None,
                        function_response: None,
                    },
                }
            }
            ContentPart::ToolCall {
                id,
                name,
                arguments,
            } => {
                // Gemini function call
                GeminiPart {
                    text: None,
                    inline_data: None,
                    function_call: Some(GeminiFunctionCall {
                        id: Some(id.clone()),
                        name: name.clone(),
                        args: arguments.clone(),
                    }),
                    function_response: None,
                }
            }
            ContentPart::ToolResult {
                tool_call_id,
                content,
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

                GeminiPart {
                    text: None,
                    inline_data: None,
                    function_call: None,
                    function_response: Some(GeminiFunctionResponse {
                        id: tool_call_id.clone(),
                        name,
                        response: content.clone(),
                    }),
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

    let candidates = resp.candidates.unwrap_or_default();
    let candidate = candidates.first();

    let mut content: Vec<ResponseContent> = Vec::new();

    if let Some(candidate) = candidate {
        for part in candidate
            .content
            .as_ref()
            .map(|c| c.parts.as_slice())
            .unwrap_or_default()
        {
            if let Some(text) = &part.text {
                content.push(ResponseContent::Text { text: text.clone() });
            }

            if let Some(function_call) = &part.function_call {
                content.push(ResponseContent::ToolCall(ToolCall {
                    id: function_call
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4())),
                    name: function_call.name.clone(),
                    arguments: function_call.args.clone(),
                }));
            }
        }
    }

    if content.is_empty() {
        content.push(ResponseContent::Text {
            text: String::new(),
        });
    }

    let usage = resp
        .usage_metadata
        .as_ref()
        .map(|u| Usage {
            prompt_tokens: u.prompt_token_count.unwrap_or(0),
            completion_tokens: u.candidates_token_count.unwrap_or(0),
            total_tokens: u.total_token_count.unwrap_or(0),
        })
        .unwrap_or_default();

    // Determine finish reason - function_call should be ToolCalls
    let finish_reason = if content
        .iter()
        .any(|c| matches!(c, ResponseContent::ToolCall(_)))
    {
        FinishReason::ToolCalls
    } else {
        candidate
            .and_then(|c| parse_finish_reason(&c.finish_reason))
            .unwrap_or(FinishReason::Other)
    };

    Ok(GenerateResponse {
        content,
        usage,
        finish_reason,
        metadata: None,
    })
}

/// Parse Gemini finish reason to unified finish reason
pub(super) fn parse_finish_reason(reason: &Option<String>) -> Option<FinishReason> {
    reason.as_ref().and_then(|r| match r.as_str() {
        "STOP" => Some(FinishReason::Stop),
        "MAX_TOKENS" => Some(FinishReason::Length),
        "SAFETY" => Some(FinishReason::ContentFilter),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::GeminiCandidate;

    #[test]
    fn test_to_gemini_content_tool_result() {
        let msg = Message {
            role: Role::Tool,
            content: crate::types::MessageContent::Parts(vec![ContentPart::ToolResult {
                tool_call_id: "call_123".to_string(),
                content: serde_json::json!({"temp": 22, "name": "get_weather"}),
            }]),
            name: None,
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
