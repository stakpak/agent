import Foundation
import FoundationModels
import SwiftUI


/// Apple Intelligence mediator that rephrases user requests to help the LLM understand them better.
/// Acts as a middleman only — never refuses, blocks, or judges requests.
///
/// Flow indicators:
/// - [AI → User]: Annotation only visible to the user
/// - [AI → LLM]: Rephrased context sent to the LLM
/// - [AI → Both]: Information relevant to both parties
///
/// Context window: Maintains last task prompt, last Apple AI message, and last LLM summary
/// so Apple Intelligence has context when a new task starts.
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

    /// Whether to inject context into LLM prompts
    @Published var injectContextToLLM: Bool = UserDefaults.standard.bool(forKey: "appleIntelligenceInjectToLLM") {
        didSet {
            UserDefaults.standard.set(injectContextToLLM, forKey: "appleIntelligenceInjectToLLM")
        }
    }

    /// Whether training data capture is active (captures user prompts, Apple AI interjections, LLM responses, and task summaries for LoRA JSONL)
    @Published var trainingEnabled: Bool = UserDefaults.standard.bool(forKey: "appleIntelligenceTrainingEnabled") {
        didSet {
            UserDefaults.standard.set(trainingEnabled, forKey: "appleIntelligenceTrainingEnabled")
        }
    }

    /// Brain icon color for the toolbar
    var brainIconColor: Color {
        if !isEnabled { return .red }
        if LoRAAdapterManager.shared.isLoaded { return .blue }
        if trainingEnabled { return .orange }
        if !showAnnotationsToUser || !injectContextToLLM { return .yellow }
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
            case user      // Only show to user
            case llm       // Inject into LLM context
            case both      // Show to both
        }

        let target: Target
        let content: String
        let timestamp: Date

        /// Formatted output with appropriate flow tag
        var formatted: String {
            let arrow: String
            switch target {
            case .user: arrow = "🍎 👉 👤"
            case .llm:  arrow = "🍎 👉 🧠"
            case .both: arrow = "🍎 👉 👤🧠"
            }
            return "\(arrow) \(content)"
        }
    }

    private init() {
        // Initialize with defaults
        if !UserDefaults.standard.bool(forKey: "appleIntelligenceMediatorConfigured") {
            showAnnotationsToUser = true
            injectContextToLLM = true
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
\(trainingEnabled ? "\nTraining mode is active. Give high-quality, clear annotations for LoRA fine-tuning." : "")
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

    /// Generate context to inject into the LLM prompt based on user message
    /// Also updates conversation context for future Apple AI calls
    func contextualizeUserMessage(_ message: String) async -> Annotation? {
        guard isEnabled && injectContextToLLM && Self.isAvailable else {
            return nil
        }

        // Short, clear prompts don't need rephrasing — skip to avoid confusing the LLM
        let trimmed = message.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.count < 60 {
            return nil
        }

        let session = ensureSession()
        let prompt = """
Fix typos only in this user request. Return the corrected version. Do NOT rephrase, clarify, add instructions, or change meaning. Do NOT ask questions. Keep tool names, file names, and identifiers exactly as written. If no typos, return the original text unchanged.

User said: "\(message)"
"""

        guard let content = await respondWithTimeout(session, prompt: prompt, label: "contextualize") else {
            return nil
        }
        let result = sanitize(content)
        if result.isEmpty || result == trimmed {
            return nil
        }
        // Reject if Apple AI added too much (>50% longer = it rewrote instead of fixing typos)
        if result.count > trimmed.count * 3 / 2 {
            return nil
        }
        return Annotation(target: .llm, content: result, timestamp: Date())
    }

    /// Generate a summary annotation after the LLM completes a task
    /// Updates conversation context for future Apple AI calls
    /// When there are no tool calls (just conversation), paraphrases or summarizes the LLM's response
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
        case answered(String)           // Apple AI handled it — show this text and skip LLM
        case directCommand(DirectCommand) // Matched command — execute locally, skip LLM
        case passThrough                // Needs tools/LLM — proceed normally
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
            || lower == "show agents" || lower == "show scripts" {
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
                let suffixes = [" using google search", " using google.com", " using google", " in safari", " on google", " on google.com", " with google", " with safari"]
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
                let suffixes = [" using google search", " using google.com", " using google", " in safari", " on google", " on google.com", " with google", " with safari"]
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

    /// Local pattern check: is this message purely conversational?
    /// Matches a small set of known social patterns. Everything else passes through.
    /// Default is passThrough — when in doubt, let the LLM handle it.
    private static func isConversationalPrompt(_ message: String) -> Bool {
        let lower = message.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
        // Must be short — long prompts are almost always tasks
        guard lower.count < 80 else { return false }
        // Known social patterns
        let greetings = ["hello", "hi", "hey", "howdy", "hola", "yo", "sup",
                         "good morning", "good afternoon", "good evening", "good night"]
        let thanks = ["thanks", "thank you", "thx", "ty", "appreciated", "cheers"]
        let farewells = ["bye", "goodbye", "see you", "later", "goodnight", "cya"]
        let social = ["how are you", "what are you", "who are you", "what can you do",
                      "how's it going", "what's up", "whats up", "tell me about yourself",
                      "nice to meet you", "i'm doing", "i am doing", "doing well",
                      "doing good", "not bad", "i'm fine", "i am fine"]
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

    /// Triage a user prompt. Checks direct commands first, then social patterns.
    /// Falls back to .passThrough for anything that needs the LLM.
    func triagePrompt(_ message: String) async -> TriageResult {
        // Direct commands execute without any AI — works even if Apple AI is off
        if let cmd = Self.matchDirectCommand(message) {
            return .directCommand(cmd)  // Caller executes the tool
        }
        guard isEnabled && Self.isAvailable else { return .passThrough }
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
            || upper.contains("I'M UNABLE") || upper.contains("NOT ABLE TO") {
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
