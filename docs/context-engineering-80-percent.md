# RFC: Context Engineering (80% Budget-Aware Trimming)

**Status:** Implemented  
**Design reference:** `AGENTS.md` — "Context Trimming with Cache Preservation"

---

## 1. Summary

The agent implements **budget-aware context trimming** so long sessions stay within the model’s context window without breaking API validity or prompt caching. Trimming is **lazy**: it only runs when estimated tokens exceed a configurable fraction of the context window (default **80%**). Only **assistant** and **tool** messages are trimmed; **user** and **system** messages are always kept. Trimmed content is replaced with a stable `[trimmed]` placeholder so the prefix stays identical across turns, preserving Anthropic prompt cache effectiveness. Trimming state is persisted in checkpoint metadata and reused on resume.

---

## 2. Motivation

- Long conversations (many turns, large tool results) can exceed the model’s context limit and cause 400 errors.
- Naively dropping old messages can break alternating user/assistant structure and tool_use/tool_result pairing (Anthropic requirements).
- Changing the prompt prefix every turn invalidates prompt caching and increases cost/latency.
- We need a single, predictable place to reduce context (before inference) that respects both API constraints and cache stability.

---

## 3. Goals and non-goals

**Goals:**

- Keep effective token count at or below `context_window × threshold` (default 80%).
- Only trim assistant and tool messages; never user or system.
- Preserve message structure (roles, tool_call_ids) so the API always sees valid sequences.
- Keep the trimmed prefix identical across turns so prompt caching remains valid.
- Persist trimming state in checkpoint metadata so resume continues with the same boundary.

**Non-goals:**

- Changing the threshold at runtime per request (it is fixed per hook configuration).
- Trimming based on semantic importance (trimming is positional and budget-driven).
- Handling provider-specific message formats beyond the shared LLM message model.

---

## 4. Design overview

1. **Before each inference**, a `TaskBoardContextHook` runs (`LifecycleEvent::BeforeInference`).
2. The hook computes an effective **context window** = model limit − system prompt tokens − reserved output tokens.
3. It calls **`reduce_context_with_budget`** with that window, current messages, checkpoint metadata, and optional tool definitions.
4. The context manager:
   - Normalizes messages (clean checkpoint tags, convert to LLM messages, merge consecutive same-role, dedup tool results).
   - Reads **`trimmed_up_to_message_index`** from metadata (previous trim boundary).
   - If there is no prior trimming and estimated tokens ≤ threshold → **fast path**: return messages and metadata unchanged.
   - Otherwise, re-applies trimming up to the previous boundary, then estimates tokens again. If still over threshold, advances the boundary (trimming more) until under threshold or no more assistant/tool messages to trim.
5. Updated metadata (new `trimmed_up_to_message_index`) is written back to `AgentState.metadata` and later persisted in the checkpoint.
6. The hook builds `LLMInput` (system prompt + reduced messages, max_tokens, tools) and stores it in state; the provider then performs the actual API call.

---

## 5. Detailed flow (logic)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  BeforeInference (TaskBoardContextHook)                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│  1. context_window = model.limit.context - system_prompt_tokens - 16000     │
│  2. (reduced_messages, updated_metadata) = reduce_context_with_budget(       │
│       messages, context_window, state.metadata, tools )                     │
│  3. state.metadata = updated_metadata                                       │
│  4. state.llm_input = system_prompt + reduced_messages + tools + max_tokens  │
└─────────────────────────────────────────────────────────────────────────────┘
                                        │
                                        ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  reduce_context_with_budget                                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│  • Clean checkpoint_id tags → convert to LLMMessage                        │
│  • merge_consecutive_same_role(llm_messages)                                │
│  • dedup_tool_results(llm_messages)                                         │
│  • tool_overhead = estimate_tool_overhead(tools)                            │
│  • threshold = context_window * context_budget_threshold  (e.g. 0.8)        │
│  • prev_trimmed_up_to = metadata["trimmed_up_to_message_index"] ?? 0         │
│                                                                             │
│  FAST PATH: if prev_trimmed_up_to == 0 && estimate_tokens(msgs)+overhead    │
│             <= threshold  → return (llm_messages, metadata)                │
│                                                                             │
│  • keep_n_trim_end = index of first message to KEEP (walk backwards,       │
│    count assistant messages until keep_last_n_assistant_messages)           │
│  • Re-apply trimming for indices [0..prev_trimmed_up_to)                    │
│  • effective_tokens = estimate_tokens(msgs) + tool_overhead                 │
│                                                                             │
│  if effective_tokens > threshold:                                            │
│    • Trim from prev_trimmed_up_to up to keep_n_trim_end (assistant/tool)     │
│    • If still over threshold, scan forward trimming one message at a time   │
│      until current_tokens <= threshold; candidate = last trimmed index + 1 │
│    • effective_trim_end = max(candidate, prev_trimmed_up_to)  // never go back│
│  else:                                                                      │
│    • effective_trim_end = prev_trimmed_up_to  // under budget, freeze        │
│                                                                             │
│  • Apply trim_message for indices [prev_clamped..effective_trim_end)       │
│  • updated_metadata["trimmed_up_to_message_index"] = effective_trim_end    │
│  • return (llm_messages, updated_metadata)                                  │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 6. Data structures

### 6.1 TaskBoardContextManagerOptions

| Field | Type | Default (hook) | Meaning |
|-------|------|----------------|---------|
| `keep_last_n_assistant_messages` | `usize` | 50 (hook), 10 (client) | Number of most recent **assistant** messages to leave untrimmed. Trim boundary is placed just before the Nth-from-last assistant. |
| `context_budget_threshold` | `f32` | 0.8 | Fraction of context window (0.0–1.0). Trimming triggers when estimated tokens exceed `context_window * threshold`. |

### 6.2 TaskBoardContextManager

| Field | Type | Meaning |
|-------|------|---------|
| `keep_last_n_assistant_messages` | `usize` | From options. |
| `context_budget_threshold` | `f32` | From options. |

### 6.3 Metadata (checkpoint / AgentState)

Stored in `AgentState.metadata` and `CheckpointState.metadata` as JSON:

| Key | Type | Meaning |
|-----|------|---------|
| `trimmed_up_to_message_index` | number | Exclusive end index of the trimmed prefix. Messages in `[0, trimmed_up_to_message_index)` have been replaced with `[trimmed]`. Only assistant and tool messages are actually changed; user/system are skipped. |

The index is **monotonic**: it never decreases. When under budget, the same index is kept so the prefix stays stable for caching.

---

## 7. Function specifications

### 7.1 TaskBoardContextManager

#### `new(options: TaskBoardContextManagerOptions) -> Self`

Creates a context manager with the given `keep_last_n_assistant_messages` and `context_budget_threshold`.

---

#### `reduce_context(&self, messages: Vec<ChatMessage>) -> Vec<LLMMessage>`

**Trait:** `ContextManager::reduce_context`.

- Removes `<checkpoint_id>...</checkpoint_id>` from each message.
- Converts to `LLMMessage`, then runs `merge_consecutive_same_role` and `dedup_tool_results`.
- No budget or trimming; used when budget-aware path is not needed.

---

#### `reduce_context_with_budget(&self, messages: Vec<ChatMessage>, context_window: u64, metadata: Option<serde_json::Value>, tools: Option<&[LLMTool]>) -> (Vec<LLMMessage>, Option<serde_json::Value>)`

**Main entry for 80% budget behaviour.**

1. **Normalize:** `clean_checkpoint_tags` → `LLMMessage::from` → `merge_consecutive_same_role` → `dedup_tool_results`.
2. **Budget:** `threshold = context_window * context_budget_threshold`, `tool_overhead = estimate_tool_overhead(tools)`.
3. **State:** `prev_trimmed_up_to = metadata["trimmed_up_to_message_index"]` (default 0).
4. **Fast path:** If `prev_trimmed_up_to == 0` and `estimate_tokens(messages) + tool_overhead <= threshold`, return `(llm_messages, metadata)` unchanged.
5. **Boundary (keep_last_n):** Walk messages backwards, count assistant messages; set `keep_n_trim_end` to the index of the first message to keep (so that the last `keep_last_n_assistant_messages` assistants remain untrimmed).
6. **Re-apply previous trim:** For indices `[0..min(prev_trimmed_up_to, len)]`, call `trim_message` on assistant/tool messages.
7. **Effective tokens:** `effective_estimated_tokens = estimate_tokens(llm_messages) + tool_overhead`.
8. **Advance or freeze:**
   - If `effective_estimated_tokens > threshold`: trim from `prev_trimmed_up_to` up to `keep_n_trim_end`, then if still over threshold scan forward trimming one message at a time until under threshold. Set `effective_trim_end = max(candidate, prev_trimmed_up_to)`.
   - Else: `effective_trim_end = prev_trimmed_up_to`.
9. **Apply new trim:** For `[prev_clamped..effective_trim_end)`, call `trim_message` on assistant/tool messages.
10. **Metadata:** Set `metadata["trimmed_up_to_message_index"] = effective_trim_end`, return `(llm_messages, Some(updated_metadata))`.

---

#### `estimate_tokens(messages: &[LLMMessage]) -> u64`

**Public.** Conservative token estimate for a list of LLM messages.

- **Bytes → tokens:** `ceil(bytes / 3.5)` (constant `BYTES_PER_TOKEN = 3.5`).
- **Per message:** content tokens + 8 (role, wrappers).
- **Content:** String → bytes_to_tokens(len). List: for each part, `estimate_content_part_tokens` + 3 tokens per part; parts: Text → bytes_to_tokens(len), ToolCall → name+args+30 bytes, ToolResult → content+30 bytes, Image → 2000 tokens.
- **Final:** sum of message estimates, then `ceil(raw_estimate * 1.05)` (5% safety buffer).

Intentionally overestimates so trimming triggers slightly early rather than exceeding the context window.

---

#### `trim_message(msg: &mut LLMMessage)` (private)

Replaces message content with a stable placeholder while keeping structure.

- **String content:** set to `"[trimmed]"`.
- **List content:** each `Text` and `ToolResult` content set to `"[trimmed]"`; `ToolCall` and `Image` parts are left as-is (structure preserved for API validity).

---

#### `estimate_tool_overhead(tools: Option<&[LLMTool]>) -> u64`

**Public.** Token estimate for tool definitions (not part of the message list but sent with the request).

- For each tool: `name.len() + description.len() + input_schema.to_string().len()`, multiplied by 1.2 for JSON overhead, then `bytes_to_tokens(adjusted_bytes) + 8`.
- Sum over all tools, or 0 if `tools` is `None`.

---

#### `bytes_to_tokens(bytes: usize) -> u64` (private)

`ceil(bytes as f64 / BYTES_PER_TOKEN) as u64` with `BYTES_PER_TOKEN = 3.5`.

---

#### `estimate_content_part_tokens(part: &LLMMessageTypedContent) -> u64` (private)

- Text: `bytes_to_tokens(text.len())`.
- ToolCall: `bytes_to_tokens(name.len() + args.len() + 30)`.
- ToolResult: `bytes_to_tokens(content.len() + 30)`.
- Image: 2000.

---

### 7.2 Helpers (task_board_context_manager)

#### `merge_consecutive_same_role(messages: Vec<LLMMessage>) -> Vec<LLMMessage>`

Merges consecutive messages with the same role into one message by concatenating content parts. Produces valid sequences for providers (e.g. Anthropic) that require alternating roles; merged tool results become one message with multiple tool_result parts.

---

#### `dedup_tool_results(messages: Vec<LLMMessage>) -> Vec<LLMMessage>`

Within each tool-role message, keeps only the **last** occurrence of each `tool_use_id` in the content parts. Removes duplicate tool results so each tool_use has at most one tool_result.

---

### 7.3 Common (context_managers/common)

#### `remove_xml_tag(tag_name: &str, content: &str) -> String`

Removes all occurrences of `<tag_name>...</tag_name>` (including newlines) via regex. Used to strip `<checkpoint_id>...</checkpoint_id>` before sending to the model.

---

### 7.4 Hook (task_board_context/mod.rs)

#### `TaskBoardContextHook::new(options: TaskBoardContextHookOptions) -> Self`

Builds `TaskBoardContextManager` with `keep_last_n_assistant_messages.unwrap_or(50)` and `context_budget_threshold.unwrap_or(0.8)`.

#### Hook handler (LifecycleEvent::BeforeInference)

1. If `event != BeforeInference`, return `Continue`.
2. `context_window = model.limit.context - system_prompt_tokens - 16000` (max_output_tokens).
3. `(reduced_messages, updated_metadata) = context_manager.reduce_context_with_budget(ctx.state.messages, context_window, ctx.state.metadata, llm_tools)`.
4. `ctx.state.metadata = updated_metadata`.
5. Build `llm_input`: system prompt (from `system_prompt.txt`) + reduced_messages, max_tokens=16000, tools.
6. `ctx.state.llm_input = Some(llm_input)`.
7. Return `Continue`.

---

## 8. Integration

- **Registration:** In `libs/api/src/client/mod.rs`, the default hook registry registers `TaskBoardContextHook` for `LifecycleEvent::BeforeInference` with `keep_last_n_assistant_messages: Some(10)` and `context_budget_threshold: Some(0.8)`.
- **Execution:** In `libs/api/src/client/provider.rs`, `run_agent_completion` calls `execute_hooks(ctx, &LifecycleEvent::BeforeInference)` before building the LLM request. The hook sets `ctx.state.llm_input` and `ctx.state.metadata`; the provider then uses `llm_input` for the API call.
- **Persistence:** When a checkpoint is created, `CheckpointState.metadata` (including `trimmed_up_to_message_index`) is stored. On resume, that metadata is loaded into `AgentState.metadata` and passed into the next `reduce_context_with_budget` call.

---

## 9. Invariants and edge cases

- **Trim index never decreases:** `effective_trim_end >= prev_trimmed_up_to`, so the trimmed prefix is stable across turns.
- **Under budget:** If after re-applying previous trim the effective tokens are ≤ threshold, `effective_trim_end` is left at `prev_trimmed_up_to` (no new trimming).
- **Budget over keep_last_n:** If the last N assistant messages alone exceed the threshold, the implementation keeps scanning forward and trimming until under threshold; budget is the hard constraint, keep_last_n is best-effort.
- **User/system never trimmed:** Only assistant and tool messages are passed to `trim_message`.
- **ToolCall structure preserved:** When trimming a message that contains tool_use blocks, only text and tool_result content are replaced; tool_use blocks stay so the API still sees valid tool_use/tool_result pairing.

---

## 10. Files reference

### Core (80% budget trimming)

| File | Role |
|------|------|
| `libs/api/src/local/context_managers/task_board_context_manager.rs` | `TaskBoardContextManager`, `reduce_context_with_budget`, `estimate_tokens`, `trim_message`, `merge_consecutive_same_role`, `dedup_tool_results` |
| `libs/api/src/local/hooks/task_board_context/mod.rs` | `TaskBoardContextHook`, hook handler (context window, call trimmer, set llm_input and metadata) |
| `libs/api/src/local/hooks/task_board_context/system_prompt.txt` | System prompt prepended after trimming |
| `libs/api/src/client/mod.rs` | Registers `TaskBoardContextHook` with 80% threshold and keep_last_n |
| `libs/api/src/client/provider.rs` | `run_agent_completion` runs `BeforeInference` hooks |

### Context managers and common

| File | Role |
|------|------|
| `libs/api/src/local/context_managers/mod.rs` | `ContextManager` trait, module exports |
| `libs/api/src/local/context_managers/common.rs` | `remove_xml_tag`, history helpers (used by other managers) |
| `libs/api/src/local/context_managers/simple_context_manager.rs` | Simple reducer (no budget) |
| `libs/api/src/local/context_managers/scratchpad_context_manager.rs` | Scratchpad-oriented reduction |
| `libs/api/src/local/context_managers/file_scratchpad_context_manager.rs` | File-scratchpad context handling |

### Other hooks and state

| File | Role |
|------|------|
| `libs/api/src/local/hooks/file_scratchpad_context/mod.rs` | File scratchpad BeforeInference hook |
| `libs/api/src/local/hooks/inline_scratchpad_context/mod.rs` | Inline scratchpad BeforeInference hook |
| `libs/api/src/local/hooks/mod.rs` | Hook module exports |
| `libs/api/src/models.rs` | `AgentState.metadata` |
| `libs/api/src/storage.rs` | `CheckpointState.metadata` |
| `libs/api/src/stakpak/models.rs` | Stakpak API model types with metadata |
| `libs/api/src/stakpak/storage.rs` | Stakpak checkpoint storage |

### Tests and docs

| File | Role |
|------|------|
| `libs/api/src/local/tests.rs` | Async/integration tests for threshold and metadata |
| `docs/context-engineering-80-percent.md` | This RFC |
| `AGENTS.md` | Project overview; "Context Trimming with Cache Preservation" |

---

## 11. Verdict

The 80% context-engineering behaviour is **fully implemented**: lazy trimming at 80% of the context window, stable prefix for prompt caching, metadata persistence, and integration in the client/hook pipeline. It is used for all chat completion flows (including interactive mode) via `AgentClient`.
