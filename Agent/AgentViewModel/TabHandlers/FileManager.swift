@preconcurrency import Foundation
import AgentMCP
import AgentD1F
import Cocoa

extension AgentViewModel {

    /// Handle FileManager tool calls for tab tasks.
    func handleTabFileManagerTool(
        tab: ScriptTab, name: String, input rawInput: [String: Any], toolId: String
    ) async -> TabToolResult {

        // Resolve relative file_path against project folder
        var input = rawInput
        if let fp = input["file_path"] as? String, !fp.isEmpty, !fp.hasPrefix("/"), !fp.hasPrefix("~") {
            let base = projectFolder.isEmpty ? NSHomeDirectory() : projectFolder
            input["file_path"] = (base as NSString).appendingPathComponent(fp)
        }

        switch name {
        case "read_file":
            let filePath = input["file_path"] as? String ?? ""
            let offset = input["offset"] as? Int
            let limit = input["limit"] as? Int
            tab.appendLog("📖 Read: \(filePath)")
            let output = await Self.offMain { CodingService.readFile(path: filePath, offset: offset, limit: limit) }
            // Cap file output at 50K chars for LLM context
            let capped = LogLimits.trim(
                output,
                cap: LogLimits.readFileChars,
                lineCount: output.components(separatedBy: "\n").count,
                suffix: "Use offset/limit to read specific sections."
            )
            let lang = Self.langFromPath(filePath)
            tab.appendLog(Self.codeFence(Self.preview(output, lines: readFilePreviewLines), language: lang))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": capped],
                isComplete: false
            )

        case "write_file":
            let filePath = input["file_path"] as? String ?? ""
            let content = input["content"] as? String ?? ""
            let expandedWrite = (filePath as NSString).expandingTildeInPath
            FileBackupService.shared.backup(filePath: expandedWrite, tabID: tab.id)
            tab.appendLog("📝 Write: \(filePath)")
            let output = await Self.offMain { CodingService.writeFile(path: filePath, content: content) }

            tab.appendLog(output)
            let lang = Self.langFromPath(filePath)
            tab.appendLog(Self.codeFence(Self.preview(content, lines: readFilePreviewLines), language: lang))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "edit_file":
            let filePath = input["file_path"] as? String ?? ""
            let oldString = input["old_string"] as? String ?? ""
            let newString = input["new_string"] as? String ?? ""
            let replaceAll = input["replace_all"] as? Bool ?? false
            let context = input["context"] as? String
            FileBackupService.shared.backup(filePath: (filePath as NSString).expandingTildeInPath, tabID: tab.id)
            tab.appendLog("📝 Edit: \(filePath)")
            // Capture original for undo
            let expandedPath = (filePath as NSString).expandingTildeInPath
            let originalContent: String? = await Self.offMain {
                guard let data = FileManager.default.contents(atPath: expandedPath),
                      let text = String(data: data, encoding: .utf8) else { return nil }
                return text
            }
            let output = await Self.offMain { CodingService.editFile(
                path: filePath,
                oldString: oldString,
                newString: newString,
                replaceAll: replaceAll,
                context: context
            ) }
            // Always invalidate read cache after edit_file

            if !output.hasPrefix("Error"), let original = originalContent {
                DiffStore.shared.recordEdit(filePath: expandedPath, originalContent: original)
                let diff = MultiLineDiff.createDiff(source: oldString, destination: newString, includeMetadata: true)
                let d1f = MultiLineDiff.displayDiff(diff: diff, source: oldString, format: .ai)
                if !d1f.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    let startLine = (diff.metadata?.sourceStartLine ?? 0) + 1
                    let numbered = Self.numberDiffLines(d1f, startLine: startLine)
                    tab.appendLog(numbered)
                }
            }
            let outLines = output.components(separatedBy: "\n")
            let status = outLines.first ?? output
            let lineInfo = outLines.last(where: { $0.hasPrefix("📍") }) ?? ""
            tab.appendLog(lineInfo.isEmpty ? status : "\(status) \(lineInfo)")
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "create_diff":
            var source = input["source"] as? String ?? ""
            let destination = input["destination"] as? String ?? ""
            let startLine = input["start_line"] as? Int
            let endLine = input["end_line"] as? Int
            if let fp = input["file_path"] as? String, !fp.isEmpty {
                let expanded = (fp as NSString).expandingTildeInPath
                if let data = FileManager.default.contents(atPath: expanded),
                   let text = String(data: data, encoding: .utf8)
                {
                    if let sl = startLine, let el = endLine {
                        let lines = text.components(separatedBy: "\n")
                        let s = max(sl - 1, 0)
                        let e = min(el, lines.count)
                        source = lines[s..<e].joined(separator: "\n")
                    } else {
                        source = text
                    }
                }
            }
            let algorithm = CodingService.selectDiffAlgorithm(source: source, destination: destination)
            let diff = MultiLineDiff.createDiff(
                source: source,
                destination: destination,
                algorithm: algorithm,
                includeMetadata: true,
                sourceStartLine: startLine.map { $0 - 1 }
            )
            let d1f = MultiLineDiff.displayDiff(diff: diff, source: source, format: .ai)
            let diffId = DiffStore.shared.store(diff: diff, source: source)
            tab.appendLog(d1f)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "diff_id: \(diffId.uuidString)\n\n\(d1f)"],
                isComplete: false
            )

        case "apply_diff":
            let filePath = input["file_path"] as? String ?? ""
            let diffIdStr = input["diff_id"] as? String ?? ""
            let asciiDiff = input["diff"] as? String ?? ""
            tab.appendLog("📝 Apply D1F diff: \(filePath)")
            let expandedPath = (filePath as NSString).expandingTildeInPath
            guard let data = FileManager.default.contents(atPath: expandedPath),
                  let source = String(data: data, encoding: .utf8) else
            {
                let err = "Error: cannot read \(filePath)"
                tab.appendLog(err)
                tab.flush()
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err],
                    isComplete: false
                )
            }
            do {
                let patched: String
                if let uuid = UUID(uuidString: diffIdStr),
                   let stored = DiffStore.shared.retrieve(uuid)
                {
                    patched = try MultiLineDiff.applyDiff(to: source, diff: stored.diff)
                } else if !asciiDiff.isEmpty {
                    patched = try MultiLineDiff.applyASCIIDiff(to: source, asciiDiff: asciiDiff)
                } else {
                    throw DiffError.invalidDiff
                }
                try patched.write(to: URL(fileURLWithPath: expandedPath), atomically: true, encoding: .utf8)
    
                DiffStore.shared.recordEdit(filePath: expandedPath, originalContent: source)
                let verifyResult = MultiLineDiff.createDiff(source: source, destination: patched, includeMetadata: true)
                let verified = MultiLineDiff.verifyDiff(verifyResult)
                let verifyDiff = MultiLineDiff.displayDiff(diff: verifyResult, source: source, format: .ai)
                tab.appendLog(verifyDiff)
                let newLineCount = patched.components(separatedBy: "\n").count
                let output =
                    "Applied diff to \(filePath) [verified: \(verified)] "
                    + "— file now has \(newLineCount) lines. "
                    + "Re-read the file before making more edits."
                DiffStore.shared.invalidateDiffs(for: expandedPath)
                tab.appendLog(output)
                tab.flush()
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "\(output)\n\n\(verifyDiff)"],
                    isComplete: false
                )
            } catch {
                let err = "Error applying diff: \(error.localizedDescription)"
                tab.appendLog(err)
                tab.flush()
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err],
                    isComplete: false
                )
            }

        case "undo_edit":
            let filePath = input["file_path"] as? String ?? ""
            let expandedUndo = (filePath as NSString).expandingTildeInPath
            guard let original = DiffStore.shared.lastEdit(for: expandedUndo) else {
                let err = "Error: no edit history for \(filePath)"
                tab.appendLog(err); tab.flush()
                return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err], isComplete: false)
            }
            tab.appendLog("↩️ Undo: \(filePath)")
            let undoOutput = await Self.offMain { CodingService.undoEdit(path: filePath, originalContent: original) }
            if !undoOutput.hasPrefix("Error") { DiffStore.shared.clearEditHistory(for: expandedUndo) }
            tab.appendLog(undoOutput); tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": undoOutput], isComplete: false)

        case "diff_and_apply":
            let filePath = input["file_path"] as? String ?? ""
            let destination = input["destination"] as? String ?? ""
            let startLine = input["start_line"] as? Int
            let endLine = input["end_line"] as? Int
            let rangeNote = (startLine != nil && endLine != nil) ? " (lines \(startLine!)-\(endLine!))" : ""

            let expanded = (filePath as NSString).expandingTildeInPath
            guard let daData = FileManager.default.contents(atPath: expanded),
                  let fullText = String(data: daData, encoding: .utf8) else
            {
                let err = "Error: cannot read \(filePath)"
                tab.appendLog(err); tab.flush()
                return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err], isComplete: false)
            }

            // Step 1: Extract source section
            let source: String
            if let sl = startLine, let el = endLine {
                let lines = fullText.components(separatedBy: "\n")
                let s = max(sl - 1, 0)
                let e = min(el, lines.count)
                source = lines[s..<e].joined(separator: "\n")
            } else {
                source = fullText
            }

            if source == destination {
                tab.appendLog("❌ source and destination are identical"); tab.flush()
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "Error: source and destination are identical"],
                    isComplete: false
                )
            }

            // Step 2: Create diff with metadata
            let algorithm = CodingService.selectDiffAlgorithm(source: source, destination: destination)
            let diff = MultiLineDiff.createDiff(
                source: source,
                destination: destination,
                algorithm: algorithm,
                includeMetadata: true,
                sourceStartLine: startLine.map { $0 - 1 }
            )
            let diffId = DiffStore.shared.store(diff: diff, source: source)

            // Step 3: Apply diff
            do {
                let patched = try MultiLineDiff.applyDiff(to: source, diff: diff)

                // No truncation guard — d1f's structural verification + applyDi

                // Splice back into full file if line range
                let finalContent: String
                if let sl = startLine, let el = endLine {
                    var allLines = fullText.components(separatedBy: "\n")
                    let s = max(sl - 1, 0)
                    let e = min(el, allLines.count)
                    allLines.replaceSubrange(s..<e, with: patched.components(separatedBy: "\n"))
                    finalContent = allLines.joined(separator: "\n")
                } else {
                    finalContent = patched
                }

                try finalContent.write(to: URL(fileURLWithPath: expanded), atomically: true, encoding: .utf8)
                DiffStore.shared.recordApply(diffId: diffId, filePath: expanded, originalContent: fullText)

                let verifyDiff = MultiLineDiff.createDiff(source: source, destination: patched, includeMetadata: true)
                let verified = MultiLineDiff.verifyDiff(verifyDiff)
                let display = MultiLineDiff.displayDiff(diff: verifyDiff, source: source, format: .ai)
                let newLineCount = finalContent.components(separatedBy: "\n").count
                tab.appendLog(display)
                tab.appendLog("📝 Diff+Apply: \(filePath)\(rangeNote) [verified: \(verified)] (\(newLineCount) lines)")
                DiffStore.shared.invalidateDiffs(for: expanded)
                tab.flush()
                return TabToolResult(
                    toolResult: [
                        "type": "tool_result",
                        "tool_use_id": toolId,
                        "content":
                            "Applied diff to \(filePath)\(rangeNote) "
                            + "[verified: \(verified)] — file now has "
                            + "\(newLineCount) lines. Re-read the file "
                            + "before making more edits. "
                            + "diff_id: \(diffId.uuidString)\n\n\(display)"
                    ],
                    isComplete: false
                )
            } catch {
                let err = "Error applying diff: \(error.localizedDescription)"
                tab.appendLog(err); tab.flush()
                return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err], isComplete: false)
            }

        case "list_files":
            let pattern = input["pattern"] as? String ?? "*.swift"
            if pattern == "*" || pattern == "*.*" {
                let err =
                    "Error: pattern '*' is too broad. "
                    + "Use a file extension like '*.swift', '*.json', '*.py'. "
                    + "Example: list_files(pattern: \"*.swift\")"
                tab.appendLog(err); tab.flush()
                return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err], isComplete: false)
            }
            let path = input["path"] as? String
            let tabFolder = Self.resolvedWorkingDirectory(tab.projectFolder.isEmpty ? projectFolder : tab.projectFolder)
            let resolvedDir = path ?? tabFolder
            let displayDir = CodingService.trimHome(resolvedDir)
            tab.appendLog("🔍 $ find \(displayDir) -name '\(pattern)'")
            tab.flush()
            let cmd = CodingService.buildListFilesCommand(pattern: pattern, path: resolvedDir)
            let result = await executeForTab(command: cmd, projectFolder: resolvedDir)
            guard !Task.isCancelled else { return TabToolResult(toolResult: nil, isComplete: false) }
            let raw = result.output.trimmingCharacters(in: .whitespacesAndNewlines)
            let formatted = raw.isEmpty ? "No files matching '\(pattern)'" : CodingService.formatFileTree(raw)
            tab.appendLog(formatted)
            tab.flush()
            let output = raw.isEmpty ? formatted : "[project folder: \(displayDir)] paths are relative to project folder\n\(formatted)"
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "search_files":
            let pattern = input["pattern"] as? String ?? ""
            let path = input["path"] as? String
            let include = input["include"] as? String
            let tabFolder = Self.resolvedWorkingDirectory(tab.projectFolder.isEmpty ? projectFolder : tab.projectFolder)
            let resolvedSearch = path ?? tabFolder
            let displaySearch = CodingService.trimHome(resolvedSearch)
            tab.appendLog("🔍 $ grep -rn '\(pattern)' \(displaySearch)")
            tab.flush()
            let cmd = CodingService.buildSearchFilesCommand(pattern: pattern, path: resolvedSearch, include: include)
            let result = await executeForTab(command: cmd, projectFolder: resolvedSearch)
            guard !Task.isCancelled else { return TabToolResult(toolResult: nil, isComplete: false) }
            let output = result.output.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                ? "No matches for '\(pattern)'" :
                "[project folder: \(displaySearch)] paths are relative to project folder\n\(result.output)"
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "mkdir":
            let rawPath = input["path"] as? String ?? ""
            guard !rawPath.isEmpty else {
                let err = "Error: path is required"
                tab.appendLog(err); tab.flush()
                return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err], isComplete: false)
            }
            let tabFolder = Self.resolvedWorkingDirectory(tab.projectFolder.isEmpty ? projectFolder : tab.projectFolder)
            let resolved = rawPath.hasPrefix("/") || rawPath.hasPrefix("~") ? rawPath : (tabFolder as NSString)
                .appendingPathComponent(rawPath)
            tab.appendLog("📁 mkdir -p \(CodingService.trimHome(resolved))")
            tab.flush()
            let result = await executeForTab(
                command: "mkdir -p \(CodingService.shellEscape(resolved)) && echo 'Created: \(resolved)'",
                projectFolder: tabFolder
            )
            let out = result.output.trimmingCharacters(in: .whitespacesAndNewlines)
            let output = out.isEmpty ? "Error creating directory" : out
            tab.appendLog(output); tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "restore_file":
            // Recover the most recent backup of a file (or a specific backup) f
            let filePath = input["file_path"] as? String ?? ""
            let backupName = input["backup"] as? String
            let expanded = (filePath as NSString).expandingTildeInPath
            let fileName = (expanded as NSString).lastPathComponent
            tab.appendLog("↩️ Restore: \(filePath)")
            tab.flush()
            let backups = FileBackupService.shared.listBackups(tabID: tab.id)
                .filter { $0.original == fileName }
            let output: String
            if let explicit = backupName {
                if let match = backups.first(where: { ($0.backup as NSString).lastPathComponent == explicit }) {
                    if FileBackupService.shared.restore(backupPath: match.backup, to: expanded) {
                        output = "Restored \(fileName) from \(explicit)."
                    } else {
                        output = "Error: failed to restore from \(explicit). Recovery: call file(action:\"list_backups\", file_path:\"\(filePath)\") to verify the backup still exists."
                    }
                } else {
                    output = "Error: backup '\(explicit)' not found for \(fileName). Recovery: call file(action:\"list_backups\", file_path:\"\(filePath)\") to see available backups."
                }
            } else if let latest = backups.first {
                if FileBackupService.shared.restore(backupPath: latest.backup, to: expanded) {
                    output = "Restored \(fileName) from latest backup (\(latest.date))."
                } else {
                    output = "Error: failed to restore latest backup of \(fileName). Recovery: call file(action:\"list_backups\", file_path:\"\(filePath)\") to inspect backups, or use undo_edit if recent."
                }
            } else {
                output = "Error: no backups found for \(fileName). Recovery: file backups are tab-scoped and 1-week TTL — try undo_edit if the change was recent, or git checkout if the file is in a repo."
            }
            tab.appendLog(output); tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "list_file_backups":
            let filePath = input["file_path"] as? String ?? ""
            let expanded = (filePath as NSString).expandingTildeInPath
            let fileName = (expanded as NSString).lastPathComponent
            let backups = FileBackupService.shared.listBackups(tabID: tab.id)
                .filter { fileName.isEmpty || $0.original == fileName }
            let output: String
            if backups.isEmpty {
                output = fileName.isEmpty
                    ? "No file backups in this tab."
                    : "No backups found for \(fileName)."
            } else {
                output = backups.map { "\(($0.backup as NSString).lastPathComponent)  (\($0.date))" }.joined(separator: "\n")
            }
            tab.appendLog("🗂️ Backups: \(backups.count)")
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        default:
            let output = await executeNativeTool(name, input: input)
            tab.appendLog(output); tab.flush()
            return tabResult(output, toolId: toolId)
        }
    }
}
