[< Back to README](../README.md)

# Agent! -- How It Compares

## Feature Comparison

| Capability | Agent! | Claude Code | Cursor | Cline | OpenClaw |
|------------|:-------|:------------|:------|:-----|:---------|
| **Native macOS App** | SwiftUI | Terminal | VS Code | VS Code | Electron |
| **Xcode Build/Run** | Full project | File edits | Via Sweetpad | No | No |
| **AgentScript (Swift)** | Compiled dylibs | No | No | No | No |
| **AppleScript/JXA** | Built-in | Via MCP* | No | No | No |
| **Accessibility API** | Full control | No | No | No | No |
| **MCP Protocol** | Stdio + SSE | Stdio + SSE | Stdio + SSE | Stdio | Sandbox* |
| **Multi-LLM** | 10 providers | Claude only | Multiple | Multiple | Claude + Local |
| **Local Models** | Ollama, vLLM, LM Studio | No | Via OpenRouter* | Ollama, LM Studio | Ollama, LM Studio |
| **Apple Intelligence** | Autocomplete, summaries | No | No | No | No |
| **iMessage Remote** | Built-in | Via Channels* | No | No | Via MoltBot* |
| **Root Operations** | XPC daemon | No | No | No | Docker sandbox |
| **Open Source** | MIT License | Partial* | No | Apache 2.0 | MIT License |

**Notes:**
- *Claude Code AppleScript: Available via community MCP servers, not built-in
- *Claude Code Channels: Telegram/Discord messaging added Jan 2026, iMessage via Dispatch
- *Claude Code Open Source: Some components open source (MCP SDK, CLI tools), core proprietary
- *Cursor Local: Via OpenRouter or API passthrough, not direct local model support
- *Cline: Open-source VS Code extension with local model support
- *OpenClaw: Open source with permissive license, Docker sandbox for isolation, MoltBot for multi-platform messaging

---

## Agent! vs Claude Code

| Aspect | Agent! | Claude Code |
|--------|--------|-------------|
| **Interface** | Native macOS SwiftUI app | Terminal-based CLI |
| **Platform Focus** | macOS-first | Cross-platform |
| **Xcode Integration** | Build, run, manage projects | File edits only |
| **App Automation** | Full ScriptingBridge support | Via MCP servers (add-on) |
| **System Access** | Accessibility API, root daemon | Sandboxed terminal |
| **LLM Choice** | 10 providers | Claude only (Claude API) |
| **Local Models** | Ollama, LM Studio, vLLM | No |
| **Scripting** | Swift, AppleScript, JXA | None native |
| **MCP** | Stdio + HTTP/SSE transports | HTTP/SSE + Stdio |
| **Remote Control** | Built-in iMessage | Claude Dispatch (separate app) |
| **Open Source** | MIT License | Proprietary (Commercial Terms of Service) |

**Verdict**: Claude Code is excellent for terminal-based cross-platform development. Agent! is superior for macOS-specific workflows, Xcode projects, and deep system automation.

---

## Agent! vs Cursor

| Aspect | Agent! | Cursor |
|--------|--------|--------|
| **Technology** | Native Swift | Electron (VS Code fork) |
| **Performance** | Native speed, low memory | Chromium overhead (~150MB+ idle) |
| **macOS Integration** | Deep system integration | Limited to file operations |
| **Xcode Support** | Full project management | Basic file editing, simulator via Sweetpad |
| **LLM Choice** | 10 providers | OpenAI, DeepSeek, Claude, Gemini |
| **Scripting** | Swift, AppleScript, JXA | None |
| **System Automation** | Accessibility, root operations | None |
| **Privacy** | Local processing options | Cloud-only by default |
| **MCP Support** | Stdio + HTTP/SSE | Stdio + HTTP/SSE |
| **Open Source** | MIT License | Proprietary |

**Verdict**: Cursor is a VS Code fork with AI features and multi-LLM support. Agent! is a purpose-built macOS app that deeply integrates with the system. Choose Cursor if you need VS Code; choose Agent! if you need macOS automation and Xcode integration.

---

## Agent! vs Cline

| Aspect | Agent! | Cline |
|--------|--------|-------|
| **Interface** | Native macOS SwiftUI app | VS Code extension |
| **Platform Focus** | macOS-first | Cross-platform (VS Code) |
| **Xcode Integration** | Build, run, manage projects | File edits only |
| **App Automation** | Full ScriptingBridge support | None |
| **System Access** | Accessibility API, root daemon | Terminal commands only |
| **LLM Choice** | 10 providers | Multiple (Claude, OpenAI, DeepSeek, local) |
| **Local Models** | Ollama, LM Studio, vLLM | Ollama, LM Studio |
| **Scripting** | Swift, AppleScript, JXA | None |
| **MCP Support** | Stdio + HTTP/SSE | Stdio |
| **Open Source** | MIT License | Apache 2.0 |

**Verdict**: Cline is an open-source VS Code extension with strong local model support. Agent! provides native macOS integration with ScriptingBridge, Xcode project management, and Accessibility API control. Choose Cline for VS Code workflows; choose Agent! for deep macOS automation.

---

## Agent! vs OpenClaw

| Aspect | Agent! | OpenClaw |
|--------|--------|----------|
| **Interface** | Native SwiftUI app | Electron desktop app |
| **Architecture** | Native macOS app | Electron with Docker sandbox |
| **Xcode Integration** | Build, run, projects | File edits only |
| **App Automation** | ScriptingBridge for 50+ apps | None (sandboxed) |
| **LLM Choice** | 10 providers | Claude API + Local LLMs (Ollama, LM Studio) |
| **Local Models** | Ollama, LM Studio, vLLM | Full support |
| **Scripting** | Swift, AppleScript, JXA | None |
| **MCP** | Full Stdio + HTTP/SSE | Basic support, sandboxed |
| **Messages** | Built-in iMessage remote | Via MoltBot (WhatsApp, Telegram, Discord, Slack, iMessage) |
| **System Access** | Full access with TCC | Sandboxed Docker containers |
| **Privacy** | Local processing available | Local-first design |
| **Open Source** | MIT License | MIT License |

**Verdict**: OpenClaw excels at privacy-focused automation with local LLM support and sandboxed security. Agent! provides deeper macOS integration with native ScriptingBridge, Xcode project management, and Accessibility API control. Choose OpenClaw for privacy-first workflows; choose Agent! for deep macOS automation and Xcode development.
