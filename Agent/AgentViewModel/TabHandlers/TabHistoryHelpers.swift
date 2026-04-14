@preconcurrency import Foundation

extension AgentViewModel {
    /// Prompt history for whichever tab is currently selected.
    var currentTabPromptHistory: [String] {
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId)
        {
            return tab.promptHistory
        }
        return promptHistory
    }

    /// Display name for the currently selected tab.
    var currentTabName: String {
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId)
        {
            return tab.displayTitle
        }
        return "Main"
    }

    /// Error history for UI display
    var errorHistory: [String] {
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId),
           !tab.isMainTab
        {
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

    /// Task summaries for UI display
    var taskSummaries: [String] {
        if let selectedId = selectedTabId,
           let tab = tab(for: selectedId),
           !tab.isMainTab
        {
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
           let tab = tab(for: selectedId)
        {
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
           !tab.isMainTab
        {
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
}
