
@preconcurrency import Foundation
import AgentTools
import AgentAudit
import AgentLLM
import AppKit
import AgentMCP
import AgentD1F

// MARK: - Tab Task Triage

extension AgentViewModel {

    /// / Outcome of pre-LLM triage for a tab task.
    enum TabTaskTriageOutcome {
        case done
        case passThrough
        case llmWithContext(String)
    }

    /// Run triage: direct commands, Apple AI conversation, accessibility agent.
    func runTabTaskTriage(
        tab: ScriptTab,
        prompt: String,
        completionSummary: inout String
    ) async -> TabTaskTriageOutcome {
        // Triage: direct commands, Apple AI conversation, accessibility agent,
        let mediator = AppleIntelligenceMediator.shared
        let triageResult = await mediator.triagePrompt(prompt, axDispatch: { [weak self] args in
            guard let self else { return "{\"success\":false,\"error\":\"agent deallocated\"}" }
            var input: [String: Any] = ["action": args.action]
            if let role = args.role { input["role"] = role }
            if let title = args.title { input["title"] = title }
            if let rawApp = args.app {
                let resolved = SDEFService.shared.resolveBundleId(name: rawApp) ?? rawApp
                input["appBundleId"] = resolved
                input["app"] = resolved
            }
            if let text = args.text { input["text"] = text }
            return await self.executeNativeTool("accessibility", input: input)
        }, runAgent: { [weak self] args in
            guard let self else { return "error: agent deallocated" }
            let success = await self.runAgentDirect(name: args.name, arguments: args.arguments ?? "", switchToTab: false)
            return success ? "Launched agent '\(args.name)'" : "Agent '\(args.name)' not found"
        }, appendLog: { msg in tab.appendLog(msg); tab.flush() }, projectFolder: tab.projectFolder.isEmpty ? self.projectFolder : tab.projectFolder)
        switch triageResult {
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
            // Apple AI ran accessibility tool(s) and produced a summary. Tool c
            tab.rawLLMOutput = summary
            tab.displayedLLMOutput = summary
            tab.dripDisplayIndex = summary.count
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
