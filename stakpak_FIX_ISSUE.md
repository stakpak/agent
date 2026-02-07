# Bug Report: Anthropic API 400 — Orphaned tool_result Blocks When Using Stakpak API

## Issue Title

**Stakpak API returns Anthropic 400 when multiple tool calls return results in one assistant turn — tool_result blocks must be merged into single user message**

---

## Summary

When using the Stakpak API (`STAKPAK_API_KEY`) with tool calling, the backend returns Anthropic API 400 errors when an assistant message contains multiple tool calls and each tool result is sent as a separate message. Anthropic requires all tool results that correspond to a single assistant message to be combined into **one user message** with multiple `tool_result` blocks.

---

## Environment

- **Stakpak API**: `https://apiv2.stakpak.dev` (or configured endpoint)
- **Auth**: `STAKPAK_API_KEY` set
- **Flow**: Client → Stakpak API (OpenAI-compatible) → Stakpak backend → Anthropic API
- **Trigger**: Multi-tool-call turns (e.g., agent calls 2+ tools in one response)

---

## Error Message

```
Provider error: Anthropic API error 400 Bad Request: {
  "type": "error",
  "error": {
    "type": "invalid_request_error",
    "message": "messages.N.content.0: unexpected `tool_use_id` found in `tool_result` blocks: toolu_XXXX. Each `tool_result` block must have a corresponding `tool_use` block in the previous message.",
    "request_id": "req_XXXX"
  }
}
```

(`N` is the index of the user message that contains the orphaned `tool_result`.)

---

## Root Cause

### Anthropic’s Message Format Rules

Per [Anthropic’s Tool Use docs](https://docs.anthropic.com/en/docs/tool-use):

1. Each `tool_result` block must reference a `tool_use` block in the **immediately preceding** message.
2. All tool results for a given assistant turn must be in **one user message** that directly follows the assistant message.

### Incorrect Structure (Current Behavior)

If tool results are sent as separate user messages:

```
Message N-1: assistant (tool_use A, tool_use B)
Message N:   user (tool_result A)     ← OK
Message N+1: user (tool_result B)     ← FAIL
```

For Message N+1, the “previous message” is the user message with `tool_result A`, which has no `tool_use` blocks. Anthropic therefore rejects `tool_result B` as having no corresponding `tool_use` in the previous message.

### Correct Structure (Expected)

```
Message N-1: assistant (tool_use A, tool_use B)
Message N:   user (tool_result A, tool_result B)   ← single merged message
```

---

## Request Format Flow

1. **Client → Stakpak API**  
   Client sends OpenAI-compatible messages (one `role: "tool"` message per tool result).

2. **Stakpak backend → Anthropic**  
   Backend converts to Anthropic format and calls Anthropic. This is where the structure breaks: multiple tool-result messages are converted 1:1 into multiple user messages instead of being merged into one.

---

## Required Fix (Backend)

When converting from the incoming (e.g. OpenAI-style) format to Anthropic format, the Stakpak backend must:

1. Detect consecutive tool-result messages that follow an assistant message with tool calls.
2. Merge them into a **single user message** whose `content` array contains multiple `tool_result` blocks.
3. Preserve the order of `tool_use_id`s to match the assistant’s `tool_use` blocks.

### Example Correct Anthropic Payload

```json
{
  "role": "user",
  "content": [
    {
      "type": "tool_result",
      "tool_use_id": "toolu_01ABC...",
      "content": "Result for first tool call"
    },
    {
      "type": "tool_result",
      "tool_use_id": "toolu_01XYZ...",
      "content": "Result for second tool call"
    }
  ]
}
```

---

## Reproduction Steps

1. Configure a Stakpak client with `STAKPAK_API_KEY`.
2. Start a chat session with tool calling enabled.
3. Send a prompt that leads the model to call multiple tools in one turn (e.g., read a file and run a command).
4. Execute both tool calls and send their results back to the Stakpak API.
5. Observe the 400 error from Anthropic on the next request.

---

## Impact

- **Severity**: High — breaks any multi-tool-call turn when using Stakpak API.
- **Workaround**: None on the client side; the fix must be in the Stakpak backend.
- **Single-tool turns**: Typically unaffected (one tool result per turn works).

---

## References

- [Anthropic Tool Use](https://docs.anthropic.com/en/docs/tool-use)
- [Anthropic: Multiple tool results in one message](https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/implement-tool-use)
- Client-side fix for **direct Anthropic** usage: `libs/ai/src/providers/anthropic/convert.rs` — `merge_consecutive_tool_messages()`

---

## Contact / Reporting

Please report this to:

- Stakpak Support (dashboard, help, or contact form)
- GitHub: https://github.com/stakpak/cli (or the Stakpak backend repo)
- Discord: https://discord.gg/c4HUkDD45d

---

*Generated: 2026-02-07*
