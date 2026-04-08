import Foundation

/// Central caps + trim helper for tool-result data being packaged into LLM prompts.
///
/// Different sites use different caps on purpose — e.g. `read_file` allows more
/// characters than a per-tool-result slot because it's meant to deliver whole
/// source files in one read. The unification here is on the *trim function*
/// and *banner format*, not the cap values.
///
/// NOTE: This is for LLM-bound data. Activity-log display trimming lives in
/// `ScriptTab.trimLog(_:)` and is a separate concern — do not conflate the two.
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

    // MARK: - Shared trim helper

    /// Trim `text` to `cap` chars, appending a consistent truncation banner.
    ///
    /// If `text.count <= cap`, returns `text` unchanged. Otherwise returns
    /// `String(text.prefix(cap))` plus a trailing banner of the form:
    ///
    ///     ... [truncated — N chars total, M lines. <suffix>]
    ///
    /// `lineCount` and `suffix` are optional; omit them to get a minimal banner.
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
