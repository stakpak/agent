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
            let expandedRead = (filePath as NSString).expandingTildeInPath
            let cacheKey = Self.fileReadCacheKey(path: expandedRead, offset: offset, limit: limit)

            // Mtime-based dedup: skip disk I/O if same range read and file unchanged
            if let cached = Self.taskFileReadCache[cacheKey],
               let attrs = try? FileManager.default.attributesOfItem(atPath: expandedRead),
               let currentMtime = attrs[.modificationDate] as? Date,
               cached.mtime == currentMtime {
                let stub = "File unchanged since last read (\(cached.outputCharCount) chars). The content from the earlier Read tool_result in this conversation is still current — refer to that instead of re-reading."
                tab.appendLog("📖 (unchanged) \(filePath)")
                tab.flush()
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": stub],
                    isComplete: false
                )
            }

            tab.appendLog("📖 Read: \(filePath)")
            let output = await Self.offMain { CodingService.readFile(path: filePath, offset: offset, limit: limit) }
            // Store mtime for next dedup check
            if let attrs = try? FileManager.default.attributesOfItem(atPath: expandedRead),
               let mtime = attrs[.modificationDate] as? Date {
                Self.taskFileReadCache[cacheKey] = FileReadCacheEntry(mtime: mtime, outputCharCount: output.count)
            }
            let lang = Self.langFromPath(filePath)
            tab.appendLog(Self.codeFence(Self.preview(output, lines: readFilePreviewLines), language: lang))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "write_file":
            let filePath = input["file_path"] as? String ?? ""
            let content = input["content"] as? String ?? ""
            FileBackupService.shared.backup(filePath: (filePath as NSString).expandingTildeInPath, tabID: tab.id)
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
            let output = await Self.offMain { CodingService.editFile(path: filePath, oldString: oldString, newString: newString, replaceAll: replaceAll, context: context) }
            if !output.hasPrefix("Error"), let original = originalContent {
                DiffStore.shared.recordEdit(filePath: expandedPath, originalContent: original)
            }
            let diff = MultiLineDiff.createDiff(source: oldString, destination: newString, includeMetadata: true)
            var d1f = MultiLineDiff.displayDiff(diff: diff, source: oldString, format: .ai)
            if d1f.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                d1f = "❌" + oldString + "\n" + "✅" + newString
            }
            var diffLog = d1f
            if let meta = diff.metadata, let startLine = meta.sourceStartLine {
                diffLog += "\n📍 Changes start at line \(startLine + 1)"
                if let total = meta.sourceTotalLines {
                    diffLog += " (of \(total) lines)"
                }
            }
            tab.appendLog(diffLog)
            tab.appendLog(output)
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
                   let text = String(data: data, encoding: .utf8) {
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
            let diff = MultiLineDiff.createDiff(source: source, destination: destination, algorithm: algorithm, includeMetadata: true, sourceStartLine: startLine.map { $0 - 1 })
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
                  let source = String(data: data, encoding: .utf8) else {
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
                   let stored = DiffStore.shared.retrieve(uuid) {
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
                let output = "Applied diff to \(filePath) [verified: \(verified)] — file now has \(newLineCount) lines. Re-read the file before making more edits."
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
                  let fullText = String(data: daData, encoding: .utf8) else {
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
                return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "Error: source and destination are identical"], isComplete: false)
            }

            // Step 2: Create diff with metadata
            let algorithm = CodingService.selectDiffAlgorithm(source: source, destination: destination)
            let diff = MultiLineDiff.createDiff(source: source, destination: destination, algorithm: algorithm, includeMetadata: true, sourceStartLine: startLine.map { $0 - 1 })
            let diffId = DiffStore.shared.store(diff: diff, source: source)

            // Step 3: Apply diff
            do {
                let patched = try MultiLineDiff.applyDiff(to: source, diff: diff)

                // Truncation guard: only fires when result is both absolutely tiny
                // and tiny relative to source — see Self.looksTruncated.
                if Self.looksTruncated(source: source, patched: patched) {
                    let err = "Error: diff rejected — would shrink section from \(source.count) to \(patched.count) chars. The result is suspiciously small (likely truncated mid-stream). Re-send the destination text in full or narrow the line range."
                    tab.appendLog(err); tab.flush()
                    return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err], isComplete: false)
                }

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
                return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "Applied diff to \(filePath)\(rangeNote) [verified: \(verified)] — file now has \(newLineCount) lines. Re-read the file before making more edits. diff_id: \(diffId.uuidString)\n\n\(display)"], isComplete: false)
            } catch {
                let err = "Error applying diff: \(error.localizedDescription)"
                tab.appendLog(err); tab.flush()
                return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err], isComplete: false)
            }

        case "list_files":
            let pattern = input["pattern"] as? String ?? "*.swift"
            if pattern == "*" || pattern == "*.*" {
                let err = "Error: pattern '*' is too broad. Use a file extension like '*.swift', '*.json', '*.py'. Example: list_files(pattern: \"*.swift\")"
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
                ? "No matches for '\(pattern)'" : "[project folder: \(displaySearch)] paths are relative to project folder\n\(result.output)"
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
            let resolved = rawPath.hasPrefix("/") || rawPath.hasPrefix("~") ? rawPath : (tabFolder as NSString).appendingPathComponent(rawPath)
            tab.appendLog("📁 mkdir -p \(CodingService.trimHome(resolved))")
            tab.flush()
            let result = await executeForTab(command: "mkdir -p \(CodingService.shellEscape(resolved)) && echo 'Created: \(resolved)'", projectFolder: tabFolder)
            let out = result.output.trimmingCharacters(in: .whitespacesAndNewlines)
            let output = out.isEmpty ? "Error creating directory" : out
            tab.appendLog(output); tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        default:
        let output = await executeNativeTool(name, input: input)
        tab.appendLog(output); tab.flush()
        return tabResult(output, toolId: toolId)
        }
    }
}
