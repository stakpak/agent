@preconcurrency import Foundation
import AgentTools
import AgentColorSyntax
import AgentTerminalNeo
import AgentLLM
import AppKit

extension AgentViewModel {
    // MARK: - Init Helpers

    /// Theme + script + system prompt bootstrap (called from init)
    func bootstrapThemeAndScripts() {
        activityLog = ChatHistoryStore.shared.buildActivityLogText(maxTasks: 3)
        // Trim main tab log on relaunch
        activityLog = ScriptTab.trimLog(activityLog)
        CodeBlockTheme.updateAppearance()
        TerminalNeoTheme.updateAppearance()
        // Restore ~/Documents/AgentScript/ folder and bundled resources if missing (off main thread)
        Task.detached { [scriptService = self.scriptService] in
            scriptService.ensurePackage()
            scriptService.rebuildAllMetadata()
            // After ensurePackage, refresh upstream-bundled scripts when Agent!
            // has been upgraded since the last sync. User-authored scripts are never touched, and any modified bundled sc...
            await scriptService.syncBundledScriptsFromRemote()
            let names = Set(scriptService.listScripts().map { $0.name.lowercased() })
            await MainActor.run { AppleIntelligenceMediator.knownAgentNames = names }
        }
        SystemPromptService.shared.ensureDefaults()
    }

    /// Messages monitor restoration on startup (called from init)
    func restoreMessagesMonitor() {
        if messagesMonitorEnabled {
            refreshMessageRecipients()
        }

        // Resume Messages monitor if it was enabled
        if messagesMonitorEnabled {
            // Delay start so UserService is connected first
            Task {
                try? await Task.sleep(nanoseconds: 3_000_000_000)
                startMessagesMonitor()
            }
        }
    }

    /// Startup ping / warmup task (called from init)
    func startupPingWarmup() {
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

    // MARK: - Provider selection dispatch

    func fetchModelsForSelectedProviderIfNeeded() {
        fetchModelsIfNeeded(for: selectedProvider)
    }
}
