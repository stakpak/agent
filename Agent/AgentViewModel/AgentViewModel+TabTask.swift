
@preconcurrency import Foundation
import AgentTools
import AgentAudit
import AgentLLM
import AppKit
import AgentMCP
import AgentD1F


// MARK: - Tab Task Execution

extension AgentViewModel {

    /// Start an LLM task on a specific script tab.
    func runTabTask(tab: ScriptTab) {
        let task = tab.taskInput.trimmingCharacters(in: .whitespaces)
        guard !task.isEmpty else { return }

        // Handle /memory in tab context
        if task.lowercased().hasPrefix("/memory") {
            tab.taskInput = ""
            let arg = task.dropFirst(7).trimmingCharacters(in: .whitespaces)
            if arg.isEmpty || arg.lowercased() == "show" {
                let content = MemoryStore.shared.content
                tab.appendLog("📝 Memory:\n\(content.isEmpty ? "(empty)" : content)")
            } else if arg.lowercased() == "clear" {
                MemoryStore.shared.write("")
                tab.appendLog("📝 Memory cleared.")
            } else if arg.lowercased() == "edit" {
                let url = URL(fileURLWithPath: NSHomeDirectory() + "/Documents/AgentScript/memory.md")
                AppKit.NSWorkspace.shared.open(url)
                tab.appendLog("📝 Opened memory.md in editor.")
            } else {
                MemoryStore.shared.append(arg)
                tab.appendLog("📝 Added to memory: \(arg)")
            }
            tab.flush()
            return
        }

        // Handle /clear in tab context
        if task.lowercased() == "/clear" {
            tab.taskInput = ""
            tab.activityLog = ""
            tab.logBuffer = ""
            tab.logFlushTask?.cancel()
            tab.logFlushTask = nil
            tab.streamLineCount = 0
            persistScriptTabs()
            return
        }

        tab.addToHistory(task)
        tab.taskInput = ""

        // Queue if already running
        if tab.isLLMRunning {
            tab.taskQueue.append(task)
            tab.appendLog("📋 Queued (\(tab.taskQueue.count)): \(task)")
            tab.flush()
            return
        }

        startTabTask(tab: tab, prompt: task)
    }

    /// Start executing a task on a tab (not queued).
    private func startTabTask(tab: ScriptTab, prompt: String) {
        tab.currentTaskPrompt = prompt
        tab.runningLLMTask = Task {
            await executeTabTask(tab: tab, prompt: prompt)
            // When done, run next queued task
            if !tab.taskQueue.isEmpty && !tab.isCancelled {
                let next = tab.taskQueue.removeFirst()
                startTabTask(tab: tab, prompt: next)
            }
        }
    }

    /// Stop the LLM task running on a script tab and clear its queue.
    func stopTabTask(tab: ScriptTab) {
        let queueCount = tab.taskQueue.count
        tab.taskQueue.removeAll()
        tab.runningLLMTask?.cancel()
        tab.runningLLMTask = nil
        tab.isLLMRunning = false
        tab.isLLMThinking = false
        tab.currentTaskPrompt = ""
        tab.currentAppleAIPrompt = ""
        if queueCount > 0 {
            tab.appendLog("🚫 Cancelled. \(queueCount) queued task(s) cleared.")
        } else {
            tab.appendLog("🚫 Cancelled.")
        }
        tab.flush()
    }

    // MARK: - Tab Task Execution Loop

    func executeTabTask(tab: ScriptTab, prompt: String) async {
        tab.isLLMRunning = true
        tab.llmMessages = [] // Fresh conversation for each task
        // Auto-expand HUD for THIS tab's run start (not on tab switches)
        tab.thinkingExpanded = true
        tab.thinkingOutputExpanded = true
        tab.thinkingDismissed = false
        // Reset fallback chain so this run starts on the primary provider
        FallbackChainService.shared.reset()

        var commandsRun: [String] = []
        var completionSummary = ""
        var directCommandContext: String?

        // Clear LLM Output for new task — show blinking cursor
        tab.dripTask?.cancel(); tab.dripTask = nil
        tab.rawLLMOutput = ""
        tab.displayedLLMOutput = ""
        tab.dripDisplayIndex = 0

        tab.appendLog(AgentViewModel.newTaskMarker)
        tab.appendLog("👤 \(prompt)")
        tab.flush()

        // Triage: direct commands, Apple AI conversation, accessibility agent,
        // or pass through to LLM. The axDispatch closure routes Apple AI's
        // tool calls through the same executeNativeTool path the cloud LLM
        // uses. If Apple AI fails, is unavailable, or doesn't call the tool,
        // runAccessibilityAgent returns nil → triage returns .passThrough →
        // we fall through to the cloud LLM loop below.
        let mediator = AppleIntelligenceMediator.shared
        let triageResult = await mediator.triagePrompt(prompt) { [weak self] args in
            guard let self else { return "{\"success\":false,\"error\":\"agent deallocated\"}" }
            var input: [String: Any] = ["action": args.action]
            if let role = args.role { input["role"] = role }
            if let title = args.title { input["title"] = title }
            if let app = args.appBundleId { input["appBundleId"] = app }
            if let text = args.text { input["text"] = text }
            return await self.executeNativeTool("accessibility", input: input)
        }
        switch triageResult {
        case .directCommand(let cmd):
            if cmd.name == "run_agent" {
                // Parse "AgentName args" from cmd.argument
                let parts = cmd.argument.components(separatedBy: " ")
                let agentName = await Self.offMain { [ss = scriptService] in ss.resolveScriptName(parts.first ?? "") }
                let args = parts.count > 1 ? parts.dropFirst().joined(separator: " ") : ""
                // Always run directly — skip LLM. Args provided by user.
                if await Self.offMain({ [ss = scriptService] in ss.compileCommand(name: agentName) }) != nil {
                    let success = await runAgentDirect(name: agentName, arguments: args, switchToTab: false)
                    if success {
                        if tab.isMessagesTab, let handle = tab.replyHandle {
                            tab.replyHandle = nil
                            sendMessagesTabReply("Ran \(agentName)", handle: handle)
                        }
                        tab.isLLMRunning = false
                        tab.isLLMThinking = false
                        return
                    }
                    // Failed — fall through to LLM to handle
                    tab.appendLog("❌ Direct run failed — passing to LLM")
                    tab.flush()
                    break
                }
            }
            let output = await executeDirectCommand(cmd, tab: tab)
            tab.flush()

            // Web commands: show results and complete
            if cmd.name == "web_open" {
                tab.appendLog("✅ \(output)")
                tab.flush()
            }
            if cmd.name == "web_open_and_search" {
                // Show a preview of search results in the log
                if let data = output.data(using: .utf8),
                   let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                   let title = json["title"] as? String,
                   let url = json["url"] as? String
                {
                    tab.appendLog("✅ \(title)")
                    tab.appendLog("🔗 \(url)")
                    if let content = json["content"] as? String {
                        tab.appendLog(String(content.prefix(1000)))
                    }
                } else {
                    tab.appendLog("✅ Search complete. Results on screen.")
                }
                tab.flush()
            }
            // google_search with results: pass to LLM for formatting
            if cmd.name == "google_search" && output.contains("\"success\": true") {
                directCommandContext =
                    """
                    Format these Google search results for the user. \
                    Be concise — show the top results with titles, \
                    URLs, and brief descriptions:

                    \(output)
                    """
                break
            }

            completionSummary = "Executed \(cmd.name)"
            let formatter = DateFormatter()
            formatter.dateFormat = "HH:mm:ss"
            let time = formatter.string(from: Date())
            tab.tabTaskSummaries.append("[\(time)] \(prompt) → \(completionSummary)")
            history.add(
                TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: [cmd.name]),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
            tab.flush()
            if tab.isMessagesTab, let handle = tab.replyHandle {
                tab.replyHandle = nil
                sendMessagesTabReply(completionSummary, handle: handle)
            }
            tab.isLLMRunning = false
            tab.isLLMThinking = false
            return
        case .answered(let reply):
            // Show in LLM Output, not LogView
            tab.rawLLMOutput = reply
            tab.displayedLLMOutput = reply
            tab.dripDisplayIndex = reply.count
            tab.appendLog("✅ Completed: \(String(reply.prefix(200)))")
            tab.flush()
            completionSummary = String(reply.prefix(200))
            history.add(
                TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: []),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
            tab.flush()
            if tab.isMessagesTab, let handle = tab.replyHandle {
                tab.replyHandle = nil
                sendMessagesTabReply(completionSummary, handle: handle)
            }
            tab.isLLMRunning = false
            tab.isLLMThinking = false
            return
        case .accessibilityHandled(let summary):
            // Apple AI ran the accessibility tool itself (one or more times)
            // and produced a final summary. The tool calls already happened
            // through the axDispatch closure above — they went through the
            // same executeNativeTool path the cloud LLM uses, so they're
            // already logged in the activity log. The summary string is
            // what Apple AI said it accomplished.
            //
            // If Apple AI never called the tool, or any tool call failed,
            // runAccessibilityAgent returns nil → triage returns .passThrough
            // → we never reach this case and the cloud LLM takes over.
            tab.rawLLMOutput = summary
            tab.displayedLLMOutput = summary
            tab.dripDisplayIndex = summary.count
            tab.appendLog("🍎 \(summary)")
            tab.flush()
            completionSummary = String(summary.prefix(200))
            history.add(
                TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: ["accessibility (Apple AI)"]),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
            tab.flush()
            if tab.isMessagesTab, let handle = tab.replyHandle {
                tab.replyHandle = nil
                sendMessagesTabReply(completionSummary, handle: handle)
            }
            tab.isLLMRunning = false
            tab.isLLMThinking = false
            return
        case .passThrough:
            break
        }

        // Build tab context from the existing log (cap at 8K characters)
        let tabContext = String(tab.activityLog.suffix(8000))
        let tccNote: String
        let lowerName = tab.scriptName.lowercased()
        if lowerName == "osascript" {
            tccNote = """
            This is a TCC tab with full Automation, Accessibility, and Screen Recording permissions. \
            Commands here run in the Agent app process. Use this tab for osascript, AppleScript, \
            and any commands that need TCC grants. Use lookup_sdef to check an app's scripting dictionary \
            before writing osascript commands.
            """
        } else if lowerName == "screencapture" {
            tccNote = """
            This is a TCC tab for screen capture. Commands run in the Agent app process with \
            Screen Recording permission. Use screencapture or ax_screenshot here.
            """
        } else {
            tccNote = "Help them debug, modify, re-run scripts, or perform any follow-up actions."
        }
        // If a script is currently executing, put LLM in conversation-only mode
        let conversationNote: String
        if tab.isRunning && !tab.isMainTab {
            conversationNote = """
            IMPORTANT: A script is currently executing in this tab. You are in CONVERSATION MODE ONLY. \
            Do NOT use any tools — just respond with plain text and call task_complete. \
            Answer questions, discuss the output, or chat. The script handles all execution.
            """
        } else {
            conversationNote = ""
        }
        let tabHistoryContext = """

        \nYou are in a tab named "\(tab.scriptName)". The user can see the tab's output.
        \(tccNote)
        \(conversationNote)
        The tab's recent output is below for context:

        \(tabContext)
        """

        // Use tab's project folder if set, otherwise fall back to main project folder
        // Resolve to directory (strip filename if path points to a file like .xcodeproj)
        let rawFolder = tab.projectFolder.isEmpty ? self.projectFolder : tab.projectFolder
        let projectFolder = Self.resolvedWorkingDirectory(rawFolder)

        var (provider, modelId) = resolvedLLMConfig(for: tab)
        tab.appendLog("🧠 \(provider.displayName) / \(modelId)")
        tab.flush()

        let mt = maxTokens
        var claude: ClaudeService?
        var openAICompatible: OpenAICompatibleService?
        var ollama: OllamaService?
        var foundationModelService: FoundationModelService?

        // Build LLM service instances for the current provider/model. Called at task
        // start and again when the fallback chain swaps providers mid-task.
        func buildLLMServices() {
            if provider == .claude {
                claude = ClaudeService(
                    apiKey: apiKey,
                    model: modelId,
                    historyContext: tabHistoryContext,
                    projectFolder: projectFolder,
                    maxTokens: mt
                )
            } else if provider == .lmStudio && lmStudioProtocol == .anthropic {
                claude = ClaudeService(
                    apiKey: lmStudioAPIKey,
                    model: modelId,
                    historyContext: tabHistoryContext,
                    projectFolder: projectFolder,
                    baseURL: lmStudioEndpoint,
                    maxTokens: mt
                )
            } else {
                claude = nil
            }
            switch provider {
            case .claude, .ollama, .localOllama, .foundationModel:
                openAICompatible = nil
            case .lmStudio where lmStudioProtocol == .anthropic:
                openAICompatible = nil
            case .lmStudio:
                let key = lmStudioProtocol == .lmStudio ? "input" : "messages"
                openAICompatible = OpenAICompatibleService(
                    apiKey: apiKeyForProvider(provider), model: modelId,
                    baseURL: lmStudioEndpoint, historyContext: tabHistoryContext,
                    projectFolder: projectFolder, provider: provider,
                    messagesKey: key, maxTokens: mt
                )
            case .vLLM:
                openAICompatible = OpenAICompatibleService(
                    apiKey: apiKeyForProvider(provider), model: modelId,
                    baseURL: vLLMEndpoint, historyContext: tabHistoryContext,
                    projectFolder: projectFolder, provider: provider,
                    maxTokens: mt
                )
            default:
                let url = chatURLForProvider(provider)
                let vision = LLMRegistry.shared.provider(provider.rawValue)?.capabilities.contains(.vision) ?? false
                openAICompatible = url.isEmpty ? nil : OpenAICompatibleService(
                    apiKey: apiKeyForProvider(provider), model: modelId,
                    baseURL: url, supportsVision: vision || forceVision,
                    historyContext: tabHistoryContext, projectFolder: projectFolder,
                    provider: provider, maxTokens: mt
                )
            }
            switch provider {
            case .ollama:
                ollama = OllamaService(
                    apiKey: ollamaAPIKey, model: modelId,
                    endpoint: ollamaEndpoint,
                    supportsVision: selectedOllamaSupportsVision || Self.isVisionModel(modelId),
                    historyContext: tabHistoryContext, projectFolder: projectFolder,
                    provider: .ollama
                )
            case .localOllama:
                ollama = OllamaService(
                    apiKey: "", model: modelId, endpoint: localOllamaEndpoint,
                    supportsVision: selectedLocalOllamaSupportsVision || Self.isVisionModel(modelId),
                    historyContext: tabHistoryContext, projectFolder: projectFolder,
                    provider: .localOllama, contextSize: localOllamaContextSize
                )
            default:
                ollama = nil
            }
            foundationModelService = provider == .foundationModel
                ? FoundationModelService(historyContext: tabHistoryContext, projectFolder: projectFolder) : nil

            claude?.temperature = temperatureForProvider(.claude)
            ollama?.temperature = temperatureForProvider(provider)
            openAICompatible?.temperature = temperatureForProvider(provider)
        }
        buildLLMServices()

        // Build on existing conversation or start fresh
        var messages: [[String: Any]] = tab.llmMessages

        // Remove trailing assistant messages — Ollama requires the last message
        // to be user or tool role. Strip any assistant messages at the end
        // (orphaned tool calls or plain text from a previous session/restart).
        while let last = messages.last, last["role"] as? String == "assistant" {
            messages.removeLast()
        }

        let promptPrefix = Self.newTaskPrefix(projectFolder: projectFolder, prompt: prompt)

        // Inject direct command context if set
        if let context = directCommandContext {
            messages.append(["role": "user", "content": context])
            tab.appendLog("📄 Page results passed to LLM (\(context.count) chars)")
            tab.flush()
        }

        // Use tab's own attached images, fall back to global
        let tabImages = tab.attachedImagesBase64.isEmpty ? attachedImagesBase64 : tab.attachedImagesBase64
        AuditLog.log(
            .shell,
            "TabTask images: tab=\(tab.attachedImagesBase64.count) global=\(attachedImagesBase64.count) using=\(tabImages.count)"
        )
        if !tabImages.isEmpty {
            tab.appendLog("(\(tabImages.count) screenshot(s) attached, \(tabImages.map(\.count).reduce(0,+)) bytes)")
            tab.flush()
            var contentBlocks: [[String: Any]] = tabImages.map { base64 in
                [
                    "type": "image",
                    "source": [
                        "type": "base64",
                        "media_type": "image/png",
                        "data": base64
                    ] as [String: Any]
                ]
            }
            contentBlocks.append(["type": "text", "text": prompt])
            messages.append(["role": "user", "content": contentBlocks])
            tab.attachedImages.removeAll()
            tab.attachedImagesBase64.removeAll()
            attachedImages.removeAll()
            attachedImagesBase64.removeAll()
        } else {
            messages.append(["role": "user", "content": promptPrefix + prompt])
        }

        // No mode filtering — every user-enabled tool is sent on every turn.
        // ToolPreferencesService is the only tool filter.
        let activeGroups: Set<String>? = nil

        var iterations = 0
        var textOnlyCount = 0
        var timeoutRetryCount = 0
        let maxTimeoutRetries = maxRetries
        var recentToolCalls: [String] = [] // Track recent tool calls to detect loops
        var stuckFiles: [String: Int] = [:] // Edit failure count per file (for nudge)
        // Plan-mode enforcement state
        var filesEditedThisTask: Set<String> = []
        // Full system prompt + full tool descriptions on every turn — no condensed
        // prompt, no compactTools, no mode auto-switching. The LLM always sees the
        // complete context and the complete tool list (filtered only by the user's
        // UI toggles in ToolPreferencesService).

        while !Task.isCancelled {
            iterations += 1

            // Mode auto-switching removed: every user-enabled tool is available on
            // every turn. ToolPreferencesService UI toggles are the only filter.

            // Prune old messages every 4 iterations to save tokens
            if iterations > 1 && iterations % 4 == 0 && messages.count > 10 {
                Self.pruneMessages(&messages)
            }
            if iterations > 2 { Self.stripOldImages(&messages) }
            // Drop oldest messages after 25 iterations
            if iterations >= 25 && messages.count > 12 {
                let keep = 8
                let drop = messages.count - keep
                if drop > 1 { messages.removeSubrange(1..<(1 + drop)) }
            }

            do {
                tab.isLLMThinking = true
                // Only auto-show overlay on the FIRST iteration. Respect manual dismiss (Cmd+B).
                if iterations == 1 { tab.thinkingDismissed = false }
                let response: (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int)
                let streamStart = CFAbsoluteTimeGetCurrent()
                // Summarize old messages every 10 iterations
                if iterations > 0 && iterations % 10 == 0 {
                    await Self.summarizeOldMessages(&messages)
                }
                let sendMessages = iterations > 1 ? Self.compressMessages(messages) : messages
                if let claude {
                    response = try await claude.sendStreaming(messages: sendMessages, activeGroups: activeGroups) { [weak tab] delta in
                        Task { @MainActor in
                            tab?.isLLMThinking = false
                            tab?.appendStreamDelta(delta)
                        }
                    }

                    tab.flushStreamBuffer()
                } else if let openAICompatible {
                    let r = try await openAICompatible
                        .sendStreaming(messages: sendMessages, activeGroups: activeGroups) { [weak tab] delta in
                            Task { @MainActor in
                                tab?.isLLMThinking = false
                                tab?.appendStreamDelta(delta)
                            }
                        }
                    response = (r.content, r.stopReason, r.inputTokens, r.outputTokens)

                    tab.flushStreamBuffer()
                } else if let ollama {
                    let r = try await ollama.sendStreaming(messages: sendMessages, activeGroups: activeGroups) { [weak tab] delta in
                        Task { @MainActor in
                            tab?.isLLMThinking = false
                            tab?.appendStreamDelta(delta)
                        }
                    }
                    response = (r.content, r.stopReason, r.inputTokens, r.outputTokens)

                    tab.flushStreamBuffer()
                } else if let foundationModelService {
                    let r = try await foundationModelService.sendStreaming(messages: sendMessages) { [weak tab] delta in
                        Task { @MainActor in
                            tab?.isLLMThinking = false
                            tab?.appendStreamDelta(delta)
                        }
                    }
                    response = (r.content, r.stopReason, 0, 0)

                    tab.flushStreamBuffer()
                } else {
                    throw AgentError.noAPIKey
                }
                // Strip done/task_complete from LLM Output
                Self.stripCompletionText(&tab.rawLLMOutput)
                // Wait for drip to finish
                while tab.dripTask != nil {
                    try? await Task.sleep(for: .milliseconds(50))
                }
                if !tab.rawLLMOutput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    tab.displayedLLMOutput = tab.rawLLMOutput
                    tab.dripDisplayIndex = tab.rawLLMOutput.count
                }
                if !tab.rawLLMOutput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    try? await Task.sleep(for: .seconds(3))
                }
                // Track token usage — use reported counts or estimate from text (~4 chars/token)
                let inTok = response.inputTokens > 0 ? response.inputTokens : Self.estimateTokens(messages: messages)
                let outTok = response.outputTokens > 0 ? response.outputTokens : Self.estimateTokens(content: response.content)
                taskInputTokens += inTok
                taskOutputTokens += outTok
                sessionInputTokens += inTok
                sessionOutputTokens += outTok
                TokenUsageStore.shared.record(inputTokens: inTok, outputTokens: outTok)
                FallbackChainService.shared.recordSuccess()
                let streamElapsed = CFAbsoluteTimeGetCurrent() - streamStart
                tab.lastElapsed = streamElapsed
                tab.tabInputTokens += inTok
                tab.tabOutputTokens += outTok
                // Show timing in activity log so user can see what's slow
                tab.appendLog("🕐 LLM \(String(format: "%.1f", streamElapsed))s | stop: \(response.stopReason) | iter \(iterations)")
                tab.flush()
                tab.isLLMThinking = false
                timeoutRetryCount = 0 // Reset on successful response
                guard !Task.isCancelled else { break }

                var toolResults: [[String: Any]] = []
                var hasToolUse = false

                for block in response.content {
                    guard let type = block["type"] as? String else { continue }

                    if type == "text" {
                        // Text goes to LLM output only — streaming delta already shows it there
                    } else if type == "tool_use" {
                        hasToolUse = true
                        guard let toolId = block["id"] as? String,
                              let rawName = block["name"] as? String,
                              let rawInput = block["input"] as? [String: Any] else { continue }

                        // Expand consolidated CRUDL tools into legacy tool names
                        let (name, input) = Self.expandConsolidatedTool(name: rawName, input: rawInput)

                        commandsRun.append(name)

                        // Plans are encouraged but never required. Track edited files for
                        // task summary purposes. No mid-stream blocking — the LLM decides
                        // whether to plan up front.
                        let editTools: Set<String> = [
                            "write_file",
                            "edit_file",
                            "diff_apply",
                            "diff_and_apply",
                            "create_diff",
                            "apply_diff"
                        ]
                        if editTools.contains(name), let filePath = input["file_path"] as? String, !filePath.isEmpty {
                            filesEditedThisTask.insert(filePath)
                        }

                        // Loop detection — block only after 20 IDENTICAL read calls
                        // (same file_path AND same offset AND same limit). Different
                        // offset/limit on the same file does NOT count toward the limit;
                        // a write to anything resets the counter for the whole tab.
                        let isRead = name == "read_file" || (name == "file_manager" && (input["action"] as? String) == "read")
                        let isWrite = name == "write_file" || name == "edit_file"
                            || name == "create_diff" || name == "apply_diff"
                            || name == "diff_and_apply"
                            ||
                            (
                                name == "file_manager" && ["write", "edit", "diff_apply", "create", "apply"]
                                    .contains(input["action"] as? String ?? "")
                            )
                        if isWrite { recentToolCalls.removeAll() }
                        if isRead {
                            let fp = input["file_path"] as? String ?? input["path"] as? String ?? ""
                            let offset = input["offset"] as? Int ?? 0
                            let limit = input["limit"] as? Int ?? 0
                            let callKey = "\(name):\(fp):\(offset):\(limit)"
                            let dupeLimit = 20
                            let dupeCount = recentToolCalls.filter { $0 == callKey }.count
                            if dupeCount >= dupeLimit {
                                tab
                                    .appendLog(
                                        """
                                        ⚠️ Already read \((fp as NSString).lastPathComponent) \
                                        \(dupeLimit) times with the same offset/limit — skipping
                                        """
                                    )
                                tab.flush()
                                toolResults.append([
                                    "type": "tool_result",
                                    "tool_use_id": toolId,
                                    "content": """
                                        Error: You already read this file \
                                        \(dupeLimit) times with the SAME offset \
                                        and limit. The content has not changed. \
                                        Use the content you already have, or read \
                                        a DIFFERENT range of the file.
                                        """
                                ])
                                continue
                            }
                            recentToolCalls.append(callKey)
                        }

                        if name == "task_complete" {
                            completionSummary = input["summary"] as? String ?? "Done"
                            // Show task complete in the LLM Output HUD so the user sees the result.
                            // Append to rawLLMOutput and let the drip task pick up the new chars
                            // naturally — DO NOT sync displayedLLMOutput, that would skip the drip.
                            let trimmedRaw = tab.rawLLMOutput.trimmingCharacters(in: .whitespacesAndNewlines)
                            if trimmedRaw.isEmpty {
                                tab.rawLLMOutput = "✅ \(completionSummary)"
                            } else if !trimmedRaw.contains(completionSummary) {
                                tab.rawLLMOutput += "\n\n✅ \(completionSummary)"
                            }
                            tab.startDripIfNeeded()
                        }
                        let toolStart = CFAbsoluteTimeGetCurrent()
                        let result = await handleTabToolCall(
                            tab: tab, name: name, input: input, toolId: toolId
                        )
                        let toolElapsed = CFAbsoluteTimeGetCurrent() - toolStart
                        if toolElapsed > 0.5 {
                            tab.appendLog("🕐 \(name) \(String(format: "%.1f", toolElapsed))s")
                            tab.flush()
                        }
                        if result.isComplete {
                            tab.llmMessages = messages
                            // Save task history for tab
                            let formatter = DateFormatter()
                            formatter.dateFormat = "HH:mm:ss"
                            let time = formatter.string(from: Date())
                            tab.tabTaskSummaries.append("[\(time)] \(prompt) → \(completionSummary)")
                            history.add(
                                TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: commandsRun),
                                maxBeforeSummary: maxHistoryBeforeSummary, apiKey: apiKey,
                                model: selectedModel
                            )
                            tab.isLLMRunning = false
                            tab.isLLMThinking = false
                            return
                        }
                        if let toolResult = result.toolResult {
                            toolResults.append(toolResult)
                            // Stuck-file nudge: if this was an edit tool and the result
                            // looks like a failure, increment the per-file failure count.
                            // At 3 failures, append an actionable recovery nudge.
                            if editTools.contains(name),
                               let path = input["file_path"] as? String ?? input["path"] as? String,
                               let output = toolResult["content"] as? String
                            {
                                let lower = output.lowercased()
                                let isFailure = lower.hasPrefix("error") || lower.contains("error:") || lower.contains("failed") || lower
                                    .contains("not found") || lower.contains("rejected")
                                if isFailure {
                                    stuckFiles[path, default: 0] += 1
                                    if stuckFiles[path]! == 3 {
                                        let nudge = """
                                        ⚠️ 3 consecutive edit failures on \(path). STOP retrying the same approach.

                                        Recovery checklist (do these in order):
                                        1. read_file(file_path:"\(path)") \
                                        with NO offset/limit to get the FULL \
                                        fresh content
                                        2. Find the EXACT lines you want to \
                                        change in the new output. Do NOT trust \
                                        the tool_result from earlier reads — \
                                        the file may have been modified by your \
                                        previous edits.
                                        3. For edit_file: copy old_string verbatim \
                                        from the fresh read, including every space, \
                                        tab, and newline.
                                        4. For diff_and_apply: pass start_line and \
                                        end_line to scope the section.
                                        5. If you keep failing, switch tools — \
                                        write_file to overwrite the whole file is \
                                        a valid last resort.
                                        """
                                        toolResults.append(["type": "tool_result", "tool_use_id": "stuck_guard_3", "content": nudge])
                                        tab.appendLog("⚠️ Stuck nudge: 3 failures on \((path as NSString).lastPathComponent)")
                                        tab.flush()
                                    }
                                } else {
                                    stuckFiles[path] = 0
                                }
                            }
                        }
                    }
                }

                messages.append(["role": "assistant", "content": response.content])
                tab.llmMessages = messages

                if hasToolUse && !toolResults.isEmpty {
                    let capped = Self.truncateToolResults(toolResults)
                    messages.append(["role": "user", "content": capped])
                    tab.llmMessages = messages
                } else if !hasToolUse {
                    // Check if model wrote task_complete as text instead of a tool call
                    let responseText = response.content.compactMap { $0["text"] as? String }.joined()
                    if responseText.contains("task_complete") || responseText.contains("done(summary") {
                        // Extract summary from task_complete/done(summary: "...") or (summary="...")
                        if let match = responseText.range(
                            of: #"(?:task_complete|done)\(summary[=:]\s*"([^"]+)""#,
                            options: .regularExpression
                        ) {
                            let raw = String(responseText[match])
                            completionSummary = raw
                                .replacingOccurrences(
                                    of: #"(?:task_complete|done)\(summary[=:]\s*""#,
                                    with: "",
                                    options: .regularExpression
                                )
                                .replacingOccurrences(of: "\"", with: "")
                        } else {
                            completionSummary = String(responseText.prefix(500))
                        }
                        tab.appendLog("✅ Completed: \(completionSummary)")
                        tab.flush()
                        break
                    }
                    // LLM responded with text only — nudge it to continue or finish
                    textOnlyCount += 1
                    if textOnlyCount >= 3 {
                        if !responseText.isEmpty { completionSummary = String(responseText.prefix(500)) }
                        break
                    }
                    messages.append([
                        "role": "user",
                        "content": "Continue with the next step. When you are completely done, call task_complete(summary: \"...\")."
                    ])
                } else {
                    // Check if LLM signaled it's done via text even though it made tool calls
                    let allText = response.content.compactMap { $0["text"] as? String }.joined().lowercased()
                    let stopPhrases = ["no more content", "no further action", "task is complete", "nothing more to do", "task_complete"]
                    if stopPhrases.contains(where: { allText.contains($0) }) && completionSummary.isEmpty {
                        completionSummary = "Done"
                        break
                    }
                }

            } catch {
                if !Task.isCancelled {
                    let errMsg = error.localizedDescription

                    // Detect timeout errors
                    let isNetworkTimeout = errMsg.lowercased().contains("timeout") || errMsg.lowercased().contains("timed out")


                    // Determine error source for better logging
                    var errorSource = "Unknown"
                    if claude != nil {
                        errorSource = "Claude API"
                    } else if openAICompatible != nil {
                        errorSource = "\(provider.displayName) API"
                    } else if ollama != nil {
                        errorSource = "Ollama API"
                    } else if foundationModelService != nil {
                        errorSource = "Apple Intelligence"
                    }


                    // Handle timeout errors with retry logic
                    if isNetworkTimeout {
                        // Check if we've already retried this timeout
                        if timeoutRetryCount < maxTimeoutRetries {
                            timeoutRetryCount += 1

                            // Special handling for Ollama timeouts - check server health
                            if errorSource == "Ollama API" || errorSource == "Local Ollama" {
                                tab.appendLog("🔍 Checking Ollama server health...")

                                // Run Ollama health check in background
                                let healthCheckResult = await Self.offMain {
                                    let healthCheckTask = Process()
                                    healthCheckTask.executableURL = URL(fileURLWithPath: "/usr/bin/curl")
                                    healthCheckTask.arguments = ["-s", "-f", "http://localhost:11434/api/tags", "--max-time", "5"]
                                    healthCheckTask.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())

                                    let pipe = Pipe()
                                    healthCheckTask.standardOutput = pipe
                                    healthCheckTask.standardError = pipe

                                    do {
                                        try healthCheckTask.run()
                                        healthCheckTask.waitUntilExit()
                                        return healthCheckTask.terminationStatus
                                    } catch {
                                        return -1
                                    }
                                }

                                if healthCheckResult != 0 {
                                    tab.appendLog("⚠️ Ollama server not responding. Attempting to restart...")

                                    // Restart Ollama via UserService XPC
                                    _ = await userService
                                        .execute(command: "pkill -f 'ollama serve' && sleep 2 && open /Applications/Ollama.app")
                                    tab.appendLog("🔄 Restart command executed")

                                    // Wait longer for Ollama startup
                                    let startupDelay = TimeInterval(min(10 * timeoutRetryCount, 30)) // Exponential backoff up to 30 seconds
                                    let retryMessage =
                                        """
                                        \(errorSource) timeout detected \
                                        (attempt \(timeoutRetryCount)/\(maxTimeoutRetries)) — \
                                        Ollama restart attempted, \
                                        waiting \(Int(startupDelay)) seconds...
                                        """
                                    tab.appendLog(retryMessage)

                                    try? await Task.sleep(for: .seconds(startupDelay))
                                    if Task.isCancelled { break }
                                    continue
                                } else {
                                    tab.appendLog("✅ Ollama server is running but API timed out")
                                }
                            }

                            let retryDelay = TimeInterval(min(10 * timeoutRetryCount, 30)) // Exponential backoff up to 30 seconds
                            let retryMessage =
                                """
                                \(errorSource) timeout detected \
                                (attempt \(timeoutRetryCount)/\(maxTimeoutRetries)) — \
                                retrying in \(Int(retryDelay)) seconds...
                                """
                            tab.appendLog(retryMessage)

                            // Log to task log for debugging

                            try? await Task.sleep(for: .seconds(retryDelay))
                            if Task.isCancelled { break }
                            continue
                        } else {
                            // Max retries reached - try final Ollama restart if applicable
                            if (errorSource == "Ollama API" || errorSource == "Local Ollama") && timeoutRetryCount == maxTimeoutRetries {
                                tab.appendLog("🔄 Max retries reached. Attempting final Ollama restart...")

                                // Restart Ollama via UserService XPC
                                _ = await userService
                                    .execute(command: "pkill -f 'ollama serve' && sleep 3 && open /Applications/Ollama.app && sleep 10")
                                tab.appendLog("🔄 Ollama restart attempted. Check Ollama status.")
                            }

                            let timeoutMessage =
                                """
                                \(errorSource) timeout after \(maxTimeoutRetries) \
                                retries. Please check your network connection \
                                or try a different LLM provider.
                                """
                            tab.appendLog(timeoutMessage)
                            break
                        }
                    } else if let agentErr = error as? AgentError, agentErr.isRateLimited {
                        // Rate limit — retry once after 30s, then stop
                        if timeoutRetryCount < 1 {
                            timeoutRetryCount += 1
                            tab.appendLog("\(errorSource) rate limited — waiting 30s before retry...")
                            tab.flush()
                            try? await Task.sleep(for: .seconds(30))
                            if Task.isCancelled { break }
                            continue
                        } else {
                            tab.appendLog("\(errorSource) rate limited. Wait a minute and try again.")
                            tab.flush()
                            break
                        }
                    } else if let agentErr = error as? AgentError, agentErr.isRecoverable, timeoutRetryCount < maxTimeoutRetries {
                        // Server/network error — retry every 10 seconds
                        timeoutRetryCount += 1
                        let retryDelay: TimeInterval = 10
                        tab
                            .appendLog(
                                """
                                \(errorSource) recoverable error \
                                (attempt \(timeoutRetryCount)/\(maxTimeoutRetries)) — \
                                retrying in \(Int(retryDelay))s...
                                """
                            )
                        tab.flush()
                        try? await Task.sleep(for: .seconds(retryDelay))
                        if Task.isCancelled { break }
                        continue
                    } else if errMsg.lowercased().contains("network")
                        || errMsg.lowercased().contains("connection")
                        || errMsg.lowercased().contains("internet")
                        || (error as? URLError)?.code == .networkConnectionLost
                        || (error as? URLError)?.code == .notConnectedToInternet
                    {
                        // Network lost — retry in 60 seconds
                        timeoutRetryCount += 1
                        if timeoutRetryCount <= maxTimeoutRetries {
                            let delay = networkRetryDelay
                            tab
                                .appendLog(
                                    """
                                    🌐 Network connection lost — \
                                    retrying in \(delay)s \
                                    (attempt \(timeoutRetryCount)/\(maxTimeoutRetries))...
                                    """
                                )
                            tab.flush()
                            try? await Task.sleep(for: .seconds(Double(delay)))
                            if Task.isCancelled { break }
                            continue
                        } else {
                            tab.appendLog("🌐 Network connection lost after \(maxTimeoutRetries) retries. Check your connection.")
                            tab.flush()
                            break
                        }
                    } else {
                        // Try fallback chain before giving up
                        if let fallback = FallbackChainService.shared.recordFailure() {
                            tab.appendLog("🔄 Switching to fallback: \(fallback.displayName)")
                            tab.flush()
                            if let fbProvider = APIProvider(rawValue: fallback.provider) {
                                provider = fbProvider
                                modelId = fallback.model
                                buildLLMServices()
                                tab.appendLog("✅ Now using \(provider.displayName) / \(modelId)")
                                tab.flush()
                            }
                            timeoutRetryCount = 0
                            continue
                        }
                        // Non-recoverable error — don't retry (400 bad request, auth errors, etc.)
                        tab.appendLog("\(errorSource) Error: \(errMsg)")
                        tab.flush()

                        if mediator.isEnabled && mediator.showAnnotationsToUser {
                            if let errorAnnotation = await mediator.explainError(toolName: "LLM request", error: errMsg) {
                                tab.appendLog(errorAnnotation.formatted)
                                tab.flush()
                            }
                        }
                        break
                    }
                }
                continue
            }
        }


        // Save task history if task didn't call task_complete
        if completionSummary.isEmpty {
            let summary = Task.isCancelled ? "(cancelled)" : commandsRun.isEmpty ? "(no actions)" : "(incomplete)"
            let formatter = DateFormatter()
            formatter.dateFormat = "HH:mm:ss"
            let time = formatter.string(from: Date())
            tab.tabTaskSummaries.append("[\(time)] \(prompt) → \(summary)")
            history.add(
                TaskRecord(prompt: prompt, summary: summary, commandsRun: commandsRun),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
        }

        // If Messages tab task ended without task_complete, still send a reply
        if tab.isMessagesTab, let handle = tab.replyHandle {
            tab.replyHandle = nil
            let reply = completionSummary.isEmpty
                ? (Task.isCancelled ? "(cancelled)" : "Done")
                : completionSummary
            sendMessagesTabReply(reply, handle: handle)
        }

        tab.flush()
        tab.isLLMRunning = false
        tab.isLLMThinking = false
    }

    // MARK: - Tab Tool Call Handler

    struct TabToolResult {
        let toolResult: [String: Any]?
        let isComplete: Bool
    }

    /// Dispatch tab tool calls — handler bodies in AgentViewModel+TabToolHandlers.swift
    func handleTabToolCall(
        tab: ScriptTab, name: String, input: [String: Any], toolId: String
    ) async -> TabToolResult {
        await handleTabToolCallBody(tab: tab, name: name, input: input, toolId: toolId)
    }

    // MARK: - Tab Command Execution

    /// Execute a command via UserService with cd prefix to ensure correct directory.
    /// Falls back to in-process execution when working directory is TCC-protected.
    func executeForTab(command: String, projectFolder pf: String = "") async -> (status: Int32, output: String) {
        // Fallback chain: passed projectFolder → self.projectFolder → home (handled by UserService)
        let folder = pf.isEmpty ? self.projectFolder : pf
        let dir = folder.isEmpty ? "" : Self.resolvedWorkingDirectory(folder)
        let fullCommand = Self.prependWorkingDirectory(command, projectFolder: dir)
        // TCC-protected folders must run in-process
        if Self.isTCCProtectedPath(dir) || Self.needsTCCPermissions(command) {
            return await Self.executeTCC(command: fullCommand)
        }
        return await userService.execute(command: fullCommand, workingDirectory: dir)
    }
}
