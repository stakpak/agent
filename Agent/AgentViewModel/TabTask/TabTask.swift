
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
        let typed = tab.taskInput.trimmingCharacters(in: .whitespaces)
        // Merge long-text attachments (captured via Cmd+V chips) into the prompt.
        let task = Self.mergePastedTexts(tab.pastedTexts, into: typed)
        tab.pastedTexts.removeAll()
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

        // Clear LLM Output for new task — show blinking cursor
        tab.dripTask?.cancel(); tab.dripTask = nil
        tab.rawLLMOutput = ""
        tab.displayedLLMOutput = ""
        tab.dripDisplayIndex = 0

        tab.appendLog(AgentViewModel.newTaskMarker)
        tab.appendLog("👤 \(prompt)")
        tab.flush()

        // Triage: direct commands, Apple AI, accessibility, or pass through.
        var directCommandContext: String?
        let triageOutcome = await runTabTaskTriage(
            tab: tab, prompt: prompt, completionSummary: &completionSummary
        )
        switch triageOutcome {
        case .done:
            return
        case .passThrough:
            break
        case .llmWithContext(let ctx):
            directCommandContext = ctx
        }

        let tabHistoryContext = buildTabHistoryContext(tab: tab)

        // Use tab's project folder if set, otherwise fall back to main project folder
        // Resolve to directory (strip filename if path points to a file like .xcodeproj)
        let rawFolder = tab.projectFolder.isEmpty ? self.projectFolder : tab.projectFolder
        let projectFolder = Self.resolvedWorkingDirectory(rawFolder)

        var (provider, modelId) = resolvedLLMConfig(for: tab)
        tab.appendLog("🧠 \(provider.displayName) / \(modelId)")
        tab.flush()

        let mt = maxTokens
        var services = buildTabLLMServices(
            provider: provider,
            modelId: modelId,
            historyContext: tabHistoryContext,
            projectFolder: projectFolder,
            maxTokens: mt
        )

        var messages = buildTabInitialMessages(
            tab: tab,
            prompt: prompt,
            projectFolder: projectFolder,
            directCommandContext: directCommandContext
        )

        // No mode filtering — every user-enabled tool is sent on every turn.
        // ToolPreferencesService is the only tool filter.
        let activeGroups: Set<String>? = nil

        var iterations = 0
        var textOnlyCount = 0
        var timeoutRetryCount = 0
        var stuckFiles: [String: Int] = [:] // Edit failure count per file (for nudge)
        // Plan-mode enforcement state
        var filesEditedThisTask: Set<String> = []
        // Full system prompt + full tool descriptions on every turn — no condensed prompt, no compactTools, no mode
        // auto-switching. The LLM always sees the complete context and the complete tool list (filtered only by the user's UI toggles in ToolPreferencesService).

        mainLoop: while !Task.isCancelled {
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
                if let claude = services.claude {
                    response = try await claude.sendStreaming(messages: sendMessages, activeGroups: activeGroups) { [weak tab] delta in
                        Task { @MainActor in
                            tab?.isLLMThinking = false
                            tab?.appendStreamDelta(delta)
                        }
                    }

                    tab.flushStreamBuffer()
                } else if let openAICompatible = services.openAICompatible {
                    let r = try await openAICompatible
                        .sendStreaming(messages: sendMessages, activeGroups: activeGroups) { [weak tab] delta in
                            Task { @MainActor in
                                tab?.isLLMThinking = false
                                tab?.appendStreamDelta(delta)
                            }
                        }
                    response = (r.content, r.stopReason, r.inputTokens, r.outputTokens)

                    tab.flushStreamBuffer()
                } else if let ollama = services.ollama {
                    let r = try await ollama.sendStreaming(messages: sendMessages, activeGroups: activeGroups) { [weak tab] delta in
                        Task { @MainActor in
                            tab?.isLLMThinking = false
                            tab?.appendStreamDelta(delta)
                        }
                    }
                    response = (r.content, r.stopReason, r.inputTokens, r.outputTokens)

                    tab.flushStreamBuffer()
                } else if let foundationModelService = services.foundationModel {
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
                TokenUsageStore.shared.recordModelUsage(
                    model: modelId, input: inTok, output: outTok, provider: provider.displayName,
                    tabId: tab.id, tabLabel: tab.displayTitle
                )
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

                // Process the response's tool_use blocks (extracted helper).
                let outcome = await processTabResponseContent(
                    tab: tab,
                    content: response.content,
                    commandsRun: &commandsRun,
                    stuckFiles: &stuckFiles,
                    filesEditedThisTask: &filesEditedThisTask,
                    completionSummary: &completionSummary
                )

                switch outcome {
                case .complete(let summary):
                    completionSummary = summary
                    tab.llmMessages = messages
                    // Save task history for tab
                    let formatter = DateFormatter()
                    formatter.dateFormat = "HH:mm:ss"
                    let time = formatter.string(from: Date())
                    tab.tabTaskSummaries.append("[\(time)] \(prompt) → \(completionSummary)")
                    history.add(
                        TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: commandsRun),
                        maxBeforeSummary: maxHistoryBeforeSummary, apiKey: apiKey,
                        model: modelId
                    )
                    tab.isLLMRunning = false
                    tab.isLLMThinking = false
                    return
                case .normal(let hasToolUse, let toolResults):
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
                            break mainLoop
                        }
                        // LLM responded with text only — nudge it to continue or finish
                        textOnlyCount += 1
                        if textOnlyCount >= 3 {
                            if !responseText.isEmpty { completionSummary = String(responseText.prefix(500)) }
                            break mainLoop
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
                            break mainLoop
                        }
                    }
                }

            } catch {
                let action = await handleTabTaskError(
                    tab: tab,
                    error: error,
                    hasClaude: services.claude != nil,
                    hasOpenAICompatible: services.openAICompatible != nil,
                    hasOllama: services.ollama != nil,
                    hasFoundationModel: services.foundationModel != nil,
                    provider: provider,
                    timeoutRetryCount: &timeoutRetryCount
                )
                switch action {
                case .retry:
                    continue
                case .giveUp:
                    break mainLoop
                case .fallback(let fbProvider, let fbModel):
                    if let fbProvider, let fbModel {
                        provider = fbProvider
                        modelId = fbModel
                        services = buildTabLLMServices(
                            provider: provider,
                            modelId: modelId,
                            historyContext: tabHistoryContext,
                            projectFolder: projectFolder,
                            maxTokens: mt
                        )
                        tab.appendLog("✅ Now using \(provider.displayName) / \(modelId)")
                        tab.flush()
                    }
                    timeoutRetryCount = 0
                    continue
                }
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
