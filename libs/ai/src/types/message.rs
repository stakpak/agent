//! Message types for AI conversations

use super::cache::CacheControl;
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
    /// Provider-specific options (e.g., cache control for Anthropic)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<MessageProviderOptions>,
}

/// Provider-specific options for a message
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageProviderOptions {
    /// Anthropic-specific message options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<AnthropicMessageOptions>,
}

impl MessageProviderOptions {
    /// Create Anthropic-specific options with cache control
    pub fn anthropic_cache(cache_control: CacheControl) -> Self {
        Self {
            anthropic: Some(AnthropicMessageOptions {
                cache_control: Some(cache_control),
            }),
        }
    }
}

/// Anthropic-specific message options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicMessageOptions {
    /// Cache control for this message (Anthropic prompt caching)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
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
                    ContentPart::Text { text, .. } => Some(text.clone()),
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
            provider_options: None,
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

    /// Set the message sender name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Add Anthropic cache control to this message
    ///
    /// # Example
    ///
    /// ```rust
    /// use stakai::{Message, Role, CacheControl};
    ///
    /// let msg = Message::new(Role::System, "System prompt...")
    ///     .with_cache_control(CacheControl::ephemeral());
    /// ```
    pub fn with_cache_control(mut self, cache_control: CacheControl) -> Self {
        self.provider_options = Some(MessageProviderOptions::anthropic_cache(cache_control));
        self
    }

    /// Add provider-specific options to this message
    pub fn with_provider_options(mut self, options: MessageProviderOptions) -> Self {
        self.provider_options = Some(options);
        self
    }

    /// Get the cache control from provider options (if set for Anthropic)
    pub fn cache_control(&self) -> Option<&CacheControl> {
        self.provider_options
            .as_ref()
            .and_then(|opts| opts.anthropic.as_ref())
            .and_then(|anthropic| anthropic.cache_control.as_ref())
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

/// Provider-specific options for content parts
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContentPartProviderOptions {
    /// Anthropic-specific content part options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<AnthropicContentPartOptions>,
}

impl ContentPartProviderOptions {
    /// Create Anthropic-specific options with cache control
    pub fn anthropic_cache(cache_control: CacheControl) -> Self {
        Self {
            anthropic: Some(AnthropicContentPartOptions {
                cache_control: Some(cache_control),
            }),
        }
    }
}

/// Anthropic-specific content part options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicContentPartOptions {
    /// Cache control for this content part (Anthropic prompt caching)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

/// A part of message content (text, image, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    /// Text content
    Text {
        /// The text content
        text: String,
        /// Provider-specific options (e.g., cache control)
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ContentPartProviderOptions>,
    },
    /// Image content
    Image {
        /// Image URL or data URI
        url: String,
        /// Optional detail level for image processing
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
        /// Provider-specific options (e.g., cache control)
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ContentPartProviderOptions>,
    },
    /// Tool/function call (for assistant messages in conversation history)
    ToolCall {
        /// Unique ID for this tool call
        id: String,
        /// Name of the function to call
        name: String,
        /// Arguments as JSON
        arguments: serde_json::Value,
        /// Provider-specific options (e.g., cache control)
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ContentPartProviderOptions>,
        /// Opaque provider-specific metadata (e.g., Gemini thought_signature).
        /// Must be preserved and echoed back in subsequent requests.
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },
    /// Tool/function call result
    ToolResult {
        /// ID of the tool call this is responding to
        tool_call_id: String,
        /// Result content (can be text or JSON)
        content: serde_json::Value,
        /// Provider-specific options (e.g., cache control)
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ContentPartProviderOptions>,
    },
}

impl ContentPart {
    /// Create a text content part
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            provider_options: None,
        }
    }

    /// Create an image content part from URL
    pub fn image(url: impl Into<String>) -> Self {
        Self::Image {
            url: url.into(),
            detail: None,
            provider_options: None,
        }
    }

    /// Create an image content part with detail level
    pub fn image_with_detail(url: impl Into<String>, detail: ImageDetail) -> Self {
        Self::Image {
            url: url.into(),
            detail: Some(detail),
            provider_options: None,
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
            provider_options: None,
            metadata: None,
        }
    }

    /// Create a tool result content part
    pub fn tool_result(tool_call_id: impl Into<String>, content: serde_json::Value) -> Self {
        Self::ToolResult {
            tool_call_id: tool_call_id.into(),
            content,
            provider_options: None,
        }
    }

    /// Add Anthropic cache control to this content part
    ///
    /// # Example
    ///
    /// ```rust
    /// use stakai::{ContentPart, CacheControl};
    ///
    /// let part = ContentPart::text("Large context...")
    ///     .with_cache_control(CacheControl::ephemeral());
    /// ```
    pub fn with_cache_control(self, cache_control: CacheControl) -> Self {
        let provider_options = Some(ContentPartProviderOptions::anthropic_cache(cache_control));

        match self {
            Self::Text { text, .. } => Self::Text {
                text,
                provider_options,
            },
            Self::Image { url, detail, .. } => Self::Image {
                url,
                detail,
                provider_options,
            },
            Self::ToolCall {
                id,
                name,
                arguments,
                metadata,
                ..
            } => Self::ToolCall {
                id,
                name,
                arguments,
                provider_options,
                metadata,
            },
            Self::ToolResult {
                tool_call_id,
                content,
                ..
            } => Self::ToolResult {
                tool_call_id,
                content,
                provider_options,
            },
        }
    }

    /// Add provider-specific options to this content part
    pub fn with_provider_options(self, options: ContentPartProviderOptions) -> Self {
        let provider_options = Some(options);

        match self {
            Self::Text { text, .. } => Self::Text {
                text,
                provider_options,
            },
            Self::Image { url, detail, .. } => Self::Image {
                url,
                detail,
                provider_options,
            },
            Self::ToolCall {
                id,
                name,
                arguments,
                metadata,
                ..
            } => Self::ToolCall {
                id,
                name,
                arguments,
                provider_options,
                metadata,
            },
            Self::ToolResult {
                tool_call_id,
                content,
                ..
            } => Self::ToolResult {
                tool_call_id,
                content,
                provider_options,
            },
        }
    }

    /// Get the provider options from this content part
    pub fn provider_options(&self) -> Option<&ContentPartProviderOptions> {
        match self {
            Self::Text {
                provider_options, ..
            } => provider_options.as_ref(),
            Self::Image {
                provider_options, ..
            } => provider_options.as_ref(),
            Self::ToolCall {
                provider_options, ..
            } => provider_options.as_ref(),
            Self::ToolResult {
                provider_options, ..
            } => provider_options.as_ref(),
        }
    }

    /// Get the cache control from provider options (if set for Anthropic)
    pub fn cache_control(&self) -> Option<&CacheControl> {
        self.provider_options()
            .and_then(|opts| opts.anthropic.as_ref())
            .and_then(|anthropic| anthropic.cache_control.as_ref())
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
