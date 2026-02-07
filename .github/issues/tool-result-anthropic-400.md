# Stakpak API: Anthropic 400 — Orphaned tool_result blocks on multi-tool turns

## Problem

When using the Stakpak API (`STAKPAK_API_KEY`) with tool calling, the backend returns Anthropic API 400 errors when an assistant message contains multiple tool calls and each tool result is sent as a separate message.

```
Anthropic API error 400: messages.N.content.0: unexpected `tool_use_id` found in `tool_result` blocks. 
Each `tool_result` block must have a corresponding `tool_use` block in the previous message.
```

## Root Cause

Anthropic requires all tool results for a given assistant turn to be in **one user message**. The Stakpak backend currently forwards separate tool-result messages, so the 2nd+ tool_result sees a user message (previous tool result) instead of the assistant message with `tool_use` blocks.

**Wrong:** assistant → user(tool_A) → user(tool_B)  
**Correct:** assistant → user(tool_A, tool_B)

## Reproduction

1. Use `STAKPAK_API_KEY` with tool calling enabled.
2. Send a prompt that triggers multiple tool calls in one turn.
3. Execute tools and return results; observe 400 on follow-up request.

## Fix (Stakpak backend)

When converting to Anthropic format, merge consecutive tool-result messages into a single user message with multiple `tool_result` blocks.

## Impact

- **Severity:** High — breaks multi-tool-call turns via Stakpak API.
- **Client workaround:** None; fix must be in Stakpak backend.
- **Direct Anthropic:** Fixed in `libs/ai` (merge in Anthropic convert).

**Labels:** `bug`, `stakpak-api`, `tool-calling`, `anthropic`
