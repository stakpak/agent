import Foundation

/// Tracks per-task token budget to detect diminishing returns and prevent runaway costs.
/// Create one at task start, call `recordTurn` after each LLM response, then check `shouldStop` or `shouldNudge`.
struct TokenBudgetTracker {
    /// Max total tokens (input+output) for this task. 0 = unlimited.
    var ceiling: Int
    /// Number of LLM round-trips completed.
    private(set) var turnCount: Int = 0
    /// Output tokens produced in the most recent turn.
    private(set) var lastDeltaTokens: Int = 0
    /// Output tokens produced in the turn before that.
    private(set) var prevDeltaTokens: Int = 0
    /// Cumulative tokens used so far (input + output).
    private(set) var totalUsed: Int = 0

    init(ceiling: Int = 0) {
        self.ceiling = ceiling
    }

    /// Call after each LLM response with the tokens consumed in that turn.
    mutating func recordTurn(inputTokens: Int, outputTokens: Int) {
        turnCount += 1
        prevDeltaTokens = lastDeltaTokens
        lastDeltaTokens = outputTokens
        totalUsed += inputTokens + outputTokens
    }

    /// Fraction of budget consumed (0.0–1.0). Returns 0 if no ceiling set.
    var usedFraction: Double {
        guard ceiling > 0 else { return 0 }
        return min(1.0, Double(totalUsed) / Double(ceiling))
    }

    /// True when 90%+ of budget consumed — inject a nudge message to the LLM.
    var shouldNudge: Bool {
        ceiling > 0 && usedFraction >= 0.9 && usedFraction < 1.0
    }

    /// True when the task should auto-stop: only when budget is fully exhausted.
    /// Diminishing returns is informational only — does NOT auto-stop.
    var shouldStop: Bool {
        guard ceiling > 0 else { return false }
        return usedFraction >= 1.0
    }

    /// Diminishing returns: 5+ turns where the last two each produced < 100 output tokens.
    /// Only meaningful when a budget ceiling is set.
    var isDiminishing: Bool {
        guard ceiling > 0 else { return false }
        return turnCount >= 5 && lastDeltaTokens < 100 && prevDeltaTokens < 100
    }

    /// Human-readable budget status for logging.
    var statusDescription: String {
        guard ceiling > 0 else { return "\(totalUsed) tokens used (no budget limit)" }
        let pct = Int(usedFraction * 100)
        return "\(totalUsed)/\(ceiling) tokens (\(pct)%)"
    }
}

/// Persists daily token usage to ~/Library/Application Support/Agent/token_usage.json
@MainActor
@Observable
final class TokenUsageStore {
    static let shared = TokenUsageStore()

    struct DayRecord: Codable {
        let date: String // "2026-03-29"
        var inputTokens: Int
        var outputTokens: Int
        /// Prompt cache read tokens — what we saved by hitting the cache instead of
        /// re-sending the prompt. Persisted so the 7-day chart can plot a cache line.
        /// Optional in JSON for backward compat with older records.
        var cacheReadTokens: Int = 0
        var totalTokens: Int { inputTokens + outputTokens }

        // Manual decoding so existing token_usage.json files (without cacheReadTokens)
        // still load cleanly.
        enum CodingKeys: String, CodingKey {
            case date, inputTokens, outputTokens, cacheReadTokens
        }
        init(date: String, inputTokens: Int, outputTokens: Int, cacheReadTokens: Int = 0) {
            self.date = date
            self.inputTokens = inputTokens
            self.outputTokens = outputTokens
            self.cacheReadTokens = cacheReadTokens
        }
        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            self.date = try c.decode(String.self, forKey: .date)
            self.inputTokens = try c.decode(Int.self, forKey: .inputTokens)
            self.outputTokens = try c.decode(Int.self, forKey: .outputTokens)
            self.cacheReadTokens = try c.decodeIfPresent(Int.self, forKey: .cacheReadTokens) ?? 0
        }
    }

    private(set) var days: [DayRecord] = []
    private let fileURL: URL

    // Prompt cache metrics (per-session, not persisted)
    private(set) var sessionCacheReadTokens: Int = 0
    private(set) var sessionCacheCreationTokens: Int = 0

    private init() {
        let urls = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
        let appSupport = urls.first ?? FileManager.default.temporaryDirectory
        let dir = appSupport.appendingPathComponent("Agent")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        fileURL = dir.appendingPathComponent("token_usage.json")
        load()
    }

    /// Record token usage — adds to today's running total.
    func record(inputTokens: Int, outputTokens: Int) {
        let today = Self.dateString(Date())
        if let idx = days.firstIndex(where: { $0.date == today }) {
            days[idx].inputTokens += inputTokens
            days[idx].outputTokens += outputTokens
        } else {
            days.append(DayRecord(date: today, inputTokens: inputTokens, outputTokens: outputTokens))
        }
        // Keep last 31 days
        if days.count > 31 {
            days = Array(days.suffix(31))
        }
        save()
    }

    /// Today's totals.
    var todayInput: Int {
        let today = Self.dateString(Date())
        return days.first(where: { $0.date == today })?.inputTokens ?? 0
    }
    var todayOutput: Int {
        let today = Self.dateString(Date())
        return days.first(where: { $0.date == today })?.outputTokens ?? 0
    }
    var todayCacheRead: Int {
        let today = Self.dateString(Date())
        return days.first(where: { $0.date == today })?.cacheReadTokens ?? 0
    }

    /// Last N days of records for charting.
    func recentDays(_ count: Int = 30) -> [DayRecord] {
        Array(days.suffix(count))
    }

    // MARK: - Prompt Cache Metrics

    /// Record cache metrics from Claude API response. Updates session counters AND
    /// persists cache-read tokens onto today's DayRecord so the 7-day chart can plot it.
    func recordCacheMetrics(read: Int, creation: Int) {
        sessionCacheReadTokens += read
        sessionCacheCreationTokens += creation
        guard read > 0 else { return }
        let today = Self.dateString(Date())
        if let idx = days.firstIndex(where: { $0.date == today }) {
            days[idx].cacheReadTokens += read
        } else {
            days.append(DayRecord(date: today, inputTokens: 0, outputTokens: 0, cacheReadTokens: read))
        }
        save()
    }

    /// Cache hit rate as a percentage (0-100). Returns 0 if no cache activity.
    var cacheHitRate: Int {
        let total = sessionCacheReadTokens + sessionCacheCreationTokens
        guard total > 0 else { return 0 }
        return Int(Double(sessionCacheReadTokens) / Double(total) * 100)
    }

    /// Reset session-level cache metrics.
    func resetCacheMetrics() {
        sessionCacheReadTokens = 0
        sessionCacheCreationTokens = 0
    }

    // MARK: - Per-Model Cost Tracking

    /// Tracks token usage per model within a session.
    struct ModelUsage {
        var inputTokens: Int = 0
        var outputTokens: Int = 0
        var callCount: Int = 0
        var totalTokens: Int { inputTokens + outputTokens }
    }

    /// Per-model usage for the current session.
    private(set) var modelUsage: [String: ModelUsage] = [:]

    /// Lines of code added/removed in the current task.
    private(set) var taskLinesAdded: Int = 0
    private(set) var taskLinesRemoved: Int = 0

    /// Record usage for a specific model.
    func recordModelUsage(model: String, input: Int, output: Int) {
        var usage = modelUsage[model, default: ModelUsage()]
        usage.inputTokens += input
        usage.outputTokens += output
        usage.callCount += 1
        modelUsage[model] = usage
    }

    /// Record lines changed from a diff/edit.
    func recordLinesChanged(added: Int, removed: Int) {
        taskLinesAdded += added
        taskLinesRemoved += removed
    }

    /// Reset per-task metrics (call at task start).
    func resetTaskMetrics() {
        taskLinesAdded = 0
        taskLinesRemoved = 0
    }

    /// Reset session-level model usage.
    func resetModelUsage() {
        modelUsage.removeAll()
    }

    // MARK: - Per-Provider Cost Rates (USD per 1M tokens)

    /// Input/output cost per million tokens. Approximate — check provider pricing pages.
    static let costRates: [String: (input: Double, output: Double)] = [
        // Claude
        "claude-sonnet-4": (3.0, 15.0), "claude-opus-4": (15.0, 75.0), "claude-haiku-4": (0.80, 4.0),
        // OpenAI
        "gpt-4o": (2.50, 10.0), "gpt-4o-mini": (0.15, 0.60), "o3": (10.0, 40.0),
        // DeepSeek
        "deepseek-chat": (0.27, 1.10), "deepseek-coder": (0.27, 1.10),
        // Google
        "gemini-2.5-pro": (1.25, 10.0), "gemini-2.5-flash": (0.15, 0.60),
        // Mistral
        "mistral-large": (2.0, 6.0), "codestral": (0.30, 0.90), "devstral": (0.30, 0.90),
        // xAI
        "grok-3": (3.0, 15.0), "grok-3-mini": (0.30, 0.50),
    ]

    /// Estimate cost for a model. Returns 0 if model not in rate table (local/free).
    func estimatedCost(model: String, inputTokens: Int, outputTokens: Int) -> Double {
        // Try exact match, then prefix match
        let rates = Self.costRates[model] ?? Self.costRates.first(where: { model.hasPrefix($0.key) })?.value
        guard let r = rates else { return 0 }
        return (Double(inputTokens) / 1_000_000 * r.input) + (Double(outputTokens) / 1_000_000 * r.output)
    }

    /// Total estimated cost across all models in the session.
    var sessionEstimatedCost: Double {
        modelUsage.reduce(0) { total, entry in
            total + estimatedCost(model: entry.key, inputTokens: entry.value.inputTokens, outputTokens: entry.value.outputTokens)
        }
    }

    /// Max cost per task (USD). 0 = unlimited. Stored in UserDefaults.
    static let udMaxCostKey = "agent.maxTaskCost"
    var maxTaskCost: Double {
        get { UserDefaults.standard.double(forKey: Self.udMaxCostKey) }
        set { UserDefaults.standard.set(newValue, forKey: Self.udMaxCostKey) }
    }

    /// Check if cost exceeds the user-configured max.
    var isCostExceeded: Bool {
        guard maxTaskCost > 0 else { return false }
        return sessionEstimatedCost >= maxTaskCost
    }

    /// Summary of model usage for display.
    func modelUsageSummary() -> String {
        guard !modelUsage.isEmpty else { return "No model usage recorded." }
        return modelUsage.sorted { $0.value.totalTokens > $1.value.totalTokens }.map { model, usage in
            let cost = estimatedCost(model: model, inputTokens: usage.inputTokens, outputTokens: usage.outputTokens)
            let costStr = cost > 0 ? String(format: " ($%.4f)", cost) : ""
            return "\(model): \(usage.callCount) calls, \(usage.totalTokens) tokens\(costStr)"
        }.joined(separator: "\n")
    }

    private static func dateString(_ date: Date) -> String {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: date)
    }

    private func load() {
        guard let data = try? Data(contentsOf: fileURL),
              let decoded = try? JSONDecoder().decode([DayRecord].self, from: data) else { return }
        days = decoded
    }

    private func save() {
        guard let data = try? JSONEncoder().encode(days) else { return }
        try? data.write(to: fileURL, options: .atomic)
    }
}
