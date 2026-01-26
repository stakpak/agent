//! Passthrough context manager - sends messages without compression
//!
//! This enables Anthropic's prompt caching by preserving the real message
//! structure. Each turn appends to the conversation, creating a stable
//! prefix that can be cached.
//!
//! # Cache Benefits
//!
//! Unlike compression-based managers that collapse all history into a single
//! message (which changes every turn), passthrough preserves the original
//! message structure:
//!
//! ```text
//! [System]           <- cached (stable)
//! [User msg 1]       <- cached after turn 1
//! [Assistant msg 1]  <- cached after turn 1
//! [User msg 2]       <- cached after turn 2
//! [Assistant msg 2]  <- cached after turn 2
//! [User msg N]       <- new (not cached yet)
//! ```
//!
//! This allows Anthropic to cache the entire conversation prefix, only
//! processing new tokens on each turn (90% cost reduction on cached tokens).

use stakpak_shared::models::{integrations::openai::ChatMessage, llm::LLMMessage};

/// A context manager that passes messages through without transformation.
///
/// Unlike compression-based managers, this preserves the original message
/// structure which enables effective prompt caching - the LLM only needs
/// to process new tokens while the conversation prefix is cached.
pub struct PassthroughContextManager;

impl super::ContextManager for PassthroughContextManager {
    fn reduce_context(&self, messages: Vec<ChatMessage>) -> Vec<LLMMessage> {
        messages.into_iter().map(LLMMessage::from).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::context_managers::ContextManager;
    use stakpak_shared::models::integrations::openai::{
        FunctionCall, MessageContent, Role, ToolCall,
    };
    use stakpak_shared::models::llm::{LLMMessageContent, LLMMessageTypedContent};

    #[test]
    fn test_passthrough_preserves_user_messages() {
        let manager = PassthroughContextManager;
        let messages = vec![ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String("Hello".to_string())),
            ..Default::default()
        }];

        let result = manager.reduce_context(messages);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "user");
        if let LLMMessageContent::String(text) = &result[0].content {
            assert_eq!(text, "Hello");
        } else {
            panic!("Expected string content");
        }
    }

    #[test]
    fn test_passthrough_preserves_assistant_messages() {
        let manager = PassthroughContextManager;
        let messages = vec![ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("Hi there!".to_string())),
            ..Default::default()
        }];

        let result = manager.reduce_context(messages);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "assistant");
    }

    #[test]
    fn test_passthrough_preserves_tool_calls() {
        let manager = PassthroughContextManager;
        let messages = vec![ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("Let me check that.".to_string())),
            tool_calls: Some(vec![ToolCall {
                id: "call_123".to_string(),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: "read_file".to_string(),
                    arguments: r#"{"path": "test.txt"}"#.to_string(),
                },
            }]),
            ..Default::default()
        }];

        let result = manager.reduce_context(messages);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "assistant");

        if let LLMMessageContent::List(parts) = &result[0].content {
            assert_eq!(parts.len(), 2); // text + tool call
            assert!(matches!(&parts[0], LLMMessageTypedContent::Text { .. }));
            assert!(matches!(&parts[1], LLMMessageTypedContent::ToolCall { .. }));
        } else {
            panic!("Expected list content with tool call");
        }
    }

    #[test]
    fn test_passthrough_preserves_tool_results() {
        let manager = PassthroughContextManager;
        let messages = vec![ChatMessage {
            role: Role::Tool,
            content: Some(MessageContent::String("File contents here".to_string())),
            tool_call_id: Some("call_123".to_string()),
            ..Default::default()
        }];

        let result = manager.reduce_context(messages);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "tool");

        if let LLMMessageContent::List(parts) = &result[0].content {
            assert_eq!(parts.len(), 1);
            if let LLMMessageTypedContent::ToolResult {
                tool_use_id,
                content,
            } = &parts[0]
            {
                assert_eq!(tool_use_id, "call_123");
                assert_eq!(content, "File contents here");
            } else {
                panic!("Expected ToolResult content");
            }
        } else {
            panic!("Expected list content with tool result");
        }
    }

    #[test]
    fn test_passthrough_preserves_message_order() {
        let manager = PassthroughContextManager;
        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("First".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("Second".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("Third".to_string())),
                ..Default::default()
            },
        ];

        let result = manager.reduce_context(messages);

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].role, "user");
        assert_eq!(result[1].role, "assistant");
        assert_eq!(result[2].role, "user");
    }

    #[test]
    fn test_passthrough_empty_messages() {
        let manager = PassthroughContextManager;
        let messages: Vec<ChatMessage> = vec![];

        let result = manager.reduce_context(messages);

        assert!(result.is_empty());
    }
}
