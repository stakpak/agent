import Foundation
import AgentD1F

/// Pure file operations for coding tools — no shell, no Process, no escaping issues.
/// Process-based tools (list, search, git) route through UserService XPC instead.
enum CodingService {

    // MARK: - Adaptive Algorithm Selection

    /// Choose the best diff algorithm based on change size.
    static func selectDiffAlgorithm(source: String, destination: String) -> DiffAlgorithm {
        let sourceLines = source.components(separatedBy: "\n").count
        let destLines = destination.components(separatedBy: "\n").count
        let maxLines = max(sourceLines, destLines)
        let charDiff = abs(source.count - destination.count)
        if maxLines < 50 || charDiff < 2000 {
            return .flash
        }
        return .megatron
    }

    // MARK: - Read File

      /// Walk up from `originalPath` to find nearest existing ancestor directory, then
      /// search up to 4 levels deep for matching basenames. Capped at `maxResults`.
    static func findFilesByBasename(originalPath: String, maxResults: Int) -> [String] {
        let basename = (originalPath as NSString).lastPathComponent
        guard !basename.isEmpty else { return [] }
        let fm = FileManager.default

        // Walk up to find the nearest existing ancestor directory
        var search = (originalPath as NSString).deletingLastPathComponent
        var isDir: ObjCBool = false
        while !search.isEmpty, search != "/" {
            if fm.fileExists(atPath: search, isDirectory: &isDir), isDir.boolValue {
                break
            }
            search = (search as NSString).deletingLastPathComponent
        }
        guard !search.isEmpty, fm.fileExists(atPath: search, isDirectory: &isDir), isDir.boolValue else {
            return []
        }

        // BFS for matching basenames, max depth 4, skip dotfiles, build/.git/.swiftpm/DerivedData
        var results: [String] = []
        let skipDirs: Set<String> = [".git", ".build", ".swiftpm", "DerivedData", "node_modules", ".Trash", "Pods"]
        var queue: [(path: String, depth: Int)] = [(search, 0)]
        while let (current, depth) = queue.first {
            queue.removeFirst()
            if results.count >= maxResults { break }
            guard depth <= 4 else { continue }
            guard let entries = try? fm.contentsOfDirectory(atPath: current) else { continue }
            for entry in entries {
                if entry.hasPrefix(".") { continue }
                if skipDirs.contains(entry) { continue }
                let full = (current as NSString).appendingPathComponent(entry)
                var entryIsDir: ObjCBool = false
                if fm.fileExists(atPath: full, isDirectory: &entryIsDir) {
                    if entryIsDir.boolValue {
                        queue.append((full, depth + 1))
                    } else if entry == basename {
                        results.append(full)
                        if results.count >= maxResults { break }
                    }
                }
            }
        }
        return results
    }

      /// Read file contents with line numbers. Parameters: path (absolute), offset
      /// (1-based start line, default 1), limit (max lines, default 2000).
    static func readFile(path: String, offset: Int?, limit: Int?) -> String {
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)

        guard FileManager.default.fileExists(atPath: url.path) else {
            // Model guessed wrong path — find files with same basename in nearby dirs
            // and hand the model the actual path inline so it can fix in one shot.
            let candidates = findFilesByBasename(originalPath: path, maxResults: 5)
            let dir = (path as NSString).deletingLastPathComponent
            let suggestPath = dir.isEmpty ? "." : dir
            if !candidates.isEmpty {
                let list = candidates.map { "  \($0)" }.joined(separator: "\n")
                return """
                    Error: file not found: \(path)
                    Found \(candidates.count) file(s) with the same name in nearby directories:
                    \(list)
                    Recovery: re-call read_file with the correct path from the list above.
                    """
            }
            return """
                Error: file not found: \(path)
                No files matching the basename were found in the parent directory or its subfolders.
                Recovery: call file_manager(action:"list", path:"\(suggestPath)") to see what \
                exists, or file_manager(action:"search", pattern:"\((path as NSString).lastPathComponent)") \
                to search the project tree.
                """
        }

        // Check if it's a directory
        var isDir: ObjCBool = false
        FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir)
        if isDir.boolValue {
            return "Error: path is a directory, not a file: \(path)"
        }

        guard let data = FileManager.default.contents(atPath: url.path),
              let content = String(data: data, encoding: .utf8) else
        {
            return "Error: could not read file (binary or encoding issue): \(path)"
        }

        let lines = content.components(separatedBy: "\n")
        let startLine = max((offset ?? 1) - 1, 0)
        let maxLines = limit ?? 2000

        guard startLine < lines.count else {
            return "Error: offset \(startLine + 1) exceeds file length (\(lines.count) lines)"
        }

        let endLine = min(startLine + maxLines, lines.count)
        let slice = lines[startLine..<endLine]
        let lineNumWidth = String(endLine).count

        var result = ""
        for (i, line) in slice.enumerated() {
            let num = String(startLine + i + 1).padding(toLength: lineNumWidth, withPad: " ", startingAt: 0)
            result += "\(num)\t\(line)\n"
        }

        if endLine < lines.count {
            result += "... (\(lines.count - endLine) more lines)"
        }

        return result
    }

    // MARK: - Write File

    /// Create or overwrite a file.
    static func writeFile(path: String, content: String) -> String {
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)

        // Create parent directories if needed
        let parent = url.deletingLastPathComponent()
        do {
            try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
        } catch {
            return "Error: could not create directory \(parent.path): \(error.localizedDescription)"
        }

        do {
            try content.write(to: url, atomically: true, encoding: .utf8)
            let lines = content.components(separatedBy: "\n").count
            return "Wrote \(lines) lines to \(url.path)"
        } catch {
            return "Error: \(error.localizedDescription)"
        }
    }

    // MARK: - Edit File (d1f-powered string replacement)

    /// / Replace exact text in a file with d1f-verified diff/apply pipeline: / read → locate match (exact → fuzzy →
    /// context) → compute replacement → / build d1f diff → verify round-trip → write → return .ai preview.
    static func editFile(path: String, oldString: String, newString: String, replaceAll: Bool, context: String? = nil) -> String {
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)

        guard FileManager.default.fileExists(atPath: url.path) else {
            return "Error: file not found: \(path)"
        }
        guard let data = FileManager.default.contents(atPath: url.path),
              let raw = String(data: data, encoding: .utf8) else
        {
            return "Error: could not read file: \(path)"
        }

        // 1. Normalize line endings on both file and inputs so a CRLF file
        //    accepts an LF old_string (and vice versa) without spurious mismatches.
        let original = raw.replacingOccurrences(of: "\r\n", with: "\n")
        let needle = oldString.replacingOccurrences(of: "\r\n", with: "\n")
        let replacement = newString.replacingOccurrences(of: "\r\n", with: "\n")

        guard needle != replacement else {
            return "Error: old_string and new_string are identical"
        }
        guard !needle.isEmpty else {
            return "Error: old_string is empty — refusing to edit (would match every position)"
        }

        // 2. Locate the match (exact → fuzzy → context-disambiguated)
        let occurrences = original.components(separatedBy: needle).count - 1
        var matchRange: Range<String.Index>?
        var matchNote = ""

        if occurrences == 0 {
            if let range = fuzzyFindRange(in: original, target: needle) {
                matchRange = range
                matchNote = " (fuzzy whitespace match)"
            } else {
                // Build a recovery message that INCLUDES the current file content around where the model probably tried
                // to edit. The model now has fresh content in the same response and can self-correct without an extra read round-trip.
                let trimmed = needle.trimmingCharacters(in: .whitespacesAndNewlines)
                let firstLine = needle.components(separatedBy: "\n")
                    .first(where: { !$0.trimmingCharacters(in: .whitespaces).isEmpty })?
                    .trimmingCharacters(in: .whitespaces)

                let contextSnippet = Self.findEditFailureContext(
                    in: original,
                    firstLine: firstLine,
                    trimmedNeedle: trimmed
                )

                if !trimmed.isEmpty && original.contains(trimmed) {
                    return """
                        Error: old_string not found (exact whitespace match) in \(path). \
                        A similar string exists — your old_string has the wrong indentation, \
                        leading/trailing spaces, or tab-vs-space differences. \
                        Current content at the suspected location:
                        \(contextSnippet)
                        Recovery: copy the exact bytes (including whitespace) from the snippet \
                        above into your next edit_file call. The read cache for this file has \
                        been invalidated so any subsequent read_file will fetch fresh.
                        """
                }
                if let firstLine, !firstLine.isEmpty, original.contains(firstLine) {
                    return """
                        Error: old_string not found in \(path) — the first line was found but the \
                        full block doesn't match. The file likely changed since your last read. \
                        Current content around the matching line:
                        \(contextSnippet)
                        Recovery: use the snippet above as the source of truth. The read cache for \
                        this file has been invalidated; any subsequent read_file will fetch fresh.
                        """
                }
                return """
                    Error: old_string not found in \(path). The file may have changed entirely. \
                    Current file head:
                    \(contextSnippet)
                    Recovery: the read cache for this file has been invalidated. Call \
                    read_file(file_path:"\(path)") to get the full current content, then retry \
                    with the exact bytes you want to replace.
                    """
            }
        } else if occurrences > 1 && !replaceAll {
            if let context = context, !context.isEmpty,
               let range = findOccurrenceByContext(in: original, target: needle, context: context)
            {
                matchRange = range
                matchNote = " (disambiguated by context)"
            } else {
                return
                    "Error: old_string appears \(occurrences) times "
                    + "in \(path). Provide more context to make it "
                    + "unique, or set replace_all=true."
            }
        } else if !replaceAll {
            matchRange = original.range(of: needle)
        }

        // 3. Compute the proposed updated content
        let updated: String
        if replaceAll {
            updated = original.replacingOccurrences(of: needle, with: replacement)
        } else if let range = matchRange {
            updated = original.replacingCharacters(in: range, with: replacement)
        } else {
            return "Error: internal — no match range computed for non-replaceAll edit"
        }

        // 3a. No-op detection: if the substring replacement produced identical content, the edit is a no-op. This
        // catches the case where the LLM thinks it's fixing something but old_string and new_string are equivalent after fuzzy matching, OR the matched range already contains the target text. Don't touch disk and tell the LLM clearly.
        if updated == original {
            return
                "Warning: edit is a no-op — applying old_string→new_string "
                + "produced identical content in \(url.path). The file was NOT "
                + "modified. Either the matched text already equals new_string, "
                + "or old_string and new_string are equivalent after whitespace "
                + "normalization. Re-read the file and verify what actually "
                + "needs to change."
        }

        // 4. Build a structured d1f diff with metadata (line numbers, totals)
        let algorithm = selectDiffAlgorithm(source: original, destination: updated)
        let diff = MultiLineDiff.createDiff(
            source: original,
            destination: updated,
            algorithm: algorithm,
            includeMetadata: true
        )

        // 5. Round-trip the diff through applyDiff to confirm it produces exactly what we computed via direct substring
        // replacement. Catches any diff-library edge cases before we touch disk.
        let applied: String
        do {
            applied = try MultiLineDiff.applyDiff(to: original, diff: diff)
        } catch {
            return "Error: d1f apply failed: \(error.localizedDescription). Edit aborted to prevent corruption."
        }
        guard applied == updated else {
            return "Error: d1f round-trip mismatch — diff would not reproduce the intended edit. Edit aborted to prevent corruption."
        }

        // 6. Verify the diff itself
        let verified = MultiLineDiff.verifyDiff(diff)

        // 7. Write to disk
        do {
            try applied.write(to: url, atomically: true, encoding: .utf8)
        } catch {
            return "Error: \(error.localizedDescription)"
        }

        // 8. Return d1f's .ai-format preview + label + line-number metadata
        let preview = MultiLineDiff.displayDiff(diff: diff, source: original, format: .ai)
        let label: String
        if replaceAll {
            label = "\(occurrences) occurrence(s)"
        } else {
            label = "1 occurrence" + matchNote
        }
        var result = "Replaced \(label) in \(url.path) [verified: \(verified)]\n\n\(preview)"
        if let meta = diff.metadata {
            if let startLine = meta.sourceStartLine { result += "\n📍 Changes start at line \(startLine + 1)" }
            if let total = meta.sourceTotalLines { result += " of \(total) lines" }
        }
        return result
    }

    /// / Fuzzy line-by-line match with multiple normalization passes. / Pass 1: tabs→spaces + strip trailing
    /// whitespace. / Pass 2: trim all leading/trailing whitespace per line (catches indentation mismatches). / Also strips leading/trailing blank lines from target before matching. / Edit failure recovery: anchors on firstLine of old_string, falls back to / trimmed match, falls back to file head. Returns ~10 lines max.
    static func findEditFailureContext(in content: String, firstLine: String?, trimmedNeedle: String) -> String {
        let lines = content.components(separatedBy: "\n")
        let contextLines = 10
        var anchorIdx: Int?

        if let firstLine, !firstLine.isEmpty {
            anchorIdx = lines.firstIndex(where: { $0.contains(firstLine) })
        }
        if anchorIdx == nil, !trimmedNeedle.isEmpty {
            // Try the first non-empty line of the trimmed needle
            let needleFirst = trimmedNeedle.components(separatedBy: "\n")
                .first(where: { !$0.trimmingCharacters(in: .whitespaces).isEmpty })?
                .trimmingCharacters(in: .whitespaces) ?? ""
            if !needleFirst.isEmpty {
                anchorIdx = lines.firstIndex(where: { $0.contains(needleFirst) })
            }
        }

        let startLine: Int
        let endLine: Int
        if let anchor = anchorIdx {
            startLine = max(0, anchor - 3)
            endLine = min(lines.count, anchor + contextLines)
        } else {
            // No anchor — fall back to file head
            startLine = 0
            endLine = min(lines.count, contextLines + 5)
        }

        let snippet = (startLine..<endLine).map { i in
            String(format: "%4d  %@", i + 1, lines[i])
        }.joined(separator: "\n")
        return "```\n\(snippet)\n```"
    }

    private static func fuzzyFindRange(in content: String, target: String) -> Range<String.Index>? {
        let contentLines = content.components(separatedBy: "\n")
        var targetLines = target.components(separatedBy: "\n")

        // Strip leading/trailing blank lines from target
        while let first = targetLines.first, first.trimmingCharacters(in: .whitespaces).isEmpty {
            targetLines.removeFirst()
        }
        while let last = targetLines.last, last.trimmingCharacters(in: .whitespaces).isEmpty {
            targetLines.removeLast()
        }

        guard !targetLines.isEmpty, targetLines.count <= contentLines.count else { return nil }

        // Pass 1: normalize tabs + trailing whitespace only
        let normalizeLight: (String) -> String = { line in
            line.replacingOccurrences(of: "\t", with: "    ")
                .replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression)
        }
        // Pass 2: trim all whitespace (catches indentation differences)
        let normalizeStrong: (String) -> String = { line in
            line.trimmingCharacters(in: .whitespaces)
        }

        for normalize in [normalizeLight, normalizeStrong] {
            let targetNorm = targetLines.map(normalize)

            for start in 0...(contentLines.count - targetLines.count) {
                let window = contentLines[start..<(start + targetLines.count)]
                if window.enumerated().allSatisfy({ normalize($0.element) == targetNorm[$0.offset] }) {
                    let beforeCount = contentLines[..<start].reduce(0) { $0 + $1.count + 1 } // +1 for \n
                    let matchStr = contentLines[start..<(start + targetLines.count)].joined(separator: "\n")
                    let startIdx = content.index(content.startIndex, offsetBy: beforeCount)
                    let endIdx = content.index(startIdx, offsetBy: matchStr.count)
                    return startIdx..<endIdx
                }
            }
        }
        return nil
    }

    /// Find the occurrence of `target` in `content` closest to the given `context` text.
    private static func findOccurrenceByContext(in content: String, target: String, context: String) -> Range<String.Index>? {
        var ranges: [Range<String.Index>] = []
        var searchStart = content.startIndex
        while let range = content.range(of: target, range: searchStart..<content.endIndex) {
            ranges.append(range)
            searchStart = range.upperBound
        }
        guard ranges.count > 1 else { return ranges.first }

        let contextLines = context.trimmingCharacters(in: .whitespacesAndNewlines)
            .components(separatedBy: "\n")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
        guard !contextLines.isEmpty else { return nil }

        var bestRange: Range<String.Index>?
        var bestScore = 0
        for range in ranges {
            let windowStart = content.index(
                range.lowerBound,
                offsetBy: -min(500, content.distance(from: content.startIndex, to: range.lowerBound)),
                limitedBy: content.startIndex
            ) ?? content.startIndex
            let windowEnd = content.index(
                range.upperBound,
                offsetBy: min(500, content.distance(from: range.upperBound, to: content.endIndex)),
                limitedBy: content.endIndex
            ) ?? content.endIndex
            let window = String(content[windowStart..<windowEnd])
            var score = 0
            for line in contextLines where window.contains(line) { score += 1 }
            if score > bestScore { bestScore = score; bestRange = range }
        }
        return bestScore > 0 ? bestRange : nil
    }

    // MARK: - Undo Edit

    /// Restore a file to its original content (before last edit).
    static func undoEdit(path: String, originalContent: String) -> String {
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        do {
            try originalContent.write(to: url, atomically: true, encoding: .utf8)
            let lines = originalContent.components(separatedBy: "\n").count
            return "Undo successful: restored \(url.path) (\(lines) lines)"
        } catch {
            return "Error undoing edit: \(error.localizedDescription)"
        }
    }

    // MARK: - Diff + Apply (single call)

    /// Create a diff and apply it to a file in one call.
    static func diffAndApply(
        path: String,
        source: String?,
        destination: String,
        startLine: Int? = nil,
        endLine: Int? = nil
    ) -> (output: String, display: String)
    {
        // Reject diff-formatted input (❌/✅ markers, +/- prefixes) — LLMs must send raw text only
        let badMarkers = ["❌ ", "✅ "]
        for marker in badMarkers {
            if destination.contains(marker) {
                return ("Error: destination contains '\(marker)' — diff_apply requires raw text only, not diff format. Remove ❌/✅ markers and +/- prefixes. Send the exact final text that should appear in the file.", "")
            }
            if let s = source, s.contains(marker) {
                return ("Error: source contains '\(marker)' — diff_apply requires raw text only, not diff format. Remove ❌/✅ markers and +/- prefixes. Send the exact original text as-is from the file.", "")
            }
        }
        // Reject unified-diff style +/- prefixed lines (common LLM mistake)
        if destination.components(separatedBy: "\n").filter({ $0.hasPrefix("+ ") || $0.hasPrefix("- ") }).count > 2 {
            return ("Error: destination looks like a unified diff — diff_apply requires raw text. Remove +/- line prefixes. Send only the final text that should replace the source lines.", "")
        }

        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)

        // Read the full file
        guard let data = FileManager.default.contents(atPath: url.path),
              let fullText = String(data: data, encoding: .utf8) else
        {
            return ("Error: cannot read \(path)", "")
        }

        let actualSource: String
        let finalContent: String

        if let sl = startLine, let el = endLine {
            // Line-range mode: LLM only sends the changed section
            let allLines = fullText.components(separatedBy: "\n")
            let s = max(sl - 1, 0)
            let e = min(el, allLines.count)
            actualSource = allLines[s..<e].joined(separator: "\n")

            guard actualSource != destination else {
                return ("Error: source and destination are identical", "")
            }

            // Splice destination back into the full file
            var newLines = allLines
            newLines.replaceSubrange(s..<e, with: destination.components(separatedBy: "\n"))
            finalContent = newLines.joined(separator: "\n")
        } else if let source = source, !source.isEmpty {
            actualSource = source
            guard actualSource != destination else {
                return ("Error: source and destination are identical", "")
            }
            finalContent = destination
        } else {
            actualSource = fullText
            guard actualSource != destination else {
                return ("Error: source and destination are identical", "")
            }
            finalContent = destination
        }

        let algorithm = selectDiffAlgorithm(source: actualSource, destination: destination)
        let diff = MultiLineDiff.createDiff(
            source: actualSource,
            destination: destination,
            algorithm: algorithm,
            includeMetadata: true,
            sourceStartLine: startLine.map { $0 - 1 }
        )
        let display = MultiLineDiff.displayDiff(diff: diff, source: actualSource, format: .ai)
        let verified = MultiLineDiff.verifyDiff(diff)
        do {
            try finalContent.write(to: url, atomically: true, encoding: .utf8)
            let rangeNote = (startLine != nil && endLine != nil) ? " (lines \(startLine!)-\(endLine!))" : ""
            return ("Applied diff to \(url.path)\(rangeNote), algorithm: \(algorithm.displayName), verified: \(verified)", display)
        } catch {
            return ("Error: \(error.localizedDescription)", "")
        }
    }

}
