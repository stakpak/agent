import Foundation

enum AgentError: Error, LocalizedError {
    case noAPIKey
    case apiError(statusCode: Int, message: String)
    case invalidResponse
    case invalidURL
    case timeout(seconds: TimeInterval)
    case serviceUnavailable(service: String)
    case permissionDenied(permission: String)
    case toolFailed(tool: String, reason: String)
    case scriptError(script: String, message: String)
    case xpcError(service: String, reason: String)
    case mcpError(server: String, reason: String)
    case accessibilityError(action: String, reason: String)
    case networkError(underlying: Error)
    case fileError(path: String, reason: String)
    case notFound(item: String)
    case invalidInput(field: String, reason: String)
    case cancelled
    case unknown(Error)

    var errorDescription: String? {
        switch self {
        case .noAPIKey: "No API key configured. Open Settings to add your Anthropic API key."
        case .apiError(let code, let msg): "API error (\(code)): \(msg)"
        case .invalidResponse: "Invalid response from API"
        case .invalidURL: "Invalid URL"
        case .timeout(let seconds): "Operation timed out after \(Int(seconds)) seconds"
        case .serviceUnavailable(let service): "Service '\(service)' is not available. Try restarting the service or the app."
        case .permissionDenied(let permission): "Permission denied: \(permission). Grant access in System Settings > Privacy & Security."
        case .toolFailed(let tool, let reason): "Tool '\(tool)' failed: \(reason)"
        case .scriptError(let script, let message): "Script '\(script)' error: \(message)"
        case .xpcError(let service, let reason): "XPC communication with '\(service)' failed: \(reason)"
        case .mcpError(let server, let reason): "MCP server '\(server)' error: \(reason)"
        case .accessibilityError(let action, let reason): "Accessibility action '\(action)' failed: \(reason)"
        case .networkError(let underlying): "Network error: \(underlying.localizedDescription)"
        case .fileError(let path, let reason): "File error at '\(path)': \(reason)"
        case .notFound(let item): "\(item) not found"
        case .invalidInput(let field, let reason): "Invalid input for '\(field)': \(reason)"
        case .cancelled: "Operation was cancelled"
        case .unknown(let error): "Unexpected error: \(error.localizedDescription)"
        }
    }
    
    var recoverySuggestion: String? {
        switch self {
        case .noAPIKey: "Add your API key in Settings (Cmd+,)"
        case .apiError(let code, _):
            switch code {
            case 401, 403: "Verify your API key is correct and has not expired"
            case 429: "Rate limit reached. Wait a moment and try again"
            case 500...599: "Server error. Try again in a few seconds"
            default: "Check your network connection and try again"
            }
        case .invalidResponse: "The API returned an unexpected response. Try again or check for service status"
        case .timeout: "The operation took too long. Try breaking it into smaller steps"
        case .serviceUnavailable: "Restart the service, or click Register in Settings to re-enable it"
        case .permissionDenied: "Open System Settings > Privacy & Security and grant the required permission"
        case .toolFailed: "Check the tool documentation and ensure parameters are correct"
        case .scriptError: "Check script syntax with 'swift build' in the AgentScript directory"
        case .xpcError: "Click Register in Settings to restart XPC services"
        case .mcpError: "Check MCP server status and restart if needed"
        case .accessibilityError: "Ensure Accessibility permission is granted in System Settings"
        case .networkError: "Check your internet connection and try again"
        case .fileError: "Verify the file exists and you have read/write permissions"
        case .notFound: "Verify the item exists and the path is correct"
        case .invalidInput: "Check the input format and try again"
        case .cancelled: "The operation was cancelled by the user"
        case .unknown: "Try restarting the app or report this issue"
        case .invalidURL: nil
        }
    }
    
    /// Whether this error is recoverable by retrying
    var isRecoverable: Bool {
        switch self {
        case .timeout, .networkError:
            return true
        case .serviceUnavailable, .xpcError:
            return true
        case .apiError(let code, _) where code >= 500:
            return true
        default:
            return false
        }
    }

    var isRateLimited: Bool {
        if case .apiError(429, _) = self { return true }
        return false
    }
    
    /// Create AgentError from any Error
    static func wrap(_ error: Error) -> AgentError {
        if let agentError = error as? AgentError {
            return agentError
        }
        return .unknown(error)
    }
}

/// Unified timeout for all LLM API requests (Claude, OpenAI-compatible, Ollama).
let llmAPITimeout: TimeInterval = 10800

/// Timeout for an internal tool call to start executing (seconds).
let toolStartTimeout: TimeInterval = 600

/// Timeout for an internal tool call to finish once started (seconds).
let toolFinishTimeout: TimeInterval = 43200

/// Timeout for automation calls (Accessibility, Selenium, AppleScript, JavaScript) to start (seconds).
let automationStartTimeout: TimeInterval = 9000

/// Timeout for automation calls to finish once started (seconds).
let automationFinishTimeout: TimeInterval = 18000

/// Maximum delay between automation retries (seconds).
let automationMaxDelay: TimeInterval = 5

// MARK: - String Extensions

extension String {
    func truncate(to length: Int, trailing: String = "...") -> String {
        if count > length {
            return prefix(length) + trailing
        }
        return self
    }
}

// MARK: - Error History

struct ErrorRecord: Codable, Identifiable {
    let id: UUID
    let timestamp: Date
    let message: String
    let errorType: String
    let context: String
    let stackTrace: String
    
    init(timestamp: Date = Date(), message: String, errorType: String, context: String = "", stackTrace: String = "") {
        self.id = UUID()
        self.timestamp = timestamp
        self.message = message
        self.errorType = errorType
        self.context = context
        self.stackTrace = stackTrace
    }
}

@MainActor @Observable
final class ErrorHistory {
    static let shared = ErrorHistory()
    
    private(set) var records: [ErrorRecord] = []
    
    private var fileURL: URL {
        guard let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else { return URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("error_history.json") }
        let dir = appSupport.appendingPathComponent("Agent", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("error_history.json")
    }
    
    private init() {
        load()
    }
    
    func add(_ record: ErrorRecord) {
        records.append(record)
        save()
    }
    
    func clear() {
        records.removeAll()
        save()
    }
    
    func recentErrors(limit: Int = 50) -> [ErrorRecord] {
        Array(records.suffix(limit))
    }
    
    func errorsByType(_ type: String) -> [ErrorRecord] {
        records.filter { $0.errorType == type }
    }
    
    private func load() {
        guard FileManager.default.fileExists(atPath: fileURL.path) else { return }
        do {
            let data = try Data(contentsOf: fileURL)
            records = try JSONDecoder().decode([ErrorRecord].self, from: data)
        } catch {
            records = []
        }
    }
    
    private func save() {
        // Capture data synchronously on main actor, then write async
        let data: Data?
        do {
            data = try JSONEncoder().encode(records)
        } catch {
            data = nil
        }
        guard let data else { return }
        
        let fileURL = self.fileURL
        Task.detached(priority: .background) {
            try? data.write(to: fileURL, options: .atomic)
        }
    }
}

// MARK: - Task History

struct TaskRecord: Codable, Identifiable {
    let id: UUID
    let date: Date
    let prompt: String
    let summary: String
    let commandsRun: [String]
    /// True if this record is a condensed summary of older records
    var isSummary: Bool

    init(prompt: String, summary: String, commandsRun: [String], isSummary: Bool = false) {
        self.id = UUID()
        self.date = Date()
        self.prompt = prompt
        self.summary = summary
        self.commandsRun = commandsRun
        self.isSummary = isSummary
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UUID.self, forKey: .id)
        date = try c.decode(Date.self, forKey: .date)
        prompt = try c.decode(String.self, forKey: .prompt)
        summary = try c.decode(String.self, forKey: .summary)
        commandsRun = try c.decode([String].self, forKey: .commandsRun)
        isSummary = try c.decodeIfPresent(Bool.self, forKey: .isSummary) ?? false
    }
}

@MainActor @Observable
final class TaskHistory {
    static let shared = TaskHistory()

    private(set) var records: [TaskRecord] = []

    private var fileURL: URL {
        guard let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else { return URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("task_history.json") }
        let dir = appSupport.appendingPathComponent("Agent", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("task_history.json")
    }

    private init() {
        load()
    }

    private var isSummarizing = false

    func add(_ record: TaskRecord, maxBeforeSummary: Int = 10, apiKey: String = "", model: String = "") {
        records.append(record)
        save()
        if !isSummarizing, records.count > maxBeforeSummary {
            summarizeWithAI(apiKey: apiKey, model: model)
        }
    }

    /// Use the current LLM to summarize all records into 1 entry, offloaded async.
    private func summarizeWithAI(apiKey: String, model: String) {
        guard !apiKey.isEmpty else {
            fallbackSummarize()
            return
        }

        isSummarizing = true

        let formatter = DateFormatter()
        formatter.dateStyle = .short
        formatter.timeStyle = .short

        let firstDate = records.first.map { formatter.string(from: $0.date) } ?? ""
        let lastDate = records.last.map { formatter.string(from: $0.date) } ?? ""
        let count = records.count

        let taskDump = records.map { r in
            if r.isSummary { return "Previous summary: \(r.summary)" }
            let d = formatter.string(from: r.date)
            let cmds = r.commandsRun.prefix(3).joined(separator: "; ")
            return "[\(d)] \(r.prompt) → \(r.summary)" + (cmds.isEmpty ? "" : " (cmds: \(cmds))")
        }.joined(separator: "\n")

        let prompt = """
        Summarize these \(count) task records into a single concise paragraph. \
        Focus on what was accomplished, key patterns, and important context for future tasks. \
        Keep it under 500 words. Do not use markdown. Just plain text.\n\n\(taskDump)
        """

        let messages: [[String: Any]] = [
            ["role": "user", "content": prompt]
        ]

        let body: [String: Any] = [
            "model": model,
            "max_tokens": 1024,
            "messages": messages
        ]

        Task { @MainActor in
            do {
                let bodyData = try JSONSerialization.data(withJSONObject: body)
                let summaryText = try await Self.performSummaryRequest(bodyData: bodyData, apiKey: apiKey)

                let summaryRecord = TaskRecord(
                    prompt: "(AI Summary of \(count) tasks, \(firstDate) – \(lastDate))",
                    summary: summaryText.trimmingCharacters(in: .whitespacesAndNewlines),
                    commandsRun: [],
                    isSummary: true
                )
                self.records = [summaryRecord]
                self.save()
                self.isSummarizing = false
            } catch {
                self.fallbackSummarize()
            }
        }
    }

    /// Fallback if API call fails — just concatenate into 1 record.
    private func fallbackSummarize() {
        let formatter = DateFormatter()
        formatter.dateStyle = .short
        formatter.timeStyle = .short

        let firstDate = records.first.map { formatter.string(from: $0.date) } ?? ""
        let lastDate = records.last.map { formatter.string(from: $0.date) } ?? ""

        let condensed = records.map { r in
            r.isSummary ? r.summary : "- \(r.prompt): \(r.summary)"
        }.joined(separator: "\n")

        let summaryRecord = TaskRecord(
            prompt: "(Summary of \(records.count) tasks, \(firstDate) – \(lastDate))",
            summary: condensed,
            commandsRun: [],
            isSummary: true
        )
        records = [summaryRecord]
        save()
        isSummarizing = false
    }

    /// Network request off main actor
    nonisolated private static func performSummaryRequest(bodyData: Data, apiKey: String) async throws -> String {
        guard let url = URL(string: "https://api.anthropic.com/v1/messages") else { throw AgentError.invalidURL }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue(apiKey, forHTTPHeaderField: "x-api-key")
        request.setValue("2023-06-01", forHTTPHeaderField: "anthropic-version")
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.httpBody = bodyData
        request.timeoutInterval = llmAPITimeout

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200,
              let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let content = json["content"] as? [[String: Any]],
              let textBlock = content.first(where: { $0["type"] as? String == "text" }),
              let summaryText = textBlock["text"] as? String else {
            throw AgentError.invalidResponse
        }
        return summaryText
    }

    func clearAll() {
        records.removeAll()
        save()
    }

    /// Returns the last task as a user/assistant message pair so the LLM sees it in conversation.
    func lastTaskMessages() -> [[String: Any]] {
        guard let last = records.last else { return [] }
        let formatter = DateFormatter()
        formatter.dateStyle = .short
        formatter.timeStyle = .short
        let date = formatter.string(from: last.date)

        let recap: String
        if last.isSummary {
            recap = "Here is a summary of our previous work:\n\(last.summary)"
        } else {
            var parts = ["Previous task [\(date)]: \(last.prompt)", "Result: \(last.summary)"]
            if !last.commandsRun.isEmpty {
                parts.append("Commands run: \(last.commandsRun.prefix(5).joined(separator: "; "))")
            }
            recap = parts.joined(separator: "\n")
        }

        return [
            ["role": "user", "content": recap],
            ["role": "assistant", "content": "Understood, I have context from our previous work. What would you like to do next?"]
        ]
    }

    /// Build a context string of recent history for the system prompt
    func contextForPrompt(maxRecent: Int = 20) -> String {
        guard !records.isEmpty else { return "" }
        let recent = records.suffix(maxRecent)
        let formatter = DateFormatter()
        formatter.dateStyle = .short
        formatter.timeStyle = .short

        var lines: [String] = ["\n\nPrevious task history (most recent last):"]
        for record in recent {
            let date = formatter.string(from: record.date)
            if record.isSummary {
                lines.append("[\(date)] Earlier work summary:")
                lines.append("  \(record.summary)")
            } else {
                lines.append("[\(date)] Task: \(record.prompt)")
                lines.append("  Result: \(record.summary)")
                if !record.commandsRun.isEmpty {
                    let cmds = record.commandsRun.prefix(5).joined(separator: "; ")
                    lines.append("  Commands: \(cmds)")
                }
            }
        }
        return lines.joined(separator: "\n")
    }

    private func load() {
        guard FileManager.default.fileExists(atPath: fileURL.path) else { return }
        do {
            let data = try Data(contentsOf: fileURL)
            records = try JSONDecoder().decode([TaskRecord].self, from: data)
        } catch {
            records = []
        }
    }

    private func save() {
        // Capture data synchronously on main actor, then write async
        let data: Data?
        do {
            data = try JSONEncoder().encode(records)
        } catch {
            data = nil
        }
        guard let data else { return }
        
        let fileURL = self.fileURL
        Task.detached(priority: .background) {
            try? data.write(to: fileURL, options: .atomic)
        }
    }
}
