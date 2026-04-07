import SwiftUI
import AppKit

// MARK: - Keyboard Shortcuts Extension

extension ContentView {
    /// Handle keyboard events for shortcuts
    func handleKeyEvent(_ event: NSEvent) -> NSEvent? {
        // Cmd+W to close tab or quit
        if event.modifierFlags.contains(.command),
           event.charactersIgnoringModifiers == "w" {
            if let selId = viewModel.selectedTabId {
                viewModel.closeScriptTab(id: selId)
            } else if viewModel.scriptTabs.isEmpty {
                showQuitConfirm = true
            }
            return nil
        }

        // Cmd+T to create a new main LLM tab
        if event.modifierFlags.contains(.command),
           event.charactersIgnoringModifiers == "t" {
            showNewTabSheet = true
            return nil
        }

        // Cmd+F to toggle search bar
        if event.modifierFlags.contains(.command),
           event.charactersIgnoringModifiers == "f" {
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
           event.charactersIgnoringModifiers == "v" {
            if viewModel.pasteImageFromClipboard() {
                return nil
            }
        }
        
        // Keyboard shortcuts for common actions
        // Cmd+N: New task (focus input)
        if event.modifierFlags.contains(.command),
           event.charactersIgnoringModifiers == "n" {
            // Focus is already on text field, this is just a quick way to clear and start new
            return nil
        }
        
        // Cmd+R: Run current task
        if event.modifierFlags.contains(.command),
           event.charactersIgnoringModifiers == "r" {
            if let selId = viewModel.selectedTabId,
               let tab = viewModel.tab(for: selId) {
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
           event.charactersIgnoringModifiers == "." {
            if let selId = viewModel.selectedTabId,
               let tab = viewModel.tab(for: selId),
               tab.isLLMRunning {
                viewModel.stopTabTask(tab: tab)
            } else if viewModel.isRunning {
                viewModel.stop()
            }
            return nil
        }
        
        // Cmd+Shift+P: Open System Prompts
        if event.modifierFlags.contains([.command, .shift]),
           event.charactersIgnoringModifiers == "p" {
            // System prompts window would be opened here
            // For now, focus on settings
            showSettings = true
            return nil
        }
        
        // Cmd+,: Open Settings
        if event.modifierFlags.contains(.command),
           event.charactersIgnoringModifiers == "," {
            showSettings = true
            return nil
        }
        
        // Cmd+D handled in ContentView.swift (toggles both LLM chevrons)

        // Cmd+H: Open History
        if event.modifierFlags.contains(.command),
           event.charactersIgnoringModifiers == "h" {
            showHistory = true
            return nil
        }
        
        // Cmd+L: Clear log (clear conversation)
        if event.modifierFlags.contains(.command),
           event.charactersIgnoringModifiers == "l" {
            showClearConfirm = true
            return nil
        }
        
        // Cmd+Shift+L: Toggle Apple Intelligence
        if event.modifierFlags.contains([.command, .shift]),
           event.charactersIgnoringModifiers == "l" {
            aiMediator.toggleEnabled()
            return nil
        }
        
        // Cmd+Shift+M: Open MCP Servers
        if event.modifierFlags.contains([.command, .shift]),
           event.charactersIgnoringModifiers == "m" {
            showMCPServers = true
            return nil
        }
        
        // Cmd+Shift+T: Open Tools
        if event.modifierFlags.contains([.command, .shift]),
           event.charactersIgnoringModifiers == "t" {
            showTools = true
            return nil
        }
        
        // Cmd+Shift+A: Open Accessibility
        if event.modifierFlags.contains([.command, .shift]),
           event.charactersIgnoringModifiers == "a" {
            showAccessibility = true
            return nil
        }
        
        // Cmd+Shift+S: Open Services
        if event.modifierFlags.contains([.command, .shift]),
           event.charactersIgnoringModifiers == "s" {
            showServices = true
            return nil
        }
        
        // Cmd+Shift+M: Toggle Messages Monitor
        if event.modifierFlags.contains([.command, .shift]),
           event.charactersIgnoringModifiers == "m" {
            viewModel.messagesMonitorEnabled.toggle()
            return nil
        }
        
        // Tab navigation: Cmd+[ and Cmd+] for next/previous tab
        if event.modifierFlags.contains(.command) {
            if event.keyCode == 30 { // [
                previousTab()
                return nil
            } else if event.keyCode == 46 { // ]
                nextTab()
                return nil
            }
        }
        
        // Cmd+Shift+]: Next tab (right arrow)
        if event.modifierFlags.contains(.command),
           event.keyCode == 124 {
            nextTab()
            return nil
        }
        
        // Cmd+Shift+[: Previous tab (left arrow)
        if event.modifierFlags.contains(.command),
           event.keyCode == 123 {
            previousTab()
            return nil
        }
        
        // Cmd+1-9: Switch between tabs
        if event.modifierFlags.contains(.command),
           let char = event.charactersIgnoringModifiers,
           let number = Int(char),
           number >= 1, number <= 9 {
            selectTab(number: number)
            return nil
        }
        
        // Escape key to cancel active context (tab or main)
        if event.keyCode == 53 {
            if let selId = viewModel.selectedTabId,
               let tab = viewModel.tab(for: selId),
               tab.isLLMRunning {
                viewModel.stopTabTask(tab: tab)
                return nil
            } else if viewModel.isRunning {
                viewModel.stop()
                return nil
            }
        }
        
        // Up/Down arrow for prompt history (per-tab or main)
        if event.keyCode == 126 || event.keyCode == 125 {
            let direction = event.keyCode == 126 ? -1 : 1
            if let tabId = viewModel.selectedTabId,
               let tab = viewModel.tab(for: tabId) {
                tab.navigateHistory(direction: direction)
            } else {
                viewModel.navigatePromptHistory(direction: direction)
            }
            return nil
        }
        
        return event
    }
    
    /// Navigate to next tab (cycle right)
    func nextTab() {
        if viewModel.scriptTabs.isEmpty { return }
        
        // On main tab - go to first script tab
        guard let currentId = viewModel.selectedTabId else {
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
    func previousTab() {
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
    func selectTab(number: Int) {
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
}