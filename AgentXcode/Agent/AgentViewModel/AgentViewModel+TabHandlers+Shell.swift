@preconcurrency import Foundation
import AgentMCP
import AgentD1F
import Cocoa

extension AgentViewModel {

    /// Handle Shell tool calls for tab tasks.
    func handleTabShellTool(
        tab: ScriptTab, name: String, input: [String: Any], toolId: String
    ) async -> TabToolResult {

        switch name {
        case "batch_commands":
            let tabFolder = Self.resolvedWorkingDirectory(tab.projectFolder.isEmpty ? projectFolder : tab.projectFolder)
            let rawCommands = input["commands"] as? String ?? ""
            let commands = rawCommands.components(separatedBy: "\n").filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
            var batchOutput = ""
            for (idx, rawCmd) in commands.enumerated() {
                guard !Task.isCancelled else { return TabToolResult(toolResult: nil, isComplete: false) }
                let cmd = Self.prependWorkingDirectory(rawCmd, projectFolder: tabFolder)
                if let suggestion = Self.suggestTool(cmd) {
                    batchOutput += "[\(idx + 1)] $ \(rawCmd)\n\(suggestion)\n\n"
                    continue
                }
                if let pathErr = Self.preflightCommand(cmd) {
                    batchOutput += "[\(idx + 1)] $ \(rawCmd)\n\(pathErr)\n\n"
                    continue
                }
                tab.appendLog("🔧 [\(idx + 1)/\(commands.count)] $ \(Self.collapseHeredocs(cmd))")
                tab.flush()

                // Route through same logic as execute_agent_command
                let result: (status: Int32, output: String)
                if Self.needsTCCPermissions(cmd) {
                    result = await Self.executeTCC(command: cmd)
                } else if userService.userReady {
                    result = await executeForTab(command: cmd, projectFolder: tabFolder)
                } else {
                    result = await Self.executeTCC(command: cmd)
                }

                let output = result.output.isEmpty ? "(no output)" : result.output
                batchOutput += "[\(idx + 1)] $ \(rawCmd)\n"
                if result.status > 0 { batchOutput += "exit code: \(result.status)\n" }
                batchOutput += output + "\n\n"
            }
            // Truncate if batch output is too large
            let truncated = batchOutput.count > 50_000
                ? String(batchOutput.prefix(50_000)) + "\n...(batch output truncated)"
                : batchOutput
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": truncated],
                isComplete: false
            )

        case "batch_tools":
            let desc = input["description"] as? String ?? "Batch Tasks"
            guard let tasks = input["tasks"] as? [[String: Any]] else {
                let err = "Error: tasks must be an array of {\"tool\": \"name\", \"input\": {...}} objects"
                tab.appendLog(err); tab.flush()
                return tabResult(err, toolId: toolId)
            }

            tab.appendLog("● \(desc) (\(tasks.count) tasks)")
            tab.flush()

            var batchOutput = ""
            var completed = 0
            for (idx, task) in tasks.enumerated() {
                guard !Task.isCancelled else { return TabToolResult(toolResult: nil, isComplete: false) }
                var toolName = task["tool"] as? String ?? ""
                var toolInput = task["input"] as? [String: Any] ?? [:]

                // Prevent recursion and dangerous nesting
                if toolName == "batch_tools" || toolName == "batch_commands" || toolName == "task_complete" {
                    batchOutput += "[\(idx + 1)] \(toolName): skipped (not allowed in batch)\n\n"
                    continue
                }

                // Expand consolidated tools
                (toolName, toolInput) = Self.expandConsolidatedTool(name: toolName, input: toolInput)

                let brief = Self.briefToolSummary(toolName, input: toolInput)
                tab.appendLog("├ [\(idx + 1)/\(tasks.count)] \(toolName)(\(brief))")
                tab.flush()

                // Dispatch through existing tab handler (suppresses sub-logging via synthetic toolId)
                let subResult = await handleTabToolCallBody(
                    tab: tab, name: toolName, input: toolInput, toolId: "\(toolId)_\(idx)"
                )

                let output = (subResult.toolResult?["content"] as? String) ?? "(no output)"
                completed += 1
                batchOutput += "[\(idx + 1)] \(toolName): \(output)\n\n"
            }

            tab.appendLog("● \(completed)/\(tasks.count) tasks completed")
            tab.flush()
            let truncatedBatch = batchOutput.count > 50_000
                ? String(batchOutput.prefix(50_000)) + "\n...(batch output truncated)"
                : batchOutput
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": truncatedBatch],
                isComplete: false
            )

        case "execute_agent_command", "execute_daemon_command", "run_shell_script":
            let tabFolder = Self.resolvedWorkingDirectory(tab.projectFolder.isEmpty ? projectFolder : tab.projectFolder)
            let command = Self.prependWorkingDirectory(
                input["command"] as? String ?? "", projectFolder: tabFolder)
            if let suggestion = Self.suggestTool(command) {
                tab.appendLog(suggestion)
                tab.flush()
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": suggestion],
                    isComplete: false
                )
            }
            if let pathErr = Self.preflightCommand(command) {
                tab.appendLog(pathErr)
                tab.flush()
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": pathErr],
                    isComplete: false
                )
            }
            let isPrivileged = (name == "execute_daemon_command") && rootEnabled
            tab.appendLog("\(isPrivileged ? "🔴 #" : "🔧 $") \(Self.collapseHeredocs(command))")
            tab.flush()

            let result: (status: Int32, output: String)
            if isPrivileged {
                // Root commands → LaunchDaemon via XPC
                result = await helperService.execute(command: command, workingDirectory: tabFolder)
            } else if Self.needsTCCPermissions(command) {
                // TCC commands → Agent process (inherits TCC permissions)
                result = await Self.executeTCC(command: command)
            } else if userService.userReady {
                // User LaunchAgent via XPC
                result = await executeForTab(command: command, projectFolder: tabFolder)
            } else {
                // Fallback: in-process when User Agent is off
                result = await Self.executeTCC(command: command)
            }

            guard !Task.isCancelled else { return TabToolResult(toolResult: nil, isComplete: false) }

            if result.status > 0 {
                tab.appendLog("exit code: \(result.status)")
            }

            let toolOutput: String
            if result.output.isEmpty {
                toolOutput = "(no output, exit code: \(result.status))"
            } else {
                toolOutput = result.output
            }

            tab.flush()
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
