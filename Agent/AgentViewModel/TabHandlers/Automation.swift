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

            let rawOutput = result.output.isEmpty
                ? "(no output, exit code: \(result.status))"
                : result.output
            let toolOutput = result.status == 0
                ? rawOutput
                : Self.enrichAppleScriptFailure(source: script, output: rawOutput)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": toolOutput],
                isComplete: false
            )

        case "lookup_sdef":
            let bundleIDInput = input["bundle_id"] as? String ?? ""
            let bundleIDArray = input["bundle_id"] as? [String]
            let className = input["class_name"] as? String

            let bundleIDs: [String] = {
                if let arr = bundleIDArray {
                    return arr.map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
                }
                return bundleIDInput
                    .split(separator: ",")
                    .map { $0.trimmingCharacters(in: .whitespaces) }
                    .filter { !$0.isEmpty }
            }()

            let output: String
            if bundleIDs.contains("list") || bundleIDInput == "list" {
                let names = SDEFService.shared.availableSDEFs()
                output = "Available SDEFs (\(names.count)):\n" + names.joined(separator: "\n")
            } else if bundleIDs.count == 1, let cls = className {
                let bundleID = bundleIDs[0]
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
            } else if bundleIDs.count == 1 {
                output = SDEFService.shared.summary(for: bundleIDs[0])
            } else if bundleIDs.isEmpty {
                output = "Error: bundle_id required (single ID, comma-separated list, array, or 'list')"
            } else {
                // Multi-bundle batch lookup.
                var blocks: [String] = []
                if className != nil {
                    blocks.append("⚠️ class_name is ignored when multiple bundle_ids are provided.")
                }
                for bundleID in bundleIDs {
                    let summary = SDEFService.shared.summary(for: bundleID)
                    blocks.append("=== \(bundleID) ===\n\(summary)")
                }
                output = blocks.joined(separator: "\n\n")
            }
            let logLabel = bundleIDs.count > 1 ? bundleIDs.joined(separator: ", ") : (bundleIDs.first ?? bundleIDInput)
            tab.appendLog("📖 SDEF: \(logLabel)\(className.map { " → \($0)" } ?? "")")
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
            let toolContent: String
            if !result.success {
                tab.appendLog(result.output)
                toolContent = Self.enrichAppleScriptFailure(source: source, output: result.output)
            } else {
                tab.appendOutput(result.output)
                toolContent = result.output
            }
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": toolContent],
                isComplete: false
            )

        case "execute_javascript":
            let script = input["source"] as? String ?? input["script"] as? String ?? ""
            tab.appendLog("⚡️ JXA:\n\(script)")
            tab.isRunning = true
            tab.flush()
            let escaped = script.replacingOccurrences(of: "'", with: "'\\''")
            let command = "osascript -l JavaScript -e '\(escaped)'"
            let result = await Self.executeTCCStreaming(command: command) { [weak tab] chunk in
                Task { @MainActor in tab?.appendOutput(chunk) }
            }
            tab.isRunning = false
            let toolContent: String
            if result.status == 0 {
                toolContent = result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
            } else {
                let jxaOutput = result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
                toolContent = Self.enrichJXAFailure(source: script, output: jxaOutput)
            }
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": toolContent],
                isComplete: false
            )

        default:
            let output = await executeNativeTool(name, input: input)
            tab.appendLog(output); tab.flush()
            return tabResult(output, toolId: toolId)
        }
    }
}
