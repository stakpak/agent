import AgentLLM
import FoundationModels
import Foundation

/// / On-device language model provider using Apple's Foundation Models framewor
@MainActor
final class FoundationModelService {
    let historyContext: String
    let userHome: String
    let userName: String
    let projectFolder: String

    private(set) var session: LanguageModelSession?

    /// Timeout for Apple Intelligence calls (seconds).
    private static let responseTimeout: TimeInterval = 5

    /// Call to force a new session (e.g. after prompt changes).
    func resetSession() { session = nil }

    /// The current session's transcript
    var transcript: Transcript? {
        session?.transcript
    }

    // MARK: - Enabled Tools (none — Apple AI is text-only, main LLM handles too

    /// Names of tools currently enabled for Apple Intelligence.
    var enabledToolNames: [String] { [] }

    // MARK: - Availability

    static var isAvailable: Bool {
        if case .available = SystemLanguageModel.default.availability {
            return true
        }
        return false
    }

    static var unavailabilityReason: String {
        switch SystemLanguageModel.default.availability {
        case .available:
            return ""
        case .unavailable(let reason):
            switch reason {
            case .appleIntelligenceNotEnabled:
                return "Apple Intelligence is not enabled in System Settings."
            case .deviceNotEligible:
                return "This device is not eligible for Apple Intelligence."
            case .modelNotReady:
                return "Apple Intelligence model is downloading or not ready."
            @unknown default:
                return "Apple Intelligence is not available."
            }
        }
    }

    // MARK: - Init

    init(historyContext: String = "", projectFolder: String = "") {
        self.historyContext = historyContext
        self.userHome = FileManager.default.homeDirectoryForCurrentUser.path
        self.userName = NSUserName()
        self.projectFolder = projectFolder
    }

    // MARK: - Session

    private func ensureSession() -> LanguageModelSession {
        let instructions = SystemPromptService.shared.prompt(
            for: .foundationModel,
            userName: userName,
            userHome: userHome,
            projectFolder: projectFolder,
            style: .compact
        )

        let s = LanguageModelSession(model: .default, instructions: Instructions(instructions))
        session = s
        return s
    }

    // MARK: - Send (non-streaming)

    func send(messages: [[String: Any]]) async throws -> (content: [[String: Any]], stopReason: String) {
        let s = ensureSession()
        let prompt = extractLastUserPrompt(from: messages)
        guard !prompt.isEmpty else {
            return ([["type": "text", "text": "(empty prompt)"]], "end_turn")
        }

        do {
            let content: String = try await withThrowingTaskGroup(of: String.self) { group in
                group.addTask {
                    let response = try await s.respond(to: prompt)
                    return response.content
                }
                group.addTask {
                    try await Task.sleep(for: .seconds(Self.responseTimeout))
                    throw CancellationError()
                }
                guard let result = try await group.next() else {
                    throw CancellationError()
                }
                group.cancelAll()
                return result
            }
            return ([["type": "text", "text": content]], "end_turn")
        } catch {
            self.session = nil
            if error is CancellationError {
                return ([["type": "text", "text": "Apple Intelligence timed out."]], "end_turn")
            }
            let msg = error.localizedDescription.lowercased()
            if msg.contains("unsafe") || msg.contains("guardrail") || msg.contains("policy") || msg.contains("safety") {
                return ([["type": "text", "text": "Apple Intelligence blocked this request due to safety filters."]], "end_turn")
            }
            throw error
        }
    }

    // MARK: - Streaming

    func sendStreaming(
        messages: [[String: Any]],
        onTextDelta: @escaping @Sendable (String) -> Void
    ) async throws -> (content: [[String: Any]], stopReason: String) {
        let s = ensureSession()
        let prompt = extractLastUserPrompt(from: messages)
        guard !prompt.isEmpty else {
            return ([["type": "text", "text": "(empty prompt)"]], "end_turn")
        }
        var fullText = ""

        do {
            fullText = try await withThrowingTaskGroup(of: String.self) { group in
                group.addTask {
                    var latest = ""
                    for try await snapshot in s.streamResponse(to: prompt) {
                        latest = snapshot.content
                    }
                    return latest
                }
                group.addTask {
                    try await Task.sleep(for: .seconds(Self.responseTimeout))
                    throw CancellationError()
                }
                guard let result = try await group.next() else {
                    throw CancellationError()
                }
                group.cancelAll()
                return result
            }
        } catch {
            self.session = nil
            if error is CancellationError {
                onTextDelta("Apple Intelligence timed out. Please try again.")
                return ([["type": "text", "text": "Apple Intelligence timed out."]], "end_turn")
            }
            let msg = error.localizedDescription.lowercased()
            if msg.contains("unsafe") || msg.contains("guardrail") || msg.contains("policy") || msg.contains("safety") {
                let notice = "Apple Intelligence blocked this request due to safety filters."
                onTextDelta(notice)
                return ([["type": "text", "text": notice]], "end_turn")
            }
            throw error
        }

        if fullText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            onTextDelta("(no response)")
        } else {
            onTextDelta(fullText)
        }
        return ([["type": "text", "text": fullText]], "end_turn")
    }

    // MARK: - Helpers

    // MARK: - Prompt Cleanup

    /// Clean up a user prompt (fix spelling/grammar) using Apple Intelligence.
    static func cleanUpPrompt(_ text: String) async -> String {
        guard isAvailable else { return text }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return text }
        do {
            let session = LanguageModelSession(
                model: .default,
                instructions: Instructions(
                    "Fix spelling and grammar only. Return ONLY the corrected text. "
                        + "Do not add quotes, explanations, or change meaning. "
                        + "Keep it concise."
                )
            )
            let response: String = try await withThrowingTaskGroup(of: String.self) { group in
                group.addTask {
                    let r = try await session.respond(to: trimmed)
                    return r.content
                }
                group.addTask {
                    try await Task.sleep(for: .seconds(3))
                    throw CancellationError()
                }
                guard let result = try await group.next() else {
                    throw CancellationError()
                }
                group.cancelAll()
                return result
            }
            let cleaned = response.trimmingCharacters(in: .whitespacesAndNewlines)
            return cleaned.isEmpty ? text : cleaned
        } catch {
            return text
        }
    }

    /// Extract only the last user message text.
    private func extractLastUserPrompt(from messages: [[String: Any]]) -> String {
        for msg in messages.reversed() {
            guard let role = msg["role"] as? String, role == "user" else { continue }
            if let text = msg["content"] as? String { return text }
            if let blocks = msg["content"] as? [[String: Any]] {
                let text = blocks.compactMap { block -> String? in
                    guard block["type"] as? String == "text" else { return nil }
                    return block["text"] as? String
                }.joined(separator: "\n")
                return text
            }
        }
        return ""
    }
}

// MARK: - Shared State (used by NativeToolHandler for task loop coordination)

/// Shared state for native tool handling
enum NativeToolContext {
    @MainActor static var projectFolder: String = ""
    /// Set when task_complete is called
    @MainActor static var taskCompleteSummary: String?
    /// Counts tool calls per session turn to prevent infinite loops.
    @MainActor static var toolCallCount = 0
    /// Max tool calls before forcing task_complete.
    static let maxToolCalls = 50
}
