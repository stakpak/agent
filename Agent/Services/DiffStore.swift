import Foundation
import AgentD1F

/// Errors for diff tool operations.
enum DiffError: LocalizedError {
    case invalidDiff

    var errorDescription: String? {
        switch self {
        case .invalidDiff: return "No diff_id or inline diff provided"
        }
    }
}

/// / In-memory store for diff results keyed by UUID.
@MainActor
final class DiffStore {
    static let shared = DiffStore()

    private var diffs: [UUID: DiffResult] = [:]
    private var sources: [UUID: String] = [:]
    private var filePaths: [UUID: String] = [:] // diff_id → file path it was ap
    private var appliedDiffs: [String: [UUID]] = [:] // file path → stack of app
    private var editHistory: [String: String] = [:] // file path → original cont

    private init() {}

    /// Store a diff and its source text. Returns the UUID key.
    func store(diff: DiffResult, source: String) -> UUID {
        let id = UUID()
        diffs[id] = diff
        sources[id] = source
        return id
    }

    /// Retrieve a stored diff by UUID.
    func retrieve(_ id: UUID) -> (diff: DiffResult, source: String)? {
        guard let diff = diffs[id], let source = sources[id] else { return nil }
        return (diff, source)
    }

    /// Record that a diff was applied to a file. Call after writing the file.
    func recordApply(diffId: UUID, filePath: String, originalContent: String) {
        filePaths[diffId] = filePath
        appliedDiffs[filePath, default: []].append(diffId)
        // Only record the original if this is the first edit
        if editHistory[filePath] == nil {
            editHistory[filePath] = originalContent
        }
    }

    /// Get the last applied diff_id for a file (for undo).
    func lastAppliedDiffId(for filePath: String) -> UUID? {
        appliedDiffs[filePath]?.last
    }

    /// Pop the last applied diff for a file after undo.
    func popLastApplied(for filePath: String) {
        appliedDiffs[filePath]?.removeLast()
        if appliedDiffs[filePath]?.isEmpty == true {
            appliedDiffs.removeValue(forKey: filePath)
        }
    }

    /// Record original content before an edit for undo support.
    func recordEdit(filePath: String, originalContent: String) {
        if editHistory[filePath] == nil {
            editHistory[filePath] = originalContent
        }
    }

    /// Retrieve original content for undo.
    func lastEdit(for filePath: String) -> String? {
        editHistory[filePath]
    }

    /// Clear undo history for a file after successful undo.
    func clearEditHistory(for filePath: String) {
        editHistory.removeValue(forKey: filePath)
        appliedDiffs.removeValue(forKey: filePath)
    }

    /// Invalidate all stored diffs for a file
    func invalidateDiffs(for filePath: String) {
        let idsToRemove = filePaths.filter { $0.value == filePath }.map { $0.key }
        // Also remove any diff whose stored source came from this file
        for id in diffs.keys {
            if filePaths[id] == nil {
                // Diff not yet applied
            }
        }
        for id in idsToRemove {
            diffs.removeValue(forKey: id)
            sources.removeValue(forKey: id)
            filePaths.removeValue(forKey: id)
        }
    }

    /// Clear all stored diffs (call at task start).
    func clear() {
        diffs.removeAll()
        sources.removeAll()
        filePaths.removeAll()
        appliedDiffs.removeAll()
        editHistory.removeAll()
    }
}
