
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import AgentAccess
import Cocoa

// MARK: - Native Tool Handler — Xcode / batch_tools / web_search / lookup_sdef

extension AgentViewModel {

    /// Handles Xcode-related tool calls plus batch_tools, web_search, and looku
    func handleXcodeNativeTool(name: String, input: [String: Any]) async -> String? {
        switch name {
        // MARK: - Xcode Tools
        case "xcode_build":
            let projectPath = input["project_path"] as? String ?? ""
            // Background mode: spawn a fresh script tab and run xcodebuild ther
            if input["background"] as? Bool == true {
                let label = projectPath.isEmpty ? "Build" : ((projectPath as NSString).lastPathComponent as NSString).deletingPathExtension
                let spawnedTab = openScriptTab(scriptName: "xcode_build:\(label)", selectTab: false)
                spawnedTab.projectFolder = projectFolder
                spawnedTab.isRunning = true
                spawnedTab.appendLog("🦾 Spawned xcode_build from main task")
                spawnedTab.flush()
                Task { [weak self, weak spawnedTab] in
                    guard let self, let spawnedTab else { return }
                    await MainActor.run {
                        spawnedTab.appendLog("🦾 Running: xcodebuild \(projectPath)")
                        spawnedTab.flush()
                    }
                    let result = await Self.offMain { XcodeService.shared.buildProject(projectPath: projectPath) }
                    await MainActor.run {
                        spawnedTab.appendOutput(result)
                        spawnedTab.isRunning = false
                        spawnedTab.exitCode = result.contains("BUILD SUCCEEDED") ? 0 : 1
                        spawnedTab.appendLog(result.contains("BUILD SUCCEEDED") ? "BUILD SUCCEEDED" : "BUILD FAILED")
                        spawnedTab.flush()
                    }
                    // Auto-checkpoint logic (same as foreground path)
                    if result.contains("BUILD SUCCEEDED"), !self.projectFolder.isEmpty {
                        let dir = self.projectFolder
                        let check = await Self.offMain {
                            let p = Process()
                            p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
                            p.arguments = ["status", "--porcelain"]
                            p.currentDirectoryURL = URL(fileURLWithPath: dir)
                            let pipe = Pipe()
                            p.standardOutput = pipe; p.standardError = pipe
                            try? p.run(); p.waitUntilExit()
                            return String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
                        }
                        if !check.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                            _ = await Self.offMain {
                                let p = Process()
                                p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
                                p.arguments = ["add", "-A"]
                                p.currentDirectoryURL = URL(fileURLWithPath: dir)
                                try? p.run(); p.waitUntilExit()
                                let c = Process()
                                c.executableURL = URL(fileURLWithPath: "/usr/bin/git")
                                c.arguments = ["commit", "-m", "WIP: auto-checkpoint after successful build"]
                                c.currentDirectoryURL = URL(fileURLWithPath: dir)
                                try? c.run(); c.waitUntilExit()
                            }
                        }
                    }
                }
                appendLog("🚀 Started xcode_build in background tab 'xcode_build:\(label)'")
                flushLog()
                return "🚀 Started xcode_build in background script tab 'xcode_build:\(label)'. Output streams to that tab — switch to it to monitor progress. The current task continues. Recovery: if the build is still running and you need the result, switch to that tab; if you need to retry without backgrounding pass background:false."
            }
            let buildResult = await Self.offMain { XcodeService.shared.buildProject(projectPath: projectPath) }
            // Git auto-checkpoint after successful build
            if buildResult.contains("BUILD SUCCEEDED") && !projectFolder.isEmpty {
                let dir = projectFolder
                let checkResult = await Self.offMain {
                    let p = Process()
                    p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
                    p.arguments = ["status", "--porcelain"]
                    p.currentDirectoryURL = URL(fileURLWithPath: dir)
                    let pipe = Pipe()
                    p.standardOutput = pipe; p.standardError = pipe
                    try? p.run(); p.waitUntilExit()
                    return String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
                }
                if !checkResult.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    _ = await Self.offMain {
                        let p = Process()
                        p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
                        p.arguments = ["add", "-A"]
                        p.currentDirectoryURL = URL(fileURLWithPath: dir)
                        try? p.run(); p.waitUntilExit()
                        let c = Process()
                        c.executableURL = URL(fileURLWithPath: "/usr/bin/git")
                        c.arguments = ["commit", "-m", "WIP: auto-checkpoint after successful build"]
                        c.currentDirectoryURL = URL(fileURLWithPath: dir)
                        try? c.run(); c.waitUntilExit()
                    }
                }
            }
            // Auto-verify: launch app and capture initial UI state (opt-in)
            if buildResult.contains("BUILD SUCCEEDED") && autoVerifyEnabled {
                appendLog("🔍 Auto-verify: launching app...")
                flushLog()
                let runResult = await Self.offMain { XcodeService.shared.runProject(projectPath: projectPath) }
                // Wait for app to launch
                try? await Task.sleep(for: .seconds(2))
                // Capture accessibility tree of launched app
                let ax = AccessibilityService.shared
                let windows = ax.listWindows(limit: 5)
                let verifyReport = """
                    BUILD SUCCEEDED

                    Auto-verify:
                    - App launched: \(runResult.prefix(200))
                    - Windows: \(windows.prefix(500))
                    """
                return verifyReport
            }
            return buildResult
        case "xcode_run":
            let projectPath = input["project_path"] as? String ?? ""
            return await Self.offMain { XcodeService.shared.runProject(projectPath: projectPath) }
        case "xcode_list_projects":
            return await Self.offMain { XcodeService.shared.listProjects() }
        case "xcode_select_project":
            let number = input["number"] as? Int ?? 0
            return await Self.offMain { XcodeService.shared.selectProject(number: number) }
        case "xcode_grant_permission":
            return await Self.offMain { XcodeService.shared.grantPermission() }
        case "xcode_add_file":
            let fp = input["file_path"] as? String ?? ""
            return await Self.offMain { XcodeService.shared.addFileToProject(filePath: fp) }
        case "xcode_remove_file":
            let fp = input["file_path"] as? String ?? ""
            return await Self.offMain { XcodeService.shared.removeFileFromProject(filePath: fp) }
        case "xcode_bump_version":
            let delta = input["delta"] as? Int ?? 1
            return await Self.offMain { XcodeService.shared.bumpVersion(delta: delta) }
        case "xcode_bump_build":
            let delta = input["delta"] as? Int ?? 1
            return await Self.offMain { XcodeService.shared.bumpBuild(delta: delta) }
        case "xcode_get_version":
            return await Self.offMain { XcodeService.shared.getVersionInfo() }
        case "xcode_analyze":
            let fp = input["file_path"] as? String ?? ""
            guard !fp.isEmpty else { return "Error: file_path is required for analyze. Recovery: pass the Swift file path, or use file(action:\"list\", pattern:\"*.swift\") to find files." }
            guard let data = FileManager.default.contents(atPath: fp),
                  let content = String(data: data, encoding: .utf8) else {
                return "Error: could not read \(fp). Recovery: use file(action:\"list\") to verify the path."
            }
            // Basic Swift analysis — check for common issues
            let lines = content.components(separatedBy: "\n")
            var issues: [String] = []
            for (i, line) in lines.enumerated() {
                let trimmed = line.trimmingCharacters(in: .whitespaces)
                if trimmed.contains("force_cast") || trimmed.contains("as!") { issues.append("[Warning] Line \(i+1): Force cast (as!)") }
                if trimmed.contains("try!") { issues.append("[Warning] Line \(i+1): Force try (try!)") }
                if trimmed.contains("implicitly unwrapped")
                    || (trimmed.contains("!")
                        && trimmed.contains("var ")
                        && trimmed.contains(": ")) { }
                if trimmed.count > 200 { issues.append("[Style] Line \(i+1): Line too long (\(trimmed.count) chars)") }
            }
            return issues.isEmpty ? "No issues found in \(fp) (\(lines.count) lines)" : issues.joined(separator: "\n")
        case "xcode_snippet":
            let fp = input["file_path"] as? String ?? ""
            guard !fp.isEmpty else { return "Error: file_path is required for snippet. Recovery: pass the file path, or use file(action:\"list\") to find files." }
            guard let data = FileManager.default.contents(atPath: fp),
                  let content = String(data: data, encoding: .utf8) else {
                return "Error: could not read \(fp). Recovery: use file(action:\"list\") to verify the path."
            }
            let lines = content.components(separatedBy: "\n")
            let s = (input["start_line"] as? Int ?? 1)
            let e = (input["end_line"] as? Int ?? lines.count)
            let start = max(s - 1, 0)
            let end = min(e, lines.count)
            guard start < end else { return "Error: invalid line range \(s)-\(e) (file has \(lines.count) lines). Recovery: use file(action:\"read\", file_path:\"\(fp)\") to see line count." }
            let ext = (fp as NSString).pathExtension
            let snippet = lines[start..<end].enumerated().map { "\(start + $0 + 1)\t\($1)" }.joined(separator: "\n")
            return "```\(ext)\n\(snippet)\n```"
        // batch_tools — run multiple tool calls in one batch
        case "batch_tools":
            let desc = input["description"] as? String ?? "Batch Tasks"
            guard let tasks = input["tasks"] as? [[String: Any]] else {
                return "Error: tasks must be an array of {\"tool\": \"name\", \"input\": {...}} objects"
            }
            var batchOutput = "● \(desc) (\(tasks.count) tasks)\n"
            var completed = 0
            for (idx, task) in tasks.enumerated() {
                var subName = task["tool"] as? String ?? ""
                var subInput = task["input"] as? [String: Any] ?? [:]
                if subName == "batch_tools" || subName == "batch_commands" || subName == "task_complete" {
                    batchOutput += "[\(idx + 1)] \(subName): skipped (not allowed in batch)\n"
                    continue
                }
                (subName, subInput) = Self.expandConsolidatedTool(name: subName, input: subInput)
                let output = await executeNativeTool(subName, input: subInput)
                completed += 1
                batchOutput += "[\(idx + 1)] \(subName): \(output)\n"
            }
            batchOutput += "● \(completed)/\(tasks.count) tasks completed"
            return batchOutput
        // web_search
        case "web_search":
            let query = input["query"] as? String ?? ""
            guard !query.isEmpty else { return "Error: query is required" }
            return await Self.performWebSearchForTask(query: query, apiKey: tavilyAPIKey, provider: selectedProvider)
        // lookup_sdef — supports single bundle ID
        case "lookup_sdef":
            let bundleIDInput = input["bundle_id"] as? String ?? ""
            let bundleIDArray = input["bundle_id"] as? [String]
            let className = input["class_name"] as? String

            // Resolve into a list of bundle IDs.
            let bundleIDs: [String] = {
                if let arr = bundleIDArray {
                    return arr.map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
                }
                return bundleIDInput
                    .split(separator: ",")
                    .map { $0.trimmingCharacters(in: .whitespaces) }
                    .filter { !$0.isEmpty }
            }()

            if bundleIDs.contains("list") || bundleIDInput == "list" {
                let names = SDEFService.shared.availableSDEFs()
                return "Available SDEFs (\(names.count)):\n" + names.joined(separator: "\n")
            }

            // Single bundle ID — preserve the original drill-in behavior.
            if bundleIDs.count == 1 {
                let bundleID = bundleIDs[0]
                if let cls = className {
                    let props = SDEFService.shared.properties(for: bundleID, className: cls)
                    let elems = SDEFService.shared.elements(for: bundleID, className: cls)
                    var lines = ["\(cls) properties:"]
                    for p in props {
                        let ro = p.readonly == true ? " (readonly)" : ""
                        let desc = p.description.map { " — \($0)" } ?? ""
                        lines.append(
                            "  .\(SDEFService.toCamelCase(p.name)): "
                            + "\(p.type ?? "any")\(ro)\(desc)")
                    }
                    if !elems.isEmpty { lines.append("elements: \(elems.joined(separator: ", "))") }
                    return lines.isEmpty ? "No class '\(cls)' found for \(bundleID)" : lines.joined(separator: "\n")
                }
                return SDEFService.shared.summary(for: bundleID)
            }

            // Multiple bundle IDs — concatenate summaries with clear headers. c
            if bundleIDs.isEmpty {
                return "Error: bundle_id required (single ID, comma-separated list, array, or 'list' to enumerate the catalog)"
            }
            var blocks: [String] = []
            if className != nil {
                blocks.append("⚠️ class_name is ignored when multiple bundle_ids are provided. Drill into a class with a single bundle_id call.")
            }
            for bundleID in bundleIDs {
                let summary = SDEFService.shared.summary(for: bundleID)
                blocks.append("=== \(bundleID) ===\n\(summary)")
            }
            return blocks.joined(separator: "\n\n")
        default:
            return nil
        }
    }
}
