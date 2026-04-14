@preconcurrency import Foundation
import AppKit
import AgentMCP

// MARK: - Miscellaneous Helpers (shell, formatting, path utilities, scripts)

extension AgentViewModel {

    /// Generate a short name for auto-saving an AppleScript from its source.
    /// Uses the first meaningful words from the script, capped at 40 chars.
    static func autoScriptName(from source: String) -> String {
        let clean = source
            .replacingOccurrences(of: "tell application", with: "")
            .replacingOccurrences(of: "display dialog", with: "dialog")
            .replacingOccurrences(of: "\"", with: "")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let words = clean.components(separatedBy: .whitespacesAndNewlines)
            .filter { !$0.isEmpty }
            .prefix(4)
            .joined(separator: "_")
        let name = words.prefix(40)
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: ".", with: "_")
        return name.isEmpty ? "untitled_\(Int(Date().timeIntervalSince1970))" : String(name)
    }

    /// Brief one-line summary of a tool call for batch_tools progress display.
    static func briefToolSummary(_ name: String, input: [String: Any]) -> String {
        // Pick the most informative parameter to show
        if let path = input["file_path"] as? String {
            return (path as NSString).lastPathComponent
        }
        if let cmd = input["command"] as? String {
            let trimmed = cmd.trimmingCharacters(in: .whitespaces)
            return trimmed.count > 60 ? String(trimmed.prefix(57)) + "..." : trimmed
        }
        if let pattern = input["pattern"] as? String {
            if let path = input["path"] as? String {
                return "\(pattern), \((path as NSString).lastPathComponent)"
            }
            return pattern
        }
        if let path = input["path"] as? String {
            return (path as NSString).lastPathComponent
        }
        if let scriptName = input["name"] as? String {
            return scriptName
        }
        if let action = input["action"] as? String {
            return action
        }
        // Fallback: show first string value
        for (_, value) in input {
            if let s = value as? String, !s.isEmpty {
                return s.count > 40 ? String(s.prefix(37)) + "..." : s
            }
        }
        return ""
    }

    /// Show first N lines of output, then "..." if there's more.
    static func preview(_ text: String, lines count: Int) -> String {
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
        if lines.count <= count { return text.trimmingCharacters(in: .newlines) }
        return lines.prefix(count).joined(separator: "\n") + "\n..."
    }

    /// Prepend line numbers to D1F diff output. ❌ lines track source, ✅ lines track destination.
    static func numberDiffLines(_ d1f: String, startLine: Int) -> String {
        var srcLine = startLine
        var dstLine = startLine
        return d1f.components(separatedBy: "\n").map { line in
            if line.hasPrefix("❌") {
                let n = srcLine; srcLine += 1
                return "\(n) \(line)"
            } else if line.hasPrefix("✅") {
                let n = dstLine; dstLine += 1
                return "\(n) \(line)"
            } else if line.hasPrefix("📎") {
                let n = srcLine; srcLine += 1; dstLine += 1
                return "\(n) \(line)"
            }
            return line
        }.joined(separator: "\n")
    }

    static func codeFence(_ text: String, language: String = "") -> String {
        "```\(language)\n\(text.trimmingCharacters(in: .newlines))\n```"
    }

    /// Guess language from file extension for syntax highlighting.
    static func langFromPath(_ path: String) -> String {
        let ext = (path as NSString).pathExtension.lowercased()
        switch ext {
        case "swift": return "swift"
        case "py": return "python"
        case "js", "jsx": return "javascript"
        case "ts", "tsx": return "typescript"
        case "rb": return "ruby"
        case "go": return "go"
        case "rs": return "rust"
        case "c", "h": return "c"
        case "cpp", "cc", "cxx", "hpp": return "cpp"
        case "m", "mm": return "objc"
        case "java": return "java"
        case "kt": return "kotlin"
        case "json": return "json"
        case "yaml", "yml": return "yaml"
        case "sql": return "sql"
        case "sh", "bash", "zsh": return "bash"
        case "html", "htm": return "html"
        case "css": return "css"
        case "xml", "plist": return "xml"
        default: return ""
        }
    }

    /// Validate that a path exists. Returns an error string if invalid, nil if OK.
    static func checkPath(_ path: String?) -> String? {
        guard let path, !path.isEmpty else { return nil }
        let expanded = (path as NSString).expandingTildeInPath
        guard FileManager.default.fileExists(atPath: expanded) else {
            return "Error: path does not exist: \(path) — check for typos"
        }
        return nil
    }

    /// / Extract user-directory paths from a shell command for preflight validation. / Catches typos like
    /// "/Users/foo/Documets/..." before running the command. / Resolve project folder to a directory (strip filename if path points to a file).
    static func resolvedWorkingDirectory(_ path: String) -> String {
        guard !path.isEmpty else { return "" }
        var isDir: ObjCBool = false
        if FileManager.default.fileExists(atPath: path, isDirectory: &isDir) {
            return isDir.boolValue ? path : (path as NSString).deletingLastPathComponent
        }
        // Path doesn't exist yet — treat as directory
        return path
    }

    /// Prepend `cd <projectFolder> &&` so the shell runs in the right directory.
    /// Skips if folder is empty or command already starts with `cd `.
    static func prependWorkingDirectory(_ command: String, projectFolder: String) -> String {
        guard !projectFolder.isEmpty, !command.hasPrefix("cd ") else { return command }
        let escaped = "'" + projectFolder.replacingOccurrences(of: "'", with: "'\\''") + "'"
        return "cd \(escaped) && \(command)"
    }

    /// Extract the target directory from a command starting with `cd `.
    /// Resolves relative paths against the current project folder.
    static func extractCdTarget(_ command: String, relativeTo base: String) -> String? {
        guard command.hasPrefix("cd ") else { return nil }
        let afterCd = String(command.dropFirst(3)).trimmingCharacters(in: .whitespaces)
        guard !afterCd.isEmpty else { return nil }
        // Extract path before any && or ;
        let path: String
        if let r = afterCd.range(of: "&&") {
            path = String(afterCd[..<r.lowerBound]).trimmingCharacters(in: .whitespaces)
        } else if let r = afterCd.range(of: ";") {
            path = String(afterCd[..<r.lowerBound]).trimmingCharacters(in: .whitespaces)
        } else {
            path = afterCd
        }
        // Strip surrounding quotes
        var cleaned = path
        if (cleaned.hasPrefix("'") && cleaned.hasSuffix("'")) ||
            (cleaned.hasPrefix("\"") && cleaned.hasSuffix("\""))
        {
            cleaned = String(cleaned.dropFirst().dropLast())
        }
        guard !cleaned.isEmpty else { return nil }
        // Expand ~
        if cleaned.hasPrefix("~/") || cleaned == "~" {
            cleaned = (cleaned as NSString).expandingTildeInPath
        }
        // Resolve relative paths against current project folder
        if !cleaned.hasPrefix("/") {
            let baseDir = resolvedWorkingDirectory(base)
            if !baseDir.isEmpty {
                cleaned = (baseDir as NSString).appendingPathComponent(cleaned)
            }
        }
        // Standardize (resolve .., .)
        cleaned = (cleaned as NSString).standardizingPath
        return cleaned
    }

    /// Intercept shell commands that should use built-in tools instead
    static func suggestTool(_ command: String) -> String? {
        // Let all commands run through the Launch Agent without blocking
        return nil
    }

    static func preflightCommand(_ command: String) -> String? {
        // Match paths under /Users/ or ~/ — most common source of typos
        guard let regex = try? NSRegularExpression(
            pattern: #"(?:^|\s)(/Users/[^\s'";&|><$]+|~/[^\s'";&|><$]+)"#
        ) else { return nil }
        let nsCmd = command as NSString
        let matches = regex.matches(in: command, range: NSRange(location: 0, length: nsCmd.length))
        for match in matches {
            var path = nsCmd.substring(with: match.range(at: 1))
                .trimmingCharacters(in: CharacterSet(charactersIn: "'\""))
            // Skip paths with glob characters — shell will expand them
            if path.contains("*") || path.contains("?") || path.contains("[") { continue }
            // Strip trailing slash
            while path.hasSuffix("/") { path = String(path.dropLast()) }
            guard !path.isEmpty else { continue }
            let expanded = (path as NSString).expandingTildeInPath
            if !FileManager.default.fileExists(atPath: expanded) {
                return "Error: path does not exist: \(path) — check for typos in the path"
            }
        }
        return nil
    }

    /// Count files at a path.
    static func countFilesAtPath(_ path: String, hasWildcard: Bool) -> Int {
        let fm: FileManager = FileManager.default
        var isDir: ObjCBool = false

        if hasWildcard {
            let parent: String = (path as NSString).deletingLastPathComponent
            guard fm.fileExists(atPath: parent, isDirectory: &isDir), isDir.boolValue else { return 0 }
            let contents: [String]? = try? fm.contentsOfDirectory(atPath: parent)
            return contents?.count ?? 0
        }

        if fm.fileExists(atPath: path, isDirectory: &isDir) {
            if isDir.boolValue {
                guard let enumerator = fm.enumerator(atPath: path) else { return 0 }
                var count: Int = 0
                while enumerator.nextObject() != nil {
                    count += 1
                    if count > 10_000 { return count }
                }
                return count
            }
            return 1
        }
        return 0
    }

    // MARK: - Combine Agent Scripts

    /// Merge two Swift script sources: deduplicate imports, handle duplicate scriptMain
    /// by keeping A's entry point and renaming B's body into a helper function.
    static func combineScriptSources(contentA: String, contentB: String, sourceA: String, sourceB: String) -> String {
        let linesA = contentA.components(separatedBy: "\n")
        let linesB = contentB.components(separatedBy: "\n")

        var imports = [String]()
        var seenImports = Set<String>()
        var bodyA = [String]()
        var bodyB = [String]()

        for line in linesA {
            let t = line.trimmingCharacters(in: .whitespaces)
            if t.hasPrefix("import ") {
                if seenImports.insert(t).inserted { imports.append(line) }
            } else {
                bodyA.append(line)
            }
        }
        for line in linesB {
            let t = line.trimmingCharacters(in: .whitespaces)
            if t.hasPrefix("import ") {
                if seenImports.insert(t).inserted { imports.append(line) }
            } else {
                bodyB.append(line)
            }
        }

        // Trim leading blank lines
        let trimmedA = Array(bodyA.drop(while: { $0.trimmingCharacters(in: .whitespaces).isEmpty }))
        var trimmedB = Array(bodyB.drop(while: { $0.trimmingCharacters(in: .whitespaces).isEmpty }))

        // Detect duplicate scriptMain in B — remove @_cdecl and rename to helper
        let hasMainA = trimmedA.contains(where: { $0.contains("func scriptMain") || $0.contains("func script_main") })
        let hasMainB = trimmedB.contains(where: { $0.contains("func scriptMain") || $0.contains("func script_main") })

        if hasMainA && hasMainB {
            // Remove @_cdecl line and rename scriptMain in B
            trimmedB = trimmedB.filter { !$0.contains("@_cdecl(\"script_main\")") }
            trimmedB = trimmedB.map { line in
                line.replacingOccurrences(of: "public func scriptMain()", with: "public func scriptMain_\(sourceB)()")
                    .replacingOccurrences(of: "public func script_main()", with: "public func scriptMain_\(sourceB)()")
            }
        }

        return imports.joined(separator: "\n")
            + "\n\n// MARK: - From \(sourceA)\n\n"
            + trimmedA.joined(separator: "\n")
            + "\n\n// MARK: - From \(sourceB)\n\n"
            + trimmedB.joined(separator: "\n")
    }

    // MARK: - Xcode Project Detection

    /// Check if the project folder contains an Xcode project.
    static func isXcodeProject(_ folder: String) -> Bool {
        guard !folder.isEmpty else { return false }
        let fm = FileManager.default
        if let contents = try? fm.contentsOfDirectory(atPath: folder) {
            return contents.contains(where: { $0.hasSuffix(".xcodeproj") || $0.hasSuffix(".xcworkspace") })
        }
        return false
    }
}
