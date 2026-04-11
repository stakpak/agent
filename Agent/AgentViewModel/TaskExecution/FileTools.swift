@preconcurrency import Foundation
import AgentD1F


// MARK: - File I/O Tool Execution
extension AgentViewModel {

    /// Handles file I/O tool calls.
    /// Returns nil if this is not a file tool call.
    @MainActor
    func handleFileTool(
        name: String,
        input: [String: Any],
        toolId: String,
        appendLog: @escaping @Sendable (String) -> Void,
        appendRawOutput: @escaping @Sendable (String) -> Void,
        toolResults: inout [[String: Any]]
    ) async -> Bool {
        // Resolve relative file_path against project folder
        var input = input
        if let fp = input["file_path"] as? String, !fp.isEmpty, !fp.hasPrefix("/"), !fp.hasPrefix("~") {
            let base = projectFolder.isEmpty ? NSHomeDirectory() : projectFolder
            input["file_path"] = (base as NSString).appendingPathComponent(fp)
        }

        // MARK: read_file
        switch name {

        case "read_file":
            let filePath = input["file_path"] as? String ?? ""
            if filePath.isEmpty {
                let err = "Error: file_path is required. Use file_manager(action:\"list\", path:\".\") to find files first."
                appendLog("📖 Read: (empty path)")
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
                return true
            }
            let offset = input["offset"] as? Int
            let limit = input["limit"] as? Int
            let expandedRead = (filePath as NSString).expandingTildeInPath
            let cacheKey = Self.fileReadCacheKey(path: expandedRead, offset: offset, limit: limit)

            // Mtime-based dedup: if we've read this exact range and the file hasn't changed,
            // return a stub WITHOUT doing disk I/O. The earlier tool_result is still in context.
            if let cached = Self.taskFileReadCache[cacheKey],
               let attrs = try? FileManager.default.attributesOfItem(atPath: expandedRead),
               let currentMtime = attrs[.modificationDate] as? Date,
               cached.mtime == currentMtime
            {
                let stub =
                        "File unchanged since last read (\(cached.outputCharCount) chars). "
                        + "The content from the earlier Read tool_result in this "
                        + "conversation is still current — refer to that instead "
                        + "of re-reading."
                appendLog("📖 (unchanged) \(filePath)")
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": stub])
                return true
            }

            appendLog("📖 Read: \(filePath)")
            let output = await Self.offMain { CodingService.readFile(path: filePath, offset: offset, limit: limit) }
            // If file not found, suggest listing files first
            if output.hasPrefix("Error:") && output.contains("not found") {
                let dir = (filePath as NSString).deletingLastPathComponent
                let suggestPath = dir.isEmpty ? "." : dir
                let suggestion = output + "\nUse file_manager(action:\"list\", path:\"\(suggestPath)\") to see available files."
                appendLog(suggestion)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": suggestion])
                return true
            }

            // Store mtime + size in cache so the next read of the same range can short-circuit
            if let attrs = try? FileManager.default.attributesOfItem(atPath: expandedRead),
               let mtime = attrs[.modificationDate] as? Date
            {
                Self.taskFileReadCache[cacheKey] = FileReadCacheEntry(mtime: mtime, outputCharCount: output.count)
            }

            // Cap file output at 50K chars for LLM context. 50K covers ~95% of Swift source files in one read — eliminates the chunked re-read storm where the LLM repeatedly calls read_file
            // with offset/limit just to see the whole file. Each chunked read is a different cache key, so the dedup cache can't help; raising the cap is what actually reduces redundant reads.
            let capped = LogLimits.trim(
                output,
                cap: LogLimits.readFileChars,
                lineCount: output.components(separatedBy: "\n").count,
                suffix: "Use offset/limit to read specific sections."
            )
            let lang = Self.langFromPath(filePath)
            appendLog(Self.codeFence(Self.preview(output, lines: readFilePreviewLines), language: lang))
            toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": capped])
            return true

        // MARK: write_file
        case "write_file":
            let filePath = input["file_path"] as? String ?? ""
            let content = input["content"] as? String ?? ""
            let expandedWrite = (filePath as NSString).expandingTildeInPath
            let beforeContent = try? String(contentsOfFile: expandedWrite, encoding: .utf8)
            FileBackupService.shared.backup(filePath: expandedWrite, tabID: selectedTabId ?? Self.mainTabID)
            appendLog("📝 Write: \(filePath)")
            let output = await Self.offMain { CodingService.writeFile(path: filePath, content: content) }
            FileChangeJournal.shared.log(
                action: "write",
                filePath: expandedWrite,
                beforeContent: beforeContent,
                afterContent: content,
                tool: "write_file"
            )
            Self.invalidateFileReadCache(path: expandedWrite)
            appendLog(output)
            let lang = Self.langFromPath(filePath)
            appendLog(Self.codeFence(Self.preview(content, lines: readFilePreviewLines), language: lang))
            commandsRun.append("write_file: \(filePath)")
            let writeDiag = await Self.postEditDiagnostic(
                filePath: (filePath as NSString).expandingTildeInPath,
                projectFolder: projectFolder
            )
            let writeResult = writeDiag.isEmpty ? output : output + "\n\n⚠️ Diagnostics:\n" + writeDiag
            toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": writeResult])
            return true

        // MARK: edit_file — uses CodingService for replacement logic, D1F for preview
        case "edit_file":
            let filePath = input["file_path"] as? String ?? ""
            let oldString = input["old_string"] as? String ?? ""
            let newString = input["new_string"] as? String ?? ""
            let replaceAll = input["replace_all"] as? Bool ?? false
            let context = input["context"] as? String
            FileBackupService.shared.backup(filePath: (filePath as NSString).expandingTildeInPath, tabID: selectedTabId ?? Self.mainTabID)
            appendLog("📝 Edit: \(filePath)")
            let expandedEdit = (filePath as NSString).expandingTildeInPath

            // Single read from disk
            guard let data = FileManager.default.contents(atPath: expandedEdit),
                  let originalContent = String(data: data, encoding: .utf8) else
            {
                let err = "Error: cannot read \(filePath)"
                appendLog(err)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
                return true
            }

            // Use CodingService for the replacement (handles fuzzy match, context, etc.)
            let output = await Self.offMain { CodingService.editFile(
                path: filePath,
                oldString: oldString,
                newString: newString,
                replaceAll: replaceAll,
                context: context
            ) }

            Self.invalidateFileReadCache(path: expandedEdit)
            if !output.hasPrefix("Error") {
                DiffStore.shared.recordEdit(filePath: expandedEdit, originalContent: originalContent)
                let diff = MultiLineDiff.createDiff(source: oldString, destination: newString, includeMetadata: true)
                var d1f = MultiLineDiff.displayDiff(diff: diff, source: oldString, format: .ai)
                if let meta = diff.metadata, let start = meta.sourceStartLine {
                    d1f += "\n📍 line \(start + 1)"
                    if let total = meta.sourceTotalLines { d1f += " of \(total)" }
                }
                if !d1f.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    appendLog(d1f)
                }
            }
            let outLines = output.components(separatedBy: "\n")
            let status = outLines.first ?? output
            let lineInfo = outLines.last(where: { $0.hasPrefix("📍") }) ?? ""
            appendLog(lineInfo.isEmpty ? status : "\(status) \(lineInfo)")
            // Log edit to journal
            let afterEdit = try? String(contentsOfFile: expandedEdit, encoding: .utf8)
            FileChangeJournal.shared.log(
                action: "edit",
                filePath: expandedEdit,
                beforeContent: originalContent,
                afterContent: afterEdit,
                tool: "edit_file"
            )
            commandsRun.append("edit_file: \(filePath)")
            // Post-edit diagnostic: quick syntax check for Swift files in Xcode projects
            let editDiag = await Self.postEditDiagnostic(filePath: expandedEdit, projectFolder: projectFolder)
            let editResult = editDiag.isEmpty ? output : output + "\n\n⚠️ Diagnostics:\n" + editDiag
            toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": editResult])
            return true

        // MARK: create_diff — reads file from disk, requires line range
        case "create_diff":
            let filePath = input["file_path"] as? String ?? ""
            let destination = input["destination"] as? String ?? ""
            guard let startLine = input["start_line"] as? Int,
                  let endLine = input["end_line"] as? Int else
            {
                let err =
                    "Error: start_line and end_line are required. "
                        + "Use read_file first to find the line numbers, "
                        + "then specify the range to edit."
                appendLog(err)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
                return true
            }

            let expanded = (filePath as NSString).expandingTildeInPath
            guard let data = FileManager.default.contents(atPath: expanded),
                  let fullText = String(data: data, encoding: .utf8) else
            {
                let err = "Error: cannot read \(filePath)"
                appendLog(err)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
                return true
            }

            let lines = fullText.components(separatedBy: "\n")
            let s = max(startLine - 1, 0)
            let e = min(endLine, lines.count)
            let source = lines[s..<e].joined(separator: "\n")

            let algorithm = CodingService.selectDiffAlgorithm(source: source, destination: destination)
            let diff = MultiLineDiff.createDiff(
                source: source,
                destination: destination,
                algorithm: algorithm,
                includeMetadata: true,
                sourceStartLine: startLine - 1
            )
            let d1f = MultiLineDiff.displayDiff(diff: diff, source: source, format: .ai)
            let diffId = DiffStore.shared.store(diff: diff, source: source)
            resetStreamCounters()
            appendLog(d1f)
            appendLog("📝 Created diff for \(filePath) (lines \(startLine)-\(endLine))")
            commandsRun.append("create_diff: \(filePath)")
            toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": "diff_id: \(diffId.uuidString)\n\n\(d1f)"])
            return true

        // MARK: apply_diff — reads file from disk, applies stored diff
        case "apply_diff":
            let filePath = input["file_path"] as? String ?? ""
            let diffIdStr = input["diff_id"] as? String ?? ""
            let asciiDiff = input["diff"] as? String ?? ""
            let expandedPath = (filePath as NSString).expandingTildeInPath
            guard let data = FileManager.default.contents(atPath: expandedPath),
                  let source = String(data: data, encoding: .utf8) else
            {
                let err = "Error: cannot read \(filePath)"
                appendLog(err)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
                return true
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
                // No truncation guard. d1f's structural verification + the applyDiff round-trip already catch malformed diffs. Legitimate refactors that delete most of a section were getting
                // blocked, and undo_edit is always available if the LLM produces something bad — we'd rather trust the model and give the user undo than second-guess every shrink with a length heuristic.
                try patched.write(to: URL(fileURLWithPath: expandedPath), atomically: true, encoding: .utf8)
                // Track the apply for UUID-based undo
                if let uuid = UUID(uuidString: diffIdStr) {
                    DiffStore.shared.recordApply(diffId: uuid, filePath: expandedPath, originalContent: source)
                } else {
                    DiffStore.shared.recordEdit(filePath: expandedPath, originalContent: source)
                }
                // Use the library's verification
                let verifyDiff = MultiLineDiff.createDiff(source: source, destination: patched, includeMetadata: true)
                let verified = MultiLineDiff.verifyDiff(verifyDiff)
                let display = MultiLineDiff.displayDiff(diff: verifyDiff, source: source, format: .ai)
                appendLog(display)
                let newLineCount = patched.components(separatedBy: "\n").count
                appendLog("📝 Applied diff to \(filePath) [verified: \(verified)] (\(newLineCount) lines)")
                // Invalidate all pending diffs for this file — line numbers have shifted
                DiffStore.shared.invalidateDiffs(for: expandedPath)
                commandsRun.append("apply_diff: \(filePath)")
                toolResults.append([
                    "type": "tool_result",
                    "tool_use_id": toolId,
                    "content":
                        "Applied diff to \(filePath) [verified: \(verified)] "
                        + "— file now has \(newLineCount) lines. "
                        + "Any pending diffs for this file are invalidated. "
                        + "Re-read the file before making more edits."
                        + "\n\n\(display)"
                ])
            } catch {
                let err = "Error applying diff: \(error.localizedDescription)"
                appendLog(err)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
            }
            return true

        // MARK: undo_edit — uses diff_id UUID or falls back to file path
        case "undo_edit":
            let filePath = input["file_path"] as? String ?? ""
            let diffIdStr = input["diff_id"] as? String
            let expandedUndo = (filePath as NSString).expandingTildeInPath

            // Try UUID-based undo first (uses D1F library's createUndoDiff)
            if let idStr = diffIdStr, let uuid = UUID(uuidString: idStr),
               let stored = DiffStore.shared.retrieve(uuid)
            {
                // Use D1F's built-in undo: create reverse diff from metadata
                if let undoDiff = MultiLineDiff.createUndoDiff(from: stored.diff) {
                    let fallbackPath: String? = DiffStore.shared.lastAppliedDiffId(for: expandedUndo).flatMap { id in
                        DiffStore.shared.retrieve(id).map { _ in expandedUndo }
                    }
                    let currentPath = filePath.isEmpty
                        ? (fallbackPath ?? expandedUndo)
                        : expandedUndo
                    guard let data = FileManager.default.contents(atPath: currentPath),
                          let current = String(data: data, encoding: .utf8) else
                    {
                        let err = "Error: cannot read \(filePath)"
                        appendLog(err)
                        toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
                        return true
                    }
                    do {
                        let restored = try MultiLineDiff.applyDiff(to: current, diff: undoDiff)
                        try restored.write(to: URL(fileURLWithPath: currentPath), atomically: true, encoding: .utf8)
                        DiffStore.shared.popLastApplied(for: currentPath)
                        let display = MultiLineDiff.displayDiff(diff: undoDiff, source: current, format: .ai)
                        appendLog(display)
                        appendLog("↩️ Undo applied (diff_id: \(idStr))")
                        commandsRun.append("undo_edit: \(filePath)")
                        toolResults.append([
                            "type": "tool_result",
                            "tool_use_id": toolId,
                            "content": "Undo applied for diff_id \(idStr)\n\n\(display)"
                        ])
                        return true
                    } catch {
                        appendLog("D1F undo failed: \(error.localizedDescription), falling back to edit history")
                    }
                }
            }

            // Fallback: file-path-based undo from edit history
            guard let original = DiffStore.shared.lastEdit(for: expandedUndo) else {
                let err = "Error: no edit history for \(filePath)"
                appendLog(err)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
                return true
            }
            appendLog("↩️ Undo: \(filePath)")
            let output = await Self.offMain { CodingService.undoEdit(path: filePath, originalContent: original) }
            if !output.hasPrefix("Error") { DiffStore.shared.clearEditHistory(for: expandedUndo) }
            appendLog(output)
            commandsRun.append("undo_edit: \(filePath)")
            toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": output])
            return true

        // MARK: diff_and_apply — same as create_diff + apply_diff in one call, no shortcuts
        case "diff_and_apply":
            let filePath = input["file_path"] as? String ?? ""
            let destination = input["destination"] as? String ?? ""
            let startLine = input["start_line"] as? Int
            let endLine = input["end_line"] as? Int
            let rangeNote: String
            if let s = startLine, let e = endLine {
                rangeNote = " (lines \(s)-\(e))"
            } else {
                rangeNote = ""
            }

            let expanded = (filePath as NSString).expandingTildeInPath
            guard let data = FileManager.default.contents(atPath: expanded),
                  let fullText = String(data: data, encoding: .utf8) else
            {
                let err = "Error: cannot read \(filePath)"
                appendLog(err)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
                return true
            }

            // Step 1: Extract source section (same as create_diff)
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
                let err = "Error: source and destination are identical"
                appendLog(err)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
                return true
            }

            // Step 2: Create diff with full metadata (same as create_diff)
            let algorithm = CodingService.selectDiffAlgorithm(source: source, destination: destination)
            let diff = MultiLineDiff.createDiff(
                source: source,
                destination: destination,
                algorithm: algorithm,
                includeMetadata: true,
                sourceStartLine: startLine.map { $0 - 1 }
            )
            let diffId = DiffStore.shared.store(diff: diff, source: source)

            // Step 3: Apply diff (same as apply_diff). No truncation guard — d1f's structural
            // verification + applyDiff already catch malformed diffs, and undo_edit is always available for recovery.
            do {
                let patched = try MultiLineDiff.applyDiff(to: source, diff: diff)

                // Splice back into full file if line range was used
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

                // Record for UUID-based undo
                DiffStore.shared.recordApply(diffId: diffId, filePath: expanded, originalContent: fullText)

                // Verify (same as apply_diff)
                let verifyDiff = MultiLineDiff.createDiff(source: source, destination: patched, includeMetadata: true)
                let verified = MultiLineDiff.verifyDiff(verifyDiff)
                let display = MultiLineDiff.displayDiff(diff: verifyDiff, source: source, format: .ai)
                let newLineCount = finalContent.components(separatedBy: "\n").count
                appendLog(display)
                appendLog("📝 Diff+Apply: \(filePath)\(rangeNote) [verified: \(verified)] (\(newLineCount) lines)")
                // Invalidate all pending diffs for this file — line numbers have shifted
                DiffStore.shared.invalidateDiffs(for: expanded)
                commandsRun.append("diff_and_apply: \(filePath)")
                toolResults.append([
                    "type": "tool_result",
                    "tool_use_id": toolId,
                    "content":
                        "Applied diff to \(filePath)\(rangeNote) "
                        + "[verified: \(verified)] — file now has "
                        + "\(newLineCount) lines. Re-read the file before "
                        + "making more edits. diff_id: \(diffId.uuidString)"
                        + "\n\n\(display)"
                ])
            } catch {
                let err = "Error applying diff: \(error.localizedDescription)"
                appendLog(err)
                toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": err])
            }
            return true

        default:
            return false
        }
    }

    // MARK: - Post-Edit Diagnostics

    /// Quick syntax check after editing a Swift file. Returns error lines or empty string.
    /// Only runs for .swift files in Xcode project folders.
    nonisolated static func postEditDiagnostic(filePath: String, projectFolder: String) async -> String {
        // Only check Swift files in Xcode projects
        guard filePath.hasSuffix(".swift") else { return "" }
        // Quick check for .xcodeproj without MainActor
        let hasXcodeProj = (try? FileManager.default.contentsOfDirectory(atPath: projectFolder))?
            .contains(where: { $0.hasSuffix(".xcodeproj") || $0.hasSuffix(".xcworkspace") }) ?? false
        guard hasXcodeProj else { return "" }

        return await withCheckedContinuation { continuation in
            DispatchQueue.global().async {
                let process = Process()
                process.executableURL = URL(fileURLWithPath: "/usr/bin/xcrun")
                process.arguments = ["swiftc", "-parse", filePath]
                process.currentDirectoryURL = URL(fileURLWithPath: projectFolder)
                let pipe = Pipe()
                process.standardOutput = pipe
                process.standardError = pipe
                var env = ProcessInfo.processInfo.environment
                env["HOME"] = NSHomeDirectory()
                process.environment = env
                do {
                    try process.run()
                    process.waitUntilExit()
                    let data = pipe.fileHandleForReading.readDataToEndOfFile()
                    let output = String(data: data, encoding: .utf8) ?? ""
                    if process.terminationStatus != 0 && !output.isEmpty {
                        // Return first 5 error lines to avoid bloating context
                        let errors = output.components(separatedBy: "\n")
                            .filter { $0.contains("error:") }
                            .prefix(5)
                            .joined(separator: "\n")
                        continuation.resume(returning: errors)
                    } else {
                        continuation.resume(returning: "")
                    }
                } catch {
                    continuation.resume(returning: "")
                }
            }
        }
    }
}
