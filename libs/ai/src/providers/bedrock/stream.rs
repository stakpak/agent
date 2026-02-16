//! Bedrock streaming support
//!
//! Bedrock uses AWS EventStream (binary framing protocol) instead of SSE.
//! The Rust SDK abstracts this — we get `EventReceiver<ResponseStream>` which
//! yields `ResponseStream::Chunk(PayloadPart)` events. Each chunk's `bytes`
//! field contains a JSON blob that follows the **same Anthropic streaming event
//! format** (message_start, content_block_start, content_block_delta, etc.).
//!
//! We reuse the Anthropic stream event processing logic since the JSON payloads
//! are identical.

use crate::error::{Error, Result};
use crate::providers::anthropic::types::AnthropicStreamEvent;
use crate::types::{
    FinishReason, FinishReasonKind, GenerateStream, InputTokenDetails, StreamEvent, Usage,
};
use aws_sdk_bedrockruntime::primitives::event_stream::EventReceiver;
use aws_sdk_bedrockruntime::types::{PayloadPart, ResponseStream};

/// Track state for each content block during streaming
#[derive(Debug, Clone)]
enum ContentBlock {
    Text,
    Reasoning,
    ToolCall {
        id: String,
        name: String,
        input: String,
    },
}

/// Create a GenerateStream from a Bedrock EventStream receiver
pub async fn create_stream(
    receiver: EventReceiver<
        ResponseStream,
        aws_sdk_bedrockruntime::types::error::ResponseStreamError,
    >,
) -> Result<GenerateStream> {
    let stream = async_stream::stream! {
        let mut accumulated_usage = Usage::default();
        let mut content_blocks: std::collections::HashMap<u32, ContentBlock> =
            std::collections::HashMap::new();
        let mut receiver = receiver;

        loop {
            match receiver.recv().await {
                Ok(Some(event)) => {
                    match event {
                        ResponseStream::Chunk(payload_part) => {
                            match parse_payload_part(&payload_part) {
                                Ok(Some(anthropic_event)) => {
                                    for stream_event in process_bedrock_event(
                                        anthropic_event,
                                        &mut accumulated_usage,
                                        &mut content_blocks,
                                    ) {
                                        yield Ok(stream_event);
                                    }
                                }
                                Ok(None) => {
                                    // Empty chunk, skip
                                    continue;
                                }
                                Err(e) => {
                                    yield Err(e);
                                    break;
                                }
                            }
                        }
                        _ => {
                            // Unknown event variant — skip for forward compatibility
                            continue;
                        }
                    }
                }
                Ok(None) => {
                    // Stream ended normally
                    break;
                }
                Err(e) => {
                    yield Err(Error::stream_error(format!("Bedrock stream error: {:?}", e)));
                    break;
                }
            }
        }
    };

    Ok(GenerateStream::new(Box::pin(stream)))
}

/// Parse a Bedrock PayloadPart into an Anthropic stream event
fn parse_payload_part(part: &PayloadPart) -> Result<Option<AnthropicStreamEvent>> {
    let Some(ref bytes) = part.bytes else {
        return Ok(None);
    };

    let json_bytes = bytes.as_ref();
    if json_bytes.is_empty() {
        return Ok(None);
    }

    serde_json::from_slice::<AnthropicStreamEvent>(json_bytes)
        .map(Some)
        .map_err(|e| Error::stream_error(format!("Failed to parse Bedrock event: {}", e)))
}

/// Process a Bedrock stream event (which is an Anthropic-format event)
///
/// The JSON payloads from Bedrock are identical to direct Anthropic streaming events,
/// so we use the same processing logic.
fn process_bedrock_event(
    event: AnthropicStreamEvent,
    accumulated_usage: &mut Usage,
    content_blocks: &mut std::collections::HashMap<u32, ContentBlock>,
) -> Vec<StreamEvent> {
    use crate::providers::anthropic::types::AnthropicContent;

    match event.type_.as_str() {
        "message_start" => {
            if let Some(message) = event.message {
                let usage = &message.usage;
                let cache_write = usage.cache_creation_input_tokens.unwrap_or(0);
                let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
                let input_tokens = usage.input_tokens;
                let total_input = input_tokens + cache_write + cache_read;

                accumulated_usage.prompt_tokens = total_input;
                accumulated_usage.input_token_details = Some(InputTokenDetails {
                    total: Some(total_input),
                    no_cache: Some(input_tokens),
                    cache_read: (cache_read > 0).then_some(cache_read),
                    cache_write: (cache_write > 0).then_some(cache_write),
                });
            }
            Vec::new()
        }
        "content_block_start" => {
            let index = event.index.unwrap_or(0);

            match event.content_block {
                Some(AnthropicContent::ToolUse { id, name, .. }) => {
                    content_blocks.insert(
                        index,
                        ContentBlock::ToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            input: String::new(),
                        },
                    );
                    vec![StreamEvent::tool_call_start(id, name)]
                }
                Some(AnthropicContent::Text { .. }) => {
                    content_blocks.insert(index, ContentBlock::Text);
                    Vec::new()
                }
                Some(AnthropicContent::Thinking { .. })
                | Some(AnthropicContent::RedactedThinking { .. }) => {
                    content_blocks.insert(index, ContentBlock::Reasoning);
                    Vec::new()
                }
                _ => Vec::new(),
            }
        }
        "content_block_delta" => {
            let index = event.index.unwrap_or(0);

            if let Some(delta) = event.delta {
                match delta.type_.as_deref() {
                    Some("text_delta") => {
                        if let Some(text) = delta.text {
                            return vec![StreamEvent::text_delta("", text)];
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(thinking) = delta.thinking {
                            return vec![StreamEvent::reasoning_delta("", thinking)];
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(partial_json) = delta.partial_json {
                            if let Some(ContentBlock::ToolCall { id, input, .. }) =
                                content_blocks.get_mut(&index)
                            {
                                input.push_str(&partial_json);
                                return vec![StreamEvent::tool_call_delta(
                                    id.clone(),
                                    partial_json,
                                )];
                            }
                            return vec![StreamEvent::tool_call_delta(
                                index.to_string(),
                                partial_json,
                            )];
                        }
                    }
                    _ => {}
                }
            }
            Vec::new()
        }
        "content_block_stop" => {
            let index = event.index.unwrap_or(0);

            if let Some(block) = content_blocks.remove(&index)
                && let ContentBlock::ToolCall { id, name, input } = block
            {
                let input_json = if input.is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::from_str(&input).unwrap_or(serde_json::json!({}))
                };
                return vec![StreamEvent::tool_call_end(id, name, input_json)];
            }
            Vec::new()
        }
        "message_delta" => {
            if let Some(usage) = event.usage {
                accumulated_usage.completion_tokens = usage.output_tokens;
                accumulated_usage.total_tokens =
                    accumulated_usage.prompt_tokens + usage.output_tokens;
            }
            Vec::new()
        }
        "message_stop" => {
            vec![StreamEvent::finish(
                accumulated_usage.clone(),
                FinishReason::with_raw(FinishReasonKind::Stop, "message_stop"),
            )]
        }
        "error" => {
            let message = event
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "Bedrock API error".to_string());
            vec![StreamEvent::error(message)]
        }
        _ => Vec::new(),
    }
}
