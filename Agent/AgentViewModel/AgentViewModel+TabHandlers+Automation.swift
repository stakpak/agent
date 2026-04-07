@preconcurrency import Foundation
import AgentMCP
import AgentD1F
import Cocoa

extension AgentViewModel {

    /// Handle Automation tool calls for tab tasks.
    func handleTabAutomationTool(
        tab: ScriptTab, name: String, input: [String: Any], toolId: String
    ) async -> TabToolResult {

        switch name {
        case "run_osascript":
            let script = input["script"] as? String ?? input["command"] as? String ?? ""
            let escaped = script.replacingOccurrences(of: "'", with: "'\\''")
            let command = "osascript -e '\(escaped)'"
            tab.appendLog("🍎 \(script)")
            tab.isRunning = true
            tab.flush()

            let result = await Self.executeTCCStreaming(command: command) { [weak tab] chunk in
                Task { @MainActor in tab?.appendOutput(chunk) }
            }
            tab.isRunning = false

            guard !Task.isCancelled else { return TabToolResult(toolResult: nil, isComplete: false) }

            if result.status > 0 {
                tab.appendLog("exit code: \(result.status)")
            }

            let toolOutput = result.output.isEmpty
                ? "(no output, exit code: \(result.status))"
                : result.output
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": toolOutput],
                isComplete: false
            )

        case "lookup_sdef":
            let bundleID = input["bundle_id"] as? String ?? ""
            let className = input["class_name"] as? String

            let output: String
            if bundleID == "list" {
                let names = SDEFService.shared.availableSDEFs()
                output = "Available SDEFs (\(names.count)):\n" + names.joined(separator: "\n")
            } else if let cls = className {
                let props = SDEFService.shared.properties(for: bundleID, className: cls)
                let elems = SDEFService.shared.elements(for: bundleID, className: cls)
                var lines = ["\(cls) properties:"]
                for p in props {
                    let ro = p.readonly == true ? " (readonly)" : ""
                    let desc = p.description.map { " — \($0)" } ?? ""
                    lines.append(
                        "  .\(SDEFService.toCamelCase(p.name)): "
                            + "\(p.type ?? "any")\(ro)\(desc)"
                    )
                }
                if !elems.isEmpty { lines.append("elements: \(elems.joined(separator: ", "))") }
                output = lines.isEmpty ? "No class '\(cls)' found for \(bundleID)" : lines.joined(separator: "\n")
            } else {
                output = SDEFService.shared.summary(for: bundleID)
            }
            tab.appendLog("📖 SDEF: \(bundleID)\(className.map { " → \($0)" } ?? "")")
            let preview = output.components(separatedBy: "\n").prefix(20).joined(separator: "\n")
            let truncated = output.components(separatedBy: "\n")
                .count > 20 ? "\n... (\(output.components(separatedBy: "\n").count) lines total)" : ""
            tab.appendLog(preview + truncated)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "run_applescript":
            let source = input["source"] as? String ?? ""
            tab.appendLog("🍎 AppleScript:\n\(source)")
            tab.isRunning = true
            tab.flush()
            let result = await Self.offMain {
                NSAppleScriptService.shared.execute(source: source)
            }
            tab.isRunning = false
            if !result.success {
                tab.appendLog(result.output)
            } else {
                tab.appendOutput(result.output)
            }
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": result.output],
                isComplete: false
            )

        default:
            let output = await executeNativeTool(name, input: input)
            tab.appendLog(output); tab.flush()
            return tabResult(output, toolId: toolId)
        }
    }
}
