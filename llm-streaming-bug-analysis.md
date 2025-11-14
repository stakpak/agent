# LLM Response Streaming Bug: Dropped Chunks Analysis

## Bug Overview

**Commit:** `1942d71` - "Fix SSE streaming dropped chunks"  
**Date:** May 30, 2025  
**Author:** George  
**Severity:** High - Directly impacted agent success rate

## The Problem

The Stakpak CLI experienced a critical bug in its LLM response streaming implementation that caused **Server-Sent Events (SSE) chunks to be dropped** during streaming. This resulted in incomplete tool calls being received from the LLM, which directly caused tool execution failures and a measurable drop in overall agent success rate.

### Root Cause

The streaming implementation was processing SSE responses incorrectly by:

1. **Manual chunk parsing**: The code was manually reading raw byte streams and attempting to parse SSE events by splitting on newlines and looking for "data: " prefixes
2. **Batching responses**: The stream was returning `Vec<ChatCompletionStreamResponse>` (multiple responses per chunk) instead of individual `ChatCompletionStreamResponse` items
3. **Fragile parsing logic**: The manual string splitting and filtering approach was prone to dropping chunks when SSE events were split across multiple byte chunks from the network

### Impact

**Direct Consequences:**
- Tool calls received incomplete JSON arguments
- Tool execution failed due to malformed input
- Agent workflows interrupted mid-execution
- Reduced agent success rate across all operations

**User Experience:**
- Unpredictable agent behavior
- Failed tasks that should have succeeded
- Need for manual retries
- Loss of confidence in agent reliability

## The Fix

### Technical Solution

The fix involved three key changes:

1. **Proper SSE parsing**: Replaced manual byte stream parsing with the `eventsource_stream` crate's `.eventsource()` method, which correctly handles SSE protocol including:
   - Event boundaries across network chunks
   - Proper handling of multi-line events
   - Automatic parsing of the SSE format

2. **Simplified stream type**: Changed from returning batched responses (`Vec<ChatCompletionStreamResponse>`) to individual responses (`ChatCompletionStreamResponse`), eliminating unnecessary complexity

3. **Removed fragile string manipulation**: Eliminated the manual UTF-8 conversion, newline splitting, and prefix filtering that was causing chunks to be dropped

### Why It Worked

The `eventsource_stream` crate is specifically designed to handle SSE protocol correctly:
- **Stateful parsing**: Maintains state across chunk boundaries
- **Buffer management**: Properly buffers incomplete events until complete
- **Protocol compliance**: Follows SSE specification exactly
- **Reliability**: Battle-tested library used across the Rust ecosystem

### Downstream Changes

The fix also required updating the consumer code in `cli/src/commands/agent/code.rs`:

- Updated `process_responses_stream` function signature to accept individual responses instead of vectors
- Removed the inner loop that was iterating over batched responses
- Simplified the delta processing logic since each stream item was now a single response

## Lessons Learned

### What Went Wrong

1. **Reinventing the wheel**: Manual SSE parsing when robust libraries exist
2. **Premature optimization**: Batching responses added complexity without clear benefit
3. **Insufficient testing**: The bug wasn't caught before impacting production usage
4. **Protocol complexity**: SSE is more complex than it appears - chunk boundaries matter

### Best Practices Applied

1. **Use specialized libraries**: For protocol handling, always prefer well-tested libraries over custom implementations
2. **Simplify data flow**: Single items are easier to reason about than batches
3. **Type safety**: The stream type change made the API clearer and harder to misuse
4. **Proper abstractions**: Let libraries handle low-level protocol details

## Related Issues

This fix was part of a broader effort to improve streaming reliability:

- **Commit `d2add9c`**: Fixed double streaming error tool results (prevented duplicate error messages in UI)
- **Commit `b884b52`**: General streaming issues fix (broader refactoring of streaming infrastructure)
- **Commit `a9fe901`**: Fixed shell mode output escaped text and cancel stream message

## Verification

After the fix:
- ✅ Tool calls received complete JSON arguments
- ✅ No more dropped chunks in SSE streams
- ✅ Agent success rate returned to expected levels
- ✅ Streaming remained responsive and real-time

## Conclusion

This bug demonstrates the importance of using appropriate libraries for protocol handling rather than implementing custom parsing logic. The SSE protocol, while seemingly simple, has subtle edge cases around chunk boundaries that are easy to get wrong. By switching to `eventsource_stream`, the implementation became both more reliable and simpler to maintain.

The impact on agent success rate highlights how low-level infrastructure bugs can have cascading effects on high-level functionality. Proper streaming is critical for LLM-based agents, as incomplete tool calls break the entire agent execution flow.
