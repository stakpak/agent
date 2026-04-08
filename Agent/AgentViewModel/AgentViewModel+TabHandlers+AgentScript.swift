@preconcurrency import Foundation
import AgentMCP
import AgentD1F
import Cocoa

extension AgentViewModel {

    /// Handle AgentScript tool calls for tab tasks.
    func handleTabAgentScriptTool(
        tab: ScriptTab, name: String, input: [String: Any], toolId: String
    ) async -> TabToolResult {

        switch name {
        case "list_agents":
            let (output, count) = await Self.offMain { [ss = scriptService] in
                (ss.numberedList(), ss.listScripts().count)
            }
            tab.appendLog("🦾 Agents: \(count) found")
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "read_agent":
            let rawName = input["name"] as? String ?? ""
            let (scriptName, output) = await Self.offMain { [ss = scriptService] in
                let resolved = ss.resolveScriptName(rawName)
                return (resolved, ss.readScript(name: resolved) ?? "Error: script '\(resolved)' not found.")
            }
            tab.appendLog("📖 Read: \(scriptName)")
            tab.appendLog(Self.codeFence(output, language: "swift"))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "create_agent":
            let scriptName = input["name"] as? String ?? ""
            let content = input["content"] as? String ?? ""
            let output = await Self.offMain { [ss = scriptService] in ss.createScript(name: scriptName, content: content) }
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "update_agent":
            let rawUpdateName = input["name"] as? String ?? ""
            let scriptName = await Self.offMain { [ss = scriptService] in ss.resolveScriptName(rawUpdateName) }
            let content = input["content"] as? String ?? ""
            let output = await Self.offMain { [ss = scriptService] in ss.updateScript(name: scriptName, content: content) }
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "delete_agent":
            let rawDeleteName = input["name"] as? String ?? ""
            let scriptName = await Self.offMain { [ss = scriptService] in ss.resolveScriptName(rawDeleteName) }
            let output = await Self.offMain { [ss = scriptService] in ss.deleteScript(name: scriptName) }
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "restore_agent":
            let restoreName = input["name"] as? String ?? ""
            let backupFilename = input["backup"] as? String
            let output = await Self.offMain { [ss = scriptService] in
                ss.restoreScript(name: restoreName, backupFilename: backupFilename)
            }
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "list_agent_backups":
            let filterName = input["name"] as? String ?? ""
            let backups = await Self.offMain { [ss = scriptService] in ss.listScriptBackups(name: filterName) }
            let output: String
            if backups.isEmpty {
                output = filterName.isEmpty
                    ? "No script backups found."
                    : "No backups found for '\(filterName)'."
            } else {
                output = backups.map { $0.lastPathComponent }.joined(separator: "\n")
            }
            tab.appendLog("🗑️ Backups: \(backups.count)")
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "pull_agent":
            let pullName = input["name"] as? String ?? ""
            tab.appendLog("⬇️ Pulling \(pullName) from AgentScripts remote...")
            tab.flush()
            let output = await scriptService.pullScriptFromRemote(name: pullName)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "combine_agents":
            let rawSourceA = input["source_a"] as? String ?? ""
            let rawSourceB = input["source_b"] as? String ?? ""
            let sourceA = await Self.offMain { [ss = scriptService] in ss.resolveScriptName(rawSourceA) }
            let sourceB = await Self.offMain { [ss = scriptService] in ss.resolveScriptName(rawSourceB) }
            let target = input["target"] as? String ?? ""
            tab.appendLog("🔗 \(sourceA) + \(sourceB) → \(target)")

            guard let contentA = await Self.offMain({ [ss = scriptService] in ss.readScript(name: sourceA) }) else {
                let err = "Error: script '\(sourceA)' not found."
                tab.appendLog(err)
                return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err], isComplete: false)
            }
            guard let contentB = await Self.offMain({ [ss = scriptService] in ss.readScript(name: sourceB) }) else {
                let err = "Error: script '\(sourceB)' not found."
                tab.appendLog(err)
                return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err], isComplete: false)
            }

            let merged = Self.combineScriptSources(contentA: contentA, contentB: contentB, sourceA: sourceA, sourceB: sourceB)

            let output: String
            if await Self.offMain({ [ss = scriptService] in ss.readScript(name: target) }) != nil {
                output = await Self.offMain { [ss = scriptService] in ss.updateScript(name: target, content: merged) }
            } else {
                output = await Self.offMain { [ss = scriptService] in ss.createScript(name: target, content: merged) }
            }
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "run_agent":
            let rawRunName = input["name"] as? String ?? ""
            let scriptName = await Self.offMain { [ss = scriptService] in ss.resolveScriptName(rawRunName) }
            let arguments = input["arguments"] as? String ?? ""
            guard let compileCmd = await Self.offMain({ [ss = scriptService] in ss.compileCommand(name: scriptName) }) else {
                let err = "Error: script '\(scriptName)' not found. Recovery: call agent_script(action:\"list\") to see available scripts, or agent_script(action:\"pull\", name:\"\(scriptName)\") if you expected an upstream script."
                tab.appendLog(err)
                tab.flush()
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err],
                    isComplete: false
                )
            }

            // Dedup: close any existing background tab for this script before
            // spawning a fresh one. Matches the user-direct runAgentDirect
            // behavior — repeated runs replace rather than pile up duplicate
            // tabs with the same scriptName. Skip main tabs (those have their
            // own LLM config and shouldn't be auto-closed).
            if let existing = scriptTabs.first(where: { $0.scriptName == scriptName && !$0.isMainTab && $0.id != tab.id }) {
                closeScriptTab(id: existing.id)
            }
            // Spawn a fresh ScriptTab for the run so the calling LLM tab is
            // not blocked. The script runs in a detached Task and streams
            // output to the spawned tab; the calling tool returns immediately.
            let spawnedTab = openScriptTab(scriptName: scriptName, selectTab: false)
            // Inherit caller's project folder explicitly (openScriptTab uses
            // self.projectFolder, which may not match the calling tab when
            // tabs have diverged).
            spawnedTab.projectFolder = tab.projectFolder
            spawnedTab.isRunning = true
            spawnedTab.appendLog("🦾 Spawned from \(tab.scriptName)")
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

            tab.appendLog("🚀 Started '\(scriptName)' in background tab")
            tab.flush()
            let toolOutput = "🚀 Started '\(scriptName)' in background script tab '\(scriptName)'. Output streams to that tab — switch to it to monitor progress. The current task continues."
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": toolOutput],
                isComplete: false
            )

        default:
            let output = await executeNativeTool(name, input: input)
            tab.appendLog(output); tab.flush()
            return tabResult(output, toolId: toolId)
        }
    }
}
