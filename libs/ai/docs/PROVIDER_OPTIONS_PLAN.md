# Provider Options Enhancement Plan

This document outlines the plan for adding missing provider options to match Vercel AI SDK functionality.

## Overview

The StakAI SDK needs to support commonly-used provider options that developers expect when working with OpenAI, Anthropic, and Google Gemini APIs. This plan focuses on the most frequently used features, not full feature parity.

## Current State

### OpenAIOptions (Existing)
```rust
pub struct OpenAIOptions {
    pub reasoning_effort: Option<ReasoningEffort>,
    pub reasoning_summary: Option<ReasoningSummary>,
    pub system_message_mode: Option<SystemMessageMode>,
    pub store: Option<bool>,
    pub user: Option<String>,
}
```

### AnthropicOptions (Existing)
```rust
pub struct AnthropicOptions {
    pub thinking: Option<ThinkingOptions>,
    pub effort: Option<ReasoningEffort>,
}
```

### GoogleOptions (Existing)
```rust
pub struct GoogleOptions {
    pub thinking_budget: Option<u32>,
}
```

---

## Phase 1: Add Missing Provider Options

### 1.1 OpenAI Options

Add to `OpenAIOptions`:

| Field | Type | Description |
|-------|------|-------------|
| `parallel_tool_calls` | `Option<bool>` | Enable/disable parallel tool calls |
| `service_tier` | `Option<ServiceTier>` | Service tier selection (auto, default, flex) |
| `max_completion_tokens` | `Option<u32>` | Max tokens for reasoning models |
| `metadata` | `Option<HashMap<String, String>>` | Request metadata |

New enum:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceTier {
    Auto,
    Default,
    Flex,
}
```

**Implementation Notes:**
- `parallel_tool_calls` defaults to `true` in OpenAI API
- `max_completion_tokens` is used instead of `max_tokens` for reasoning models (o1/o3/o4)
- `service_tier` affects request routing and pricing

### 1.2 Anthropic Options

Add to `AnthropicOptions`:

| Field | Type | Description |
|-------|------|-------------|
| `send_reasoning` | `Option<bool>` | Include reasoning in requests (default true) |

**Implementation Notes:**
- `send_reasoning` controls whether to include the thinking block in responses
- When `false`, thinking is performed but not returned to save tokens

### 1.3 Google Options

Add to `GoogleOptions`:

| Field | Type | Description |
|-------|------|-------------|
| `safety_settings` | `Option<Vec<SafetySetting>>` | Content safety filter configuration |
| `cached_content` | `Option<String>` | Cached content resource name |

New types:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetySetting {
    pub category: HarmCategory,
    pub threshold: HarmBlockThreshold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HarmCategory {
    #[serde(rename = "HARM_CATEGORY_HATE_SPEECH")]
    HateSpeech,
    #[serde(rename = "HARM_CATEGORY_DANGEROUS_CONTENT")]
    DangerousContent,
    #[serde(rename = "HARM_CATEGORY_SEXUALLY_EXPLICIT")]
    SexuallyExplicit,
    #[serde(rename = "HARM_CATEGORY_HARASSMENT")]
    Harassment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HarmBlockThreshold {
    #[serde(rename = "BLOCK_NONE")]
    BlockNone,
    #[serde(rename = "BLOCK_LOW_AND_ABOVE")]
    BlockLowAndAbove,
    #[serde(rename = "BLOCK_MEDIUM_AND_ABOVE")]
    BlockMediumAndAbove,
    #[serde(rename = "BLOCK_ONLY_HIGH")]
    BlockOnlyHigh,
}
```

**Implementation Notes:**
- Safety settings control content filtering per category
- `cached_content` requires pre-created cache via separate API call

---

## Files to Modify

### Types Layer (`libs/ai/src/types/`)

| File | Changes |
|------|---------|
| `request.rs` | Add new option fields and types (ServiceTier, SafetySetting, HarmCategory, HarmBlockThreshold) |
| `mod.rs` | Export new types |

### Provider Layer (`libs/ai/src/providers/`)

| File | Changes |
|------|---------|
| `openai/types.rs` | Add new fields to ChatCompletionRequest |
| `openai/convert.rs` | Use new options in request conversion |
| `gemini/types.rs` | Add safety_settings and cached_content to GeminiRequest |
| `gemini/convert.rs` | Use safety_settings in conversion |

### Adapter Layer (`libs/shared/src/models/`)

| File | Changes |
|------|---------|
| `stakai_adapter.rs` | Update option conversion (set new fields to None initially) |

---

## Implementation Order

1. **Add types to `request.rs`** - Define all new structs and enums
2. **Export from `mod.rs`** - Make types publicly available
3. **Update OpenAI types** - Add fields to ChatCompletionRequest
4. **Update OpenAI convert** - Use options in to_openai_request()
5. **Update Gemini types** - Add fields to GeminiRequest
6. **Update Gemini convert** - Use options in to_gemini_request()
7. **Update stakai_adapter** - Handle new options in conversion
8. **Run tests** - Ensure all existing tests pass
9. **Add new tests** - Test new option handling

---

## Usage Examples

### OpenAI with Service Tier
```rust
use stakai::{ProviderOptions, OpenAIOptions, ServiceTier};

let request = GenerateRequest::new("openai:gpt-4o", messages)
    .with_provider_options(ProviderOptions::OpenAI(OpenAIOptions {
        service_tier: Some(ServiceTier::Flex),
        parallel_tool_calls: Some(false),
        ..Default::default()
    }));
```

### OpenAI Reasoning Model with Max Completion Tokens
```rust
use stakai::{ProviderOptions, OpenAIOptions, ReasoningEffort};

let request = GenerateRequest::new("openai:o3", messages)
    .with_provider_options(ProviderOptions::OpenAI(OpenAIOptions {
        reasoning_effort: Some(ReasoningEffort::High),
        max_completion_tokens: Some(16000),
        ..Default::default()
    }));
```

### Google with Safety Settings
```rust
use stakai::{
    ProviderOptions, GoogleOptions, SafetySetting,
    HarmCategory, HarmBlockThreshold
};

let request = GenerateRequest::new("google:gemini-2.5-pro", messages)
    .with_provider_options(ProviderOptions::Google(GoogleOptions {
        safety_settings: Some(vec![
            SafetySetting {
                category: HarmCategory::DangerousContent,
                threshold: HarmBlockThreshold::BlockNone,
            },
        ]),
        ..Default::default()
    }));
```

---

## Features NOT Included (Out of Scope)

The following features are intentionally excluded from this phase:

- **Provider Tools**: Web search, code interpreter, computer use
- **Object Generation**: Structured output schemas
- **Embeddings API**: Vector embeddings
- **Transcription/Speech**: Audio processing
- **Batch API**: Bulk request processing
- **Image Generation**: DALL-E, Imagen, etc.

These may be added in future phases based on user demand.

---

## Breaking Changes

This plan allows breaking changes. The following may break existing code:

1. Adding required fields to existing structs (mitigated by using `Option<T>` and `Default`)
2. Changing enum variants (not planned)
3. Modifying trait signatures (not planned)

All new fields use `Option<T>` and structs derive `Default`, so existing code should continue to work.

---

## Testing Strategy

1. **Unit tests** - Test type serialization/deserialization
2. **Integration tests** - Test end-to-end with mock responses
3. **Manual tests** - Test against real APIs (optional, requires API keys)

---

## Timeline

- Phase 1 (this plan): ~2-3 hours implementation
- Testing and validation: ~1 hour
- Documentation updates: ~30 minutes

---

## References

- [Vercel AI SDK - OpenAI Provider](https://sdk.vercel.ai/providers/ai-sdk-providers/openai)
- [Vercel AI SDK - Anthropic Provider](https://sdk.vercel.ai/providers/ai-sdk-providers/anthropic)
- [Vercel AI SDK - Google Provider](https://sdk.vercel.ai/providers/ai-sdk-providers/google-generative-ai)
- [OpenAI API Reference](https://platform.openai.com/docs/api-reference)
- [Anthropic API Reference](https://docs.anthropic.com/en/api)
- [Google Gemini API Reference](https://ai.google.dev/api)
