
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

    // MARK: - Scopes

    /// Backing store for a memory call. `global` is shared across every
    /// project (user-level). `project` is scoped to the active project folder
    /// so different codebases can each carry their own notes without leaking.
    enum MemoryScope {
        case global
        case project
    }

    private func scopeFrom(_ raw: String?) -> MemoryScope {
        switch (raw ?? "global").lowercased() {
        case "project", "local", "folder": return .project
        default: return .global
        }
    }

    /// Resolve a scope to its filesystem root. Returns `nil` for project
    /// scope when no project folder is selected.
    private func rootURL(for scope: MemoryScope) -> URL? {
        switch scope {
        case .global:
            return MemoryStore.shared.memoryRoot
        case .project:
            let pf = projectFolder.trimmingCharacters(in: .whitespaces)
            guard !pf.isEmpty else { return nil }
            return AgentProjectPaths.url(in: pf, .memory)
        }
    }

    // MARK: - Path sandboxing

    /// Map a `/memories/...` path onto the chosen backing root, rejecting
    /// `..` traversal and anything that resolves outside the sandbox.
    private func resolveMemoryURL(_ rawPath: String, scope: MemoryScope) -> (URL, URL)? {
        guard let root = rootURL(for: scope) else { return nil }
        var p = rawPath.trimmingCharacters(in: .whitespaces)
        if p.hasPrefix("/") { p.removeFirst() }
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
        return (standardized, rootStd)
    }

    private func ensureRoot(scope: MemoryScope) -> Bool {
        guard let root = rootURL(for: scope) else { return false }
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        return true
    }

    private static let projectScopeRequiredMsg = "❌ memory: scope:\"project\" requires a project folder to be selected. Recovery: set the project folder (directory tool or the folder picker), or use scope:\"global\"."

    // MARK: - Commands

    private func memView(input: [String: Any]) -> String {
        let scope = scopeFrom(input["scope"] as? String)
        let rawPath = input["path"] as? String ?? "/memories"
        guard ensureRoot(scope: scope) else { return Self.projectScopeRequiredMsg }
        guard let (url, root) = resolveMemoryURL(rawPath, scope: scope) else {
            return "❌ memory.view: path must stay under /memories (no .. traversal)."
        }
        let fm = FileManager.default
        var isDir: ObjCBool = false
        let exists = fm.fileExists(atPath: url.path, isDirectory: &isDir)
        if !exists {
            return url.path == root.path
                ? "(\(scope == .project ? "project " : "")memory is empty — no files under /memories yet)"
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
        let scope = scopeFrom(input["scope"] as? String)
        let rawPath = input["path"] as? String ?? ""
        let content = input["file_text"] as? String ?? input["content"] as? String ?? ""
        guard !rawPath.isEmpty else { return "❌ memory.create: `path` is required." }
        guard ensureRoot(scope: scope) else { return Self.projectScopeRequiredMsg }
        guard let (url, root) = resolveMemoryURL(rawPath, scope: scope) else {
            return "❌ memory.create: path must stay under /memories."
        }
        if url.path == root.path {
            return "❌ memory.create: cannot write to the /memories directory itself — include a filename."
        }
        let fm = FileManager.default
        try? fm.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        do {
            try content.write(to: url, atomically: true, encoding: .utf8)
            return "✅ Created \(rawPath) in \(scope == .project ? "project" : "global") memory (\(content.count) chars)."
        } catch {
            return "❌ memory.create failed: \(error.localizedDescription)"
        }
    }

    private func memStrReplace(input: [String: Any]) -> String {
        let scope = scopeFrom(input["scope"] as? String)
        let rawPath = input["path"] as? String ?? ""
        let oldStr = input["old_str"] as? String ?? input["old_string"] as? String ?? ""
        let newStr = input["new_str"] as? String ?? input["new_string"] as? String ?? ""
        guard !rawPath.isEmpty else { return "❌ memory.str_replace: `path` is required." }
        guard !oldStr.isEmpty else { return "❌ memory.str_replace: `old_str` is required." }
        guard ensureRoot(scope: scope) else { return Self.projectScopeRequiredMsg }
        guard let (url, _) = resolveMemoryURL(rawPath, scope: scope) else {
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
        let scope = scopeFrom(input["scope"] as? String)
        let rawPath = input["path"] as? String ?? ""
        let insertLine = input["insert_line"] as? Int ?? 0
        let insertText = input["insert_text"] as? String ?? input["text"] as? String ?? ""
        guard !rawPath.isEmpty else { return "❌ memory.insert: `path` is required." }
        guard ensureRoot(scope: scope) else { return Self.projectScopeRequiredMsg }
        guard let (url, _) = resolveMemoryURL(rawPath, scope: scope) else {
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
        let scope = scopeFrom(input["scope"] as? String)
        let rawPath = input["path"] as? String ?? ""
        guard !rawPath.isEmpty else { return "❌ memory.delete: `path` is required." }
        guard ensureRoot(scope: scope) else { return Self.projectScopeRequiredMsg }
        guard let (url, root) = resolveMemoryURL(rawPath, scope: scope) else {
            return "❌ memory.delete: path must stay under /memories."
        }
        if url.path == root.path {
            return "❌ memory.delete: refusing to delete the /memories root directory."
        }
        let fm = FileManager.default
        guard fm.fileExists(atPath: url.path) else {
            return "(already gone: \(rawPath))"
        }
        do {
            try fm.removeItem(at: url)
            return "🧹 Deleted \(rawPath) from \(scope == .project ? "project" : "global") memory."
        } catch {
            return "❌ memory.delete failed: \(error.localizedDescription)"
        }
    }

    private func memRename(input: [String: Any]) -> String {
        let scope = scopeFrom(input["scope"] as? String)
        let oldPath = input["old_path"] as? String ?? input["path"] as? String ?? ""
        let newPath = input["new_path"] as? String ?? ""
        guard !oldPath.isEmpty, !newPath.isEmpty else {
            return "❌ memory.rename: `old_path` and `new_path` are required."
        }
        guard ensureRoot(scope: scope) else { return Self.projectScopeRequiredMsg }
        guard let (src, _) = resolveMemoryURL(oldPath, scope: scope),
              let (dst, _) = resolveMemoryURL(newPath, scope: scope) else {
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
