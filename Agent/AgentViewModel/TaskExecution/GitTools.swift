@preconcurrency import Foundation


// MARK: - Git Tool Execution
extension AgentViewModel {

    /// Handles git tool calls routed through User LaunchAgent via XPC.
    /// Includes: git_status, git_diff, git_log, git_commit, git_diff_patch, git_branch
    @MainActor
    func handleGitTool(
        name: String,
        input: [String: Any],
        toolId: String,
        projectFolder: String,
        appendLog: @escaping @MainActor @Sendable (String) -> Void,
        flushLog: @escaping @MainActor @Sendable () -> Void,
        commandsRun: inout [String],
        toolResults: inout [[String: Any]]
    ) async -> Bool {
        guard name.hasPrefix("git_") else { return false }

        // MARK: git_status
        switch name {

        case "git_status":
            let path = input["path"] as? String
            if let pathErr = Self.checkPath(path) {
                appendLog(pathErr)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": pathErr])
                return true
            }
            appendLog("🔀 $ git status\(path.map { " (\($0))" } ?? "")")
            flushLog()
            let dir = CodingService.resolveDir(path)
            let cmd = CodingService.buildGitStatusCommand(path: path)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            guard !Task.isCancelled else { return true }
            let output = result.output.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                ? "(no output, exit code: \(result.status))" : result.output
            toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": output])

            return true

        // MARK: git_diff
        case "git_diff":
            let path = input["path"] as? String
            let staged = input["staged"] as? Bool ?? false
            let target = input["target"] as? String
            if let pathErr = Self.checkPath(path) {
                appendLog(pathErr)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": pathErr])
                return true
            }
            appendLog("🔀 $ git diff\(staged ? " --cached" : "")\(target.map { " \($0)" } ?? "")")
            flushLog()
            let dir = CodingService.resolveDir(path)
            let cmd = CodingService.buildGitDiffCommand(path: path, staged: staged, target: target)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            guard !Task.isCancelled else { return true }
            let output: String
            if result.output.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                output = staged ? "No staged changes" : "No changes"
                appendLog(output)
            } else {
                output = result.output
            }
            flushLog()
            toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": output])

            return true

        // MARK: git_log
        case "git_log":
            let path = input["path"] as? String
            let count = input["count"] as? Int
            if let pathErr = Self.checkPath(path) {
                appendLog(pathErr)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": pathErr])
                return true
            }
            appendLog("🔀 $ git log\(path.map { " (\($0))" } ?? "")")
            flushLog()
            let dir = CodingService.resolveDir(path)
            let cmd = CodingService.buildGitLogCommand(path: path, count: count)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            guard !Task.isCancelled else { return true }
            let output: String
            if result.output.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                output = "Error: \(result.status == 0 ? "empty log" : "exit code \(result.status)")"
                appendLog(output)
            } else {
                output = result.output
            }
            flushLog()
            toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": output])

            return true

        // MARK: git_commit
        case "git_commit":
            let path = input["path"] as? String
            let message = input["message"] as? String ?? ""
            let files = input["files"] as? [String]
            if let pathErr = Self.checkPath(path) {
                appendLog(pathErr)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": pathErr])
                return true
            }

            // Git safety guardrails — block dangerous patterns
            let dangerousFlags = ["--no-verify", "--amend", "--force", "-f", "--no-gpg-sign"]
            for flag in dangerousFlags where message.contains(flag) {
                let warning = "⚠️ Blocked: '\(flag)' is not allowed in commit messages. Create a new commit instead."
                appendLog(warning)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": warning])
                return true
            }

            // Warn on files that likely contain secrets
            let secretPatterns = [".env", "credentials", "secret", ".pem", ".key", "token"]
            if let filesToCommit = files {
                let suspectFiles = filesToCommit.filter { f in secretPatterns.contains(where: { f.lowercased().contains($0) }) }
                if !suspectFiles.isEmpty {
                    let warning =
                        "⚠️ Warning: committing files that may contain "
                            + "secrets: \(suspectFiles.joined(separator: ", ")). "
                            + "Proceeding anyway — review before pushing."
                    appendLog(warning)
                    flushLog()
                }
            }

            appendLog("🔀 Git commit: \(message)")
            flushLog()
            let dir = CodingService.resolveDir(path)
            let cmd = CodingService.buildGitCommitCommand(path: path, message: message, files: files)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            guard !Task.isCancelled else { return true }
            if !result.output.isEmpty { appendLog(result.output) }
            commandsRun.append("git_commit: \(message)")
            let output = result.output.isEmpty
                ? "(no output, exit code: \(result.status))"
                : result.output
            toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": output])

            return true

        // MARK: git_diff_patch
        case "git_diff_patch":
            let path = input["path"] as? String
            let patch = input["patch"] as? String ?? ""
            appendLog("🔧 git apply patch")
            flushLog()
            let dir = CodingService.resolveDir(path)
            // Write patch to temp file, apply, clean up
            let tempName = "agent_patch_\(UUID().uuidString).patch"
            let tempPath = "/tmp/\(tempName)"
            let cmd =
                "cat > \(tempPath) << 'AGENT_PATCH_EOF'\n\(patch)\n"
                    + "AGENT_PATCH_EOF\ngit apply --verbose \(tempPath); "
                    + "STATUS=$?; rm -f \(tempPath); exit $STATUS"
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            guard !Task.isCancelled else { return true }
            if !result.output.isEmpty { appendLog(result.output) }
            commandsRun.append("git_diff_patch")
            let output: String
            if result.status != 0 {
                output = result.output.isEmpty ? "Patch failed (exit code: \(result.status))" : result.output
            } else {
                output = result.output.isEmpty ? "Patch applied successfully" : result.output
            }
            toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": output])

            return true

        // MARK: git_branch
        case "git_branch":
            let path = input["path"] as? String
            let branchName = input["name"] as? String ?? ""
            let checkout = input["checkout"] as? Bool ?? true
            guard !branchName.isEmpty else {
                let err = "Error: branch name is empty. Recovery: pass name:\"my-branch\" to git(action:\"branch\", name:\"...\")."
                appendLog(err)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
                return true
            }
            if let pathErr = Self.checkPath(path) {
                appendLog(pathErr)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": pathErr])
                return true
            }
            appendLog("🔧 git branch: \(branchName)")
            flushLog()
            let dir = CodingService.resolveDir(path)
            let cmd = CodingService.buildGitBranchCommand(path: path, name: branchName, checkout: checkout)
            let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
            guard !Task.isCancelled else { return true }
            if !result.output.isEmpty { appendLog(result.output) }
            commandsRun.append("git_branch: \(branchName)")
            let output = result.output.isEmpty
                ? (result.status == 0 ? "Created branch '\(branchName)'" : "Error (exit code: \(result.status))")
                : result.output
            toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": output])

            return true

        // MARK: git_worktree
        case "git_worktree":
            let path = input["path"] as? String
            let action = input["action"] as? String ?? "list"
            let branchName = input["name"] as? String ?? ""
            if let pathErr = Self.checkPath(path) {
                appendLog(pathErr)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": pathErr])
                return true
            }
            let dir = CodingService.resolveDir(path)

            switch action {
            case "create":
                guard !branchName.isEmpty else {
                    let err = "Error: 'name' is required to create a worktree."
                    toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
                    return true
                }
                // Sanitize branch name
                let sanitized = branchName.replacingOccurrences(of: "[^a-zA-Z0-9._-]", with: "-", options: .regularExpression)
                let worktreesDir = AgentProjectPaths.path(in: dir, .worktrees)
                let wtPath = "\(worktreesDir)/\(sanitized)"
                appendLog("🌳 Creating worktree: \(sanitized)")
                flushLog()
                let cmd = "mkdir -p \"\(worktreesDir)\" && git worktree add -b \"\(sanitized)\" \"\(wtPath)\" 2>&1"
                let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
                let output = result.status == 0 ? "Worktree created at \(wtPath) on branch \(sanitized)" : result.output
                commandsRun.append("git_worktree: create \(sanitized)")
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": output])
                return true

            case "remove":
                guard !branchName.isEmpty else {
                    let err = "Error: 'name' is required to remove a worktree."
                    toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
                    return true
                }
                let wtPath = "\(AgentProjectPaths.path(in: dir, .worktrees))/\(branchName)"
                appendLog("🌳 Removing worktree: \(branchName)")
                flushLog()
                let cmd = "git worktree remove \"\(wtPath)\" --force 2>&1"
                let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
                let output = result.status == 0 ? "Worktree '\(branchName)' removed" : result.output
                commandsRun.append("git_worktree: remove \(branchName)")
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": output])
                return true

            default: // list
                appendLog("🌳 Listing worktrees")
                flushLog()
                let cmd = "git worktree list 2>&1"
                let result = await executeViaUserAgent(command: cmd, workingDirectory: dir)
                let output = result.output.isEmpty ? "No worktrees" : result.output
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": output])
                return true
            }

        default:
            return false
        }
    }
}
