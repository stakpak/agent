
@preconcurrency import Foundation
import AgentTools
import AgentAudit
import AgentLLM
import AppKit
import AgentMCP
import AgentD1F


// MARK: - Tab Task Triage

extension AgentViewModel {

    /// Outcome of pre-LLM triage for a tab task. When `.passThrough` the caller
    /// should fall through to the cloud LLM loop; when `.done` the task has
    /// already been handled and the caller should return immediately. When
    /// `.llmWithContext` the caller should pass the provided context string
    /// through to the LLM as an additional user message.
    enum TabTaskTriageOutcome {
        case done
        case passThrough
        case llmWithContext(String)
    }

    /// Run triage: direct commands, Apple AI conversation, accessibility agent.
    /// Mirrors the triage block in the old monolithic executeTabTask.
    func runTabTaskTriage(
        tab: ScriptTab,
        prompt: String,
        completionSummary: inout String
    ) async -> TabTaskTriageOutcome {
        // Triage: direct commands, Apple AI conversation, accessibility agent,
        // or pass through to LLM. The axDispatch closure routes Apple AI's
        // tool calls through the same executeNativeTool path the cloud LLM
        // uses. If Apple AI fails, is unavailable, or doesn't call the tool,
        // runAccessibilityAgent returns nil → triage returns .passThrough →
        // we fall through to the cloud LLM loop below.
        let mediator = AppleIntelligenceMediator.shared
        let triageResult = await mediator.triagePrompt(prompt) { [weak self] args in
            guard let self else { return "{\"success\":false,\"error\":\"agent deallocated\"}" }
            var input: [String: Any] = ["action": args.action]
            if let role = args.role { input["role"] = role }
            if let title = args.title { input["title"] = title }
            // SDEF catalog is the canonical source — see TaskExecution.swift
            // for the full rationale. Falls through to the runtime
            // NSRunningApplications scan in handleAccessibilityAction for
            // apps not in the SDEF list (Photo Booth, etc.).
            if let rawApp = args.app {
                let resolved = SDEFService.shared.resolveBundleId(name: rawApp) ?? rawApp
                input["appBundleId"] = resolved
                input["app"] = resolved
            }
            if let text = args.text { input["text"] = text }
            return await self.executeNativeTool("accessibility", input: input)
        }
        switch triageResult {
        case .directCommand(let cmd):
            if cmd.name == "run_agent" {
                // Parse "AgentName args" from cmd.argument
                let parts = cmd.argument.components(separatedBy: " ")
                let agentName = await Self.offMain { [ss = scriptService] in ss.resolveScriptName(parts.first ?? "") }
                let args = parts.count > 1 ? parts.dropFirst().joined(separator: " ") : ""
                // Always run directly — skip LLM. Args provided by user.
                if await Self.offMain({ [ss = scriptService] in ss.compileCommand(name: agentName) }) != nil {
                    let success = await runAgentDirect(name: agentName, arguments: args, switchToTab: false)
                    if success {
                        if tab.isMessagesTab, let handle = tab.replyHandle {
                            tab.replyHandle = nil
                            sendMessagesTabReply("Ran \(agentName)", handle: handle)
                        }
                        tab.isLLMRunning = false
                        tab.isLLMThinking = false
                        return .done
                    }
                    // Failed — fall through to LLM to handle
                    tab.appendLog("❌ Direct run failed — passing to LLM")
                    tab.flush()
                    return .passThrough
                }
                // compileCommand returned nil: fall through to executeDirectCommand below
            }
            let output = await executeDirectCommand(cmd, tab: tab)
            tab.flush()

            // Web commands: show results and complete
            if cmd.name == "web_open" {
                tab.appendLog("✅ \(output)")
                tab.flush()
            }
            if cmd.name == "web_open_and_search" {
                // Show a preview of search results in the log
                if let data = output.data(using: .utf8),
                   let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                   let title = json["title"] as? String,
                   let url = json["url"] as? String
                {
                    tab.appendLog("✅ \(title)")
                    tab.appendLog("🔗 \(url)")
                    if let content = json["content"] as? String {
                        tab.appendLog(String(content.prefix(1000)))
                    }
                } else {
                    tab.appendLog("✅ Search complete. Results on screen.")
                }
                tab.flush()
            }
            // google_search with results: pass to LLM for formatting
            if cmd.name == "google_search" && output.contains("\"success\": true") {
                let context = """
                    Format these Google search results for the user. \
                    Be concise — show the top results with titles, \
                    URLs, and brief descriptions:

                    \(output)
                    """
                return .llmWithContext(context)
            }

            completionSummary = "Executed \(cmd.name)"
            let formatter = DateFormatter()
            formatter.dateFormat = "HH:mm:ss"
            let time = formatter.string(from: Date())
            tab.tabTaskSummaries.append("[\(time)] \(prompt) → \(completionSummary)")
            history.add(
                TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: [cmd.name]),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
            tab.flush()
            if tab.isMessagesTab, let handle = tab.replyHandle {
                tab.replyHandle = nil
                sendMessagesTabReply(completionSummary, handle: handle)
            }
            tab.isLLMRunning = false
            tab.isLLMThinking = false
            return .done
        case .answered(let reply):
            // Show in LLM Output, not LogView
            tab.rawLLMOutput = reply
            tab.displayedLLMOutput = reply
            tab.dripDisplayIndex = reply.count
            tab.appendLog("✅ Completed: \(String(reply.prefix(200)))")
            tab.flush()
            completionSummary = String(reply.prefix(200))
            history.add(
                TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: []),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
            tab.flush()
            if tab.isMessagesTab, let handle = tab.replyHandle {
                tab.replyHandle = nil
                sendMessagesTabReply(completionSummary, handle: handle)
            }
            tab.isLLMRunning = false
            tab.isLLMThinking = false
            return .done
        case .accessibilityHandled(let summary):
            // Apple AI ran the accessibility tool itself (one or more times)
            // and produced a final summary. The tool calls already happened
            // through the axDispatch closure above — they went through the
            // same executeNativeTool path the cloud LLM uses, so they're
            // already logged in the activity log. The summary string is
            // what Apple AI said it accomplished.
            //
            // If Apple AI never called the tool, or any tool call failed,
            // runAccessibilityAgent returns nil → triage returns .passThrough
            // → we never reach this case and the cloud LLM takes over.
            tab.rawLLMOutput = summary
            tab.displayedLLMOutput = summary
            tab.dripDisplayIndex = summary.count
            tab.appendLog("🍎 \(summary)")
            tab.flush()
            completionSummary = String(summary.prefix(200))
            history.add(
                TaskRecord(prompt: prompt, summary: completionSummary, commandsRun: ["accessibility (Apple AI)"]),
                maxBeforeSummary: maxHistoryBeforeSummary,
                apiKey: apiKey,
                model: selectedModel
            )
            tab.flush()
            if tab.isMessagesTab, let handle = tab.replyHandle {
                tab.replyHandle = nil
                sendMessagesTabReply(completionSummary, handle: handle)
            }
            tab.isLLMRunning = false
            tab.isLLMThinking = false
            return .done
        case .passThrough:
            return .passThrough
        }
    }
}
