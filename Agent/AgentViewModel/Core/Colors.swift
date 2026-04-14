@preconcurrency import Foundation
import AgentTools
import AgentLLM
import AgentAccess
import SwiftUI

extension AgentViewModel {
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

    // MARK: - Temperature helpers

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
}
