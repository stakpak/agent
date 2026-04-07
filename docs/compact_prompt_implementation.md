# COMPACT SYSTEM PROMPT IMPLEMENTATION

## Problem
Full system prompt (~5300 chars) consumes significant token budget, especially with chat history. Need shorter prompts for all LLMs while preserving:
- Core agent identity
- Tool usage rules
- TCC/permission boundaries
- Previous message context

## Solution Design

### 1. New Prompt Structure
```
COMPACT_PROMPT = core identity + essential rules + tool reference + [history]
```

### 2. API Changes
- Add `promptStyle` enum: `.full`, `.compact`
- Update `SystemPromptService` to support both styles
- Add setting in UI to choose style per provider

### 3. Compact Prompt Content
```swift
static func compactPrompt(userName: String, userHome: String, projectFolder: String = "") -> String {
    """
    You are an autonomous macOS agent for \(userName).
    
    CORE RULES:
    - Act, don't explain. Never ask questions. Call task_complete when done.
    - Don't repeat script stdout — user sees it live.
    - Current folder: \(folder) (default for operations)
    
    TOOL PRIORITY:
    1. Native tools (read_file, write_file, edit_file, git_*, xcode_*)
    2. MCP tools (mcp_*)
    3. Shell (execute_agent_command, execute_daemon_command) ONLY if native/MCP unavailable
    
    TCC PERMISSIONS:
    - Full TCC in Agent: run_agent_script, apple_event_query, run_applescript, run_osascript, ax_*
    - User shell: execute_agent_command (as \(userName), ~=\(userHome)) — NO TCC
    - Root shell: execute_daemon_command — NO TCC
    
    CRITICAL DON'Ts:
    - Never use shell for file/coding when native tools exist
    - Never use xcodebuild/swift build via shell when xcode_build or MCP available
    - Never use execute_agent_command for AX/Automation (use run_agent_script)
    - Never build AgentScripts with xcode_build (use run_agent_script)
    
    ALWAYS: xcode_build → MCP → Shell (last resort)
    
    [Previous message context appended here]
    """
}
```

### 4. Implementation Steps

#### Step 1: Add PromptStyle enum
```swift
enum PromptStyle: String, CaseIterable {
    case full
    case compact
}
```

#### Step 2: Update SystemPromptService
- Add `promptStyle` parameter to `prompt(for:userName:userHome:projectFolder:style:)`
- Add `compactPrompt(for:)` method
- Store compact prompts in separate files: `claude_compact.txt`, etc.

#### Step 3: Update Service Constructors
- Pass promptStyle from view model to services
- Use compact prompt when style is `.compact`

#### Step 4: UI Control
- Add prompt style dropdown in settings
- Per-provider or global setting

#### Step 5: Migration
- Keep full prompt as default
- Add migration for existing users

## Benefits
- ~70% reduction in system prompt tokens (5300 → ~1500 chars)
- More room for chat history
- Faster context processing for LLMs
- Preserves all essential guidance

## Testing
- Verify tool calling still works
- Check Apple Intelligence compatibility
- Test with various LLM providers