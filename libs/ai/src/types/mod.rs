//! Core types for the AI SDK

mod cache;
mod cache_strategy;
mod cache_validator;
mod headers;
mod message;
mod model;
mod options;
mod request;
mod response;
mod stream;

// Cache control types
pub use cache::{CacheControl, CacheWarning, CacheWarningType, PromptCacheRetention};
pub use cache_strategy::{AnthropicCacheConfig, CacheStrategy};
pub use cache_validator::{CacheContext, CacheControlValidator};

// Headers
pub use headers::Headers;

// Message types
pub use message::{
    AnthropicContentPartOptions, AnthropicMessageOptions, ContentPart, ContentPartProviderOptions,
    ImageDetail, Message, MessageContent, MessageProviderOptions, Role,
};

// Options types
pub use options::{
    AnthropicToolOptions, GenerateOptions, Tool, ToolChoice, ToolFunction, ToolProviderOptions,
};

// Request types
pub use request::{
    AnthropicOptions, CompletionsConfig, GenerateRequest, GoogleOptions, OpenAIApiConfig,
    OpenAIOptions, ProviderOptions, ReasoningEffort, ReasoningSummary, ResponsesConfig,
    SystemMessageMode, ThinkingOptions,
};

// Response types
pub use response::{
    FinishReason, FinishReasonKind, GenerateResponse, InputTokenDetails, OutputTokenDetails,
    ResponseContent, ResponseWarning, ToolCall, Usage,
};

// Stream types
pub use stream::{GenerateStream, StreamEvent};

// Model types
pub use model::{Model, ModelCost, ModelLimit};
