# StakAI Integration Plan for CLI (BYOM)

## Overview

This document outlines the integration of the `stakai` SDK into the Stakpak CLI to replace the current manual provider implementations for BYOM (Bring Your Own Model) functionality.

## Current State

### CLI's Current LLM Implementation (`libs/shared/src/models/`)

```
libs/shared/src/models/
├── integrations/
│   ├── anthropic.rs    # ~800 lines - Anthropic API client
│   ├── gemini.rs       # Gemini API client  
│   ├── openai.rs       # ~1800 lines - OpenAI API client
│   └── mod.rs
├── llm.rs              # ~534 lines - LLMModel enum, chat/chat_stream functions
├── error.rs            # AgentError types
└── model_pricing.rs    # Context/pricing info
```

**Key Components:**
- `LLMModel` enum: `Anthropic(AnthropicModel) | Gemini(GeminiModel) | OpenAI(OpenAIModel) | Custom(String)`
- `LLMProviderConfig`: Holds configs for all providers
- `chat()` / `chat_stream()`: Dispatch functions based on model type
- `GenerationDelta`: Streaming events via mpsc channels
- `LLMMessage`, `LLMMessageContent`, `LLMTool`: Message/tool types

### StakAI SDK (`/Users/ahmedhesham/Desktop/Work/stakpak/ai`)

```
stakai/src/
├── client/           # Inference client with auto-registration
├── provider/         # Provider trait definition
├── providers/        # OpenAI, Anthropic, Gemini implementations
├── registry/         # Runtime provider management
└── types/            # Message, Request, Response, Stream types
```

**Key Components:**
- `Inference`: High-level client with `generate()` and `stream()` methods
- `Provider` trait: Unified interface for all providers
- `GenerateRequest/Response`: Request/response types
- `StreamEvent`: Streaming events via async Stream
- `Message`, `ContentPart`, `Tool`: Message/tool types

## Type Mappings

| CLI Type | StakAI Type | Notes |
|----------|-------------|-------|
| `LLMModel` | Model string (e.g., "openai:gpt-4") | StakAI uses string-based model IDs |
| `LLMMessage` | `Message` | Similar structure, different field names |
| `LLMMessageContent` | `MessageContent` | String or Parts enum |
| `LLMMessageTypedContent` | `ContentPart` | Text, Image, ToolCall, ToolResult |
| `LLMTool` | `Tool` | Same concept, different structure |
| `GenerationDelta` | `StreamEvent` | Different enum variants |
| `LLMCompletionResponse` | `GenerateResponse` | Similar structure |
| `LLMTokenUsage` | `Usage` | Same fields |
| `LLMProviderConfig` | `InferenceConfig` | Provider configuration |

## Integration Strategy: Option A - Adapter Layer

Create an adapter layer that converts between CLI types and StakAI types, allowing gradual migration.

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     CLI Application                          │
│  (libs/api, tui, cli commands)                              │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                   Adapter Layer (NEW)                        │
│  libs/shared/src/models/stakai_adapter.rs                   │
│  - Converts LLMMessage ↔ stakai::Message                    │
│  - Converts GenerationDelta ↔ stakai::StreamEvent           │
│  - Wraps stakai::Inference for CLI usage                    │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      StakAI SDK                              │
│  - Provider implementations (OpenAI, Anthropic, Gemini)     │
│  - Streaming support                                         │
│  - Auto-registration from env vars                          │
└─────────────────────────────────────────────────────────────┘
```

### Benefits
- ✅ Minimal changes to existing CLI code
- ✅ Gradual migration possible
- ✅ CLI types remain stable for other consumers
- ✅ Easy rollback if issues arise

### Drawbacks
- ⚠️ Some code duplication in adapter
- ⚠️ Extra conversion overhead (minimal)

## Implementation Plan

### Phase 1: Add StakAI Dependency

1. **Add stakai to workspace Cargo.toml**
   ```toml
   [workspace.dependencies]
   stakai = { path = "../ai" }  # or git/crates.io when published
   ```

2. **Add to libs/shared/Cargo.toml**
   ```toml
   [dependencies]
   stakai = { workspace = true }
   ```

### Phase 2: Create Adapter Module

Create `libs/shared/src/models/stakai_adapter.rs`:

```rust
//! Adapter layer between CLI LLM types and StakAI SDK

use crate::models::llm::*;
use stakai::{
    Inference, InferenceConfig, GenerateRequest, GenerateResponse,
    Message, Role, ContentPart, StreamEvent, Tool, ToolFunction,
};

/// Convert CLI LLMMessage to StakAI Message
pub fn to_stakai_message(msg: &LLMMessage) -> Message {
    // Implementation
}

/// Convert StakAI Message to CLI LLMMessage  
pub fn from_stakai_message(msg: &Message) -> LLMMessage {
    // Implementation
}

/// Convert CLI LLMTool to StakAI Tool
pub fn to_stakai_tool(tool: &LLMTool) -> Tool {
    // Implementation
}

/// Convert StakAI StreamEvent to CLI GenerationDelta
pub fn from_stakai_stream_event(event: StreamEvent) -> Option<GenerationDelta> {
    // Implementation
}

/// Wrapper around StakAI Inference for CLI usage
pub struct StakAIClient {
    inference: Inference,
}

impl StakAIClient {
    pub fn new(config: &LLMProviderConfig) -> Result<Self, String> {
        // Build InferenceConfig from LLMProviderConfig
    }
    
    pub async fn chat(&self, input: LLMInput) -> Result<LLMCompletionResponse, AgentError> {
        // Convert input, call stakai, convert response
    }
    
    pub async fn chat_stream(
        &self,
        input: LLMStreamInput,
    ) -> Result<LLMCompletionResponse, AgentError> {
        // Convert input, call stakai stream, forward events via channel
    }
}
```

### Phase 3: Update LocalClient

Modify `libs/api/src/local/mod.rs` to use StakAI adapter:

```rust
use stakpak_shared::models::stakai_adapter::StakAIClient;

impl LocalClient {
    async fn run_agent_completion(&self, ...) -> Result<...> {
        // Use StakAIClient instead of direct provider calls
        let stakai_client = StakAIClient::new(&self.provider_config())?;
        stakai_client.chat_stream(input).await
    }
}
```

### Phase 4: Deprecate Old Provider Code

Once StakAI adapter is working:

1. Mark old provider implementations as deprecated
2. Remove unused code from `integrations/openai.rs`, `anthropic.rs`, `gemini.rs`
3. Keep only model definitions and pricing info

### Phase 5: Testing & Validation

1. Unit tests for type conversions
2. Integration tests with each provider
3. Streaming tests
4. Error handling tests

## Files to Modify

| File | Changes |
|------|---------|
| `Cargo.toml` (workspace) | Add stakai dependency |
| `libs/shared/Cargo.toml` | Add stakai dependency |
| `libs/shared/src/models/mod.rs` | Add stakai_adapter module |
| `libs/shared/src/models/stakai_adapter.rs` | NEW - Adapter layer |
| `libs/api/src/local/mod.rs` | Use StakAI adapter |
| `libs/shared/src/models/llm.rs` | Keep types, remove chat/chat_stream |
| `libs/shared/src/models/integrations/*.rs` | Keep models/pricing, remove API calls |

## Detailed Type Conversions

### Message Conversion

```rust
// CLI -> StakAI
fn to_stakai_message(msg: &LLMMessage) -> Message {
    let role = match msg.role.as_str() {
        "system" => Role::System,
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    };
    
    let content = match &msg.content {
        LLMMessageContent::String(s) => MessageContent::Text(s.clone()),
        LLMMessageContent::List(parts) => {
            MessageContent::Parts(parts.iter().map(to_stakai_content_part).collect())
        }
    };
    
    Message::new(role, content)
}

fn to_stakai_content_part(part: &LLMMessageTypedContent) -> ContentPart {
    match part {
        LLMMessageTypedContent::Text { text } => ContentPart::text(text),
        LLMMessageTypedContent::Image { source } => {
            ContentPart::image(format!("data:{};base64,{}", source.media_type, source.data))
        }
        LLMMessageTypedContent::ToolCall { id, name, args } => {
            ContentPart::tool_call(id, name, args.clone())
        }
        LLMMessageTypedContent::ToolResult { tool_use_id, content } => {
            ContentPart::tool_result(tool_use_id, serde_json::Value::String(content.clone()))
        }
    }
}
```

### Stream Event Conversion

```rust
fn from_stakai_stream_event(event: StreamEvent) -> Option<GenerationDelta> {
    match event {
        StreamEvent::TextDelta { delta, .. } => {
            Some(GenerationDelta::Content { content: delta })
        }
        StreamEvent::ToolCallStart { id, name } => {
            Some(GenerationDelta::ToolUse {
                tool_use: GenerationDeltaToolUse {
                    id: Some(id),
                    name: Some(name),
                    input: None,
                    index: 0,
                }
            })
        }
        StreamEvent::ToolCallDelta { id, delta } => {
            Some(GenerationDelta::ToolUse {
                tool_use: GenerationDeltaToolUse {
                    id: Some(id),
                    name: None,
                    input: Some(delta),
                    index: 0,
                }
            })
        }
        StreamEvent::Finish { usage, .. } => {
            Some(GenerationDelta::Usage {
                usage: LLMTokenUsage {
                    prompt_tokens: usage.prompt_tokens,
                    completion_tokens: usage.completion_tokens,
                    total_tokens: usage.total_tokens,
                    prompt_tokens_details: None,
                }
            })
        }
        _ => None,
    }
}
```

## Configuration Mapping

```rust
fn build_inference_config(config: &LLMProviderConfig) -> InferenceConfig {
    let mut inference_config = InferenceConfig::new();
    
    if let Some(openai) = &config.openai_config {
        inference_config = inference_config.openai(
            openai.api_key.clone().unwrap_or_default(),
            openai.api_endpoint.clone(),
        );
    }
    
    if let Some(anthropic) = &config.anthropic_config {
        inference_config = inference_config.anthropic(
            anthropic.api_key.clone(),
            None, // Use default base URL
        );
    }
    
    if let Some(gemini) = &config.gemini_config {
        inference_config = inference_config.gemini(
            gemini.api_key.clone(),
            None,
        );
    }
    
    inference_config
}
```

## Timeline Estimate

| Phase | Duration | Dependencies |
|-------|----------|--------------|
| Phase 1: Add Dependency | 1 hour | None |
| Phase 2: Create Adapter | 4-6 hours | Phase 1 |
| Phase 3: Update LocalClient | 2-3 hours | Phase 2 |
| Phase 4: Deprecate Old Code | 2-3 hours | Phase 3 tested |
| Phase 5: Testing | 2-4 hours | All phases |

**Total: ~12-17 hours**

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Type conversion bugs | Comprehensive unit tests |
| Streaming behavior differences | Integration tests with real providers |
| Performance regression | Benchmark before/after |
| Breaking changes in stakai | Pin version, use workspace dependency |

## Success Criteria

1. ✅ All existing BYOM functionality works with StakAI
2. ✅ Streaming works correctly for all providers
3. ✅ Tool calling works correctly
4. ✅ Error handling is consistent
5. ✅ No performance regression
6. ✅ Code is cleaner and more maintainable
