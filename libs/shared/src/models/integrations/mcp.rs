use rmcp::model::{Annotated, CallToolResult, Content, RawContent};

use crate::models::integrations::openai::{ChatMessage, MessageContent, ToolCallResultStatus};

pub trait CallToolResultExt {
    /// Create a success result with a simple text message
    fn cancel(content: Option<&Vec<Annotated<RawContent>>>) -> Self;
    fn get_status(&self) -> ToolCallResultStatus;
    fn get_status_from_chat_message(message: &ChatMessage) -> ToolCallResultStatus;
}

impl CallToolResultExt for CallToolResult {
    fn cancel(content: Option<&Vec<Annotated<RawContent>>>) -> Self {
        let mut error_content = vec![Content::text("TOOL_CALL_CANCELLED")];

        // Convert RawContent to Content by extracting text and creating new Content
        if let Some(content) = content {
            for raw in content {
                if let Some(text_content) = raw.as_text() {
                    error_content.push(Content::text(&text_content.text));
                }
            }
        }

        CallToolResult::error(error_content)
    }

    fn get_status(&self) -> ToolCallResultStatus {
        if self.is_error == Some(true) {
            // Check if any content contains the cancellation message
            let is_cancelled = self.content.iter().any(|content| {
                if let Some(text_content) = content.raw.as_text() {
                    text_content.text.contains("TOOL_CALL_CANCELLED")
                } else {
                    false
                }
            });

            if is_cancelled {
                ToolCallResultStatus::Cancelled
            } else {
                ToolCallResultStatus::Error
            }
        } else {
            ToolCallResultStatus::Success
        }
    }

    fn get_status_from_chat_message(message: &ChatMessage) -> ToolCallResultStatus {
        match message
            .content
            .as_ref()
            .unwrap_or(&MessageContent::String(String::new()))
            .to_string()
            .contains("TOOL_CALL_CANCELLED")
            || message
                .content
                .as_ref()
                .unwrap_or(&MessageContent::String(String::new()))
                .to_string()
                .contains("cancelled")
        {
            true => ToolCallResultStatus::Cancelled,
            false => ToolCallResultStatus::Success,
        }
    }
}
