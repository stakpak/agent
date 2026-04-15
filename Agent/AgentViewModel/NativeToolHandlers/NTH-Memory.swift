
@preconcurrency import Foundation

// MARK: - Native Tool Handler — Memory (Claude-compatible)

extension AgentViewModel {

    /// Claude `memory_20250818` tool, implemented locally. Paths must start
    /// with `/memories` and are mapped to ~/Documents/AgentScript/memory/.
    /// Supported commands: view, create, str_replace, insert, delete, rename.
    func handleMemoryTool(input: [String: Any]) -> String {
        let command = (input["command"] as? String ?? "").lowercased()
        switch command {
        case "view":
            return memView(input: input)
        case "create":
            return memCreate(input: input)
        case "str_replace":
            return memStrReplace(input: input)
        case "insert":
            return memInsert(input: input)
        case "delete":
            return memDelete(input: input)
        case "rename":
            return memRename(input: input)
        case "":
            return "❌ memory: missing `command`. Use view|create|str_replace|insert|delete|rename. Recovery: start with memory(command:\"view\", path:\"/memories\")."
        default:
            return "❌ memory: unknown command '\(command)'. Supported: view|create|str_replace|insert|delete|rename."
        }
    }

    // MARK: - Path sandboxing

    /// All memory paths live under the MemoryStore root. Accepts `/memories`,
    /// `/memories/foo.md`, or `memories/foo.md` — never escapes the sandbox.
    private static func resolveMemoryURL(_ rawPath: String) -> URL? {
        let root = MemoryStore.shared.memoryRoot
        var p = rawPath.trimmingCharacters(in: .whitespaces)
        if p.hasPrefix("/") { p.removeFirst() }
        // Accept `memories` prefix but tolerate callers who omit it.
        if p == "memories" { p = "" }
        else if p.hasPrefix("memories/") { p.removeFirst("memories/".count) }
        if p.contains("..") { return nil }
        let url: URL = p.isEmpty ? root : root.appendingPathComponent(p)
        let standardized = url.standardizedFileURL
        let rootStd = root.standardizedFileURL
        guard standardized.path == rootStd.path
            || standardized.path.hasPrefix(rootStd.path + "/") else {
            return nil
        }
        return standardized
    }

    private static func ensureRoot() {
        try? FileManager.default.createDirectory(
            at: MemoryStore.shared.memoryRoot,
            withIntermediateDirectories: true
        )
    }

    // MARK: - Commands

    private func memView(input: [String: Any]) -> String {
        let rawPath = input["path"] as? String ?? "/memories"
        Self.ensureRoot()
        guard let url = Self.resolveMemoryURL(rawPath) else {
            return "❌ memory.view: path must stay under /memories (no .. traversal)."
        }
        let fm = FileManager.default
        var isDir: ObjCBool = false
        let exists = fm.fileExists(atPath: url.path, isDirectory: &isDir)
        if !exists {
            return url == MemoryStore.shared.memoryRoot
                ? "(memory is empty — no files under /memories yet)"
                : "❌ memory.view: path not found: \(rawPath). Recovery: call view on /memories to list."
        }
        if isDir.boolValue {
            let names = (try? fm.contentsOfDirectory(atPath: url.path))?.sorted() ?? []
            if names.isEmpty { return "(empty directory: \(rawPath))" }
            return names.map { "/memories/\($0)" }.joined(separator: "\n")
        }
        guard let text = try? String(contentsOf: url, encoding: .utf8) else {
            return "❌ memory.view: cannot read \(rawPath) as UTF-8."
        }
        // Optional view_range: [start, end] (1-based, inclusive).
        if let range = input["view_range"] as? [Int], range.count == 2 {
            let lines = text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
            let start = max(1, range[0])
            let end = min(lines.count, range[1] == -1 ? lines.count : range[1])
            guard start <= end else { return "" }
            return lines[(start - 1)..<end].joined(separator: "\n")
        }
        return text
    }

    private func memCreate(input: [String: Any]) -> String {
        let rawPath = input["path"] as? String ?? ""
        let content = input["file_text"] as? String ?? input["content"] as? String ?? ""
        guard !rawPath.isEmpty else { return "❌ memory.create: `path` is required." }
        Self.ensureRoot()
        guard let url = Self.resolveMemoryURL(rawPath) else {
            return "❌ memory.create: path must stay under /memories."
        }
        if url == MemoryStore.shared.memoryRoot {
            return "❌ memory.create: cannot write to the /memories directory itself — include a filename."
        }
        let fm = FileManager.default
        try? fm.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        do {
            try content.write(to: url, atomically: true, encoding: .utf8)
            return "✅ Created \(rawPath) (\(content.count) chars)."
        } catch {
            return "❌ memory.create failed: \(error.localizedDescription)"
        }
    }

    private func memStrReplace(input: [String: Any]) -> String {
        let rawPath = input["path"] as? String ?? ""
        let oldStr = input["old_str"] as? String ?? input["old_string"] as? String ?? ""
        let newStr = input["new_str"] as? String ?? input["new_string"] as? String ?? ""
        guard !rawPath.isEmpty else { return "❌ memory.str_replace: `path` is required." }
        guard !oldStr.isEmpty else { return "❌ memory.str_replace: `old_str` is required." }
        guard let url = Self.resolveMemoryURL(rawPath) else {
            return "❌ memory.str_replace: path must stay under /memories."
        }
        guard let text = try? String(contentsOf: url, encoding: .utf8) else {
            return "❌ memory.str_replace: file not found: \(rawPath). Recovery: create it first with command=create."
        }
        let occurrences = text.components(separatedBy: oldStr).count - 1
        if occurrences == 0 {
            return "❌ memory.str_replace: `old_str` not found in \(rawPath). Recovery: view the file first to see its exact contents."
        }
        if occurrences > 1 {
            return "❌ memory.str_replace: `old_str` appears \(occurrences) times — must be unique. Recovery: include more context in old_str to disambiguate."
        }
        let replaced = text.replacingOccurrences(of: oldStr, with: newStr)
        do {
            try replaced.write(to: url, atomically: true, encoding: .utf8)
            return "✅ Replaced 1 occurrence in \(rawPath)."
        } catch {
            return "❌ memory.str_replace failed: \(error.localizedDescription)"
        }
    }

    private func memInsert(input: [String: Any]) -> String {
        let rawPath = input["path"] as? String ?? ""
        let insertLine = input["insert_line"] as? Int ?? 0
        let insertText = input["insert_text"] as? String ?? input["text"] as? String ?? ""
        guard !rawPath.isEmpty else { return "❌ memory.insert: `path` is required." }
        guard let url = Self.resolveMemoryURL(rawPath) else {
            return "❌ memory.insert: path must stay under /memories."
        }
        guard let text = try? String(contentsOf: url, encoding: .utf8) else {
            return "❌ memory.insert: file not found: \(rawPath). Recovery: create it first with command=create."
        }
        var lines = text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        let insertAt = max(0, min(lines.count, insertLine))
        let newLines = insertText.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        lines.insert(contentsOf: newLines, at: insertAt)
        let joined = lines.joined(separator: "\n")
        do {
            try joined.write(to: url, atomically: true, encoding: .utf8)
            return "✅ Inserted \(newLines.count) line(s) at line \(insertAt) in \(rawPath)."
        } catch {
            return "❌ memory.insert failed: \(error.localizedDescription)"
        }
    }

    private func memDelete(input: [String: Any]) -> String {
        let rawPath = input["path"] as? String ?? ""
        guard !rawPath.isEmpty else { return "❌ memory.delete: `path` is required." }
        guard let url = Self.resolveMemoryURL(rawPath) else {
            return "❌ memory.delete: path must stay under /memories."
        }
        if url == MemoryStore.shared.memoryRoot {
            return "❌ memory.delete: refusing to delete the /memories root directory."
        }
        let fm = FileManager.default
        guard fm.fileExists(atPath: url.path) else {
            return "(already gone: \(rawPath))"
        }
        do {
            try fm.removeItem(at: url)
            return "🧹 Deleted \(rawPath)."
        } catch {
            return "❌ memory.delete failed: \(error.localizedDescription)"
        }
    }

    private func memRename(input: [String: Any]) -> String {
        let oldPath = input["old_path"] as? String ?? input["path"] as? String ?? ""
        let newPath = input["new_path"] as? String ?? ""
        guard !oldPath.isEmpty, !newPath.isEmpty else {
            return "❌ memory.rename: `old_path` and `new_path` are required."
        }
        guard let src = Self.resolveMemoryURL(oldPath), let dst = Self.resolveMemoryURL(newPath) else {
            return "❌ memory.rename: paths must stay under /memories."
        }
        let fm = FileManager.default
        guard fm.fileExists(atPath: src.path) else {
            return "❌ memory.rename: source not found: \(oldPath)"
        }
        if fm.fileExists(atPath: dst.path) {
            return "❌ memory.rename: destination already exists: \(newPath)"
        }
        do {
            try fm.createDirectory(at: dst.deletingLastPathComponent(), withIntermediateDirectories: true)
            try fm.moveItem(at: src, to: dst)
            return "✅ Renamed \(oldPath) → \(newPath)."
        } catch {
            return "❌ memory.rename failed: \(error.localizedDescription)"
        }
    }
}
