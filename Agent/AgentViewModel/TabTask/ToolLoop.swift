
@preconcurrency import Foundation
import AgentTools
import AgentLLM


// MARK: - Tab Task Tool Loop

extension AgentViewModel {

    /// Outcome of processing a single LLM response's tool_use blocks.
    enum TabToolProcessingOutcome {
        /// task_complete was called — caller should finalize and return
        case complete(summary: String)
        /// Normal completion; caller should append assistant/tool messages and continue
        case normal(hasToolUse: Bool, toolResults: [[String: Any]])
    }

    /// Process the tool_use blocks in an LLM streaming response. Extracted from the legacy monolithic
    /// executeTabTask loop body. Mutates the per-task tracking state (`commandsRun`, `stuckFiles`, `filesEditedThisTask`) in place.
    func processTabResponseContent(
        tab: ScriptTab,
        content: [[String: Any]],
        commandsRun: inout [String],
        stuckFiles: inout [String: Int],
        filesEditedThisTask: inout Set<String>,
        completionSummary: inout String
    ) async -> TabToolProcessingOutcome {
        var toolResults: [[String: Any]] = []
        var hasToolUse = false

        for block in content {
            guard let type = block["type"] as? String else { continue }

            if type == "text" {
                // Text goes to LLM output only — streaming delta already shows it there
            } else if type == "tool_use" {
                hasToolUse = true
                guard let toolId = block["id"] as? String,
                      let rawName = block["name"] as? String,
                      let rawInput = block["input"] as? [String: Any] else { continue }

                // Expand consolidated CRUDL tools into legacy tool names
                let (name, input) = Self.expandConsolidatedTool(name: rawName, input: rawInput)

                commandsRun.append(name)

                // Plans are encouraged but never required. Track edited files for task summary purposes. No mid-stream
                // blocking — the LLM decides whether to plan up front.
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
                    completionSummary = input["summary"] as? String ?? "Done"
                    // Show task complete in the LLM Output HUD so the user sees the result. Append to rawLLMOutput and
                    // let the drip task pick up the new chars naturally — DO NOT sync displayedLLMOutput, that would skip the drip.
                    let trimmedRaw = tab.rawLLMOutput.trimmingCharacters(in: .whitespacesAndNewlines)
                    if trimmedRaw.isEmpty {
                        tab.rawLLMOutput = "✅ \(completionSummary)"
                    } else if !trimmedRaw.contains(completionSummary) {
                        tab.rawLLMOutput += "\n\n✅ \(completionSummary)"
                    }
                    tab.startDripIfNeeded()
                }
                let toolStart = CFAbsoluteTimeGetCurrent()
                let result = await handleTabToolCall(
                    tab: tab, name: name, input: input, toolId: toolId
                )
                let toolElapsed = CFAbsoluteTimeGetCurrent() - toolStart
                if toolElapsed > 0.5 {
                    tab.appendLog("🕐 \(name) \(String(format: "%.1f", toolElapsed))s")
                    tab.flush()
                }
                if result.isComplete {
                    return .complete(summary: completionSummary)
                }
                if let toolResult = result.toolResult {
                    toolResults.append(toolResult)
                    // Stuck-file nudge: if this was an edit tool and the result looks like a failure, increment the
                    // per-file failure count. At 3 failures, append an actionable recovery nudge.
                    appendStuckFileNudgeIfNeeded(
                        tab: tab,
                        name: name,
                        input: input,
                        toolResult: toolResult,
                        editTools: editTools,
                        stuckFiles: &stuckFiles,
                        toolResults: &toolResults
                    )
                }
            }
        }

        return .normal(hasToolUse: hasToolUse, toolResults: toolResults)
    }
}
