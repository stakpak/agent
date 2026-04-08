
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import Cocoa

// MARK: - Task Execution — Triage Result Handling

extension AgentViewModel {

    /// Outcome of triage result processing.
    /// - `completed`: the triage handler fully finished the task; caller should
    ///   return from executeTask immediately without touching cleanup.
    /// - `fallThroughToLLM`: triage did not fully handle the prompt; caller
    ///   should proceed into the cloud-LLM while-loop.
    enum TriageOutcome {
        case completed
        case fallThroughToLLM
    }

    /// Processes the result of `AppleIntelligenceMediator.triagePrompt`, mirroring the
    /// original inline switch: direct commands, Apple AI answers, accessibility-tool
    /// handoffs, and passthrough. Mutates `messages`, `completionSummary`, and
    /// `commandsRun` inout where needed and performs all side effects
    /// (history, logging, progress updates) exactly as the original inline code did.
    func handleTriageOutcome(
        _ triageResult: AppleIntelligenceMediator.TriageResult,
        prompt: String,
        cloudModelLogLine: String,
        messages: inout [[String: Any]],
        completionSummary: inout String
    ) async -> TriageOutcome {
        switch triageResult {
        case .directCommand(let cmd):
            if cmd.name == "run_agent" {
                // Parse "AgentName args" and always run directly — skip LLM
                let parts = cmd.argument.components(separatedBy: " ")
                let agentName = await Self.offMain { [ss = scriptService] in ss.resolveScriptName(parts.first ?? "") }
                let args = parts.count > 1 ? parts.dropFirst().joined(separator: " ") : ""
                if await Self.offMain({ [ss = scriptService] in ss.compileCommand(name: agentName) }) != nil {
                    let success = await runAgentDirect(name: agentName, arguments: args)
                    if success {
                        completionSummary = "Ran \(agentName)"
                        history.add(
                            TaskRecord(
                                prompt: prompt,
                                summary: completionSummary,
                                commandsRun: ["run_agent: \(agentName)"]
                            ),
                            maxBeforeSummary: maxHistoryBeforeSummary, apiKey: apiKey,
                            model: selectedModel
                        )
                        ChatHistoryStore.shared.endCurrentTask(summary: completionSummary)
                        stopProgressUpdates()
                        flushLog()
                        persistLogNow()
                        isRunning = false
                        isThinking = false
                        return .completed
                    }
                    // Failed — fall through to LLM to handle
                    appendLog("Direct run failed — passing to LLM")
                    flushLog()
                    return .fallThroughToLLM
                }
            }
            // Execute known commands instantly without the LLM
            let output = await executeDirectCommand(cmd)
            flushLog()

            // For safari commands, pass results to LLM for formatting
            if cmd.name == "safari_open_and_search" {
                appendLog("✅ Opened page and searched. Results on screen.")
                flushLog()
            }
            if cmd.name == "google_search" && output.contains("\"success\": true") {
                messages.append(["role": "user", "content": """
                Format these Google search results for the user. \
                Be concise — show the top results with titles, URLs, \
                and brief descriptions:

                \(output)
                """])
                return .fallThroughToLLM
            }
            if cmd.name == "safari_read" && !output.contains("Error") {
                messages.append(["role": "user", "content": """
                Summarize this web page for the user. \
                Show the title, URL, and key content:

                \(output)
                """])
                return .fallThroughToLLM
            }
            // safari_open: if user had additional instructions, read page and pass to LLM
            if cmd.name == "safari_open" {
                appendLog("✅ \(output)")
                // Check if the original prompt has more than just "open <url>"
                let urlArg = cmd.argument.lowercased()
                let remaining = prompt.lowercased().replacingOccurrences(of: urlArg, with: "")
                let noise = Set([
                    "open",
                    "safari",
                    "in",
                    "on",
                    "to",
                    "and",
                    "the",
                    "using",
                    "webpage",
                    "web",
                    "page",
                    "website",
                    "url",
                    "go",
                    "navigate",
                    "visit",
                    "browse"
                ])
                let meaningfulWords = remaining.components(separatedBy: .whitespacesAndNewlines)
                    .filter { !$0.isEmpty && !noise.contains($0) }
                if !meaningfulWords.isEmpty {
                    // Wait briefly for page to load
                    try? await Task.sleep(for: .seconds(2))
                    let pageContent = await WebAutomationService.shared.readPageContent(maxLength: 3000)
                    let pageTitle = await WebAutomationService.shared.getPageTitle()
                    let pageURL = await WebAutomationService.shared.getPageURL()
                    messages.append([
                        "role": "user",
                        "content": """
                            I opened \(pageURL) (\(pageTitle)). \
                            Here is the page content:

                            \(pageContent)

                            Now complete this request: \(prompt)
                            """
                    ])
                    return .fallThroughToLLM
                }
            }

            completionSummary = "Executed \(cmd.name)"
            history.add(
                TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: [cmd.name]),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
            ChatHistoryStore.shared.endCurrentTask(summary: completionSummary)
            stopProgressUpdates()
            if agentReplyHandle != nil { sendProgressUpdate(output) }
            flushLog()
            persistLogNow()
            isRunning = false
            isThinking = false
            return .completed
        case .answered(let reply):
            // Show in LLM Output, not LogView
            rawLLMOutput = reply
            displayedLLMOutput = reply
            dripDisplayIndex = reply.count
            appendLog("✅ Completed: \(String(reply.prefix(200)))")
            flushLog()
            completionSummary = String(reply.prefix(200))
            history.add(
                TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: []),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
            ChatHistoryStore.shared.endCurrentTask(summary: completionSummary)
            stopProgressUpdates()
            if agentReplyHandle != nil { sendProgressUpdate(reply) }
            flushLog()
            persistLogNow()
            isRunning = false
            isThinking = false
            return .completed
        case .accessibilityHandled(let summary):
            // Apple AI ran the accessibility tool itself (one or more times)
            // and produced a final summary. The tool calls already happened
            // through the axDispatch closure above — they went through the
            // same executeNativeTool path the cloud LLM uses, so they're
            // already logged in the activity log. The summary string is
            // what Apple AI said it accomplished after the tool calls.
            //
            // If Apple AI never called the tool, or any tool call failed,
            // runAccessibilityAgent returns nil and we never reach this
            // case (we fall through to .passThrough → cloud LLM).
            rawLLMOutput = summary
            displayedLLMOutput = summary
            dripDisplayIndex = summary.count
            appendLog("🍎 \(summary)")
            flushLog()
            completionSummary = String(summary.prefix(200))
            history.add(
                TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: ["accessibility (Apple AI)"]),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
            ChatHistoryStore.shared.endCurrentTask(summary: completionSummary)
            stopProgressUpdates()
            if agentReplyHandle != nil { sendProgressUpdate(summary) }
            flushLog()
            persistLogNow()
            isRunning = false
            isThinking = false
            return .completed
        case .passThrough:
            // Apple AI didn't handle the request — log the cloud LLM
            // model now so the user sees which provider is actually
            // doing the work.
            appendLog(cloudModelLogLine)
            flushLog()
            return .fallThroughToLLM
        }
    }
}
