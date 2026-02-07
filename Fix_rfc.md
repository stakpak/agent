# RFC: Anthropic Tool Result Message Merging

## Document Info

| Field | Value |
|-------|-------|
| Title | Fix Anthropic API 400: Orphaned tool_result blocks |
| Status | Implemented |
| Author | Abdalla Mohamed  |
| Created | 2026-02-07 |
| Affected Component | `libs/ai` (StakAI) — Anthropic provider conversion layer |

---

## 1. Issue Statement

### 1.1 Observed Error

```
Provider error: Anthropic API error 400 Bad Request: {
  "type": "error",
  "error": {
    "type": "invalid_request_error",
    "message": "messages.32.content.0: unexpected `tool_use_id` found in `tool_result` blocks: toolu_01WmiPNhpu1ESKtSdi6zbfdj. Each `tool_result` block must have a corresponding `tool_use` block in the previous message."
  }
}
```

### 1.2 Root Cause

Anthropic requires that **each `tool_result` block refer to a `tool_use` block in the immediately preceding message**. In practice, that means:

- Assistant message with one or more `tool_use` blocks
- **Exactly one** user message with all corresponding `tool_result` blocks

e were emitting **one Tool message per tool result**:

```
assistant (tool_use A, tool_use B)
user      (tool_result A)   ← Message N
user      (tool_result B)   ← Message N+1
```

For Message N+1, the “previous message” is the user message containing `tool_result A`, which has no `tool_use` blocks. Anthropic therefore rejects `tool_result B` as having no corresponding `tool_use` in the previous message.

### 1.3 When It Occurs

- ACP server chat with tool calling
- Multi-turn conversations with multiple tool calls in one assistant turn
- Any flow where tool results are sent as separate `Role::Tool` messages

---

## 2. Solution

### 2.1 Approach

Before converting to Anthropic format, merge consecutive `Role::Tool` messages into a single message that contains multiple `tool_result` content parts.

### 2.2 Implementation

**Location:** `libs/ai/src/providers/anthropic/convert.rs`

1. **`merge_consecutive_tool_messages(messages)`**  
   - Walks the message list  
   - Groups consecutive `Role::Tool` messages  
   - Replaces each group with one message whose content is all ToolResult parts from that group  

2. **`build_messages_with_caching()`**  
   - Calls `merge_consecutive_tool_messages()` before converting to Anthropic messages  

Resulting structure:

```
assistant (tool_use A, tool_use B)
user      (tool_result A, tool_result B)   ← single merged message
```

### 2.3 Design Choices

| Choice | Rationale |
|--------|-----------|
| Merge in conversion layer | Anthropic-specific; OpenAI/others keep separate tool messages. |
| Merge before caching logic | Caching operates on the merged message list for correct tail caching. |
| Only merge consecutive Tool messages | Preserves ordering and avoids touching non-tool messages. |

---

## 3. Problems & Considerations

### 3.1 Potential Edge Cases

| Case | Description | Handling |
|------|-------------|----------|
| Tool message with non-ToolResult parts | Tool message containing Text + ToolResult | Current impl only collects ToolResult parts; Text in Tool messages may be dropped. |
| Empty Tool messages | Tool message with no ToolResult parts | Merged group can be empty; we skip emitting if no parts remain. |
| Single Tool message | One Tool message in a row | Still passed through; merge is a no-op. |

### 3.2 Context Trimming / Orphan tool_results

If an assistant message (with `tool_use`) is removed by trimming and the corresponding tool results remain, we still send orphan `tool_result`s. This fix does **not** address that. A future enhancement could:

- Validate that each `tool_result`’s `tool_use_id` appears in the previous assistant message  
- Drop or rewrite orphan `tool_result` blocks before sending  

### 3.3 Provider Differences

- **Anthropic:** One user message with all tool results following the assistant.
- **OpenAI:** Can use separate role="tool" messages; no merge needed.
- **Gemini:** Different format again; separate handling.

The change is scoped to the Anthropic conversion path only.

---

## 4. Dependencies

### 4.1 Internal

- `crate::types::{ContentPart, Message, MessageContent, Role}`
- `build_messages_with_caching` (conversion pipeline)
- `to_anthropic_message_with_caching` (per-message conversion)

### 4.2 External

- None beyond existing StakAI/Anthropic dependencies.

### 4.3 Downstream

- ACP server chat
- CLI interactive mode (when using Anthropic)
- Any client using the StakAI SDK with Anthropic and tool calling

---

## 5. Testing & Validation

### 5.1 Unit Tests

- **`test_consecutive_tool_messages_merged_for_anthropic`**  
  - Builds request: assistant (2 tool_uses) + 2 separate Tool messages.  
  - Asserts: 2 Anthropic messages total; second is a single user message with 2 ToolResult blocks.

### 5.2 Existing Tests

- All Anthropic convert tests, including `test_tool_role_message_converted_to_user_with_tool_result`, should continue to pass (merge is transparent for single tool results).

### 5.3 Manual Validation

1. Run ACP server with Anthropic and trigger multiple tool calls in one turn.
2. Verify no 400 errors from Anthropic.
3. Confirm tool results are applied and the model receives correct feedback.

### 5.4 Comparison: Old vs New Logic

| Aspect | Old Logic | New Logic |
|--------|-----------|-----------|
| Tool result messages | N Tool messages → N Anthropic user messages | N Tool messages → 1 Anthropic user message |
| Message count to API | Higher | Lower (fewer user messages) |
| API compliance | Fails for 2+ tool results | Compliant with Anthropic spec |
| Other providers | Unchanged | Unchanged (Anthropic-only) |

---

## 6. Monitoring the Solution

### 6.1 Metrics to Watch

- Anthropic 400 error rate (should decrease)
- Successful tool-call rounds per session
- Latency (should be unchanged or slightly better due to fewer messages)

### 6.2 Logs

- Existing `Stream error:` / `Provider error:` logs should show fewer Anthropic 400s for tool_result issues.
- No new log lines added by this change.

### 6.3 Rollback

- Revert the merge logic in `convert.rs`.
- Revert the `build_messages_with_caching` integration.
- Restore previous behavior for Anthropic (and reintroduce the 400 risk).

---

## 7. Pros and Cons

### 7.1 Pros

| Pro | Detail |
|-----|--------|
| Fixes 400 errors | Resolves the “unexpected tool_use_id” failure for multi-tool turns. |
| Anthropic-compliant | Matches the intended use of tool_result blocks. |
| Localized change | Only affects Anthropic conversion; no shared types or flows altered. |
| Backward compatible | Single tool result behaves as before. |
| Fewer API messages | Slightly simpler payload and potentially better caching. |

### 7.2 Cons

| Con | Detail |
|-----|--------|
| Provider-specific behavior | Merge logic is Anthropic-specific; others may diverge. |
| Orphan handling | Does not handle trimming that drops assistant messages. |
| Potential data loss | Tool messages with mixed content (e.g., Text + ToolResult) may lose non-ToolResult parts; needs verification if that format is used. |

---

## 8. Impact Analysis

### 8.1 Affected Flows

- ACP server chat with Anthropic + tools
- CLI `stakpak` interactive mode with Anthropic
- Any StakAI client using Anthropic and tool calling

### 8.2 Not Affected

- OpenAI provider
- Gemini provider
- Non-tool conversations
- Tool calls that produce a single tool result per assistant turn (already worked; merge is no-op)

### 8.3 Risk Level

**Low.** The change:

- Is limited to Anthropic conversion.
- Preserves behavior for single-tool-result cases.
- Fixes a deterministic API failure for multi-tool turns.
- Is covered by unit tests.

---

## 9. References

- [Anthropic Tool Use Documentation](https://docs.anthropic.com/en/docs/tool-use)
- Implementation: `libs/ai/src/providers/anthropic/convert.rs`
- Related: `merge_consecutive_tool_messages`, `build_messages_with_caching`
