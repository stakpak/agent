
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import Cocoa


// MARK: - Task Execution Loop

extension AgentViewModel {

    func executeTask(_ prompt: String) async {
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
        // No mode filtering — send every user-enabled tool on every turn.
        // The LLM picks what it needs; ToolPreferencesService is the only filter.
        let activeGroups: Set<String>? = nil
        let isXcode = Self.isXcodeProject(projectFolder)
        appendLog(Self.newTaskMarker)
        appendLog("👤 \(prompt)")
        flushLog()

        // Use ChatHistoryStore for LLM context (summaries for older tasks, full messages for recent)
        let historyContext = ChatHistoryStore.shared.buildLLMContext()
        var provider = selectedProvider
        var modelName: String
        var isVision: Bool
        switch provider {
        case .claude:
            modelName = selectedModel
            isVision = true // Claude Sonnet/Opus/Haiku all support vision
        case .openAI:
            modelName = openAIModel
            isVision = true // GPT-4o, GPT-4 Turbo support vision
        case .deepSeek:
            modelName = deepSeekModel
            isVision = Self.isVisionModel(deepSeekModel)
        case .huggingFace:
            modelName = huggingFaceModel
            isVision = Self.isVisionModel(huggingFaceModel)
        case .ollama:
            modelName = ollamaModel
            isVision = selectedOllamaSupportsVision || Self.isVisionModel(ollamaModel)
        case .localOllama:
            modelName = localOllamaModel
            isVision = selectedLocalOllamaSupportsVision || Self.isVisionModel(localOllamaModel)
        case .vLLM:
            modelName = vLLMModel
            isVision = Self.isVisionModel(vLLMModel)
        case .lmStudio:
            modelName = lmStudioModel
            isVision = Self.isVisionModel(lmStudioModel)
        case .zAI:
            isVision = zAIModel.hasSuffix(":v")
            modelName = zAIModel.replacingOccurrences(of: ":v", with: "")
        case .bigModel:
            isVision = bigModelModel.hasSuffix(":v")
            modelName = bigModelModel.replacingOccurrences(of: ":v", with: "")
        case .qwen:
            modelName = qwenModel
            isVision = Self.isVisionModel(qwenModel)
        case .gemini:
            modelName = geminiModel
            isVision = true // Gemini supports vision
        case .grok:
            modelName = grokModel
            isVision = Self.isVisionModel(grokModel)
        case .mistral:
            modelName = mistralModel
            isVision = true
        case .codestral:
            modelName = codestralModel
            isVision = false
        case .vibe:
            modelName = vibeModel
            isVision = false
        case .foundationModel:
            modelName = "Apple Intelligence"
            isVision = false // Apple Intelligence doesn't support image input
        }
        if forceVision { isVision = true }
        appendLog("🧠 \(provider.displayName) / \(modelName)\(isVision ? " (vision)" : "")")
        flushLog()

        let mt = maxTokens
        var claude: ClaudeService?
        var openAICompatible: OpenAICompatibleService?
        var ollama: OllamaService?
        var foundationModelService: FoundationModelService?

        // Build LLM service instances for the current provider/model. Called at task start
        // and again whenever the fallback chain swaps providers mid-task.
        func buildLLMServices() {
            if provider == .claude {
                claude = ClaudeService(
                    apiKey: apiKey, model: modelName,
                    historyContext: historyContext,
                    projectFolder: projectFolder, maxTokens: mt
                )
            } else if provider == .lmStudio && lmStudioProtocol == .anthropic {
                claude = ClaudeService(
                    apiKey: lmStudioAPIKey, model: lmStudioModel,
                    historyContext: historyContext,
                    projectFolder: projectFolder,
                    baseURL: lmStudioEndpoint, maxTokens: mt
                )
            } else {
                claude = nil
            }
            // OpenAI-compatible service — URLs from LLMRegistry (single source of truth)
            switch provider {
            case .claude, .ollama, .localOllama, .foundationModel:
                openAICompatible = nil
            case .lmStudio where lmStudioProtocol == .anthropic:
                openAICompatible = nil
            case .lmStudio:
                let key = lmStudioProtocol == .lmStudio ? "input" : "messages"
                openAICompatible = OpenAICompatibleService(
                    apiKey: apiKeyForProvider(provider), model: modelName,
                    baseURL: lmStudioEndpoint, historyContext: historyContext,
                    projectFolder: projectFolder, provider: provider,
                    messagesKey: key, maxTokens: mt
                )
            case .vLLM:
                openAICompatible = OpenAICompatibleService(
                    apiKey: apiKeyForProvider(provider), model: modelName,
                    baseURL: vLLMEndpoint, historyContext: historyContext,
                    projectFolder: projectFolder, provider: provider,
                    maxTokens: mt
                )
            default:
                let url = chatURLForProvider(provider)
                openAICompatible = url.isEmpty ? nil : OpenAICompatibleService(
                    apiKey: apiKeyForProvider(provider), model: modelName,
                    baseURL: url, supportsVision: isVision,
                    historyContext: historyContext, projectFolder: projectFolder,
                    provider: provider, maxTokens: mt
                )
            }
            switch provider {
            case .ollama:
                ollama = OllamaService(
                    apiKey: ollamaAPIKey, model: modelName,
                    endpoint: ollamaEndpoint, supportsVision: isVision,
                    historyContext: historyContext, projectFolder: projectFolder,
                    provider: .ollama
                )
            case .localOllama:
                ollama = OllamaService(
                    apiKey: "", model: modelName, endpoint: localOllamaEndpoint,
                    supportsVision: isVision, historyContext: historyContext,
                    projectFolder: projectFolder, provider: .localOllama,
                    contextSize: localOllamaContextSize
                )
            default:
                ollama = nil
            }
            foundationModelService = provider == .foundationModel
                ? FoundationModelService(historyContext: historyContext, projectFolder: projectFolder) : nil

            // Set temperature per provider
            claude?.temperature = temperatureForProvider(provider == .claude ? .claude : provider)
            ollama?.temperature = temperatureForProvider(provider)
            openAICompatible?.temperature = temperatureForProvider(provider)
        }
        buildLLMServices()

        // Start fresh — no prior conversation context to avoid corrupted messages
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

        // Triage: direct commands, Apple AI conversation, accessibility agent,
        // or pass through to LLM. The axDispatch closure is what Apple AI's
        // Tool calls invoke when it decides to fire the accessibility tool —
        // we route it through the same executeNativeTool path the cloud LLM
        // uses, so the AX action goes through every existing safety check.
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
                // Parse "AgentName args" and always run directly — skip LLM
                let parts = cmd.argument.components(separatedBy: " ")
                let agentName = await Self.offMain { [ss = scriptService] in ss.resolveScriptName(parts.first ?? "") }
                let args = parts.count > 1 ? parts.dropFirst().joined(separator: " ") : ""
                if await Self.offMain({ [ss = scriptService] in ss.compileCommand(name: agentName) }) != nil {
                    let success = await runAgentDirect(name: agentName, arguments: args)
                    if success {
                        completionSummary = "Ran \(agentName)"
                        history.add(
                            TaskRecord(
                                prompt: prompt,
                                summary: completionSummary,
                                commandsRun: ["run_agent: \(agentName)"]
                            ),
                            maxBeforeSummary: maxHistoryBeforeSummary, apiKey: apiKey,
                            model: selectedModel
                        )
                        ChatHistoryStore.shared.endCurrentTask(summary: completionSummary)
                        stopProgressUpdates()
                        flushLog()
                        persistLogNow()
                        isRunning = false
                        isThinking = false
                        return
                    }
                    // Failed — fall through to LLM to handle
                    appendLog("Direct run failed — passing to LLM")
                    flushLog()
                    break
                }
            }
            // Execute known commands instantly without the LLM
            let output = await executeDirectCommand(cmd)
            flushLog()

            // For safari commands, pass results to LLM for formatting
            if cmd.name == "safari_open_and_search" {
                appendLog("✅ Opened page and searched. Results on screen.")
                flushLog()
            }
            if cmd.name == "google_search" && output.contains("\"success\": true") {
                messages.append(["role": "user", "content": """
                Format these Google search results for the user. \
                Be concise — show the top results with titles, URLs, \
                and brief descriptions:

                \(output)
                """])
                break // Fall through to LLM loop
            }
            if cmd.name == "safari_read" && !output.contains("Error") {
                messages.append(["role": "user", "content": """
                Summarize this web page for the user. \
                Show the title, URL, and key content:

                \(output)
                """])
                break // Fall through to LLM loop
            }
            // safari_open: if user had additional instructions, read page and pass to LLM
            if cmd.name == "safari_open" {
                appendLog("✅ \(output)")
                // Check if the original prompt has more than just "open <url>"
                let urlArg = cmd.argument.lowercased()
                let remaining = prompt.lowercased().replacingOccurrences(of: urlArg, with: "")
                let noise = Set([
                    "open",
                    "safari",
                    "in",
                    "on",
                    "to",
                    "and",
                    "the",
                    "using",
                    "webpage",
                    "web",
                    "page",
                    "website",
                    "url",
                    "go",
                    "navigate",
                    "visit",
                    "browse"
                ])
                let meaningfulWords = remaining.components(separatedBy: .whitespacesAndNewlines)
                    .filter { !$0.isEmpty && !noise.contains($0) }
                if !meaningfulWords.isEmpty {
                    // Wait briefly for page to load
                    try? await Task.sleep(for: .seconds(2))
                    let pageContent = await WebAutomationService.shared.readPageContent(maxLength: 3000)
                    let pageTitle = await WebAutomationService.shared.getPageTitle()
                    let pageURL = await WebAutomationService.shared.getPageURL()
                    messages.append([
                        "role": "user",
                        "content": """
                            I opened \(pageURL) (\(pageTitle)). \
                            Here is the page content:

                            \(pageContent)

                            Now complete this request: \(prompt)
                            """
                    ])
                    break // Fall through to LLM loop
                }
            }

            completionSummary = "Executed \(cmd.name)"
            history.add(
                TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: [cmd.name]),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
            ChatHistoryStore.shared.endCurrentTask(summary: completionSummary)
            stopProgressUpdates()
            if agentReplyHandle != nil { sendProgressUpdate(output) }
            flushLog()
            persistLogNow()
            isRunning = false
            isThinking = false
            return
        case .answered(let reply):
            // Show in LLM Output, not LogView
            rawLLMOutput = reply
            displayedLLMOutput = reply
            dripDisplayIndex = reply.count
            appendLog("✅ Completed: \(String(reply.prefix(200)))")
            flushLog()
            completionSummary = String(reply.prefix(200))
            history.add(
                TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: []),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
            ChatHistoryStore.shared.endCurrentTask(summary: completionSummary)
            stopProgressUpdates()
            if agentReplyHandle != nil { sendProgressUpdate(reply) }
            flushLog()
            persistLogNow()
            isRunning = false
            isThinking = false
            return
        case .accessibilityHandled(let summary):
            // Apple AI ran the accessibility tool itself (one or more times)
            // and produced a final summary. The tool calls already happened
            // through the axDispatch closure above — they went through the
            // same executeNativeTool path the cloud LLM uses, so they're
            // already logged in the activity log. The summary string is
            // what Apple AI said it accomplished after the tool calls.
            //
            // If Apple AI never called the tool, or any tool call failed,
            // runAccessibilityAgent returns nil and we never reach this
            // case (we fall through to .passThrough → cloud LLM).
            rawLLMOutput = summary
            displayedLLMOutput = summary
            dripDisplayIndex = summary.count
            appendLog("🍎 \(summary)")
            flushLog()
            completionSummary = String(summary.prefix(200))
            history.add(
                TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: ["accessibility (Apple AI)"]),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
            ChatHistoryStore.shared.endCurrentTask(summary: completionSummary)
            stopProgressUpdates()
            if agentReplyHandle != nil { sendProgressUpdate(summary) }
            flushLog()
            persistLogNow()
            isRunning = false
            isThinking = false
            return
        case .passThrough:
            break
        }

        // Apple Intelligence context injection removed — was confusing LLMs at task start
        // Apple AI still runs on task_complete to summarize results for the user

        var iterations = 0
        // Token budget tracker — detects diminishing returns and prevents runaway costs
        var budgetTracker = TokenBudgetTracker(ceiling: tokenBudgetCeiling)
        // Context compaction state — token-aware triggers with circuit breaker
        var compactionState = CompactionState()
        // Overnight coding guards
        var consecutiveReadOnlyCount = 0 // read guard — force stop after 10
        var unbuiltEditCount = 0 // build enforcement — nudge after edit without build
        var consecutiveBuildFailures = 0 // error budget — stop after 5
        var stuckFiles: [String: Int] = [:] // stuck detection — skip after 5 failures per file
        // Full system prompt + full tool descriptions on every turn. The earlier
        // condensed-prompt + compactTools optimization saved ~4K tokens/turn but the
        // user prefers the LLM having maximum context every iteration over the savings.
        let userName = NSFullUserName()
        let userHome = NSHomeDirectory()
        _ = userName; _ = userHome // kept for any future per-task prompt customization
        // Track unique files edited (write_file/edit_file/diff_apply/create_diff/apply_diff) for plan-mode enforcement
        var filesEditedThisTask: Set<String> = []

        while !Task.isCancelled {
            iterations += 1

            // No prompt tiering and no mode auto-switching: every turn sends the
            // full system prompt with full tool descriptions, filtered only by the
            // user's UI toggles in ToolPreferencesService.

            // Token-aware context compaction — replaces fixed iteration-based triggers
            if iterations > 1 {
                _ = await Self.tieredCompact(&messages, state: &compactionState) { [weak self] msg in
                    self?.appendLog(msg)
                    self?.flushLog()
                }
            }

            do {
                isThinking = true
                // Only auto-show overlay on the FIRST iteration. Subsequent iterations
                // must respect the user's manual dismiss (Cmd+B during a running task).
                if iterations == 1 { thinkingDismissed = false }

                let sendMessages = iterations > 1 ? Self.compressMessages(messages) : messages

                let response: (content: [[String: Any]], stopReason: String, inputTokens: Int, outputTokens: Int)
                flushLog()
                if let claude {
                    response = try await claude.sendStreaming(messages: sendMessages, activeGroups: activeGroups) { [weak self] delta in
                        Task { @MainActor in
                            self?.isThinking = false
                            self?.appendStreamDelta(delta)
                        }
                    }

                } else if let openAICompatible {
                    let r = try await openAICompatible
                        .sendStreaming(messages: sendMessages, activeGroups: activeGroups) { [weak self] delta in
                            Task { @MainActor in
                                self?.isThinking = false
                                self?.appendStreamDelta(delta)
                            }
                        }
                    response = (r.content, r.stopReason, r.inputTokens, r.outputTokens)

                } else if let ollama {
                    let r = try await ollama.sendStreaming(messages: sendMessages, activeGroups: activeGroups) { [weak self] delta in
                        Task { @MainActor in
                            self?.isThinking = false
                            self?.appendStreamDelta(delta)
                        }
                    }
                    response = (r.content, r.stopReason, r.inputTokens, r.outputTokens)

                } else if let foundationModelService {
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
                // Track token usage — use reported counts or estimate from text (~4 chars/token)
                let inTok = response.inputTokens > 0 ? response.inputTokens : Self.estimateTokens(messages: messages)
                let outTok = response.outputTokens > 0 ? response.outputTokens : Self.estimateTokens(content: response.content)
                taskInputTokens += inTok
                taskOutputTokens += outTok
                sessionInputTokens += inTok
                sessionOutputTokens += outTok
                TokenUsageStore.shared.record(inputTokens: inTok, outputTokens: outTok)
                budgetTracker.recordTurn(inputTokens: inTok, outputTokens: outTok)
                budgetUsedFraction = budgetTracker.usedFraction
                TokenUsageStore.shared.recordModelUsage(model: modelName, input: inTok, output: outTok)
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
                var hasToolUse = false
                var pendingTools: [(toolId: String, name: String, input: [String: Any])] = []

                for block in response.content {
                    guard let type = block["type"] as? String else { continue }

                    if type == "text" {
                        // LLM text goes to LLM Output only — LogView is for user status
                    } else if type == "server_tool_use" {
                        // Server-side tool (web search) — executed by the API, just log it
                        hasToolUse = true
                        if let input = block["input"] as? [String: Any],
                           let query = input["query"] as? String
                        {
                            appendLog("Web search: \(query)")
                        }
                    } else if type == "web_search_tool_result" {
                        // Display search results summary
                        if let content = block["content"] as? [[String: Any]] {
                            let results = content.compactMap { result -> String? in
                                guard result["type"] as? String == "web_search_result",
                                      let title = result["title"] as? String,
                                      let url = result["url"] as? String else { return nil }
                                return "  \(title)\n    \(url)"
                            }
                            if !results.isEmpty {
                                appendLog("📊\n" + results.prefix(5).joined(separator: "\n"))
                            }
                        }
                        flushLog()
                    } else if type == "tool_use" {
                        hasToolUse = true
                        guard let toolId = block["id"] as? String,
                              var name = block["name"] as? String,
                              var input = block["input"] as? [String: Any] else { continue }

                        // Expand consolidated CRUDL tools into legacy tool names
                        (name, input) = Self.expandConsolidatedTool(name: name, input: input)

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

                        if name == "task_complete" {
                            var summary = input["summary"] as? String ?? "Done"
                            let stripped = summary.trimmingCharacters(in: CharacterSet(charactersIn: ". "))
                            if stripped.isEmpty || summary == "..." {
                                let lastText = rawLLMOutput.trimmingCharacters(in: .whitespacesAndNewlines)
                                if !lastText.isEmpty { summary = String(lastText.prefix(300)) }
                            }
                            completionSummary = summary
                            // Show task complete in the LLM Output HUD so the user sees the result.
                            // Append to rawLLMOutput and let the drip task pick up the new chars
                            // naturally — DO NOT sync displayedLLMOutput, that would skip the drip.
                            let trimmedRaw = rawLLMOutput.trimmingCharacters(in: .whitespacesAndNewlines)
                            if trimmedRaw.isEmpty {
                                rawLLMOutput = "✅ \(summary)"
                            } else if !trimmedRaw.contains(summary) {
                                rawLLMOutput += "\n\n✅ \(summary)"
                            }
                            // Make sure the drip task is still running so it picks up the appended chars
                            startDripIfNeeded()

                            // Apple Intelligence summary annotation
                            if mediator.isEnabled && mediator.showAnnotationsToUser && !commandsRun.isEmpty {
                                if let summaryAnnotation = await mediator.summarizeCompletion(summary: summary, commandsRun: commandsRun) {
                                    appleAIAnnotations.append(summaryAnnotation)
                                    appendLog(summaryAnnotation.formatted)
                                    flushLog()
                                    if agentReplyHandle != nil {
                                        sendProgressUpdate(summaryAnnotation.formatted)
                                    }
                                }
                            }

                            appendLog("✅ Completed: \(summary)")
                            flushLog()
                            history.add(
                                TaskRecord(prompt: prompt, summary: summary, commandsRun: commandsRun),
                                maxBeforeSummary: maxHistoryBeforeSummary,
                                apiKey: apiKey,
                                model: selectedModel
                            )
                            // End the task in SwiftData chat history
                            ChatHistoryStore.shared.endCurrentTask(summary: summary)
                            // Stop progress updates before sending final reply
                            stopProgressUpdates()
                            // Reply to the iMessage sender if this was an Agent! prompt
                            sendAgentReply(summary)
                            isRunning = false
                            return
                        }

                        pendingTools.append((toolId: toolId, name: name, input: input))
                    }
                }

                // Execute pending tools — partition into read/write batches
                // Consecutive read-only tools run in parallel; write tools serialize
                if !pendingTools.isEmpty {
                    let maxConcurrency = 10
                    // Partition into batches: consecutive read-only = parallel batch, write = serial batch
                    var batches: [(parallel: Bool, tools: [(toolId: String, name: String, input: [String: Any])])] = []
                    for tool in pendingTools {
                        let isReadOnly = Self.readOnlyTools.contains(tool.name)
                        if isReadOnly, let last = batches.last, last.parallel {
                            batches[batches.count - 1].tools.append(tool)
                        } else {
                            batches.append((parallel: isReadOnly, tools: [tool]))
                        }
                    }

                    for batch in batches {
                        if batch.parallel && batch.tools.count > 1 {
                            // Parallel batch: pre-execute shell tools off MainActor
                            let shellTools: Set<String> = [
                                "read_file",
                                "list_files",
                                "search_files",
                                "read_dir",
                                "git_status",
                                "git_diff",
                                "git_log",
                                "git_diff_patch"
                            ]
                            let shellBatch = batch.tools.filter { shellTools.contains($0.name) }
                            if shellBatch.count > 1 {
                                let capturedPF = projectFolder
                                let cmds = shellBatch.map { (
                                    $0.toolId,
                                    Self.buildReadOnlyCommand(name: $0.name, input: $0.input, projectFolder: capturedPF)
                                ) }
                                var preResults: [String: String] = [:]
                                await withTaskGroup(of: (String, String).self) { group in
                                    for (i, (id, cmd)) in cmds.enumerated() where i < maxConcurrency {
                                        let cid = id; let ccmd = cmd
                                        let workDir = capturedPF.isEmpty ? NSHomeDirectory() : capturedPF
                                        group.addTask {
                                            guard !ccmd.isEmpty else { return (cid, "") }
                                            let pipe = Pipe(); let p = Process()
                                            p.executableURL = URL(fileURLWithPath: "/bin/zsh")
                                            p.arguments = ["-c", ccmd]
                                            p.currentDirectoryURL = URL(fileURLWithPath: workDir)
                                            var env = ProcessInfo.processInfo.environment
                                            env["HOME"] = NSHomeDirectory()
                                            env["PATH"] = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:" +
                                                (env["PATH"] ?? "")
                                            p.environment = env; p.standardOutput = pipe; p.standardError = pipe
                                            try? p.run(); p.waitUntilExit()
                                            return (
                                                cid,
                                                String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
                                            )
                                        }
                                    }
                                    for await (id, result) in group { preResults[id] = result }
                                }
                                Self.precomputedResults = preResults
                            }
                            for tool in batch.tools {
                                let ctx = ToolContext(
                                    toolId: tool.toolId,
                                    projectFolder: projectFolder,
                                    selectedProvider: selectedProvider,
                                    tavilyAPIKey: tavilyAPIKey
                                )
                                _ = await dispatchTool(name: tool.name, input: tool.input, ctx: ctx, toolResults: &toolResults)
                            }
                            Self.precomputedResults = nil
                        } else {
                            // Serial batch: execute one by one
                            for tool in batch.tools {
                                let ctx = ToolContext(
                                    toolId: tool.toolId,
                                    projectFolder: projectFolder,
                                    selectedProvider: selectedProvider,
                                    tavilyAPIKey: tavilyAPIKey
                                )
                                _ = await dispatchTool(name: tool.name, input: tool.input, ctx: ctx, toolResults: &toolResults)
                            }
                        }
                    }
                }

                // Vision verification: auto-screenshot after UI actions so the LLM can see the result.
                // OPT-IN via visionAutoScreenshotEnabled (Settings → Vision Auto-Screenshot).
                // Default OFF because it (1) hogs the main thread on every UI iteration,
                // (2) bloats every prompt with a base64 image even for non-vision models,
                // and (3) the next accessibility(find_element) query usually tells the LLM
                // what happened just as well, without the screenshot cost.
                if visionAutoScreenshotEnabled && isVision && !pendingTools.isEmpty {
                    let uiActions: Set<String> = [
                        "ax_click",
                        "ax_click_element",
                        "ax_perform_action",
                        "ax_type_text",
                        "ax_type_into_element",
                        "ax_open_app",
                        "ax_scroll",
                        "ax_drag",
                        "click",
                        "click_element",
                        "perform_action",
                        "type_text",
                        "open_app",
                        "web_click",
                        "web_type",
                        "web_navigate"
                    ]
                    let hadUIAction = pendingTools.contains { uiActions.contains($0.name) }
                    if hadUIAction {
                        let screenshotResult = await Self.captureVerificationScreenshot()
                        if let imageData = screenshotResult {
                            // Append screenshot as image content block to tool results
                            toolResults.append([
                                "type": "tool_result",
                                "tool_use_id": "vision_verify",
                                "content": [
                                    ["type": "text", "text": "[Auto-screenshot after UI action — verify the action succeeded]"],
                                    ["type": "image", "source": ["type": "base64", "media_type": "image/png", "data": imageData]]
                                ]
                            ])
                            appendLog("📸 Vision: auto-screenshot for verification")
                        }
                    }
                }

                // Token budget checks — nudge LLM or auto-stop if budget exhausted / diminishing returns
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

                // Cost alerting — stop if estimated cost exceeds user-configured max
                if TokenUsageStore.shared.isCostExceeded {
                    let cost = String(format: "$%.2f", TokenUsageStore.shared.sessionEstimatedCost)
                    let max = String(format: "$%.2f", TokenUsageStore.shared.maxTaskCost)
                    appendLog("⚠️ Auto-stopping: estimated cost \(cost) exceeds limit \(max)")
                    flushLog()
                    break
                }

                // MARK: Overnight coding guards
                if !pendingTools.isEmpty {
                    let editTools: Set<String> = ["write_file", "edit_file", "diff_apply", "apply_diff", "create_diff", "diff_and_apply"]
                    let buildTools: Set<String> = ["xcode_build", "xc_build"]
                    let actionTools: Set<String> = editTools.union(buildTools).union([
                        "git_commit",
                        "run_shell_script",
                        "execute_agent_command",
                        "execute_daemon_command",
                        "task_complete"
                    ])
                    let automationPrefixes = ["ax_", "web_", "selenium_"]
                    let automationTools: Set<String> = [
                        "accessibility",
                        "run_applescript",
                        "run_osascript",
                        "execute_javascript",
                        "lookup_sdef",
                        "ax",
                        "web",
                        "sel"
                    ]
                    let hadAction = pendingTools.contains { tool in
                        actionTools.contains(tool.name)
                            || automationPrefixes.contains(where: { tool.name.hasPrefix($0) })
                            || automationTools.contains(tool.name)
                    }
                    let hadEdit = pendingTools.contains { editTools.contains($0.name) }
                    let hadBuild = pendingTools.contains { buildTools.contains($0.name) }

                    // 1. Read guard — nudge at 5, hard snap-out at 10 (no stop)
                    //
                    // The previous snap-out message told the model to "pick the most
                    // likely file and make an edit". That phrasing pushed the LLM into
                    // confabulating answers from incomplete data — observed in the
                    // wild as a "gap analysis" tool result that invented findings the
                    // model had never read. The new wording forces the only two
                    // legitimate moves when reads are exhausted: narrow the question
                    // to a SINGLE concrete fact and act on it, OR call done() and
                    // honestly report what's still unknown. Fabricating from partial
                    // reads is explicitly worse than admitting uncertainty.
                    if hadAction { consecutiveReadOnlyCount = 0 } else { consecutiveReadOnlyCount += pendingTools.count }
                    if consecutiveReadOnlyCount >= 10 {
                        toolResults.append([
                            "type": "tool_result",
                            "tool_use_id": "read_snap",
                            "content": """
                                🛑 INSUFFICIENT EVIDENCE: \(consecutiveReadOnlyCount) \
                                consecutive reads/searches with NO edits, builds, or actions. \
                                You do NOT have enough information to produce a confident answer. \
                                You have exactly TWO legitimate moves: \
                                (a) Narrow your question to ONE specific file/function/fact, \
                                look it up, then act ONLY on what you actually read. \
                                (b) Call done() and honestly report what you found AND what is \
                                still unknown. DO NOT fabricate findings, gap analyses, summaries, \
                                or comparisons from incomplete reads. DO NOT 'pick the most likely \
                                answer' — confabulating from partial evidence is worse than \
                                admitting uncertainty. If you cannot narrow further, call done() \
                                with 'I need more information about X' in the summary.
                                """
                        ])
                        appendLog("🛑 Snap-out: \(consecutiveReadOnlyCount) reads — narrow or call done()")
                        flushLog()
                        consecutiveReadOnlyCount = 0 // Reset after snap so we don't loop the nudge
                    } else if consecutiveReadOnlyCount >= 5 {
                        toolResults.append([
                            "type": "tool_result",
                            "tool_use_id": "read_guard",
                            "content": """
                                ⚠️ \(consecutiveReadOnlyCount) consecutive reads without \
                                editing or acting. Either narrow to one specific question \
                                and act on it, or call done() with what you actually know — \
                                do NOT guess or fabricate from partial evidence.
                                """
                        ])
                    }

                    // 2. Build enforcement — only for Xcode projects
                    if isXcode {
                        if hadEdit { unbuiltEditCount += 1 }
                        if hadBuild { unbuiltEditCount = 0 }
                        if unbuiltEditCount >= 3 {
                            toolResults.append([
                                "type": "tool_result",
                                "tool_use_id": "build_nudge",
                                "content": """
                                    ⚠️ You've edited \(unbuiltEditCount) times \
                                    without building. Run xc(action:"build") now \
                                    to catch errors early.
                                    """
                            ])
                        }
                    }

                    // 3. Error budget — track consecutive build failures (Xcode only)
                    for tool in pendingTools where isXcode && buildTools.contains(tool.name) {
                        let buildOutput = toolResults.last?["content"] as? String ?? ""
                        if buildOutput.contains("BUILD FAILED") || buildOutput.contains("error:") {
                            consecutiveBuildFailures += 1
                            if consecutiveBuildFailures >= 5 {
                                appendLog("⚠️ Auto-stopping: 5 consecutive build failures")
                                flushLog()
                                break
                            }
                        } else {
                            consecutiveBuildFailures = 0
                        }
                    }
                    if consecutiveBuildFailures >= 5 { break }

                    // 4. Stuck detection — track edit failures per file. Fires at 3
                    //    failures with an actionable nudge (re-read fresh, copy bytes
                    //    exactly), then again at 6 with a "skip and move on" message.
                    //    Lowered from 5 because users were cancelling tasks at 3-4
                    //    repeated failures, well before the old threshold fired.
                    for tool in pendingTools where editTools.contains(tool.name) {
                        guard let path = tool.input["file_path"] as? String ?? tool.input["path"] as? String else { continue }
                        let output = toolResults.last?["content"] as? String ?? ""
                        let lower = output.lowercased()
                        let isFailure = lower.hasPrefix("error") || lower.contains("error:") || lower.contains("failed") || lower
                            .contains("not found") || lower.contains("rejected")
                        if isFailure {
                            stuckFiles[path, default: 0] += 1
                            let count = stuckFiles[path]!
                            if count == 3 {
                                // First nudge — actionable recovery guidance
                                let nudge = """
                                ⚠️ 3 consecutive edit failures on \(path). STOP retrying the same approach.

                                Recovery checklist (do these in order):
                                1. read_file(file_path:"\(path)") with NO offset/limit to get the FULL fresh content
                                2. Find the EXACT lines you want to change in the new output. \
                                Do NOT trust the tool_result from earlier reads — the file may \
                                have been modified by your previous edits or by other code.
                                3. For edit_file: copy old_string verbatim from the fresh read, \
                                including every space, tab, and newline. Even one wrong character \
                                causes 'old_string not found'.
                                4. For diff_and_apply: pass start_line and end_line of the section \
                                you're editing so the section is small and unambiguous.
                                5. If you keep failing, switch tools — write_file to overwrite \
                                the whole file is a valid last resort.
                                """
                                toolResults.append([
                                    "type": "tool_result",
                                    "tool_use_id": "stuck_guard_3",
                                    "content": nudge
                                ])
                                appendLog("⚠️ Stuck nudge: 3 failures on \((path as NSString).lastPathComponent)")
                                flushLog()
                            } else if count >= 6 {
                                // Second nudge — give up on this file
                                toolResults.append([
                                    "type": "tool_result",
                                    "tool_use_id": "stuck_guard_6",
                                    "content": """
                                        🛑 6 failures on \(path). Stop trying to edit \
                                        this file. Move on to the next part of your task \
                                        or call done with what you've completed so far.
                                        """
                                ])
                                appendLog("🛑 Stuck-out: 6 failures on \((path as NSString).lastPathComponent)")
                                flushLog()
                                stuckFiles[path] = 0
                            }
                        } else {
                            stuckFiles[path] = 0
                        }
                    }
                }

                // Collect completed sub-agent notifications and inject into tool results
                let subAgentNotifs = collectSubAgentNotifications()
                for notif in subAgentNotifs {
                    toolResults.append([
                        "type": "tool_result",
                        "tool_use_id": "subagent_notification",
                        "content": notif
                    ])
                }

                // Add assistant response to conversation
                // Guard against empty content — Ollama rejects assistant messages with no content or tool_calls
                let assistantContent: Any = response.content.isEmpty
                    ? "I'll continue with the task." as Any
                    : response.content as Any
                let assistantMsg: [String: Any] = ["role": "assistant", "content": assistantContent]
                messages.append(assistantMsg)
                SessionStore.shared.appendMessage(assistantMsg)

                if hasToolUse && !toolResults.isEmpty {
                    // Truncate large tool results to save tokens (cap at 8K chars each)
                    let capped = Self.truncateToolResults(toolResults)
                    let userMsg: [String: Any] = ["role": "user", "content": capped]
                    messages.append(userMsg)
                    SessionStore.shared.appendMessage(userMsg)
                } else if !hasToolUse {
                    // Check if model wrote task_complete/done as text instead of a tool call
                    let responseText = response.content.compactMap { $0["text"] as? String }.joined()
                    if responseText.contains("task_complete") || responseText.contains("done(summary") {
                        if let match = responseText.range(
                            of: #"(?:task_complete|done)\(summary[=:]\s*"([^"]+)""#,
                            options: .regularExpression
                        ) {
                            let raw = String(responseText[match])
                            let summary = raw.replacingOccurrences(
                                of: #"(?:task_complete|done)\(summary[=:]\s*""#,
                                with: "",
                                options: .regularExpression
                            ).replacingOccurrences(of: "\"", with: "")
                            appendLog("✅ Completed: \(summary)")
                        }
                        flushLog()
                        break
                    }
                    // Check if model signaled completion via natural language
                    let lower = responseText.lowercased()
                    let doneSignals = [
                        "conclude this task",
                        "i'll conclude",
                        "task is complete",
                        "no further action",
                        "nothing more to do",
                        "no more content"
                    ]
                    if doneSignals.contains(where: { lower.contains($0) }) {
                        // Ensure LLM Output shows the response
                        displayedLLMOutput = rawLLMOutput
                        dripDisplayIndex = rawLLMOutput.count
                        let summary = String(responseText.prefix(300))
                        appendLog("✅ Completed: \(summary)")
                        flushLog()
                        break
                    }
                    // Text-only response (no tool calls) — complete immediately
                    if rawLLMOutput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        rawLLMOutput = responseText
                    }
                    displayedLLMOutput = rawLLMOutput
                    dripDisplayIndex = rawLLMOutput.count
                    let summary = String(responseText.prefix(300))
                    appendLog("✅ Completed: \(summary)")
                    flushLog()
                    break
                } else {
                    // Check if LLM signaled it's done via text even though it made tool calls
                    let allText = response.content.compactMap { $0["text"] as? String }.joined().lowercased()
                    let stopPhrases = [
                        "no more content",
                        "no further action",
                        "task is complete",
                        "nothing more to do",
                        "task_complete",
                        "conclude this task",
                        "i'll conclude",
                        "feel free to ask",
                        "let me know if"
                    ]
                    if stopPhrases.contains(where: { allText.contains($0) }) {
                        break
                    }
                }

            } catch {
                if !Task.isCancelled {
                    let errMsg = error.localizedDescription

                    // Context overflow — prune messages aggressively and retry
                    let isOverflow = errMsg.contains("max_tokens") || errMsg.contains("context_length") || errMsg
                        .contains("too many tokens") || errMsg.contains("prompt is too long")
                    if isOverflow {
                        appendLog("⚠️ Context overflow — pruning messages and retrying")
                        flushLog()
                        Self.pruneMessages(&messages, keepRecent: 4)
                        Self.stripOldImages(&messages)
                        continue
                    }

                    // Stale connection — retry with fresh request
                    let isStaleConnection = errMsg.contains("ECONNRESET") || errMsg.contains("EPIPE")
                        || errMsg.contains("connection reset") || errMsg.contains("broken pipe")
                    if isStaleConnection && timeoutRetryCount < maxTimeoutRetries {
                        timeoutRetryCount += 1
                        appendLog("🔌 Connection reset — retrying (\(timeoutRetryCount)/\(maxTimeoutRetries))")
                        flushLog()
                        try? await Task.sleep(for: .seconds(2))
                        continue
                    }

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
                                appendLog("🔍 Checking Ollama server health...")
                                flushLog()

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
                                    appendLog("⚠️ Ollama server not responding. Attempting to restart...")
                                    flushLog()

                                    // Restart Ollama via UserService XPC
                                    _ = await userService
                                        .execute(command: "pkill -f 'ollama serve' && sleep 2 && open /Applications/Ollama.app")
                                    appendLog("🔄 Restart command executed")
                                    flushLog()

                                    // Wait longer for Ollama startup
                                    let startupDelay = TimeInterval(min(10 * timeoutRetryCount, 30)) // Exponential backoff up to 30 seconds
                                    let retryMessage =
                                        """
                                        \(errorSource) timeout detected \
                                        (attempt \(timeoutRetryCount)/\(maxTimeoutRetries)) — \
                                        Ollama restart attempted, \
                                        waiting \(Int(startupDelay)) seconds...
                                        """
                                    appendLog(retryMessage)
                                    flushLog()
                                    if agentReplyHandle != nil {
                                        sendProgressUpdate(retryMessage)
                                    }

                                    try? await Task.sleep(for: .seconds(startupDelay))
                                    if Task.isCancelled { break }
                                    continue
                                } else {
                                    appendLog("✅ Ollama server is running but API timed out")
                                    flushLog()
                                }
                            }

                            let retryDelay = TimeInterval(min(10 * timeoutRetryCount, 30)) // Exponential backoff up to 30 seconds
                            let retryMessage =
                                """
                                \(errorSource) timeout detected \
                                (attempt \(timeoutRetryCount)/\(maxTimeoutRetries)) — \
                                retrying in \(Int(retryDelay)) seconds...
                                """
                            appendLog(retryMessage)
                            flushLog()
                            if agentReplyHandle != nil {
                                sendProgressUpdate(retryMessage)
                            }

                            // Log to task log for debugging

                            try? await Task.sleep(for: .seconds(retryDelay))
                            if Task.isCancelled { break }
                            continue
                        } else {
                            // Max retries reached - try final Ollama restart if applicable
                            if (errorSource == "Ollama API" || errorSource == "Local Ollama") && timeoutRetryCount == maxTimeoutRetries {
                                appendLog("🔄 Max retries reached. Attempting final Ollama restart...")
                                flushLog()

                                // Restart Ollama via UserService XPC
                                _ = await userService
                                    .execute(command: "pkill -f 'ollama serve' && sleep 3 && open /Applications/Ollama.app && sleep 10")
                                appendLog("Ollama restart attempted. Please check Ollama application status.")
                                flushLog()
                            }

                            let timeoutMessage =
                                """
                                \(errorSource) timeout after \(maxTimeoutRetries) \
                                retries. Please check your network connection \
                                or try a different LLM provider.
                                """
                            appendLog(timeoutMessage)
                            flushLog()
                            if agentReplyHandle != nil {
                                sendProgressUpdate(timeoutMessage)
                            }
                            break
                        }
                    } else if let agentErr = error as? AgentError, agentErr.isRecoverable, timeoutRetryCount < maxTimeoutRetries {
                        // Server/network error — retry every 10 seconds
                        timeoutRetryCount += 1
                        let retryDelay: TimeInterval = 10
                        appendLog(
                            """
                            \(errorSource) recoverable error \
                            (attempt \(timeoutRetryCount)/\(maxTimeoutRetries)) — \
                            retrying in \(Int(retryDelay))s...
                            \(errMsg)
                            """
                        )
                        flushLog()
                        try? await Task.sleep(for: .seconds(retryDelay))
                        if Task.isCancelled { break }
                        continue
                    } else if errMsg.lowercased().contains("network")
                        || errMsg.lowercased().contains("connection")
                        || errMsg.lowercased().contains("internet")
                        || (error as? URLError)?.code == .networkConnectionLost
                        || (error as? URLError)?.code == .notConnectedToInternet
                    {
                        timeoutRetryCount += 1
                        if timeoutRetryCount <= maxTimeoutRetries {
                            let delay = networkRetryDelay
                            appendLog(
                                """
                                🌐 Network connection lost — retrying in \(delay)s \
                                (attempt \(timeoutRetryCount)/\(maxTimeoutRetries))...
                                """
                            )
                            flushLog()
                            try? await Task.sleep(for: .seconds(Double(delay)))
                            if Task.isCancelled { break }
                            continue
                        } else {
                            appendLog("🌐 Network connection lost after \(maxTimeoutRetries) retries.")
                            flushLog()
                            break
                        }
                    } else {
                        // Try fallback chain before giving up
                        if let fallback = FallbackChainService.shared.recordFailure() {
                            appendLog("🔄 Switching to fallback: \(fallback.displayName)")
                            flushLog()
                            // Resolve provider+model from the fallback entry, then rebuild services
                            if let fbProvider = APIProvider(rawValue: fallback.provider) {
                                provider = fbProvider
                                modelName = fallback.model
                                isVision = Self.isVisionModel(modelName)
                                if forceVision { isVision = true }
                                buildLLMServices()
                                appendLog("✅ Now using \(provider.displayName) / \(modelName)")
                                flushLog()
                            }
                            timeoutRetryCount = 0
                            continue
                        }

                        // Non-recoverable error — no fallback available
                        appendLog("\(errorSource) Error: \(errMsg)")
                        flushLog()

                        // Apple Intelligence error explanation
                        if mediator.isEnabled && mediator.showAnnotationsToUser {
                            if let errorAnnotation = await mediator.explainError(toolName: "LLM request", error: errMsg) {
                                appendLog(errorAnnotation.formatted)
                                flushLog()
                            }
                        }
                        break
                    }
                }
                continue
            }
        }

        // Apple Intelligence: suggest next steps after completion (skip for pure conversation)
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
