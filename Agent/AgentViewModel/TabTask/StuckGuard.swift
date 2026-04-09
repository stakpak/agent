
@preconcurrency import Foundation


// MARK: - Tab Task Stuck-File Guard

extension AgentViewModel {

    /// / Detect 3 consecutive edit failures on the same file and append a / recovery nudge as a synthetic tool_result.
    /// Mirrors the inline stuck-file / block from the legacy monolithic executeTabTask.
    func appendStuckFileNudgeIfNeeded(
        tab: ScriptTab,
        name: String,
        input: [String: Any],
        toolResult: [String: Any],
        editTools: Set<String>,
        stuckFiles: inout [String: Int],
        toolResults: inout [[String: Any]]
    ) {
        guard editTools.contains(name),
              let path = input["file_path"] as? String ?? input["path"] as? String,
              let output = toolResult["content"] as? String
        else { return }
        let lower = output.lowercased()
        let isFailure = lower.hasPrefix("error") || lower.contains("error:") || lower.contains("failed") || lower
            .contains("not found") || lower.contains("rejected")
        if isFailure {
            stuckFiles[path, default: 0] += 1
            if stuckFiles[path]! == 3 {
                let nudge = """
                ⚠️ 3 consecutive edit failures on \(path). STOP retrying the same approach.

                Recovery checklist (do these in order):
                1. read_file(file_path:"\(path)") \
                with NO offset/limit to get the FULL \
                fresh content
                2. Find the EXACT lines you want to \
                change in the new output. Do NOT trust \
                the tool_result from earlier reads — \
                the file may have been modified by your \
                previous edits.
                3. For edit_file: copy old_string verbatim \
                from the fresh read, including every space, \
                tab, and newline.
                4. For diff_and_apply: pass start_line and \
                end_line to scope the section.
                5. **REWIND**: file(action:"restore", \
                file_path:"\(path)") recovers the most \
                recent FileBackupService snapshot from \
                before your edits. Backups are auto-created \
                on every write_file/edit_file call.
                6. If you keep failing, switch tools — \
                write_file to overwrite the whole file is \
                a valid last resort.
                """
                toolResults.append(["type": "tool_result", "tool_use_id": "stuck_guard_3", "content": nudge])
                tab.appendLog("⚠️ Stuck nudge: 3 failures on \((path as NSString).lastPathComponent)")
                tab.flush()
            }
        } else {
            stuckFiles[path] = 0
        }
    }
}
