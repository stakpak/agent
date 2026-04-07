<div align="center">
<img width="256" height="256" alt="Agent! icon" src="https://github.com/user-attachments/assets/7a452184-6b31-49fa-9b24-d450d2889f66" />

# 🦾 Agent! for macOS 26

## **Agentic AI for your  Mac Desktop**
## Open Source replacement for Claude Code, Cursor, Open Claw

[![Latest Release](https://img.shields.io/github/v/release/macOS26/Agent?label=Download&color=blue&style=for-the-badge)](https://github.com/macOS26/Agent/releases/latest)
[![GitHub Stars](https://img.shields.io/github/stars/macOS26/Agent?style=for-the-badge&logo=github&label=Stars&color=hotpink)](https://github.com/macOS26/Agent/stargazers)
[![GitHub Forks](https://img.shields.io/github/forks/macOS26/Agent?style=for-the-badge&logo=github&label=Forks&color=white)](https://github.com/macOS26/Agent/fork)
[![macOS 26+](https://img.shields.io/badge/macOS-26%2B-purple?style=for-the-badge)](https://github.com/apple)
[![Swift 6.2](https://img.shields.io/badge/Swift-6.2-orange?style=for-the-badge)](https://www.swift.org)
</div>

## What's New 🚀

- **Autonomous Task Loop:** Agent! now reasons, executes, and self-corrects until the task is complete.
- **Agentic Coding:** Advanced code editing with **Time Machine-style backups** for every file change.
- **Native Xcode Tools:** Faster, project-aware builds and runs without external MCP configuration.
- **Privileged Root Access:** Secure, user-approved daemon for executing any system command.
- **Desktop Automation:** Full control of any macOS app via AXorcist (Accessibility API).
- **Expanded AI Support:** Stabilized tool calling for **Mistral** and **Google Gemini** models.
- **Unified Provider Registry:** Centralized model and URL management via `LLMRegistry`.
- **Ollama Pre-warming:** Eliminates cold-start delays by pre-loading models on launch.
- **Enhanced Logging & Diagnostics:** Improved daemon status checks and error reporting in the activity log.
- **Multi-tab LLM Configuration:** Per-tab provider/model settings for flexible multi-agent workflows.
---

A native macOS AI agent that controls your apps, writes code, automates workflows, and runs tasks from your iPhone via iMessage. All powered by the AI provider of your choice.

<img width="1600" height="900" alt="image" src="https://github.com/user-attachments/assets/f43aad58-d78d-445f-8778-4b75f37e529a" />

---

## Quick Start

1. **Download** [Agent!](https://github.com/macOS26/Agent/releases/latest) and drag to Applications
2. **Open Agent!** -- it sets up everything automatically
3. **Pick your AI** -- Settings → choose a provider → enter API key
## Quick Start

1. **Clone the repository:**
   ```bash
   git clone https://github.com/toddbruss/Agent.git
   cd Agent
   ```
2. **Open `Agent.xcodeproj` in Xcode.**
3. **Build and Run the `Agent` target.**
4. **Approve the Helper Tool:** When prompted, authorize the privileged daemon to allow root-level command execution.
5. **Configure your AI Provider:** Go to Settings and enter your API key or select a local provider like Ollama.

> 💡 **No API key?** Use **Ollama** with **GLM-5** -- completely free, runs offline, no account needed. Requires 32GB+ RAM.


## What Can It Do?

> *"Play my Workout playlist in Music"*
> *"Build the Xcode project and fix any errors"*
> *"Take a photo with Photo Booth"*
> *"Send an iMessage to Mom saying I'll be home at 6"*
> *"Open Safari and search for flights to Tokyo"*
> *"Refactor this class into smaller files"*
> *"What calendar events do I have today?"*

Just type what you want. Agent! figures out how and makes it happen.

---

## Key Features

### 🧠 Agentic AI Framework
Built-in autonomous task loop that reasons, executes, and self-corrects. Agent! doesn't just run code; it observes the results, debugs errors, and iterates until the task is complete.

### 🛠 Agentic Coding
Full coding environment built in. Reads codebases, edits files with precision, runs shell commands, builds Xcode projects, manages git, and auto-enables coding mode to focus the AI on development tools. Replaces Claude Code, Cursor, and Cline -- no terminal, no IDE plugins, no monthly fee. Features **Time Machine-style backups** for every file change, letting you revert any edit instantly.

### 🔍 Dynamic Tool Discovery
Automatically detects and uses available tools (Xcode, Playwright, Shell, etc.) based on your prompt. No manual configuration required for core tools.

### 🛡 Privileged Execution
Securely runs root-level commands via a dedicated macOS Launch Daemon. The user approves the daemon once, then the agent can execute commands autonomously via XPC.

### 🖥 Desktop Automation (AXorcist)
Control any Mac app through the Accessibility API. Click buttons, type into fields, navigate menus, scroll, drag -- all programmatically. Powered by [AXorcist](https://github.com/steipete/AXorcist) for reliable, fuzzy-matched element finding.

### 🤖 12 AI Providers
| Provider | Cost | Best For |
|---|---|---|
| **Z.ai/GLM-5.1** | Paid | Recommended starting point |
| **Claude** (Anthropic) | Paid | Complex tasks |
| **ChatGPT** (OpenAI) | Paid | General purpose |
| **Google Gemini** | Paid/Free | High performance, long context |
| **Apple Intelligence** | Free | On-device, assistant |
| **DeepSeek** | Paid | Budget cloud AI |
| **Grok-2** (xAI) | Paid | Real-time info |
| **Local Ollama** | Free | Full privacy, offline |
| **LM Studio** | Free | Easy local setup |
| **Hugging Face** | Varies | Open-source models |
| **vLLM** | Free | Local or Cloud |
| **Mistral** | AI Studio | High-performance open models |
| **Mistral Vibe** | Le Chat | High-performance open models |

## Toolbar Features

The Agent! toolbar contains **15 buttons** that provide quick access to core functionality. Each button is represented by a SF Symbol icon:

| # | Icon | Name | Description |
|---|------|------|-------------|
| 1 | ⚙️ `gearshape.2` | **Services** | Access system services and configuration options |
| 2 | 💬 `bubble.fill` | **Chat** | Open chat/conversation view for interacting with AI |
| 3 | ✋ `hand.stop` | **Stop** | Cancel/halt the current running task or operation |
| 4 | 🖥️ `server.rack` | **Servers** | Manage MCP (Model Context Protocol) servers |
| 5 | </> `chevron.left.forwardslash.chevron.right` | **Code** | Open code editor or coding mode |
| 6 | 🔧 `wrench.and.screwdriver` | **Tools** | Access tool configuration and management |
| 7 | 🖥️ `cpu` | **Settings** | LLM settings — configure AI provider, model, API keys |
| 8 | 🧠 `brain.head.profile` | **Brain** | AI brain/configuration settings for agent behavior |
| 9 | 🎚️ `slider.horizontal.3` | **Sliders** | Adjust parameters, thresholds, or preferences |
| 10 | 🔄 `arrow.clockwise` | **Refresh** | Refresh/reload current state, data, or connection |
| 11 | ⛶ `arrow.up.left.and.arrow.down.right` | **Fullscreen** | Toggle fullscreen mode for the Agent! window |
| 12 | 📊 `chart.bar` | **Chart** | View statistics, metrics, or analytics dashboard |
| 13 | ↩️ `arrow.uturn.backward` | **Undo** | Undo the last action or revert recent changes |
| 14 | 🕐 `clock.arrow.circlepath` | **History** | View activity history, past tasks, or conversation log |
| 15 | 🗑️ `trash` | **Trash** | Delete/clear selected item, conversation, or data |

---

### Detailed Button Descriptions

#### 1. Services (`gearshape.2`)
- **Purpose**: System services hub
- **Behavior**: Toggles services panel or menu
- **Details**: Provides access to background services and system-level configurations

#### 2. Chat (`bubble.fill`)
- **Purpose**: Main conversation interface
- **Behavior**: Switches to chat view
- **Details**: Primary interaction point for communicating with the AI agent

#### 3. Stop (`hand.stop`)
- **Purpose**: Emergency stop / cancel
- **Behavior**: Immediately halts any running task
- **Details**: Critical control for stopping long-running or unwanted operations

#### 4. Servers (`server.rack`)
- **Purpose**: MCP Server management
- **Behavior**: Opens MCP server configuration popover
- **Details**:
  - Displays connected/available MCP servers
  - Shows "No servers configured" when empty
  - MCP servers extend capabilities with `mcp_*` prefixed tools
  - Add, remove, and configure server connections

#### 5. Code (`chevron.left.forwardslash.chevron.right`)
- **Purpose**: Code editing mode
- **Behavior**: Activates coding-focused workspace
- **Details**: Switches context to development tools — file editing, Xcode builds, git operations

#### 6. Tools (`wrench.and.screwdriver`)
- **Purpose**: Tool configuration
- **Behavior**: Opens tool management panel
- **Details**: Configure which tools are available, set up integrations, manage tool groups

#### 7. Settings (`cpu`) ⭐ *Most Used*
- **Purpose**: LLM Configuration
- **Behavior**: Opens LLM settings sheet/popover
- **Details**:
  - Select AI provider (Claude, Ollama, OpenAI, etc.)
  - Choose model within provider
  - Enter API keys
  - Configure provider-specific parameters
  - This is the primary setup step for new users

#### 8. Brain (`brain.head.profile`)
- **Purpose**: Agent intelligence settings
- **Behavior**: Opens agent behavior configuration
- **Details**: Adjust how the AI reasons, plans, and makes decisions

#### 9. Sliders (`slider.horizontal.3`)
- **Purpose**: Parameter adjustment
- **Behavior**: Opens controls for tuning values
- **Details**: Adjust temperature, token limits, timeouts, and other configurable thresholds

#### 10. Refresh (`arrow.clockwise`)
- **Purpose**: Reload/refresh
- **Behavior**: Refreshes current state
- **Details**: Reloads data, reconnects to services, or refreshes the active view

#### 11. Fullscreen (`arrow.up.left.and.arrow.down.right`)
- **Purpose**: Toggle fullscreen
- **Behavior**: Expands Agent! to fill the screen
- **Details**: Maximizes workspace for focused task execution; click again to restore

#### 12. Chart (`chart.bar`)
- **Purpose**: Analytics/Metrics
- **Behavior**: Opens statistics dashboard
- **Details**: View usage stats, performance metrics, task history charts

#### 13. Undo (`arrow.uturn.backward`)
- **Purpose**: Revert last action
- **Behavior**: Undoes the most recent change
- **Details**: Works across file edits, configuration changes, and some UI actions

#### 14. History (`clock.arrow.circlepath`)
- **Purpose**: Activity timeline
- **Behavior**: Opens history/log view
- **Details**: Browse past conversations, completed tasks, and previous actions with timestamps

#### 15. Trash (`trash`)
- **Purpose**: Delete/Clear
- **Behavior**: Removes selected item or clears data
- **Details**: Delete conversations, clear cache, remove files, or reset current session

---

### 🎙 Voice Control
Click the microphone and speak. Agent! transcribes in real time and executes your request.

### 📱 Remote Control via iMessage
Text your Mac from your iPhone:
```
Agent! What song is playing?
Agent! Check my email
Agent! Next Song
```
Your Mac runs the task and texts back the result. Only approved contacts can send commands.

### 🌐 Web Automation
Drives Safari hands-free -- search Google, click links, fill forms, read pages, extract information.

### 📋 Smart Planning
For complex tasks, Agent! creates a step-by-step plan, works through each step, and checks them off in real time.

### 🗂 Tabs
Work on multiple tasks simultaneously. Each tab has its own project folder and conversation history.

### 📸 Screenshot & Vision
Take screenshots or paste images. Vision-capable AI models analyze what they see -- describe content, read text, spot UI issues.

### 🌐 Safari Web Automation (Built-in)

Agent! includes built-in Safari web automation via JavaScript and AppleScript. Search Google, click links, fill forms, read page content, and execute JavaScript -- all hands-free.

**To enable:** Open Safari → Settings → Advanced → check "Show features for web developers". Then go to Developer menu → check "Allow JavaScript from Apple Events".

### 🎭 Playwright Web Automation (Optional)

Full cross-browser automation via [Microsoft Playwright MCP](https://github.com/microsoft/playwright-mcp). Click, type, screenshot, and navigate any website in Chrome, Firefox, or WebKit -- all controlled by the AI.

**Setup (one-time):**

```bash
# 1. Install Node.js (if not already installed)
brew install node

# 2. Install Playwright MCP server globally
npm install -g @playwright/mcp@latest

# 3. Install browser binaries (pick one or all)
npx playwright install chromium          # Chrome (~165MB)
npx playwright install firefox           # Firefox (~97MB)
npx playwright install webkit            # Safari/WebKit (~75MB)
npx playwright install                   # All browsers
```

**Configure in Agent!:**

Go to Settings → MCP Servers → Add Server, paste this JSON:

```json
{
    "mcpServers": {
        "playwright": {
            "command": "npx",
            "args": ["@playwright/mcp"],
            "transport": "stdio"
        }
    }
}
```

> **Note:** If `npx` is not found, use the full path: run `which npx` in Terminal and replace `"npx"` with the result (e.g. `"/opt/homebrew/bin/npx"`).

Toggle ON and Playwright tools appear automatically. The AI can now control browsers directly.

### Project & Configuration Tools

These tools manage project settings and coding mode:

| Tool | What It Does |
|---|---|
| **project_folder** | Get or change the working directory for this tab — use `set`, `home`, `documents`, `library`, or `none` |
| **coding_mode** | Toggle coding mode on/off — when ON, only Core+Workflow+Coding+UserAgent tools are available for faster responses |
| **plan_mode** | Create, update, read, list, or delete step-by-step plans with status tracking — ideal for complex tasks |
| **memory** | Read/write persistent user preferences — use `append` to remember things across sessions |

> 💡 **Pro Tip:** Use `coding_mode(true)` when working on code — it removes unnecessary tools and speeds up responses.

These tools interact with macOS UI and web pages:

| Tool | What It Does |
|---|---|
| **accessibility** | Control any app — click buttons, type text, read elements, manage windows, navigate menus, capture screenshots |
| **web** | Automate Safari — open URLs, click elements, type text, execute JS, search, navigate tabs |
| **mcp_playwright_browser_*** | Advanced browser automation via Playwright — snapshot, click, hover, drag, fill forms, upload files, etc. |
| **web_search** | Search the web for current information — returns relevant page titles, URLs, and content snippets |

> 💡 **Pro Tip:** Use `accessibility` for macOS UI automation — it’s faster and more reliable than screenshots.

These tools manage Swift and AppleScript automation scripts:

| Tool | What It Does |
|---|---|
| **agent** | Create, read, update, run, delete, or combine Swift automation scripts with TCC permissions |
| **applescript_tool** | Execute, save, delete, or list AppleScript scripts — use `lookup_sdef` to inspect app dictionaries |
| **javascript_tool** | Run JXA (JavaScript for Automation) scripts — ideal for lightweight automation tasks |
| **batch_tools** | Run multiple tool calls in one batch with progress tracking — no round-trips, ideal for complex workflows |

> 💡 **Pro Tip:** Use `agent` for Swift scripts that need TCC permissions — it’s the most powerful scripting tool.

These tools execute shell commands and system-level operations:

| Tool | What It Does |
|---|---|
| **execute_agent_command** | Run shell commands as current user — use for git, ls, grep, find, homebrew, scripts |
| **execute_daemon_command** | Run shell commands as ROOT via Launch Daemon — no sudo needed, use for system logs, disk ops, network debug |
| **run_shell_script** | Execute shell scripts with automatic fallback to in-process when User Agent is off |
| **batch_commands** | Run multiple shell commands in one call — no round-trips, ideal for setup scripts |

> 💡 **Pro Tip:** Use `execute_daemon_command` instead of `sudo` — it’s safer and doesn’t require password prompts.

These tools handle file operations and version control:

| Tool | What It Does |
|---|---|
| **file_manager** | Read/write/edit/list/search files — use `diff_apply` for code changes, `edit` for single-line fixes |
| **git** | Git operations: status, diff, log, commit, branch — always use this instead of shell git commands |
| **xcode** | Build/run Xcode projects, analyze/snippet Swift code, add/remove files, grant permissions |

> 💡 **Pro Tip:** Always use `file_manager` for file operations — it’s safer and more reliable than shell commands.

These tools help manage the agent's workflow and state:

| Tool | What It Does |
|---|---|
| **plan_mode** | Create, update, read, list, or delete step-by-step plans with status tracking |
| **memory** | Read/write persistent user preferences — store notes, settings, or context across sessions |
| **coding_mode** | Toggle coding mode on/off to restrict available tools for focused development |
| **project_folder** | Get or change the working directory for this tab — set to home, documents, library, or custom path |

> 💡 **Pro Tip:** Use `plan_mode` to break complex tasks into manageable steps and track progress.

These are the foundational tools that every agent needs:

| Tool | What It Does |
|---|---|
| **task_complete** | Signal when a task is finished — always call this at the end of any task |
| **list_tools** | List all available tools and their descriptions |
| **web_search** | Search the web for current information or facts you're unsure about |

> 💡 **Pro Tip:** Always call `task_complete` at the end of every task to signal completion and avoid hanging.

These tools enable system automation and UI interaction:

| Tool | What It Does |
|---|---|
| **applescript_tool** | Execute AppleScript, list scripts, save/delete, or lookup SDEFs for apps |
| **accessibility** | Control any macOS app via AX API — click buttons, type text, read elements, manage windows |
| **javascript_tool** | Run JXA (JavaScript for Automation) scripts, list/save/delete scripts |
| **lookup_sdef** | Inspect AppleScript dictionary definitions for any app (e.g., Music, Safari) |

> 💡 **Pro Tip:** Use `accessibility` to automate UI interactions across all macOS apps — it’s the most powerful tool for GUI automation.

These tools provide code editing, file management, and Xcode integration capabilities:

| Tool | What It Does |
|---|---|
| **read_file** | Read the contents of any file in the project |
| **write_file** | Write content to a file (creates if doesn't exist) |
| **edit_file** | Replace exact string matches in a file |
| **create_diff** | Preview changes before applying them to a file |
| **apply_diff** | Apply previously previewed changes to a file |
| **diff_and_apply** | Create and apply changes to a file in one step |
| **undo_edit** | Revert the last edit made to a file |
| **list_files** | List files in a directory with optional pattern matching |
| **search_files** | Search for files containing specific text patterns |
| **read_dir** | Get detailed information about files in a directory |
| **file_manager** | Comprehensive file operations including read, write, edit, list, search |
| **xcode** | Build, run, analyze, and manage Xcode projects |
| **project_folder** | Set or get the current project directory |
| **mode** | Toggle coding mode on/off for optimized tool selection |

> 💡 **Pro Tip:** Use `create_diff` to preview changes before applying them with `apply_diff` to avoid accidental edits.

## Privacy & Safety

- **Your data stays on your Mac.** Files, screen contents, and personal data are never uploaded.
- **Cloud AI only sees your prompt text.** Use local AI to stay 100% offline.
- **You're in control.** Agent! shows everything it does and logs every action.
- **Built on Apple's security model.** macOS permissions protect your system.

---

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| `Enter` | Run task |
| `⌘ R` | Run current task |
| `⌘ .` | Stop task |
| `Escape` | Cancel active task |
| `⌘ D` | Toggle LLM output panel |
| `⌘ T` | New tab |
| `⌘ W` | Close tab |
| `⌘ 1-9` | Switch to tab |
| `⌘ [` / `⌘ ]` | Previous / next tab |
| `⌘ F` | Search activity log |
| `⌘ L` | Clear conversation |
| `⌘ H` | Task history |
| `⌘ ,` | Settings |
| `⌘ V` | Paste image |
| `↑` / `↓` | Prompt history |

---

## FAQ

**Do I need to know how to code?** No. Just type what you want in plain English.

**Is it safe?** Yes. Standard macOS automation, full activity logging, you approve permissions.

**How much does it cost?** Agent! is free (MIT License). Cloud AI providers charge for API usage. Local models are free.

**What Mac do I need?** macOS 26+. Apple Silicon recommended. 32GB+ RAM for local models.

**How is this different from Siri?** Siri answers questions. Agent! *performs actions* -- controls apps, manages files, builds code, automates workflows.

---

## Documentation

- [Technical Architecture](docs/TECHNICAL.md) -- Tools, scripting, developer details
- [Comparisons](docs/COMPARISON.md) -- vs Claude Code, Cursor, Cline, OpenClaw
- [Security Model](docs/SECURITY.md) -- XPC architecture, privilege separation
- [FAQ](docs/FAQ.md) -- Common questions

---

## Built-in Xcode Tools

Agent! includes native Xcode integration that works without any MCP server setup. These built-in tools are often faster and more reliable than the MCP alternative since they run directly inside the app.

| Tool | What It Does |
|---|---|
| **xcode build** | Build the current Xcode project, capture errors and warnings. Errors in the activity log are **clickable** and open directly in Xcode. |
| **xcode run** | Build and run the app |
| **xcode list_projects** | Discover open Xcode workspaces and projects |
| **xcode select_project** | Switch the active project |
| **xcode grant_permission** | Grant file access to the Xcode project folder |

The AI automatically uses these when you ask it to build, fix errors, or work with Xcode projects. No configuration needed -- just have your project open in Xcode.

> 🚀 **iOS/iPadOS Support:** Coming soon! Native support for building, running, and testing iOS and iPadOS apps directly from Agent! is in development.

> **Tip:** For most coding workflows, the built-in tools are all you need. The MCP Xcode server below adds extras like SwiftUI Preview rendering and documentation search.


---

<img width="1349" height="1438" alt="Screenshot 2026-04-02 at 12 00 03 PM" src="https://github.com/user-attachments/assets/b0d9346e-f807-4089-bab3-29c7058868d8" />

## Messages App Integration

Agent! can be controlled via voice command "Agent!" using the Messages app. This feature allows users to send commands to Agent! through text messages, enabling remote control and automation of tasks on their macOS device.

### How It Works

1. **Voice Command Setup**: Users can set up a voice command in macOS that triggers sending a message to a predefined contact or group chat.
2. **Message Reception**: Agent! monitors incoming messages for specific keywords or phrases (e.g., "Agent!").
3. **Command Execution**: Upon detecting the keyword, Agent! parses the message content and executes the corresponding task or command.
4. **Response**: Agent! sends a reply message back to the sender with the results or status of the executed command.

### Example Use Cases

- **Remote Task Execution**: Send a message like "Agent! open Finder" to remotely open the Finder application.
- **System Commands**: Execute system commands such as "Agent! restart" to restart the computer.
- **File Operations**: Perform file operations like "Agent! copy /path/to/file" to copy files to a specified location.

### Configuration

To enable this feature, users need to configure the Messages app to allow Agent! to access and monitor incoming messages. This can be done through the system preferences under Security & Privacy → Privacy → Accessibility.

> **Note**: Ensure that the Messages app is running and that the user has granted the necessary permissions for Agent! to interact with it.
### Services Popover Button

The Services button (gear icon) provides quick access to project folder management and task configuration options:

- **Move/Go Down**: Navigate to a different project folder location
- **New Folder**: Create a new folder for your project
- **Home**: Quickly return to your home directory
- **Close**: Clear the current project folder selection
- **Project Folder Input**: Enter or paste a custom project folder path
- **Folder Size Display**: Shows the current folder size (e.g., "20.0M")
- **User Prompt**: Configure user prompts for tasks
- **Cancel**: Cancel the current operation
- **Thinking Indicator**: Shows when Agent! is processing a task
- **Task Progress**: Displays progress information during task execution
- **Context Usage**: Shows how much context is being used for the current task
- **More Options**: Additional configuration settings
- **Dismiss**: Close the popover (currently disabled when active)
- **Steps**: View the steps of the current task (7 steps shown)
- **Screenshot**: Take a screenshot to attach to your task
- **Paste Image**: Paste an image from clipboard into the task
- **Cancel Task**: Cancel a running task
- **Dictation**: Start voice dictation for entering tasks
- **Hotword**: Activate voice command with "Agent!"
- **Run Task**: Execute the current task (currently disabled when not ready)
- **Task Input Field**: Enter your task description here

This popover provides comprehensive control over your project environment and task execution workflow.


Agent! supports [MCP](https://modelcontextprotocol.io) servers for extended capabilities. Configure in Settings → MCP Servers.

### Xcode MCP Server

Connect Agent! directly to Xcode for project-aware operations:

```json
{
  "mcpServers" : {
    "xcode" : {
      "command" : "xcrun",
      "args" : [
        "mcpbridge"
      ],
      "transport" : "stdio"
    }
  }
}
```

**Xcode MCP provides:**
- Project-aware file operations (read/write/edit/delete)
- Build and test integration
- SwiftUI Preview rendering
- Code snippet execution
- Apple Developer Documentation search
- Real-time issue tracking


---

## License

MIT - free and open source.

---

<div align="center">

### **Agent! for macOS 26 - Agentic AI for your  Mac Desktop**
> Note: Claude refers to the Anthropic AI model integrated into Agent! for LLM functionality. It is not a human contributor Agent!
</div>

---

## Agent! vs Claude Code — Architectural Comparison

Agent! is a 100% original pure Swift macOS application. It is not a port, fork, or derivative of any other project.

| | Claude Code | Agent! |
|---|---|---|
| **Language** | TypeScript/JavaScript | Pure Swift 6.2 |
| **UI Framework** | Ink (terminal React) | SwiftUI (native macOS) |
| **Platform** | CLI — Linux, macOS, Windows | Native macOS 26 only |
| **Runtime** | Node.js/Bun | Native compiled binary |
| **Architecture** | Terminal REPL with streaming | Desktop app with XPC daemons |
| **Accessibility** | None (CLI) | Full macOS AX via AXorcist (30+ actions) |
| **AppleScript** | None | Full NSAppleScript + JXA |
| **Xcode Integration** | None | Native build/run/analyze |
| **Apple Intelligence** | None | FoundationModels on-device |
| **ScriptingBridge** | None | Full SDEF + event bridges |
| **Vision/Screenshots** | None | Auto-verification after UI actions |
| **iMessage** | None | Remote agent via Messages |
| **UI** | Terminal text | Native SwiftUI with CRT shader |
| **Privilege Model** | User sandbox | XPC Launch Agent + Daemon |
| **Sub-agents** | Fork-based with shared cache | Independent tasks with mailbox |
| **MCP** | Node.js stdio/SSE | Swift AgentMCP package |
| **Scripts** | None | Swift dylib compilation at runtime |
