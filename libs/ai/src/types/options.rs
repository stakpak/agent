//! Generation options and tool definitions

use super::cache::CacheControl;
use super::Headers;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Options for generation requests
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerateOptions {
    /// Sampling temperature (0.0 to 2.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Maximum tokens to generate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Nucleus sampling parameter (0.0 to 1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// Sequences where generation should stop
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Available tools/functions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,

    /// How the model should choose tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,

    /// Frequency penalty (-2.0 to 2.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,

    /// Presence penalty (-2.0 to 2.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,

    /// Custom HTTP headers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,
}

impl GenerateOptions {
    /// Create new default options
    pub fn new() -> Self {
        Self::default()
    }

    /// Set temperature
    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Set max tokens
    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Set top_p
    pub fn top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Add stop sequence
    pub fn add_stop_sequence(mut self, sequence: impl Into<String>) -> Self {
        self.stop_sequences
            .get_or_insert_with(Vec::new)
            .push(sequence.into());
        self
    }

    /// Add tool
    pub fn add_tool(mut self, tool: Tool) -> Self {
        self.tools.get_or_insert_with(Vec::new).push(tool);
        self
    }

    /// Set tool choice
    pub fn tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Set custom headers
    pub fn headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Add a single header
    pub fn add_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(key, value);
        self
    }
}

/// Provider-specific options for tool definitions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolProviderOptions {
    /// Anthropic-specific tool options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<AnthropicToolOptions>,
}

impl ToolProviderOptions {
    /// Create Anthropic-specific options with cache control
    pub fn anthropic_cache(cache_control: CacheControl) -> Self {
        Self {
            anthropic: Some(AnthropicToolOptions {
                cache_control: Some(cache_control),
            }),
        }
    }
}

/// Anthropic-specific tool options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicToolOptions {
    /// Cache control for this tool (Anthropic prompt caching)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

/// A tool/function definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Tool type (currently only "function")
    #[serde(rename = "type")]
    pub tool_type: String,
    /// Function definition
    pub function: ToolFunction,
    /// Provider-specific options (e.g., cache control for Anthropic)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ToolProviderOptions>,
}

impl Tool {
    /// Create a new function tool
    pub fn function(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: name.into(),
                description: description.into(),
                parameters: Value::Object(Default::default()),
            },
            provider_options: None,
        }
    }

    /// Set function parameters (JSON Schema)
    pub fn parameters(mut self, parameters: Value) -> Self {
        self.function.parameters = parameters;
        self
    }

    /// Add Anthropic cache control to this tool
    ///
    /// # Example
    ///
    /// ```rust
    /// use stakai::{Tool, CacheControl};
    /// use serde_json::json;
    ///
    /// let tool = Tool::function("search", "Search documents")
    ///     .parameters(json!({"type": "object", "properties": {}}))
    ///     .with_cache_control(CacheControl::ephemeral());
    /// ```
    pub fn with_cache_control(mut self, cache_control: CacheControl) -> Self {
        self.provider_options = Some(ToolProviderOptions::anthropic_cache(cache_control));
        self
    }

    /// Add provider-specific options to this tool
    pub fn with_provider_options(mut self, options: ToolProviderOptions) -> Self {
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

/// Function definition for a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    /// Function name
    pub name: String,
    /// Function description
    pub description: String,
    /// Function parameters (JSON Schema)
    pub parameters: Value,
}

/// How the model should choose tools
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    /// Auto-select tools
    Auto,
    /// Never use tools
    None,
    /// Always use a specific tool
    Required { name: String },
}
