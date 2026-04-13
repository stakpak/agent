[< Back to README](../README.md)

# Security Architecture

This document details Agent!'s security model and entitlements.

## Entitlements

Agent! requires the following entitlements in `Agent.entitlements`:

| Entitlement | Purpose |
|-------------|---------|
| `automation.apple-events` | AppleScript and ScriptingBridge automation |
| `cs.allow-unsigned-executable-memory` | Required for dlopen'd AgentScript dylibs |
| `cs.disable-library-validation` | Load user-compiled script dylibs at runtime |
| `assets.music.read-write` | Music library access via MusicBridge |
| `device.audio-input` | Microphone access for audio scripts |
| `device.bluetooth` | Bluetooth device interaction |
| `device.camera` | Camera capture (CapturePhoto script) |
| `device.usb` | USB device access |
| `files.downloads.read-write` | Read/write Downloads folder |
| `files.user-selected.read-write` | Read/write user-selected files |
| `network.client` | Outbound connections (API calls, web search) |
| `network.server` | Inbound connections (MCP HTTP/SSE transport) |
| `personal-information.addressbook` | Contacts access via ContactsBridge |
| `personal-information.calendars` | Calendar access via CalendarBridge |
| `personal-information.location` | Location services |
| `personal-information.photos-library` | Photos access via PhotosBridge |
| `keychain-access-groups` | Secure API key storage |

## TCC Permissions (Accessibility, Screen Recording, Automation)

Protected macOS APIs require user approval. Agent handles TCC correctly:

| Component | TCC Grants |
|-----------|------------|
| `run_agent_script`, `applescript_tool`, TCC shell commands | **ALL** (Accessibility, Screen Recording, Automation) |
| `execute_user_command` (LaunchAgent) | **None** |
| `execute_command` (root) | **Separate context** |

**Rule:** Use `run_agent_script` or `applescript_tool` for Accessibility/Automation tasks, not shell commands.

## Write Protection

- `applescript_tool` blocks destructive operations (`delete`, `close`, `move`, `quit`) by default
- The AI must explicitly set `allow_writes: true` to permit them
- This prevents accidental data loss from misinterpreted commands

## XPC Sandboxing

All privileged operations go through XPC (Inter-Process Communication):

```
Agent.app (SwiftUI)
    |
    |-- UserService (XPC) → Agent.app.toddbruss.user    (LaunchAgent, runs as user)
    |-- HelperService (XPC) → Agent.app.toddbruss.helper  (LaunchDaemon, runs as root)
```

The XPC boundary ensures:
- The main app runs with minimal privileges
- Root operations are isolated to the daemon
- Each XPC call is a discrete, auditable transaction
- File permissions are restored to the user after root operations

## Action Verification (action_not_performed)

Agent! prevents false-action claims — where an AI reports performing an action it never executed — with three independent layers:

### Layer 1: Prompt Rule
The system prompt instructs the LLM to say "action not performed" if it did not call a tool. It may never claim to have searched, opened, clicked, ran, or found something without a matching `tool_result`.

### Layer 2: App-Layer Detection
If the LLM returns text claiming "I searched", "I opened", "I clicked", etc. but made zero tool calls in that turn, the app injects a correction `tool_result` telling the LLM to use the actual tool or admit it cannot perform the action. This is logged in the activity view as `⚠️ action not performed`.

### Layer 3: Apple AI Gating
Apple Intelligence tool calls (accessibility, applescript, shell) are logged to the activity view with the 🍎 prefix showing exactly what was called and what it returned. If Apple AI's tools produce no substantive output (empty, just an exit code, or error), the request is automatically forwarded to the cloud LLM. Apple AI can only claim task completion when its tools return real evidence of work.

### Architecture
All tool execution flows through the app's `dispatchTool()` layer — the LLM never self-reports tool results. The flow is:

1. LLM returns a `tool_use` JSON block
2. Agent!'s dispatch layer executes the tool via XPC, shell, or in-process
3. The real output goes back as `tool_result`
4. The LLM summarizes the real result

The LLM cannot fabricate tool outputs because it never controls what `tool_result` contains — the app does.