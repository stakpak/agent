
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

    /// / Process the tool_use blocks in an LLM streaming response. Extracted / from the legacy monolithic
    /// executeTabTask loop body. / / Mutates the per-task tracking state (`commandsRun`, `recentToolCalls`, / `stuckFiles`, `filesEditedThisTask`) in place.
    func processTabResponseContent(
        tab: ScriptTab,
        content: [[String: Any]],
        commandsRun: inout [String],
        recentToolCalls: inout [String],
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

                // Loop detection — block only after 20 IDENTICAL read calls (same file_path AND same offset AND same
                // limit). Different offset/limit on the same file does NOT count toward the limit; a write to anything resets the counter for the whole tab.
                let isRead = name == "read_file" || (name == "file_manager" && (input["action"] as? String) == "read")
                let isWrite = name == "write_file" || name == "edit_file"
                    || name == "create_diff" || name == "apply_diff"
                    || name == "diff_and_apply"
                    ||
                    (
                        name == "file_manager" && ["write", "edit", "diff_apply", "create", "apply"]
                            .contains(input["action"] as? String ?? "")
                    )
                if isWrite { recentToolCalls.removeAll() }
                if isRead {
                    let fp = input["file_path"] as? String ?? input["path"] as? String ?? ""
                    let offset = input["offset"] as? Int ?? 0
                    let limit = input["limit"] as? Int ?? 0
                    let callKey = "\(name):\(fp):\(offset):\(limit)"
                    let dupeLimit = 20
                    let dupeCount = recentToolCalls.filter { $0 == callKey }.count
                    if dupeCount >= dupeLimit {
                        tab
                            .appendLog(
                                """
                                ⚠️ Already read \((fp as NSString).lastPathComponent) \
                                \(dupeLimit) times with the same offset/limit — skipping
                                """
                            )
                        tab.flush()
                        toolResults.append([
                            "type": "tool_result",
                            "tool_use_id": toolId,
                            "content": """
                                Error: You already read this file \
                                \(dupeLimit) times with the SAME offset \
                                and limit. The content has not changed. \
                                Use the content you already have, or read \
                                a DIFFERENT range of the file.
                                """
                        ])
                        continue
                    }
                    recentToolCalls.append(callKey)
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
