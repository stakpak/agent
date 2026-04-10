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
            // batch_commands: runs all steps in ONE bash process so env vars, cwd, exports, and aliases persist across
            // steps. Builds a single script with unique delimiters to capture per-step exit codes, then splits output.
            let tabFolder = Self.resolvedWorkingDirectory(tab.projectFolder.isEmpty ? projectFolder : tab.projectFolder)
            let rawCommands = input["commands"] as? String ?? ""
            let commands = rawCommands.components(separatedBy: "\n").filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
            guard !commands.isEmpty else {
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "(no commands)"],
                    isComplete: false
                )
            }

            // Pre-flight: check every command for tool suggestions / unsafe patterns BEFORE running anything. If any
            // are blocked, return all the blocks at once so the LLM can fix them in a single retry instead of half-running the batch.
            var blocks = ""
            for (idx, rawCmd) in commands.enumerated() {
                let prefixed = Self.prependWorkingDirectory(rawCmd, projectFolder: tabFolder)
                if let suggestion = Self.suggestTool(prefixed) {
                    blocks += "[\(idx + 1)] $ \(rawCmd)\n\(suggestion)\n\n"
                } else if let pathErr = Self.preflightCommand(prefixed) {
                    blocks += "[\(idx + 1)] $ \(rawCmd)\n\(pathErr)\n\n"
                }
            }
            if !blocks.isEmpty {
                tab.appendLog("⚠️ Batch blocked by preflight checks")
                tab.flush()
                return TabToolResult(
                    toolResult: [
                        "type": "tool_result",
                        "tool_use_id": toolId,
                        "content": blocks + "(batch aborted — fix errors and retry)"
                    ],
                    isComplete: false
                )
            }

            tab.appendLog("🔧 batch_commands (\(commands.count) steps)")
            tab.flush()

            guard !Task.isCancelled else { return TabToolResult(toolResult: nil, isComplete: false) }

            // Run the whole batch as a single script — no per-step delimiters.
            // Per-step splitting broke multiline constructs (for/case/esac/done)
            // and leaked ===AGENT_BATCH_STEP=== markers into the activity log.
            let script = commands.joined(separator: "\n")
            let needsTCC = Self.needsTCCPermissions(script)
            let result: (status: Int32, output: String)
            if needsTCC {
                let prefixed = Self.prependWorkingDirectory(script, projectFolder: tabFolder)
                result = await Self.executeTCC(command: prefixed)
            } else if userService.userReady {
                result = await executeForTab(command: script, projectFolder: tabFolder)
            } else {
                let prefixed = Self.prependWorkingDirectory(script, projectFolder: tabFolder)
                result = await Self.executeTCC(command: prefixed)
            }

            let output = result.output.trimmingCharacters(in: .whitespacesAndNewlines)
            // Don't appendLog — the streaming callback already displayed the output.
            // Only build the tool_result for the LLM.
            var batchOutput = output.isEmpty ? "(no output)" : output
            if result.status != 0 { batchOutput += "\nexit code: \(result.status)" }
            let truncated = LogLimits.trim(batchOutput, cap: LogLimits.batchOutputChars, suffix: "Batch output truncated.")
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
            let truncatedBatch = LogLimits.trim(
                batchOutput,
                cap: LogLimits.batchOutputChars,
                suffix: "Batch output truncated."
            )
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": truncatedBatch],
                isComplete: false
            )

        case "execute_agent_command", "execute_daemon_command", "run_shell_script":
            let tabFolder = Self.resolvedWorkingDirectory(tab.projectFolder.isEmpty ? projectFolder : tab.projectFolder)
            let command = Self.prependWorkingDirectory(
                input["command"] as? String ?? "", projectFolder: tabFolder
            )
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
            // TCC commands MUST run in-process where Agent! holds the user's TCC grants. This check has to come BEFORE
            // the privileged-daemon branch — otherwise an `execute_daemon_command(command:"osascript ...")` would go to the root daemon (which has zero TCC) and fail with a confusing permission error.
            let needsTCC = Self.needsTCCPermissions(command)
            let isPrivileged = (name == "execute_daemon_command") && rootEnabled && !needsTCC
            let routePrefix: String
            if needsTCC {
                routePrefix = "🔧 $ (in-process for TCC)"
            } else if isPrivileged {
                routePrefix = "🔴 #"
            } else {
                routePrefix = "🔧 $"
            }
            tab.appendLog("\(routePrefix) \(Self.collapseHeredocs(command))")
            tab.flush()

            let result: (status: Int32, output: String)
            if needsTCC {
                // TCC commands → Agent process (inherits TCC permissions). Wins over the privileged check by design —
                // TCC grants belong to the GUI app, not to the root daemon.
                result = await Self.executeTCC(command: command)
            } else if isPrivileged {
                // Root commands → LaunchDaemon via XPC
                result = await helperService.execute(command: command, workingDirectory: tabFolder)
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
