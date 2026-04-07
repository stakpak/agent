# Apple Intelligence Optimization Guide

## Context Window Limitations
Apple Intelligence models have a significantly smaller context window compared to Claude or Ollama. This requires:

1. **Compact System Prompt**: Use `compactSystemPrompt` instead of the full `systemPrompt`.
2. **Tool Format**: Use plain-text tool calling (`tool_name {"param": value}`) instead of JSON tool_use blocks.
3. **History Truncation**: Limit task history context to the most recent 1-2 tasks.
4. **Output Truncation**: Limit tool output to essential information only.

---

## Tool Calling Format
Apple Intelligence uses **text-based tool calling** instead of structured tool_use blocks. Example:

```
read_file {"file_path": "/path/to/file.swift"}
```

The model will generate tool calls in this format, which must be parsed from the text response.

---

## FoundationModelService Adjustments
### 1. **Session Initialization**
- Skip history context to save tokens.
- Use `compactSystemPrompt`.
- Use `nativeTools()` for native FoundationModels.Tool integration.

### 2. **Response Parsing**
- Parse tool calls from **text output** using regex patterns.
- Example regex for `read_file`:
  ```swift
  guard let nameRange = text.range(of: "read_file") else { continue }
  let afterName = text[nameRange.upperBound...].trimmingCharacters(in: .whitespaces)
  guard afterName.hasPrefix("{") else { continue }
  ```

### 3. **Tool Execution Flow**
1. Model generates: `read_file {"file_path": "/path/to/file"}`
2. `FoundationModelService.parseResponse()` extracts tool calls from text.
3. `TaskExecution.executeToolCall()` processes the tool call.
4. Tool output is fed back to the model.

---

## TaskExecution Adjustments
### 1. **Context Management**
- Limit `buildLLMContext()` to:
  - Current task only (no history).
  - Last 3-5 tool outputs (truncate older outputs).

### 2. **Output Truncation**
- Truncate tool outputs to `maxOutputLines` (default: 1000).
- For file operations, use `readFilePreviewLines` (default: 3).

### 3. **Iteration Limits**
- Reduce `maxIterations` (default: 50) to prevent context overflow.

---

## AgentViewModel Adjustments
### 1. **Provider Selection**
- Apple Intelligence is selected via `.foundationModel` provider.
- No API key required (on-device).

### 2. **Settings UI**
- Show availability status:
  ```swift
  FoundationModelService.isAvailable ? "Available" : "Not available"
  FoundationModelService.unavailabilityReason
  ```
- No configuration needed (on-device).

---

## Error Handling
### Common Errors
1. **Context Window Exceeded**
   - **Symptoms**: Model stops mid-response or repeats itself.
   - **Solution**:
     - Reduce `maxOutputLines`.
     - Truncate history context.
     - Use `compactSystemPrompt`.

2. **Tool Call Parsing Failures**
   - **Symptoms**: Model generates malformed tool calls.
   - **Solution**:
     - Improve regex patterns in `parseResponse()`.
     - Add validation for JSON parsing.

3. **Permission Denied**
   - **Symptoms**: Tool calls fail with permission errors.
   - **Solution**:
     - Ensure TCC permissions are granted.
     - Use `run_agent_script` for automation (full TCC).

---

## Testing Recommendations
1. **Test with Small Files**: Verify `read_file` truncation works.
2. **Test Tool Chains**: Ensure multi-step workflows don't exceed context.
3. **Test Error Recovery**: Verify model recovers from truncated outputs.

---

## Example: Optimized Tool Call
### Before (Claude/Ollama)
```json
{
  "type": "tool_use",
  "id": "123",
  "name": "read_file",
  "input": {"file_path": "/path/to/file"}
}
```

### After (Apple Intelligence)
```
read_file {"file_path": "/path/to/file"}
```