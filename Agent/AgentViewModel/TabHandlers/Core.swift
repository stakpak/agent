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
            let output = Self.handlePlanMode(
                action: action, input: input,
                projectFolder: tab.projectFolder.isEmpty ? projectFolder : tab.projectFolder,
                tabName: tab.displayTitle, userPrompt: tab.currentTaskPrompt
            )
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

        default:
            let output = await executeNativeTool(name, input: input)
            tab.appendLog(output); tab.flush()
            return tabResult(output, toolId: toolId)
        }
    }
}
