import Foundation

/// Central caps + trim helper for LLM-bound tool-result data.
enum LogLimits {

    // MARK: - Named caps

    /// File content cap — covers ~95% of Swift files in one read.
    static let readFileChars = 50_000

    /// Per-tool-result cap when packaging results into a user message.
    /// The smaller cap that protects context budget.
    static let toolResultChars = 8_000

    /// Total per-message budget across all tool results.
    static let toolResultsPerMessageChars = 50_000

    /// Batch shell command aggregate output cap.
    static let batchOutputChars = 50_000

    /// `web_fetch` cleaned HTML cap.
    static let webFetchChars = 8_000

    /// Short summary cap for sub-agent snapshots and compression.
    static let summaryChars = 2_000

    /// Outbound iMessage reply cap. iMessage tolerates ~65 KB but carriers
    /// may split anything bigger than ~4 KB unpredictably.
    static let messageReplyChars = 4_000

    /// Aggregate cap for merged config text (CLAUDE.md / agent.md / @include
    /// resolution). Keeps the merged-config block from blowing up the prompt.
    static let configMergeChars = 4_000

    // MARK: - Shared trim helper

    /// Trim text to cap chars with a truncation banner if over.
    static func trim(
        _ text: String,
        cap: Int,
        lineCount: Int? = nil,
        suffix: String? = nil
    ) -> String {
        guard text.count > cap else { return text }
        var banner = "\n\n... [truncated — \(text.count) chars total"
        if let lineCount {
            banner += ", \(lineCount) lines"
        }
        banner += "."
        if let suffix, !suffix.isEmpty {
            banner += " \(suffix)"
        }
        banner += "]"
        return String(text.prefix(cap)) + banner
    }
}
