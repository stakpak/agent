import Foundation
import CryptoKit

/// Writes and reads a portable JSONL project index at
/// `{projectFolder}/.agent-index/index.jsonl`. One JSON object per line per
/// file, so any LLM can consume the file directly via `read_file`.
///
/// Record shape:
/// ```json
/// {"path":"Agent/App.swift","size":1234,"mtime":"2026-04-15T12:34:56Z","language":"swift","sha256":"..."}
/// ```
enum ProjectIndexService {

    // MARK: - Paths

    static let indexDirName = ".agent-index"
    static let indexFileName = "index.jsonl"

    static func indexDir(in projectFolder: String) -> URL {
        URL(fileURLWithPath: projectFolder).appendingPathComponent(indexDirName, isDirectory: true)
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
        "Pods", "Carthage", indexDirName, "xcuserdata", ".venv", "venv",
        "__pycache__", ".next", ".nuxt", "dist", "out",
    ]

    static let defaultMaxFileSize: Int = 1_000_000

    // MARK: - Record

    struct Record: Codable {
        let path: String
        let size: Int
        let mtime: String
        let language: String
        let sha256: String
    }

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

    /// Lazy sequence-like walker returning Record values for each eligible file.
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
            // Skip ignored dirs by name
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
            let mtime = iso.string(from: vals.contentModificationDate ?? Date())
            let rel = relativePath(of: url, from: root)
            let hash = sha256(of: url) ?? ""
            records.append(Record(
                path: rel,
                size: size,
                mtime: mtime,
                language: ext,
                sha256: hash
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

    private static func sha256(of url: URL) -> String? {
        guard let data = try? Data(contentsOf: url, options: .mappedIfSafe) else { return nil }
        let digest = SHA256.hash(data: data)
        return digest.map { String(format: "%02x", $0) }.joined()
    }
}
