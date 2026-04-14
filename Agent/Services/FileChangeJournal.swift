import Foundation
import CryptoKit

/// Logs every file edit as JSONL for overnight review.
@MainActor
final class FileChangeJournal {
    static let shared = FileChangeJournal()

    private let journalDir: URL

    private init() {
        let home = FileManager.default.homeDirectoryForCurrentUser
        journalDir = home.appendingPathComponent("Documents/AgentScript/journal")
        try? FileManager.default.createDirectory(at: journalDir, withIntermediateDirectories: true)
        cleanOld()
    }

    /// Log a file change. Call before or after writes/edits.
    func log(action: String, filePath: String, beforeContent: String? = nil, afterContent: String? = nil, tool: String = "") {
        let entry: [String: Any] = [
            "time": ISO8601DateFormatter().string(from: Date()),
            "action": action,
            "file": filePath,
            "tool": tool,
            "before_hash": beforeContent.map { sha256($0) } ?? "",
            "after_hash": afterContent.map { sha256($0) } ?? "",
            "before_lines": beforeContent?.components(separatedBy: "\n").count ?? 0,
            "after_lines": afterContent?.components(separatedBy: "\n").count ?? 0,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: entry),
              var line = String(data: data, encoding: .utf8) else { return }
        line += "\n"
        let url = todayFile()
        if FileManager.default.fileExists(atPath: url.path) {
            guard let handle = try? FileHandle(forWritingTo: url) else { return }
            handle.seekToEndOfFile()
            handle.write(line.data(using: .utf8) ?? Data())
            handle.closeFile()
        } else {
            try? line.write(to: url, atomically: true, encoding: .utf8)
        }
    }

    /// Read today's journal as an array of entries.
    func todayEntries() -> [[String: Any]] {
        let url = todayFile()
        guard let content = try? String(contentsOf: url, encoding: .utf8) else { return [] }
        return content.components(separatedBy: "\n")
            .filter { !$0.isEmpty }
            .compactMap { line in
                guard let data = line.data(using: .utf8) else { return nil }
                return try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            }
    }

    /// Summary for display — files changed today with action counts.
    func todaySummary() -> String {
        let entries = todayEntries()
        guard !entries.isEmpty else { return "No file changes today." }
        var fileCounts: [String: Int] = [:]
        for entry in entries {
            let file = (entry["file"] as? String ?? "unknown")
            fileCounts[file, default: 0] += 1
        }
        return fileCounts.sorted { $0.value > $1.value }
            .map { "\($0.key): \($0.value) change(s)" }
            .joined(separator: "\n")
    }

    private func todayFile() -> URL {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd"
        return journalDir.appendingPathComponent("\(formatter.string(from: Date())).jsonl")
    }

    private func sha256(_ string: String) -> String {
        let data = Data(string.utf8)
        let hash = SHA256.hash(data: data)
        return hash.prefix(8).map { String(format: "%02x", $0) }.joined()
    }

    /// Remove journals older than 30 days.
    private func cleanOld() {
        let cutoff = Date().addingTimeInterval(-30 * 24 * 60 * 60)
        let files = (try? FileManager.default.contentsOfDirectory(
            at: journalDir,
            includingPropertiesForKeys: [.contentModificationDateKey]
        )) ?? []
        for file in files {
            if let date = (try? file.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate), date < cutoff {
                try? FileManager.default.removeItem(at: file)
            }
        }
    }
}
