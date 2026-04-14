import Foundation

/// Persists conversation messages to JSONL for session resume.
@MainActor
final class SessionStore {
    static let shared = SessionStore()

    private let sessionsDir: URL

    /// Current session ID (new UUID per task, or restored).
    private(set) var currentSessionId: String = UUID().uuidString

    /// Token state for cost restoration on resume.
    private(set) var sessionInputTokens: Int = 0
    private(set) var sessionOutputTokens: Int = 0

    private init() {
        let home = FileManager.default.homeDirectoryForCurrentUser
        sessionsDir = home.appendingPathComponent("Documents/AgentScript/sessions")
        try? FileManager.default.createDirectory(at: sessionsDir, withIntermediateDirectories: true)
        cleanOldSessions()
    }

    // MARK: - Write

    /// Start a new session. Call at task start.
    func newSession() {
        currentSessionId = UUID().uuidString
        sessionInputTokens = 0
        sessionOutputTokens = 0
    }

    /// Append a message to the current session's JSONL file.
    func appendMessage(_ message: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: message),
              var line = String(data: data, encoding: .utf8) else { return }
        line += "\n"
        let url = sessionFile(currentSessionId)
        if FileManager.default.fileExists(atPath: url.path) {
            guard let handle = try? FileHandle(forWritingTo: url) else { return }
            handle.seekToEndOfFile()
            handle.write(line.data(using: .utf8) ?? Data())
            handle.closeFile()
        } else {
            try? line.write(to: url, atomically: true, encoding: .utf8)
        }
    }

    /// Save token state for cost restoration.
    func saveTokenState(input: Int, output: Int) {
        sessionInputTokens = input
        sessionOutputTokens = output
        let meta: [String: Any] = [
            "_type": "session_meta",
            "inputTokens": input,
            "outputTokens": output,
            "timestamp": ISO8601DateFormatter().string(from: Date())
        ]
        appendMessage(meta)
    }

    // MARK: - Read / Resume

    /// List available sessions, newest first. Returns
    func listSessions() -> [(id: String, date: Date, messageCount: Int)] {
        let fm = FileManager.default
        let files = (try? fm.contentsOfDirectory(at: sessionsDir, includingPropertiesForKeys: [.contentModificationDateKey])) ?? []
        return files
            .filter { $0.pathExtension == "jsonl" }
            .compactMap { url -> (String, Date, Int)? in
                let id = url.deletingPathExtension().lastPathComponent
                let date = (try? url.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
                let lines = (try? String(contentsOf: url, encoding: .utf8))?.components(separatedBy: "\n").filter { !$0.isEmpty }.count ?? 0
                return (id, date, lines)
            }
            .sorted { $0.1 > $1.1 }
    }

    /// Load messages from a session.
    func loadSession(_ sessionId: String) -> [[String: Any]] {
        let url = sessionFile(sessionId)
        guard let content = try? String(contentsOf: url, encoding: .utf8) else { return [] }

        var messages: [[String: Any]] = []
        for line in content.components(separatedBy: "\n") where !line.isEmpty {
            guard let data = line.data(using: .utf8),
                  let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }

            // Restore token state from meta entries
            if obj["_type"] as? String == "session_meta" {
                sessionInputTokens = obj["inputTokens"] as? Int ?? 0
                sessionOutputTokens = obj["outputTokens"] as? Int ?? 0
                continue
            }
            messages.append(obj)
        }
        currentSessionId = sessionId
        return messages
    }

    /// Resume the most recent session. Returns messages or empty if none.
    func resumeLatest() -> [[String: Any]] {
        guard let latest = listSessions().first else { return [] }
        return loadSession(latest.id)
    }

    // MARK: - Cleanup

    /// Delete a session.
    func deleteSession(_ sessionId: String) {
        try? FileManager.default.removeItem(at: sessionFile(sessionId))
    }

    /// Remove sessions older than 7 days.
    private func cleanOldSessions() {
        let cutoff = Date().addingTimeInterval(-7 * 24 * 60 * 60)
        let sessions = listSessions()
        for session in sessions where session.date < cutoff {
            deleteSession(session.id)
        }
    }

    private func sessionFile(_ id: String) -> URL {
        sessionsDir.appendingPathComponent("\(id).jsonl")
    }
}
