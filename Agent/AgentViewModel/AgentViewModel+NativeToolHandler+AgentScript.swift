
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import AgentAccess
import Cocoa

// MARK: - Native Tool Handler — Agent Scripts (CRUDL + run)

extension AgentViewModel {

    /// Handles agent_script CRUDL + run/restore/pull/combine tool calls.
    /// Returns `nil` if the name is not an agent-script tool.
    func handleAgentScriptNativeTool(name: String, input: [String: Any]) async -> String? {
        switch name {
        // Script management
        case "list_agents":
            let scripts = await Self.offMain { [ss = scriptService] in ss.listScripts() }
            return scripts.isEmpty ? "No scripts found" : scripts.map { "\($0.name) (\($0.size) bytes)" }.joined(separator: "\n")
        case "run_agent":
            let rawRunName = input["name"] as? String ?? ""
            let scriptName = await Self.offMain { [ss = scriptService] in ss.resolveScriptName(rawRunName) }
            let arguments = input["arguments"] as? String ?? ""
            guard let compileCmd = await Self.offMain({ [ss = scriptService] in ss.compileCommand(name: scriptName) }) else {
                return "Error: script '\(scriptName)' not found. Recovery: call agent_script(action:\"list\") to see available scripts, or agent_script(action:\"pull\", name:\"\(scriptName)\") if you expected an upstream script."
            }

            // Dedup: close any existing background tab for this script before
            // spawning a fresh one. Matches the user-direct runAgentDirect
            // behavior — repeated runs replace rather than pile up duplicate
            // tabs with the same scriptName. Skip main tabs.
            if let existing = scriptTabs.first(where: { $0.scriptName == scriptName && !$0.isMainTab }) {
                closeScriptTab(id: existing.id)
            }
            // Spawn a fresh ScriptTab so the calling main task is not blocked.
            // The script runs in a detached Task and streams output to the
            // spawned tab; this handler returns immediately.
            let spawnedTab = openScriptTab(scriptName: scriptName, selectTab: false)
            spawnedTab.projectFolder = projectFolder
            spawnedTab.isRunning = true
            spawnedTab.appendLog("🦾 Spawned from main task")
            spawnedTab.flush()
            RecentAgentsService.shared.recordRun(agentName: scriptName, arguments: arguments, prompt: "run \(scriptName) \(arguments)")

            let stderrCapture = scriptCaptureStderr
            Task { [weak self, weak spawnedTab] in
                guard let self, let spawnedTab else { return }

                // Compile only if needed. MUST run via executeTCC (in-process)
                // so swift build inherits the main app's TCC grants for
                // ~/Documents access.
                if await Self.offMain({ [ss = self.scriptService] in !ss.isDylibCurrent(name: scriptName) }) {
                    await MainActor.run {
                        spawnedTab.appendLog("🦾 Compiling: \(scriptName)")
                        spawnedTab.flush()
                    }
                    let compileResult = await Self.executeTCC(command: compileCmd)
                    if compileResult.status != 0 {
                        await MainActor.run {
                            spawnedTab.appendLog("❌ Compile failed (exit code: \(compileResult.status))")
                            spawnedTab.appendOutput(compileResult.output)
                            spawnedTab.isRunning = false
                            spawnedTab.exitCode = compileResult.status
                            spawnedTab.flush()
                        }
                        RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .failed)
                        return
                    }
                }

                await MainActor.run {
                    spawnedTab.appendLog("🦾 Running: \(scriptName)")
                    spawnedTab.flush()
                }

                let cancelFlag = spawnedTab._cancelFlag
                let runResult = await self.scriptService.loadAndRunScriptViaProcess(
                    name: scriptName,
                    arguments: arguments,
                    projectFolder: spawnedTab.projectFolder,
                    captureStderr: stderrCapture,
                    isCancelled: { cancelFlag.value }
                ) { [weak spawnedTab] chunk in
                    Task { @MainActor in
                        spawnedTab?.appendOutput(chunk)
                    }
                }

                let isUsageOutput = runResult.output.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("Usage:")
                let statusNote = runResult.status == 0 ? "completed" : (isUsageOutput ? "usage" : "exit code: \(runResult.status)")
                let wasCancelled = await MainActor.run { spawnedTab.isCancelled } || runResult.status == 15

                await MainActor.run {
                    spawnedTab.isRunning = false
                    spawnedTab.exitCode = runResult.status
                    spawnedTab.appendLog("\(scriptName) \(statusNote)")
                    spawnedTab.flush()
                }

                if wasCancelled {
                    RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .cancelled)
                } else if isUsageOutput || runResult.status != 0 {
                    RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .failed)
                } else {
                    RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .success)
                }
            }

            appendLog("🚀 Started '\(scriptName)' in background tab")
            flushLog()
            return "🚀 Started '\(scriptName)' in background script tab '\(scriptName)'. Output streams to that tab — switch to it to monitor progress. The current task continues."
        case "read_agent":
            let readName = input["name"] as? String ?? ""
            return await Self.offMain { [ss = scriptService] in ss.readScript(name: readName) ?? "Not found" }
        case "create_agent", "update_agent":
            let createName = input["name"] as? String ?? ""
            let createContent = input["content"] as? String ?? ""
            return await Self.offMain { [ss = scriptService] in ss.createScript(name: createName, content: createContent) }
        case "delete_agent":
            let deleteName = input["name"] as? String ?? ""
            return await Self.offMain { [ss = scriptService] in ss.deleteScript(name: deleteName) }
        case "restore_agent":
            let restoreName = input["name"] as? String ?? ""
            let backupFilename = input["backup"] as? String
            return await Self.offMain { [ss = scriptService] in
                ss.restoreScript(name: restoreName, backupFilename: backupFilename)
            }
        case "list_agent_backups":
            let filterName = input["name"] as? String ?? ""
            let backups = await Self.offMain { [ss = scriptService] in ss.listScriptBackups(name: filterName) }
            if backups.isEmpty {
                return filterName.isEmpty
                    ? "No script backups found."
                    : "No backups found for '\(filterName)'."
            }
            return backups.map { $0.lastPathComponent }.joined(separator: "\n")
        case "pull_agent":
            let pullName = input["name"] as? String ?? ""
            return await scriptService.pullScriptFromRemote(name: pullName)
        case "combine_agents":
            let sourceA = input["source_a"] as? String ?? ""
            let sourceB = input["source_b"] as? String ?? ""
            let target = input["target"] as? String ?? ""
            guard let contentA = await Self.offMain(
                { [ss = scriptService] in ss.readScript(name: sourceA) }
            ) else { return "Error: script '\(sourceA)' not found." }
            guard let contentB = await Self.offMain(
                { [ss = scriptService] in ss.readScript(name: sourceB) }
            ) else { return "Error: script '\(sourceB)' not found." }
            let merged = Self.combineScriptSources(contentA: contentA, contentB: contentB, sourceA: sourceA, sourceB: sourceB)
            if await Self.offMain({ [ss = scriptService] in ss.readScript(name: target) }) != nil {
                return await Self.offMain { [ss = scriptService] in ss.updateScript(name: target, content: merged) }
            } else {
                return await Self.offMain { [ss = scriptService] in ss.createScript(name: target, content: merged) }
            }
        default:
            return nil
        }
    }
}
