
<div align="center">
   <p>🎗️ Our Founder! of this project is battling cancer. Your <b>Stars</b> and <b>Forks</b> are appreciated. 🎗️</p>
<img width="256" height="256" alt="Agent! icon" src="https://github.com/user-attachments/assets/7a452184-6b31-49fa-9b24-d450d2889f66" />

# 🦾 Agent! for macOS 26.4+

<img width="1326" height="1428" alt="image" src="https://github.com/user-attachments/assets/9616193e-ab81-456b-ae8f-6fc182d0d6b0" />

## **Agentic AI for your  Mac Desktop**
## Open Source replacement for Claude Code, Cursor, Cline, OpenClaw

[![Latest Release](https://img.shields.io/github/v/release/macOS26/Agent?label=Download&color=blue&style=for-the-badge)](https://github.com/macOS26/Agent/releases/latest)
[![GitHub Stars](https://img.shields.io/github/stars/macOS26/Agent?style=for-the-badge&logo=github&label=Stars&color=hotpink)](https://github.com/macOS26/Agent/stargazers)
[![GitHub Forks](https://img.shields.io/github/forks/macOS26/Agent?style=for-the-badge&logo=github&label=Forks&color=white)](https://github.com/macOS26/Agent/fork)
[![macOS 26.4+](https://img.shields.io/badge/macOS-26.4%2B-purple?style=for-the-badge)](https://github.com/apple)
[![Swift 6.2](https://img.shields.io/badge/Swift-6.2-orange?style=for-the-badge)](https://www.swift.org)
<p><a href="https://www.paypal.com/ncp/payment/9C6RY2UAE5M3S"><img src="https://img.shields.io/badge/Tip_Jar-PayPal-00457C?style=for-the-badge&logo=paypal&logoColor=white" alt="Tip Jar" /></a>
</div>

## What's New 🚀

- **Apple AI as a real tool-calling agent:** On-device Apple Intelligence (FoundationModels.Tool) handles UI automation requests like *"take a photo using Photo Booth"* locally — multi-step tool calls, zero cloud LLM tokens, falls through to the cloud LLM only on failure.
- **SDEF + runtime app discovery:** Bundle ID resolution is now zero-hardcoded. Apps in `Agent/SDEFs/` plus every `.app` in `/Applications`, `/System/Applications`, `~/Applications` are discovered at runtime — installing a new app extends what the agent can target with no code edit.
- **Prompt caching for every OpenAI-format provider:** Z.ai, OpenAI, Grok, Mistral, DeepSeek, Qwen, Gemini, BigModel, Hugging Face — `cached_tokens` is parsed from the response and shown in the LLM Usage panel. JSON request bodies use `.sortedKeys` so byte-stable prefixes actually hit the provider's cache.
- **On-device token compression:** Apple AI summarizes old conversation turns when context exceeds 30K tokens (Tier 1 of `tieredCompact`) — free, private, no API tokens consumed. Toggleable in the brain icon popover.
- **Anti-hallucination prompt rule:** Every system prompt now includes explicit guidance against fabricating findings from incomplete tool reads. The 10-consecutive-reads guard pushes the model toward "narrow or call done()" instead of "guess".
- **Autonomous task loop, Xcode integration, AXorcist desktop automation, privileged daemon, multi-tab LLM config, Ollama pre-warming via `LLMRegistry`** — all the previously-shipped fundamentals are still there.

A native macOS AI agent that controls your apps, writes code, automates workflows, and runs tasks from your iPhone via iMessage. All powered by the AI provider of your choice.

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

#### Option A: Build with Xcode (Apple Developer account)
2. **Open `Agent.xcodeproj` in Xcode.**
3. **Build and Run the `Agent` target.**
4. **Approve the Helper Tool:** When prompted, authorize the privileged daemon to allow root-level command execution.

#### Option B: Build without an Apple Developer account
2. **Run the build script** (requires only Xcode Command Line Tools):
   ```bash
   ./build.sh              # Debug build
   ./build.sh Release      # Release build
   ```
3. The app lands in `build/DerivedData/Build/Products/Debug/Agent!.app`
4. **Run it:** `open "build/DerivedData/Build/Products/Debug/Agent!.app"`

> ⚠️ Without a developer account the app is ad-hoc signed. The Launch Agent/Daemon helpers won't register (SMAppService needs a team ID), but the LLM loop, all tools, accessibility, AppleScript, shell, and MCP all work.

#### Then:
5. **Configure your AI Provider:** Go to Settings and enter your API key or select a local provider like Ollama.

> 💡 **Cheapest cloud path?** **GLM-5.1** (the latest) is now available on **all four** of the cheap cloud providers — **Ollama**, **Hugging Face**, **Z.ai**, and **BigModel**. Pennies per million tokens vs Claude/GPT pricing. Pick whichever you already have an account with; pricing is competitive across all of them.
>
> 💡 **Z.ai is still the recommended starting point** if you don't have an account anywhere yet — fastest signup, GLM-5.1 is the default model, no infrastructure to provision.
>
> ⚠️ **Running GLM locally?** Only **GLM-4.7-Turbo** (32B) runs well on consumer hardware — M2/M3/M4 Mac with 64-128GB unified memory via Ollama. GLM-5 (744B MoE) and GLM-5.1 (754B MoE) are too large to run locally (~1.6TB full weight) — use them via **Z.ai**, **BigModel**, **Hugging Face** cloud, or **Ollama** cloud.


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

### 🤖 17 AI Providers

The provider picker (LLM Settings, toolbar button #7) shows 16 providers; Apple Intelligence is reached via the separate brain icon (#8). Source of truth: `AgentTools.APIProvider`.

| Provider | API key | Best for |
|---|---|---|
| **Claude** (Anthropic) | Paid | Long autonomous tasks, complex reasoning, prompt caching |
| **OpenAI** | Paid | General purpose, tool calling, vision |
| **Google Gemini** | Paid (free tier) | Long context, vision, fast |
| **Grok** (xAI) | Paid | Real-time info |
| **Mistral** | Paid | Open-weight cloud, fast tool calling |
| **Codestral** (Mistral) | Paid | Code-specialized Mistral |
| **Mistral Vibe** | Paid | Mistral's chat/agent product |
| **DeepSeek** | Cheap | Budget cloud, strong coding, prompt cache hit reporting |
| **Hugging Face** | Varies | Open-source models hosted serverless or on dedicated endpoints |
| **Z.ai** | Cheap | GLM-5.1 via API — recommended starting point |
| **BigModel** (Zhipu) | Cheap | GLM family via Zhipu's API |
| **Qwen** (Alibaba) | Cheap | Qwen 2.5 / 3 via Dashscope |
| **Ollama** (cloud) | Free tier | Run open models via Ollama's hosted endpoint |
| **Local Ollama** | Free + hardware | Self-hosted Ollama daemon — fully offline, no account |
| **vLLM** | Free + hardware | Self-hosted vLLM server with prefix caching |
| **LM Studio** | Free + hardware | Self-hosted, easiest GUI for local models |
| **Apple Intelligence** | Free, on-device | Triage, summary, accessibility intent (via brain icon, not the provider picker) |

> 💡 **Self-hosted "free" providers (Local Ollama, vLLM, LM Studio) are only free in the API-fee sense.** Running a 30B+ model with usable speed needs an M2/M3/M4 Ultra Mac Studio (64-128GB unified memory) or a Linux box with 24GB+ VRAM. If you don't already have that hardware, the cloud paths above (Ollama Cloud, Hugging Face, Z.ai, BigModel, DeepSeek) are dramatically cheaper than buying it.

## Toolbar Buttons

The Agent! header contains **15 buttons** for quick access to settings, monitors, and tools. Each button opens a popover when clicked. Source of truth: `Agent/Views/HeaderSectionView.swift`.

| # | Icon | Name | What it does |
|---|------|------|--------------|
| 1 | ⚙️ | **Services** | Toggle the Launch Agent / Launch Daemon, manage project folder, scan command output |
| 2 | 💬 | **Messages Monitor** | Toggle iMessage monitoring on/off — green when active. Opens the recipients list and approval UI |
| 3 | ✋ | **Accessibility** | Open the Accessibility settings sheet (permission status, axorcist diagnostics) |
| 4 | 🖥️ | **MCP Servers** | Add/remove/configure MCP (Model Context Protocol) servers — extends Agent! with `mcp_*` tools |
| 5 | </> | **Coding Preferences** | Toggle auto-verify, visual tests, auto-PR, auto-scaffold. Green when any are on |
| 6 | 🔧 | **Tools** | Per-provider tool toggles. Enable/disable individual built-in and MCP tools |
| 7 | 🧠 | **LLM Settings** | Pick AI provider, model, API key, base URL. Pulses when a task is running |
| 8 | 🧬 | **Apple Intelligence** | Configure FoundationModels (on-device Apple AI). Filled when available |
| 9 | 🎛️ | **Agent Options** | Temperature, max iterations, vision auto-screenshot, plan-mode encouragement, etc. |
| 10 | 🔄 | **Fallback Chain** | Configure provider fallback order — Agent! retries with the next provider when one fails |
| 11 | 🔲 | **HUD** | Toggle the green-CRT scanline overlay on the LLM Output view |
| 12 | 📊 | **LLM Usage** | Per-model token usage and cost tracking. Green when there's recorded usage |
| 13 | ↩️ | **Rollback** | Time-Machine-style file backup browser. Restore any previous version of any file Agent! edited |
| 14 | 🕐 | **History** | Past prompts, errors, and task summaries for the active tab. Re-run a previous prompt with one click |
| 15 | 🗑️ | **Clear Log** | Delete the activity log for the active tab (or all task history when no tab is selected). Confirms first |

---

### 🎙 Voice Control — "Agent!" Hotword
**Hotword-anchored dictation via `SFSpeechRecognizer`.** Click the microphone in the input bar to start the hotword session, then say **"Agent!"** followed by your task. Transcription is on-device, runs in real time, and listens for `agent` as a complete word (not as a substring of "intelligent" or "management"). Anything you say after the wake word becomes the task — after ~2.5 seconds of silence, it auto-runs. The session loops automatically: when one task completes, it starts listening again. Click the mic to stop.

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

### Defense Layers

| Layer | What it does |
|---|---|
| **Shell Safety Service** | Hard-blocks catastrophic commands (`rm -rf /`, `rm -rf ~`, `dd` to `/dev/disk`, fork bombs, `--no-preserve-root`) before the Process is even constructed. Cannot be bypassed by the LLM. |
| **TCC In-Process Routing** | 17-keyword detector routes AppleScript, osascript, JXA, screencapture, accessibility, Shortcuts, and ScriptingBridge commands to run in-process where Agent! holds TCC grants — never through the Launch Agent/Daemon (separate bundle IDs = no TCC). |
| **File Backup on Every Edit** | `FileBackupService` auto-snapshots every file before `write_file`, `edit_file`, and `diff_apply`. Recoverable via `file(action:"restore")` or the Rollback UI. 1-week TTL. |
| **Agent Script .Trash** | `delete_agent` copies the script to `~/Documents/AgentScript/agents/.Trash/` before removal. Recoverable via `agent_script(action:"restore")`. |
| **Working Directory Normalization** | Every shell execution path (`executeTCC`, `UserService`, `HelperService`) normalizes the working directory — if a file path is accidentally passed as cwd, it strips to the parent directory instead of crashing with "Not a directory". |
| **Task Drain-Before-Start** | Starting a new task awaits the previous task's full termination before beginning — prevents orphaned retry loops from mixing log output across providers. |
| **Fallback Chain** | When the primary LLM fails (429, timeout, network), Agent! auto-switches to the next provider in the user-configured chain after 2 failures. |
| **Actionable Errors** | Every tool error includes a `Recovery:` hint telling the LLM exactly what to try next — no dead-end error messages that waste turns. |
| **Read Cache Invalidation** | File read cache is invalidated on both successful edits AND failed edits, so the LLM always gets fresh content on the next read. |
| **Basename Search** | When `read_file` or `edit_file` gets a wrong path, Agent! searches nearby directories for files with the same name and returns the correct paths inline — the LLM self-corrects in one turn. |

---

## Keyboard Shortcuts

Source of truth: the TextField `.onSubmit` in `Agent/Views/InputSectionView.swift` for `Return`, and the inline `NSEvent.addLocalMonitorForEvents` block in `Agent/Views/ContentView.swift` for everything else.

| Shortcut | Action |
|---|---|
| `Return` | Run current task (TextField submit — no modifier needed) |
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

## Slash Commands

Type these in the input field and press Return — they execute locally without going to any LLM. Source of truth: `AgentViewModel+RunStop.swift`.

| Command | Action |
|---|---|
| `/clear` or `/clear log` | Clear the activity log for the current tab |
| `/clear all` | Clear everything (log, LLM output, prompt history, task history, tokens) |
| `/clear llm` | Clear the LLM output panel only |
| `/clear history` | Clear prompt history |
| `/clear tasks` | Clear task history |
| `/clear tokens` | Reset token counters (task + session) |
| `/memory` or `/memory show` | Print the current memory file contents to the activity log |
| `/memory clear` | Wipe memory |
| `/memory edit` | Open `~/Documents/AgentScript/memory.md` in the system default editor |
| `/memory <text>` | Append `<text>` to memory (anything else after `/memory` becomes the new line) |

---

## FAQ

**Do I need to know how to code?** No. Just type what you want in plain English.

**Is it safe?** Yes. Standard macOS automation, full activity logging, you approve permissions.

**How much does it cost?** The Agent! app itself is free (MIT License). Cloud AI providers charge for API usage — the cheapest options for serious work are GLM-5/5.1 via Z.ai, BigModel, or Hugging Face (pennies per million tokens), or DeepSeek for budget coding. Self-hosted local models (Ollama, vLLM, LM Studio) have no API fees but only make sense if you already own the hardware to run them — see the hardware note below.

**What Mac do I need?** macOS 26.4+. Apple Silicon required. For cloud providers, any modern Mac works fine. For self-hosted local models (Ollama, vLLM, LM Studio): a 7B model fits in 16GB unified memory, a 13B model in 24GB, a 30B model needs 64GB+ (M2/M3/M4 Ultra Mac Studio territory). Apple Intelligence (the on-device mediator for triage / accessibility intent / token compression) needs an Apple Silicon Mac with Apple Intelligence enabled in System Settings.

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
| **xcode get_version** | Read the current marketing version and build number from the Xcode project |
| **xcode bump_version** | Bump the marketing version (major, minor, or patch), update the build number, build to verify, and auto-commit |
| **xcode bump_build** | Increment only the build number |

Just say *"bump version"* and Agent! reads the current version, asks major/minor/patch, updates Info.plist and project settings, builds to verify, and commits the change. No manual plist editing, no missed build numbers.

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

### **Agent! for macOS 26.4+ - Agentic AI for your  Mac Desktop**
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
| **Platform** | CLI — Linux, macOS, Windows | Native macOS 26.4+ only |
| **Runtime** | Node.js/Bun | Native compiled binary |
| **Architecture** | Terminal REPL with streaming | Desktop app with XPC daemons |
| **Accessibility** | None (CLI) | Full macOS AX via AXorcist (25 top-level actions, 30+ AX subtypes via `perform_action`) |
| **AppleScript** | None | Full NSAppleScript + JXA in-process with TCC |
| **Xcode Integration** | Via Bash (`xcodebuild`) | Native (build/run/analyze/snippet/add_file/bump_version/code_review — 13 actions) |
| **Apple Intelligence** | None | FoundationModels on-device — runs as a real `Tool`-calling agent for accessibility intent (e.g. *"take a photo using Photo Booth"* parsed and dispatched locally), task summaries, error explanations, and Tier 1 token compression. Falls through to the cloud LLM only on failure |
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
| **Context compaction** | Cloud Claude (paid tokens; conversation re-sent to Anthropic) | Tiered: Tier 1 = on-device Apple Intelligence summarization (free, private, no API tokens). Tier 2 = aggressive prune if Apple AI unavailable. Triggers at 30K est. tokens, summaries memoized, 3-failure circuit breaker |
