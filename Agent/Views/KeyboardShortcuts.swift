//
//  KeyboardShortcuts.swift
//  Agent
//
//  Keyboard shortcut handlers for ContentView
//

import Foundation

// MARK: - Keyboard Shortcuts

/// Navigate to next tab (cycle right)
@MainActor
func nextTab(viewModel: AgentViewModel) {
    if viewModel.scriptTabs.isEmpty { return }
    guard let currentId = viewModel.selectedTabId else {
        // On main tab - go to first script tab
        if let firstTab = viewModel.scriptTabs.first {
            viewModel.selectedTabId = firstTab.id
        }
        return
    }
    
    guard let currentIndex = viewModel.scriptTabs.firstIndex(where: { $0.id == currentId }) else { return }
    let nextIndex = (currentIndex + 1) % viewModel.scriptTabs.count
    viewModel.selectedTabId = viewModel.scriptTabs[nextIndex].id
    viewModel.persistScriptTabs()
}

/// Navigate to previous tab (cycle left)
@MainActor
func previousTab(viewModel: AgentViewModel) {
    if viewModel.scriptTabs.isEmpty { return }
    guard let currentId = viewModel.selectedTabId else {
        // On main tab - go to last script tab
        if let lastTab = viewModel.scriptTabs.last {
            viewModel.selectedTabId = lastTab.id
        }
        return
    }
    
    guard let currentIndex = viewModel.scriptTabs.firstIndex(where: { $0.id == currentId }) else { return }
    let prevIndex = (currentIndex - 1 + viewModel.scriptTabs.count) % viewModel.scriptTabs.count
    viewModel.selectedTabId = viewModel.scriptTabs[prevIndex].id
    viewModel.persistScriptTabs()
}

/// Navigate to tab by number (1-9)
@MainActor
func selectTab(viewModel: AgentViewModel, number: Int) {
    guard number >= 1, number <= 9 else { return }
    if number == 1 {
        // Cmd+1 = Main tab
        viewModel.selectMainTab()
        return
    }
    
    // Cmd+2-9 = Script tabs (0-indexed from index 1)
    let tabIndex = number - 2
    guard tabIndex < viewModel.scriptTabs.count else { return }
    viewModel.selectedTabId = viewModel.scriptTabs[tabIndex].id
    viewModel.persistScriptTabs()
}