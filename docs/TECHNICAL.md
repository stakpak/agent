[< Back to README](../README.md)

<img width="107" height="107" alt="Agent! Agentic AI for the rest of us only on  macOS Desktop" src="https://github.com/user-attachments/assets/245b3612-c354-4177-a500-3ee4f22a5111" />

# Agent! for macOS26

[![Swift 6.2](https://img.shields.io/badge/Swift-6.2-blue.svg)](https://swift.org)
[![Website](https://img.shields.io/badge/website-macos26.app-blue.svg)](https://macos26.app)
[![Version](https://img.shields.io/badge/version-latest-blue.svg)](https://github.com/macOS26/Agent)
[![GitHub downloads](https://img.shields.io/github/downloads/macOS26/Agent/total.svg)](https://github.com/macOS26/Agent/releases)
[![GitHub stars](https://img.shields.io/github/stars/macOS26/Agent.svg?style=social)](https://github.com/macOS26/Agent/stargazers)

## Latest source code of Agent! running
<video src="AgentScriptDarkLight.mp4" width="100%" controls autoplay loop muted playsinline></video> 

## Agent! running inside a virtual machine
<img width="1460" height="1031" alt="Screenshot 2026-03-26 at 5 22 43 PM" src="https://github.com/user-attachments/assets/05e25a70-f101-4118-bb07-dbce897ea007" />

## 🧠 Agentic AI for the  Mac Desktop 
Agent! is the result of 27 years of Mac automation experience — from FaceSpan and AppleScript on macOS 9 through AppleScript Studio, AppleScript-ObjC, and now Swift. It connects LLMs to Apple Events, ScriptingBridge, Accessibility APIs, and XPC services for native macOS control.

Now with Apple Intelligence supporting 10 LLM providers.

### Apple Intelligence Mediator

Apple Intelligence serves as a **communication mediator** between the LLM and the user, not as an LLM provider. It observes conversations and adds helpful context using on-device intelligence:

Annotations are tagged with `[AI]` prefixes to distinguish them from LLM responses.

Enable Apple Intelligence Mediator in Settings to enhance communication clarity. Requires Apple Intelligence-capable Mac running macOS 26+.

### Key Capabilities

- **50+ App Automation** via ScriptingBridge (Mail, Messages, Music, Safari, Xcode, etc.)
	- **86 Tools** for file ops, git, web, accessibility, Xcode, MCP integration
	- **AgentScripts** — Swift Package-based automation scripts
	- **Apple Messages Monitor** — Remote control via iMessage
	- **MCP Support** — Model Context Protocol for extended functionality

---

## Table of Contents

- [Getting Started](#getting-started)
- [Security Hardening](#security-hardening)
- [Messages Monitor](#messages-monitor)
- [MCP Servers](#mcp-servers)
- [Architecture](#architecture)
- [Available Tools](#available-tools)
- [AgentScripts](#agentscripts)
- [Agent! vs. OpenClaw on Mac](#agent-vs-openclaw-on-mac)
- [License](#license)

---

## Getting Started

### 1. Prerequisites

- **macOS 26+** (Tahoe)
- **Xcode Command Line Tools** (Agent will prompt to install if missing)
- **Apple Silicon recommended** for local LLMs (minimum 32GB RAM, recommended 64-128GB)
- An API key for your preferred provider

### 2. Build and Run

1. Open `Agent.xcodeproj` in Xcode
2. Build and run the **Agent!** target (⌘R)
3. If prompted, install Xcode Command Line Tools via the system check overlay

### 3. Register Background Services

Click the **Register** button in the toolbar to install the background services:

This registers two background services using Apple's SMAppService framework:

1. **User Agent** (`Agent.app.toddbruss.user`) — Runs commands as your user account
2. **Privileged Daemon** (`Agent.app.toddbruss.helper`) — Runs commands as root when needed

### 4. Approve in System Settings

After clicking Register, macOS will prompt you to approve the background services:

1. **System Settings** → **General** → **Login Items**
2. Allow both **Agent** and **AgentHelper** (you may see two prompts)

The privileged daemon requires explicit approval because it runs with root privileges. Agent follows Apple's recommended XPC + SMAppService pattern for secure privilege separation.

### 5. Configure Your Provider

Click the **gear icon** (⚙️) to open Settings and configure one of the 10 supported providers:

<details>
<summary>Supported Providers (Click to expand)</summary>

| Provider | Models | Notes |
|----------|--------|-------|
| **Claude** | Sonnet 4, Opus 4, Haiku 3.5 | Recommended for complex tasks |
| **OpenAI** | GPT-4o, GPT-4 Turbo, GPT-3.5 | General-purpose option |
| **DeepSeek** | DeepSeek V2, DeepSeek Coder | Cost-effective |
| **Hugging Face** | Any model ID | Direct model access |
| **Ollama Cloud** | Various | Ollama Pro API |
| **Local Ollama** | Local models | Requires 32GB+ RAM |
| **vLLM** | OpenAI-compatible | Self-hosted |
| **LM Studio** | OpenAI/Anthropic | Local hosting |

</details>

> **Note:** Local models require significant RAM (minimum 32GB, recommended 64-128GB). For Mac minis or devices with limited RAM, cloud-based LLMs are strongly recommended.

### 6. Set a Project Folder (optional)

Click the **folder icon** in the toolbar to select a project folder or file. This sets a default working directory for file operations.

### 7. Connect and Run

1. Click **Connect** to test the XPC services
2. Type a task in natural language
3. Press **Run** (or ⌘Enter)

Agent will autonomously execute your task using the appropriate tools.

---

## Security Hardening

Agent! implements a comprehensive security model based on Apple's recommended patterns:

### Dual Privilege Model

<details>
<summary>Privilege Model Details (Click to expand)</summary>

| Service | Identifier | Runs As | Purpose |
|---------|------------|---------|---------|
| **User Agent** | `Agent.app.toddbruss.user` | User account | File editing, git, builds, scripts |
| **Privileged Daemon** | `Agent.app.toddbruss.helper` | Root (via LaunchDaemon) | System packages, /Library, launchd, disk operations |

</details>
The AI defaults to **user-level execution** and only uses the privileged daemon when explicitly required for system-level operations.



<details>
<summary>Full Entitlements & Security Details (Click to expand)</summary>

#### TCC Permissions

| Component | TCC Grants |
|-----------|------------|
| `run_agent_script`, `applescript_tool`, TCC shell commands | **ALL** (Accessibility, Screen Recording, Automation) |
| `execute_user_command` (LaunchAgent) | **None** |
| `execute_command` (root) | **Separate context** |

**Rule:** Use `run_agent_script` or `applescript_tool` for Accessibility/Automation tasks, not shell commands.

#### Write Protection

- `applescript_tool` blocks destructive operations (`delete`, `close`, `move`, `quit`) by default
- The AI must explicitly set `allow_writes: true` to permit them
- This prevents accidental data loss from misinterpreted commands

#### Full Entitlements List

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

See [SECURITY.md](SECURITY.md) for complete XPC architecture details.

</details>



---

## Messages Monitor

Agent! includes a built-in **Apple Messages monitor** that lets you control your Mac remotely via iMessage. Send a text message starting with `Agent!` from any approved contact and Agent will execute it as a task — then reply with the result.

### Dedicated Messages Tab

Agent! now features a dedicated **Messages tab** (green) specifically for iMessage Agent! commands. This tab:
- Uses the main LLM for processing (not a separate model)
- Provides a focused interface for remote command execution
- Shows real-time message monitoring status
- Integrates seamlessly with the Messages Monitor popover

### How It Works

1. Toggle **Messages** ON in the toolbar (green switch next to "Messages")
2. Click the **speech bubble icon** to open the Messages Monitor popover
3. Send a message starting with `Agent!` from another device or contact (e.g., `Agent! Next Song`)
4. The sender's handle (phone number or email) appears in the recipients list
5. Toggle the recipient ON to approve them
6. Future `Agent!` messages from approved recipients will automatically run as tasks
7. When the task completes, Agent sends the result (up to 256 characters) back via iMessage

### Message Format

```
Agent! <your prompt here>
```

Examples:
- `Agent! What song is playing?`
- `Agent! Next Song`
- `Agent! Check my email`
- `Agent! Build and run my Xcode project`

### Message Filter

The filter picker controls which messages are monitored:

<details>
<summary>Filter Options (Click to expand)</summary>

| Filter | Description |
|--------|-------------|
| **From Others** | Only incoming messages from other people (default) |
| **From Me** | Only your own sent messages (useful for self-testing between your devices) |
| **Both** | All messages regardless of sender |

</details>

### Recipient Approval

Every recipient must be explicitly approved before their `Agent!` commands trigger tasks:

- Recipients are auto-discovered when they send an `Agent!` message
- Unapproved messages are logged with a "not approved" note but not acted on
- Use **All** / **None** buttons to bulk-toggle recipients within the current filter
- Use **Clear** to remove all discovered recipients and start fresh

### How It Reads Messages

Agent reads the macOS Messages database (`~/Library/Messages/chat.db`) directly using the SQLite3 C API. It polls every 5 seconds for new messages. The `attributedBody` blob is decoded using the Objective-C runtime for messages where the `text` column is NULL (common with iMessage).

No external dependencies. No network requests. Everything runs locally on your Mac.

---

## MCP Servers

Agent! supports **MCP (Model Context Protocol)** servers for extended functionality.

### Available MCP Servers

<details>
<summary>MCP Server List (Click to expand)</summary>

| Server | Description | Link |
|--------|-------------|------|
| **internet-names-mcp** | Domain and social handle availability | https://github.com/drewster99/InternetNamesMCP |
| **xcode-mcp-server** | Xcode project building, running, screenshots | https://github.com/drewster99/xcode-mcp-server |
| **appstore-mcp-server** | App Store search, rankings, keywords | https://github.com/drewster99/appstore-mcp-server |
| **XCF** | External MCP server | https://xcf.ai |

</details>

### Configuration

1. Click the **server icon** in toolbar → **+** to add
2. Configure: Name, Command, Arguments, Environment, Transport (stdio/HTTP/SSE)
3. Enable **Auto-start** to connect on launch
4. Enable/disable individual tools per server as needed

---

## Architecture

```
Agent.app (SwiftUI)
  |-- AgentViewModel         Task loop, screenshots, clipboard, project folder
  |-- ClaudeService          Anthropic Messages API (streaming)
  |-- OllamaService          Ollama native API
  |-- ChatHistoryStore       SwiftData task memory
  |-- CodingService          File operations
  |-- MCPService             MCP client for external tools
  |-- ScriptService          Swift Package for agent scripts
  |-- XcodeService           Xcode ScriptingBridge
  |-- AppleEventService      Dynamic Apple Events
  |-- AccessibilityService   AXUIElement API
  |-- Messages Monitor       iMessage remote control

Execution contexts:
  |-- [In-Process]           TCC commands (ALL grants)
  |-- UserService XPC        LaunchAgent (user)
  |-- HelperService XPC      LaunchDaemon (root)
```

### App Automation Priority

<details>
<summary>Tool Priority Table (Click to expand)</summary>

| Priority | Tool | Best For |
|----------|------|----------|
| 1 | `run_agent_script` | Complex automation (full TCC) |
| 2 | `applescript_tool` | Reading app data and AppleScript automation |
| 3 | `execute_shell_command` (TCC) | Quick one-off AppleScript commands |
| 4 | Accessibility tools | UI inspection and interaction |

</details>


### Coding Tools Priority

<details>
<summary>Coding Tools Priority (Click to expand)</summary>

| Priority | Tool Type | Examples |
|-----------|-----------|----------|
| 1 | Native tools | `read_file`, `write_file`, `edit_file`, `git`, `xcode` |
| 2 | MCP server tools | `mcp_xcf_*` |
| 3 | Shell commands | Last resort only |

</details>

### Xcode Build Priority

For Xcode project builds, the AI follows this priority:

<details>
<summary>Xcode Build Priority (Click to expand)</summary>

| Priority | Tool | When to Use |
|-----------|------|-------------|
| 1 | `xcode_build` | Native ScriptingBridge tool — ALWAYS PREFERRED |
| 2 | XCF MCP server (`mcp_xcf_*`) | Backup if native tools unavailable |
| 3 | xcode-mcp-server (`mcp_xcode-mcp-server_*`) | Third choice if XCF unavailable |
| 4 | `xcodebuild` via shell | LAST RESORT only if no other options |

</details>

### System Prompt Version Management

Agent! manages system prompts with automatic version tracking:

<details>
<summary>Version Headers Table (Click to expand)</summary>

| Header | Behavior |
|--------|----------|
| `// Agent! v{version}` | Default prompt — auto-updates when app version changes |
| `// Agent! custom v{version}` | User-edited prompt — auto-updates on version change (preserves custom edits) |
| `// Agent! READ ONLY v{version}` | Locked prompt — never auto-overwritten, even on version changes |

</details>

**To lock a prompt:** Add `READ ONLY` or `// READ ONLY` at the top of your custom prompt. This prevents automatic updates even when a new Agent! version is released.

Prompts are stored in `~/Documents/AgentScript/system/` as `{provider}.txt` files.

---

## Available Tools

Agent! provides **86 tools** across multiple categories for autonomous task execution.

### Quick Reference

| Category | Tools | Description |
|----------|-------|-------------|
| Core | 28 | File ops, git, web, text generation |
| Agent Scripts | 7 | Create, run, manage Swift scripts |
| AppleScript/JS | 11 | AppleScript and JXA automation |
| Accessibility | 12 | UI automation via AXUIElement API |
| Xcode | 7 | Build, run, manage Xcode projects |
| MCP — XCF | 21 | Xcode project automation via MCP |

### Core Tools (28 tools)

| Tool | Description |
|------|-------------|
| `about_self` | Describe Agent's capabilities, features, and usage |
| `read_file` | Read file contents with line numbers |
| `write_file` | Create or overwrite a file |
| `edit_file` | Replace exact text in a file |
| `list_files` | Find files matching a glob pattern |
| `search_files` | Search file contents by regex pattern |
| `read_dir` | List directory contents |
| `create_diff` | Compare two text strings and return a pretty D1F diff with emoji markers |
| `apply_diff` | Apply a D1F ASCII diff to a file for precise multi-line edits |
| `diff_and_apply` | Edit a file by line range in one step |
| `undo_edit` | Undo a previous diff edit |
| `file_manager` | Unified file operations (read, write, edit, list, search, read_dir, if_to_switch, extract_function) |
| `git` | Git operations (status, diff, log, commit, diff_patch, branch) |
| `execute_agent_command` | Execute shell command as current user (no TCC) |
| `execute_daemon_command` | Execute shell command as ROOT via LaunchDaemon (no TCC) |
| `run_shell_script` | Run shell command or script (alias for execute_agent_command) |
| `batch_commands` | Run multiple shell commands sequentially in one call |
| `batch_tools` | Run multiple tool calls sequentially in one batch |
| `plan_mode` | Manage step-by-step plans (create, update, read, list, delete) |
| `task_complete` | Signal that a task has been completed |
| `write_text` | Generate well-structured prose on any topic |
| `transform_text` | Convert text into different formats (lists, outlines, summaries, tables) |
| `fix_text` | Correct spelling and grammar errors |
| `send_message` | Send message via iMessage, email, or other channels |
| `web` | Web browser automation (open, scan, click, type, execute_js, etc.) |
| `web_search` | Search the web for current information |
| `lookup_sdef` | Look up an app's AppleScript scripting dictionary |
| `list_tools` | List all enabled tools (built-in and MCP) |

### Agent Scripts (7 tools)

| Tool | Description |
|------|-------------|
| `agent` (list) | List all Swift automation scripts |
| `agent` (read) | Read source code of a script |
| `agent` (create) | Create a new Swift script |
| `agent` (update) | Update an existing script |
| `agent` (run) | Compile and run a Swift dylib script |
| `agent` (delete) | Delete a script |
| `agent` (combine) | Merge two scripts together |

### AppleScript & JavaScript (11 tools)

| Tool | Description |
|------|-------------|
| `applescript_tool` (execute) | Run inline AppleScript source with full TCC |
| `applescript_tool` (list) | List saved AppleScripts |
| `applescript_tool` (run) | Run saved AppleScript by name |
| `applescript_tool` (save) | Save AppleScript for reuse |
| `applescript_tool` (delete) | Delete saved AppleScript |
| `applescript_tool` (lookup_sdef) | Read app's scripting dictionary |
| `javascript_tool` (execute) | Run inline JXA source |
| `javascript_tool` (list) | List saved JXA scripts |
| `javascript_tool` (run) | Run saved JXA script |
| `javascript_tool` (save) | Save JXA for reuse |
| `javascript_tool` (delete) | Delete saved JXA |

### Accessibility API (12 tools)

| Tool | Description |
|------|-------------|
| `accessibility` (list_windows) | List all visible windows with positions and owner apps |
| `accessibility` (get_properties) | Get all properties of an accessibility element |
| `accessibility` (perform_action) | Perform an accessibility action (AXPress, AXConfirm, etc.) |
| `accessibility` (type_text) | Simulate keyboard typing |
| `accessibility` (click) | Simulate mouse clicks at coordinates |
| `accessibility` (press_key) | Simulate key presses with modifiers |
| `accessibility` (screenshot) | Capture screenshot of region or window |
| `accessibility` (set_properties) | Set accessibility property values |
| `accessibility` (find_element) | Find element by role/title/value with timeout |
| `accessibility` (get_children) | Get children of an accessibility element |
| `accessibility` (check_permission) | Check if Accessibility access is granted |
| `accessibility` (request_permission) | Request Accessibility permission |

### Xcode Automation (7 tools)

| Tool | Description |
|------|-------------|
| `xcode` (build) | Build an Xcode project/workspace (auto-detects) |
| `xcode` (run) | Build and run an Xcode project |
| `xcode` (list_projects) | List all open Xcode projects |
| `xcode` (select_project) | Select a project by number |
| `xcode` (add_file) | Add a file to pbxproj |
| `xcode` (remove_file) | Remove a file from pbxproj |
| `xcode` (grant_permission) | Grant macOS Automation permission for Xcode |

### MCP Tools — XCF (21 tools)

| Tool | Description |
|------|-------------|
| `mcp_xcf_xcf` | Execute an xcf action or command |
| `mcp_xcf_list` | List all available tools on this server |
| `mcp_xcf_xcf_help` | Help for xcf actions only |
| `mcp_xcf_help` | Regular help with common examples |
| `mcp_xcf_snippet` | Extract code snippets from files |
| `mcp_xcf_analyzer` | Analyze Swift code for potential issues |
| `mcp_xcf_read_dir` | List contents of a directory |
| `mcp_xcf_read_file` | Read content from a file |
| `mcp_xcf_cd_dir` | Change current directory |
| `mcp_xcf_use_xcf` | Activate XCF mode |
| `mcp_xcf_tools` | Show detailed reference for all tools |
| `mcp_xcf_show_help` | Display help information |
| `mcp_xcf_grant_permission` | Grant Xcode automation permissions |
| `mcp_xcf_run_project` | Run the current Xcode project |
| `mcp_xcf_build_project` | Build the current Xcode project |
| `mcp_xcf_show_current_project` | Show information about the current project |
| `mcp_xcf_show_env` | Display all environment variables |
| `mcp_xcf_show_folder` | Display the current working folder |
| `mcp_xcf_list_projects` | List all open Xcode projects |
| `mcp_xcf_select_project` | Select an Xcode project by number |
| `mcp_xcf_analyze_swift_code` | Analyze Swift code for potential issues |

---

## AgentScripts

Agent! includes a built-in Swift scripting system. Scripts live in `~/Documents/Agent/agents/` as a Swift Package:

```
~/Documents/Agent/agents/
├── Package.swift
└── Sources/
    ├── Scripts/           ← one .swift file per script
    │   ├── CheckMail.swift
    │   ├── Hello.swift
    │   └── ...
    └── XCFScriptingBridges/  ← one .swift file per app bridge
        ├── ScriptingBridgeCommon.swift
        ├── MailBridge.swift
        └── ...
```

### Core Scripts (bundled)

29 scripts come pre-compiled in Agent.app/Contents/Resources/:

| Script | Description |
|--------|-------------|
| `AccessibilityRecorder` | Record accessibility actions |
| `AXDemo` | Accessibility API demonstration |
| `CapturePhoto` | Capture photo from camera |
| `CheckMail` | Check for new email messages |
| `CreateDMG` | Create a DMG disk image |
| `EmailAccounts` | List email accounts |
| `ExtractAlbumArt` | Extract album artwork from Music |
| `GenerateBridge` | Generate ScriptingBridge for any app |
| `Hello` | Simple hello world script |
| `ListHomeContents` | List home directory contents |
| `ListNotes` | List Apple Notes |
| `ListReminders` | List Reminders |
| `MusicScriptingExamples` | Music app scripting examples |
| `NowPlaying` | Get currently playing track |
| `NowPlayingHTML` | Now playing info as HTML |
| `OrganizeEmails` | Organize email into folders |
| `PlayPlaylist` | Play a Music playlist |
| `PlayRandomFromCurrent` | Play random track from current playlist |
| `QuitApps` | Quit running applications |
| `RunningApps` | List running applications |
| `SDEFtoJSON` | Convert SDEF to JSON |
| `SafariSearch` | Search in Safari |
| `SaveImageFromClipboard` | Save image from clipboard |
| `Selenium` | WebDriver automation |
| `SendGroupMessage` | Send group iMessage |
| `SendMessage` | Send iMessage |
| `SystemInfo` | Get system information |
| `TodayEvents` | Get today's calendar events |
| `WebForm` | Web form automation |
| `WebNavigate` | Web navigation |
| `WebScrape` | Web scraping |

The AI can create, read, update, delete, compile, and run these scripts autonomously:

- `list_agent_scripts` — list all scripts
- `create_agent_script` — write a new script
- `read_agent_script` — read source code
- `update_agent_script` — modify an existing script
- `run_agent_script` — compile with `swift build --product <name>` and execute
- `delete_agent_script` — remove a script

### D1F Diff Integration

Agent includes the **D1F (Diff 1 Format)** package integrated as a local dependency for pretty diff output:

- **create_diff** — Compare two text strings and get a visual diff with emoji markers:
  - 📎 Retained lines (unchanged)
  - ❌ Deleted lines (removed)
  - ✅ Inserted lines (added)
- **apply_diff** — Apply D1F ASCII diffs directly to files
- **edit_file** — Shows D1F diff preview when replacing text

The D1F package lives in the project folder as a local Swift package dependency, enabling clear visual diffs for file edits without external dependencies.

### Dynamic Apple Event Queries

Agent includes an `applescript_tool` tool that lets the AI query any scriptable Mac app **instantly — with zero compilation**. It uses Objective-C dynamic dispatch to walk an app's Apple Event object graph at runtime.

| Operation | Description | Example |
|-----------|-------------|---------|
| `get` | Access a property | `{action: "get", key: "currentTrack"}` |
| `iterate` | Read properties from array items | `{action: "iterate", properties: ["name", "artist"], limit: 10}` |
| `index` | Pick one item from array | `{action: "index", index: 0}` |
| `call` | Invoke a method | `{action: "call", method: "playpause"}` |
| `filter` | NSPredicate filter | `{action: "filter", predicate: "name contains 'inbox'"}` |

### ScriptingBridges Library

Agent ships with pre-generated Swift protocol definitions for **50 macOS applications**:

| Category | Applications |
|----------|--------------|
| **Apple Apps** | Automator, Calendar, Contacts, Finder, Mail, Messages, Music, Notes, Numbers, Pages, Photos, Preview, QuickTime Player, Reminders, Safari, Script Editor, Shortcuts, System Events, Terminal, TextEdit, TV |
| **Developer Tools** | Xcode, Developer Tools, Instruments, Simulator |
| **Creative Apps** | Keynote, Logic Pro, Final Cut Pro, Adobe Illustrator, Pixelmator Pro |
| **Browsers** | Google Chrome, Firefox, Microsoft Edge |
| **System** | System Settings, System Information, Screen Sharing, Bluetooth File Exchange, Console, Database Events, Folder Actions Setup, Voice Over, UTM |
| **Legacy** | Pages Creator Studio, Numbers Creator Studio, Logic Pro Creator Studio, Final Cut Pro Creator Studio |

Each bridge is its own Swift module. Scripts import only what they need (e.g. `import MailBridge`), keeping builds fast and isolated.

### Streaming & Markdown

Agent streams responses token-by-token in real time. The activity log renders markdown inline: **bold**, *italic*, `inline code`, and fenced code blocks with syntax highlighting.

### Vision: Screenshot and Clipboard Support

Attach screenshots or paste images directly into Agent. Images are encoded as base64 PNG and sent as vision content blocks. The AI can see what's on your screen and act on it.

### Task Memory

Agent persists task history using SwiftData. Recent task messages and older task summaries are injected into the system prompt, giving the AI memory across sessions.

---

## Agent! vs. OpenClaw on Mac

| | **Agent!** | **OpenClaw** |
|---|---|---|
| **Focus** | macOS-native depth | Cross-platform breadth |
| **Runtime** | Native Swift binary | Node.js server |
| **UI** | SwiftUI app | Web chat / Telegram / CLI |
| **Privilege model** | XPC + Launch Daemon (Apple's official pattern) | Shell commands |
| **macOS integration** | Apple Events, ScriptingBridge, AppleScript, SMAppService, Accessibility | Generic shell access |
| **Xcode automation** | Built-in: build, run, grant permissions | N/A |
| **Accessibility** | Full AXUIElement API integration | Limited |
| **Scripting language** | Swift Package-based AgentScripts | Python/JS scripts |
| **MCP support** | Yes (stdio, HTTP/SSE) | Yes |
| **Messaging** | Native Apple Messages (iMessage/SMS) with per-recipient approval | WhatsApp, Telegram, Slack, Discord, iMessage, and more |
| **Message reply** | Auto-replies task results via iMessage to approved senders | Platform-specific replies |
| **App size** | ~13 MB | ~90.5 MB unpacked (npm) |
| **Installation** | Run the .app, install Xcode Command Line Tools (`xcode-select --install`) | `openclaw onboard` wizard |
| **Dependencies** | Xcode Command Line Tools | Node.js + npm ecosystem |
| **Apple Silicon** | Native ARM64 | Interpreted (Node.js) |

Both tools have their strengths. If you want a personal assistant across every messaging platform, OpenClaw is excellent. If you want an AI agent that reads Apple Messages natively, drives Xcode, compiles Swift, controls Mac apps through ScriptingBridge, and escalates to root through a proper Launch Daemon — Agent is built for that.

---

## License

MIT
