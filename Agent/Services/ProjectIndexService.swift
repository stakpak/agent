import Foundation
import CryptoKit

/// Writes and reads a portable JSONL project index at
/// `{projectFolder}/.agent/index/index.jsonl`. One JSON object per line per
/// file, so any LLM can consume the file directly via `read_file`.
///
/// Record shape:
/// ```json
/// {"path":"Agent/App.swift","size":1234,"mtime":"2026-04-15T12:34:56Z","language":"swift","sha256":"..."}
/// ```
enum ProjectIndexService {

    // MARK: - Paths

    static let indexFileName = "index.jsonl"

    static func indexDir(in projectFolder: String) -> URL {
        AgentProjectPaths.url(in: projectFolder, .index)
    }

    static func indexFile(in projectFolder: String) -> URL {
        indexDir(in: projectFolder).appendingPathComponent(indexFileName)
    }

    // MARK: - Config

    /// Common code/text extensions included by default. Override via the
    /// `extensions` argument on the tool call.
    static let defaultExtensions: Set<String> = [
        "swift", "m", "mm", "h", "c", "cpp", "cc", "hpp",
        "js", "ts", "tsx", "jsx", "mjs", "cjs",
        "py", "rb", "go", "rs", "java", "kt", "scala",
        "sh", "bash", "zsh", "fish",
        "json", "yaml", "yml", "toml", "xml", "plist", "entitlements", "pbxproj",
        "md", "txt", "rst", "html", "css", "scss", "sass",
        "applescript", "scpt", "scptd",
    ]

    /// Directories we always skip.
    static let skipDirs: Set<String> = [
        ".git", ".build", ".swiftpm", "DerivedData", "build", "node_modules",
        "Pods", "Carthage", AgentProjectPaths.rootDirName, "xcuserdata",
        ".venv", "venv", "__pycache__", ".next", ".nuxt", "dist", "out",
    ]

    static let defaultMaxFileSize: Int = 1_000_000

    // MARK: - Record

    struct Record: Codable {
        let path: String
        let size: Int
        let lines: Int
        let mtime: String
        let language: String
        let sha256: String
        let doc: String?
        let symbols: [String]
    }

    /// Max characters of the leading doc comment stored per file.
    static let docCharCap = 200
    /// Max number of top-level symbols stored per file.
    static let symbolCountCap = 100

    // MARK: - Public API

    /// Returns the existing index as JSONL string (empty if missing).
    static func read(projectFolder: String, offset: Int = 1, limit: Int = 500) throws -> String {
        let url = indexFile(in: projectFolder)
        guard FileManager.default.fileExists(atPath: url.path) else { return "" }
        let content = try String(contentsOf: url, encoding: .utf8)
        let lines = content.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        let start = max(0, offset - 1)
        guard start < lines.count else { return "" }
        let end = min(lines.count, start + max(1, limit))
        return lines[start..<end].joined(separator: "\n")
    }

    /// Delete the index file (and its directory if empty).
    static func remove(projectFolder: String) throws {
        let file = indexFile(in: projectFolder)
        let dir = indexDir(in: projectFolder)
        let fm = FileManager.default
        if fm.fileExists(atPath: file.path) {
            try fm.removeItem(at: file)
        }
        if let contents = try? fm.contentsOfDirectory(atPath: dir.path), contents.isEmpty {
            try? fm.removeItem(at: dir)
        }
    }

    /// Write a fresh index. If one already exists and `overwrite` is false, throws.
    @discardableResult
    static func create(
        projectFolder: String,
        extensions: Set<String>? = nil,
        maxFileSize: Int = defaultMaxFileSize,
        overwrite: Bool = false
    ) throws -> (fileCount: Int, bytes: Int) {
        let fm = FileManager.default
        let dir = indexDir(in: projectFolder)
        let file = indexFile(in: projectFolder)
        if fm.fileExists(atPath: file.path) && !overwrite {
            throw NSError(domain: "ProjectIndex", code: 1, userInfo: [
                NSLocalizedDescriptionKey: "Index already exists at \(file.path). Use action=recreate or action=append."
            ])
        }
        try fm.createDirectory(at: dir, withIntermediateDirectories: true)
        if fm.fileExists(atPath: file.path) {
            try fm.removeItem(at: file)
        }
        fm.createFile(atPath: file.path, contents: nil)
        let exts = extensions ?? defaultExtensions
        let handle = try FileHandle(forWritingTo: file)
        defer { try? handle.close() }
        var count = 0
        for record in walkRecords(projectFolder: projectFolder, extensions: exts, maxFileSize: maxFileSize) {
            let line = try jsonLine(for: record)
            handle.write(Data(line.utf8))
            count += 1
        }
        let attrs = try? fm.attributesOfItem(atPath: file.path)
        let bytes = (attrs?[.size] as? Int) ?? 0
        return (count, bytes)
    }

    /// Append new/changed files to an existing index. Creates the index if
    /// missing. Returns counts of new + updated records.
    @discardableResult
    static func append(
        projectFolder: String,
        extensions: Set<String>? = nil,
        maxFileSize: Int = defaultMaxFileSize
    ) throws -> (added: Int, updated: Int, total: Int) {
        let fm = FileManager.default
        let file = indexFile(in: projectFolder)
        if !fm.fileExists(atPath: file.path) {
            let result = try create(projectFolder: projectFolder, extensions: extensions, maxFileSize: maxFileSize)
            return (result.fileCount, 0, result.fileCount)
        }
        // Build map of existing records keyed by path
        let existing = try readAllRecords(projectFolder: projectFolder)
        var byPath: [String: Record] = [:]
        for r in existing { byPath[r.path] = r }
        let exts = extensions ?? defaultExtensions
        var added = 0
        var updated = 0
        var finalRecords: [Record] = []
        var seenPaths: Set<String> = []
        for record in walkRecords(projectFolder: projectFolder, extensions: exts, maxFileSize: maxFileSize) {
            seenPaths.insert(record.path)
            if let prior = byPath[record.path] {
                if prior.sha256 != record.sha256 || prior.size != record.size {
                    updated += 1
                    finalRecords.append(record)
                } else {
                    finalRecords.append(prior)
                }
            } else {
                added += 1
                finalRecords.append(record)
            }
        }
        // Preserve existing records whose files no longer exist? No — drop them
        // so the index stays truthful. Rewrite the file atomically.
        try writeAll(finalRecords, to: file)
        return (added, updated, finalRecords.count)
    }

    // MARK: - Internals

    private static func readAllRecords(projectFolder: String) throws -> [Record] {
        let url = indexFile(in: projectFolder)
        guard FileManager.default.fileExists(atPath: url.path) else { return [] }
        let content = try String(contentsOf: url, encoding: .utf8)
        let decoder = JSONDecoder()
        var out: [Record] = []
        for line in content.split(separator: "\n", omittingEmptySubsequences: true) {
            if let data = String(line).data(using: .utf8),
               let rec = try? decoder.decode(Record.self, from: data)
            {
                out.append(rec)
            }
        }
        return out
    }

    private static func writeAll(_ records: [Record], to url: URL) throws {
        var buffer = ""
        for r in records {
            buffer += try jsonLine(for: r)
        }
        try buffer.write(to: url, atomically: true, encoding: .utf8)
    }

    private static func jsonLine(for record: Record) throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let data = try encoder.encode(record)
        guard let str = String(data: data, encoding: .utf8) else { return "" }
        return str + "\n"
    }

    /// Walks the project tree, reading each eligible file ONCE to extract size,
    /// sha256, line count, leading doc comment, and top-level symbols.
    private static func walkRecords(
        projectFolder: String,
        extensions: Set<String>,
        maxFileSize: Int
    ) -> [Record] {
        let fm = FileManager.default
        let root = URL(fileURLWithPath: projectFolder)
        guard let enumerator = fm.enumerator(
            at: root,
            includingPropertiesForKeys: [.isRegularFileKey, .fileSizeKey, .contentModificationDateKey],
            options: [.skipsHiddenFiles]
        ) else {
            return []
        }
        var records: [Record] = []
        let iso = ISO8601DateFormatter()
        iso.formatOptions = [.withInternetDateTime]
        for case let url as URL in enumerator {
            let components = url.pathComponents
            if components.contains(where: { skipDirs.contains($0) }) {
                if (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true {
                    enumerator.skipDescendants()
                }
                continue
            }
            guard let vals = try? url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey, .contentModificationDateKey]),
                  vals.isRegularFile == true else { continue }
            let ext = url.pathExtension.lowercased()
            guard extensions.contains(ext) else { continue }
            let size = vals.fileSize ?? 0
            if size > maxFileSize { continue }
            guard let data = try? Data(contentsOf: url, options: .mappedIfSafe) else { continue }
            let text = String(data: data, encoding: .utf8) ?? ""
            let sha = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
            let mtime = iso.string(from: vals.contentModificationDate ?? Date())
            let rel = relativePath(of: url, from: root)
            let (lineCount, doc, symbols) = analyze(text: text, language: ext)
            records.append(Record(
                path: rel,
                size: size,
                lines: lineCount,
                mtime: mtime,
                language: ext,
                sha256: sha,
                doc: doc,
                symbols: symbols
            ))
        }
        return records
    }

    private static func relativePath(of url: URL, from root: URL) -> String {
        let rootPath = root.resolvingSymlinksInPath().path
        let filePath = url.resolvingSymlinksInPath().path
        if filePath.hasPrefix(rootPath + "/") {
            return String(filePath.dropFirst(rootPath.count + 1))
        }
        return filePath
    }

    // MARK: - Per-File Analysis

    /// Returns (line count, leading doc comment or nil, top-level symbol signatures).
    static func analyze(text: String, language: String) -> (Int, String?, [String]) {
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
        let lineCount = lines.count
        let doc = extractLeadingDoc(lines: lines, language: language)
        let symbols = extractSymbols(lines: lines, language: language)
        return (lineCount, doc, symbols)
    }

    /// Captures the leading comment block at the top of the file (skipping
    /// blank lines and shebang), up to the first non-comment content line.
    /// Capped at `docCharCap` characters.
    private static func extractLeadingDoc(lines: [Substring], language: String) -> String? {
        var collected: [String] = []
        var started = false
        var inBlock = false
        for raw in lines {
            let line = String(raw).trimmingCharacters(in: .whitespaces)
            if !started {
                // Skip leading blank lines and shebang
                if line.isEmpty { continue }
                if line.hasPrefix("#!") { started = true; continue }
                started = true
            }
            if inBlock {
                if line.contains("*/") {
                    let trimmed = line.replacingOccurrences(of: "*/", with: "")
                        .replacingOccurrences(of: "*", with: "")
                        .trimmingCharacters(in: .whitespaces)
                    if !trimmed.isEmpty { collected.append(trimmed) }
                    inBlock = false
                    continue
                }
                let stripped = line.drop(while: { $0 == "*" || $0 == " " })
                collected.append(String(stripped))
                continue
            }
            if line.hasPrefix("///") {
                collected.append(String(line.dropFirst(3)).trimmingCharacters(in: .whitespaces))
            } else if line.hasPrefix("//") {
                collected.append(String(line.dropFirst(2)).trimmingCharacters(in: .whitespaces))
            } else if line.hasPrefix("/**") || line.hasPrefix("/*") {
                if line.contains("*/") {
                    let inner = line
                        .replacingOccurrences(of: "/**", with: "")
                        .replacingOccurrences(of: "/*", with: "")
                        .replacingOccurrences(of: "*/", with: "")
                        .trimmingCharacters(in: .whitespaces)
                    if !inner.isEmpty { collected.append(inner) }
                } else {
                    let inner = line
                        .replacingOccurrences(of: "/**", with: "")
                        .replacingOccurrences(of: "/*", with: "")
                        .trimmingCharacters(in: .whitespaces)
                    if !inner.isEmpty { collected.append(inner) }
                    inBlock = true
                }
            } else if line.hasPrefix("#") && (language == "py" || language == "rb" || language == "sh" || language == "bash" || language == "zsh" || language == "fish" || language == "yaml" || language == "yml" || language == "toml") {
                collected.append(String(line.dropFirst()).trimmingCharacters(in: .whitespaces))
            } else {
                break
            }
            if collected.map(\.count).reduce(0, +) > docCharCap * 2 { break }
        }
        let joined = collected.joined(separator: " ")
            .replacingOccurrences(of: "\u{00A0}", with: " ")
        let collapsed = joined.split(separator: " ", omittingEmptySubsequences: true).joined(separator: " ")
        if collapsed.isEmpty { return nil }
        if collapsed.count <= docCharCap { return collapsed }
        return String(collapsed.prefix(docCharCap)) + "…"
    }

    /// Top-level symbol signatures, language-aware regex extraction. Truncates
    /// each signature to keep the index compact.
    private static func extractSymbols(lines: [Substring], language: String) -> [String] {
        let patterns: [NSRegularExpression]
        switch language {
        case "swift":
            patterns = Self.swiftPatterns
        case "py":
            patterns = Self.pythonPatterns
        case "js", "ts", "jsx", "tsx", "mjs", "cjs":
            patterns = Self.jsPatterns
        case "go":
            patterns = Self.goPatterns
        case "rs":
            patterns = Self.rustPatterns
        case "rb":
            patterns = Self.rubyPatterns
        case "java", "kt", "scala":
            patterns = Self.jvmPatterns
        case "m", "mm", "h", "c", "cpp", "cc", "hpp":
            patterns = Self.cPatterns
        default:
            return []
        }
        var out: [String] = []
        var seen: Set<String> = []
        for raw in lines {
            let line = String(raw)
            if line.count > 240 { continue } // skip obvious non-signatures
            for rx in patterns {
                let range = NSRange(line.startIndex..., in: line)
                guard let match = rx.firstMatch(in: line, range: range) else { continue }
                let matched = line[Range(match.range, in: line)!]
                let sig = matched.trimmingCharacters(in: .whitespaces)
                    .replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
                if sig.isEmpty || seen.contains(sig) { continue }
                seen.insert(sig)
                out.append(sig.count > 120 ? String(sig.prefix(120)) + "…" : sig)
                if out.count >= symbolCountCap { return out }
                break
            }
        }
        return out
    }

    // Anchored to line-start with optional leading whitespace — matches
    // top-level-ish decls plus one indent level. Good enough without a parser.
    private static let swiftPatterns: [NSRegularExpression] = compile([
        #"^\s{0,8}(?:public|internal|private|fileprivate|open)?\s*(?:final\s+)?(?:class|struct|enum|protocol|actor)\s+\w+[^{]*"#,
        #"^\s{0,8}(?:public|internal|private|fileprivate|open)?\s*extension\s+\w+[^{]*"#,
        #"^\s{0,8}(?:public|internal|private|fileprivate|open)?\s*(?:static\s+)?func\s+\w+[^{]*"#,
        #"^\s{0,8}(?:public|internal|private|fileprivate|open)?\s*typealias\s+\w+\s*="#,
    ])
    private static let pythonPatterns: [NSRegularExpression] = compile([
        #"^\s{0,4}(?:async\s+)?def\s+\w+\([^)]*\)[^:]*"#,
        #"^\s{0,4}class\s+\w+[^:]*"#,
    ])
    private static let jsPatterns: [NSRegularExpression] = compile([
        #"^\s{0,4}(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*\w+\s*\([^)]*\)"#,
        #"^\s{0,4}(?:export\s+)?(?:default\s+)?class\s+\w+[^{]*"#,
        #"^\s{0,4}(?:export\s+)?(?:const|let|var)\s+\w+\s*=\s*(?:async\s+)?(?:\([^)]*\)|function)"#,
        #"^\s{0,4}(?:export\s+)?interface\s+\w+[^{]*"#,
        #"^\s{0,4}(?:export\s+)?type\s+\w+\s*="#,
    ])
    private static let goPatterns: [NSRegularExpression] = compile([
        #"^func\s+(?:\([^)]+\)\s+)?\w+\s*\([^)]*\)[^{]*"#,
        #"^type\s+\w+\s+(?:struct|interface)"#,
        #"^type\s+\w+\s+\w+"#,
    ])
    private static let rustPatterns: [NSRegularExpression] = compile([
        #"^\s{0,4}(?:pub\s+)?(?:async\s+)?fn\s+\w+[^{]*"#,
        #"^\s{0,4}(?:pub\s+)?(?:struct|enum|trait|union)\s+\w+[^{]*"#,
        #"^\s{0,4}impl(?:<[^>]+>)?\s+\w+[^{]*"#,
    ])
    private static let rubyPatterns: [NSRegularExpression] = compile([
        #"^\s{0,4}(?:def|class|module)\s+\w+[^\n]*"#,
    ])
    private static let jvmPatterns: [NSRegularExpression] = compile([
        #"^\s{0,4}(?:public|private|protected|internal|open)?\s*(?:final|abstract|sealed|data)?\s*(?:class|interface|object|enum)\s+\w+[^{]*"#,
        #"^\s{0,4}(?:public|private|protected|internal|open|override)?\s*(?:suspend\s+)?fun\s+\w+\s*\([^)]*\)[^{]*"#,
    ])
    private static let cPatterns: [NSRegularExpression] = compile([
        #"^@interface\s+\w+[^\n]*"#,
        #"^@protocol\s+\w+[^\n]*"#,
        #"^@implementation\s+\w+[^\n]*"#,
        #"^(?:static\s+|inline\s+)*[\w\*\s]+\s+\w+\s*\([^)]*\)\s*\{?\s*$"#,
    ])

    private static func compile(_ patterns: [String]) -> [NSRegularExpression] {
        patterns.compactMap { try? NSRegularExpression(pattern: $0) }
    }
}
