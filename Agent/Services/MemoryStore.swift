import Foundation

/// Memory types — user, feedback, project, reference.
enum MemoryType: String, CaseIterable {
    case user // role, goals, preferences, knowledge
    case feedback // approach guidance — what to avoid or repeat
    case project // ongoing work, deadlines, decisions
    case reference // pointers to external systems
}

/// A single memory entry with frontmatter metadata and content.
struct MemoryEntry: Identifiable {
    let id: String // filename without extension
    var name: String
    var description: String
    var type: MemoryType
    var content: String // body text below frontmatter

    /// Parse a .md file with YAML-style frontmatter.
    static func parse(id: String, raw: String) -> MemoryEntry? {
        guard raw.hasPrefix("---") else { return nil }
        let parts = raw.components(separatedBy: "---")
        guard parts.count >= 3 else { return nil }
        let frontmatter = parts[1]
        let body = parts.dropFirst(2).joined(separator: "---").trimmingCharacters(in: .whitespacesAndNewlines)

        var name = id
        var description = ""
        var type: MemoryType = .user

        for line in frontmatter.components(separatedBy: "\n") {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("name:") {
                name = String(trimmed.dropFirst(5)).trimmingCharacters(in: .whitespaces)
            } else if trimmed.hasPrefix("description:") {
                description = String(trimmed.dropFirst(12)).trimmingCharacters(in: .whitespaces)
            } else if trimmed.hasPrefix("type:") {
                let raw = String(trimmed.dropFirst(5)).trimmingCharacters(in: .whitespaces)
                type = MemoryType(rawValue: raw) ?? .user
            }
        }
        return MemoryEntry(id: id, name: name, description: description, type: type, content: body)
    }

    /// Serialize back to markdown with frontmatter.
    func serialize() -> String {
        """
        ---
        name: \(name)
        description: \(description)
        type: \(type.rawValue)
        ---

        \(content)
        """
    }

    /// One-line manifest entry for relevance scanning.
    var manifestLine: String {
        "[\(type.rawValue)] \(id) — \(description.prefix(120))"
    }
}

/// Persistent user memory system with typed, per-topic files.
/// Backward compatible with the legacy flat memory.md file.
/// Directory: ~/Documents/AgentScript/memory/
/// Index: ~/Documents/AgentScript/memory/MEMORY.md
/// Legacy: ~/Documents/AgentScript/memory.md (migrated on first access)
@MainActor
final class MemoryStore {
    static let shared = MemoryStore()

    private let baseDir: URL
    private let memoryDir: URL
    private let legacyFileURL: URL
    private let indexURL: URL

    private init() {
        let home = FileManager.default.homeDirectoryForCurrentUser
        let dir = home.appendingPathComponent("Documents/AgentScript")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

        baseDir = dir
        legacyFileURL = dir.appendingPathComponent("memory.md")
        memoryDir = dir.appendingPathComponent("memory")
        indexURL = memoryDir.appendingPathComponent("MEMORY.md")

        try? FileManager.default.createDirectory(at: memoryDir, withIntermediateDirectories: true)
        migrateLegacyIfNeeded()
    }

    // MARK: - Migration

    /// Migrate legacy memory.md content into the new directory structure.
    private func migrateLegacyIfNeeded() {
        guard FileManager.default.fileExists(atPath: legacyFileURL.path),
              let legacy = try? String(contentsOf: legacyFileURL, encoding: .utf8),
              !legacy.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }

        // Only migrate if memory dir is empty (no entries yet)
        let existing = (try? FileManager.default.contentsOfDirectory(at: memoryDir, includingPropertiesForKeys: nil)) ?? []
        let mdFiles = existing.filter { $0.pathExtension == "md" && $0.lastPathComponent != "MEMORY.md" }
        guard mdFiles.isEmpty else { return }

        // Create a single "legacy" memory entry with all prior content
        let entry = MemoryEntry(
            id: "legacy_preferences",
            name: "Legacy preferences",
            description: "Migrated from memory.md — user preferences and notes",
            type: .user,
            content: legacy
        )
        save(entry)
        rebuildIndex()
    }

    // MARK: - CRUD

    /// Save or update a memory entry.
    func save(_ entry: MemoryEntry) {
        let url = memoryDir.appendingPathComponent("\(entry.id).md")
        try? entry.serialize().write(to: url, atomically: true, encoding: .utf8)
    }

    /// Delete a memory entry by ID.
    func delete(id: String) {
        let url = memoryDir.appendingPathComponent("\(id).md")
        try? FileManager.default.removeItem(at: url)
    }

    /// Load a single memory entry by ID.
    func load(id: String) -> MemoryEntry? {
        let url = memoryDir.appendingPathComponent("\(id).md")
        guard let raw = try? String(contentsOf: url, encoding: .utf8) else { return nil }
        return MemoryEntry.parse(id: id, raw: raw)
    }

    /// List all memory entries (frontmatter only — content loaded lazily).
    func listAll() -> [MemoryEntry] {
        let files = (try? FileManager.default.contentsOfDirectory(
            at: memoryDir,
            includingPropertiesForKeys: [.contentModificationDateKey]
        )) ?? []
        return files
            .filter { $0.pathExtension == "md" && $0.lastPathComponent != "MEMORY.md" }
            .sorted { a, b in
                let da = (try? a.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
                let db = (try? b.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
                return da > db // newest first
            }
            .compactMap { url in
                let id = url.deletingPathExtension().lastPathComponent
                guard let raw = try? String(contentsOf: url, encoding: .utf8) else { return nil }
                return MemoryEntry.parse(id: id, raw: raw)
            }
    }

    /// Build a manifest string for relevance scanning (one line per entry).
    func manifest() -> String {
        listAll().map(\.manifestLine).joined(separator: "\n")
    }

    /// Rebuild the MEMORY.md index file from current entries.
    func rebuildIndex() {
        let lines = listAll().map { "- [\($0.name)](\($0.id).md) — \($0.description.prefix(100))" }
        let index = lines.joined(separator: "\n")
        try? index.write(to: indexURL, atomically: true, encoding: .utf8)
    }

    // MARK: - Context Injection

    /// Memory content formatted for injection into LLM system prompt.
    /// Loads all entries and includes full content (for small memory sets).
    var contextBlock: String {
        let entries = listAll()
        guard !entries.isEmpty else { return "" }

        var block = "\n\nUSER MEMORY (follow these preferences):\n"
        for entry in entries.prefix(10) { // cap at 10 entries for context size
            block += "\n## \(entry.name) [\(entry.type.rawValue)]\n\(entry.content)\n"
        }
        return block
    }

    // MARK: - Legacy Compatibility

    /// Read the full legacy memory content (for backward compat with /memory commands).
    var content: String {
        let entries = listAll()
        return entries.map(\.content).joined(separator: "\n\n")
    }

    /// Write/replace — creates or updates a "general" entry.
    func write(_ text: String) {
        if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            // Clear all entries
            for entry in listAll() { delete(id: entry.id) }
            rebuildIndex()
            return
        }
        let entry = MemoryEntry(id: "general", name: "General", description: "User preferences and notes", type: .user, content: text)
        save(entry)
        rebuildIndex()
    }

    /// Append a line to the "general" memory entry.
    func append(_ line: String) {
        var entry = load(id: "general") ??
            MemoryEntry(id: "general", name: "General", description: "User preferences and notes", type: .user, content: "")
        if !entry.content.isEmpty && !entry.content.hasSuffix("\n") { entry.content += "\n" }
        entry.content += line + "\n"
        save(entry)
        rebuildIndex()
    }

    /// Remove a line containing the given text from the "general" entry.
    func removeLine(containing text: String) {
        guard var entry = load(id: "general") else { return }
        let lines = entry.content.components(separatedBy: "\n")
        entry.content = lines.filter { !$0.contains(text) }.joined(separator: "\n")
        save(entry)
        rebuildIndex()
    }
}
