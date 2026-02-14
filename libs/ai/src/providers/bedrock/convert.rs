//! Conversion between unified types and Bedrock-compatible request/response formats
//!
//! Bedrock uses the same Messages API body as direct Anthropic, with these differences:
//! 1. `anthropic_version` goes in the body as `"bedrock-2023-05-31"` (not an HTTP header)
//! 2. `model` field is removed from the body (it goes in the SDK call as `model_id`)
//! 3. `anthropic_beta` features move from HTTP headers to a JSON array in the body
//! 4. `stream` field is removed (determined by endpoint choice: invoke vs invoke-with-response-stream)
//!
//! This module reuses `to_anthropic_request()` from the Anthropic provider and applies
//! a thin transformation layer on top. No message/tool conversion logic is duplicated.

use crate::error::Result;
use crate::providers::anthropic::convert::to_anthropic_request;
use crate::providers::anthropic::types::AnthropicConfig;
use crate::types::{CacheWarning, GenerateRequest};

/// Bedrock anthropic_version value (different from direct API's "2023-06-01")
const BEDROCK_ANTHROPIC_VERSION: &str = "bedrock-2023-05-31";

/// Result of converting a request to Bedrock format
pub struct BedrockConversionResult {
    /// The JSON body to send to Bedrock's InvokeModel API
    pub body: serde_json::Value,
    /// The model ID to pass to the SDK call (extracted from the request)
    pub model_id: String,
    /// Warnings generated during conversion (e.g., cache validation)
    pub warnings: Vec<CacheWarning>,
}

/// Convert a unified GenerateRequest into a Bedrock-compatible JSON body
///
/// This wraps `to_anthropic_request()` and applies Bedrock-specific transformations:
/// - Replaces `anthropic_version` with `"bedrock-2023-05-31"`
/// - Removes `model` from body (returned separately as `model_id`)
/// - Removes `stream` field (Bedrock determines streaming by endpoint choice)
///
/// Note: Bedrock supports prompt caching natively via `cache_control` in the
/// request body — no `anthropic_beta` flag is needed (and Bedrock rejects it).
pub fn to_bedrock_body(
    req: &GenerateRequest,
    config: &AnthropicConfig,
) -> Result<BedrockConversionResult> {
    // Reuse the Anthropic conversion (stream=false since Bedrock ignores the field)
    let conversion_result = to_anthropic_request(req, config, false)?;

    // Serialize the Anthropic request to a mutable JSON value
    let mut body = serde_json::to_value(&conversion_result.request).map_err(|e| {
        crate::error::Error::invalid_response(format!("Failed to serialize request: {}", e))
    })?;

    // Extract model_id and resolve to Bedrock format if needed
    let model_id = super::models::resolve_bedrock_model_id(&req.model.id);

    // Apply Bedrock-specific transformations
    if let serde_json::Value::Object(ref mut map) = body {
        // 1. Add anthropic_version to body
        map.insert(
            "anthropic_version".to_string(),
            serde_json::Value::String(BEDROCK_ANTHROPIC_VERSION.to_string()),
        );

        // 2. Remove model from body (goes in SDK call)
        map.remove("model");

        // 3. Remove stream from body (determined by endpoint choice)
        map.remove("stream");

        // 4. Bedrock does NOT use the `anthropic_beta` body field.
        //    Prompt caching is supported natively via `cache_control` in the request body —
        //    no beta flag needed. Bedrock rejects `anthropic_beta` with "invalid beta flag".
        //    See: https://docs.aws.amazon.com/bedrock/latest/userguide/prompt-caching.html
    }

    Ok(BedrockConversionResult {
        body,
        model_id,
        warnings: conversion_result.warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::anthropic::types::AnthropicConfig;
    use crate::types::{GenerateRequest, Message, Model, Role};

    /// Helper to create a dummy AnthropicConfig for Bedrock conversion
    /// (Bedrock doesn't use the API key, but the conversion layer needs a valid config)
    fn dummy_anthropic_config() -> AnthropicConfig {
        AnthropicConfig::new("dummy-key-for-bedrock")
    }

    #[test]
    fn test_bedrock_body_has_anthropic_version() {
        let request = GenerateRequest::new(
            Model::custom("anthropic.claude-sonnet-4-5-20250929-v1:0", "bedrock"),
            vec![Message::new(Role::User, "Hello")],
        );

        let result = to_bedrock_body(&request, &dummy_anthropic_config()).unwrap();

        assert_eq!(result.body["anthropic_version"], "bedrock-2023-05-31");
    }

    #[test]
    fn test_bedrock_body_removes_model() {
        let request = GenerateRequest::new(
            Model::custom("anthropic.claude-sonnet-4-5-20250929-v1:0", "bedrock"),
            vec![Message::new(Role::User, "Hello")],
        );

        let result = to_bedrock_body(&request, &dummy_anthropic_config()).unwrap();

        assert!(result.body.get("model").is_none());
        assert_eq!(result.model_id, "anthropic.claude-sonnet-4-5-20250929-v1:0");
    }

    #[test]
    fn test_bedrock_body_removes_stream() {
        let request = GenerateRequest::new(
            Model::custom("anthropic.claude-sonnet-4-5-20250929-v1:0", "bedrock"),
            vec![Message::new(Role::User, "Hello")],
        );

        let result = to_bedrock_body(&request, &dummy_anthropic_config()).unwrap();

        assert!(result.body.get("stream").is_none());
    }

    #[test]
    fn test_bedrock_body_no_anthropic_beta() {
        // Bedrock does NOT use anthropic_beta — prompt caching is native.
        // Verify the field is never present in the output body.
        let request = GenerateRequest::new(
            Model::custom("anthropic.claude-sonnet-4-5-20250929-v1:0", "bedrock"),
            vec![Message::new(Role::User, "Hello")],
        );

        let result = to_bedrock_body(&request, &dummy_anthropic_config()).unwrap();

        assert!(
            result.body.get("anthropic_beta").is_none(),
            "anthropic_beta must NOT be present in Bedrock body — Bedrock rejects it"
        );
    }

    #[test]
    fn test_bedrock_body_cache_control_without_beta() {
        // Bedrock supports cache_control natively — no beta flag needed.
        // CacheStrategy::Auto adds cache_control breakpoints, but anthropic_beta must NOT appear.
        let request = GenerateRequest::new(
            Model::custom("anthropic.claude-sonnet-4-5-20250929-v1:0", "bedrock"),
            vec![Message::new(Role::User, "Hello")],
        );

        let result = to_bedrock_body(&request, &dummy_anthropic_config()).unwrap();

        // anthropic_beta must NOT be present
        assert!(
            result.body.get("anthropic_beta").is_none(),
            "anthropic_beta must NOT be present even when cache_control is used"
        );
    }

    #[test]
    fn test_bedrock_body_preserves_messages() {
        let request = GenerateRequest::new(
            Model::custom("anthropic.claude-sonnet-4-5-20250929-v1:0", "bedrock"),
            vec![Message::new(Role::User, "What is Rust?")],
        );

        let result = to_bedrock_body(&request, &dummy_anthropic_config()).unwrap();

        let messages = result.body["messages"].as_array().unwrap();
        assert!(!messages.is_empty());
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn test_bedrock_body_preserves_max_tokens() {
        let request = GenerateRequest::new(
            Model::custom("anthropic.claude-sonnet-4-5-20250929-v1:0", "bedrock"),
            vec![Message::new(Role::User, "Hello")],
        );

        let result = to_bedrock_body(&request, &dummy_anthropic_config()).unwrap();

        // max_tokens should be present (inferred from model name)
        assert!(result.body.get("max_tokens").is_some());
    }

    #[test]
    fn test_bedrock_body_preserves_cache_control_in_messages_system_tools() {
        use crate::types::{CacheControl, GenerateOptions, Tool};
        use serde_json::json;

        // Build a request with cache_control on system message, user message, and tool
        let system_msg = Message::new(Role::System, "You are a helpful assistant.")
            .with_cache_control(CacheControl::ephemeral());

        let user_msg =
            Message::new(Role::User, "Hello").with_cache_control(CacheControl::ephemeral());

        let tool = Tool::function("search", "Search documents")
            .parameters(json!({"type": "object", "properties": {}}))
            .with_cache_control(CacheControl::ephemeral());

        let options = GenerateOptions::default().add_tool(tool);

        let mut request = GenerateRequest::new(
            Model::custom("anthropic.claude-sonnet-4-5-20250929-v1:0", "bedrock"),
            vec![system_msg, user_msg],
        );
        request.options = options;

        let result = to_bedrock_body(&request, &dummy_anthropic_config()).unwrap();

        // Verify cache_control is present in the system content
        let system = result.body.get("system").expect("system field must exist");
        let system_arr = system.as_array().expect("system should be an array");
        let has_system_cache = system_arr
            .iter()
            .any(|block| block.get("cache_control").is_some());
        assert!(
            has_system_cache,
            "cache_control must be preserved on system message, got: {system_arr:?}"
        );

        // Verify cache_control is present in user message content
        let messages = result.body["messages"].as_array().expect("messages array");
        let user_msg_body = &messages[0]; // first non-system message
        assert_eq!(user_msg_body["role"], "user");
        // User message with cache_control gets converted to block format
        let content = &user_msg_body["content"];
        let has_msg_cache = if let Some(arr) = content.as_array() {
            arr.iter().any(|block| block.get("cache_control").is_some())
        } else {
            false
        };
        assert!(
            has_msg_cache,
            "cache_control must be preserved on user message, got: {content:?}"
        );

        // Verify cache_control is present on tools
        let tools = result.body.get("tools").expect("tools field must exist");
        let tools_arr = tools.as_array().expect("tools should be an array");
        let has_tool_cache = tools_arr
            .iter()
            .any(|tool| tool.get("cache_control").is_some());
        assert!(
            has_tool_cache,
            "cache_control must be preserved on tool definitions, got: {tools_arr:?}"
        );

        // Verify anthropic_beta is NOT present (Bedrock doesn't use it)
        assert!(
            result.body.get("anthropic_beta").is_none(),
            "anthropic_beta must NOT be present in Bedrock body"
        );
    }

    #[test]
    fn test_anthropic_model_id_maps_to_bedrock() {
        // Using Anthropic-style model ID should auto-map to cross-region Bedrock format
        let request = GenerateRequest::new(
            Model::custom("claude-sonnet-4-5-20250929", "bedrock"),
            vec![Message::new(Role::User, "Hello")],
        );

        let result = to_bedrock_body(&request, &dummy_anthropic_config()).unwrap();

        assert_eq!(
            result.model_id,
            "us.anthropic.claude-sonnet-4-5-20250929-v1:0"
        );
        // model should still be removed from body
        assert!(result.body.get("model").is_none());
    }

    #[test]
    fn test_cross_region_model_id_passthrough() {
        let request = GenerateRequest::new(
            Model::custom("us.anthropic.claude-sonnet-4-5-20250929-v1:0", "bedrock"),
            vec![Message::new(Role::User, "Hello")],
        );

        let result = to_bedrock_body(&request, &dummy_anthropic_config()).unwrap();

        assert_eq!(
            result.model_id,
            "us.anthropic.claude-sonnet-4-5-20250929-v1:0"
        );
    }
}
