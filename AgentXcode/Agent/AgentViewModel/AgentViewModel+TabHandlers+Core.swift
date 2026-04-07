@preconcurrency import Foundation
import AgentMCP
import AgentD1F
import Cocoa


extension AgentViewModel {

    /// Handle Core tool calls for tab tasks.
    func handleTabCoreTool(
        tab: ScriptTab, name: String, input: [String: Any], toolId: String
    ) async -> TabToolResult {

        switch name {
        case "task_complete":
            let summary = input["summary"] as? String ?? "Done"
            tab.appendLog("✅ Completed: \(summary)")
            tab.flush()
            
            // Apple Intelligence mediator summary (same as main task)
            let mediator = AppleIntelligenceMediator.shared
            if mediator.isEnabled && mediator.showAnnotationsToUser {
                if let summaryAnnotation = await mediator.summarizeCompletion(summary: summary, commandsRun: []) {
                    if mediator.trainingEnabled {
                        TrainingDataStore.shared.captureAppleAIDecision(summaryAnnotation.content)
                    }
                    tab.appendLog(summaryAnnotation.formatted)
                    tab.flush()
                }
            }
            
            // If this is the Messages tab, reply to the iMessage sender
            if tab.isMessagesTab, let handle = tab.replyHandle {
                tab.replyHandle = nil
                sendMessagesTabReply(summary, handle: handle)
            }
            return TabToolResult(toolResult: nil, isComplete: true)

        case "plan_mode":
            let action = input["action"] as? String ?? "read"
            let output = Self.handlePlanMode(action: action, input: input, projectFolder: tab.projectFolder.isEmpty ? projectFolder : tab.projectFolder, tabName: tab.displayTitle, userPrompt: tab.currentTaskPrompt)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "project_folder":
            let output = handleProjectFolder(tab: tab, input: input)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "mode":
            let action = input["action"] as? String
            let output: String
            if let action {
                switch action {
                case "coding", "coding_mode":
                    codingModeEnabled = true; automationModeEnabled = false
                    output = "Coding mode ON — Core+Workflow+Coding+UserAgent tools active."
                case "automation", "workflow", "workflow_mode":
                    automationModeEnabled = true; codingModeEnabled = false
                    output = "Workflow mode ON — Core+Workflow+Automation+UserAgent tools active."
                default:
                    codingModeEnabled = false; automationModeEnabled = false
                    output = "Standard mode — all user-enabled tools restored."
                }
            } else {
                let enabled = input["enabled"] as? Bool ?? true
                codingModeEnabled = enabled; automationModeEnabled = false
                output = enabled ? "Coding mode ON — Core+Workflow+Coding+UserAgent tools active." : "Standard mode — all tools restored."
            }
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        default:
        let output = await executeNativeTool(name, input: input)
        tab.appendLog(output); tab.flush()
        return tabResult(output, toolId: toolId)
        }
    }
}
