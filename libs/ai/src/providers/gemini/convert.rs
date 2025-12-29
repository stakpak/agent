//! Conversion between unified types and Gemini types

use super::types::{
    GeminiContent, GeminiGenerationConfig, GeminiInlineData, GeminiPart, GeminiRequest,
    GeminiResponse,
};
use crate::error::{Error, Result};
use crate::types::{
    ContentPart, FinishReason, GenerateRequest, GenerateResponse, Message, ProviderOptions,
    ResponseContent, Role, Usage,
};

/// Convert unified request to Gemini request
pub fn to_gemini_request(req: &GenerateRequest) -> Result<GeminiRequest> {
    use serde_json::json;

    // Gemini doesn't have separate system messages - prepend to first user message
    let contents = convert_messages(&req.messages)?;

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
            opts.thinking_budget
                .map(|budget| super::types::GeminiThinkingConfig {
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
        vec![json!({
            "function_declarations": tools.iter().map(|tool| {
                json!({
                    "name": tool.function.name,
                    "description": tool.function.description,
                    "parameters": tool.function.parameters,
                })
            }).collect::<Vec<_>>()
        })]
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
        tool_config,
    })
}

/// Convert messages to Gemini format
fn convert_messages(messages: &[Message]) -> Result<Vec<GeminiContent>> {
    let mut result = Vec::new();
    let mut system_text = String::new();

    // Collect system messages
    for msg in messages {
        if msg.role == Role::System
            && let Some(text) = msg.text()
        {
            if !system_text.is_empty() {
                system_text.push_str("\n\n");
            }
            system_text.push_str(&text);
        }
    }

    // Convert non-system messages
    let mut first_user_message = true;
    for msg in messages {
        if msg.role == Role::System {
            continue; // Already handled
        }

        let mut content = to_gemini_content(msg)?;

        // Prepend system message to first user message
        if first_user_message && content.role == "user" && !system_text.is_empty() {
            content.parts.insert(
                0,
                GeminiPart {
                    text: Some(format!("System instructions: {}\n\n", system_text)),
                    inline_data: None,
                    function_call: None,
                    function_response: None,
                },
            );
            first_user_message = false;
        }

        result.push(content);
    }

    Ok(result)
}

/// Convert unified message to Gemini content
fn to_gemini_content(msg: &Message) -> Result<GeminiContent> {
    let role = match msg.role {
        Role::User | Role::System => "user",
        Role::Assistant => "model", // Gemini uses "model" instead of "assistant"
        Role::Tool => {
            return Err(Error::invalid_response(
                "Tool messages not yet supported for Gemini",
            ));
        }
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
                id: _,
                name,
                arguments,
            } => {
                // Gemini function call
                GeminiPart {
                    text: None,
                    inline_data: None,
                    function_call: Some(super::types::GeminiFunctionCall {
                        name: name.clone(),
                        args: arguments.clone(),
                    }),
                    function_response: None,
                }
            }
            ContentPart::ToolResult {
                tool_call_id: _,
                content,
            } => {
                // Gemini function response
                // Note: Gemini doesn't use call IDs, just function names
                // We'll extract the function name from the content if possible
                let (name, response) = if let Some(obj) = content.as_object() {
                    let name = obj
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let response = obj.get("result").cloned().unwrap_or(content.clone());
                    (name, response)
                } else {
                    ("unknown".to_string(), content.clone())
                };

                GeminiPart {
                    text: None,
                    inline_data: None,
                    function_call: None,
                    function_response: Some(super::types::GeminiFunctionResponse {
                        name,
                        response,
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

    let candidate = resp
        .candidates
        .first()
        .ok_or_else(|| Error::invalid_response("No candidates in response"))?;

    let mut content: Vec<ResponseContent> = Vec::new();

    for part in &candidate.content.parts {
        if let Some(text) = &part.text {
            content.push(ResponseContent::Text { text: text.clone() });
        }

        if let Some(function_call) = &part.function_call {
            // Gemini doesn't provide IDs, so we generate one
            content.push(ResponseContent::ToolCall(ToolCall {
                id: format!("call_{}", uuid::Uuid::new_v4()),
                name: function_call.name.clone(),
                arguments: function_call.args.clone(),
            }));
        }
    }

    if content.is_empty() {
        return Err(Error::invalid_response("No content in response"));
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
        parse_finish_reason(&candidate.finish_reason).unwrap_or(FinishReason::Other)
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

    #[test]
    fn test_parse_image_data() {
        let data_url = "data:image/png;base64,iVBORw0KGgoAAAANS";
        let result = parse_image_data(data_url).unwrap();

        assert_eq!(result.mime_type, "image/png");
        assert_eq!(result.data, "iVBORw0KGgoAAAANS");
    }
}
