
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import Cocoa

// MARK: - Task Execution — LLM Response Content Parsing

extension AgentViewModel {

    /// / Result of parsing a single LLM response turn's content blocks.
    struct ResponseParseResult {
        var hasToolUse: Bool
        var pendingTools: [(toolId: String, name: String, input: [String: Any])]
        var taskCompleted: Bool
    }

    /// / Walks the LLM response content blocks
    func parseLLMResponseContent(
        _ responseContent: [[String: Any]],
        prompt: String,
        mediator: AppleIntelligenceMediator,
        appleAIAnnotations: inout [AppleIntelligenceMediator.Annotation],
        filesEditedThisTask: inout Set<String>,
        completionSummary: inout String
    ) async -> ResponseParseResult {
        var hasToolUse = false
        var pendingTools: [(toolId: String, name: String, input: [String: Any])] = []

        for block in responseContent {
            guard let type = block["type"] as? String else { continue }

            if type == "text" {
                // LLM text goes to LLM Output only — LogView is for user status
            } else if type == "server_tool_use" {
                // Server-side tool (web search) — executed by the API, just log
                hasToolUse = true
                if let input = block["input"] as? [String: Any],
                   let query = input["query"] as? String
                {
                    appendLog("Web search: \(query)")
                }
            } else if type == "web_search_tool_result" {
                // Display search results summary
                if let content = block["content"] as? [[String: Any]] {
                    let results = content.compactMap { result -> String? in
                        guard result["type"] as? String == "web_search_result",
                              let title = result["title"] as? String,
                              let url = result["url"] as? String else { return nil }
                        return "  \(title)\n    \(url)"
                    }
                    if !results.isEmpty {
                        appendLog("📊\n" + results.prefix(5).joined(separator: "\n"))
                    }
                }
                flushLog()
            } else if type == "tool_use" {
                hasToolUse = true
                guard let toolId = block["id"] as? String,
                      var name = block["name"] as? String,
                      var input = block["input"] as? [String: Any] else { continue }

                // Expand consolidated CRUDL tools into legacy tool names
                (name, input) = Self.expandConsolidatedTool(name: name, input: input)

                // Plans are encouraged but never required.
                let editTools: Set<String> = [
                    "write_file",
                    "edit_file",
                    "diff_apply",
                    "diff_and_apply",
                    "create_diff",
                    "apply_diff"
                ]
                if editTools.contains(name), let filePath = input["file_path"] as? String, !filePath.isEmpty {
                    filesEditedThisTask.insert(filePath)
                }

                if name == "task_complete" {
                    var summary = input["summary"] as? String ?? "Done"
                    let stripped = summary.trimmingCharacters(in: CharacterSet(charactersIn: ". "))
                    if stripped.isEmpty || summary == "..." {
                        let lastText = rawLLMOutput.trimmingCharacters(in: .whitespacesAndNewlines)
                        if !lastText.isEmpty { summary = String(lastText.prefix(300)) }
                    }
                    completionSummary = summary
                    // Show task complete in LLM Output HUD so user sees the res
                    let trimmedRaw = rawLLMOutput.trimmingCharacters(in: .whitespacesAndNewlines)
                    if trimmedRaw.isEmpty {
                        rawLLMOutput = "✅ \(summary)"
                    } else if !trimmedRaw.contains(summary) {
                        rawLLMOutput += "\n\n✅ \(summary)"
                    }
                    // Make sure the drip task is still running so it picks up t
                    startDripIfNeeded()

                    // Apple Intelligence summary annotation
                    if mediator.isEnabled && mediator.showAnnotationsToUser && !commandsRun.isEmpty {
                        if let summaryAnnotation = await mediator.summarizeCompletion(summary: summary, commandsRun: commandsRun) {
                            appleAIAnnotations.append(summaryAnnotation)
                            appendLog(summaryAnnotation.formatted)
                            flushLog()
                            if agentReplyHandle != nil {
                                sendProgressUpdate(summaryAnnotation.formatted)
                            }
                        }
                    }

                    appendLog("✅ Completed: \(summary)")
                    flushLog()
                    history.add(
                        TaskRecord(prompt: prompt, summary: summary, commandsRun: commandsRun),
                        maxBeforeSummary: maxHistoryBeforeSummary,
                        apiKey: apiKey,
                        model: selectedModel
                    )
                    // End the task in SwiftData chat history
                    ChatHistoryStore.shared.endCurrentTask(summary: summary)
                    // Stop progress updates before sending final reply
                    stopProgressUpdates()
                    // Reply to the iMessage sender if this was an Agent! prompt
                    sendAgentReply(summary)
                    isRunning = false
                    return ResponseParseResult(hasToolUse: hasToolUse, pendingTools: pendingTools, taskCompleted: true)
                }

                pendingTools.append((toolId: toolId, name: name, input: input))
            }
        }

        return ResponseParseResult(hasToolUse: hasToolUse, pendingTools: pendingTools, taskCompleted: false)
    }

    /// / Post-tool-dispatch handling: append the assistant turn to the / conver
    func finalizeTurnAndDetectCompletion(
        responseContent: [[String: Any]],
        hasToolUse: Bool,
        toolResults: [[String: Any]],
        messages: inout [[String: Any]]
    ) -> Bool {
        // Add assistant response to conversation Guard against empty content
        let assistantContent: Any = responseContent.isEmpty
            ? "I'll continue with the task." as Any
            : responseContent as Any
        let assistantMsg: [String: Any] = ["role": "assistant", "content": assistantContent]
        messages.append(assistantMsg)
        SessionStore.shared.appendMessage(assistantMsg)

        if hasToolUse && !toolResults.isEmpty {
            // Truncate large tool results to save tokens (cap at 8K chars each)
            let capped = Self.truncateToolResults(toolResults)
            let userMsg: [String: Any] = ["role": "user", "content": capped]
            messages.append(userMsg)
            SessionStore.shared.appendMessage(userMsg)
            return false
        } else if !hasToolUse {
            // Check if model wrote task_complete/done as text instead of a tool
            let responseText = responseContent.compactMap { $0["text"] as? String }.joined()
            if responseText.contains("task_complete") || responseText.contains("done(summary") {
                if let match = responseText.range(
                    of: #"(?:task_complete|done)\(summary[=:]\s*"([^"]+)""#,
                    options: .regularExpression
                ) {
                    let raw = String(responseText[match])
                    let summary = raw.replacingOccurrences(
                        of: #"(?:task_complete|done)\(summary[=:]\s*""#,
                        with: "",
                        options: .regularExpression
                    ).replacingOccurrences(of: "\"", with: "")
                    appendLog("✅ Completed: \(summary)")
                }
                flushLog()
                return true
            }
            // Check if model signaled completion via natural language
            let lower = responseText.lowercased()
            let doneSignals = [
                "conclude this task",
                "i'll conclude",
                "task is complete",
                "no further action",
                "nothing more to do",
                "no more content"
            ]
            if doneSignals.contains(where: { lower.contains($0) }) {
                // Ensure LLM Output shows the response
                displayedLLMOutput = rawLLMOutput
                dripDisplayIndex = rawLLMOutput.count
                let summary = String(responseText.prefix(300))
                appendLog("✅ Completed: \(summary)")
                flushLog()
                return true
            }
            // Text-only response (no tool calls) — complete immediately
            if rawLLMOutput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                rawLLMOutput = responseText
            }
            displayedLLMOutput = rawLLMOutput
            dripDisplayIndex = rawLLMOutput.count
            let summary = String(responseText.prefix(300))
            appendLog("✅ Completed: \(summary)")
            flushLog()
            return true
        } else {
            // Check if LLM signaled it's done via text even though it made tool
            let allText = responseContent.compactMap { $0["text"] as? String }.joined().lowercased()
            let stopPhrases = [
                "no more content",
                "no further action",
                "task is complete",
                "nothing more to do",
                "task_complete",
                "conclude this task",
                "i'll conclude",
                "feel free to ask",
                "let me know if"
            ]
            if stopPhrases.contains(where: { allText.contains($0) }) {
                return true
            }
            return false
        }
    }
}
