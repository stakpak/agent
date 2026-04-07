//
//  AgentViewModel+TaskExecution+SavedScripts.swift
//  Agent
//
//  Saved scripts (AppleScript and JXA) management tools
//

import Foundation

// MARK: - Saved Script Tools

extension AgentViewModel {

    /// Handles saved script management tools (AppleScript and JXA)
    func handleSavedScriptTool(name: String, input: [String: Any]) async -> String {
        // Saved AppleScripts
        switch name {

        case "list_apple_scripts":
            let scripts = scriptService.listAppleScripts()
            return scripts.isEmpty ? "No saved AppleScripts" : scripts.map { "\($0.name) (\($0.size) bytes)" }.joined(separator: "\n")
        case "save_apple_script":
            return scriptService.saveAppleScript(name: input["name"] as? String ?? "", source: input["source"] as? String ?? "")
        case "delete_apple_script":
            return scriptService.deleteAppleScript(name: input["name"] as? String ?? "")
        case "run_apple_script":
            let scriptName = input["name"] as? String ?? ""
            guard let source = scriptService.readAppleScript(name: scriptName) else {
                return "Error: AppleScript '\(scriptName)' not found. Use list_apple_scripts first."
            }
            let result = await Self.offMain { () -> String in
                var err: NSDictionary?
                guard let script = NSAppleScript(source: source) else { return "Error creating script" }
                let out = script.executeAndReturnError(&err)
                if let e = err { return "AppleScript error: \(e)" }
                return out.stringValue ?? "(no output)"
            }
            return result

        // Saved JavaScript/JXA
        case "list_javascript":
            let scripts = scriptService.listJavaScripts()
            return scripts.isEmpty ? "No saved JXA scripts" : scripts.map { "\($0.name) (\($0.size) bytes)" }.joined(separator: "\n")
        case "save_javascript":
            return scriptService.saveJavaScript(name: input["name"] as? String ?? "", source: input["source"] as? String ?? "")
        case "delete_javascript":
            return scriptService.deleteJavaScript(name: input["name"] as? String ?? "")
        case "run_javascript":
            let scriptName = input["name"] as? String ?? ""
            guard let source = scriptService.readJavaScript(name: scriptName) else {
                return "Error: JXA script '\(scriptName)' not found. Use list_javascript first."
            }
            let escaped = source.replacingOccurrences(of: "'", with: "'\\''")
            let result = await Self.executeTCCStreaming(command: "osascript -l JavaScript -e '\(escaped)'") { _ in }
            return result.output.isEmpty ? "(no output, exit \(result.status))" : result.output

        default:
            return "Error: Unknown saved script tool: \(name)"
        }
    }
}
