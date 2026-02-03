//! OpenTelemetry GenAI semantic conventions tracing support
//!
//! This module provides helpers for recording GenAI content as span attributes
//! according to [OpenTelemetry GenAI Semantic Conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/).
//!
//! ## Attribute Names (v1.38.0)
//!
//! | Attribute | Description |
//! |-----------|-------------|
//! | `gen_ai.operation.name` | Operation type: "chat", "stream", "embeddings" |
//! | `gen_ai.provider.name` | Provider: "openai", "anthropic", "gcp.gemini" |
//! | `gen_ai.request.model` | Requested model name |
//! | `gen_ai.response.model` | Actual model used in response |
//! | `gen_ai.response.id` | Unique completion ID |
//! | `gen_ai.input.messages` | Input messages (opt-in, JSON) |
//! | `gen_ai.output.messages` | Output messages (opt-in, JSON) |
//! | `gen_ai.usage.input_tokens` | Prompt tokens |
//! | `gen_ai.usage.output_tokens` | Completion tokens |
//! | `gen_ai.response.finish_reasons` | Array of finish reasons |
//!
//! ## Non-Standard Attributes
//!
//! The following attributes are NOT part of the official OTel GenAI semantic conventions
//! but are useful for tracking provider-specific cache token usage:
//!
//! | Attribute | Description |
//! |-----------|-------------|
//! | `gen_ai.usage.cache_read_input_tokens` | Tokens read from cache (cache hit) |
//! | `gen_ai.usage.cache_write_input_tokens` | Tokens written to cache (cache miss) |

use crate::types::{ContentPart, GenerateResponse, Message, ResponseContent, Role, Tool};
use std::collections::HashMap;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Tool call information for tracing
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Record input messages as span attribute `gen_ai.input.messages`
///
/// Records messages following the OTel GenAI input messages JSON schema:
/// ```json
/// [
///   {
///     "role": "user",
///     "parts": [
///       { "type": "text", "content": "Hello" }
///     ]
///   }
/// ]
/// ```
///
/// Note: This attribute contains potentially sensitive data and should be opt-in.
pub fn record_input_messages(messages: &[Message]) {
    let span = Span::current();

    let messages_json: Vec<serde_json::Value> =
        messages.iter().map(message_to_otel_format).collect();

    let json_str = serde_json::to_string(&messages_json).unwrap_or_default();
    span.record("gen_ai.input.messages", json_str.as_str());
}

/// Record response content as span attribute `gen_ai.output.messages`
///
/// Records the response following the OTel GenAI output messages JSON schema:
/// ```json
/// [
///   {
///     "role": "assistant",
///     "parts": [
///       { "type": "text", "content": "Hello!" }
///     ],
///     "finish_reason": "stop"
///   }
/// ]
/// ```
///
/// Note: This attribute contains potentially sensitive data and should be opt-in.
pub fn record_response_content(response: &GenerateResponse, finish_reason: &str) {
    let span = Span::current();

    let mut parts: Vec<serde_json::Value> = Vec::new();

    // Collect text content
    let text_content: String = response
        .content
        .iter()
        .filter_map(|c| match c {
            ResponseContent::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    if !text_content.is_empty() {
        parts.push(serde_json::json!({
            "type": "text",
            "content": text_content,
        }));
    }

    // Collect tool calls
    for content in &response.content {
        if let ResponseContent::ToolCall(tc) = content {
            parts.push(serde_json::json!({
                "type": "tool_call",
                "id": tc.id,
                "name": tc.name,
                "arguments": serde_json::from_str::<serde_json::Value>(&tc.arguments.to_string())
                    .unwrap_or(tc.arguments.clone()),
            }));
        }
    }

    // Single message representing the assistant's response
    let output_message = serde_json::json!({
        "role": "assistant",
        "parts": parts,
        "finish_reason": finish_reason,
    });

    // Output is an array of messages (one per choice/candidate)
    let output_messages = vec![output_message];
    let json_str = serde_json::to_string(&output_messages).unwrap_or_default();
    span.record("gen_ai.output.messages", json_str.as_str());
}

/// Record streamed response content as span attribute `gen_ai.output.messages`
pub fn record_streamed_response(
    text_content: &str,
    tool_calls: &[ToolCallInfo],
    finish_reason: &str,
) {
    let span = Span::current();

    let mut parts: Vec<serde_json::Value> = Vec::new();

    if !text_content.is_empty() {
        parts.push(serde_json::json!({
            "type": "text",
            "content": text_content,
        }));
    }

    for tc in tool_calls {
        parts.push(serde_json::json!({
            "type": "tool_call",
            "id": tc.id,
            "name": tc.name,
            "arguments": serde_json::from_str::<serde_json::Value>(&tc.arguments)
                .unwrap_or(serde_json::Value::String(tc.arguments.clone())),
        }));
    }

    let output_message = serde_json::json!({
        "role": "assistant",
        "parts": parts,
        "finish_reason": finish_reason,
    });

    let output_messages = vec![output_message];
    let json_str = serde_json::to_string(&output_messages).unwrap_or_default();
    span.record("gen_ai.output.messages", json_str.as_str());
}

// Helper functions

/// Convert a Message to OTel GenAI input message format
fn message_to_otel_format(message: &Message) -> serde_json::Value {
    let role = role_to_string(&message.role);
    let mut parts: Vec<serde_json::Value> = Vec::new();

    for part in message.parts() {
        match part {
            ContentPart::Text { text, .. } => {
                parts.push(serde_json::json!({
                    "type": "text",
                    "content": text,
                }));
            }
            ContentPart::ToolCall {
                id,
                name,
                arguments,
                ..
            } => {
                parts.push(serde_json::json!({
                    "type": "tool_call",
                    "id": id,
                    "name": name,
                    "arguments": serde_json::from_str::<serde_json::Value>(&arguments.to_string())
                        .unwrap_or(arguments.clone()),
                }));
            }
            ContentPart::ToolResult {
                tool_call_id,
                content,
                ..
            } => {
                parts.push(serde_json::json!({
                    "type": "tool_call_response",
                    "id": tool_call_id,
                    "result": content,
                }));
            }
            ContentPart::Image { .. } => {
                // Images are not included in tracing to avoid large payloads
                parts.push(serde_json::json!({
                    "type": "image",
                    "content": "[image omitted]",
                }));
            }
        }
    }

    serde_json::json!({
        "role": role,
        "parts": parts,
    })
}

fn role_to_string(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

/// Record custom telemetry metadata as direct span attributes.
///
/// Each key-value pair is recorded as a separate OpenTelemetry span attribute.
/// This allows dynamic attributes without pre-defining fields.
///
/// # Example
///
/// ```rust,ignore
/// use std::collections::HashMap;
///
/// let mut metadata = HashMap::new();
/// metadata.insert("user.id".to_string(), "user-123".to_string());
/// metadata.insert("user.name".to_string(), "John Doe".to_string());
/// metadata.insert("session.id".to_string(), "session-456".to_string());
///
/// record_telemetry_metadata(&metadata);
/// // Records as direct attributes:
/// //   user.id = "user-123"
/// //   user.name = "John Doe"
/// //   session.id = "session-456"
/// ```
pub fn record_telemetry_metadata(metadata: &HashMap<String, String>) {
    let span = Span::current();

    // Record each metadata field as a direct OpenTelemetry attribute
    for (key, value) in metadata {
        span.set_attribute(key.clone(), value.clone());
    }
}

/// Record available tool definitions on the current span.
///
/// Records the tool definitions following the OTel GenAI tool definitions schema:
/// ```json
/// [
///   {
///     "type": "function",
///     "name": "get_current_weather",
///     "description": "Get the current weather in a given location",
///     "parameters": { ... }
///   }
/// ]
/// ```
///
/// Note: This attribute is Opt-In per the GenAI semantic conventions as it could be large.
///
/// # Example
///
/// ```rust,ignore
/// use stakai::Tool;
/// use serde_json::json;
///
/// let tools = vec![
///     Tool::function("get_weather", "Get current weather")
///         .parameters(json!({"type": "object", "properties": {"location": {"type": "string"}}})),
/// ];
///
/// record_tool_definitions(&tools);
/// // Records: gen_ai.tool.definitions = [{"type": "function", "name": "get_weather", ...}]
/// ```
pub fn record_tool_definitions(tools: &[Tool]) {
    let span = Span::current();

    let tools_json: Vec<serde_json::Value> = tools.iter().map(tool_to_otel_format).collect();

    let json_str = serde_json::to_string(&tools_json).unwrap_or_default();
    span.record("gen_ai.tool.definitions", json_str.as_str());
}

/// Convert a Tool to the OTel GenAI tool definition format
fn tool_to_otel_format(tool: &Tool) -> serde_json::Value {
    serde_json::json!({
        "type": tool.tool_type,
        "name": tool.function.name,
        "description": tool.function.description,
        "parameters": tool.function.parameters,
    })
}
