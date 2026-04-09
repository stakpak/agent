
@preconcurrency import Foundation
import AgentTools
import AgentAudit
import AgentLLM


// MARK: - Tab Task LLM Services

extension AgentViewModel {

    /// / Container for the LLM service instances used by a tab task run. / Only one of
    /// claude/openAICompatible/ollama/foundationModel is non-nil / at a time (matching the legacy if-else chain in executeTabTask).
    struct TabLLMServices {
        var claude: ClaudeService?
        var openAICompatible: OpenAICompatibleService?
        var ollama: OllamaService?
        var foundationModel: FoundationModelService?
    }

    /// / Build the tab's system history context string used as the LLM's / history context argument. Mirrors the inline
    /// block in the legacy / monolithic executeTabTask.
    func buildTabHistoryContext(tab: ScriptTab) -> String {
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
        return """

        \nYou are in a tab named "\(tab.scriptName)". The user can see the tab's output.
        \(tccNote)
        \(conversationNote)
        The tab's recent output is below for context:

        \(tabContext)
        """
    }

    /// / Build the initial message list for a tab task run. Applies the / assistant-trailing strip, optional direct
    /// command context injection, / and attached image handling.
    func buildTabInitialMessages(
        tab: ScriptTab,
        prompt: String,
        projectFolder: String,
        directCommandContext: String?
    ) -> [[String: Any]] {
        // Build on existing conversation or start fresh
        var messages: [[String: Any]] = tab.llmMessages

        // Remove trailing assistant messages — Ollama requires the last message to be user or tool role. Strip any
        // assistant messages at the end (orphaned tool calls or plain text from a previous session/restart).
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
        return messages
    }

    /// / Build LLM service instances for the given provider/model. Called at task / start and again when the fallback
    /// chain swaps providers mid-task. / Mirrors the legacy `buildLLMServices` nested closure.
    func buildTabLLMServices(
        provider: APIProvider,
        modelId: String,
        historyContext: String,
        projectFolder: String,
        maxTokens mt: Int
    ) -> TabLLMServices {
        var services = TabLLMServices()

        if provider == .claude {
            services.claude = ClaudeService(
                apiKey: apiKey,
                model: modelId,
                historyContext: historyContext,
                projectFolder: projectFolder,
                maxTokens: mt
            )
        } else if provider == .lmStudio && lmStudioProtocol == .anthropic {
            services.claude = ClaudeService(
                apiKey: lmStudioAPIKey,
                model: modelId,
                historyContext: historyContext,
                projectFolder: projectFolder,
                baseURL: lmStudioEndpoint,
                maxTokens: mt
            )
        } else {
            services.claude = nil
        }
        switch provider {
        case .claude, .ollama, .localOllama, .foundationModel:
            services.openAICompatible = nil
        case .lmStudio where lmStudioProtocol == .anthropic:
            services.openAICompatible = nil
        case .lmStudio:
            let key = lmStudioProtocol == .lmStudio ? "input" : "messages"
            services.openAICompatible = OpenAICompatibleService(
                apiKey: apiKeyForProvider(provider), model: modelId,
                baseURL: lmStudioEndpoint, historyContext: historyContext,
                projectFolder: projectFolder, provider: provider,
                messagesKey: key, maxTokens: mt
            )
        case .vLLM:
            services.openAICompatible = OpenAICompatibleService(
                apiKey: apiKeyForProvider(provider), model: modelId,
                baseURL: vLLMEndpoint, historyContext: historyContext,
                projectFolder: projectFolder, provider: provider,
                maxTokens: mt
            )
        default:
            let url = chatURLForProvider(provider)
            let vision = LLMRegistry.shared.provider(provider.rawValue)?.capabilities.contains(.vision) ?? false
            services.openAICompatible = url.isEmpty ? nil : OpenAICompatibleService(
                apiKey: apiKeyForProvider(provider), model: modelId,
                baseURL: url, supportsVision: vision || forceVision,
                historyContext: historyContext, projectFolder: projectFolder,
                provider: provider, maxTokens: mt
            )
        }
        switch provider {
        case .ollama:
            services.ollama = OllamaService(
                apiKey: ollamaAPIKey, model: modelId,
                endpoint: ollamaEndpoint,
                supportsVision: selectedOllamaSupportsVision || Self.isVisionModel(modelId),
                historyContext: historyContext, projectFolder: projectFolder,
                provider: .ollama
            )
        case .localOllama:
            services.ollama = OllamaService(
                apiKey: "", model: modelId, endpoint: localOllamaEndpoint,
                supportsVision: selectedLocalOllamaSupportsVision || Self.isVisionModel(modelId),
                historyContext: historyContext, projectFolder: projectFolder,
                provider: .localOllama, contextSize: localOllamaContextSize
            )
        default:
            services.ollama = nil
        }
        services.foundationModel = provider == .foundationModel
            ? FoundationModelService(historyContext: historyContext, projectFolder: projectFolder) : nil

        services.claude?.temperature = temperatureForProvider(.claude)
        services.ollama?.temperature = temperatureForProvider(provider)
        services.openAICompatible?.temperature = temperatureForProvider(provider)
        return services
    }
}
