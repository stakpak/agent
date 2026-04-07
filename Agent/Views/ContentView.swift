import SwiftUI
import AppKit
import WebKit

struct ContentView: View {
    @State private var viewModel = AgentViewModel()
    @State private var showSettings = false
    @State private var showHistory = false
    @State private var dependencyStatus: DependencyStatus?
    @State private var showDependencyOverlay = true
    @State private var showSearch = false
    @State private var searchText = ""
    @State private var caseSensitive = false
    @FocusState private var isSearchFieldFocused: Bool
    @FocusState private var isTaskFieldFocused: Bool
    @State private var currentMatchIndex = 0
    @State private var totalMatches = 0
    @State private var showMCPServers = false
    @State private var showTools = false
    @State private var showOptions = false
    @ObservedObject private var aiMediator = AppleIntelligenceMediator.shared
    @State private var showAIPopover = false
    @State private var showMessages = false
    @State private var showAccessibility = false
    @State private var showQuitConfirm = false
    @State private var showClearConfirm = false
    @State private var showNewTabSheet = false
    @State private var showServices = false
    @State private var showAppleAIBanner = false
    @State private var showUserQuestion = false
    @State private var userQuestionText = ""
    @State private var userAnswerText = ""

    var body: some View {
        VStack(spacing: 0) {
            // Project folder section (main tab or selected script tab)
            ProjectFolderSectionView(
                viewModel: viewModel,
                selectedTab: viewModel.selectedTabId.flatMap { id in
                    viewModel.tab(for: id)
                }
            )

            Divider()

            // Search bar
            if showSearch {
                SearchBarView(
                    searchText: $searchText,
                    caseSensitive: $caseSensitive,
                    totalMatches: totalMatches,
                    currentMatchIndex: currentMatchIndex,
                    previousMatch: previousMatch,
                    nextMatch: nextMatch,
                    onClose: { showSearch = false; searchText = "" }
                )
                .focused($isSearchFieldFocused)
            }

            // Tab bar (only when script tabs exist)
            if !viewModel.scriptTabs.isEmpty {
                TabBarView(viewModel: viewModel)
                Divider()
            }

            // Current task banner with cancel button
            if let prompt = activeTaskPrompt, !prompt.isEmpty {
                TaskBannerView(
                    prompt: prompt,
                    appleAIPrompt: activeAppleAIPrompt,
                    showAppleAIBanner: $showAppleAIBanner,
                    onCancel: {
                        if let selId = viewModel.selectedTabId,
                           let tab = viewModel.tab(for: selId)
                        {
                            if tab.isLLMRunning {
                                viewModel.stopTabTask(tab: tab)
                            } else if tab.isRunning {
                                viewModel.cancelScriptTab(id: tab.id)
                            }
                        } else {
                            viewModel.stop()
                        }
                    }
                )
            }

            // Activity Log with LLM Output overlaid on top — overlay doesn't push log down
            ZStack(alignment: .top) {
                ActivityLogView(
                    text: viewModel.selectedTab?.activityLog ?? viewModel.activityLog,
                    tabID: viewModel.selectedTabId,
                    isActive: viewModel.selectedTab?.isBusy ?? viewModel.isRunning,
                    textProvider: { [weak viewModel] in
                        guard let vm = viewModel else { return "" }
                        return vm.selectedTab?.activityLog ?? vm.activityLog
                    },
                    searchText: searchText,
                    caseSensitive: caseSensitive,
                    currentMatchIndex: currentMatchIndex,
                    onMatchCount: { count in
                        DispatchQueue.main.async {
                            totalMatches = count
                            if currentMatchIndex >= count { currentMatchIndex = max(0, count - 1) }
                        }
                    }
                )

                thinkingIndicator
            }

            Divider()

            // Screenshot previews (per-tab or main)
            if let tab = viewModel.selectedTabId.flatMap({ viewModel.tab(for: $0) }) {
                if !tab.attachedImages.isEmpty {
                    ScreenshotPreviewView(
                        images: tab.attachedImages,
                        onRemove: { index in
                            guard tab.attachedImages.indices.contains(index) else { return }
                            tab.attachedImages.remove(at: index)
                            tab.attachedImagesBase64.remove(at: index)
                        },
                        onRemoveAll: {
                            tab.attachedImages.removeAll()
                            tab.attachedImagesBase64.removeAll()
                        }
                    )
                }
            } else if !viewModel.attachedImages.isEmpty {
                ScreenshotPreviewView(
                    images: viewModel.attachedImages,
                    onRemove: { index in viewModel.removeAttachment(at: index) },
                    onRemoveAll: { viewModel.removeAllAttachments() }
                )
            }

            // Input — switches between main and tab input
            InputSectionView(
                viewModel: viewModel,
                isTaskFieldFocused: $isTaskFieldFocused,
                selectedTab: viewModel.selectedTabId.flatMap { id in
                    viewModel.tab(for: id)
                }
            )
        }
        .frame(minWidth: 600, minHeight: 500)
        .onTapGesture {
            NSApp.keyWindow?.makeFirstResponder(nil)
        }
        .toolbarBackground(Color(nsColor: .windowBackgroundColor), for: .windowToolbar)
        .toolbar {
            ToolbarItemGroup(placement: .navigation) {
                HeaderStatusView(viewModel: viewModel)
            }
            ToolbarItem(placement: .automatic) {
                Spacer()
            }
            ToolbarItemGroup(placement: .automatic) {
                HeaderToolbarButtons(
                    viewModel: viewModel,
                    showServices: $showServices,
                    showMessages: $showMessages,
                    showAccessibility: $showAccessibility,
                    showMCPServers: $showMCPServers,
                    showTools: $showTools,
                    showSettings: $showSettings,
                    showAIPopover: $showAIPopover,
                    showOptions: $showOptions,
                    showHistory: $showHistory,
                    showClearConfirm: $showClearConfirm
                )
            }
        }
        .onAppear {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
                isTaskFieldFocused = true
            }
        }
        .overlay {
            DependencyOverlay(status: dependencyStatus, isVisible: $showDependencyOverlay)
        }
        .alert("Quit Agent?", isPresented: $showQuitConfirm) {
            Button("Quit", role: .destructive) { NSApplication.shared.terminate(nil) }
            Button("Cancel", role: .cancel) { }
        } message: {
            Text("Are you sure you want to close the window and quit?")
        }
        .sheet(isPresented: $showNewTabSheet) {
            NewMainTabSheet(viewModel: viewModel)
        }
        .alert("Agent Failed", isPresented: $viewModel.showFailedAgentAlert) {
            Button("Remove", role: .destructive) { if let id = viewModel.failedAgentId { RecentAgentsService.shared.removeById(id) } }
            Button("Keep", role: .cancel) { }
        } message: {
            Text("'\(viewModel.failedAgentName)' failed. Remove from 🦾 Agents menu?")
        }
        // Auto-expand of HUD on run-start happens in AgentViewModel.executeTask now,
        // so we don't fire it from a generic onChange (which would also trigger on tab swaps).
        .onChange(of: viewModel.selectedTabId) { _, _ in
            // Reset search state when switching tabs
            if showSearch {
                showSearch = false
                searchText = ""
                currentMatchIndex = 0
                totalMatches = 0
            }
            // Focus task input when switching/creating tabs
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
                isTaskFieldFocused = true
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .appWillQuit)) { _ in
            viewModel.stopAll()
            viewModel.stopMessagesMonitor()
            Task { await MCPService.shared.disconnectAll() }
        }
        .onAppear {
            setupMenuObservers()
            AgentsMenuDelegate.shared.viewModel = viewModel
            DispatchQueue.global(qos: .userInitiated).async {
                let status = DependencyChecker.check()
                DispatchQueue.main.async {
                    dependencyStatus = status
                    // Don't dismiss here — the overlay animates its own
                    // 2.5-second auto-dismiss when allGood is true.
                }
            }
            NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
                // Cmd+W to close current tab or quit
                if event.modifierFlags.contains(.command),
                   event.charactersIgnoringModifiers == "w"
                {
                    if let selId = viewModel.selectedTabId {
                        viewModel.closeScriptTab(id: selId)
                    } else if viewModel.scriptTabs.isEmpty {
                        showQuitConfirm = true
                    } else {
                        // Tabs exist but Main is selected — do nothing
                    }
                    return nil
                }

                // Cmd+T to create a new main LLM tab
                if event.modifierFlags.contains(.command),
                   event.charactersIgnoringModifiers == "t"
                {
                    showNewTabSheet = true
                    return nil
                }

                // Cmd+F to toggle search bar
                if event.modifierFlags.contains(.command),
                   event.charactersIgnoringModifiers == "f"
                {
                    showSearch.toggle()
                    if showSearch {
                        isSearchFieldFocused = true
                    } else {
                        searchText = ""
                    }
                    return nil
                }

                // Escape to close search bar
                if event.keyCode == 53, showSearch {
                    showSearch = false
                    searchText = ""
                    return nil
                }

                // Intercept Cmd+V for image paste
                if event.modifierFlags.contains(.command),
                   event.charactersIgnoringModifiers == "v"
                {
                    if viewModel.pasteImageFromClipboard() {
                        return nil
                    }
                }

                // Keyboard shortcuts for common actions
                // Cmd+N: New task (focus input)
                if event.modifierFlags.contains(.command),
                   event.charactersIgnoringModifiers == "n"
                {
                    // Focus is already on text field, this is just a quick way to clear and start new
                    return nil
                }

                // Cmd+B: Toggle LLM Overlay
                if event.modifierFlags.contains(.command),
                   event.charactersIgnoringModifiers == "b"
                {
                    withAnimation(.easeInOut(duration: 0.2)) {
                        if let selId = viewModel.selectedTabId, let tab = viewModel.tab(for: selId) {
                            tab.thinkingDismissed.toggle()
                        } else {
                            viewModel.thinkingDismissed.toggle()
                        }
                    }
                    return nil
                }

                // Cmd+Return: Run current task
                if event.modifierFlags.contains(.command),
                   event.keyCode == 36
                {
                    if let selId = viewModel.selectedTabId,
                       let tab = viewModel.tab(for: selId)
                    {
                        if !tab.taskInput.isEmpty && !tab.isLLMRunning {
                            viewModel.runTabTask(tab: tab)
                        }
                    } else if !viewModel.taskInput.isEmpty && !viewModel.isRunning {
                        viewModel.run()
                    }
                    return nil
                }

                // Cmd+.: Cancel current task
                if event.modifierFlags.contains(.command),
                   event.charactersIgnoringModifiers == "."
                {
                    if let selId = viewModel.selectedTabId,
                       let tab = viewModel.tab(for: selId),
                       tab.isBusy
                    {
                        if tab.isLLMRunning {
                            viewModel.stopTabTask(tab: tab)
                        } else if tab.isRunning {
                            viewModel.cancelScriptTab(id: tab.id)
                        }
                    } else if viewModel.isRunning || viewModel.isThinking {
                        viewModel.stop()
                    }
                    return nil
                }

                // Cmd+Shift+P: Open System Prompts
                if event.modifierFlags.contains([.command, .shift]),
                   event.charactersIgnoringModifiers == "p"
                {
                    // System prompts window would be opened here
                    // For now, focus on settings
                    showSettings = true
                    return nil
                }

                // Cmd+Shift+M: Toggle Messages Monitor
                if event.modifierFlags.contains([.command, .shift]),
                   event.charactersIgnoringModifiers == "m"
                {
                    viewModel.messagesMonitorEnabled.toggle()
                    return nil
                }

                // Cmd+B: Toggle entire LLM Output overlay visibility
                if event.modifierFlags.contains(.command),
                   !event.modifierFlags.contains(.shift),
                   event.charactersIgnoringModifiers == "b"
                {
                    withAnimation(.easeInOut(duration: 0.2)) {
                        if let selId = viewModel.selectedTabId,
                           let tab = viewModel.tab(for: selId)
                        {
                            tab.thinkingDismissed.toggle()
                        } else {
                            viewModel.thinkingDismissed.toggle()
                        }
                    }
                    return nil
                }

                // Cmd+D: Toggle both LLM chevrons on current tab
                if event.modifierFlags.contains(.command),
                   !event.modifierFlags.contains(.shift),
                   event.charactersIgnoringModifiers == "d"
                {
                    withAnimation(.easeInOut(duration: 0.25)) {
                        if let selId = viewModel.selectedTabId,
                           let tab = viewModel.tab(for: selId)
                        {
                            let expand = !tab.thinkingExpanded
                            tab.thinkingExpanded = expand
                            tab.thinkingOutputExpanded = expand
                        } else {
                            let expand = !viewModel.thinkingExpanded
                            viewModel.thinkingExpanded = expand
                            viewModel.thinkingOutputExpanded = expand
                        }
                    }
                    return nil
                }

                // Cmd+L: Clear log
                if event.modifierFlags.contains(.command),
                   !event.modifierFlags.contains(.shift),
                   event.charactersIgnoringModifiers == "l"
                {
                    viewModel.clearSelectedLog()
                    return nil
                }

                // Cmd+Shift+K: Clear all
                if event.modifierFlags.contains([.command, .shift]),
                   event.charactersIgnoringModifiers == "k"
                {
                    viewModel.clearAll()
                    return nil
                }

                // Cmd+Shift+L: Clear LLM output
                if event.modifierFlags.contains([.command, .shift]),
                   event.charactersIgnoringModifiers == "l"
                {
                    viewModel.rawLLMOutput = ""
                    if let selId = viewModel.selectedTabId, let tab = viewModel.tab(for: selId) {
                        tab.rawLLMOutput = ""
                    }
                    return nil
                }

                // Cmd+Shift+H: Clear prompt history
                if event.modifierFlags.contains([.command, .shift]),
                   event.charactersIgnoringModifiers == "h"
                {
                    viewModel.promptHistory.removeAll()
                    UserDefaults.standard.removeObject(forKey: "agentPromptHistory")
                    if let selId = viewModel.selectedTabId, let tab = viewModel.tab(for: selId) {
                        tab.promptHistory.removeAll()
                    }
                    return nil
                }

                // Cmd+Shift+J: Clear task history
                if event.modifierFlags.contains([.command, .shift]),
                   event.charactersIgnoringModifiers == "j"
                {
                    viewModel.history.clearAll()
                    return nil
                }

                // Cmd+Shift+U: Clear tokens
                if event.modifierFlags.contains([.command, .shift]),
                   event.charactersIgnoringModifiers == "u"
                {
                    viewModel.taskInputTokens = 0; viewModel.taskOutputTokens = 0
                    viewModel.sessionInputTokens = 0; viewModel.sessionOutputTokens = 0
                    return nil
                }

                // Cmd+1-9: Switch between tabs
                if event.modifierFlags.contains(.command),
                   let char = event.charactersIgnoringModifiers,
                   let number = Int(char),
                   number >= 1, number <= 9
                {
                    selectTab(viewModel: viewModel, number: number)
                    return nil
                }

                // Cmd+Shift+]: Next tab
                if event.modifierFlags.contains(.command),
                   event.keyCode == 124
                { // Right arrow
                    nextTab(viewModel: viewModel)
                    return nil
                }

                // Cmd+Shift+[: Previous tab
                if event.modifierFlags.contains(.command),
                   event.keyCode == 123
                { // Left arrow
                    previousTab(viewModel: viewModel)
                    return nil
                }

                // Escape key to cancel active context (tab or main)
                if event.keyCode == 53 {
                    if let selId = viewModel.selectedTabId,
                       let tab = viewModel.tab(for: selId),
                       tab.isBusy
                    {
                        if tab.isLLMRunning {
                            viewModel.stopTabTask(tab: tab)
                        } else if tab.isRunning {
                            viewModel.cancelScriptTab(id: tab.id)
                        }
                        return nil
                    } else if viewModel.isRunning || viewModel.isThinking {
                        viewModel.stop()
                        return nil
                    }
                }

                // Up/Down arrow for prompt history (per-tab or main)
                // Only navigate history for short, single-line input;
                // let arrows move the cursor in longer/multi-line text.
                // Always allow history navigation when already browsing history.
                if event.keyCode == 126 || event.keyCode == 125 {
                    let text: String
                    let browsingHistory: Bool
                    if let tabId = viewModel.selectedTabId,
                       let tab = viewModel.tab(for: tabId)
                    {
                        text = tab.taskInput
                        browsingHistory = tab.historyIndex != -1
                    } else {
                        text = viewModel.taskInput
                        browsingHistory = viewModel.historyIndex != -1
                    }
                    let font = NSFont.systemFont(ofSize: NSFont.systemFontSize)
                    let textWidth = (text as NSString).size(withAttributes: [.font: font]).width
                    let isSingleLine = !text.contains("\n") && textWidth <= viewModel.inputFieldWidth
                    if isSingleLine || browsingHistory {
                        let direction = event.keyCode == 126 ? -1 : 1
                        if let tabId = viewModel.selectedTabId,
                           let tab = viewModel.tab(for: tabId)
                        {
                            tab.navigateHistory(direction: direction)
                        } else {
                            viewModel.navigatePromptHistory(direction: direction)
                        }
                        return nil
                    }
                    return event // Let TextField handle cursor movement
                }

                return event
            }
        }
    }

    // MARK: - Menu Command Observers

    private static let menuNotifications: [Notification.Name] = [
        .menuToggleChevrons, .menuToggleOverlay, .menuRunTask, .menuCancelTask,
        .menuFind, .menuNewTab, .menuCloseTab, .menuNextTab, .menuPrevTab,
        .menuClearAll, .menuClearLog, .menuClearLLM, .menuClearHistory,
        .menuClearTasks, .menuClearTokens
    ]

    func setupMenuObservers() {
        for name in Self.menuNotifications {
            let menuName = name
            NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { _ in
                MainActor.assumeIsolated { [self] in handleMenuCommand(menuName) }
            }
        }
        // AskUserQuestion observer — shows NSAlert dialog
        NotificationCenter.default.addObserver(forName: .askUserQuestion, object: nil, queue: .main) { _ in
            MainActor.assumeIsolated { [self] in
                let question = viewModel.pendingQuestion
                guard !question.isEmpty else { return }
                let alert = NSAlert()
                alert.messageText = "Agent Question"
                alert.informativeText = question
                alert.alertStyle = .informational
                let input = NSTextField(frame: NSRect(x: 0, y: 0, width: 300, height: 24))
                input.placeholderString = "Your answer"
                alert.accessoryView = input
                alert.addButton(withTitle: "Send")
                alert.addButton(withTitle: "Skip")
                let response = alert.runModal()
                viewModel.pendingAnswer = response == .alertFirstButtonReturn
                    ? (input.stringValue.isEmpty ? "(no answer)" : input.stringValue)
                    : "(skipped)"
            }
        }
    }

    func handleMenuCommand(_ name: Notification.Name) {
        switch name {
        case .menuToggleChevrons:
            withAnimation(.easeInOut(duration: 0.25)) {
                if let selId = viewModel.selectedTabId, let tab = viewModel.tab(for: selId) {
                    let expand = !tab.thinkingExpanded
                    tab.thinkingExpanded = expand; tab.thinkingOutputExpanded = expand
                } else {
                    let expand = !viewModel.thinkingExpanded
                    viewModel.thinkingExpanded = expand; viewModel.thinkingOutputExpanded = expand
                }
            }
        case .menuToggleOverlay:
            withAnimation(.easeInOut(duration: 0.2)) {
                if let selId = viewModel.selectedTabId, let tab = viewModel.tab(for: selId) {
                    tab.thinkingDismissed.toggle()
                } else { viewModel.thinkingDismissed.toggle() }
            }
        case .menuRunTask:
            if let selId = viewModel.selectedTabId, let tab = viewModel.tab(for: selId) {
                if !tab.taskInput.isEmpty && !tab.isLLMRunning { viewModel.runTabTask(tab: tab) }
            } else if !viewModel.taskInput.isEmpty && !viewModel.isRunning { viewModel.run() }
        case .menuCancelTask:
            if let selId = viewModel.selectedTabId, let tab = viewModel.tab(for: selId), tab.isBusy {
                if tab.isLLMRunning { viewModel.stopTabTask(tab: tab) }
                else if tab.isRunning { viewModel.cancelScriptTab(id: tab.id) }
            } else if viewModel.isRunning { viewModel.stop() }
        case .menuFind:
            showSearch.toggle()
            if showSearch { isSearchFieldFocused = true } else { searchText = "" }
        case .menuNewTab: showNewTabSheet = true
        case .menuCloseTab:
            if let selId = viewModel.selectedTabId { viewModel.closeScriptTab(id: selId) }
        case .menuNextTab: nextTab(viewModel: viewModel)
        case .menuPrevTab: previousTab(viewModel: viewModel)
        case .menuClearAll: viewModel.clearAll()
        case .menuClearLog: viewModel.clearSelectedLog()
        case .menuClearLLM:
            viewModel.rawLLMOutput = ""
            if let selId = viewModel.selectedTabId, let tab = viewModel.tab(for: selId) { tab.rawLLMOutput = "" }
        case .menuClearHistory:
            viewModel.promptHistory.removeAll()
            UserDefaults.standard.removeObject(forKey: "agentPromptHistory")
            if let selId = viewModel.selectedTabId, let tab = viewModel.tab(for: selId) { tab.promptHistory.removeAll() }
        case .menuClearTasks: viewModel.history.clearAll()
        case .menuClearTokens:
            viewModel.taskInputTokens = 0; viewModel.taskOutputTokens = 0
            viewModel.sessionInputTokens = 0; viewModel.sessionOutputTokens = 0
        default: break
        }
    }

    private func nextMatch() {
        guard totalMatches > 0 else { return }
        currentMatchIndex = (currentMatchIndex + 1) % totalMatches
    }

    private func previousMatch() {
        guard totalMatches > 0 else { return }
        currentMatchIndex = (currentMatchIndex - 1 + totalMatches) % totalMatches
    }

    private static let tabColors: [Color] = [
        .orange, .purple, .pink, .cyan, .mint, .indigo, .teal, .yellow
    ]

    /// Assign a consistent color per tab based on its index. Main tab uses .red.
    static func tabColor(for tabId: UUID, in tabs: [ScriptTab]) -> Color {
        guard let idx = tabs.firstIndex(where: { $0.id == tabId }) else { return .orange }
        return tabColors[idx % tabColors.count]
    }

    @ViewBuilder
    private var thinkingIndicator: some View {
        if let selId = viewModel.selectedTabId,
           let tab = viewModel.tab(for: selId)
        {
            if !tab.thinkingDismissed {
                ThinkingIndicatorView(viewModel: viewModel, tab: tab)
            }
        } else if viewModel.showThinkingIndicator && !isActiveDismissed {
            ThinkingIndicatorView(viewModel: viewModel)
        }
    }

    /// Whether the active context's thinking indicator has been dismissed.
    private var isActiveDismissed: Bool {
        if let selId = viewModel.selectedTabId,
           let tab = viewModel.tab(for: selId)
        {
            return tab.thinkingDismissed
        }
        return viewModel.thinkingDismissed
    }

    /// Whether the active context (selected tab or main) is in thinking state.
    private var isActiveThinking: Bool {
        if let selId = viewModel.selectedTabId,
           let tab = viewModel.tab(for: selId)
        {
            return tab.isLLMThinking
        }
        return viewModel.isThinking
    }

    /// Whether the active context is doing anything — thinking, running, or executing.
    private var isActiveRunning: Bool {
        if let selId = viewModel.selectedTabId,
           let tab = viewModel.tab(for: selId)
        {
            return tab.isLLMRunning || tab.isLLMThinking || tab.isRunning
        }
        return viewModel.isRunning || viewModel.isThinking
    }

    /// The prompt of the currently running task (main or selected tab).
    private var activeTaskPrompt: String? {
        // Check selected tab first
        if let selId = viewModel.selectedTabId,
           let tab = viewModel.tab(for: selId)
        {
            if tab.isLLMRunning { return tab.currentTaskPrompt }
            if tab.isRunning { return "Running: \(tab.scriptName)" }
        }
        // Always show main tab's prompt if it's running
        if viewModel.isRunning { return viewModel.currentTaskPrompt }
        return nil
    }

    /// The Apple AI annotation for the currently running task.
    private var activeAppleAIPrompt: String? {
        // Check selected tab first
        if let selId = viewModel.selectedTabId,
           let tab = viewModel.tab(for: selId)
        {
            let p = tab.currentAppleAIPrompt
            if !p.isEmpty { return p }
        }
        // Fall back to main tab
        let p = viewModel.currentAppleAIPrompt
        return p.isEmpty ? nil : p
    }

    /// Color for the currently selected tab.
    private var currentTabColor: Color {
        guard let selectedId = viewModel.selectedTabId else { return .red }
        if let tab = viewModel.tab(for: selectedId) {
            return tab.isMainTab ? .blue : Self.tabColor(for: selectedId, in: viewModel.scriptTabs)
        }
        return .red
    }
}


