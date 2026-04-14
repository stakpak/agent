
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import Cocoa

// MARK: - Task Execution — Triage Result

extension AgentViewModel {

    /// Outcome of triage result processing.
    enum TriageOutcome {
        case completed
        case fallThroughToLLM
    }

    /// Processes the result of `AppleIntelligenceMediator.triagePrompt`
    func handleTriageOutcome(
        _ triageResult: AppleIntelligenceMediator.TriageResult,
        prompt: String,
        cloudModelLogLine: String,
        messages: inout [[String: Any]],
        completionSummary: inout String
    ) async -> TriageOutcome {
        switch triageResult {
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
            // Apple AI ran accessibility tool(s) and produced a summary. Tool c
            rawLLMOutput = summary
            displayedLLMOutput = summary
            dripDisplayIndex = summary.count
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
            // Apple AI didn't handle request
            appendLog(cloudModelLogLine)
            flushLog()
            return .fallThroughToLLM
        }
    }
}
