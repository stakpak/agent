@preconcurrency import Foundation
import AgentMCP
import AgentD1F
import Cocoa

extension AgentViewModel {

    /// Handle Git tool calls for tab tasks.
    func handleTabGitTool(
        tab: ScriptTab, name: String, input: [String: Any], toolId: String
    ) async -> TabToolResult {

        let tabFolder = Self.resolvedWorkingDirectory(tab.projectFolder.isEmpty ? projectFolder : tab.projectFolder)

        switch name {
        case "git_status":
            let path = input["path"] as? String
            tab.appendLog("🔀 $ git status")
            tab.flush()
            let cmd = CodingService.buildGitStatusCommand(path: path)
            let result = await executeForTab(command: cmd, projectFolder: tabFolder)
            guard !Task.isCancelled else { return TabToolResult(toolResult: nil, isComplete: false) }
            let output = result.output.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                ? "(no output, exit code: \(result.status))" : result.output
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "git_diff":
            let path = input["path"] as? String
            let staged = input["staged"] as? Bool ?? false
            let target = input["target"] as? String
            tab.appendLog("🔀 $ git diff\(staged ? " --cached" : "")")
            tab.flush()
            let cmd = CodingService.buildGitDiffCommand(path: path, staged: staged, target: target)
            let result = await executeForTab(command: cmd, projectFolder: tabFolder)
            guard !Task.isCancelled else { return TabToolResult(toolResult: nil, isComplete: false) }
            let output: String
            if result.output.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                output = staged ? "No staged changes" : "No changes"
            } else {
                output = result.output
            }
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "git_log":
            let path = input["path"] as? String
            let count = input["count"] as? Int
            tab.appendLog("🔀 $ git log")
            tab.flush()
            let cmd = CodingService.buildGitLogCommand(path: path, count: count)
            let result = await executeForTab(command: cmd, projectFolder: tabFolder)
            guard !Task.isCancelled else { return TabToolResult(toolResult: nil, isComplete: false) }
            let output = result.output.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                ? "Error: empty log" : result.output
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "git_commit":
            let path = input["path"] as? String
            let message = input["message"] as? String ?? ""
            let files = input["files"] as? [String]
            tab.appendLog("🔀 Git commit: \(message)")
            tab.flush()
            let cmd = CodingService.buildGitCommitCommand(path: path, message: message, files: files)
            let result = await executeForTab(command: cmd, projectFolder: tabFolder)
            guard !Task.isCancelled else { return TabToolResult(toolResult: nil, isComplete: false) }
            let output = result.output.isEmpty
                ? "(no output, exit code: \(result.status))" : result.output
            if !result.output.isEmpty { tab.appendLog(result.output) }
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        default:
        let output = await executeNativeTool(name, input: input)
        tab.appendLog(output); tab.flush()
        return tabResult(output, toolId: toolId)
        }
    }
}
