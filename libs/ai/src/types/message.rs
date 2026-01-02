//! Message types for AI conversations

use serde::{Deserialize, Serialize};

/// A message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// The role of the message sender
    pub role: Role,
    /// The content of the message - can be a string or array of content parts
    #[serde(with = "content_serde")]
    pub content: MessageContent,
    /// Optional name for the message sender
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Message content can be either a simple string or structured parts
#[derive(Debug, Clone)]
pub enum MessageContent {
    /// Simple text content
    Text(String),
    /// Structured content parts (for multimodal messages)
    Parts(Vec<ContentPart>),
}

impl MessageContent {
    /// Get all content parts, converting text to a single text part if needed
    pub fn parts(&self) -> Vec<ContentPart> {
        match self {
            MessageContent::Text(text) => vec![ContentPart::text(text.clone())],
            MessageContent::Parts(parts) => parts.clone(),
        }
    }

    /// Get the text content (if any)
    pub fn text(&self) -> Option<String> {
        match self {
            MessageContent::Text(text) => Some(text.clone()),
            MessageContent::Parts(parts) => parts
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .reduce(|mut acc, text| {
                    acc.push_str(&text);
                    acc
                }),
        }
    }
}

// Custom serde for MessageContent to handle both string and array
mod content_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(content: &MessageContent, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match content {
            MessageContent::Text(text) => serializer.serialize_str(text),
            MessageContent::Parts(parts) => parts.serialize(serializer),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<MessageContent, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        let value = serde_json::Value::deserialize(deserializer)?;

        match value {
            serde_json::Value::String(s) => Ok(MessageContent::Text(s)),
            serde_json::Value::Array(_) => {
                let parts: Vec<ContentPart> = serde_json::from_value(value)
                    .map_err(|e| D::Error::custom(format!("Invalid content parts: {}", e)))?;
                Ok(MessageContent::Parts(parts))
            }
            _ => Err(D::Error::custom("Content must be a string or array")),
        }
    }
}

impl Message {
    /// Create a new message with text content
    pub fn new(role: Role, content: impl Into<MessageContent>) -> Self {
        Self {
            role,
            content: content.into(),
            name: None,
        }
    }

    /// Get the text content of the message (if any)
    pub fn text(&self) -> Option<String> {
        self.content.text()
    }

    /// Get all content parts
    pub fn parts(&self) -> Vec<ContentPart> {
        self.content.parts()
    }
}

// Convenience conversions
impl From<String> for MessageContent {
    fn from(text: String) -> Self {
        MessageContent::Text(text)
    }
}

impl From<&str> for MessageContent {
    fn from(text: &str) -> Self {
        MessageContent::Text(text.to_string())
    }
}

impl From<Vec<ContentPart>> for MessageContent {
    fn from(parts: Vec<ContentPart>) -> Self {
        MessageContent::Parts(parts)
    }
}

/// The role of a message sender
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System message (instructions)
    System,
    /// User message
    User,
    /// Assistant message
    Assistant,
    /// Tool/function result message
    Tool,
}

/// A part of message content (text, image, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    /// Text content
    Text {
        /// The text content
        text: String,
    },
    /// Image content
    Image {
        /// Image URL or data URI
        url: String,
        /// Optional detail level for image processing
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },
    /// Tool/function call (for assistant messages in conversation history)
    ToolCall {
        /// Unique ID for this tool call
        id: String,
        /// Name of the function to call
        name: String,
        /// Arguments as JSON
        arguments: serde_json::Value,
    },
    /// Tool/function call result
    ToolResult {
        /// ID of the tool call this is responding to
        tool_call_id: String,
        /// Result content (can be text or JSON)
        content: serde_json::Value,
    },
}

impl ContentPart {
    /// Create a text content part
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Create an image content part from URL
    pub fn image(url: impl Into<String>) -> Self {
        Self::Image {
            url: url.into(),
            detail: None,
        }
    }

    /// Create an image content part with detail level
    pub fn image_with_detail(url: impl Into<String>, detail: ImageDetail) -> Self {
        Self::Image {
            url: url.into(),
            detail: Some(detail),
        }
    }

    /// Create a tool call content part
    pub fn tool_call(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: serde_json::Value,
    ) -> Self {
        Self::ToolCall {
            id: id.into(),
            name: name.into(),
            arguments,
        }
    }

    /// Create a tool result content part
    pub fn tool_result(tool_call_id: impl Into<String>, content: serde_json::Value) -> Self {
        Self::ToolResult {
            tool_call_id: tool_call_id.into(),
            content,
        }
    }
}

/// Image detail level for processing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageDetail {
    /// Low detail (faster, cheaper)
    Low,
    /// High detail (slower, more expensive)
    High,
    /// Auto-select based on image
    Auto,
}
