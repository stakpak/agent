import Foundation
import FoundationModels

/// Tracks compaction state across iterations to avoid redundant or runaway compaction attempts.
struct CompactionState {
    /// Estimated tokens at which compaction should trigger (leave buffer for response).
    var compactThreshold: Int
    /// Consecutive compaction failures — stops retrying after 3.
    var consecutiveFailures: Int = 0
    /// Whether the last compaction attempt succeeded.
    var lastCompactSucceeded: Bool = true
    /// Total tokens estimated before the last compaction attempt.
    var tokensBeforeLastCompact: Int = 0

    /// Max consecutive failures before circuit breaker trips.
    static let maxFailures = 3
    /// Compact when accumulated messages exceed this token estimate.
    /// Lower = more aggressive compaction = fewer input tokens wasted.
    static let defaultThreshold = 30_000

    init(contextWindow: Int = 200_000) {
        self.compactThreshold = Self.defaultThreshold
    }

    /// True if we should attempt compaction for the given estimated token count.
    func shouldCompact(estimatedTokens: Int) -> Bool {
        guard consecutiveFailures < Self.maxFailures else { return false }
        return estimatedTokens > compactThreshold
    }

    /// Record a compaction attempt result. Returns true if compaction actually reduced tokens.
    mutating func recordAttempt(tokensBefore: Int, tokensAfter: Int) -> Bool {
        tokensBeforeLastCompact = tokensBefore
        let reduced = tokensAfter < tokensBefore
        if reduced {
            consecutiveFailures = 0
            lastCompactSucceeded = true
        } else {
            consecutiveFailures += 1
            lastCompactSucceeded = false
        }
        return reduced
    }
}

extension AgentViewModel {
    // MARK: - Message History Compression

    /// Compress old tool results — use Apple AI summary if cached, otherwise first 3 lines.
    /// Last 4 messages keep full content. Tool calls (assistant) stay intact.
    static func compressMessages(_ messages: [[String: Any]], keepRecent: Int = 4) -> [[String: Any]] {
        guard messages.count > keepRecent + 1 else { return messages }

        var result: [[String: Any]] = []
        let middleEnd = messages.count - keepRecent

        for i in 0..<middleEnd {
            var msg = messages[i]
            let role = msg["role"] as? String ?? ""

            if role == "user" {
                if var blocks = msg["content"] as? [[String: Any]] {
                    for j in 0..<blocks.count {
                        if blocks[j]["type"] as? String == "tool_result",
                           let content = blocks[j]["content"] as? String, content.count > 200
                        {
                            let key = content.hashValue
                            if let cached = _summaryCache[key] {
                                blocks[j]["content"] = cached
                            } else {
                                let preview = content.components(separatedBy: "\n").prefix(3).joined(separator: "\n")
                                blocks[j]["content"] = preview + "\n(... already processed)"
                            }
                        }
                    }
                    msg["content"] = blocks
                }
            } else if role == "assistant" {
                // Compress old assistant text (keep tool_use blocks intact)
                if var blocks = msg["content"] as? [[String: Any]] {
                    for j in 0..<blocks.count {
                        if blocks[j]["type"] as? String == "text",
                           let text = blocks[j]["text"] as? String, text.count > 150
                        {
                            blocks[j]["text"] = String(text.prefix(100)) + "..."
                        }
                    }
                    msg["content"] = blocks
                }
            }
            result.append(msg)
        }

        result.append(contentsOf: messages.suffix(keepRecent))
        return result
    }

    /// Use Apple AI to summarize long text, fall back to truncation if unavailable.
    private static func summarizeOrTruncate(_ text: String) -> String {
        let key = text.hashValue
        if let cached = _summaryCache[key] { return cached }

        // Fallback: truncate (Apple AI summary happens async via compressMessagesAsync)
        let truncated = String(text.prefix(150)) + "...(truncated \(text.count) chars)"
        _summaryCache[key] = truncated
        return truncated
    }

    /// Cache summaries so we don't re-summarize the same content.
    nonisolated(unsafe) private static var _summaryCache: [Int: String] = [:]

    /// Async version: summarize old messages using Apple AI before sending.
    /// Call this before compressMessages for best results.
    static func summarizeOldMessages(_ messages: inout [[String: Any]], keepRecent: Int = 4) async {
        guard messages.count > keepRecent + 1, FoundationModelService.isAvailable else {
            return
        }

        let middleEnd = messages.count - keepRecent
        let session = LanguageModelSession(
            model: .default,
            instructions: Instructions("Summarize in 1-2 concise sentences. Keep file paths, function names, errors, and key results.")
        )

        for i in 1..<middleEnd {
            let role = messages[i]["role"] as? String ?? ""

            if role == "user" {
                if var blocks = messages[i]["content"] as? [[String: Any]] {
                    var changed = false
                    for j in 0..<blocks.count {
                        if let content = blocks[j]["content"] as? String, content.count > 300 {
                            let key = content.hashValue
                            if _summaryCache[key] == nil {
                                let input = LogLimits.trim(content, cap: LogLimits.summaryChars)
                                if let resp = try? await session.respond(to: input) {
                                    _summaryCache[key] = "[summary] " + resp.content
                                }
                            }
                            if let cached = _summaryCache[key] {
                                blocks[j]["content"] = cached
                                changed = true
                            }
                        }
                    }
                    if changed { messages[i]["content"] = blocks }
                } else if let text = messages[i]["content"] as? String, text.count > 300 {
                    let key = text.hashValue
                    if _summaryCache[key] == nil {
                        let input = LogLimits.trim(text, cap: LogLimits.summaryChars)
                        if let resp = try? await session.respond(to: input) {
                            _summaryCache[key] = "[summary] " + resp.content
                        }
                    }
                    if let cached = _summaryCache[key] { messages[i]["content"] = cached }
                }
            }
        }
    }

    // MARK: - Tiered Compaction (token-budget-aware)

    /// Two-tier compaction: try Apple AI summarization first (fast), fall back to aggressive pruning.
    /// Returns true if tokens were meaningfully reduced.
    @MainActor
    static func tieredCompact(
        _ messages: inout [[String: Any]],
        state: inout CompactionState,
        log: ((String) -> Void)? = nil
    ) async -> Bool
    {
        let tokensBefore = await preciseTokenCount(messages: messages)
        guard state.shouldCompact(estimatedTokens: tokensBefore) else { return false }

        log?("🗜️ Compacting context (\(tokensBefore) est. tokens, threshold \(state.compactThreshold))...")

        // Microcompact: clear old tool results to "[cleared]" (keeps last 3)
        microcompact(&messages)

        // Strip images — they're huge and won't summarize well
        stripOldImages(&messages)

        // Tier 1: Apple AI summarization (fast, on-device).
        // Gated on tokenCompressionEnabled so users can opt out if it's slow on their device or if the summaries are ...
        if FoundationModelService.isAvailable && AppleIntelligenceMediator.shared.tokenCompressionEnabled {
            await summarizeOldMessages(&messages)
            let tokensAfterT1 = await preciseTokenCount(messages: messages)
            if state.recordAttempt(tokensBefore: tokensBefore, tokensAfter: tokensAfterT1) {
                log?("🗜️ Apple AI compaction: \(tokensBefore) → \(tokensAfterT1) tokens")
                if tokensAfterT1 <= state.compactThreshold { return true }
            }
        }

        // Tier 2: Aggressive prune (drops middle messages into summary)
        pruneMessages(&messages)
        let tokensAfterT2 = await preciseTokenCount(messages: messages)
        let reduced = state.recordAttempt(tokensBefore: tokensBefore, tokensAfter: tokensAfterT2)
        if reduced {
            log?("🗜️ Pruned context: \(tokensBefore) → \(tokensAfterT2) tokens")
        } else {
            log?("⚠️ Compaction had no effect (\(state.consecutiveFailures)/\(CompactionState.maxFailures) failures)")
        }
        return reduced
    }

    // MARK: - Microcompaction (clear old tool results)

    /// Clear old tool_result content to save tokens while preserving message structure.
    /// Keeps only the last `keepRecent` tool results intact; older ones replaced with "[cleared]".
    static func microcompact(_ messages: inout [[String: Any]], keepRecent: Int = 3) {
        // Find all tool_result indices
        var toolResultIndices: [(msgIdx: Int, blockIdx: Int)] = []
        for (i, msg) in messages.enumerated() {
            if let blocks = msg["content"] as? [[String: Any]] {
                for (j, block) in blocks.enumerated() {
                    if block["type"] as? String == "tool_result",
                       let content = block["content"] as? String,
                       content.count > 100
                    {
                        toolResultIndices.append((i, j))
                    }
                }
            }
        }
        // Clear all but the last keepRecent
        let clearCount = max(0, toolResultIndices.count - keepRecent)
        for k in 0..<clearCount {
            let (i, j) = toolResultIndices[k]
            if var blocks = messages[i]["content"] as? [[String: Any]] {
                blocks[j]["content"] = "[cleared]"
                messages[i]["content"] = blocks
            }
        }
    }

    // MARK: - Token Counting (precise via Apple AI on macOS 26.4+, fallback ~4 chars/token)

    /// Count tokens using Apple Intelligence's tokenCount(for:) when available (macOS 26.4+),
    /// falls back to ~4 chars per token estimate otherwise.
    @MainActor
    private static func countTokens(for text: String) async -> Int {
        if FoundationModelService.isAvailable {
            do {
                return try await SystemLanguageModel.default.tokenCount(for: text)
            } catch {
                // Fall through to estimate
            }
        }
        return max(1, text.count / 4)
    }

    /// Synchronous ~4 chars per token estimate (used when async isn't available).
    private static func estimateTokensFallback(chars: Int) -> Int {
        max(1, chars / 4)
    }

    /// Count input tokens from message array.
    static func estimateTokens(messages: [[String: Any]]) -> Int {
        var chars = 0
        for msg in messages {
            if let text = msg["content"] as? String {
                chars += text.count
            } else if let blocks = msg["content"] as? [[String: Any]] {
                for block in blocks {
                    if let text = block["text"] as? String { chars += text.count }
                    else if let text = block["content"] as? String { chars += text.count }
                }
            }
        }
        return estimateTokensFallback(chars: chars)
    }

    /// Precise async token count using Apple Intelligence when available.
    @MainActor
    static func preciseTokenCount(messages: [[String: Any]]) async -> Int {
        var allText = ""
        for msg in messages {
            if let text = msg["content"] as? String {
                allText += text
            } else if let blocks = msg["content"] as? [[String: Any]] {
                for block in blocks {
                    if let text = block["text"] as? String { allText += text }
                    else if let text = block["content"] as? String { allText += text }
                }
            }
        }
        return await countTokens(for: allText)
    }

    /// Count output tokens from response content blocks.
    static func estimateTokens(content: [[String: Any]]) -> Int {
        var chars = 0
        for block in content {
            if let text = block["text"] as? String { chars += text.count }
            if let input = block["input"] as? [String: Any],
               let data = try? JSONSerialization.data(withJSONObject: input)
            {
                chars += data.count
            }
        }
        return estimateTokensFallback(chars: chars)
    }
}
