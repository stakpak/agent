import Foundation
import AgentAudit
import AgentTools

/// / Manages editable system prompt files stored at ~/Documents/AgentScript/system/. / On first access, copies the
/// default prompts from AgentTools to disk. / At runtime, services read the on-disk prompts (with {userName}/{userHome} substitution).
@MainActor
final class SystemPromptService {
    static let shared = SystemPromptService()

    private static let systemDir: URL = {
        let home = FileManager.default.homeDirectoryForCurrentUser
        return home.appendingPathComponent("Documents/AgentScript/system")
    }()

    /// Single system prompt file for all large-context providers.
    static let commonFileName = "system_prompt.txt"
    /// Compact prompt file for Apple AI (Foundation Models).
    static let compactFileName = "system_prompt_compact.txt"

    /// Legacy per-provider file names — kept for migration cleanup only.
    private static let legacyFileNames = [
        "claude.txt", "openai.txt", "deepseek.txt", "hugging_face.txt",
        "ollama.txt", "local_ollama.txt", "vllm.txt", "lm_studio.txt",
        "foundation_model.txt", "claude_compact.txt", "openai_compact.txt",
        "deepseek_compact.txt", "hugging_face_compact.txt", "ollama_compact.txt",
        "local_ollama_compact.txt", "vllm_compact.txt", "lm_studio_compact.txt",
        "foundation_model_compact.txt", "shared_llm.txt",
    ]

    /// For backward compatibility — maps any provider to the common file.
    static let fileNames: [APIProvider: String] = {
        var map: [APIProvider: String] = [:]
        for provider in APIProvider.allCases {
            map[provider] = commonFileName
        }
        return map
    }()

    /// Version header prefix embedded in each prompt file.
    private static let versionPrefix = "// Agent! v"
    /// Custom header prefix for user-edited prompts (never auto-overwritten).
    private static let customPrefix = "// Agent! custom v"
    /// READ ONLY header prefix for locked prompts (never auto-overwritten, even on version change).
    private static let readOnlyPrefix = "// Agent! READ ONLY v"

    /// Bump this when system prompt content changes to force re-sync of saved prompts.
    private static let promptRevision = "86"

    /// / Anti-hallucination rule appended to every system prompt (full + compact). / Triggered by an observed
    /// real-world failure: the in-app Agent produced a / confident, structured "gap analysis" of its own codebase right after the / 10-consecutive-reads guard fired, citing tools and files it had never / actually read. The lesson: when evidence runs out, models default to / confabulating polished prose rather than admitting "I don't know yet." / This rule forbids that move explicitly.
    static let antiHallucinationRules = """

    ANTI-HALLUCINATION (HIGHEST PRIORITY — overrides any other rule):
    - NEVER fabricate, guess, infer, or hallucinate. If you do not have \
    direct evidence from a tool result for a claim, you may NOT make the claim.
    - When asked to analyze, audit, compare, or summarize: report ONLY what \
    you have read directly. Cite the file path and line number for every claim \
    about the codebase. If you have not read a file, you do not know what is \
    in it — period.
    - "Probably", "I think", "based on my understanding", "typically", \
    "this kind of project usually..." are confabulation flags. If you catch \
    yourself writing them, STOP, go read the actual file, or call done() and \
    say what you don't know.
    - Producing a confident, structured, polished answer from incomplete \
    evidence is the WORST possible outcome — strictly worse than admitting \
    uncertainty. Users would much rather hear "I read 3 files and here's what \
    they say; I don't know about the other 47" than a fabricated \
    comprehensive summary.
    - When the read guard fires (🛑 INSUFFICIENT EVIDENCE), you have exactly \
    two legitimate moves: narrow to one concrete fact and look it up, OR call \
    done() and honestly report what is still unknown. You may NOT produce a \
    synthesis, gap analysis, or comparison from partial reads.
    - If a previous tool call failed or returned ambiguous output, do NOT \
    reinterpret or extrapolate. Re-run with more specific input or call done() \
    and report the ambiguity.
    - Never claim you performed an action (searched, opened, clicked, ran, \
    executed, found) unless you made a tool call AND received a tool_result \
    confirming it. If you did not call a tool, say "action not performed" \
    instead of fabricating a result.
    """

    /// / Wrap an AgentTools-provided base prompt with the anti-hallucination / rules. Used by both the on-disk
    /// default-prompt seeding and by the local / endpoint code paths in ClaudeService / OpenAICompatibleService that / bypass the on-disk path.
    static func wrapWithRules(_ base: String) -> String {
        return base + "\n" + antiHallucinationRules
    }

    /// Combined version: app version + prompt revision. Change in either triggers re-sync.
    private static let appVersion: String = {
        let bundleVersion = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0"
        return "\(bundleVersion).\(promptRevision)"
    }()

    private init() {}

    /// Ensure the system/ directory exists and default prompts are written.
    /// Replaces prompts when the app version changes (unless READ ONLY).
    func ensureDefaults() {
        let fm = FileManager.default
        try? fm.createDirectory(at: Self.systemDir, withIntermediateDirectories: true)

        // Clean up legacy per-provider files
        for legacy in Self.legacyFileNames {
            let url = Self.systemDir.appendingPathComponent(legacy)
            try? fm.removeItem(at: url)
        }

        // Write common system prompt
        writeIfNeeded(fileName: Self.commonFileName, defaultContent: Self.defaultPrompt())

        // Write compact prompt (Apple AI)
        writeIfNeeded(fileName: Self.compactFileName, defaultContent: Self.defaultCompactPrompt())
    }

    /// Write a prompt file if it doesn't exist or the version changed.
    private func writeIfNeeded(fileName: String, defaultContent: String) {
        let fm = FileManager.default
        let url = Self.systemDir.appendingPathComponent(fileName)
        let needsWrite: Bool
        if !fm.fileExists(atPath: url.path) {
            needsWrite = true
        } else if let existing = try? String(contentsOf: url, encoding: .utf8),
                  let firstLine = existing.components(separatedBy: "\n").first
        {
            if firstLine.hasPrefix(Self.readOnlyPrefix) {
                needsWrite = false
            } else if firstLine.hasPrefix(Self.customPrefix) {
                let fileVersion = String(firstLine.dropFirst(Self.customPrefix.count))
                needsWrite = fileVersion != Self.appVersion
            } else if firstLine.hasPrefix(Self.versionPrefix) {
                let fileVersion = String(firstLine.dropFirst(Self.versionPrefix.count))
                needsWrite = fileVersion != Self.appVersion
            } else {
                needsWrite = true
            }
        } else {
            needsWrite = true
        }

        if needsWrite {
            let versioned = Self.versionPrefix + Self.appVersion + "\n" + defaultContent
            do {
                try versioned.write(to: url, atomically: true, encoding: .utf8)
            } catch {
                AuditLog.log(.api, "[SystemPrompt] Failed to write \(url.lastPathComponent): \(error)")
            }
        }
    }

    /// Read the on-disk prompt, substituting {userName}, {userHome}, and {projectFolder}.
    /// Strips the version comment line before returning.
    func prompt(
        for provider: APIProvider,
        userName: String,
        userHome: String,
        projectFolder: String = "",
        style: PromptStyle = .full
    ) -> String
    {
        ensureDefaults()
        let fileName: String
        switch style {
        case .full:
            fileName = Self.commonFileName
        case .compact:
            fileName = Self.compactFileName
        }
        let url = Self.systemDir.appendingPathComponent(fileName)
        guard let template = try? String(contentsOf: url, encoding: .utf8) else {
            return style == .compact ? Self.defaultCompactPrompt() : Self.defaultPrompt()
        }
        let content = Self.stripVersionLine(template)
        let folder = projectFolder.isEmpty ? userHome : projectFolder
        // Live shell name from the user's toggle (zsh/bash) — re-reads on every prompt fetch so flipping the toggle in
        // Options updates the LLM context on the next iteration without rewriting the template file.
        let shellName = (AppConstants.shellPath as NSString).lastPathComponent
        return content
            .replacingOccurrences(of: "{userName}", with: userName)
            .replacingOccurrences(of: "{userHome}", with: userHome)
            .replacingOccurrences(of: "{projectFolder}", with: folder)
            .replacingOccurrences(of: "{shell}", with: shellName)
    }

    /// Remove the version/custom/readonly comment line from prompt content.
    private static func stripVersionLine(_ text: String) -> String {
        if text.hasPrefix(readOnlyPrefix) || text.hasPrefix(customPrefix) || text.hasPrefix(versionPrefix) {
            let lines = text.components(separatedBy: "\n")
            return lines.dropFirst().joined(separator: "\n")
        }
        return text
    }

    /// Read the raw template (with placeholders) for editing. Strips version line.
    func rawTemplate(compact: Bool = false) -> String {
        ensureDefaults()
        let fileName = compact ? Self.compactFileName : Self.commonFileName
        let url = Self.systemDir.appendingPathComponent(fileName)
        let fallback = compact ? Self.defaultCompactPrompt() : Self.defaultPrompt()
        let raw = (try? String(contentsOf: url, encoding: .utf8)) ?? fallback
        return Self.stripVersionLine(raw)
    }

    /// Legacy overload for backward compatibility.
    func rawTemplate(for provider: APIProvider) -> String {
        return rawTemplate(compact: provider == .foundationModel)
    }

    /// Save an edited template back to disk (prepends custom header to prevent auto-overwrite).
    func saveTemplate(_ content: String, compact: Bool = false) {
        let fileName = compact ? Self.compactFileName : Self.commonFileName
        let url = Self.systemDir.appendingPathComponent(fileName)
        let stripped = Self.stripVersionLine(content)
        let trimmed = content.trimmingCharacters(in: .whitespacesAndNewlines)
        let isReadOnly = trimmed.hasPrefix("READ ONLY") || trimmed.hasPrefix("// READ ONLY")
        let header = isReadOnly ? Self.readOnlyPrefix : Self.customPrefix
        let versioned = header + Self.appVersion + "\n" + stripped
        do {
            try versioned.write(to: url, atomically: true, encoding: .utf8)
        } catch {
            AuditLog.log(.api, "[SystemPrompt] Failed to save template \(fileName): \(error)")
        }
    }

    /// Legacy overload for backward compatibility.
    func saveTemplate(_ content: String, for provider: APIProvider) {
        saveTemplate(content, compact: provider == .foundationModel)
    }

    /// Reset prompt to the built-in default.
    func resetToDefault(compact: Bool = false) {
        let fileName = compact ? Self.compactFileName : Self.commonFileName
        let url = Self.systemDir.appendingPathComponent(fileName)
        let content = compact ? Self.defaultCompactPrompt() : Self.defaultPrompt()
        let versioned = Self.versionPrefix + Self.appVersion + "\n" + content
        do {
            try versioned.write(to: url, atomically: true, encoding: .utf8)
        } catch {
            AuditLog.log(.api, "[SystemPrompt] Failed to reset \(fileName): \(error)")
        }
    }

    /// Legacy overload for backward compatibility.
    func resetToDefault(for provider: APIProvider) {
        resetToDefault(compact: provider == .foundationModel)
    }

    /// Check if a prompt is READ ONLY.
    func isReadOnly(compact: Bool = false) -> Bool {
        let fileName = compact ? Self.compactFileName : Self.commonFileName
        let url = Self.systemDir.appendingPathComponent(fileName)
        guard let existing = try? String(contentsOf: url, encoding: .utf8),
              let firstLine = existing.components(separatedBy: "\n").first else
        {
            return false
        }
        return firstLine.hasPrefix(Self.readOnlyPrefix)
    }

    /// The built-in default system prompt.
    private static func defaultPrompt() -> String {
        let base = AgentTools.systemPrompt(
            userName: "{userName}",
            userHome: "{userHome}",
            projectFolder: "{projectFolder}",
            shell: "{shell}"
        )
        return wrapWithRules(base)
    }

    /// The built-in default compact prompt (Apple AI).
    private static func defaultCompactPrompt() -> String {
        let base = AgentTools.compactSystemPrompt(userName: "{userName}", userHome: "{userHome}", projectFolder: "{projectFolder}")
        return wrapWithRules(base)
    }
}
