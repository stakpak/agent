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
            // batch_commands runs every step inside the SAME bash process so env vars,
            // cwd changes, exported functions, and aliases all persist across steps.
            // Without this the LLM has to manually && everything together because
            // STAGING="..." in step 1 wouldn't survive into step 2.
            //
            // How it works: build a single shell script with all commands separated by
            // a unique delimiter line that captures each command's exit code, run it as
            // ONE bash invocation, then split the aggregated output on the delimiter to
            // attribute per-step output and rc back to each command for the UI display.
            let tabFolder = Self.resolvedWorkingDirectory(tab.projectFolder.isEmpty ? projectFolder : tab.projectFolder)
            let rawCommands = input["commands"] as? String ?? ""
            let commands = rawCommands.components(separatedBy: "\n").filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
            guard !commands.isEmpty else {
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "(no commands)"],
                    isComplete: false
                )
            }

            // Pre-flight: check every command for tool suggestions / unsafe patterns
            // BEFORE running anything. If any are blocked, return all the blocks at once
            // so the LLM can fix them in a single retry instead of half-running the batch.
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

            // Log each step before running so the user sees the plan
            for (idx, cmd) in commands.enumerated() {
                tab.appendLog("🔧 [\(idx + 1)/\(commands.count)] $ \(Self.collapseHeredocs(cmd))")
            }
            tab.flush()

            // Build the single-process script. After each command, print a unique
            // delimiter line followed by the exit code so the parser can split per-step.
            // Using printf (not echo) to guarantee the format isn't mangled by aliases.
            let delim = "===AGENT_BATCH_STEP_\(UUID().uuidString.prefix(8))==="
            var script = ""
            for cmd in commands {
                script += "\(cmd)\n"
                script += "printf '\\n%s:%d\\n' '\(delim)' $?\n"
            }

            guard !Task.isCancelled else { return TabToolResult(toolResult: nil, isComplete: false) }

            // executeForTab handles the cd-prepend + TCC routing internally; passing
            // the full multi-line script lets every step run in the same shell process.
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

            // Split the aggregated output on the delimiter and attribute per-step.
            var batchOutput = ""
            var remaining = result.output
            for (idx, cmd) in commands.enumerated() {
                if let range = remaining.range(of: "\(delim):") {
                    let stepOutput = String(remaining[remaining.startIndex..<range.lowerBound])
                    let afterDelim = remaining[range.upperBound...]
                    let nlIdx = afterDelim.firstIndex(of: "\n") ?? afterDelim.endIndex
                    let rc = Int(afterDelim[afterDelim.startIndex..<nlIdx]) ?? 0

                    let trimmed = stepOutput.trimmingCharacters(in: CharacterSet(charactersIn: "\n"))
                    batchOutput += "[\(idx + 1)] $ \(cmd)\n"
                    if rc != 0 { batchOutput += "exit code: \(rc)\n" }
                    batchOutput += (trimmed.isEmpty ? "(no output)" : trimmed) + "\n\n"

                    remaining = nlIdx < afterDelim.endIndex
                        ? String(afterDelim[afterDelim.index(after: nlIdx)...])
                        : ""
                } else {
                    // Bash bailed out before this step printed its delimiter (syntax
                    // error in an earlier command, exit, etc.). Show whatever's left.
                    batchOutput += "[\(idx + 1)] $ \(cmd)\n"
                    if remaining.isEmpty {
                        batchOutput += "(no output — batch aborted before this step ran)\n\n"
                    } else {
                        batchOutput += "(batch aborted, trailing output below)\n\(remaining)\n\n"
                        remaining = ""
                    }
                }
            }

            // Truncate if batch output is too large
            let truncated = LogLimits.trim(
                batchOutput,
                cap: LogLimits.batchOutputChars,
                suffix: "Batch output truncated."
            )
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
            // TCC commands MUST run in-process where Agent! holds the
            // user's TCC grants. This check has to come BEFORE the
            // privileged-daemon branch — otherwise an
            // `execute_daemon_command(command:"osascript ...")` would go to
            // the root daemon (which has zero TCC) and fail with a
            // confusing permission error.
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
                // TCC commands → Agent process (inherits TCC permissions).
                // Wins over the privileged check by design — TCC grants
                // belong to the GUI app, not to the root daemon.
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
