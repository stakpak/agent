
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import AgentAccess
import Cocoa

// MARK: - Native Tool Handler — File Operations

extension AgentViewModel {

    /// / Handles file CRUD, diff, list/search, backup/restore, symbol search, / and refactor_rename tool calls. Returns
    /// `nil` if the name is not a / file-group tool so the main dispatcher can fall through.
    func handleFileNativeTool(name: String, input: [String: Any]) async -> String? {
        let pf = projectFolder
        switch name {
        // File operations
        case "read_file":
            let path = input["file_path"] as? String ?? ""
            guard !path.isEmpty else {
                return """
                    Error: file_path is required for read_file. \
                    Pass an absolute path like file_path:"/Users/...". \
                    Use file_manager(action:"list", path:...) to see \
                    what files exist if you don't know the path.
                    """
            }
            // Delegate to CodingService.readFile which returns line-numbered output and gives a clear 'file not found'
            // error with a list-files suggestion when the path is wrong. Honors offset+limit (1-based offset).
            let offset = input["offset"] as? Int
            let limit = input["limit"] as? Int
            return await Self.offMain {
                CodingService.readFile(path: path, offset: offset, limit: limit)
            }
        case "write_file":
            let path = input["file_path"] as? String ?? ""
            let content = input["content"] as? String ?? ""
            // Back up before overwriting
            let tabID = selectedTabId ?? Self.mainTabID
            FileBackupService.shared.backup(filePath: path, tabID: tabID)
            let url = URL(fileURLWithPath: path)
            try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            do { try content.write(to: url, atomically: true, encoding: .utf8); return "Wrote \(path)" }
            catch { return "Error: \(error.localizedDescription)" }
        // MARK: edit_file — delegate to CodingService.editFile (d1f-powered with line-ending normalization, fuzzy
        // whitespace match, context disambiguation, and round-trip verification). The duplicate edit_file logic that lived here had none of those safeguards and was the source of most "old_string not found" errors when the LLM had a slightly-stale snapshot of the file.
        case "edit_file":
            let path = input["file_path"] as? String ?? ""
            guard !path.isEmpty else { return "Error: file_path is required for edit_file" }
            let old = input["old_string"] as? String ?? ""
            let new = input["new_string"] as? String ?? ""
            let replaceAll = input["replace_all"] as? Bool ?? false
            let context = input["context"] as? String
            // Back up before editing so undo_edit can restore
            let tabID = selectedTabId ?? Self.mainTabID
            FileBackupService.shared.backup(filePath: path, tabID: tabID)
            return await Self.offMain {
                CodingService.editFile(path: path, oldString: old, newString: new, replaceAll: replaceAll, context: context)
            }
        // MARK: create_diff
        case "create_diff":
            var source = input["source"] as? String ?? ""
            let destination = input["destination"] as? String ?? ""
            if let fp = input["file_path"] as? String, !fp.isEmpty {
                let expanded = (fp as NSString).expandingTildeInPath
                if let data = FileManager.default.contents(atPath: expanded),
                   let text = String(data: data, encoding: .utf8) {
                    source = text
                }
            }
            let diff = MultiLineDiff.createDiff(source: source, destination: destination, includeMetadata: true)
            let d1f = MultiLineDiff.displayDiff(diff: diff, source: source, format: .ai)
            let diffId = DiffStore.shared.store(diff: diff, source: source)
            return "diff_id: \(diffId.uuidString)\n\n\(d1f)"
        // MARK: apply_diff
        case "apply_diff":
            let path = input["file_path"] as? String ?? ""
            let diffIdStr = input["diff_id"] as? String ?? ""
            let asciiDiff = input["diff"] as? String ?? ""
            let expanded = (path as NSString).expandingTildeInPath
            guard let data = FileManager.default.contents(atPath: expanded),
                  let source = String(data: data, encoding: .utf8) else { return "Error: cannot read \(path)" }
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
                try patched.write(to: URL(fileURLWithPath: expanded), atomically: true, encoding: .utf8)
                let verifyDiff = MultiLineDiff.createAndDisplayDiff(source: source, destination: patched, format: .ai)
                return "Applied diff to \(path)\n\n\(verifyDiff)"
            } catch {
                return "Error applying diff: \(error.localizedDescription)"
            }
        // List/search files (via User LaunchAgent - no TCC required)
        case "list_files":
            let rawPat = input["pattern"] as? String ?? "*.swift"
            // Reject wildcard-only patterns — too broad, suggest specific extension
            if rawPat == "*" || rawPat == "*.*" {
                return """
                    Error: pattern '*' is too broad. \
                    Use a file extension like '*.swift', '*.json', \
                    '*.py', or '*.txt'. \
                    Example: list_files(pattern: "*.swift")
                    """
            }
            let pat = CodingService.shellEscape(rawPat)
            let rawDir = input["path"] as? String ?? pf
            let displayDir = CodingService.trimHome(rawDir)
            let findCmd =
                "find . -maxdepth 8 \\( -type f -o -type d \\) -name \(pat)"
                + " ! -path '*/.*' ! -path '*/.build/*'"
                + " ! -path '*/.git/*' ! -path '*/.swiftpm/*'"
                + " ! -name '.DS_Store' ! -name '*.xcuserstate'"
                + " 2>/dev/null | sed 's|^\\./||' | sort | head -100"
            let result = await executeViaUserAgent(
                command: findCmd,
                workingDirectory: rawDir, silent: true)
            let raw = result.output.trimmingCharacters(in: .whitespacesAndNewlines)
            if raw.isEmpty {
                return "No files found"
            }
            return """
                [project folder: \(displayDir)] \
                paths are relative to project folder
                \(CodingService.formatFileTree(raw))
                """
        case "search_files":
            let pat = CodingService.shellEscape(input["pattern"] as? String ?? "")
            let rawDir = input["path"] as? String ?? pf
            let displayDir = CodingService.trimHome(rawDir)
            let escapedDir = CodingService.shellEscape(rawDir)
            let result = await executeViaUserAgent(command: "grep -rn \(pat) \(escapedDir) 2>/dev/null | head -50")
            if result.output.isEmpty {
                return "No matches"
            }
            return """
                [project folder: \(displayDir)] \
                paths are relative to project folder
                \(result.output)
                """
        case "read_dir":
            let rawDir = input["path"] as? String ?? pf
            let displayDir = CodingService.trimHome(rawDir)
            let detail = (input["detail"] as? String ?? "slim") == "more"
            let cmd = detail
                ? "ls -la . 2>/dev/null"
                : "find . -maxdepth 1 -not -name '.*' 2>/dev/null | sed 's|^\\./||' | sort"
            let result = await executeViaUserAgent(command: cmd, workingDirectory: rawDir, silent: !detail)
            let raw = result.output.trimmingCharacters(in: .whitespacesAndNewlines)
            return raw.isEmpty ? "Directory not found or empty" : "[project folder: \(displayDir)]\n\(raw)"
        case "mkdir":
            let rawPath = input["path"] as? String ?? ""
            guard !rawPath.isEmpty else { return "Error: path is required" }
            let stripped = rawPath.hasPrefix("./") ? String(rawPath.dropFirst(2)) : rawPath
            let resolved = stripped.hasPrefix("/") || stripped.hasPrefix("~")
                ? (stripped as NSString).expandingTildeInPath
                : (pf as NSString).appendingPathComponent(stripped)
            let escaped = CodingService.shellEscape(resolved)
            let result = await executeViaUserAgent(command: "mkdir -p \(escaped) && echo 'Created: \(resolved)'")
            let out = result.output.trimmingCharacters(in: .whitespacesAndNewlines)
            if out.hasPrefix("Created:") {
                projectFolder = resolved
                return "\(out)\nProject folder set to: \(resolved)"
            }
            return out.isEmpty ? "Error creating directory" : out
        case "if_to_switch":
            let filePath = input["file_path"] as? String ?? ""
            return await Self.offMain { CodingService.convertIfToSwitch(path: filePath) }
        case "extract_function":
            let filePath = input["file_path"] as? String ?? ""
            let funcName = input["function_name"] as? String ?? ""
            let newFile = input["new_file"] as? String ?? ""
            return await Self.offMain {
                CodingService.extractFunctionToFile(
                    sourcePath: filePath,
                    functionName: funcName,
                    newFileName: newFile)
            }
        case "symbol_search":
            let query = input["query"] as? String ?? ""
            let path = input["path"] as? String ?? pf
            let exact = input["exact"] as? Bool ?? false
            guard !query.isEmpty else { return "Error: query is required" }
            let results = SymbolSearchService.search(query: query, in: path, exactMatch: exact)
            if results.isEmpty { return "No symbols found matching '\(query)'" }
            return results.prefix(50).map { r in
                "\(r.kind) \(r.name) — \(r.filePath):\(r.line)\n  \(r.signature)"
            }.joined(separator: "\n")
        // AST-based multi-file rename using Swift-Syntax
        case "refactor_rename":
            let oldName = input["old_name"] as? String ?? ""
            let newName = input["new_name"] as? String ?? ""
            let path = input["path"] as? String ?? pf
            guard !oldName.isEmpty && !newName.isEmpty else { return "Error: old_name and new_name required." }
            // Find all occurrences using symbol search
            let occurrences = SymbolSearchService.search(query: oldName, in: path, exactMatch: true)
            if occurrences.isEmpty { return "No symbols found matching '\(oldName)'" }
            // Perform rename across all files
            var renamedFiles: Set<String> = []
            var errors: [String] = []
            for occ in occurrences {
                let filePath = occ.filePath
                guard let data = FileManager.default.contents(atPath: filePath),
                      var content = String(data: data, encoding: .utf8) else { continue }
                let before = content
                // Word-boundary replacement to avoid partial matches
                let pattern = "\\b\(NSRegularExpression.escapedPattern(for: oldName))\\b"
                if let regex = try? NSRegularExpression(pattern: pattern) {
                    content = regex.stringByReplacingMatches(
                        in: content,
                        range: NSRange(content.startIndex..., in: content),
                        withTemplate: newName)
                }
                if content != before {
                    FileBackupService.shared.backup(filePath: filePath, tabID: selectedTabId ?? Self.mainTabID)
                    do {
                        try content.write(toFile: filePath, atomically: true, encoding: .utf8)
                        renamedFiles.insert((filePath as NSString).lastPathComponent)
                    } catch {
                        errors.append("\(filePath): \(error.localizedDescription)")
                    }
                }
            }
            if renamedFiles.isEmpty && errors.isEmpty { return "No changes needed — '\(oldName)' not found in source files." }
            var result = "Renamed '\(oldName)' → '\(newName)' in \(renamedFiles.count) file(s):\n"
            result += renamedFiles.sorted().joined(separator: "\n")
            if !errors.isEmpty { result += "\n\nErrors:\n" + errors.joined(separator: "\n") }
            return result
        // undo_edit
        case "undo_edit":
            let fp = input["file_path"] as? String ?? ""
            let expanded = (fp as NSString).expandingTildeInPath
            guard let original = DiffStore.shared.lastEdit(for: expanded) else {
                return "Error: no edit history for \(fp). Recovery: call file(action:\"restore\", file_path:\"\(fp)\") to recover the most recent FileBackupService snapshot, or git checkout if the file is in a repo."
            }
            let result = CodingService.undoEdit(path: fp, originalContent: original)
            if !result.hasPrefix("Error") { DiffStore.shared.clearEditHistory(for: expanded) }
            return result
        // restore_file — recover the most recent FileBackupService snapshot for a file
        case "restore_file":
            let fp = input["file_path"] as? String ?? ""
            let backupName = input["backup"] as? String
            let expanded = (fp as NSString).expandingTildeInPath
            let fileName = (expanded as NSString).lastPathComponent
            let tabID = selectedTabId ?? Self.mainTabID
            let backups = FileBackupService.shared.listBackups(tabID: tabID)
                .filter { $0.original == fileName }
            if let explicit = backupName {
                guard let match = backups.first(where: { ($0.backup as NSString).lastPathComponent == explicit }) else {
                    return "Error: backup '\(explicit)' not found for \(fileName). Recovery: call file(action:\"list_backups\", file_path:\"\(fp)\") to see available backups."
                }
                if FileBackupService.shared.restore(backupPath: match.backup, to: expanded) {
                    Self.invalidateFileReadCache(path: expanded)
                    return "Restored \(fileName) from \(explicit)."
                }
                return "Error: failed to restore from \(explicit). Recovery: try a different backup via file(action:\"list_backups\", file_path:\"\(fp)\")."
            }
            guard let latest = backups.first else {
                return "Error: no backups found for \(fileName). Recovery: file backups are tab-scoped and 1-week TTL — try undo_edit if the change was very recent, or git checkout if the file is in a repo."
            }
            if FileBackupService.shared.restore(backupPath: latest.backup, to: expanded) {
                Self.invalidateFileReadCache(path: expanded)
                return "Restored \(fileName) from latest backup (\(latest.date))."
            }
            return "Error: failed to restore latest backup of \(fileName). Recovery: call file(action:\"list_backups\", file_path:\"\(fp)\") to see other backups, or use undo_edit if recent."
        // list_file_backups — show what's in the FileBackupService TTL store for this tab
        case "list_file_backups":
            let fp = input["file_path"] as? String ?? ""
            let expanded = (fp as NSString).expandingTildeInPath
            let fileName = (expanded as NSString).lastPathComponent
            let tabID = selectedTabId ?? Self.mainTabID
            let backups = FileBackupService.shared.listBackups(tabID: tabID)
                .filter { fileName.isEmpty || $0.original == fileName }
            if backups.isEmpty {
                return fileName.isEmpty
                    ? "No file backups in this tab."
                    : "No backups found for \(fileName)."
            }
            return backups.map { "\(($0.backup as NSString).lastPathComponent)  (\($0.date))" }.joined(separator: "\n")
        // diff_and_apply
        case "diff_and_apply":
            let fp = input["file_path"] as? String ?? ""
            // Back up before diff_and_apply
            FileBackupService.shared.backup(filePath: fp, tabID: selectedTabId ?? Self.mainTabID)
            let dest = input["destination"] as? String ?? ""
            let source = input["source"] as? String
            let startLine = input["start_line"] as? Int
            let endLine = input["end_line"] as? Int
            let result = CodingService.diffAndApply(path: fp, source: source, destination: dest, startLine: startLine, endLine: endLine)
            return result.output
        default:
            return nil
        }
    }
}
