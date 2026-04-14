@preconcurrency import Foundation
import AgentMCP


// MARK: - MCP Tool Execution
extension AgentViewModel {

    /// Handles MCP tool calls (mcp_ServerName_toolName).
    /// Returns true if this was an MCP tool call, false otherwise.
    @MainActor
    func handleMCPTool(
        name: String,
        input: [String: Any],
        toolId: String,
        appendLog: @escaping @MainActor @Sendable (String) -> Void,
        flushLog: @escaping @MainActor @Sendable () -> Void,
        toolResults: inout [[String: Any]]
    ) async -> Bool {
        guard name.hasPrefix("mcp_") else { return false }

        let parts = name.dropFirst(4).split(separator: "_", maxSplits: 1)
        let serverName = String(parts.first ?? "")
        let toolName = String(parts.last ?? "")

        // Snapshot disabled state once to avoid TOCTOU races
        let disabledSnapshot = MCPService.shared.disabledTools
        let toolKey = MCPService.toolKey(serverName: serverName, toolName: toolName)

        // Block disabled tools
        guard !disabledSnapshot.contains(toolKey) else {
            let msg = "Tool '\(toolName)' is disabled"
            appendLog("🖥️ MCP[\(serverName)]: \(msg)")
            toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": msg])
            return true
        }

        appendLog("🖥️ MCP[\(serverName)]: \(toolName)")
        flushLog()

        var mcpOutput = ""

        // Validate total argument size (1 MB cap)
        let argData = try? JSONSerialization.data(withJSONObject: input)
        if let argData, argData.count > 1_024 * 1_024 {
            mcpOutput = "MCP error: arguments exceed 1 MB limit"
            appendLog(mcpOutput)
            flushLog()
            toolResults.append(["type": "tool_result", "tool_use_id": toolId, "content": mcpOutput])

            return true
        }

        if let mcpTool = MCPService.shared.discoveredTools.first(where: {
            $0.serverName == serverName && $0.name == toolName
        }) {
            do {
                let args = input.mapValues { value -> JSONValue in
                    Self.toJSONValue(value)
                }
                let result = try await MCPService.shared.callTool(
                    serverId: mcpTool.serverId,
                    name: toolName,
                    arguments: args
                )
                mcpOutput = result.content.compactMap { block -> String? in
                    if case .text(let t) = block { return Self.formatMCPText(t) }
                    return nil
                }.joined(separator: "\n")
                // Wrap file content in code fence for syntax highlighting
                if toolName == "XcodeRead" || toolName == "XcodeGrep" {
                    let filePath = input["filePath"] as? String ?? input["path"] as? String ?? ""
                    let lang = Self.langFromPath(filePath)
                    mcpOutput = Self.codeFence(mcpOutput, language: lang)
                }
            } catch {
                mcpOutput = "MCP error: \(error.localizedDescription)"
            }
        } else {
            mcpOutput = "MCP tool not found: \(serverName)/\(toolName)"
        }

        appendLog(mcpOutput)
        flushLog()
        toolResults.append([
            "type": "tool_result",
            "tool_use_id": toolId,
            "content": mcpOutput,
        ])

        return true
    }

    /// Format MCP text responses — extract readable content from JSON wrappers.
    private static func formatMCPText(_ text: String) -> String {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("{"),
              let data = trimmed.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else
        {
            return text
        }

        var lines: [String] = []

        // Primary content fields (XcodeRead, diagnostics)
        if let content = json["content"] as? String {
            lines.append(content)
        }

        // Summary/result fields (build, tests)
        for key in ["summary", "buildResult", "executionResults"] {
            if let val = json[key] as? String, !val.isEmpty {
                lines.append(val)
            }
        }

        // Message field (additional info)
        if let msg = json["message"] as? String, !msg.isEmpty {
            lines.append(msg)
        }

        // String arrays: results, matches, items (XcodeGrep, XcodeGlob, XcodeLS)
        for key in ["results", "matches", "items"] {
            if let arr = json[key] as? [String], !arr.isEmpty {
                lines.append(contentsOf: arr)
            }
        }

        // Object arrays: issues, errors, tests (build errors, diagnostics, test results)
        for key in ["issues", "errors", "buildLogEntries"] {
            if let arr = json[key] as? [[String: Any]], !arr.isEmpty {
                for obj in arr {
                    lines.append(formatIssue(obj))
                }
            }
        }

        // Test results
        if let results = json["results"] as? [[String: Any]], !results.isEmpty {
            for r in results {
                let name = r["displayName"] as? String ?? r["identifier"] as? String ?? ""
                let state = r["state"] as? String ?? ""
                let errs = (r["errors"] as? [String])?.joined(separator: "; ") ?? ""
                let line = errs.isEmpty ? "\(state) \(name)" : "\(state) \(name): \(errs)"
                lines.append(line)
            }
        }

        // Document search results
        if let docs = json["documents"] as? [[String: Any]], !docs.isEmpty {
            for doc in docs {
                let title = doc["title"] as? String ?? ""
                let contents = doc["contents"] as? String ?? ""
                lines.append("### \(title)")
                lines.append(contents)
            }
        }

        return lines.isEmpty ? text : lines.joined(separator: "\n")
    }

    /// Format a single issue/error object into a readable line.
    private static func formatIssue(_ obj: [String: Any]) -> String {
        let severity = obj["severity"] as? String ?? obj["classification"] as? String ?? ""
        let message = obj["message"] as? String ?? ""
        let path = obj["filePath"] as? String ?? obj["path"] as? String ?? ""
        let line = obj["line"] as? Int ?? obj["lineNumber"] as? Int
        let lineStr = line.map { ":\($0)" } ?? ""
        if path.isEmpty {
            return "[\(severity)] \(message)"
        }
        return "[\(severity)] \(path)\(lineStr): \(message)"
    }
}
