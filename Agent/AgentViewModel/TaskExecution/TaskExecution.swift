
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import Cocoa

// MARK: - Task Execution Loop

extension AgentViewModel {

    func executeTask(_ rawPrompt: String) async {
        // Strip ! or !apple prefix (bypasses Apple AI triage)
        var prompt = rawPrompt
        if prompt.hasPrefix("\u{F8FF}") {
            prompt = String(prompt.dropFirst()).trimmingCharacters(in: .whitespaces)
        } else if prompt.lowercased().hasPrefix("!apple ") {
            prompt = String(prompt.dropFirst(7)).trimmingCharacters(in: .whitespaces)
        }
        isRunning = true
        userWasActive = false
        rootWasActive = false
        // Auto-expand HUD for the main tab's run start (not on tab switches)
        thinkingExpanded = true
        thinkingOutputExpanded = true
        thinkingDismissed = false
        recentOutputHashes.removeAll()
        toolSteps.removeAll()
        DiffStore.shared.clear()

        // Start progress updates for iMessage requests (every 10 minutes)
        if agentReplyHandle != nil {
            startProgressUpdates(for: prompt)
        }

        // Clear LLM Output for new task — show blinking cursor
        dripTask?.cancel(); dripTask = nil
        rawLLMOutput = ""
        displayedLLMOutput = ""
        dripDisplayIndex = 0

        trimToRecentTasks()
        taskInputTokens = 0
        taskOutputTokens = 0
        budgetUsedFraction = 0
        subAgents.removeAll()
        FileBackupService.shared.clearTaskSnapshots()
        TokenUsageStore.shared.resetTaskMetrics()
        SessionStore.shared.newSession()
        FallbackChainService.shared.reset()
        Self.clearToolCache()
        // No mode filtering — send every user-enabled tool on every turn. The L
        let activeGroups: Set<String>? = nil
        let isXcode = Self.isXcodeProject(projectFolder)
        appendLog(Self.newTaskMarker)
        appendLog("👤 \(prompt)")
        flushLog()

        // Use ChatHistoryStore for LLM context
        let historyContext = ChatHistoryStore.shared.buildLLMContext()
        var (provider, modelName, isVision) = resolveInitialProviderConfig()
        // Defer "🧠 provider/model" log line until AFTER triage has run and we k
        let displayModel = modelDisplayName(provider: provider, modelId: modelName)
        let apiURL = chatURLForProvider(provider)
        let isCoding = apiURL.contains("/coding/")
        let cloudModelLogLine = "🧠 \(provider.displayName) / \(displayModel)\(isCoding ? " (code)" : "")\(isVision ? " (vision)" : "")"

        let mt = maxTokens
        var services = buildLLMServiceBundle(
            provider: provider,
            modelName: modelName,
            isVision: isVision,
            historyContext: historyContext,
            maxTokens: mt
        )

        // Start fresh — no prior conversation context to avoid corrupted messag
        var messages: [[String: Any]] = []

        // No agent name injection — avoid message format issues with some APIs

        let effectivePrompt = Self.newTaskPrefix(projectFolder: projectFolder, prompt: prompt) + prompt

        if !attachedImagesBase64.isEmpty {
            appendLog("(\(attachedImagesBase64.count) screenshot(s) attached)")
            var contentBlocks: [[String: Any]] = attachedImagesBase64.map { base64 in
                [
                    "type": "image",
                    "source": [
                        "type": "base64",
                        "media_type": "image/png",
                        "data": base64
                    ] as [String: Any]
                ]
            }
            contentBlocks.append(["type": "text", "text": effectivePrompt])
            messages.append(["role": "user", "content": contentBlocks])
            // Clear attachments after use
            attachedImages.removeAll()
            attachedImagesBase64.removeAll()
        } else {
            messages.append(["role": "user", "content": effectivePrompt])
        }

        commandsRun = []
        var completionSummary = ""
        var timeoutRetryCount = 0
        let maxTimeoutRetries = maxRetries

        // Apple Intelligence mediator for contextual annotations
        let mediator = AppleIntelligenceMediator.shared
        var appleAIAnnotations: [AppleIntelligenceMediator.Annotation] = []

        // ! or !apple prefix bypasses Apple AI triage
        let appleBypass = rawPrompt.hasPrefix("\u{F8FF}") || rawPrompt.lowercased().hasPrefix("!apple ")
        if appleBypass {
            appendLog("⏭ Apple AI bypassed")
            appendLog(cloudModelLogLine)
            flushLog()
        } else {
            // Triage: direct commands
            let triageResult = await mediator.triagePrompt(prompt, axDispatch: { [weak self] args in
                guard let self else { return "{\"success\":false,\"error\":\"agent deallocated\"}" }
                var input: [String: Any] = ["action": args.action]
                if let role = args.role { input["role"] = role }
                if let title = args.title { input["title"] = title }
                if let rawApp = args.app {
                    let resolved = SDEFService.shared.resolveBundleId(name: rawApp) ?? rawApp
                    input["appBundleId"] = resolved
                    input["app"] = resolved
                }
                if let text = args.text { input["text"] = text }
                return await self.executeNativeTool("accessibility", input: input)
            }, runAgent: { [weak self] args in
                guard let self else { return "error: agent deallocated" }
                let success = await self.runAgentDirect(name: args.name, arguments: args.arguments ?? "")
                return success ? "Launched agent '\(args.name)'" : "Agent '\(args.name)' not found"
            }, appendLog: { [weak self] msg in self?.appendLog(msg) }, projectFolder: projectFolder)
            let triageOutcome = await handleTriageOutcome(
                triageResult,
                prompt: prompt,
                cloudModelLogLine: cloudModelLogLine,
                messages: &messages,
                completionSummary: &completionSummary
            )
            if case .completed = triageOutcome { return }
        }

        // Apple Intelligence context injection removed

        var iterations = 0
        // Token budget tracker — detects diminishing returns and prevents runaw
        var budgetTracker = TokenBudgetTracker(ceiling: tokenBudgetCeiling)
        // Context compaction state — token-aware triggers with circuit breaker
        var compactionState = CompactionState()
        // Overnight coding guards
        var consecutiveReadOnlyCount = 0 // read guard — force stop after 10
        var unbuiltEditCount = 0 // build enforcement — nudge after edit without
        var consecutiveBuildFailures = 0 // error budget — stop after 5
        var stuckFiles: [String: Int] = [:] // stuck detection — skip after 5 fa
        // Full system prompt + full tool descriptions on every turn.
        let userName = NSFullUserName()
        let userHome = NSHomeDirectory()
        _ = userName; _ = userHome // kept for any future per-task prompt custom
        // Track unique files edited (write_file/edit_file/diff_apply/create_dif
        var filesEditedThisTask: Set<String> = []

        taskLoop: while !Task.isCancelled {
            iterations += 1

            // No prompt tiering and no mode auto-switching: every turn sends th

            // Token-aware context compaction — replaces fixed iteration-based t
            if iterations > 1 {
                _ = await Self.tieredCompact(&messages, state: &compactionState) { [weak self] msg in
                    self?.appendLog(msg)
                    self?.flushLog()
                }
            }

            do {
                isThinking = true
                // Only auto-show overlay on the FIRST iteration. Subsequent ite
                if iterations == 1 { thinkingDismissed = false }

                let sendMessages = iterations > 1 ? Self.compressMessages(messages) : messages

                let response: (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int)
                flushLog()
                if let claude = services.claude {
                    response = try await claude.sendStreaming(messages: sendMessages, activeGroups: activeGroups) { [weak self] delta in
                        Task { @MainActor in
                            self?.isThinking = false
                            self?.appendStreamDelta(delta)
                        }
                    }

                } else if let openAICompatible = services.openAICompatible {
                    let r = try await openAICompatible
                        .sendStreaming(messages: sendMessages, activeGroups: activeGroups) { [weak self] delta in
                            Task { @MainActor in
                                self?.isThinking = false
                                self?.appendStreamDelta(delta)
                            }
                        }
                    response = (r.content, r.stopReason, r.inputTokens, r.outputTokens)

                } else if let ollama = services.ollama {
                    let r = try await ollama.sendStreaming(messages: sendMessages, activeGroups: activeGroups) { [weak self] delta in
                        Task { @MainActor in
                            self?.isThinking = false
                            self?.appendStreamDelta(delta)
                        }
                    }
                    response = (r.content, r.stopReason, r.inputTokens, r.outputTokens)

                } else if let foundationModelService = services.foundationModel {
                    let r = try await foundationModelService.sendStreaming(messages: sendMessages) { [weak self] delta in
                        Task { @MainActor in
                            self?.isThinking = false
                            self?.appendStreamDelta(delta)
                        }
                    }
                    response = (r.content, r.stopReason, 0, 0)

                } else {
                    throw AgentError.noAPIKey
                }
                // Track token usage — use reported counts or estimate from text
                let inTok = response.inputTokens > 0 ? response.inputTokens : Self.estimateTokens(messages: messages)
                let outTok = response.outputTokens > 0 ? response.outputTokens : Self.estimateTokens(content: response.content)
                taskInputTokens += inTok
                taskOutputTokens += outTok
                sessionInputTokens += inTok
                sessionOutputTokens += outTok
                TokenUsageStore.shared.record(inputTokens: inTok, outputTokens: outTok)
                budgetTracker.recordTurn(inputTokens: inTok, outputTokens: outTok)
                budgetUsedFraction = budgetTracker.usedFraction
                TokenUsageStore.shared.recordModelUsage(model: modelName, input: inTok, output: outTok, provider: provider.displayName)
                FallbackChainService.shared.recordSuccess()
                flushStreamBuffer()
                isThinking = false
                timeoutRetryCount = 0 // Reset on successful response
                // Strip done/task_complete from LLM Output
                Self.stripCompletionText(&rawLLMOutput)
                // Wait for drip to finish
                while dripTask != nil {
                    try? await Task.sleep(for: .milliseconds(50))
                }
                if !rawLLMOutput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    displayedLLMOutput = rawLLMOutput
                    dripDisplayIndex = rawLLMOutput.count
                }
                if !rawLLMOutput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    try? await Task.sleep(for: .seconds(3))
                }
                guard !Task.isCancelled else { break }

                var toolResults: [[String: Any]] = []
                let parseResult = await parseLLMResponseContent(
                    response.content,
                    prompt: prompt,
                    mediator: mediator,
                    appleAIAnnotations: &appleAIAnnotations,
                    filesEditedThisTask: &filesEditedThisTask,
                    completionSummary: &completionSummary
                )
                if parseResult.taskCompleted { return }
                let hasToolUse = parseResult.hasToolUse
                let pendingTools = parseResult.pendingTools

                // App-layer action verification: if the LLM returned text claim
                if !hasToolUse && pendingTools.isEmpty {
                    let llmText = (response.content.compactMap { $0["text"] as? String }).joined()
                    let lower = llmText.lowercased()
                    let actionClaims = ["i searched", "i opened", "i clicked", "i ran ", "i executed",
                                        "i found the", "i read the file", "i checked the", "i listed"]
                    if actionClaims.contains(where: { lower.contains($0) }) {
                        appendLog("⚠️ action not performed — LLM claimed action without a tool call")
                        toolResults.append([
                            "type": "tool_result",
                            "tool_use_id": "action_not_performed",
                            "content": "action not performed — you claimed to perform an action but made no tool call. Use the appropriate tool or say you cannot do it."
                        ])
                    }
                }

                // Execute pending tools
                await executePendingToolBatches(
                    pendingTools: pendingTools,
                    toolResults: &toolResults
                )

                // Vision verification: auto-screenshot after UI actions so the
                await runVisionAutoScreenshotIfNeeded(
                    pendingTools: pendingTools,
                    isVision: isVision,
                    toolResults: &toolResults
                )

                // Token budget checks — nudge LLM or auto-stop if budget exhaus
                if budgetTracker.shouldStop {
                    let reason = budgetTracker.isDiminishing ? "diminishing returns detected" : "token budget exhausted"
                    appendLog("⚠️ Auto-stopping: \(reason) (\(budgetTracker.statusDescription))")
                    flushLog()
                    break
                }
                if budgetTracker.shouldNudge && !toolResults.isEmpty {
                    toolResults.append([
                        "type": "tool_result",
                        "tool_use_id": "budget_nudge",
                        "content": """
                            ⚠️ Approaching token budget limit \
                            (\(budgetTracker.statusDescription)). \
                            Wrap up your current work and call \
                            task_complete with a summary.
                            """
                    ])
                }

                // Cost alerting — stop if estimated cost exceeds user-configure
                if TokenUsageStore.shared.isCostExceeded {
                    let cost = String(format: "$%.2f", TokenUsageStore.shared.sessionEstimatedCost)
                    let max = String(format: "$%.2f", TokenUsageStore.shared.maxTaskCost)
                    appendLog("⚠️ Auto-stopping: estimated cost \(cost) exceeds limit \(max)")
                    flushLog()
                    break
                }

                // Overnight coding guards — read/build/error-budget/stuck-file
                let guardShouldBreak = runOvernightCodingGuards(
                    pendingTools: pendingTools,
                    toolResults: &toolResults,
                    consecutiveReadOnlyCount: &consecutiveReadOnlyCount,
                    unbuiltEditCount: &unbuiltEditCount,
                    consecutiveBuildFailures: &consecutiveBuildFailures,
                    stuckFiles: &stuckFiles,
                    isXcode: isXcode
                )
                if guardShouldBreak { break }

                // Collect completed sub-agent notifications and inject into too
                let subAgentNotifs = collectSubAgentNotifications()
                for notif in subAgentNotifs {
                    toolResults.append([
                        "type": "tool_result",
                        "tool_use_id": "subagent_notification",
                        "content": notif
                    ])
                }

                let finalizeShouldBreak = finalizeTurnAndDetectCompletion(
                    responseContent: response.content,
                    hasToolUse: hasToolUse,
                    toolResults: toolResults,
                    messages: &messages
                )
                if finalizeShouldBreak { break taskLoop }

            } catch {
                let activeService: ActiveLLMService
                if services.claude != nil {
                    activeService = .claude
                } else if services.openAICompatible != nil {
                    activeService = .openAICompatible
                } else if services.ollama != nil {
                    activeService = .ollama
                } else if services.foundationModel != nil {
                    activeService = .foundationModel
                } else {
                    activeService = .none
                }
                let outcome = await handleTaskLoopError(
                    error,
                    activeService: activeService,
                    providerDisplayName: provider.displayName,
                    messages: &messages,
                    timeoutRetryCount: &timeoutRetryCount,
                    maxTimeoutRetries: maxTimeoutRetries
                )
                switch outcome {
                case .continueLoop:
                    continue taskLoop
                case .breakLoop:
                    break taskLoop
                case .fallbackRequested(let newProvider, let newModel, let newIsVision):
                    provider = newProvider
                    modelName = newModel
                    isVision = newIsVision
                    services = buildLLMServiceBundle(
                        provider: provider,
                        modelName: modelName,
                        isVision: isVision,
                        historyContext: historyContext,
                        maxTokens: mt
                    )
                    continue taskLoop
                }
            }
        }

        // Apple Intelligence: suggest next steps after completion
        if mediator.isEnabled && mediator.showAnnotationsToUser && !completionSummary.isEmpty && !commandsRun.isEmpty {
            let context = "Task: \(prompt)\nResult: \(completionSummary)\nCommands: \(commandsRun.joined(separator: ", "))"
            if let nextSteps = await mediator.suggestNextSteps(context: context) {
                appendLog(nextSteps.formatted)
                flushLog()
                if agentReplyHandle != nil {
                    sendProgressUpdate(nextSteps.formatted)
                }
            }
        }

        // Always save history if task didn't call task_complete
        if completionSummary.isEmpty {
            let summary = Task.isCancelled ? "(cancelled)" : commandsRun.isEmpty ? "(no actions)" : "(incomplete)"
            history.add(
                TaskRecord(prompt: prompt, summary: summary, commandsRun: commandsRun),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
        }

        // End the task in SwiftData chat history
        ChatHistoryStore.shared.endCurrentTask(summary: completionSummary.isEmpty ? nil : completionSummary, cancelled: Task.isCancelled)

        // Stop progress updates
        stopProgressUpdates()

        flushLog()
        persistLogNow()
        isRunning = false
        isThinking = false
        userServiceActive = false
        rootServiceActive = false
        userWasActive = false
        rootWasActive = false
    }
}
