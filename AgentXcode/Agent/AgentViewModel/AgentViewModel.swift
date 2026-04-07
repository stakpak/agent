@preconcurrency import Foundation
import AgentTools
import AgentColorSyntax
import AgentTerminalNeo
import AgentLLM
import AgentAccess
import AppKit
import SwiftUI
import SQLite3
import Speech
import AVFoundation
import FoundationModels

/// Per-tab LLM configuration for multi-main-tab support
struct LLMConfig: Codable {
    var provider: APIProvider
    var model: String
    var displayName: String
}

enum LMStudioProtocol: String, CaseIterable, Codable {
    case openAI = "openAI"
    case anthropic = "anthropic"
    case lmStudio = "lmStudio"

    var displayName: String {
        switch self {
        case .openAI: "OpenAI Compatible"
        case .anthropic: "Anthropic Compatible"
        case .lmStudio: "LM Studio Native"
        }
    }

    var defaultEndpoint: String {
        switch self {
        case .openAI: "http://localhost:1234/v1/chat/completions"
        case .anthropic: "http://localhost:1234/v1/messages"
        case .lmStudio: "http://localhost:1234/api/v1/chat"
        }
    }
}

enum PromptStyle: String, CaseIterable, Codable {
    case full
    case compact
    
    var displayName: String {
        switch self {
        case .full: "Full"
        case .compact: "Compact"
        }
    }
}

@MainActor @Observable
final class AgentViewModel {
    /// Stable UUID for the main tab — persisted across launches
    static let mainTabID: UUID = {
        let key = "agentMainTabUUID"
        if let str = UserDefaults.standard.string(forKey: key), let id = UUID(uuidString: str) {
            return id
        }
        let id = UUID()
        UserDefaults.standard.set(id.uuidString, forKey: key)
        return id
    }()

    var taskInput = ""
    /// Width of the task input field, updated by InputSectionView via GeometryReader
    var inputFieldWidth: CGFloat = 0

    // MARK: - Tool Steps (structured tool call tracking)

    /// A single tool invocation step for structured display
    struct ToolStep: Identifiable {
        let id = UUID()
        let name: String
        let detail: String
        let startTime: Date
        var duration: TimeInterval?
        var status: Status = .running

        enum Status {
            case running, success, error
        }
    }

    /// Tool steps for the current task (main tab)
    var toolSteps: [ToolStep] = []

    /// Record a tool step starting. Returns the step ID for later completion.
    @discardableResult
    func recordToolStep(name: String, detail: String) -> UUID {
        let step = ToolStep(name: name, detail: detail, startTime: Date())
        toolSteps.append(step)
        return step.id
    }

    /// Mark a tool step as completed.
    func completeToolStep(id: UUID, status: ToolStep.Status = .success) {
        if let idx = toolSteps.firstIndex(where: { $0.id == id }) {
            toolSteps[idx].duration = Date().timeIntervalSince(toolSteps[idx].startTime)
            toolSteps[idx].status = status
        }
    }

    // Stored property drives live UI; ChatHistoryStore persists across launches via SwiftData
    var activityLog = ""
    var isRunning = false
    var isThinking = false
    /// When true, only focused tool groups sent to LLM
    var codingModeEnabled = false
    var automationModeEnabled = false
    static let codingModeGroups = Tool.codingGroups
    static let automationModeGroups = Tool.automationGroups

    // MARK: - Coding Preferences (opt-in features)

    /// Auto-verify: after successful build, launch app and run accessibility checks
    var autoVerifyEnabled: Bool = UserDefaults.standard.bool(forKey: "codingAutoVerify") {
        didSet { UserDefaults.standard.set(autoVerifyEnabled, forKey: "codingAutoVerify") }
    }
    /// Visual test assertions: allow LLM to define click/verify UI tests
    var visualTestsEnabled: Bool = UserDefaults.standard.bool(forKey: "codingVisualTests") {
        didSet { UserDefaults.standard.set(visualTestsEnabled, forKey: "codingVisualTests") }
    }
    /// Auto PR: create branch, commit, push, open PR after task complete
    var autoPREnabled: Bool = UserDefaults.standard.bool(forKey: "codingAutoPR") {
        didSet { UserDefaults.standard.set(autoPREnabled, forKey: "codingAutoPR") }
    }
    /// Auto-scaffold: allow LLM to create new Xcode projects from templates
    var autoScaffoldEnabled: Bool = UserDefaults.standard.bool(forKey: "codingAutoScaffold") {
        didSet { UserDefaults.standard.set(autoScaffoldEnabled, forKey: "codingAutoScaffold") }
    }
    var thinkingDismissed: Bool = UserDefaults.standard.object(forKey: "thinkingDismissed") as? Bool ?? true {
        didSet { UserDefaults.standard.set(thinkingDismissed, forKey: "thinkingDismissed") }
    }
    var showThinkingIndicator: Bool = UserDefaults.standard.object(forKey: "showThinkingIndicator") as? Bool ?? true {
        didSet { UserDefaults.standard.set(showThinkingIndicator, forKey: "showThinkingIndicator") }
    }
    var thinkingExpanded: Bool = UserDefaults.standard.object(forKey: "thinkingExpanded") as? Bool ?? false {
        didSet { UserDefaults.standard.set(thinkingExpanded, forKey: "thinkingExpanded") }
    }
    var thinkingOutputExpanded: Bool = UserDefaults.standard.object(forKey: "thinkingOutputExpanded") as? Bool ?? false {
        didSet { UserDefaults.standard.set(thinkingOutputExpanded, forKey: "thinkingOutputExpanded") }
    }
    /// User's drag-resized height for the LLM Output HUD on the main tab. Persisted across launches.
    var llmOutputHeight: Double = UserDefaults.standard.object(forKey: "llmOutputHeight") as? Double ?? 80 {
        didSet { UserDefaults.standard.set(llmOutputHeight, forKey: "llmOutputHeight") }
    }
    var isListening = false
    var mainTaskStartDate: Date?
    var _mainTaskElapsedFrozen: TimeInterval = 0
    var mainTaskElapsed: TimeInterval {
        get {
            if let start = mainTaskStartDate, isRunning {
                return Date().timeIntervalSince(start)
            }
            return _mainTaskElapsedFrozen
        }
        set { _mainTaskElapsedFrozen = newValue }
    }

    // Failed agent alert
    var showFailedAgentAlert = false
    var failedAgentName = ""
    var failedAgentId: UUID?

    /// Call when an agent fails — triggers the remove-from-menu alert.
    /// Finds the most recent matching entry by name and stores its UUID for exact removal.
    func notifyAgentFailed(name: String, arguments: String) {
        if let entry = RecentAgentsService.shared.entries.first(where: { $0.agentName == name && $0.arguments == arguments })
            ?? RecentAgentsService.shared.entries.first(where: { $0.agentName == name }) {
            failedAgentName = name
            failedAgentId = entry.id
            showFailedAgentAlert = true
        }
    }

    // Token tracking
    var taskInputTokens: Int = 0
    var taskOutputTokens: Int = 0
    var sessionInputTokens: Int = 0
    var sessionOutputTokens: Int = 0

    /// Live budget usage fraction for UI display (updated by task loop)
    var budgetUsedFraction: Double = 0

    /// Active sub-agents spawned by the current task
    var subAgents: [SubAgent] = []

    /// AskUserQuestion — pending question for mid-task dialog
    var pendingQuestion: String = ""
    var pendingAnswer: String? = nil
    var userServiceActive = false
    var rootServiceActive = false
    var userWasActive = false
    var rootWasActive = false
    var userPingOK = false
    var daemonPingOK = false
    var userEnabled: Bool = UserDefaults.standard.object(forKey: "agentUserEnabled") as? Bool ?? true {
        didSet {
            UserDefaults.standard.set(userEnabled, forKey: "agentUserEnabled")
            if !userEnabled {
                userService.shutdownAgent()
                userPingOK = false
                appendLog("⚙️ User Agent shut down. Re-enable: Connect → Register.")
            }
            syncServicesGroup()
        }
    }
    var rootEnabled: Bool = UserDefaults.standard.object(forKey: "agentRootEnabled") as? Bool ?? true {
        didSet {
            UserDefaults.standard.set(rootEnabled, forKey: "agentRootEnabled")
            if !rootEnabled {
                helperService.shutdownDaemon()
                daemonPingOK = false
                appendLog("⚙️ Launch Daemon shut down. Re-enable: Connect → Register.")
            }
            syncServicesGroup()
        }
    }

    /// Keep service tool groups and individual tools in sync with userEnabled/rootEnabled.
    private func syncServicesGroup() {
        let prefs = ToolPreferencesService.shared
        // Sync User Agent group
        let agentGroupOn = prefs.isGroupEnabled(Tool.Group.user)
        if userEnabled != agentGroupOn { prefs.toggleGroup(Tool.Group.user) }
        // Sync individual tool — re-enable if service turned on
        if userEnabled && !prefs.isEnabled(selectedProvider, "execute_agent_command") {
            prefs.toggle(selectedProvider, "execute_agent_command")
        }
        // Sync Launch Daemon group
        let daemonGroupOn = prefs.isGroupEnabled(Tool.Group.root)
        if rootEnabled != daemonGroupOn { prefs.toggleGroup(Tool.Group.root) }
        // Sync individual tool — re-enable if service turned on
        if rootEnabled && !prefs.isEnabled(selectedProvider, "execute_daemon_command") {
            prefs.toggle(selectedProvider, "execute_daemon_command")
        }
    }

    /// CPU icon color: green = running, blue = configured, red = not configured
    var llmStatusColor: Color {
        let needsKey: Set<APIProvider> = [.claude, .openAI, .deepSeek, .huggingFace]
        if needsKey.contains(selectedProvider) && apiKey.isEmpty { return .red }
        // When running, use the active tab's color
        if isRunning || isThinking {
            if let selId = selectedTabId {
                return ContentView.tabColor(for: selId, in: scriptTabs)
            }
            return .blue
        }
        // Check if any tab is running
        if let runningTab = scriptTabs.first(where: { $0.isLLMRunning || $0.isLLMThinking }) {
            return ContentView.tabColor(for: runningTab.id, in: scriptTabs)
        }
        return .green
    }

    /// Gear icon color reflecting overall service health
    var servicesGearColor: Color {
        if !userEnabled && !rootEnabled { return .gray }
        if userEnabled && rootEnabled { return .green }
        return .yellow
    }

    /// Tool icon color reflecting tool accessibility
    var toolsIconColor: Color {
        let prefs = ToolPreferencesService.shared
        let all = AgentTools.tools(for: selectedProvider)
        let enabledCount = all.filter { prefs.isEnabled(selectedProvider, $0.name) }.count
        if enabledCount == 0 { return .red }
        if !userEnabled { return .yellow }
        if !rootEnabled { return .orange }
        return .green
    }

    /// Hand icon color reflecting accessibility status
    var accessibilityIconColor: Color {
        if !AccessibilityEnabled.shared.accessibilityGlobalEnabled { return .gray }
        if !AccessibilityService.hasAccessibilityPermission() { return .red }
        return .green
    }

    /// History icon color reflecting history state
    var historyIconColor: Color {
        let hasPrompts = !currentTabPromptHistory.isEmpty
        let hasTasks = !taskSummaries.isEmpty
        let hasErrors = !errorHistory.isEmpty
        if !hasPrompts && !hasTasks && !hasErrors { return Color.gray }
        if hasErrors { return .red }
        return .green
    }

    /// Options slider icon color based on temperature
    var optionsIconColor: Color {
        temperatureColor(temperatureForProvider(selectedProvider))
    }

    /// Color for temperature value: 0–0.3 green, 0.3–1.0 yellow, 1.0–1.5 orange, 1.5–2.0 red
    func temperatureColor(_ temp: Double) -> Color {
        if temp >= 1.5 { return .pink }
        if temp >= 1.0 { return .orange }
        if temp >= 0.5 { return .yellow }
        return .green
    }

    /// MCP server icon color based on connection and tool state
    var mcpIconColor: Color {
        let mcp = MCPService.shared
        let config = MCPServerRegistry.shared
        let servers = config.servers
        // No servers configured
        guard !servers.isEmpty else { return .gray }
        let connectedIds = mcp.connectedServerIds
        let tools = mcp.discoveredTools
        // No servers connected
        guard !connectedIds.isEmpty else { return .gray }
        // Check if all tools are disabled
        let enabledTools = tools.filter { mcp.isToolEnabled(serverName: $0.serverName, toolName: $0.name) }
        if enabledTools.isEmpty && !tools.isEmpty { return .red }
        // Check if some servers have errors or some tools disabled
        let hasErrors = !mcp.connectionErrors.isEmpty
        let someDisabled = enabledTools.count < tools.count
        if hasErrors || someDisabled { return .orange }
        // All good
        return .green
    }

    /// Tooltip for the gear icon
    var servicesGearHelp: String {
        let userStatus = userPingOK ? "connected" : (userEnabled ? "not responding" : "disabled")
        let rootStatus = daemonPingOK ? "connected" : (rootEnabled ? "not responding" : "disabled")
        return "Background Agents — Agent: \(userStatus), Daemon: \(rootStatus)"
    }

    var selectedProvider: APIProvider = {
        let rawValue = UserDefaults.standard.string(forKey: "agentProvider") ?? "ollama"
        let provider = APIProvider(rawValue: rawValue) ?? .ollama
        // foundationModel is NEVER a valid selection - it's for LoRA training only
        // If somehow stored, fall back to ollama
        return APIProvider.selectableProviders.contains(provider) ? provider : .ollama
    }() {
        didSet {
            // Ensure foundationModel can never be stored as selected provider
            guard APIProvider.selectableProviders.contains(selectedProvider) else {
                selectedProvider = .ollama
                return
            }
            UserDefaults.standard.set(selectedProvider.rawValue, forKey: "agentProvider")
            if selectedProvider == .ollama && ollamaModels.isEmpty {
                fetchOllamaModels()
            }
            if selectedProvider == .localOllama && localOllamaModels.isEmpty {
                fetchLocalOllamaModels()
            }
            if selectedProvider == .claude && availableClaudeModels.isEmpty {
                Task { await fetchClaudeModels() }
            }
            if selectedProvider == .openAI && openAIModels.isEmpty {
                fetchOpenAIModels()
            }
            if selectedProvider == .deepSeek && deepSeekModels.isEmpty {
                fetchDeepSeekModels()
            }
            if selectedProvider == .huggingFace && huggingFaceModels.isEmpty {
                fetchHuggingFaceModels()
            }
            if selectedProvider == .vLLM && vLLMModels.isEmpty {
                fetchVLLMModels()
            }
            if selectedProvider == .lmStudio && lmStudioModels.isEmpty {
                fetchLMStudioModels()
            }
            if selectedProvider == .zAI && zAIModels.isEmpty {
                fetchZAIModels()
            }
            if selectedProvider == .gemini && geminiModels.isEmpty {
                fetchGeminiModels()
            }
            if selectedProvider == .grok && grokModels.isEmpty {
                fetchGrokModels()
            }
        }
    }

    // Claude settings - stored securely in Keychain
    var apiKey: String = KeychainService.shared.getClaudeAPIKey() ?? "" {
        didSet { KeychainService.shared.setClaudeAPIKey(apiKey) }
    }

    var selectedModel: String = UserDefaults.standard.string(forKey: "agentModel") ?? "claude-sonnet-4-20250514" {
        didSet { UserDefaults.standard.set(selectedModel, forKey: "agentModel") }
    }

    // Ollama settings - API key stored securely in Keychain
    var ollamaAPIKey: String = KeychainService.shared.getOllamaAPIKey() ?? "" {
        didSet { KeychainService.shared.setOllamaAPIKey(ollamaAPIKey) }
    }

    // Tavily web search API key (available for all providers)
    var tavilyAPIKey: String = KeychainService.shared.getTavilyAPIKey() ?? "" {
        didSet { KeychainService.shared.setTavilyAPIKey(tavilyAPIKey) }
    }

    let ollamaEndpoint = "https://ollama.com/api/chat"

    // OpenAI settings
    var openAIAPIKey: String = KeychainService.shared.getOpenAIAPIKey() ?? "" {
        didSet { KeychainService.shared.setOpenAIAPIKey(openAIAPIKey) }
    }

    var openAIModel: String = UserDefaults.standard.string(forKey: "openAIModel") ?? "gpt-4.1-nano" {
        didSet { UserDefaults.standard.set(openAIModel, forKey: "openAIModel") }
    }

    struct OpenAIModelInfo: Identifiable {
        let id: String
        let name: String
    }

    var openAIModels: [OpenAIModelInfo] = []
    var isFetchingOpenAIModels = false

    nonisolated static let defaultOpenAIModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "gpt-4.1-nano", name: "GPT-4.1 Nano"),
        OpenAIModelInfo(id: "gpt-4.1-mini", name: "GPT-4.1 Mini"),
        OpenAIModelInfo(id: "gpt-4.1", name: "GPT-4.1"),
        OpenAIModelInfo(id: "gpt-4o-mini", name: "GPT-4o Mini"),
        OpenAIModelInfo(id: "gpt-4o", name: "GPT-4o"),
        OpenAIModelInfo(id: "o4-mini", name: "o4-mini"),
        OpenAIModelInfo(id: "o3-mini", name: "o3-mini"),
        OpenAIModelInfo(id: "o3", name: "o3"),
    ]

    // DeepSeek settings
    var deepSeekAPIKey: String = KeychainService.shared.getDeepSeekAPIKey() ?? "" {
        didSet { KeychainService.shared.setDeepSeekAPIKey(deepSeekAPIKey) }
    }

    var deepSeekModel: String = UserDefaults.standard.string(forKey: "deepSeekModel") ?? "deepseek-chat" {
        didSet { UserDefaults.standard.set(deepSeekModel, forKey: "deepSeekModel") }
    }

    nonisolated static let defaultDeepSeekModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "deepseek-chat", name: "DeepSeek Chat (V3)"),
        OpenAIModelInfo(id: "deepseek-reasoner", name: "DeepSeek Reasoner (R1)"),
    ]

    var deepSeekModels: [OpenAIModelInfo] = []
    var isFetchingDeepSeekModels = false

    // Hugging Face settings
    var huggingFaceAPIKey: String = KeychainService.shared.getHuggingFaceAPIKey() ?? "" {
        didSet { KeychainService.shared.setHuggingFaceAPIKey(huggingFaceAPIKey) }
    }

    var huggingFaceModel: String = UserDefaults.standard.string(forKey: "huggingFaceModel") ?? "deepseek-ai/DeepSeek-V3-0324" {
        didSet { UserDefaults.standard.set(huggingFaceModel, forKey: "huggingFaceModel") }
    }

    var huggingFaceModels: [OpenAIModelInfo] = []
    var isFetchingHuggingFaceModels = false

    // vLLM settings
    var vLLMAPIKey: String = KeychainService.shared.getVLLMAPIKey() ?? "" {
        didSet { KeychainService.shared.setVLLMAPIKey(vLLMAPIKey) }
    }

    var vLLMEndpoint: String = UserDefaults.standard.string(forKey: "vLLMEndpoint") ?? "http://localhost:8000/v1/chat/completions" {
        didSet { UserDefaults.standard.set(vLLMEndpoint, forKey: "vLLMEndpoint") }
    }

    var vLLMModel: String = UserDefaults.standard.string(forKey: "vLLMModel") ?? "" {
        didSet { UserDefaults.standard.set(vLLMModel, forKey: "vLLMModel") }
    }

    var vLLMModels: [OpenAIModelInfo] = []
    var isFetchingVLLMModels = false

    // LM Studio settings
    var lmStudioProtocol: LMStudioProtocol = {
        let raw = UserDefaults.standard.string(forKey: "lmStudioProtocol") ?? "openAI"
        return LMStudioProtocol(rawValue: raw) ?? .openAI
    }() {
        didSet {
            UserDefaults.standard.set(lmStudioProtocol.rawValue, forKey: "lmStudioProtocol")
            lmStudioEndpoint = lmStudioProtocol.defaultEndpoint
        }
    }

    var lmStudioEndpoint: String = UserDefaults.standard.string(forKey: "lmStudioEndpoint") ?? "http://localhost:1234/v1/chat/completions" {
        didSet { UserDefaults.standard.set(lmStudioEndpoint, forKey: "lmStudioEndpoint") }
    }

    var lmStudioModel: String = UserDefaults.standard.string(forKey: "lmStudioModel") ?? "" {
        didSet { UserDefaults.standard.set(lmStudioModel, forKey: "lmStudioModel") }
    }

    var lmStudioAPIKey: String = UserDefaults.standard.string(forKey: "lmStudioAPIKey") ?? "" {
        didSet { UserDefaults.standard.set(lmStudioAPIKey, forKey: "lmStudioAPIKey") }
    }

    var lmStudioModels: [OpenAIModelInfo] = []
    var isFetchingLMStudioModels = false

    // Z.ai (ZhipuAI GLM) settings
    var zAIAPIKey: String = KeychainService.shared.getZAIAPIKey() ?? "" {
        didSet { KeychainService.shared.setZAIAPIKey(zAIAPIKey) }
    }

    var zAIModel: String = UserDefaults.standard.string(forKey: "zAIModel") ?? "glm-4.7" {
        didSet { UserDefaults.standard.set(zAIModel, forKey: "zAIModel") }
    }

    nonisolated static let defaultZAIModels: [OpenAIModelInfo] = [
        // Coding models (use /api/coding/paas/ endpoint)
        OpenAIModelInfo(id: "glm-5.1", name: "GLM-5.1"),
        OpenAIModelInfo(id: "glm-5", name: "GLM-5"),
        OpenAIModelInfo(id: "glm-5-turbo", name: "GLM-5 Turbo"),
        OpenAIModelInfo(id: "glm-4.7", name: "GLM-4.7"),
        OpenAIModelInfo(id: "glm-4.7-flash", name: "GLM-4.7 Flash"),
        OpenAIModelInfo(id: "glm-4.6", name: "GLM-4.6"),
        OpenAIModelInfo(id: "glm-4.5", name: "GLM-4.5"),
        OpenAIModelInfo(id: "glm-4.5-air", name: "GLM-4.5 Air"),
        OpenAIModelInfo(id: "glm-4.5-flash", name: "GLM-4.5 Flash"),
        OpenAIModelInfo(id: "glm-4-32b-0414-128k", name: "GLM-4-32B-128K"),
        // Non-coding / general models (use /api/paas/ endpoint)
        // Tagged with :v suffix — stripped before sending to API
        OpenAIModelInfo(id: "glm-5.1:v", name: "GLM-5.1"),
        OpenAIModelInfo(id: "glm-5:v", name: "GLM-5"),
        OpenAIModelInfo(id: "glm-5-turbo:v", name: "GLM-5 Turbo"),
        OpenAIModelInfo(id: "glm-4.7:v", name: "GLM-4.7"),
        OpenAIModelInfo(id: "glm-4.7-flash:v", name: "GLM-4.7 Flash"),
        OpenAIModelInfo(id: "glm-4.6:v", name: "GLM-4.6"),
        OpenAIModelInfo(id: "glm-4.5:v", name: "GLM-4.5"),
        OpenAIModelInfo(id: "glm-4.5-air:v", name: "GLM-4.5 Air"),
        OpenAIModelInfo(id: "glm-4.5-flash:v", name: "GLM-4.5 Flash"),
        // Vision models (use /api/paas/ endpoint, vision-capable)
        OpenAIModelInfo(id: "glm-5v-turbo:v", name: "GLM-5V-Turbo (Vision)"),
        OpenAIModelInfo(id: "glm-4.6v:v", name: "GLM-4.6V (Vision)"),
        OpenAIModelInfo(id: "glm-4.5v:v", name: "GLM-4.5V (Vision)"),
        OpenAIModelInfo(id: "glm-ocr:v", name: "GLM-OCR"),
        // Image/Video/Voice models
        OpenAIModelInfo(id: "glm-image:v", name: "GLM-Image"),
        OpenAIModelInfo(id: "cogvideox-3:v", name: "CogVideoX-3"),
        OpenAIModelInfo(id: "glm-asr-2512:v", name: "GLM-ASR-2512 (Voice)"),
    ]

    var zAIModels: [OpenAIModelInfo] = []
    var isFetchingZAIModels = false

    // MARK: - BigModel (China)

    var bigModelAPIKey: String = KeychainService.shared.getBigModelAPIKey() ?? "" {
        didSet { KeychainService.shared.setBigModelAPIKey(bigModelAPIKey) }
    }

    var bigModelModel: String = UserDefaults.standard.string(forKey: "bigModelModel") ?? "glm-4.7" {
        didSet { UserDefaults.standard.set(bigModelModel, forKey: "bigModelModel") }
    }

    // MARK: - Qwen (Alibaba DashScope)

    var qwenAPIKey: String = KeychainService.shared.getQwenAPIKey() ?? "" {
        didSet { KeychainService.shared.setQwenAPIKey(qwenAPIKey) }
    }

    var qwenModel: String = UserDefaults.standard.string(forKey: "qwenModel") ?? "qwen-plus" {
        didSet { UserDefaults.standard.set(qwenModel, forKey: "qwenModel") }
    }

    nonisolated static let defaultQwenModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "qwen-plus", name: "Qwen Plus"),
        OpenAIModelInfo(id: "qwen-max", name: "Qwen Max"),
        OpenAIModelInfo(id: "qwen-turbo", name: "Qwen Turbo"),
        OpenAIModelInfo(id: "qwen-long", name: "Qwen Long"),
        OpenAIModelInfo(id: "qwen-vl-plus", name: "Qwen VL Plus"),
        OpenAIModelInfo(id: "qwen-vl-max", name: "Qwen VL Max"),
        OpenAIModelInfo(id: "qwen-coder-plus", name: "Qwen Coder Plus"),
    ]

    var qwenModels: [OpenAIModelInfo] = []
    var isFetchingQwenModels = false

    // MARK: - Google Gemini

    var geminiAPIKey: String = KeychainService.shared.getGeminiAPIKey() ?? "" {
        didSet { KeychainService.shared.setGeminiAPIKey(geminiAPIKey) }
    }

    var geminiModel: String = UserDefaults.standard.string(forKey: "geminiModel") ?? "gemini-2.5-flash" {
        didSet { UserDefaults.standard.set(geminiModel, forKey: "geminiModel") }
    }

    nonisolated static let defaultGeminiModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "gemini-2.5-pro-preview-05-06", name: "Gemini 2.5 Pro"),
        OpenAIModelInfo(id: "gemini-2.5-flash-preview-05-20", name: "Gemini 2.5 Flash"),
        OpenAIModelInfo(id: "gemini-2.5-flash", name: "Gemini 2.5 Flash (Stable)"),
        OpenAIModelInfo(id: "gemini-2.0-flash", name: "Gemini 2.0 Flash"),
    ]

    var geminiModels: [OpenAIModelInfo] = []
    var isFetchingGeminiModels = false

    // MARK: - Grok (xAI)

    var grokAPIKey: String = KeychainService.shared.getGrokAPIKey() ?? "" {
        didSet { KeychainService.shared.setGrokAPIKey(grokAPIKey) }
    }

    var grokModel: String = UserDefaults.standard.string(forKey: "grokModel") ?? "grok-3-mini-fast" {
        didSet { UserDefaults.standard.set(grokModel, forKey: "grokModel") }
    }

    nonisolated static let defaultGrokModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "grok-3", name: "Grok 3"),
        OpenAIModelInfo(id: "grok-3-fast", name: "Grok 3 Fast"),
        OpenAIModelInfo(id: "grok-3-mini", name: "Grok 3 Mini"),
        OpenAIModelInfo(id: "grok-3-mini-fast", name: "Grok 3 Mini Fast"),
    ]

    var grokModels: [OpenAIModelInfo] = []
    var isFetchingGrokModels = false

    // MARK: - Mistral

    var mistralAPIKey: String = KeychainService.shared.getMistralAPIKey() ?? "" {
        didSet { KeychainService.shared.setMistralAPIKey(mistralAPIKey) }
    }

    var mistralModel: String = UserDefaults.standard.string(forKey: "mistralModel") ?? "mistral-large-latest" {
        didSet { UserDefaults.standard.set(mistralModel, forKey: "mistralModel") }
    }

    nonisolated static let defaultMistralModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "mistral-large-latest", name: "Mistral Large"),
        OpenAIModelInfo(id: "mistral-small-latest", name: "Mistral Small"),
        OpenAIModelInfo(id: "codestral-latest", name: "Codestral"),
        OpenAIModelInfo(id: "mistral-medium-latest", name: "Mistral Medium"),
    ]

    var mistralModels: [OpenAIModelInfo] = []
    var isFetchingMistralModels = false

    // MARK: - Codestral (codestral.mistral.ai)

    var codestralAPIKey: String = KeychainService.shared.getCodestralAPIKey() ?? "" {
        didSet { KeychainService.shared.setCodestralAPIKey(codestralAPIKey) }
    }

    var codestralModel: String = UserDefaults.standard.string(forKey: "codestralModel") ?? "codestral-latest" {
        didSet { UserDefaults.standard.set(codestralModel, forKey: "codestralModel") }
    }

    var codestralModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "codestral-latest", name: "Codestral Latest"),
        OpenAIModelInfo(id: "codestral-2508", name: "Codestral 25.08"),
    ]
    var isFetchingCodestralModels = false

    nonisolated static let defaultCodestralModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "codestral-latest", name: "Codestral Latest"),
        OpenAIModelInfo(id: "codestral-2508", name: "Codestral 25.08"),
    ]

    // MARK: - Mistral Vibe (api.mistral.ai with Vibe key, Devstral models)

    var vibeAPIKey: String = KeychainService.shared.getVibeAPIKey() ?? "" {
        didSet { KeychainService.shared.setVibeAPIKey(vibeAPIKey) }
    }

    var vibeModel: String = UserDefaults.standard.string(forKey: "vibeModel") ?? "devstral-latest" {
        didSet { UserDefaults.standard.set(vibeModel, forKey: "vibeModel") }
    }

    var vibeModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "devstral-latest", name: "Devstral Latest"),
        OpenAIModelInfo(id: "devstral-medium-latest", name: "Devstral Medium Latest"),
    ]
    var isFetchingVibeModels = false

    nonisolated static let defaultVibeModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "devstral-latest", name: "Devstral Latest"),
        OpenAIModelInfo(id: "devstral-medium-latest", name: "Devstral Medium Latest"),
    ]

    nonisolated static let defaultHuggingFaceModels: [OpenAIModelInfo] = [
        OpenAIModelInfo(id: "deepseek-ai/DeepSeek-V3-0324", name: "DeepSeek V3"),
        OpenAIModelInfo(id: "deepseek-ai/DeepSeek-R1", name: "DeepSeek R1"),
        OpenAIModelInfo(id: "Qwen/Qwen2.5-Coder-32B-Instruct", name: "Qwen 2.5 Coder 32B"),
        OpenAIModelInfo(id: "meta-llama/Llama-3.3-70B-Instruct", name: "Llama 3.3 70B"),
        OpenAIModelInfo(id: "mistralai/Mistral-Small-24B-Instruct-2501", name: "Mistral Small 24B"),
    ]

    /// Detect vision-capable models by name patterns
    /// Auto-detect vision-capable models by name keywords.
    /// Sources: ollama.com/search?c=vision, OpenAI docs, Anthropic docs
    nonisolated static func isVisionModel(_ model: String) -> Bool {
        let lower = model.lowercased()
        let visionKeywords = [
            // Ollama vision models (from ollama.com/search?c=vision)
            "llava", "llava-llama3", "bakllava", "minicpm-v",
            "gemma3", "gemma4", "gemma-3", "gemma-4",
            "qwen-vl", "qwen2.5vl", "qwen2.5-vl", "qwen3-vl", "qwen3.5",
            "llama3.2-vision", "llama-3.2-vision", "llama4",
            "mistral-small3.1", "mistral-small3.2", "mistral-large-3",
            "kimi-k2.5", "gemini-3-flash", "glm-ocr", "deepseek-ocr",
            "ministral-3", "devstral-small-2",
            // General vision keywords
            "vision", "-vl", "cogvlm", "internvl", "pixtral", "molmo",
            "phi-3-vision", "phi-3.5-vision", "phi-4", "idefics", "fuyu",
            // Cloud API vision models
            "gpt-4o", "gpt-4-turbo", "gpt-4-vision", "claude",
            "glm-4v", "glm-4.5v", "glm-4.6v", "glm-5v", "deepseek-vl",
        ]
        return visionKeywords.contains { lower.contains($0) }
    }

    var maxHistoryBeforeSummary: Int = UserDefaults.standard.object(forKey: "agentMaxHistory") as? Int ?? 10 {
        didSet { UserDefaults.standard.set(maxHistoryBeforeSummary, forKey: "agentMaxHistory") }
    }

    var visibleTaskCount: Int = UserDefaults.standard.object(forKey: "agentVisibleTasks") as? Int ?? 3 {
        didSet { UserDefaults.standard.set(visibleTaskCount, forKey: "agentVisibleTasks") }
    }

    static let iterationOptions = [25, 50, 100, 200, 400, 800, 1600]

    var maxIterations: Int = UserDefaults.standard.object(forKey: "agentMaxIterations") as? Int ?? 50 {
        didSet { UserDefaults.standard.set(maxIterations, forKey: "agentMaxIterations") }
    }

    static let retryOptions = [1, 2, 3, 5, 10, 15, 20]

    var maxRetries: Int = UserDefaults.standard.object(forKey: "agentMaxRetries") as? Int ?? 10 {
        didSet { UserDefaults.standard.set(maxRetries, forKey: "agentMaxRetries") }
    }
    var networkRetryDelay: Int = UserDefaults.standard.object(forKey: "agentNetworkRetryDelay") as? Int ?? 60 {
        didSet { UserDefaults.standard.set(networkRetryDelay, forKey: "agentNetworkRetryDelay") }
    }

    // MARK: - Temperature per provider
    var claudeTemperature: Double = UserDefaults.standard.object(forKey: "claudeTemperature") as? Double ?? 0.2 {
        didSet { UserDefaults.standard.set(claudeTemperature, forKey: "claudeTemperature") }
    }
    var ollamaTemperature: Double = UserDefaults.standard.object(forKey: "ollamaTemperature") as? Double ?? 0.2 {
        didSet { UserDefaults.standard.set(ollamaTemperature, forKey: "ollamaTemperature") }
    }
    var openAITemperature: Double = UserDefaults.standard.object(forKey: "openAITemperature") as? Double ?? 0.2 {
        didSet { UserDefaults.standard.set(openAITemperature, forKey: "openAITemperature") }
    }
    var deepSeekTemperature: Double = UserDefaults.standard.object(forKey: "deepSeekTemperature") as? Double ?? 0.2 {
        didSet { UserDefaults.standard.set(deepSeekTemperature, forKey: "deepSeekTemperature") }
    }
    var huggingFaceTemperature: Double = UserDefaults.standard.object(forKey: "huggingFaceTemperature") as? Double ?? 0.2 {
        didSet { UserDefaults.standard.set(huggingFaceTemperature, forKey: "huggingFaceTemperature") }
    }
    var localOllamaTemperature: Double = UserDefaults.standard.object(forKey: "localOllamaTemperature") as? Double ?? 0.2 {
        didSet { UserDefaults.standard.set(localOllamaTemperature, forKey: "localOllamaTemperature") }
    }
    /// Context window size for local Ollama. 0 = let model decide.
    var localOllamaContextSize: Int = UserDefaults.standard.object(forKey: "localOllamaContextSize") as? Int ?? 0 {
        didSet { UserDefaults.standard.set(localOllamaContextSize, forKey: "localOllamaContextSize") }
    }
    var vLLMTemperature: Double = UserDefaults.standard.object(forKey: "vLLMTemperature") as? Double ?? 0.2 {
        didSet { UserDefaults.standard.set(vLLMTemperature, forKey: "vLLMTemperature") }
    }
    var lmStudioTemperature: Double = UserDefaults.standard.object(forKey: "lmStudioTemperature") as? Double ?? 0.2 {
        didSet { UserDefaults.standard.set(lmStudioTemperature, forKey: "lmStudioTemperature") }
    }
    var zAITemperature: Double = UserDefaults.standard.object(forKey: "zAITemperature") as? Double ?? 0.2 {
        didSet { UserDefaults.standard.set(zAITemperature, forKey: "zAITemperature") }
    }
    var geminiTemperature: Double = UserDefaults.standard.object(forKey: "geminiTemperature") as? Double ?? 0.2 {
        didSet { UserDefaults.standard.set(geminiTemperature, forKey: "geminiTemperature") }
    }
    var grokTemperature: Double = UserDefaults.standard.object(forKey: "grokTemperature") as? Double ?? 0.2 {
        didSet { UserDefaults.standard.set(grokTemperature, forKey: "grokTemperature") }
    }

    /// Current provider's temperature value.
    var currentTemperature: Double { temperatureForProvider(selectedProvider) }

    /// Get temperature for the current provider.
    func temperatureForProvider(_ provider: APIProvider) -> Double {
        switch provider {
        case .claude: return claudeTemperature
        case .ollama: return ollamaTemperature
        case .openAI: return openAITemperature
        case .deepSeek: return deepSeekTemperature
        case .huggingFace: return huggingFaceTemperature
        case .localOllama: return localOllamaTemperature
        case .vLLM: return vLLMTemperature
        case .lmStudio: return lmStudioTemperature
        case .zAI: return zAITemperature
        case .bigModel: return zAITemperature
        case .qwen: return openAITemperature
        case .gemini: return geminiTemperature
        case .grok: return grokTemperature
        case .mistral: return openAITemperature
        case .codestral: return openAITemperature
        case .vibe: return openAITemperature
        case .foundationModel: return claudeTemperature
        }
    }

    /// Max output tokens per provider. 0 = let provider decide (omit from request).
    /// Claude API requires max_tokens so 0 defaults to 16384 at the service level.
    var maxTokens: Int = UserDefaults.standard.object(forKey: "maxTokens") as? Int ?? 0 {
        didSet { UserDefaults.standard.set(maxTokens, forKey: "maxTokens") }
    }

    /// Per-task token budget ceiling (input+output). 0 = unlimited (default).
    /// When set, the task loop will nudge the LLM at 90% and auto-stop at 100% or on diminishing returns.
    var tokenBudgetCeiling: Int = UserDefaults.standard.object(forKey: "tokenBudgetCeiling") as? Int ?? 0 {
        didSet { UserDefaults.standard.set(tokenBudgetCeiling, forKey: "tokenBudgetCeiling") }
    }

    var ollamaModel: String = UserDefaults.standard.string(forKey: "ollamaModel") ?? "" {
        didSet {
            UserDefaults.standard.set(ollamaModel, forKey: "ollamaModel")
            if !ollamaModel.isEmpty && oldValue != ollamaModel {
                let vision = selectedOllamaSupportsVision ? " (vision)" : ""
                appendLog("🔄\(ollamaModel)\(vision)")
                flushLog()
            }
        }
    }

    struct OllamaModelInfo: Identifiable {
        let id: String // same as name
        let name: String
        let supportsVision: Bool
    }

    nonisolated static let defaultOllamaModels: [OllamaModelInfo] = [
        OllamaModelInfo(id: "nemotron-3-super", name: "nemotron-3-super", supportsVision: false),
        OllamaModelInfo(id: "qwen3.5:397b", name: "qwen3.5:397b", supportsVision: false),
        OllamaModelInfo(id: "minimax-m2.5", name: "minimax-m2.5", supportsVision: false),
        OllamaModelInfo(id: "glm-5", name: "glm-5", supportsVision: false),
        OllamaModelInfo(id: "kimi-k2.5", name: "kimi-k2.5", supportsVision: true),
        OllamaModelInfo(id: "glm-4.7", name: "glm-4.7", supportsVision: false),
        OllamaModelInfo(id: "minimax-m2.1", name: "minimax-m2.1", supportsVision: false),
        OllamaModelInfo(id: "gemini-3-flash-preview", name: "gemini-3-flash-preview", supportsVision: true),
        OllamaModelInfo(id: "nemotron-3-nano:30b", name: "nemotron-3-nano:30b", supportsVision: false),
        OllamaModelInfo(id: "devstral-small-2:24b", name: "devstral-small-2:24b", supportsVision: false),
        OllamaModelInfo(id: "devstral-2:123b", name: "devstral-2:123b", supportsVision: false),
        OllamaModelInfo(id: "ministral-3:8b", name: "ministral-3:8b", supportsVision: false),
        OllamaModelInfo(id: "ministral-3:14b", name: "ministral-3:14b", supportsVision: false),
        OllamaModelInfo(id: "deepseek-v3.2", name: "deepseek-v3.2", supportsVision: false),
        OllamaModelInfo(id: "mistral-large-3:675b", name: "mistral-large-3:675b", supportsVision: false),
        OllamaModelInfo(id: "deepseek-v3.1:671b", name: "deepseek-v3.1:671b", supportsVision: false),
        OllamaModelInfo(id: "cogito-2.1:671b", name: "cogito-2.1:671b", supportsVision: false),
        OllamaModelInfo(id: "minimax-m2", name: "minimax-m2", supportsVision: false),
        OllamaModelInfo(id: "glm-4.6", name: "glm-4.6", supportsVision: false),
        OllamaModelInfo(id: "qwen3-vl:235b-instruct", name: "qwen3-vl:235b-instruct", supportsVision: true),
        OllamaModelInfo(id: "qwen3-vl:235b", name: "qwen3-vl:235b", supportsVision: true),
        OllamaModelInfo(id: "qwen3-next:80b", name: "qwen3-next:80b", supportsVision: false),
        OllamaModelInfo(id: "kimi-k2:1t", name: "kimi-k2:1t", supportsVision: false),
        OllamaModelInfo(id: "gpt-oss:120b", name: "gpt-oss:120b", supportsVision: false),
        OllamaModelInfo(id: "qwen3-coder:480b", name: "qwen3-coder:480b", supportsVision: false),
        OllamaModelInfo(id: "gemma3:27b", name: "gemma3:27b", supportsVision: true),
        OllamaModelInfo(id: "gemma3:12b", name: "gemma3:12b", supportsVision: true),
        OllamaModelInfo(id: "gemma3:4b", name: "gemma3:4b", supportsVision: true),
        OllamaModelInfo(id: "qwen3-coder-next", name: "qwen3-coder-next", supportsVision: false),
        OllamaModelInfo(id: "gpt-oss:20b", name: "gpt-oss:20b", supportsVision: false)

    ]
    // MARK: - Claude Models

    struct ClaudeModelInfo: Identifiable, Codable {
        let id: String
        let name: String
        let displayName: String
        let createdAt: String?
        let description: String?

        var formattedDisplayName: String {
            if let created = createdAt {
                let dateStr = String(created.prefix(10))
                return "\(displayName) (\(dateStr))"
            }
            return displayName
        }
    }

    var availableClaudeModels: [ClaudeModelInfo] = []

    nonisolated static let defaultClaudeModels: [ClaudeModelInfo] = [
        ClaudeModelInfo(id: "claude-sonnet-4-6", name: "claude-sonnet-4-6", displayName: "Claude Sonnet 4.6", createdAt: "2026-02-17", description: nil),
        ClaudeModelInfo(id: "claude-opus-4-6", name: "claude-opus-4-6", displayName: "Claude Opus 4.6", createdAt: "2026-02-04", description: nil),
        ClaudeModelInfo(id: "claude-opus-4-5-20251101", name: "claude-opus-4-5-20251101", displayName: "Claude Opus 4.5", createdAt: "2025-11-24", description: nil),
        ClaudeModelInfo(id: "claude-haiku-4-5-20251001", name: "claude-haiku-4-5-20251001", displayName: "Claude Haiku 4.5", createdAt: "2025-10-15", description: nil),
        ClaudeModelInfo(id: "claude-sonnet-4-5-20250929", name: "claude-sonnet-4-5-20250929", displayName: "Claude Sonnet 4.5", createdAt: "2025-09-29", description: nil),
        ClaudeModelInfo(id: "claude-opus-4-1-20250805", name: "claude-opus-4-1-20250805", displayName: "Claude Opus 4.1", createdAt: "2025-08-05", description: nil),
        ClaudeModelInfo(id: "claude-opus-4-20250514", name: "claude-opus-4-20250514", displayName: "Claude Opus 4", createdAt: "2025-05-22", description: nil),
        ClaudeModelInfo(id: "claude-sonnet-4-20250514", name: "claude-sonnet-4-20250514", displayName: "Claude Sonnet 4", createdAt: "2025-05-22", description: nil),
        ClaudeModelInfo(id: "claude-3-haiku-20240307", name: "claude-3-haiku-20240307", displayName: "Claude Haiku 3", createdAt: "2024-03-07", description: nil)
    ]

    var ollamaModels: [OllamaModelInfo] = []
    var isFetchingModels = false

    var selectedOllamaSupportsVision: Bool {
        ollamaModels.first(where: { $0.name == ollamaModel })?.supportsVision ?? false
    }

    // Local Ollama settings
    var localOllamaEndpoint: String = UserDefaults.standard.string(forKey: "localOllamaEndpoint") ?? "http://localhost:11434/api/chat" {
        didSet { UserDefaults.standard.set(localOllamaEndpoint, forKey: "localOllamaEndpoint") }
    }

    var localOllamaModel: String = UserDefaults.standard.string(forKey: "localOllamaModel") ?? "" {
        didSet {
            UserDefaults.standard.set(localOllamaModel, forKey: "localOllamaModel")
            if !localOllamaModel.isEmpty && oldValue != localOllamaModel {
                let vision = selectedLocalOllamaSupportsVision ? " (vision)" : ""
                appendLog("🔄\(localOllamaModel)\(vision)")
                flushLog()
            }
        }
    }

    var localOllamaModels: [OllamaModelInfo] = []
    var isFetchingLocalModels = false

    var selectedLocalOllamaSupportsVision: Bool {
        localOllamaModels.first(where: { $0.name == localOllamaModel })?.supportsVision ?? false
    }

    var projectFolder: String = UserDefaults.standard.string(forKey: "agentProjectFolder") ?? "" {
        didSet { UserDefaults.standard.set(projectFolder, forKey: "agentProjectFolder") }
    }

    var attachedImages: [NSImage] = []
    var attachedImagesBase64: [String] = []

    /// Force vision mode for all providers (for testing image sending)
    var forceVision: Bool = UserDefaults.standard.bool(forKey: "agentForceVision") {
        didSet { UserDefaults.standard.set(forceVision, forKey: "agentForceVision") }
    }

    var promptHistory: [String] = UserDefaults.standard.stringArray(forKey: "agentPromptHistory") ?? []
    var historyIndex = -1
    var savedInput = ""

    /// Prompt history for whichever tab is currently selected.
    var currentTabPromptHistory: [String] {
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId) {
            return tab.promptHistory
        }
        return promptHistory
    }

    /// Display name for the currently selected tab.
    var currentTabName: String {
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId) {
            return tab.displayTitle
        }
        return "Main"
    }
    
    /// Error history for UI display — per-tab when a tab is selected, global for main
    var errorHistory: [String] {
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId),
           !tab.isMainTab {
            return tab.tabErrors
        }
        return ErrorHistory.shared.recentErrors(limit: 50).map { error in
            let formatter = DateFormatter()
            formatter.dateFormat = "HH:mm:ss"
            let time = formatter.string(from: error.timestamp)
            let message = error.message.truncate(to: 100)
            return "[\(time)] \(error.errorType): \(message)"
        }
    }

    /// Task summaries for UI display — per-tab when a tab is selected, global for main
    var taskSummaries: [String] {
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId),
           !tab.isMainTab {
            return tab.tabTaskSummaries
        }
        return history.records.suffix(50).map { record in
            let formatter = DateFormatter()
            formatter.dateFormat = "HH:mm:ss"
            let time = formatter.string(from: record.date)
            return "[\(time)] \(record.prompt) → \(record.summary)"
        }
    }

    /// Clear prompt history for whichever tab is currently selected.
    func clearCurrentTabPromptHistory() {
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId) {
            tab.promptHistory.removeAll()
            tab.historyIndex = -1
            tab.savedInput = ""
        } else {
            promptHistory.removeAll()
            historyIndex = -1
            savedInput = ""
            UserDefaults.standard.removeObject(forKey: "agentPromptHistory")
        }
    }

    /// Clear history by type: "Prompts", "Error History", or "Task Summaries".
    func clearHistory(type: String) {
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId),
           !tab.isMainTab {
            switch type {
            case "Prompts":
                tab.promptHistory.removeAll()
                tab.historyIndex = -1
                tab.savedInput = ""
            case "Error History":
                tab.tabErrors.removeAll()
            case "Task Summaries":
                tab.tabTaskSummaries.removeAll()
            default:
                break
            }
        } else {
            switch type {
            case "Prompts":
                clearCurrentTabPromptHistory()
            case "Error History":
                ErrorHistory.shared.clear()
            case "Task Summaries":
                history.clearAll()
            default:
                break
            }
        }
    }

    let helperService = HelperService()
    let userService = UserService()
    let scriptService = ScriptService()
    let history = TaskHistory.shared
    var isCancelled = false
    var runningTask: Task<Void, Never>?
    var mainTaskQueue: [String] = []
    var currentTaskPrompt: String = ""
    var currentAppleAIPrompt: String = ""
    /// Commands run during current task — used by history, mediator, and tool handlers.
    var commandsRun: [String] = []
    @ObservationIgnored private var terminationObserver: Any?

    // MARK: - Messages Monitor
    var messagesMonitorEnabled: Bool = UserDefaults.standard.object(forKey: "agentMessagesMonitor") as? Bool ?? false {
        didSet {
            UserDefaults.standard.set(messagesMonitorEnabled, forKey: "agentMessagesMonitor")
            if messagesMonitorEnabled {
                startMessagesMonitor()
            } else {
                stopMessagesMonitor()
            }
        }
    }
    var messagesMonitorTask: Task<Void, Never>?
    /// ROWID of the last message we've already processed
    var lastSeenMessageROWID: Int = 0
    /// Briefly true during each poll cycle so the StatusDot pulses on the timer
    var messagesPolling = false
    /// Handle ID to reply to when an Agent! task completes (nil = no reply needed)
    var agentReplyHandle: String?
    /// Task for periodic progress updates during long-running tasks
    var progressUpdateTask: Task<Void, Never>?
    /// Counter for progress updates sent
    var progressUpdateCount: Int = 0
    /// Current task description for progress updates
    var currentTaskDescription: String = ""
    /// Timestamp when the current task started
    var taskStartTime: Date?

    var messageFilter: MessageFilter = {
        MessageFilter(rawValue: UserDefaults.standard.string(forKey: "agentMessageFilter") ?? "") ?? .fromOthers
    }() {
        didSet { UserDefaults.standard.set(messageFilter.rawValue, forKey: "agentMessageFilter") }
    }

    var messageRecipients: [MessageRecipient] = []
    /// Set of handle IDs (phone/email) the user has enabled for monitoring
    var enabledHandleIds: Set<String> = {
        let saved = UserDefaults.standard.stringArray(forKey: "agentEnabledHandleIds") ?? []
        return Set(saved)
    }() {
        didSet { UserDefaults.standard.set(Array(enabledHandleIds), forKey: "agentEnabledHandleIds") }
    }

    // MARK: - Script Tabs

    var scriptTabs: [ScriptTab] = [] {
        didSet { rebuildTabIndex() }
    }
    var selectedTabId: UUID?   // nil = Main tab

    /// O(1) tab lookup by UUID
    private var tabsByID: [UUID: ScriptTab] = [:]


    /// The currently selected ScriptTab, or nil for main — O(1)
    var selectedTab: ScriptTab? {
        guard let id = selectedTabId else { return nil }
        return tabsByID[id]
    }

    /// O(1) tab lookup
    func tab(for id: UUID) -> ScriptTab? { tabsByID[id] }

    private func rebuildTabIndex() {
        tabsByID = Dictionary(uniqueKeysWithValues: scriptTabs.map { ($0.id, $0) })
    }

    // MARK: - Logging State

    static let timestampFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "HH:mm:ss"
        return f
    }()

    var logBuffer = ""
    var logFlushTask: Task<Void, Never>?
    var logPersistTask: Task<Void, Never>?
    var streamLineCount = 0
    var streamTruncated = false
    static let outputLineOptions = [10, 50, 75, 100, 150, 200, 250, 500, 750, 1000, 1500]
    var maxOutputLines: Int = UserDefaults.standard.object(forKey: "agentMaxOutputLines") as? Int ?? 1000 {
        didSet { UserDefaults.standard.set(maxOutputLines, forKey: "agentMaxOutputLines") }
    }

    static let readPreviewOptions = [3, 10, 50, 100, 250, 500, 750, 1000]
    var readFilePreviewLines: Int = UserDefaults.standard.object(forKey: "agentReadFilePreviewLines") as? Int ?? 3 {
        didSet { UserDefaults.standard.set(readFilePreviewLines, forKey: "agentReadFilePreviewLines") }
    }

    var scriptCaptureStderr: Bool = UserDefaults.standard.object(forKey: "agentScriptCaptureStderr") as? Bool ?? false {
        didSet { UserDefaults.standard.set(scriptCaptureStderr, forKey: "agentScriptCaptureStderr") }
    }

    var taskAutoComplete: Bool = UserDefaults.standard.object(forKey: "agentTaskAutoComplete") as? Bool ?? true {
        didSet { UserDefaults.standard.set(taskAutoComplete, forKey: "agentTaskAutoComplete") }
    }

    var deletionLimit: Int = UserDefaults.standard.object(forKey: "agentDeletionLimit") as? Int ?? 10 {
        didSet { UserDefaults.standard.set(deletionLimit, forKey: "agentDeletionLimit") }
    }

    // MARK: - Terminal Speed

    enum TerminalSpeed: Int, CaseIterable {
        case current = 22
        case fast = 15
        case faster = 10
        case blazing = 5
        case ludicrous = 1

        var label: String {
            switch self {
            case .current: "Normal"
            case .fast: "Fast"
            case .faster: "Faster"
            case .blazing: "Blazing"
            case .ludicrous: "Ludicrous"
            }
        }
    }

    var terminalSpeed: TerminalSpeed = TerminalSpeed(rawValue: UserDefaults.standard.integer(forKey: "terminalSpeed")) ?? .current {
        didSet { UserDefaults.standard.set(terminalSpeed.rawValue, forKey: "terminalSpeed") }
    }

    var scanLinesEnabled: Bool = UserDefaults.standard.object(forKey: "scanLinesEnabled") as? Bool ?? true {
        didSet { UserDefaults.standard.set(scanLinesEnabled, forKey: "scanLinesEnabled") }
    }

    // LLM streaming state
    var streamBuffer = ""
    @ObservationIgnored var rawLLMOutput: String = UserDefaults.standard.string(forKey: "mainRawLLMOutput") ?? "" {
        didSet { UserDefaults.standard.set(rawLLMOutput, forKey: "mainRawLLMOutput") }
    }
    /// Character-by-character dripped version of rawLLMOutput for terminal effect
    var displayedLLMOutput: String = UserDefaults.standard.string(forKey: "mainRawLLMOutput") ?? ""
    var dripDisplayIndex: Int = (UserDefaults.standard.string(forKey: "mainRawLLMOutput") ?? "").count
    var dripTask: Task<Void, Never>?
    var streamFlushTask: Task<Void, Never>?
    var streamingTextStarted = false
    static let maxLogSize = 60_000
    var recentOutputHashes: Set<Int> = []

    // MARK: - Image snapshot cache (persists across launches)

    static let logImageCacheDir: URL = {
        guard let caches = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first else { return URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("Agent/log_images") }
        let dir = caches.appendingPathComponent("Agent/log_images")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }()

    static let imagePathRegex: NSRegularExpression? = try? NSRegularExpression(
        pattern: #"(/[^\s"'<>]+\.(?:jpg|jpeg|png|gif|tiff|bmp|webp|heic))"#,
        options: .caseInsensitive
    )

    // MARK: - Off-Main-Thread Helper

    /// Run synchronous work off the main thread to avoid blocking the UI.
    static func offMain<T: Sendable>(_ work: @Sendable @escaping () -> T) async -> T {
        await Task.detached { work() }.value
    }


    // MARK: - Computed Properties

    var daemonReady: Bool { helperService.helperReady }
    var agentReady: Bool { userService.userReady }
    var hasAttachments: Bool { !attachedImages.isEmpty }

    // MARK: - Voice Input

    /// Check if speech recognition is authorized
    var isSpeechRecognitionAuthorized: Bool {
        SFSpeechRecognizer.authorizationStatus() == .authorized
    }

    var speechAudioEngine: AVAudioEngine?
    var speechRecognitionRequest: SFSpeechAudioBufferRecognitionRequest?
    var speechRecognitionTask: SFSpeechRecognitionTask?
    var preDictationText: String = ""
    /// Tracks which tab was selected when dictation started, so speech goes to the correct input field
    var preDictationTabId: UUID?

    // MARK: - Hotword ("Agent!") Listening
    /// When true, mic stays open waiting for "Agent!" wake word
    var isHotwordListening = false {
        didSet { UserDefaults.standard.set(isHotwordListening, forKey: "isHotwordListening") }
    }
    /// True while capturing a command after the wake word was detected
    var isHotwordCapturing = false
    /// Timer that fires after 5 seconds of silence to auto-submit
    var hotwordSilenceTimer: Timer?
    /// Transcription length at last change — used to detect silence
    var hotwordLastTranscriptionLength = 0

    // MARK: - Init

    /// Prevents duplicate startup work when @State evaluates AgentViewModel() multiple times
    private static var _started = false

    init() {
        guard !Self._started else { return }
        Self._started = true

        // Register all LLM providers with AgentLLM framework
        LLMProviderSetup.registerAllProviders()

        activityLog = ChatHistoryStore.shared.buildActivityLogText(maxTasks: 3)
        // Trim main tab log on relaunch only
        activityLog = ScriptTab.trimForRelaunch(activityLog)
        CodeBlockTheme.updateAppearance()
        TerminalNeoTheme.updateAppearance()
        // Restore ~/Documents/AgentScript/ folder and bundled resources if missing (off main thread)
        Task.detached { [scriptService = self.scriptService] in
            scriptService.ensurePackage()
            scriptService.rebuildAllMetadata()
            let names = Set(scriptService.listScripts().map { $0.name.lowercased() })
            await MainActor.run { AppleIntelligenceMediator.knownAgentNames = names }
        }
        SystemPromptService.shared.ensureDefaults()

        restoreScriptTabs()
        syncServicesGroup()

        // Register with Daemon and UserService
        registerDaemon()
        registerAgent()

        // Add observer for app termination
        terminationObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.willTerminateNotification,
            object: nil, queue: .main
        ) { [weak self] _ in
            Task { @MainActor in
                self?.persistScriptTabs()
            }
        }

        // Restore messages monitor state
        if messagesMonitorEnabled {
            refreshMessageRecipients()
        }

        // No auto-fetch on launch — avoids wasting API calls for inactive LLMs.

        // Xcode Command Line Tools check is handled by DependencyOverlay in ContentView

        // Resume Messages monitor if it was enabled
        if messagesMonitorEnabled {
            // Delay start so UserService is connected first
            Task {
                try? await Task.sleep(nanoseconds: 3_000_000_000)
                startMessagesMonitor()
            }
        }

        // Test daemon connectivity on startup — auto-fix if not responding
        Task {
            try? await Task.sleep(nanoseconds: 500_000_000)
            appendLog("🔥 Warming up...")
            var userOK = await userService.ping()
            userPingOK = userOK
            appendLog("⚙️ User agent: \(userOK ? "ping OK" : "no response")")
            var daemonOK = false
            if rootEnabled {
                daemonOK = await helperService.ping()
                daemonPingOK = daemonOK
                appendLog("⚙️ Launch Daemon: \(daemonOK ? "ping OK" : "no response")")
            } else {
                daemonPingOK = false
                appendLog("⚙️ Launch Daemon: disabled")
            }
            if !userOK {
                appendLog("🔄 User agent: mending...")
                _ = userService.restartAgent()
                try? await Task.sleep(nanoseconds: 1_000_000_000)
                userOK = await userService.ping()
                userPingOK = userOK
                appendLog("⚙️ User agent: \(userOK ? "mended — ping OK" : "still NOT responding")")
            }
            if rootEnabled && !daemonOK {
                appendLog("🔄 Launch Daemon: mending...")
                _ = helperService.restartDaemon()
                try? await Task.sleep(nanoseconds: 1_000_000_000)
                daemonOK = await helperService.ping()
                daemonPingOK = daemonOK
                appendLog("⚙️ Launch Daemon: \(daemonOK ? "mended — ping OK" : "still NOT responding")")
            }
            if !userOK || (rootEnabled && !daemonOK) {
                appendLog("⚠️ Click Register to restart services")
            }

            // Pre-warm Ollama model to avoid cold-start delay on first task
            await self.preWarmOllama()
        }
    }

    /// Send a tiny request to Ollama to load the model into memory.
    /// This eliminates the 5-15s cold-start delay on the first task.
    private func preWarmOllama() async {
        let provider = selectedProvider
        guard provider == .ollama || provider == .localOllama else { return }
        let endpoint = provider == .ollama ? ollamaEndpoint : localOllamaEndpoint
        let model = provider == .ollama ? ollamaModel : localOllamaModel
        guard !model.isEmpty else { return }

        let chatURL = endpoint.isEmpty ? "http://localhost:11434/api/chat" : endpoint
        guard let url = URL(string: chatURL) else { return }

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.timeoutInterval = 30
        let body: [String: Any] = [
            "model": model,
            "messages": [["role": "user", "content": "hi"]],
            "stream": false,
            "options": ["num_predict": 1]  // Generate just 1 token — enough to load model
        ]
        request.httpBody = try? JSONSerialization.data(withJSONObject: body)

        do {
            let _ = try await URLSession.shared.data(for: request)
            appendLog("⚙️ Ollama: \(model) pre-warmed")
        } catch {
            // Silent fail — model will load on first task instead
        }
    }
}
