import Foundation
import AgentTools
import AgentLLM

extension AgentViewModel {
    // MARK: - Script Tabs

    func openScriptTab(scriptName: String, selectTab: Bool = true) -> ScriptTab {
        let tab = ScriptTab(scriptName: scriptName)
        // Inherit LLM config from the currently selected main tab
        if let selId = selectedTabId,
           let parent = self.tab(for: selId), parent.isMainTab
        {
            tab.parentTabId = parent.id
        }
        // Inherit project folder from current context (resolve to directory, not file)
        tab.projectFolder = Self.resolvedWorkingDirectory(self.projectFolder)
        scriptTabs.append(tab)
        if selectTab { selectedTabId = tab.id }
        persistScriptTabs()
        return tab
    }

    /// Create a new main tab with its own LLM provider/model.
    @discardableResult
    func createMainTab(config: LLMConfig) -> ScriptTab {
        // Number duplicate model names: glm-5, glm-5 2, glm-5 3, etc.
        var numberedConfig = config
        let baseName = config.displayName
        let existingCount = scriptTabs.filter { $0.scriptName.hasPrefix(baseName) && $0.isMainTab }.count
        if existingCount > 0 {
            numberedConfig.displayName = "\(baseName) \(existingCount + 1)"
        }
        let tab = ScriptTab(llmConfig: numberedConfig)
        // Inherit project folder from main tab (resolve to directory, not file)
        tab.projectFolder = Self.resolvedWorkingDirectory(self.projectFolder)
        scriptTabs.append(tab)
        selectedTabId = tab.id
        persistScriptTabs()
        return tab
    }

    /// Resolve the LLM provider and model for a given tab.
    /// Main tabs use their own config; script tabs inherit from parent; fallback to global.
    func resolvedLLMConfig(for tab: ScriptTab) -> (provider: APIProvider, model: String) {
        if let config = tab.llmConfig {
            return (config.provider, config.model)
        }
        if let parentId = tab.parentTabId,
           let parent = self.tab(for: parentId),
           let config = parent.llmConfig
        {
            return (config.provider, config.model)
        }
        return (selectedProvider, globalModelForProvider(selectedProvider))
    }

    /// Return the current global model ID for the given provider.
    func globalModelForProvider(_ provider: APIProvider) -> String {
        switch provider {
        case .claude: return selectedModel
        case .openAI: return openAIModel
        case .deepSeek: return deepSeekModel
        case .huggingFace: return huggingFaceModel
        case .ollama: return ollamaModel
        case .localOllama: return localOllamaModel
        case .vLLM: return vLLMModel
        case .lmStudio: return lmStudioModel
        case .zAI: return zAIModel.replacingOccurrences(of: ":v", with: "")
        case .bigModel: return bigModelModel.replacingOccurrences(of: ":v", with: "")
        case .qwen: return qwenModel
        case .gemini: return geminiModel
        case .grok: return grokModel
        case .mistral: return mistralModel
        case .codestral: return codestralModel
        case .vibe: return vibeModel
        case .foundationModel: return "Apple Intelligence"
        }
    }

    /// Return the API key for the given provider.
    func apiKeyForProvider(_ provider: APIProvider) -> String {
        switch provider {
        case .claude: return apiKey
        case .openAI: return openAIAPIKey
        case .deepSeek: return deepSeekAPIKey
        case .huggingFace: return huggingFaceAPIKey
        case .ollama: return ollamaAPIKey
        case .localOllama: return ""
        case .vLLM: return vLLMAPIKey
        case .lmStudio: return lmStudioAPIKey
        case .zAI: return zAIAPIKey
        case .bigModel: return bigModelAPIKey
        case .qwen: return qwenAPIKey
        case .gemini: return geminiAPIKey
        case .grok: return grokAPIKey
        case .mistral: return mistralAPIKey
        case .codestral: return codestralAPIKey
        case .vibe: return vibeAPIKey
        case .foundationModel: return ""
        }
    }

    /// Return the chat URL for the given provider from the LLM registry (single source of truth).
    /// Z.ai and BigModel swap to coding endpoint for non-vision models.
    func chatURLForProvider(_ provider: APIProvider) -> String {
        guard let url = LLMRegistry.shared.provider(provider.rawValue)?.endpoint.chatURL else { return "" }
        // Z.ai/BigModel: coding endpoint for text models, general endpoint for vision models
        // Non-coding (vision) models are tagged with ":v" suffix in their stored ID
        if provider == .zAI || provider == .bigModel {
            let raw = provider == .zAI ? zAIModel : bigModelModel
            let isVisionModel = raw.hasSuffix(":v")
            if !isVisionModel {
                return url.replacingOccurrences(of: "/api/paas/", with: "/api/coding/paas/")
            }
        }
        return url
    }

    /// Return a human-readable display name for a model ID given its provider.
    func modelDisplayName(provider: APIProvider, modelId: String) -> String {
        switch provider {
        case .claude:
            return availableClaudeModels.first(where: { $0.id == modelId })?.displayName ?? modelId
        case .openAI:
            return openAIModels.first(where: { $0.id == modelId })?.name
                ?? Self.defaultOpenAIModels.first(where: { $0.id == modelId })?.name ?? modelId
        case .deepSeek:
            return deepSeekModels.first(where: { $0.id == modelId })?.name
                ?? Self.defaultDeepSeekModels.first(where: { $0.id == modelId })?.name ?? modelId
        case .huggingFace:
            return huggingFaceModels.first(where: { $0.id == modelId })?.name
                ?? Self.defaultHuggingFaceModels.first(where: { $0.id == modelId })?.name ?? modelId
        case .ollama:
            return ollamaModels.first(where: { $0.id == modelId })?.name ?? modelId
        case .localOllama:
            return localOllamaModels.first(where: { $0.id == modelId })?.name ?? modelId
        case .vLLM:
            return vLLMModels.first(where: { $0.id == modelId })?.name ?? modelId
        case .lmStudio:
            return lmStudioModels.first(where: { $0.id == modelId })?.name ?? modelId
        case .zAI:
            return zAIModels.first(where: { $0.id == modelId })?.name
                ?? Self.defaultZAIModels.first(where: { $0.id == modelId })?.name ?? modelId
        case .bigModel:
            return modelId
        case .qwen:
            return modelId
        case .gemini:
            return geminiModels.first(where: { $0.id == modelId })?.name
                ?? Self.defaultGeminiModels.first(where: { $0.id == modelId })?.name ?? modelId
        case .grok:
            return grokModels.first(where: { $0.id == modelId })?.name
                ?? Self.defaultGrokModels.first(where: { $0.id == modelId })?.name ?? modelId
        case .mistral:
            return modelId
        case .codestral:
            return modelId
        case .vibe:
            return modelId
        case .foundationModel:
            return "Apple Intelligence"
        }
    }

    func closeScriptTab(id: UUID) {
        if let tab = tab(for: id) {
            // Stop LLM task and clear queue
            if tab.isLLMRunning || !tab.taskQueue.isEmpty {
                stopTabTask(tab: tab)
            }
            // Cancel running script
            if tab.isRunning {
                tab.isCancelled = true
                tab.cancelHandler?()
                tab.isRunning = false
            }
            tab.logFlushTask?.cancel()
            tab.llmStreamFlushTask?.cancel()
            // Clear log before removal — prevents expensive NSAttributedString copy on tab switch
            tab.activityLog = ""
            tab.rawLLMOutput = ""
        }
        if selectedTabId == id {
            if let idx = scriptTabs.firstIndex(where: { $0.id == id }) {
                if idx > 0 {
                    selectedTabId = scriptTabs[idx - 1].id
                } else if scriptTabs.count > 1 {
                    selectedTabId = scriptTabs[1].id
                } else {
                    selectedTabId = nil
                }
            } else {
                selectedTabId = nil
            }
        }
        scriptTabs.removeAll { $0.id == id }
        persistScriptTabs()
    }

    func cancelScriptTab(id: UUID) {
        guard let tab = tab(for: id) else { return }
        tab.isCancelled = true
        tab.cancelHandler?()
        tab.isRunning = false
        // Also cancel any running LLM task
        if tab.isLLMRunning {
            stopTabTask(tab: tab)
        }
    }

    func selectMainTab() {
        selectedTabId = nil
        persistScriptTabs()
    }

    /// Ensure an LLM tab is selected when a task comes in:
    /// 1. If currently on a main LLM tab, stay there
    /// 2. If on a script tab with a parent, switch to the parent LLM tab
    /// 3. Otherwise, switch to main tab
    func ensureLLMTabSelected() {
        if selectedTabId == nil {
            // Already on main tab
            return
        }

        guard let currentTab = selectedTab else {
            // Tab not found, go to main
            selectMainTab()
            return
        }

        if currentTab.isMainTab {
            // Already on an LLM main tab, stay there
            return
        }

        // On a script tab - find its parent
        if let parentId = currentTab.parentTabId,
           let parentTab = self.tab(for: parentId), parentTab.isMainTab
        {
            // Switch to parent LLM tab
            selectedTabId = parentTab.id
            persistScriptTabs()
        } else {
            // No parent found or not a main tab, go to main
            selectMainTab()
        }
    }

    // MARK: - Script Tab Persistence

    /// Save open script tabs: order/selected to UserDefaults, log data to SwiftData.
    func persistScriptTabs() {
        for tab in scriptTabs { tab.flush() }

        let ids = scriptTabs.map { $0.id.uuidString }
        UserDefaults.standard.set(ids, forKey: "agentScriptTabIds")
        UserDefaults.standard.set(selectedTabId?.uuidString, forKey: "agentSelectedTabId")

        let tabData = scriptTabs.map { tab in
            let configJSON: String? = {
                guard let config = tab.llmConfig,
                      let data = try? JSONEncoder().encode(config) else { return nil }
                return String(data: data, encoding: .utf8)
            }()
            let historyJSON: String? = {
                guard !tab.promptHistory.isEmpty,
                      let data = try? JSONEncoder().encode(tab.promptHistory) else { return nil }
                return String(data: data, encoding: .utf8)
            }()
            let summariesJSON: String? = {
                guard !tab.tabTaskSummaries.isEmpty,
                      let data = try? JSONEncoder().encode(tab.tabTaskSummaries) else { return nil }
                return String(data: data, encoding: .utf8)
            }()
            let tabErrorsJSON: String? = {
                guard !tab.tabErrors.isEmpty,
                      let data = try? JSONEncoder().encode(tab.tabErrors) else { return nil }
                return String(data: data, encoding: .utf8)
            }()
            return (
                id: tab.id,
                scriptName: tab.scriptName,
                activityLog: tab.activityLog,
                exitCode: tab.exitCode,
                llmConfigJSON: configJSON,
                parentTabIdString: tab.parentTabId?.uuidString,
                isMessagesTab: tab.isMessagesTab,
                projectFolder: tab.projectFolder,
                promptHistoryJSON: historyJSON,
                taskSummariesJSON: summariesJSON,
                errorsJSON: tabErrorsJSON,
                rawLLMOutput: tab.rawLLMOutput,
                lastElapsed: tab.lastElapsed,
                thinkingExpanded: tab.thinkingExpanded,
                thinkingOutputExpanded: tab.thinkingOutputExpanded,
                thinkingDismissed: tab.thinkingDismissed,
                tabInputTokens: tab.tabInputTokens,
                tabOutputTokens: tab.tabOutputTokens
            )
        }
        ChatHistoryStore.shared.saveScriptTabs(tabData)
    }

    /// Restore script tabs from UserDefaults (order) + SwiftData (data).
    func restoreScriptTabs() {
        guard let ids = UserDefaults.standard.stringArray(forKey: "agentScriptTabIds"),
              !ids.isEmpty else { return }

        let records = ChatHistoryStore.shared.fetchScriptTabs()
        let recordMap = Dictionary(records.compactMap { r in (r.tabId, r) }, uniquingKeysWith: { first, _ in first })

        for idStr in ids {
            guard let uuid = UUID(uuidString: idStr),
                  let record = recordMap[uuid] else { continue }
            let tab = ScriptTab(record: record)
            scriptTabs.append(tab)
        }

        // Always start on Main tab
        selectedTabId = nil
    }

}
