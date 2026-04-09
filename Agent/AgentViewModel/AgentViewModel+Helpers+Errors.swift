@preconcurrency import Foundation
import AppKit
import AgentMCP

// MARK: - SDEF / TCC Error Enrichment

extension AgentViewModel {

    /// Auto-inject SDEF dictionary into failed AppleScript results. Extracts `tell application "X"`,
    /// looks up X's SDEF, and prepends it. Skips if no tell clause, X not in catalog, or TCC error.
    static func enrichAppleScriptFailure(source: String, output: String) -> String {
        // TCC permission errors need System Settings, not a vocabulary dump.
        if let tcc = Self.detectTCCError(output) {
            Self.openTCCPaneIfNeeded(tcc)
            return Self.formatTCCError(originalOutput: output, kind: tcc)
        }
        // Match `tell application "X"` clauses and inject each SDEF.
        let pattern = #"tell\s+application\s+(?:id\s+)?"([^"]+)""#
        let appNames = Self.collectAppReferences(source: source, pattern: pattern, caseInsensitive: true)
        return Self.injectMultipleSDEFs(appNames: appNames, output: output, syntaxHint: "AppleScript")
    }

    /// Auto-inject SDEF into failed JXA results. Extracts `Application("X")` names,
    /// resolves via SDEFService, and prepends canonical terms. Skips if not found.
    static func enrichJXAFailure(source: String, output: String) -> String {
        // TCC errors take priority — the dictionary won't fix a missing permission.
        if let tcc = Self.detectTCCError(output) {
            Self.openTCCPaneIfNeeded(tcc)
            return Self.formatTCCError(originalOutput: output, kind: tcc)
        }
        // Match Application("X") references, skip currentApplication().
        let pattern = #"Application\s*\(\s*['"]([^'"]+)['"]\s*\)"#
        let appNames = Self.collectAppReferences(source: source, pattern: pattern, caseInsensitive: false)
        return Self.injectMultipleSDEFs(appNames: appNames, output: output, syntaxHint: "JXA")
    }

    /// Extract every distinct app reference matching `pattern` from `source`,
    /// preserving order of first appearance and trimming whitespace.
    private static func collectAppReferences(source: String, pattern: String, caseInsensitive: Bool) -> [String] {
        let options: NSRegularExpression.Options = caseInsensitive ? .caseInsensitive : []
        guard let regex = try? NSRegularExpression(pattern: pattern, options: options) else {
            return []
        }
        let matches = regex.matches(in: source, range: NSRange(source.startIndex..., in: source))
        var seen = Set<String>()
        var ordered: [String] = []
        for match in matches where match.numberOfRanges > 1 {
            guard let range = Range(match.range(at: 1), in: source) else { continue }
            let name = String(source[range]).trimmingCharacters(in: .whitespaces)
            if name.isEmpty { continue }
            if seen.insert(name.lowercased()).inserted {
                ordered.append(name)
            }
        }
        return ordered
    }

    /// Resolve each app name → bundle ID via SDEFService, append SDEF summaries to
    /// output. 9KB total budget split evenly; apps that don't resolve are skipped.
    private static func injectMultipleSDEFs(appNames: [String], output: String, syntaxHint: String) -> String {
        if appNames.isEmpty { return output }

        // (originalName, bundleID, summary) — pre-resolve so we budget only across apps with data.
        var resolved: [(name: String, bundleID: String, summary: String)] = []
        for name in appNames {
            guard let bundleID = SDEFService.shared.resolveBundleId(name: name) else { continue }
            let summary = SDEFService.shared.summary(for: bundleID)
            if summary.hasPrefix("No SDEF found") { continue }
            resolved.append((name, bundleID, summary))
        }
        if resolved.isEmpty { return output }

        // ~9KB total budget. Per-app cap = budget / count, min 1500 chars.
        let totalBudget = 9000
        let perAppCap = max(1500, totalBudget / resolved.count)

        var blocks: [String] = []
        for entry in resolved {
            let cappedSummary = String(entry.summary.prefix(perAppCap))
            blocks.append("""
            📖 SDEF auto-injected for "\(entry.name)" (bundle: \(entry.bundleID)):

            \(cappedSummary)
            """)
        }

        let appList = resolved.map { "\"\($0.name)\"" }.joined(separator: ", ")
        let header = resolved.count == 1
            ? "📖 \(syntaxHint) failure — SDEF auto-injected. Use ONLY documented terms in your retry; everything else will fail the same way."
            : "📖 \(syntaxHint) failure — \(resolved.count) SDEFs auto-injected for \(appList). Use ONLY documented terms from each app's dictionary; everything else will fail the same way."

        return ([output, "", header, ""] + blocks).joined(separator: "\n\n")
    }

    // MARK: - TCC error detection

    /// Which TCC permission a failed AppleScript needs. Used to skip the
    /// SDEF dump (it's noise for permission errors) and to open the right
    /// System Settings pane on the user's behalf.
    enum TCCRequirement: Sendable {
        case accessibility    // sending keystrokes / clicking via System Events
        case automation       // sending Apple Events to other apps
        case screenRecording  // capturing screen
        case fullDiskAccess   // reading ~/Library, Mail.app, etc.
        case inputMonitoring  // raw key events
    }

    /// Track which TCC panes we've already opened during this app session,
    /// so a script that fails 5 times in a row doesn't pop System Settings
    /// 5 times. The user only needs the prompt once.
    nonisolated(unsafe) private static var openedTCCPanes = Set<String>()
    private static let openedTCCPanesLock = NSLock()

    /// Inspect a failed AppleScript / JXA / osascript output for TCC error
    /// signatures. Returns the relevant TCCRequirement when one matches,
    /// nil for vocabulary or other errors.
    static func detectTCCError(_ output: String) -> TCCRequirement? {
        let lower = output.lowercased()
        // Accessibility — most common, fired by `keystroke`, `key code`, and AX click attempts via System
        // Events when Agent! isn't on the Accessibility allow list.
        if lower.contains("not allowed to send keystrokes")
            || lower.contains("not allowed assistive access")
            || lower.contains("assistive access is")
            || lower.contains("requires accessibility")
        {
            return .accessibility
        }
        // Automation — sending Apple Events to a target app the user hasn't
        // approved in System Settings → Privacy & Security → Automation.
        if lower.contains("not authorized to send apple events")
            || lower.contains("not allowed to send apple events")
            || lower.contains("not permitted to send apple events")
            || lower.contains("apple events to")
        {
            return .automation
        }
        // Screen Recording — needed for `screencapture`, AVCaptureSession,
        // and AppleScript paths that read window content.
        if lower.contains("screen recording") || lower.contains("not allowed to record") {
            return .screenRecording
        }
        // Full Disk Access — operations on protected directories like
        // ~/Library/Mail or chat.db.
        if lower.contains("operation not permitted") && lower.contains("library") {
            return .fullDiskAccess
        }
        // Input Monitoring — raw key event capture.
        if lower.contains("input monitoring") || lower.contains("listen events") {
            return .inputMonitoring
        }
        return nil
    }

    /// Format a short, LLM-targeted TCC error message without dumping a full SDEF.
    /// The original macOS error is preserved so the LLM can read the exact message.
    static func formatTCCError(originalOutput: String, kind: TCCRequirement) -> String {
        let permName: String
        let permPath: String
        let why: String
        switch kind {
        case .accessibility:
            permName = "Accessibility"
            permPath = "System Settings → Privacy & Security → Accessibility"
            why = "needed for sending keystrokes (`keystroke`, `key code`), clicking UI elements via System Events, and AX automation"
        case .automation:
            permName = "Automation"
            permPath = "System Settings → Privacy & Security → Automation → Agent!"
            why = "needed for sending Apple Events to the target app — the user must approve Agent! controlling that specific app"
        case .screenRecording:
            permName = "Screen Recording"
            permPath = "System Settings → Privacy & Security → Screen & System Audio Recording"
            why = "needed for screen capture and reading window content"
        case .fullDiskAccess:
            permName = "Full Disk Access"
            permPath = "System Settings → Privacy & Security → Full Disk Access"
            why = "needed for reading protected directories like ~/Library/Mail and the Messages chat.db"
        case .inputMonitoring:
            permName = "Input Monitoring"
            permPath = "System Settings → Privacy & Security → Input Monitoring"
            why = "needed for raw key event capture"
        }
        return """
        \(originalOutput)

        🔒 macOS TCC permission required: **\(permName)** for Agent!
        \(permPath)
        Why: \(why).

        DO NOT retry the same script — it will fail the same way until the user grants the permission. The SDEF dictionary is NOT relevant here; this is a system permission error, not a vocabulary problem. System Settings has been opened to the right pane (once per session). Tell the user what you were trying to do, ask them to enable Agent! in the \(permName) list, and call task_complete with that summary.
        """
    }

    /// Open the System Settings pane for a TCC requirement, once per kind per app session.
    static func openTCCPaneIfNeeded(_ kind: TCCRequirement) {
        let key: String
        let urlString: String
        switch kind {
        case .accessibility:
            key = "accessibility"
            urlString = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        case .automation:
            key = "automation"
            urlString = "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"
        case .screenRecording:
            key = "screencapture"
            urlString = "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
        case .fullDiskAccess:
            key = "fulldisk"
            urlString = "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"
        case .inputMonitoring:
            key = "inputmon"
            urlString = "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
        }
        // Dedupe per session.
        let shouldOpen: Bool = openedTCCPanesLock.withLock {
            openedTCCPanes.insert(key).inserted
        }
        guard shouldOpen, let url = URL(string: urlString) else { return }
        // NSWorkspace.open is main-actor-isolated.
        Task { @MainActor in
            NSWorkspace.shared.open(url)
        }
    }
}
