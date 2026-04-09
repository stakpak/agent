import Foundation

/// / Central caps + trim helper for LLM-bound tool-result data. / Different caps per site on purpose (read_file allows
/// more than per-tool-result slots). / Unification is on the trim function and banner format, not cap values. / NOTE: For LLM-bound data only. Activity-log display trimming is in ScriptTab.trimLog.
enum LogLimits {

    // MARK: - Named caps

    /// `read_file` tool output cap (file content sent to LLM).
    /// Covers ~95% of Swift source files in a single read.
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

    /// / Short summary/excerpt cap — used for sub-agent result snapshots and / per-message compression input. Small
    /// enough that an LLM can summarize / it cheaply, large enough to preserve meaningful context.
    static let summaryChars = 2_000

    /// Outbound iMessage reply cap. iMessage tolerates ~65 KB but carriers
    /// may split anything bigger than ~4 KB unpredictably.
    static let messageReplyChars = 4_000

    /// Aggregate cap for merged config text (CLAUDE.md / agent.md / @include
    /// resolution). Keeps the merged-config block from blowing up the prompt.
    static let configMergeChars = 4_000

    // MARK: - Shared trim helper

    /// / Trim `text` to `cap` chars with a trailing truncation banner. / If under cap, returns unchanged. Otherwise:
    /// prefix + banner like / "... [truncated — N chars total, M lines. suffix]"
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
