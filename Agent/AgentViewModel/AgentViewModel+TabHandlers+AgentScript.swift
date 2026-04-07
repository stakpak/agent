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
                let err = "Error: script '\(scriptName)' not found."
                tab.appendLog(err)
                tab.flush()
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err],
                    isComplete: false
                )
            }

            // Skip compilation if dylib is up to date.
            // MUST run via executeTCC (in-process) so swift build inherits the
            // main app's TCC grants for ~/Documents access. The Launch Agent
            // path (executeForTab → userService.execute) runs in a separate
            // TCC context that can't getcwd() inside ~/Documents/AgentScript/.
            if await Self.offMain({ [ss = scriptService] in !ss.isDylibCurrent(name: scriptName) }) {
                tab.appendLog("🦾 Compiling: \(scriptName)")
                tab.flush()

                let compileResult = await Self.executeTCC(command: compileCmd)
                guard !Task.isCancelled else {
                    return TabToolResult(
                        toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "Script cancelled"],
                        isComplete: false
                    )
                }

                if compileResult.status != 0 {
                    tab.appendLog("❌ Compile failed (exit code: \(compileResult.status))")
                    tab.appendOutput(compileResult.output)
                    tab.flush()
                    let toolOutput = compileResult.output.isEmpty
                        ? "(compile failed, exit code: \(compileResult.status))"
                        : compileResult.output
                    return TabToolResult(
                        toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": String(toolOutput.prefix(10000))],
                        isComplete: false
                    )
                }
            }

            tab.appendLog("🦾 Running: \(scriptName)")
            tab.isRunning = true
            tab.flush()
            RecentAgentsService.shared.recordRun(agentName: scriptName, arguments: arguments, prompt: "run \(scriptName) \(arguments)")

            tab.resetLLMStreamCounters()
            let cancelFlag = tab._cancelFlag
            let runResult = await scriptService.loadAndRunScriptViaProcess(
                name: scriptName,
                arguments: arguments,
                captureStderr: scriptCaptureStderr,
                isCancelled: { cancelFlag.value }
            ) { [weak tab] chunk in
                Task { @MainActor in
                    tab?.appendOutput(chunk)
                }
            }

            tab.isRunning = false
            tab.flush()
            let isUsageOutput = runResult.output.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("Usage:")
            let statusNote = runResult.status == 0 ? "completed" : (isUsageOutput ? "usage" : "exit code: \(runResult.status)")
            tab.appendLog("\(scriptName) \(statusNote)")
            tab.flush()

            // Update agent menu status based on outcome
            let wasCancelled = tab.isCancelled || runResult.status == 15
            if wasCancelled {
                RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .cancelled)
            } else if isUsageOutput || runResult.status != 0 {
                RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .failed)
            } else {
                RecentAgentsService.shared.updateStatus(agentName: scriptName, arguments: arguments, status: .success)
            }

            let toolOutput = runResult.output.isEmpty
                ? "(no output, exit code: \(runResult.status))"
                : runResult.output
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": String(toolOutput.prefix(10000))],
                isComplete: false
            )

        default:
        let output = await executeNativeTool(name, input: input)
        tab.appendLog(output); tab.flush()
        return tabResult(output, toolId: toolId)
        }
    }
}
