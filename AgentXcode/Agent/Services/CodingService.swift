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

    /// Read file contents with line numbers (like `cat -n`).
    /// - Parameters:
    ///   - path: Absolute file path
    ///   - offset: 1-based line to start from (default 1)
    ///   - limit: Max lines to return (default 2000)
    static func readFile(path: String, offset: Int?, limit: Int?) -> String {
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)

        guard FileManager.default.fileExists(atPath: url.path) else {
            let dir = (path as NSString).deletingLastPathComponent
            let suggestPath = dir.isEmpty ? "." : dir
            return "Error: file not found: \(path)\nSTOP guessing paths. Call file_manager(action:\"list\", path:\"\(suggestPath)\") to see what files exist."
        }

        // Check if it's a directory
        var isDir: ObjCBool = false
        FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir)
        if isDir.boolValue {
            return "Error: path is a directory, not a file: \(path)"
        }

        guard let data = FileManager.default.contents(atPath: url.path),
              let content = String(data: data, encoding: .utf8) else {
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

    // MARK: - Edit File (exact string replacement)

    /// Replace exact text in a file. The old_string must be unique unless replace_all is true.
    /// If `context` is provided and old_string has multiple matches, uses context to disambiguate.
    static func editFile(path: String, oldString: String, newString: String, replaceAll: Bool, context: String? = nil) -> String {
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)

        guard FileManager.default.fileExists(atPath: url.path) else {
            return "Error: file not found: \(path)"
        }

        guard let data = FileManager.default.contents(atPath: url.path),
              var content = String(data: data, encoding: .utf8) else {
            return "Error: could not read file: \(path)"
        }

        // Normalize line endings
        content = content.replacingOccurrences(of: "\r\n", with: "\n")
        let oldString = oldString.replacingOccurrences(of: "\r\n", with: "\n")
        let newString = newString.replacingOccurrences(of: "\r\n", with: "\n")

        guard oldString != newString else {
            return "Error: old_string and new_string are identical"
        }

        let occurrences = content.components(separatedBy: oldString).count - 1

        if occurrences == 0 {
            // Fuzzy fallback: match with whitespace-trimmed lines
            if !replaceAll, let range = fuzzyFindRange(in: content, target: oldString) {
                content.replaceSubrange(range, with: newString)
                do {
                    try content.write(to: url, atomically: true, encoding: .utf8)
                    return "Replaced 1 occurrence in \(url.path) (fuzzy whitespace match)"
                } catch {
                    return "Error: \(error.localizedDescription)"
                }
            }

            // Try to give a helpful hint
            let trimmed = oldString.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty && content.contains(trimmed) {
                return "Error: old_string not found (exact match). A similar string exists — check whitespace/indentation. Re-read the file to verify content."
            }
            // Check if first non-blank line exists anywhere in the file
            let firstLine = oldString.components(separatedBy: "\n")
                .first(where: { !$0.trimmingCharacters(in: .whitespaces).isEmpty })?
                .trimmingCharacters(in: .whitespaces)
            if let firstLine, !firstLine.isEmpty, content.contains(firstLine) {
                return "Error: old_string not found in \(path). Content may have changed — re-read the file before retrying."
            }
            return "Error: old_string not found in \(path). Re-read the file to verify the exact content."
        }

        if !replaceAll && occurrences > 1 {
            // Try context-based disambiguation
            if let context = context, !context.isEmpty,
               let range = findOccurrenceByContext(in: content, target: oldString, context: context) {
                content.replaceSubrange(range, with: newString)
                do {
                    try content.write(to: url, atomically: true, encoding: .utf8)
                    return "Replaced 1 of \(occurrences) occurrences in \(url.path) (disambiguated by context)"
                } catch {
                    return "Error: \(error.localizedDescription)"
                }
            }
            return "Error: old_string appears \(occurrences) times. Provide more context to make it unique, or set replace_all=true."
        }

        if replaceAll {
            content = content.replacingOccurrences(of: oldString, with: newString)
        } else {
            // Replace first occurrence only
            if let range = content.range(of: oldString) {
                content.replaceSubrange(range, with: newString)
            }
        }

        do {
            try content.write(to: url, atomically: true, encoding: .utf8)
            let label = replaceAll ? "\(occurrences) occurrence(s)" : "1 occurrence"
            return "Replaced \(label) in \(url.path)"
        } catch {
            return "Error: \(error.localizedDescription)"
        }
    }

    /// Fuzzy line-by-line match with multiple normalization passes.
    /// Pass 1: tabs→spaces + strip trailing whitespace.
    /// Pass 2: trim all leading/trailing whitespace per line (catches indentation mismatches).
    /// Also strips leading/trailing blank lines from target before matching.
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
                    let beforeCount = contentLines[..<start].reduce(0) { $0 + $1.count + 1 }  // +1 for \n
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
            let windowStart = content.index(range.lowerBound, offsetBy: -min(500, content.distance(from: content.startIndex, to: range.lowerBound)), limitedBy: content.startIndex) ?? content.startIndex
            let windowEnd = content.index(range.upperBound, offsetBy: min(500, content.distance(from: range.upperBound, to: content.endIndex)), limitedBy: content.endIndex) ?? content.endIndex
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
    static func diffAndApply(path: String, source: String?, destination: String, startLine: Int? = nil, endLine: Int? = nil) -> (output: String, display: String) {
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)

        // Read the full file
        guard let data = FileManager.default.contents(atPath: url.path),
              let fullText = String(data: data, encoding: .utf8) else {
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
        let diff = MultiLineDiff.createDiff(source: actualSource, destination: destination, algorithm: algorithm, includeMetadata: true, sourceStartLine: startLine.map { $0 - 1 })
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
