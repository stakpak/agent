import Foundation
import FoundationModels
import SwiftUI


/// Apple Intelligence mediator — middleman that rephrases/annotates requests for the LLM.
/// Never refuses/blocks. [AI→User]=annotation, [AI→LLM]=rephrased context, [AI→Both]=shared info.
@MainActor
final class AppleIntelligenceMediator: ObservableObject {
    static let shared = AppleIntelligenceMediator()

    /// Timeout for Apple Intelligence to start responding (seconds).
    private static let startTimeout: TimeInterval = 1
    /// Timeout for Apple Intelligence to finish once started (seconds).
    private static let finishTimeout: TimeInterval = 2

    /// Maximum context window size (approximate token limit for context)
    private static let maxContextTokens: Int = 4096

    /// Whether Apple Intelligence mediation is enabled
    @Published var isEnabled: Bool = UserDefaults.standard.bool(forKey: "appleIntelligenceMediatorEnabled") {
        didSet {
            UserDefaults.standard.set(isEnabled, forKey: "appleIntelligenceMediatorEnabled")
        }
    }

    /// Whether to show Apple Intelligence annotations to the user
    @Published var showAnnotationsToUser: Bool = UserDefaults.standard.bool(forKey: "appleIntelligenceShowToUser") {
        didSet {
            UserDefaults.standard.set(showAnnotationsToUser, forKey: "appleIntelligenceShowToUser")
        }
    }

    /// On-device summarization for context compaction (Tier 1). Off → falls through to Tier 2 aggressive pruning.
    @Published var tokenCompressionEnabled: Bool = UserDefaults.standard.object(forKey: "appleIntelligenceTokenCompression") as? Bool ?? true {
        didSet {
            UserDefaults.standard.set(tokenCompressionEnabled, forKey: "appleIntelligenceTokenCompression")
        }
    }

    /// Parses accessibility commands locally; UI automation requests dispatched on-device — zero cloud tokens.
    @Published var accessibilityIntentEnabled: Bool = UserDefaults.standard.object(forKey: "appleIntelligenceAccessibilityIntent") as? Bool ?? true {
        didSet {
            UserDefaults.standard.set(accessibilityIntentEnabled, forKey: "appleIntelligenceAccessibilityIntent")
        }
    }

    /// Brain icon color for the toolbar
    var brainIconColor: Color {
        if !isEnabled { return .red }
        // Yellow when ANY sub-feature is disabled — at-a-glance partial-config signal.
        if !showAnnotationsToUser || !tokenCompressionEnabled || !accessibilityIntentEnabled { return .yellow }
        return .green
    }

    // MARK: - Conversation Context (for Apple AI session)

    /// Known agent script names (lowercase) for direct command matching.
    /// Updated by the ViewModel when scripts are loaded/changed.
    static var knownAgentNames: Set<String> = []

    /// Last task prompt from the user
    private var lastUserPrompt: String?

    /// Last Apple AI annotation (for context continuity)
    private var lastAppleAIMessage: String?

    /// Last LLM response summary (truncated to fit context window)
    private var lastLLMResponse: String?

    /// Running summary of conversation for context
    private var conversationSummary: String?

    private var session: LanguageModelSession?

    /// Represents an Apple Intelligence annotation
    struct Annotation {
        enum Target {
            case user // Only show to user
            case llm // Inject into LLM context
            case both // Show to both
        }

        let target: Target
        let content: String
        let timestamp: Date

        /// Formatted output with appropriate flow tag
        var formatted: String {
            let arrow: String
            switch target {
            case .user: arrow = "🍎 👉 👤"
            case .llm: arrow = "🍎 👉 🧠"
            case .both: arrow = "🍎 👉 👤🧠"
            }
            return "\(arrow) \(content)"
        }
    }

    private init() {
        // Initialize with defaults
        if !UserDefaults.standard.bool(forKey: "appleIntelligenceMediatorConfigured") {
            showAnnotationsToUser = true
            UserDefaults.standard.set(true, forKey: "appleIntelligenceMediatorConfigured")
        }
    }

    /// Check if Apple Intelligence is available
    static var isAvailable: Bool {
        switch SystemLanguageModel.default.availability {
        case .available: return true
        case .unavailable: return false
        }
    }

    static var unavailabilityReason: String {
        switch SystemLanguageModel.default.availability {
        case .available: return ""
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

    // MARK: - Context Management

    /// Update the conversation context after each exchange
    func updateContext(userPrompt: String?, appleAIMessage: String?, llmResponse: String?) {
        if let prompt = userPrompt {
            // Keep prompts within context limits
            lastUserPrompt = String(prompt.prefix(500))
        }
        if let aiMsg = appleAIMessage {
            // Keep AI messages brief
            lastAppleAIMessage = String(aiMsg.prefix(200))
        }
        if let llm = llmResponse {
            // Truncate LLM response to avoid blowing context window
            lastLLMResponse = String(llm.prefix(1000))
        }
    }

    /// Build context string for the session instructions (fits within ~4096 token window)
    /// System instructions for Apple AI. Only sees the current user prompt — no history.
    private func buildContextInstructions() -> String {
        return """
        You rephrase user requests to help an AI assistant understand them better. You are a middleman, NOT a gatekeeper.

        Rules:
        - NEVER refuse, block, or judge a request. You do NOT decide what is allowed.
        - Reply with 1 sentence only. Never multiple lines.
        - Rephrase or add brief context to clarify the user's intent for the AI.
        - If the request is already clear, reply with nothing.
        - Never include tags, labels, or prefixes like [AI], LLM:, User:, CLEAR, etc.
        - NEVER change agent names, tool names, script names, or identifiers.
        - Just give the plain helpful text. Nothing else.
        """
    }

    private func ensureSession() -> LanguageModelSession {
        // Always create a fresh session with current context to avoid stale/stuck state
        let s = LanguageModelSession(
            model: .default,
            instructions: Instructions(buildContextInstructions())
        )
        session = s
        return s
    }

    /// Wraps a session.respond call with timeout.
    /// Returns nil on timeout so the request goes straight to the LLM.
    private func respondWithTimeout(_ session: LanguageModelSession, prompt: String, label: String) async -> String? {

        let startLimit = Self.startTimeout
        let finishLimit = Self.finishTimeout

        do {
            let content: String = try await withThrowingTaskGroup(of: String.self) { group in
                group.addTask {
                    // Start timeout — must begin responding within startTimeout
                    let startDeadline = CFAbsoluteTimeGetCurrent()
                    let response = try await session.respond(to: prompt)
                    let startElapsed = CFAbsoluteTimeGetCurrent() - startDeadline
                    if startElapsed > startLimit {
                        throw CancellationError()
                    }
                    return response.content
                }
                group.addTask {
                    // Finish timeout — entire call must complete within finishTimeout
                    try await Task.sleep(for: .seconds(finishLimit))
                    throw CancellationError()
                }
                guard let result = try await group.next() else {
                    throw CancellationError()
                }
                group.cancelAll()
                return result
            }

            return content
        } catch {
            self.session = nil
            return nil
        }
    }

    /// Generate summary annotation after LLM task completion; updates context. Paraphrases when no tool calls.
    func summarizeCompletion(summary: String, commandsRun: [String]) async -> Annotation? {
        guard isEnabled && showAnnotationsToUser && Self.isAvailable else { return nil }

        // Store a truncated version for context (keep within token limits)
        let summaryForContext: String
        if summary.count > 500 {
            summaryForContext = String(summary.prefix(200)) + "..."
        } else {
            summaryForContext = summary
        }
        lastLLMResponse = summaryForContext

        let session = ensureSession()

        // Different behavior based on whether tools were used
        let prompt: String
        if commandsRun.isEmpty {
            prompt = """
            The AI responded: "\(String(summary.prefix(800)))"

            Summarize the key point in 1 sentence. If trivial, reply with nothing.
            """
        } else {
            prompt = """
            Task completed. Summary: "\(summary)"
            Commands: \(commandsRun.joined(separator: ", "))

            Summarize the outcome in 1 sentence. If trivial, reply with nothing.
            """
        }

        do {
            guard let content = await respondWithTimeout(session, prompt: prompt, label: "summarize") else {
                return nil
            }
            let trimmed = sanitize(content)
            if trimmed.isEmpty {
                return nil
            }
            return Annotation(target: .both, content: trimmed, timestamp: Date())
        }
    }

    /// Explain an error that occurred during tool execution
    func explainError(toolName: String, error: String) async -> Annotation? {
        guard isEnabled && showAnnotationsToUser && Self.isAvailable else { return nil }

        let session = ensureSession()
        let prompt = """
        Error in \(toolName): \(error.prefix(300))

        Explain in 1 sentence and suggest a fix.
        """

        guard let content = await respondWithTimeout(session, prompt: prompt, label: "explainError") else {
            return nil
        }
        let trimmed = sanitize(content)
        if trimmed.isEmpty {
            return nil
        }
        return Annotation(target: .user, content: trimmed, timestamp: Date())
    }

    /// Provide suggestions for what the user might want to do next
    func suggestNextSteps(context: String) async -> Annotation? {
        guard isEnabled && showAnnotationsToUser && Self.isAvailable else { return nil }

        let session = ensureSession()
        let prompt = """
        Context: \(context.prefix(500))

        Suggest the next step in 1 sentence. If none obvious, reply with nothing.
        """

        guard let content = await respondWithTimeout(session, prompt: prompt, label: "nextSteps") else {
            return nil
        }
        let trimmed = sanitize(content)
        if trimmed.isEmpty {
            return nil
        }
        return Annotation(target: .user, content: trimmed, timestamp: Date())
    }

    // MARK: - Conversation Triage

    /// Triage result: Apple AI answers, a direct command is executed, or pass through to the LLM.
    enum TriageResult {
        case answered(String) // Apple AI handled it — show this text and skip LLM
        case directCommand(DirectCommand) // Matched command — execute locally, skip LLM
        case accessibilityHandled(String) // Apple AI ran the accessibility tool — show its summary, skip LLM
        case passThrough // Needs tools/LLM — proceed normally
    }

    /// Parsed direct command with optional argument.
    struct DirectCommand {
        let name: String
        let argument: String
    }

    /// Known direct commands that can be executed without the LLM.
    /// Matches patterns like "list agents", "run AgentName", "read AgentName", "delete AgentName".
    static func matchDirectCommand(_ message: String) -> DirectCommand? {
        let trimmed = message.trimmingCharacters(in: .whitespacesAndNewlines)
        let lower = trimmed.lowercased()

        // list agents
        if lower == "list agents" || lower == "list agent" || lower == "list scripts"
            || lower == "show agents" || lower == "show scripts"
        {
            return DirectCommand(name: "list_agents", argument: "")
        }

        // "read X", "read agent X", "show agent X" — safe, no args needed
        let readPatterns = ["read agent ", "read script ", "show agent "]
        for prefix in readPatterns {
            if lower.hasPrefix(prefix) {
                let arg = String(trimmed.dropFirst(prefix.count)).trimmingCharacters(in: .whitespaces)
                if !arg.isEmpty { return DirectCommand(name: "read_agent", argument: arg) }
            }
        }

        // "delete agent X", "remove agent X" — safe, no args needed
        let deletePatterns = ["delete agent ", "remove agent ", "delete script ", "remove script "]
        for prefix in deletePatterns {
            if lower.hasPrefix(prefix) {
                let arg = String(trimmed.dropFirst(prefix.count)).trimmingCharacters(in: .whitespaces)
                if !arg.isEmpty { return DirectCommand(name: "delete_agent", argument: arg) }
            }
        }

        // Google search — extract query from many phrasings
        if let query = extractGoogleQuery(lower, original: trimmed) {
            return DirectCommand(name: "google_search", argument: query)
        }

        // "run agent X" or "agent run X" — direct agent execution
        if lower.hasPrefix("run agent ") {
            let arg = String(trimmed.dropFirst("run agent ".count)).trimmingCharacters(in: .whitespaces)
            if !arg.isEmpty { return DirectCommand(name: "run_agent", argument: arg) }
        }
        if lower.hasPrefix("agent run ") {
            let arg = String(trimmed.dropFirst("agent run ".count)).trimmingCharacters(in: .whitespaces)
            if !arg.isEmpty { return DirectCommand(name: "run_agent", argument: arg) }
        }

        return nil
    }

    /// Extract a Google search query from many phrasings.
    /// Returns the query string or nil if no match.
    private static func extractGoogleQuery(_ lower: String, original: String) -> String? {
        // Prefix patterns — longest first so "do a google search for" matches before "google search"
        let prefixPatterns = [
            "do a google web search in safari for ",
            "do a google web search for ",
            "do a google search for ",
            "do a google search ",
            "go a google search for ",
            "google web search in safari for ",
            "google web search for ",
            "google web search ",
            "google search for ",
            "google search ",
            "search google for ",
            "search the web for ",
            "search web for ",
            "web search for ",
            "google for ",
        ]
        for prefix in prefixPatterns {
            if lower.hasPrefix(prefix) {
                var arg = String(original.dropFirst(prefix.count)).trimmingCharacters(in: .whitespaces)
                // Strip trailing noise
                let suffixes = [
                    " using google search",
                    " using google.com",
                    " using google",
                    " in safari",
                    " on google",
                    " on google.com",
                    " with google",
                    " with safari"
                ]
                for suffix in suffixes {
                    if arg.lowercased().hasSuffix(suffix) {
                        arg = String(arg.dropLast(suffix.count)).trimmingCharacters(in: .whitespaces)
                    }
                }
                // Strip surrounding quotes
                if (arg.hasPrefix("\"") && arg.hasSuffix("\"")) || (arg.hasPrefix("'") && arg.hasSuffix("'")) {
                    arg = String(arg.dropFirst().dropLast())
                }
                if !arg.isEmpty { return arg }
            }
        }
        // Keyword fallback: contains "google" somewhere — extract "for X" pattern
        if lower.contains("google") {
            // Look for "for <query>" pattern
            if let forRange = lower.range(of: " for ") {
                let afterFor = String(original[forRange.upperBound...]).trimmingCharacters(in: .whitespaces)
                // Strip trailing noise
                var query = afterFor
                let suffixes = [
                    " using google search",
                    " using google.com",
                    " using google",
                    " in safari",
                    " on google",
                    " on google.com",
                    " with google",
                    " with safari"
                ]
                for suffix in suffixes {
                    if query.lowercased().hasSuffix(suffix) {
                        query = String(query.dropLast(suffix.count)).trimmingCharacters(in: .whitespaces)
                    }
                }
                query = query.replacingOccurrences(of: "\"", with: "").replacingOccurrences(of: "'", with: "")
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                if !query.isEmpty { return query }
            }
        }
        return nil
    }

    /// Resolve common site names to their URLs (e.g. "linkedin" → "linkedin.com")
    private static let siteNames: [String: String] = [
        "linkedin": "linkedin.com", "linked in": "linkedin.com",
        "facebook": "facebook.com", "face book": "facebook.com",
        "twitter": "twitter.com", "x": "x.com",
        "instagram": "instagram.com", "insta": "instagram.com",
        "youtube": "youtube.com", "yt": "youtube.com",
        "reddit": "reddit.com",
        "github": "github.com",
        "gmail": "gmail.com", "google mail": "gmail.com",
        "google": "google.com",
        "amazon": "amazon.com",
        "ebay": "ebay.com",
        "netflix": "netflix.com",
        "spotify": "spotify.com",
        "pinterest": "pinterest.com",
        "tiktok": "tiktok.com", "tik tok": "tiktok.com",
        "wikipedia": "wikipedia.org", "wiki": "wikipedia.org",
        "stackoverflow": "stackoverflow.com", "stack overflow": "stackoverflow.com",
        "apple": "apple.com",
        "microsoft": "microsoft.com",
        "slack": "slack.com",
        "discord": "discord.com",
        "twitch": "twitch.tv",
        "hacker news": "news.ycombinator.com", "hackernews": "news.ycombinator.com", "hn": "news.ycombinator.com",
    ]

    private static func resolveSiteName(_ token: String) -> String {
        let lower = token.lowercased()
        if let url = siteNames[lower] { return url }
        return token
    }

    /// Local pattern check for purely conversational messages. Defaults to passThrough — when in doubt, let the LLM handle it.
    private static func isConversationalPrompt(_ message: String) -> Bool {
        let lower = message.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
        // Must be short — long prompts are almost always tasks
        guard lower.count < 80 else { return false }
        // Known social patterns
        let greetings = [
            "hello",
            "hi",
            "hey",
            "howdy",
            "hola",
            "yo",
            "sup",
            "good morning",
            "good afternoon",
            "good evening",
            "good night"
        ]
        let thanks = ["thanks", "thank you", "thx", "ty", "appreciated", "cheers"]
        let farewells = ["bye", "goodbye", "see you", "later", "goodnight", "cya"]
        let social = [
            "how are you",
            "what are you",
            "who are you",
            "what can you do",
            "how's it going",
            "what's up",
            "whats up",
            "tell me about yourself",
            "nice to meet you",
            "i'm doing",
            "i am doing",
            "doing well",
            "doing good",
            "not bad",
            "i'm fine",
            "i am fine"
        ]
        // Check exact match or starts-with for greetings (e.g. "hi agent", "hello there")
        for g in greetings {
            if lower == g || lower.hasPrefix(g + " ") { return true }
        }
        for t in thanks {
            if lower == t || lower.hasPrefix(t + " ") { return true }
        }
        for f in farewells {
            if lower == f || lower.hasPrefix(f + " ") { return true }
        }
        for s in social {
            if lower.contains(s) { return true }
        }
        return false
    }

    // MARK: - Accessibility Intent

    /// Cheap pre-filter: does this prompt look like a UI automation request? False negatives fall through to cloud LLM (no harm).
    static func looksLikeAccessibilityRequest(_ message: String) -> Bool {
        let lower = message.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
        // Long prompts rarely single-shot UI commands; typical AX requests are short imperatives.
        guard lower.count > 3, lower.count < 240 else { return false }
        // Each verb has a trailing space so we don't false-match prefixes
        // (e.g. "take" matches "take a photo" but not "taken").
        let verbs = [
            // Original AX-named verbs
            "click ", "tap ", "press ", "type ", "select ", "scroll ",
            "open ", "find ", "show me ", "hide ", "activate ", "minimize ",
            "close ", "switch to ", "focus ", "save ", "quit ",
            // Mac-natural verbs people actually say
            "take ", "launch ", "start ", "stop ", "record ", "play ",
            "pause ", "send ", "visit ", "go to ", "navigate ", "search ",
            "check ", "toggle ", "enable ", "disable ", "choose ", "pick ",
            "use ", "using ", "run ", "drag ", "jump ", "delete ", "edit "
        ]
        return verbs.contains { lower.contains($0) }
    }

    /// Run Apple AI as tool-calling agent with accessibility tool. Returns final text on success, or nil if tool wasn't called/failed/timed out.
    func runAccessibilityAgent(_ message: String, dispatch: @escaping @Sendable (AccessibilityArgs) async -> String) async -> String? {
        guard isEnabled && accessibilityIntentEnabled && Self.isAvailable else { return nil }

        // Thread-safe boxes to track tool-call/error state across the Sendable closure.
        final class CallTracker: @unchecked Sendable {
            private let lock = NSLock()
            private var _called = false
            private var _failed = false
            var called: Bool { lock.lock(); defer { lock.unlock() }; return _called }
            var failed: Bool { lock.lock(); defer { lock.unlock() }; return _failed }
            func markCalled() { lock.lock(); _called = true; lock.unlock() }
            func markFailed() { lock.lock(); _failed = true; lock.unlock() }
        }
        let tracker = CallTracker()

        let tool = AccessibilityAppleTool { args in
            tracker.markCalled()
            let result = await dispatch(args)
            let lower = result.lowercased()
            if lower.contains("error") || lower.contains("not found") || lower.contains("\"success\":false") {
                tracker.markFailed()
            }
            return result
        }

        // Apple AI's ~4096-token window requires terse instructions. Known apps from SDEFService; unknowns fall back to NSRunningApplications scan.
        let knownApps = SDEFService.shared.availableAppNames().joined(separator: ", ")
        let instructions = Instructions("""
        You automate Mac UI via the accessibility tool. Use the EXACT natural app \
        name the user said in their request — bundle ID resolution happens \
        automatically. NEVER substitute a different app name than what the user asked \
        for. After tool calls succeed, reply with 1 sentence.

        Roles: AXButton, AXTextField, AXLink, AXMenuItem, AXCheckBox, AXImage, AXWebArea

        Apps Agent knows about (use these names exactly when applicable): \(knownApps)

        For apps not in that list, use whatever name the user gave you verbatim.

        Multi-step example — when the user says "open Calculator and click 5":
          1. open_app(app="Calculator")
          2. click_element(role="AXButton", title="5", app="Calculator")
          Reply: "Opened Calculator and clicked 5."

        The example app above is illustrative ONLY. Use the SAME app the user named \
        in the actual request — do NOT pattern-match to apps from this prompt.

        If the request isn't Mac UI automation, reply briefly without calling the tool.
        """)

        let session = LanguageModelSession(model: .default, tools: [tool], instructions: instructions)

        // Wrap respond(to:) in task-group timeout. The agent loop runs inside respond(to:), so we need a generous timeout for multiple tool calls.
        let timeoutSeconds: TimeInterval = 15
        do {
            let content: String = try await withThrowingTaskGroup(of: String.self) { group in
                group.addTask {
                    let response = try await session.respond(to: message)
                    return response.content
                }
                group.addTask {
                    try await Task.sleep(for: .seconds(timeoutSeconds))
                    throw CancellationError()
                }
                guard let result = try await group.next() else { throw CancellationError() }
                group.cancelAll()
                return result
            }

            // If Apple AI didn't actually call the tool, it just chatted at the user — that means it didn't recognize
            // the request as UI automation. Fall through to the cloud LLM.
            guard tracker.called else { return nil }
            // If any tool call failed, fall through to the cloud LLM with
            // the failure context (caller handles the partial-success case).
            if tracker.failed { return nil }
            return sanitize(content)
        } catch {
            return nil
        }
    }

    /// / Triage a prompt: direct commands → accessibility agent (Apple AI) → conversational patterns. / Falls back to
    /// .passThrough for anything needing the cloud LLM. / `axDispatch` routes AccessibilityArgs to AgentViewModel.executeNativeTool.
    func triagePrompt(_ message: String, axDispatch: @escaping @Sendable (AccessibilityArgs) async -> String) async -> TriageResult {
        // Direct commands execute without any AI — works even if Apple AI is off
        if let cmd = Self.matchDirectCommand(message) {
            return .directCommand(cmd) // Caller executes the tool
        }
        guard isEnabled && Self.isAvailable else { return .passThrough }
        // Accessibility agent — let Apple AI try to handle UI automation requests locally with full tool-calling
        // support. Pre-filter on action verbs so we don't spend an AI call on every user message.
        if accessibilityIntentEnabled && Self.looksLikeAccessibilityRequest(message) {
            if let result = await runAccessibilityAgent(message, dispatch: axDispatch) {
                return .accessibilityHandled(result)
            }
        }
        // Local classification — no AI needed
        guard Self.isConversationalPrompt(message) else { return .passThrough }
        // Ask Apple AI to answer (not classify)
        let session = ensureSession()
        let prompt = """
        You are Agent, a friendly macOS assistant. Reply to the user in 1-2 sentences. Be warm and concise.

        User: "\(message)"
        """
        guard let content = await respondWithTimeout(session, prompt: prompt, label: "triage") else {
            return .passThrough
        }
        let trimmed = sanitize(content)
        let upper = trimmed.uppercased()
        // If Apple AI refused or gave a useless response, pass through to LLM
        if trimmed.isEmpty || upper.contains("I CAN'T") || upper.contains("I CANNOT")
            || upper.contains("I'M UNABLE") || upper.contains("NOT ABLE TO")
        {
            return .passThrough
        }
        lastAppleAIMessage = String(trimmed.prefix(200))
        return .answered(trimmed)
    }

    /// Clear the session and conversation context to start fresh (call when switching contexts or starting a new conversation)
    func resetSession() {
        session = nil
        lastUserPrompt = nil
        lastAppleAIMessage = nil
        lastLLMResponse = nil
        conversationSummary = nil
    }

    /// Clear all conversation context (call when user clears the chat)
    func clearContext() {
        lastUserPrompt = nil
        lastAppleAIMessage = nil
        lastLLMResponse = nil
        conversationSummary = nil
        session = nil
    }

    /// Strip tags, labels, and junk that Apple AI sometimes echoes back
    private func sanitize(_ raw: String) -> String {
        var text = raw.trimmingCharacters(in: .whitespacesAndNewlines)

        // Remove any [AI ...] tags, [AI → User], LLM:, CLEAR, etc.
        let patterns = [
            #"\[AI\s*→?\s*(?:User|LLM|Both)\]"#,
            #"\[AI\s+Context\]"#,
            #"(?i)^CLEAR$"#,
            #"(?i)^LLM:\s*$"#,
        ]
        for pattern in patterns {
            if let regex = try? NSRegularExpression(pattern: pattern, options: .anchorsMatchLines) {
                text = regex.stringByReplacingMatches(in: text, range: NSRange(text.startIndex..., in: text), withTemplate: "")
            }
        }
        // Collapse multiple newlines/whitespace into single space, trim again
        text = text.components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
            .joined(separator: " ")
        return text
    }

    /// Get the current conversation context for debugging/inspection
    func getContextStatus() -> String {
        var parts: [String] = []
        if let prompt = lastUserPrompt { parts.append("Last user prompt: \(prompt.prefix(100))...") }
        if let aiMsg = lastAppleAIMessage { parts.append("Last Apple AI: \(aiMsg.prefix(100))...") }
        if let llm = lastLLMResponse { parts.append("Last LLM: \(String(llm.prefix(100)))...") }
        if let summary = conversationSummary { parts.append("Summary: \(summary)") }
        return parts.isEmpty ? "No context stored" : parts.joined(separator: "\n")
    }
}

// MARK: - Accessibility Tool for Apple Intelligence FoundationModels-native tool — framework handles
// schema/validation/agent loop. We just provide the dispatch closure routing to AgentViewModel.executeNativeTool.

/// / Generable arguments for the accessibility tool. @Generable derives / ConvertibleFromGeneratedContent +
/// GenerationSchema for Apple AI. / `app` uses natural names (e.g. "Photo Booth"); dispatch resolves via / AccessibilityService.resolveBundleId().
@Generable
struct AccessibilityArgs: Sendable {
    @Guide(description: "The accessibility action: click_element, type_into_element, scroll_to_element, open_app, or find_element")
    let action: String

    @Guide(description: "AX role like AXButton, AXTextField, AXLink. Optional for open_app.")
    let role: String?

    @Guide(description: "Element title or label to match (partial, case-insensitive). Optional for open_app.")
    let title: String?

    @Guide(description: "App name — use the EXACT natural name from the user's request " +
        "(e.g. 'Calculator', 'Safari', 'TextEdit', 'Mail'). Bundle ID resolution happens " +
        "automatically. Do NOT substitute a different app than the user named.")
    let app: String?

    @Guide(description: "Text to type — only for type_into_element.")
    let text: String?
}

/// FoundationModels Tool conformance. Dispatch closure injected by runAccessibilityAgent; no AgentViewModel dependency.
struct AccessibilityAppleTool: FoundationModels.Tool {
    typealias Output = String

    let name = "accessibility"
    let description = "Click, type, scroll, or open Mac UI elements via the macOS Accessibility API. " +
        "Every action takes role+title+app (use the natural app name the user said, verbatim), never coordinates. " +
        "For multi-step requests, call this tool multiple times in order — first open_app, then click_element."

    /// Closure that performs the accessibility action. Injected by the caller so the tool doesn't reference AgentViewModel.
    let dispatch: @Sendable (AccessibilityArgs) async -> String

    func call(arguments: AccessibilityArgs) async throws -> String {
        return await dispatch(arguments)
    }
}
