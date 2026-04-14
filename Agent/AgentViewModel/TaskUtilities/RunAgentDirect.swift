
@preconcurrency import Foundation
import AgentTools
import AgentAudit
import AgentMCP
import AgentD1F
import Cocoa

// MARK: - Task : Direct Agent Execution

extension AgentViewModel {

    // MARK: - Direct Agent Execution (no LLM)

    /// / Run an agent script directly
    @discardableResult
    func runAgentDirect(name: String, arguments: String = "", switchToTab: Bool = true) async -> Bool {
        let resolved = await Self.offMain { [ss = scriptService] in ss.resolveScriptName(name) }
        guard let compileCmd = await Self.offMain({ [ss = scriptService] in ss.compileCommand(name: resolved) }) else {
            appendLog("❌ agent '\(resolved)' not found.")
            return false
        }

        AuditLog.log(.agentScript, "runAgentDirect: start \(resolved)")

        // Close any existing tab for this agent and open fresh
        if let existing = scriptTabs.first(where: { $0.scriptName == resolved }) {
            closeScriptTab(id: existing.id)
        }
        let tab = openScriptTab(scriptName: resolved, selectTab: switchToTab)

        // Log on main tab so user sees something — main tab is now free
        appendLog("🏃 \(resolved)... (see tab)")
        flushLog()
        isRunning = false

        // Fire and forget — run in the tab's own Task, main tab doesn't wait
        Task { [weak self] in
            guard let self else { return }
            await self.executeAgentInTab(tab: tab, name: resolved, arguments: arguments, compileCmd: compileCmd)
        }
        return true
    }

    /// Execute the agent script inside its tab
    private func executeAgentInTab(tab: ScriptTab, name: String, arguments: String, compileCmd: String) async {

        let prompt = arguments.isEmpty ? "run \(name)" : "run \(name) \(arguments)"
        tab.addToHistory(prompt)

        tab.isRunning = true
        tab.taskStartDate = Date()
        tab._taskElapsedFrozen = 0
        tab.isLLMRunning = false
        tab.isLLMThinking = false
        tab.appendLog("--- Direct Run ---")

        // Compile only if needed. Run via executeTCC
        if await Self.offMain({ [ss = scriptService] in !ss.isDylibCurrent(name: name) }) {
            tab.appendLog("🦾 Compiling: \(name)")
            tab.flush()
            let compileResult = await Self.executeTCC(command: compileCmd)
            if compileResult.status != 0 {
                tab.appendLog("❌ Compile error:\n\(compileResult.output)")
                tab.flush()
                tab._taskElapsedFrozen = tab.taskElapsed
                tab.taskStartDate = nil
                tab.isRunning = false
                return
            }
        }

        tab.appendLog("🦾 Running: \(name)")
        tab.flush()
        RecentAgentsService.shared.recordRun(agentName: name, arguments: arguments, prompt: prompt)

        let cancelFlag = tab._cancelFlag
        let runResult = await scriptService.loadAndRunScriptViaProcess(
            name: name,
            arguments: arguments,
            projectFolder: tab.projectFolder,
            isCancelled: { cancelFlag.value }
        ) { [weak tab] chunk in
            Task { @MainActor in
                tab?.appendOutput(chunk)
            }
        }

        tab.flush()
        let success = runResult.status == 0
        let isUsageOutput = runResult.output.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("Usage:")
        let statusNote = success ? "completed" : (isUsageOutput ? "usage" : "exit code: \(runResult.status)")
        tab.appendLog("\(name) \(statusNote)")
        tab.flush()
        tab._taskElapsedFrozen = tab.taskElapsed
        tab.taskStartDate = nil
        tab.isRunning = false

        let wasCancelled = tab.isCancelled || runResult.status == 15
        if wasCancelled {
            RecentAgentsService.shared.updateStatus(agentName: name, arguments: arguments, status: .cancelled)
        } else if isUsageOutput || !success {
            RecentAgentsService.shared.updateStatus(agentName: name, arguments: arguments, status: .failed)
        } else {
            RecentAgentsService.shared.updateStatus(agentName: name, arguments: arguments, status: .success)
        }
    }
}
