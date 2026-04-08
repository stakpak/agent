
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import AgentAccess
import Cocoa

// MARK: - Native Tool Handler — Git / batch_commands / wait

extension AgentViewModel {

    /// Handles git_* tool calls plus batch_commands and wait/sleep/pause.
    /// Returns `nil` if the name is not a git-group tool.
    func handleGitNativeTool(name: String, input: [String: Any]) async -> String? {
        let pf = projectFolder
        switch name {
        // Git tools (expanded from git(action:X) → git_X)
        case "git_status":
            let dir = CodingService.resolveDir(pf.isEmpty ? nil : pf)
            let cmd = CodingService.buildGitStatusCommand(path: pf.isEmpty ? nil : pf)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            return result.output.isEmpty ? "(no output)" : result.output
        case "git_diff":
            let staged = input["staged"] as? Bool ?? false
            let target = input["target"] as? String
            let dir = CodingService.resolveDir(pf.isEmpty ? nil : pf)
            let cmd = CodingService.buildGitDiffCommand(path: pf.isEmpty ? nil : pf, staged: staged, target: target)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            return result.output.isEmpty ? "(no changes)" : result.output
        case "git_log":
            let count = input["count"] as? Int
            let dir = CodingService.resolveDir(pf.isEmpty ? nil : pf)
            let cmd = CodingService.buildGitLogCommand(path: pf.isEmpty ? nil : pf, count: count)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            return result.output.isEmpty ? "(no commits)" : result.output
        case "git_commit":
            let message = input["message"] as? String ?? "Update"
            let files = input["files"] as? [String]
            let dir = CodingService.resolveDir(pf.isEmpty ? nil : pf)
            let cmd = CodingService.buildGitCommitCommand(path: pf.isEmpty ? nil : pf, message: message, files: files)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            return result.output.isEmpty ? "(no output)" : result.output
        case "git_branch":
            let branchName = input["name"] as? String ?? ""
            let checkout = input["checkout"] as? Bool ?? false
            let dir = CodingService.resolveDir(pf.isEmpty ? nil : pf)
            let cmd = CodingService.buildGitBranchCommand(path: pf.isEmpty ? nil : pf, name: branchName, checkout: checkout)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            return result.output.isEmpty ? "(no output)" : result.output
        case "git_diff_patch":
            let target = input["target"] as? String
            let dir = CodingService.resolveDir(pf.isEmpty ? nil : pf)
            let cmd = CodingService.buildGitDiffCommand(path: pf.isEmpty ? nil : pf, staged: false, target: target)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            return result.output.isEmpty ? "(no changes)" : result.output
        // Batch commands — single bash process so env vars / cwd / functions persist
        // across steps. Per-step output is split out via delimiter markers so the LLM
        // sees which command produced what (and the per-command exit code).
        case "batch_commands":
            let commands = (input["commands"] as? String ?? "")
                .components(separatedBy: "\n")
                .filter {
                    !$0.trimmingCharacters(in: .whitespaces).isEmpty
                }
            guard !commands.isEmpty else { return "(no commands)" }
            for (idx, cmd) in commands.enumerated() {
                appendLog("🔧 [\(idx+1)/\(commands.count)] $ \(cmd)")
            }
            flushLog()
            let delim = "===AGENT_BATCH_STEP_\(UUID().uuidString.prefix(8))==="
            var script = ""
            for cmd in commands {
                script += "\(cmd)\n"
                script += "printf '\\n%s:%d\\n' '\(delim)' $?\n"
            }
            let fullCmd = Self.prependWorkingDirectory(script, projectFolder: pf)
            let result = await executeViaUserAgent(command: fullCmd)

            // Split per-step using the delimiter
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
                    batchOutput += "[\(idx + 1)] $ \(cmd)\n"
                    if remaining.isEmpty {
                        batchOutput += "(no output — batch aborted before this step ran)\n\n"
                    } else {
                        batchOutput += "(batch aborted, trailing output below)\n\(remaining)\n\n"
                        remaining = ""
                    }
                }
            }
            return batchOutput.isEmpty ? "(no output)" : batchOutput
        // Wait/pause for accessibility automation
        case "wait", "sleep", "pause":
            let seconds = input["seconds"] as? Double ?? input["duration"] as? Double ?? 3
            let capped = min(seconds, 30) // max 30 seconds
            try? await Task.sleep(for: .seconds(capped))
            return "Waited \(capped) seconds"
        default:
            return nil
        }
    }
}
