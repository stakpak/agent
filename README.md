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

<img width="1175" height="1257" alt="image" src="https://github.com/user-attachments/assets/4c4fc9bb-069c-4134-a3e0-090160e4bb20" />

---

## Quick Start (Download)

1. **Download** [Agent!](https://github.com/macOS26/Agent/releases/latest) and drag to Applications
2. **Open Agent!** -- it sets up everything automatically
3. **Pick your AI** -- Settings → choose a provider → enter API key

## Quick Start (Build from Source)

1. **Clone the repository:**
   ```bash
   git clone https://github.com/toddbruss/Agent.git
   cd Agent
   ```
2. **Open `Agent.xcodeproj` in Xcode.**
3. **Build and Run the `Agent` target.**
4. **Approve the Helper Tool:** When prompted, authorize the privileged daemon to allow root-level command execution.
5. **Configure your AI Provider:** Go to Settings and enter your API key or select a local provider like Ollama.

> 💡 **No API key?** Use **Ollama** or **Hugging Face** with **GLM-5** -- completely free, runs offline (Ollama) or in the cloud (HF), no account needed for Ollama. Requires 32GB+ RAM for local.
>
> 💡 **Want the latest?** **Z.ai** ships **GLM-5.1** via API -- paid, but it's the recommended starting point for cloud use.


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

## Toolbar Buttons

The Agent! header contains **15 buttons** for quick access to settings, monitors, and tools. Each button opens a popover when clicked. Source of truth: `Agent/Views/HeaderSectionView.swift`.

| # | Icon | Name | What it does |
|---|------|------|--------------|
| 1 | `gearshape.2` | **Services** | Toggle the Launch Agent / Launch Daemon, manage project folder, scan command output |
| 2 | `message.fill` | **Messages Monitor** | Toggle iMessage monitoring on/off — green when active. Opens the recipients list and approval UI |
| 3 | `hand.raised` | **Accessibility** | Open the Accessibility settings sheet (permission status, axorcist diagnostics) |
| 4 | `server.rack` | **MCP Servers** | Add/remove/configure MCP (Model Context Protocol) servers — extends Agent! with `mcp_*` tools |
| 5 | `chevron.left.forwardslash.chevron.right` | **Coding Preferences** | Toggle auto-verify, visual tests, auto-PR, auto-scaffold. Green when any are on |
| 6 | `wrench.and.screwdriver` | **Tools** | Per-provider tool toggles. Enable/disable individual built-in and MCP tools |
| 7 | `cpu` | **LLM Settings** | Pick AI provider, model, API key, base URL. Pulses when a task is running |
| 8 | `brain.fill` / `brain` | **Apple Intelligence** | Configure FoundationModels (on-device Apple AI). Filled when available |
| 9 | `slider.horizontal.3` | **Agent Options** | Temperature, max iterations, vision auto-screenshot, plan-mode encouragement, etc. |
| 10 | `arrow.triangle.2.circlepath` | **Fallback Chain** | Configure provider fallback order — Agent! retries with the next provider when one fails |
| 11 | `viewfinder` | **HUD** | Toggle the green-CRT scanline overlay on the LLM Output view |
| 12 | `chart.bar.fill` | **LLM Usage** | Per-model token usage and cost tracking. Green when there's recorded usage |
| 13 | `arrow.uturn.backward.circle` | **Rollback** | Time-Machine-style file backup browser. Restore any previous version of any file Agent! edited |
| 14 | `clock.arrow.circlepath` | **History** | Past prompts, errors, and task summaries for the active tab. Re-run a previous prompt with one click |
| 15 | `trash` | **Clear Log** | Delete the activity log for the active tab (or all task history when no tab is selected). Confirms first |

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

### Tools — what `list_tools` actually returns

These are the canonical tool names defined in `AgentTools.Name.*` and exposed to every LLM provider via `AgentTools.tools(for:)`. Source of truth: `~/Documents/GitHub/AgentTools/Sources/AgentTools/AgentTools.swift`. The Agent app's user-pref toggles can hide individual tools per-provider, but the list below is the full set the LLM ever sees.

#### Core / discovery

| Tool | Actions / args | What it does |
|---|---|---|
| **done** | `summary` | Signal task complete. Required at end of every task |
| **list_tools** | — | Returns the live tool list for the current provider (built-in + MCP) |
| **search** | `query` | Web search via Tavily |
| **chat** | `write` / `transform` / `fix` / `about` | Write prose, transform/fix text, describe Agent capabilities |
| **memory** | `read` / `write` / `append` / `clear` | Persistent user preferences. "remember X" → `append` |
| **plan** | `create` / `update` / `read` / `list` / `delete` | Multi-plan CRUD with per-step status tracking |
| **directory** | `get` / `set` / `home` / `documents` / `library` / `none` / `cd` | Project folder for the current tab |
| **fetch** | `url` | Fetch URL, strip HTML, cap 8K chars |
| **skill** | `list` / `invoke` / `save` / `delete` | Reusable prompt templates |
| **ask_user** | `question` | Mid-task user dialog (waits up to 5 min) |

#### Code / files / build

| Tool | Actions / args | What it does |
|---|---|---|
| **file** | `read` / `write` / `edit` / `create` / `apply` / `undo` / `diff_apply` / `list` / `search` / `read_dir` / `mkdir` / `cd` / `if_to_switch` / `extract_function` | All file operations. `edit` = single-string replace. `diff_apply` = preferred for multi-line code edits |
| **git** | `status` / `diff` / `log` / `commit` / `diff_patch` / `branch` / `worktree` | Git operations — use this instead of shell git |
| **xcode** | `build` / `run` / `list_projects` / `select_project` / `add_file` / `remove_file` / `grant_permission` / `analyze` / `snippet` / `code_review` / `get_version` / `bump_version` / `bump_build` | Native Xcode integration. Errors in the activity log are clickable |
| **agent_script** | `list` / `read` / `create` / `update` / `run` / `delete` / `combine` | Swift dylib scripts in `~/Documents/AgentScript/agents/` with full TCC |

#### Shell / privilege tiers

| Tool | Args | What it does |
|---|---|---|
| **user_shell** | `command` | Shell as current user via Launch Agent. Primary shell tool |
| **root_shell** | `command` | Shell as ROOT via Launch Daemon. Admin tasks only — no sudo |
| **shell** | `command` | Fallback in-process shell (when Launch Agent is off) |
| **batch** | `commands` | Multiple shell commands in one call (newline-separated) |
| **multi** | `description`, `tasks` | Multiple tool calls in one batch |

#### macOS automation

| Tool | Actions / args | What it does |
|---|---|---|
| **accessibility** | `open_app` / `find_element` / `click_element` / `type_into_element` / `scroll_to_element` / `list_windows` / `inspect_element` / `get_properties` / `perform_action` / `set_properties` / `get_focused_element` / `get_children` / `read_focused` / `wait_for_element` / `wait_adaptive` / `highlight_element` / `manage_app` / `show_menu` / `click_menu_item` / `set_window_frame` / `get_window_frame` / `screenshot` / `check_permission` / `request_permission` / `get_audit_log` | Element-based AXorcist automation. Every action takes `role`+`title`+`appBundleId` — no coordinates |
| **applescript** | `execute` / `lookup_sdef` / `list` / `run` / `save` / `delete` | NSAppleScript in-process with TCC |
| **javascript** | `execute` / `list` / `run` / `save` / `delete` | JXA (JavaScript for Automation) |

#### Web automation

| Tool | Actions / args | What it does |
|---|---|---|
| **safari** | `open` / `find` / `click` / `type` / `execute_js` / `get_url` / `get_title` / `read_content` / `google_search` / `scroll_to` / `select` / `submit` / `navigate` / `list_tabs` / `switch_tab` / `list_windows` / `scan` / `search` | Safari automation via JavaScript + AppleScript |
| **selenium** | `start` / `stop` / `navigate` / `find` / `click` / `type` / `execute` / `screenshot` / `wait` | Selenium WebDriver session — use `safari` for normal Safari |
| **mcp_playwright_browser_\*** | (see Playwright MCP) | Optional. Cross-browser automation via Playwright MCP |

#### Sub-agents

| Tool | Args | What it does |
|---|---|---|
| **spawn_agent** | `name`, `prompt`, `tools`, `max_iterations` | Spawn isolated sub-agent. Max 3 concurrent. Independent message history + mailbox |
| **tell_agent** | `to`, `message` | Send a message to a running sub-agent's mailbox |

> 💡 **Note:** The on-device app filters this list per-provider — toggle individual tools in the **Tools** popover (button #6 in the toolbar above). Apple Intelligence has its own minimal default set because of its small context window. MCP tools are appended at runtime as `mcp_<server>_<tool>` and listed under "--- MCP Tools ---" by `list_tools`.

## Privacy & Safety

- **Your data stays on your Mac.** Files, screen contents, and personal data are never uploaded.
- **Cloud AI only sees your prompt text.** Use local AI to stay 100% offline.
- **You're in control.** Agent! shows everything it does and logs every action.
- **Built on Apple's security model.** macOS permissions protect your system.

---

## Keyboard Shortcuts

Source of truth: the inline `NSEvent.addLocalMonitorForEvents` block in `Agent/Views/ContentView.swift`.

| Shortcut | Action |
|---|---|
| `⌘ Return` | Run current task |
| `⌘ .` / `Escape` | Cancel running task |
| `⌘ B` | Toggle LLM Output overlay (show/hide) |
| `⌘ D` | Toggle both LLM chevrons on the current tab (expand/collapse) |
| `⌘ T` | New tab |
| `⌘ W` | Close current tab (or quit if no tabs) |
| `⌘ 1`–`⌘ 9` | Switch tab. `⌘1` is the main tab; `⌘2`–`⌘9` are script tabs |
| `⌘ Shift ←` / `⌘ Shift →` | Previous / next tab |
| `⌘ F` | Toggle activity-log search bar |
| `⌘ L` | Clear log for the active tab |
| `⌘ V` | Paste image from clipboard |
| `↑` / `↓` | Prompt history (in the input field) |
| `⌘ Shift M` | Toggle Messages Monitor on/off |
| `⌘ Shift P` | Open Settings (system prompt editor lives here) |
| `⌘ Shift K` | Clear all (full reset) |
| `⌘ Shift L` | Clear LLM output panel only |
| `⌘ Shift H` | Clear prompt history |
| `⌘ Shift J` | Clear task history |
| `⌘ Shift U` | Clear token counters |

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

## Two ways to talk to Agent! — voice and iMessage

Both features use the same wake word: **"Agent!"** (case-insensitive — `Agent!`, `agent!`, `AGENT!`, even just `Agent ` or `agent ` all work).

### 🎤 Voice (dictation hotword)

Click the microphone in the input bar and start the hotword session, then speak. Agent! transcribes in real time using `SFSpeechRecognizer` and listens for the word "agent" as a complete word (not as a substring of "intelligent" or "management"). Anything you say after "agent" becomes the task. After ~2.5 seconds of silence, the task auto-runs.

Examples:
- *"Agent, what song is playing?"*
- *"Agent take a screenshot of Safari"*
- *"Agent build the Xcode project"*

The hotword session loops automatically — after one task completes, it goes back to listening. Click the mic again to stop.

### 📱 iMessage (remote control)

Text your Mac from your iPhone. Agent! polls `~/Library/Messages/chat.db` every 5 seconds for new messages and reacts to anything starting with **`Agent!`** (case-insensitive, exclamation mark optional).

Examples:
```
Agent! What song is playing?
agent! check my email
AGENT! next song
Agent  open Safari
```

Agent! sends an immediate "Working on it..." acknowledgment, runs the task on a dedicated Messages tab using your main tab's LLM config, and then texts the result back to you.

**Setup (one-time):**

1. **Grant Full Disk Access** — System Settings → Privacy & Security → Full Disk Access → enable Agent! (required to read `chat.db` directly via SQLite)
2. **Open the Messages Monitor** — toolbar button #2 (chat bubble icon, turns green when on)
3. **Approve a sender** — once a message arrives from a new contact, that contact appears in the recipients list. Toggle them on to approve.

Only approved senders can run tasks. Unapproved messages are logged but ignored. Your reply is sent back via AppleScript to the same handle that sent the command, capped at 4000 characters.

Outgoing replies have any leading "Agent!" stripped so the receiving Mac doesn't trigger its own command loop.

---

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

## Agent's first poem written in Pages
<img width="1155" height="1416" alt="image" src="https://github.com/user-attachments/assets/e71ceb3d-7ca2-4225-8138-b4b9beefdbd2" />

## Agent! vs Claude Code — Architectural Comparison

Agent! is a 100% original pure Swift macOS application. It is not a port, fork, or derivative of any other project.

| | Claude Code | Agent! |
|---|---|---|
| **Language** | TypeScript/JavaScript | Pure Swift 6.2 |
| **UI Framework** | Ink (terminal React) | SwiftUI (native macOS) |
| **Platform** | CLI — Linux, macOS, Windows | Native macOS 26 only |
| **Runtime** | Node.js/Bun | Native compiled binary |
| **Architecture** | Terminal REPL with streaming | Desktop app with XPC daemons |
| **Accessibility** | None (CLI) | Full macOS AX via AXorcist (25 top-level actions, 30+ AX subtypes via `perform_action`) |
| **AppleScript** | None | Full NSAppleScript + JXA in-process with TCC |
| **Xcode Integration** | Via Bash (`xcodebuild`) | Native (build/run/analyze/snippet/add_file/bump_version/code_review — 13 actions) |
| **Apple Intelligence** | None | FoundationModels on-device |
| **ScriptingBridge** | None | Full SDEF + 51 event bridges (Finder, Mail, Music, Safari, Calendar, etc.) |
| **Vision** | Image input via API | Image input via API |
| **Auto-screenshots** | None (no UI) | Opt-in auto-verification after UI actions (default OFF — see `visionAutoScreenshotEnabled`) |
| **iMessage** | None | Remote agent via Messages (Full Disk Access required for `chat.db`) |
| **Voice** | None | Hotword-anchored dictation via SFSpeechRecognizer |
| **CRT effect** | None | Optional SwiftUI Canvas scanline overlay (toggle via HUD button) |
| **Privilege Model** | User sandbox | XPC Launch Agent (user) + Launch Daemon (root) |
| **Sub-agents** | Task tool (publicly documented; implementation details not stated by Anthropic) | Up to 3 concurrent isolated agents with mailbox messaging |
| **MCP** | Node.js stdio/SSE | Swift AgentMCP package |
| **Scripts** | None | Swift dylib compilation at runtime, dlopen'd in-process with full TCC |
| **Prompt caching** | Anthropic `cache_control` ephemeral | Anthropic `cache_control` ephemeral + automatic prefix-cache hit tracking for OpenAI/Z.ai/Grok/Mistral/Gemini/Qwen/DeepSeek; Ollama `keep_alive: 30m` |
