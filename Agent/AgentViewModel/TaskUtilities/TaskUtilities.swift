
@preconcurrency import Foundation
import AgentTools
import AgentAudit
import AgentMCP
import AgentD1F
import Cocoa




// MARK: - Task Utilities

extension AgentViewModel {

    /// / Read project-specific instructions from config files in the project folder. / Checks: .agent.md, AGENT.md,
    /// .claude/CLAUDE.md, .claude/rules/*.md / Supports @include directives: @path, @./relative, @~/home, @/absolute
    nonisolated static func readProjectConfig(projectFolder: String) -> String {
        guard !projectFolder.isEmpty else { return "" }
        let fm = FileManager.default
        var parts: [String] = []

        // Main config file (first found wins)
        let candidates = [
            "\(projectFolder)/.agent.md",
            "\(projectFolder)/AGENT.md",
            "\(projectFolder)/.claude/CLAUDE.md",
        ]
        for path in candidates {
            if fm.fileExists(atPath: path),
               let content = try? String(contentsOfFile: path, encoding: .utf8),
               !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            {
                parts.append(processIncludes(content, basePath: projectFolder))
                break
            }
        }

        // Additional rules from .claude/rules/*.md
        let rulesDir = "\(projectFolder)/.claude/rules"
        if let ruleFiles = try? fm.contentsOfDirectory(atPath: rulesDir) {
            for file in ruleFiles.sorted() where file.hasSuffix(".md") {
                let path = "\(rulesDir)/\(file)"
                if let content = try? String(contentsOfFile: path, encoding: .utf8),
                   !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                {
                    parts.append(content)
                }
            }
        }

        let combined = parts.joined(separator: "\n\n")
        return LogLimits.trim(combined, cap: LogLimits.configMergeChars)
    }

    /// Process @include directives in config content.
    /// Supports: @path, @./relative, @~/home, @/absolute
    private nonisolated static func processIncludes(_ content: String, basePath: String, processed: Set<String> = []) -> String {
        let allowedExtensions = Set([
            "md",
            "txt",
            "json",
            "yaml",
            "yml",
            "toml",
            "swift",
            "py",
            "js",
            "ts",
            "rs",
            "go",
            "java",
            "c",
            "cpp",
            "h"
        ])
        var result: [String] = []
        var seen = processed

        for line in content.components(separatedBy: "\n") {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            // Skip @include inside code blocks
            guard trimmed.hasPrefix("@") && !trimmed.hasPrefix("@_") && !trimmed.hasPrefix("@@") else {
                result.append(line)
                continue
            }
            var path = String(trimmed.dropFirst()) // remove @
            // Resolve path
            if path.hasPrefix("~/") {
                path = (path as NSString).expandingTildeInPath
            } else if path.hasPrefix("./") || !path.hasPrefix("/") {
                path = (basePath as NSString).appendingPathComponent(path)
            }
            // Safety: check extension, prevent circular refs, check existence
            let ext = (path as NSString).pathExtension.lowercased()
            guard allowedExtensions.contains(ext),
                  !seen.contains(path),
                  let included = try? String(contentsOfFile: path, encoding: .utf8) else
            {
                result.append(line) // keep original line if can't include
                continue
            }
            seen.insert(path)
            // Recursively process includes in the included file
            let processed = processIncludes(included, basePath: (path as NSString).deletingLastPathComponent, processed: seen)
            result.append(processed)
        }
        return result.joined(separator: "\n")
    }

    /// Build the prompt prefix for a new task — shared between main task and tab task.
    nonisolated static func newTaskPrefix(projectFolder: String, prompt: String = "") -> String {
        let folderPrefix = projectFolder.isEmpty ? "" : "[project folder: \(projectFolder)] "
        let projectConfig = readProjectConfig(projectFolder: projectFolder)
        let configPrefix = projectConfig.isEmpty ? "" : "[Project instructions:\n\(projectConfig)]\n\n"
        let isQuestion = isQuestionPrompt(prompt)
        let taskHeader = isQuestion
            ?
            """
            [QUESTION — Answer this directly. Do NOT use tools unless \
            the question requires reading files or running commands. \
            Call done(summary:"...") with your answer.]
            """
            :
            """
            [NEW TASK — Do ONLY what is asked below. Ignore all previous \
            task history. When done, call done(summary:"...") immediately. \
            Do NOT continue with unrelated work.]
            """
        return taskHeader + folderPrefix + configPrefix
    }

    /// Detect if a prompt is a question (How/What/When/Where/Why/Can/Is/Does/Do/Which)
    nonisolated static func isQuestionPrompt(_ prompt: String) -> Bool {
        let lower = prompt.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
        let questionStarters = [
            "how ",
            "what ",
            "when ",
            "where ",
            "why ",
            "who ",
            "can ",
            "is ",
            "does ",
            "do ",
            "which ",
            "should ",
            "could ",
            "would ",
            "will ",
            "are ",
            "was ",
            "were ",
            "has ",
            "have ",
            "explain ",
            "describe ",
            "tell me "
        ]
        return questionStarters.contains { lower.hasPrefix($0) } || lower.hasSuffix("?")
    }

    /// Strip done(summary:...) and task_complete(summary:...) text from a string
    static func stripCompletionText(_ text: inout String) {
        // Remove done(summary: "...") and task_complete(summary: "...")
        if let regex = try? NSRegularExpression(pattern: #"(?:done|task_complete)\(summary[=:]\s*"[^"]*"\)"#) {
            text = regex.stringByReplacingMatches(
                in: text,
                range: NSRange(location: 0, length: (text as NSString).length),
                withTemplate: ""
            )
        }
        // Trim trailing whitespace left behind
        while text.hasSuffix("\n\n") { text = String(text.dropLast()) }
    }

    static func truncateToolResults(_ results: [[String: Any]]) -> [[String: Any]] {
        // Step 1: truncate individual results
        var truncated = results.map { result -> [String: Any] in
            guard var content = result["content"] as? String,
                  content.count > LogLimits.toolResultChars else { return result }
            let keepChars = LogLimits.toolResultChars / 2
            let head = String(content.prefix(keepChars))
            let tail = String(content.suffix(keepChars))
            let trimmed = content.count - LogLimits.toolResultChars
            content = head + "\n\n... (\(trimmed) chars truncated) ...\n\n" + tail
            var updated = result
            updated["content"] = content
            return updated
        }
        // Step 2: enforce per-message budget — drop largest results first
        var totalChars = truncated.reduce(0) { $0 + ((($1["content"] as? String)?.count) ?? 0) }
        while totalChars > LogLimits.toolResultsPerMessageChars && truncated.count > 1 {
            // Find largest result and truncate it further
            if let maxIdx = truncated.enumerated()
                .max(by: { (($0.element["content"] as? String)?.count ?? 0) < (($1.element["content"] as? String)?.count ?? 0) })?.offset
            {
                let content = truncated[maxIdx]["content"] as? String ?? ""
                truncated[maxIdx]["content"] = LogLimits.trim(
                    content,
                    cap: 2000,
                    suffix: "Budget-truncated from \(content.count) chars."
                )
                totalChars = truncated.reduce(0) { $0 + ((($1["content"] as? String)?.count) ?? 0) }
            } else {
                break
            }
        }
        return truncated
    }

    // MARK: - Message Pruning

    /// / Prune old messages to reduce token usage on long tasks. / Keeps the first user message and the most recent
    /// messages. / Middle messages are summarized into a compact text block.
    static func pruneMessages(_ messages: inout [[String: Any]], keepRecent: Int = 6) {
        guard messages.count > keepRecent + 4 else { return }

        let firstMsg = messages[0]
        let recentMessages = Array(messages.suffix(keepRecent))
        let middleMessages = Array(messages.dropFirst(1).dropLast(keepRecent))

        // Build compact summary of middle messages
        var summaryLines: [String] = []
        for msg in middleMessages {
            let role = msg["role"] as? String ?? "?"
            if let text = msg["content"] as? String {
                summaryLines.append("\(role): \(String(text.prefix(150)))")
            } else if let blocks = msg["content"] as? [[String: Any]] {
                for block in blocks {
                    let type = block["type"] as? String ?? ""
                    if type == "tool_use", let name = block["name"] as? String {
                        summaryLines.append("tool: \(name)")
                    } else if type == "tool_result" {
                        let content = block["content"] as? String ?? ""
                        let preview = content.hasPrefix("Error") ? String(content.prefix(100)) : "OK"
                        summaryLines.append("result: \(preview)")
                    } else if type == "text", let text = block["text"] as? String {
                        summaryLines.append("\(role): \(String(text.prefix(150)))")
                    } else if type == "image" {
                        summaryLines.append("[image removed]")
                    }
                }
            }
        }
        let summary = summaryLines.joined(separator: "\n")

        messages = [firstMsg]
        messages.append(["role": "user", "content": "Summary of previous \(middleMessages.count) messages:\n\(summary)"])
        messages.append(["role": "assistant", "content": "Understood, continuing."])
        messages.append(contentsOf: recentMessages)
    }

    /// Strip base64 image data from older messages to save tokens.
    static func stripOldImages(_ messages: inout [[String: Any]], keepRecentCount: Int = 4) {
        let cutoff = max(0, messages.count - keepRecentCount)
        for i in 0..<cutoff {
            guard var blocks = messages[i]["content"] as? [[String: Any]] else { continue }
            var changed = false
            for j in 0..<blocks.count {
                if blocks[j]["type"] as? String == "image" {
                    blocks[j] = ["type": "text", "text": "[screenshot removed]"]
                    changed = true
                }
            }
            if changed { messages[i]["content"] = blocks }
        }
    }

    // MARK: - Conversational Reply Detection

    /// / Determines if an LLM text response (with no tool calls) is a valid conversational reply / that should be
    /// accepted immediately, rather than nudging the LLM to use tools. / / On iteration 1, the LLM has seen the user's prompt fresh — if it chose text over tools, / it's almost certainly a conversational reply (greeting, answer, explanation). / After iteration 1, the LLM has already been given tool results and is mid-task, / so a text-only response more likely means it forgot to call tools.
    static func isConversationalReply(_ text: String, iteration: Int) -> Bool {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return false }
        // On iteration 1, if the LLM responded with text and no tools, trust it
        if iteration == 1 { return true }
        // On later iterations, only accept short non-code responses as conversational
        let hasCodeBlock = trimmed.contains("```")
        let isLong = trimmed.count > 1500
        return !hasCodeBlock && !isLong
    }

    /// Helper function to check if a Unicode scalar is an emoji
    func isEmoji(_ scalar: Unicode.Scalar) -> Bool {
        switch scalar.value {
        case 0x1F600...0x1F64F, // Emoticons
             0x1F300...0x1F5FF, // Misc Symbols and Pictographs
             0x1F680...0x1F6FF, // Transport and Map Symbols
             0x1F1E6...0x1F1FF, // Regional indicator symbols
             0x2600...0x26FF, // Misc symbols
             0x2700...0x27BF, // Dingbats
             0xFE00...0xFE0F, // Variation Selectors
             0x1F900...0x1F9FF, // Supplemental Symbols and Pictographs
             0x1FA00...0x1FA6F, // Chess Symbols
             0x1FA70...0x1FAFF: // Symbols and Pictographs Extended-A
            return true
        default:
            return false
        }
    }
}
