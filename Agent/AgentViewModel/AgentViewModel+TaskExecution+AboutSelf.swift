//
//  AgentViewModel+TaskExecution+AboutSelf.swift
//  Agent
//
//  AboutSelf tool implementation - provides information about Agent's capabilities
//

import Foundation
import FoundationModels

// MARK: - AboutSelf Tool

extension AgentViewModel {

    /// Handles the about_self tool - returns information about Agent's capabilities
    func handleAboutSelfTool(name: String, input: [String: Any]) async -> String? {
        guard name == "about_self" else { return nil }

        let topic = input["topic"] as? String ?? "all"
        let detail = input["detail"] as? String ?? "standard"

        let detailPrefix = detail == "brief" ? "Brief" : detail == "detailed" ? "Detailed" : ""

        let aboutText: String

        switch topic.lowercased() {
        case "tools":
            aboutText = """
            \(detailPrefix) Agent! Tools Overview

            Agent! uses consolidated action-based tools for macOS automation:

            FILE & CODING (action-based):
            - file_manager (action: read, write, edit, list, search, create, apply, undo, diff_apply): All file operations
            - git (action: status, diff, log, commit, diff_patch, branch): Git version control

            XCODE (action-based):
            - xcode (action: build, run, list_projects, select_project, add_file, remove_file, analyze, snippet): Xcode project management

            AUTOMATION (action-based):
            - applescript_tool (action: execute, lookup_sdef, list, run, save, delete): AppleScript with full TCC
            - javascript_tool (action: execute, list, run, save, delete): JavaScript for Automation (JXA)
            - agent (action: list, read, create, update, run, delete, combine): Swift AgentScripts
            - accessibility (action: list_windows, click, type_text, press_key, find_element, etc.): UI automation

            WEB (action-based):
            - web (action: search, open, find, click, type, read_content, navigate, etc.): Browser automation

            SHELL & PLANNING:
            - user: Run shell commands via User Space Launch Agent (preferred)
            - sh: Run shell scripts in-process (fallback when launch agent unavailable)
            - root: Run as root via privileged daemon (for admin tasks only)
            - batch: Run multiple shell commands in one call
            - multi: Run multiple tool calls in one batch
            - plan (action: create, update, read, list, delete): Task planning

            CONVERSATION (action-based):
            - conversation (action: write, transform, fix, about): Text generation, formatting, corrections, self-description
            - send_message: Send content via iMessage, email, or SMS

            Use list_tools to see all available tools.
            """

        case "features":
            aboutText = """
            \(detailPrefix) Agent! Features

            CORE FEATURES:
            - Multi-provider LLM support (Claude, OpenAI, Ollama, Apple Intelligence)
            - Streaming output with real-time display
            - Task history with AI-powered summarization
            - Chat history management with persistence
            - Screenshot and image attachment support

            AUTOMATION FEATURES:
            - Full TCC permissions (Accessibility, Automation, Screen Recording)
            - ScriptingBridge integration for app control
            - MCP (Model Context Protocol) server support
            - Reusable AgentScripts for complex automation

            DEVELOPER FEATURES:
            - Xcode project building and running
            - Git integration for version control
            - Code editing with diff visualization
            - Swift script compilation and execution

            UI FEATURES:
            - Native macOS design with split-pane interface
            - Conversation history with task tracking
            - Tab-based workflow for multiple tasks
            - Keyboard shortcuts for efficiency

            PRIVACY:
            - All automation runs locally on your Mac
            - API keys stored securely in Keychain
            - No data collection or telemetry
            """

        case "scripting":
            aboutText = """
            \(detailPrefix) Agent! Scripting Guide

            SWIFT AGENTSCRIPTS:
            Agent! can compile and run Swift scripts with full TCC permissions.
            Scripts are stored in ~/Documents/AgentScript/agents/

            Script template:
            ```swift
            import Foundation

            @_cdecl("script_main")
            public func scriptMain() -> Int32 {
                // Your automation code here
                print("Hello from AgentScript!")
                return 0
            }
            ```

            Rules:
            - Use @_cdecl("script_main") and return Int32
            - No exit() calls or top-level code
            - Access arguments via AGENT_SCRIPT_ARGS environment variable
            - Or use JSON files: ~/Documents/AgentScript/json/{Name}_input.json

            APPLESCRIPT:
            - applescript_tool (action: execute): Run AppleScript directly
            - applescript_tool (action: save, run, list, delete): Manage saved scripts
            - applescript_tool (action: lookup_sdef): Read app scripting dictionaries

            JXA (JAVASCRIPT FOR AUTOMATION):
            - javascript_tool (action: execute): Run JavaScript directly
            - javascript_tool (action: save, run, list, delete): Manage saved scripts

            SCRIPTINGBRIDGE:
            - applescript_tool (action: lookup_sdef): Read app dictionaries
            """

        case "automation":
            aboutText = """
            \(detailPrefix) Agent! Automation Capabilities

            APP CONTROL:
            Agent! can control macOS apps using:
            - applescript_tool (action: execute): AppleScript with full TCC
            - javascript_tool (action: execute): JavaScript for Automation
            - agent (action: run): Swift AgentScripts with ScriptingBridge
            - accessibility (action: click, type_text, find_element, etc.): UI automation

            ACCESSIBILITY AUTOMATION:
            Use accessibility tool with actions:
            - find_element, get_children: Find UI elements by role, title, or value
            - click, type_text, press_key: Simulate user input
            - scroll_to_element, drag: Navigate and interact
            - highlight_element: Visual verification

            WEB AUTOMATION:
            Use web tool with actions:
            - open, navigate: Open URLs and navigate
            - find, click, type: Interact with web elements
            - read_content: Extract page content
            - execute_js: Run JavaScript in browser

            SHELL (LEAST PRIVILEGE):
            - sh: Shell commands via User Space Launch Agent (always try this first)
            - root: Privileged daemon — for admin tasks only only when user-space cannot do the job

            SECURITY:
            All automation inherits Agent!'s TCC permissions
            No additional permission prompts needed
            Always operate under least privilege — escalate only when necessary
            """

        case "coding":
            aboutText = """
            \(detailPrefix) Agent! Coding Assistance

            CODE OPERATIONS:
            - Read any text file with line numbers
            - Write new files or edit existing ones
            - Search files by content or pattern
            - Apply diffs for precise changes

            GIT WORKFLOW:
            - View status, diffs, and history
            - Stage and commit changes
            - Create and switch branches
            - Apply patches

            XCODE INTEGRATION:
            - xcode (action: build): Build projects
            - xcode (action: run): Run apps
            - xcode (action: list_projects, select_project): Manage open projects
            - xcode (action: analyze, snippet): Code review and inspection

            BEST PRACTICES:
            Agent! prefers native tools over shell commands
            Use file_manager for file operations, git for version control
            Build Xcode projects with xcode (action: build)
            """

        default: // "all"
            let aiInsight = await generateAppleAIInsight()
            aboutText = """
            \(detailPrefix) Agent! for macOS 26

            I'm Agent! — an open-source autonomous AI that lives on your Mac.
            I act autonomously under least privilege. I use my User Space Launch Agent
            for shell commands, Accessibility API for UI automation, AppleScript and
            ScriptingBridge for app control, and Swift AgentScripts for complex workflows.
            Root access via the privileged Launch Daemon is a last resort — only for
            operations that truly require it.

            USER SPACE LAUNCH AGENT (primary):
            - Shell commands (find, grep, git, build tools) run as the current user
            - No privilege escalation needed for everyday development tasks
            - Full TCC permissions inherited from Agent!

            AUTOMATION & SCRIPTING:
            - Accessibility API: click, type, navigate any app's UI elements
            - AppleScript: in-process NSAppleScript with full TCC permissions
            - JavaScript for Automation (JXA): scriptable apps via OSA
            - ScriptingBridge: native Swift bridges to 50+ macOS apps (Xcode, Safari, Finder, Mail, etc.)
            - AgentScripts: compiled Swift dylibs at ~/Documents/AgentScript/agents/

            CODE & BUILD:
            - Read, edit, and create files with automatic backup and undo
            - Build and run Xcode projects, fix compiler errors in a loop
            - Git version control — branches, commits, diffs, patches
            - Code analysis, refactoring, and snippet extraction

            WEB & RESEARCH:
            - Safari automation — search, click, fill forms, extract content
            - Web search via Tavily or Claude native search
            - MCP server integration for extended capabilities

            PRIVILEGED DAEMON (last resort):
            - Root-level commands only when user-space cannot do the job
            - System diagnostics, disk health, launchd service management

            HOW I CODE:
            - Work on 1 file at a time. Make 1 change at a time. Build. Commit. Repeat.
            - Break tasks into small bites — a few lines per change.
            - Use plan mode for multi-step work, coding mode to focus tools.
            - Don't re-read files already in context. Don't waste tokens.
            - Keep files under 250 lines. Split large files into focused extensions.
            - If a build fails, fix the error and rebuild — don't start over.
            - NEVER loop endlessly. If stuck after 3 attempts, stop and ask.

            MULTI-PROVIDER AI:
            Claude, OpenAI, Gemini, Grok, Mistral, Codestral, Mistral Vibe, DeepSeek, Hugging Face, Z.ai, BigModel, Qwen, Ollama, LM Studio, and Apple Intelligence — all with tool calling, streaming, and vision.

            RIGHT NOW:
            - Project: \(projectFolder.isEmpty ? "(none)" : projectFolder)
            - User: \(NSFullUserName())
            - System: macOS \(ProcessInfo.processInfo.operatingSystemVersionString)
            \(aiInsight.isEmpty ? "" : "\n            APPLE AI SAYS:\n            \(aiInsight)")

            Just tell me what you want done.
            """
        }

        return aboutText
    }

    /// Ask Apple Intelligence for a brief insight about what Agent! could help with right now.
    private func generateAppleAIInsight() async -> String {
        guard AppleIntelligenceMediator.isAvailable else { return "" }
        do {
            let session = LanguageModelSession()
            let prompt =
                "In one sentence, what's something creative or useful a Mac automation agent could do for the user right now? Be specific and practical. No filler."
            let response = try await session.respond(to: prompt)
            return response.content.trimmingCharacters(in: .whitespacesAndNewlines)
        } catch {
            return ""
        }
    }
}
