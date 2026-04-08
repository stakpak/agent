
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import AgentAccess
import Cocoa

// MARK: - Native Tool Handler — Shell / AppleScript / JXA

extension AgentViewModel {

    /// Handles shell, AppleScript, osascript, and JXA tool calls.
    /// Returns `nil` if the name is not a shell-group tool so the main
    /// dispatcher can fall through to the next handler.
    func handleShellNativeTool(name: String, input: [String: Any]) async -> String? {
        let pf = projectFolder
        switch name {
        // Shell commands
        case "execute_agent_command", "run_shell_script":
            let command = input["command"] as? String ?? ""
            if let suggestion = Self.suggestTool(command) { return suggestion }
            if let pathErr = Self.preflightCommand(command) { return pathErr }
            appendLog("🔧 $ \(Self.collapseHeredocs(command))")
            flushLog()
            if Self.needsTCCPermissions(command) {
                let result = await Self.executeTCCStreaming(command: command, workingDirectory: pf) { [weak self] chunk in
                    Task { @MainActor in self?.appendRawOutput(chunk) }
                }
                if result.status > 0 { appendLog("exit code: \(result.status)") }
                flushLog()
                return result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
            }
            let result = await executeViaUserAgent(command: command, workingDirectory: pf)
            // Auto-detect "command not found" and respond with whereis lookup
            if result.status != 0 && result.output.contains("command not found") {
                let tool = command.trimmingCharacters(in: .whitespaces).components(separatedBy: " ").first ?? ""
                if !tool.isEmpty {
                    let cmd = "/usr/bin/whereis \(tool) 2>/dev/null; "
                        + "which \(tool) 2>/dev/null; "
                        + "ls /opt/homebrew/bin/\(tool) "
                        + "/usr/local/bin/\(tool) 2>/dev/null"
                    let lookup = await executeViaUserAgent(command: cmd)
                    let paths = lookup.output
                        .trimmingCharacters(in: .whitespacesAndNewlines)
                    appendLog(
                        "🔍 whereis \(tool): "
                        + "\(paths.isEmpty ? "not found" : paths)")
                    return """
                        command not found: \(tool)
                        whereis results:
                        \(paths.isEmpty ? "Not installed on this system." : paths)
                        Use the full path to run it, or ask the user to install it.
                        """
                }
            }
            return result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
        case "execute_daemon_command":
            let command = input["command"] as? String ?? ""
            // TCC GUARD — even though this tool is "run as root", any
            // command that needs TCC permissions (osascript, screencapture,
            // accessibility, automation, etc.) MUST run in-process where
            // Agent! holds the user's TCC grants. The Launch Daemon runs
            // as root with NO TCC, so an `osascript -e 'tell ...'` from
            // there would fail with a confusing permission error. Reroute
            // to executeTCCStreaming silently and tell the user via the
            // log so the model can see what happened.
            if Self.needsTCCPermissions(command) {
                appendLog("🔧 $ (rerouted to in-process for TCC) \(Self.collapseHeredocs(command))")
                flushLog()
                let result = await Self.executeTCCStreaming(command: command, workingDirectory: pf) { [weak self] chunk in
                    Task { @MainActor in self?.appendRawOutput(chunk) }
                }
                if result.status > 0 { appendLog("exit code: \(result.status)") }
                flushLog()
                return result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
            }
            appendLog("🔴 # \(Self.collapseHeredocs(command))")
            flushLog()
            let result = await helperService.execute(command: command, workingDirectory: pf)
            if result.status > 0 { appendLog("exit code: \(result.status)") }
            flushLog()
            return result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
        // AppleScript (NSAppleScript in-process with TCC)
        case "run_applescript":
            let source = (input["source"] as? String ?? "")
            let result = await Self.offMain { () -> (String, Bool) in
                var err: NSDictionary?
                guard let script = NSAppleScript(source: source) else { return ("Error", false) }
                let out = script.executeAndReturnError(&err)
                if let e = err { return ("AppleScript error: \(e)", false) }
                return (out.stringValue ?? "(no output)", true)
            }
            if result.1 {
                let autoName = Self.autoScriptName(from: source)
                let _ = await Self.offMain { [ss = scriptService] in ss.saveAppleScript(name: autoName, source: source) }
                return result.0
            }
            return Self.enrichAppleScriptFailure(source: source, output: result.0)
        // osascript (runs osascript CLI in-process with TCC)
        case "run_osascript":
            let script = input["script"] as? String ?? input["command"] as? String ?? ""
            let escaped = script.replacingOccurrences(of: "'", with: "'\\''")
            let command = "osascript -e '\(escaped)'"
            let result = await Self.executeTCCStreaming(command: command) { _ in }
            if result.status == 0 {
                let _ = scriptService.saveAppleScript(name: Self.autoScriptName(from: script), source: script)
                return result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
            }
            let osaOutput = result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
            return Self.enrichAppleScriptFailure(source: script, output: osaOutput)
        // JavaScript for Automation (JXA via osascript -l JavaScript)
        case "execute_javascript":
            let script = input["source"] as? String ?? input["script"] as? String ?? ""
            let escaped = script.replacingOccurrences(of: "'", with: "'\\''")
            let command = "osascript -l JavaScript -e '\(escaped)'"
            let result = await Self.executeTCCStreaming(command: command) { _ in }
            if result.status == 0 {
                let _ = scriptService.saveJavaScript(name: Self.autoScriptName(from: script), source: script)
                return result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
            }
            let jxaOutput = result.output.isEmpty ? "(no output, exit \(result.status))" : result.output
            return Self.enrichJXAFailure(source: script, output: jxaOutput)
        default:
            return nil
        }
    }
}
