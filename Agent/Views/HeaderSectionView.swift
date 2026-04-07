import SwiftUI

/// Left side of the toolbar: status indicators and spinner
struct HeaderStatusView: View {
    @Environment(\.colorScheme) private var colorScheme
    @Bindable var viewModel: AgentViewModel

    /// Yellow that's readable on both light and dark
    private var runningColor: Color { colorScheme == .dark ? .yellow : Color(red: 0.7, green: 0.5, blue: 0.0) }

    var body: some View {
        HStack(spacing: 12) {
            // Service status indicators
            HStack(spacing: 4) {
                StatusDot(
                    isActive: viewModel.userServiceActive,
                    wasActive: viewModel.userWasActive,
                    isBusy: viewModel.isRunning,
                    enabled: viewModel.userEnabled
                )
                Text("Agent!")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .help("User Agent: \(viewModel.userServiceActive ? "Running" : (viewModel.userEnabled ? "Stopped" : "Disabled"))")
            .accessibilityElement(children: .combine)
            .accessibilityLabel("User Agent")
            .accessibilityValue(viewModel.userServiceActive ? "Running" : (viewModel.userEnabled ? "Stopped" : "Disabled"))

            HStack(spacing: 4) {
                StatusDot(
                    isActive: viewModel.rootServiceActive,
                    wasActive: viewModel.rootWasActive,
                    isBusy: viewModel.isRunning,
                    enabled: viewModel.rootEnabled
                )
                Text("Daemon")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .help("Daemon: \(viewModel.rootServiceActive ? "Running" : (viewModel.rootEnabled ? "Stopped" : "Disabled"))")
            .accessibilityElement(children: .combine)
            .accessibilityLabel("Daemon")
            .accessibilityValue(viewModel.rootServiceActive ? "Running" : (viewModel.rootEnabled ? "Stopped" : "Disabled"))
        }
        .padding(.leading, 15)
        .fixedSize()
        .padding(.trailing, 18)
    }
}

/// Right side of the toolbar: all action buttons with popovers
struct HeaderToolbarButtons: View {
    @Bindable var viewModel: AgentViewModel
    @Binding var showServices: Bool
    @Binding var showMessages: Bool
    @Binding var showAccessibility: Bool
    @Binding var showMCPServers: Bool
    @Binding var showTools: Bool
    @Binding var showSettings: Bool
    @State private var showRollback = false
    @Binding var showAIPopover: Bool
    @Binding var showOptions: Bool
    @Binding var showHistory: Bool
    @Binding var showClearConfirm: Bool
    @State private var showLLMUsage = false
    @State private var showCodingPrefs = false
    @State private var showFallbackChain = false
    @State private var showHUDOptions = false
    @ObservedObject var aiMediator = AppleIntelligenceMediator.shared

    /// Fallback chain icon color: green when configured (enabled + has ≥1 enabled entry),
    /// orange when enabled but empty, gray when disabled.
    private var fallbackIconColor: Color {
        let svc = FallbackChainService.shared
        guard svc.enabled else { return .secondary }
        let hasEnabledEntry = svc.chain.contains { $0.enabled }
        return hasEnabledEntry ? .green : .orange
    }

    var currentTabColor: Color {
        guard let selectedId = viewModel.selectedTabId,
              let tab = viewModel.tab(for: selectedId) else
        {
            return .primary
        }
        return ContentView.tabColor(for: tab.id, in: viewModel.scriptTabs)
    }

    /// Whether any LLM task is actively running (main or any tab)
    var isLLMActive: Bool {
        viewModel.isRunning || viewModel.isThinking || viewModel.scriptTabs.contains(where: { $0.isLLMRunning })
    }

    /// LLM icon color: throbs when running, static otherwise
    var llmIconColor: Color {
        isLLMActive ? .cyan : viewModel.llmStatusColor
    }

    var body: some View {
        Button { showServices.toggle() } label: {
            Image(systemName: "gearshape.2")
                .foregroundStyle(viewModel.servicesGearColor)
        }
        .help(viewModel.servicesGearHelp)
        .accessibilityLabel("Services")
        .popover(isPresented: $showServices, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            ServicesPopover(viewModel: viewModel)
        }

        Button { showMessages.toggle() } label: {
            Image(systemName: "message.fill")
                .foregroundStyle(viewModel.messagesMonitorEnabled ? Color.green : Color.gray)
        }
        .help(viewModel.messagesMonitorEnabled ? "Messages Monitor: ON" : "Messages Monitor: OFF")
        .accessibilityLabel("Messages Monitor")
        .accessibilityValue(viewModel.messagesMonitorEnabled ? "On" : "Off")
        .popover(isPresented: $showMessages, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            MessagesView(viewModel: viewModel)
        }

        Button { showAccessibility.toggle() } label: {
            Image(systemName: "hand.raised")
                .foregroundStyle(viewModel.accessibilityIconColor)
        }
        .accessibilityLabel("Accessibility")
        .popover(isPresented: $showAccessibility, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            AccessibilitySettingsView()
        }

        Button { showMCPServers.toggle() } label: {
            Image(systemName: "server.rack")
                .foregroundStyle(viewModel.mcpIconColor)
        }
        .accessibilityLabel("MCP Servers")
        .popover(isPresented: $showMCPServers, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            MCPServersView()
        }

        Button { showCodingPrefs.toggle() } label: {
            Image(systemName: "chevron.left.forwardslash.chevron.right")
                .foregroundStyle(
                    viewModel.autoVerifyEnabled || viewModel.visualTestsEnabled || viewModel.autoPREnabled || viewModel
                        .autoScaffoldEnabled ? Color.green : Color.secondary
                )
        }
        .help("Coding Preferences")
        .accessibilityLabel("Coding Preferences")
        .popover(isPresented: $showCodingPrefs, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            CodingPreferencesView(viewModel: viewModel)
        }

        Button { showTools.toggle() } label: {
            Image(systemName: "wrench.and.screwdriver")
                .foregroundStyle(viewModel.toolsIconColor)
        }
        .accessibilityLabel("Tools")
        .popover(isPresented: $showTools, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            ToolsView(selectedProvider: $viewModel.selectedProvider, viewModel: viewModel)
        }

        Button { showSettings.toggle() } label: {
            Image(systemName: "cpu")
                .foregroundStyle(llmIconColor)
                .symbolEffect(.pulse, isActive: isLLMActive)
        }
        .accessibilityLabel("LLM Settings")
        .accessibilityValue(isLLMActive ? "Active" : "Idle")
        .popover(isPresented: $showSettings, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            SettingsView(viewModel: viewModel)
        }

        Button { showAIPopover.toggle() } label: {
            Image(systemName: AppleIntelligenceMediator.isAvailable ? "brain.fill" : "brain")
                .foregroundStyle(aiMediator.brainIconColor)
        }
        .help("Apple Intelligence Settings")
        .accessibilityLabel("Apple Intelligence")
        .accessibilityValue(AppleIntelligenceMediator.isAvailable ? "Available" : "Unavailable")
        .popover(isPresented: $showAIPopover, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            AppleIntelligencePopover()
        }

        Button { showOptions.toggle() } label: {
            Image(systemName: "slider.horizontal.3")
                .foregroundStyle(.green)
        }
        .accessibilityLabel("Options")
        .popover(isPresented: $showOptions, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            AgentOptionsView(viewModel: viewModel)
        }

        Button { showFallbackChain.toggle() } label: {
            Image(systemName: "arrow.triangle.2.circlepath")
                .foregroundStyle(fallbackIconColor)
        }
        .help("Fallback Chain")
        .accessibilityLabel("Fallback Chain")
        .popover(isPresented: $showFallbackChain, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            FallbackChainView(viewModel: viewModel)
        }

        Button { showHUDOptions.toggle() } label: {
            Image(systemName: "viewfinder")
                .foregroundStyle(viewModel.scanLinesEnabled ? Color.green : Color.secondary)
        }
        .help("HUD (Heads-Up Display)")
        .accessibilityLabel("HUD")
        .popover(isPresented: $showHUDOptions, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            HUDOptionsView(viewModel: viewModel)
        }

        Button { showLLMUsage.toggle() } label: {
            Image(systemName: "chart.bar.fill")
                .foregroundStyle(TokenUsageStore.shared.modelUsage.isEmpty ? Color.secondary : Color.green)
        }
        .help("LLM Usage & Costs")
        .accessibilityLabel("LLM Usage")
        .popover(isPresented: $showLLMUsage, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            LLMUsageView(viewModel: viewModel)
        }

        Button { showRollback.toggle() } label: {
            Image(systemName: "arrow.uturn.backward.circle")
                .foregroundStyle(FileBackupService.shared.totalBackupCount() > 0 ? .green : .secondary)
        }
        .help("File Backups & Rollback")
        .accessibilityLabel("Rollback")
        .accessibilityValue("\(FileBackupService.shared.totalBackupCount()) backups")
        .popover(isPresented: $showRollback, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            RollbackView(viewModel: viewModel)
        }

        Button { showHistory.toggle() } label: {
            Image(systemName: "clock.arrow.circlepath")
                .foregroundStyle(viewModel.historyIconColor)
        }
        .accessibilityLabel("History")
        .popover(isPresented: $showHistory, attachmentAnchor: .rect(.bounds), arrowEdge: .bottom) {
            HistoryView(
                prompts: viewModel.currentTabPromptHistory,
                errorHistory: viewModel.errorHistory,
                taskSummaries: viewModel.taskSummaries,
                tabName: viewModel.currentTabName,
                onClear: { type in viewModel.clearHistory(type: type) },
                onRerun: { prompt in
                    if let selectedId = viewModel.selectedTabId,
                       let tab = viewModel.tab(for: selectedId)
                    {
                        tab.taskInput = prompt
                        viewModel.runTabTask(tab: tab)
                    } else {
                        viewModel.taskInput = prompt
                        viewModel.run()
                    }
                }
            )
        }

        Button { showClearConfirm = true } label: {
            Image(systemName: "trash")
                .foregroundStyle(.primary)
        }
        .accessibilityLabel("Clear Log")
        .alert("Clear Log", isPresented: $showClearConfirm) {
            Button("Clear", role: .destructive) { viewModel.clearSelectedLog() }
            Button("Cancel", role: .cancel) { }
        } message: {
            Text(
                viewModel.selectedTabId != nil
                    ? "Clear this tab's log?"
                    : "Clear all task history?"
            )
        }
    }
}
