@preconcurrency import Foundation
import AgentMCP
import AgentD1F
import Cocoa

extension AgentViewModel {

    /// Run a Selenium script with arguments.
    func runSeleniumHelper(tab: ScriptTab, toolId: String, args: String, logMessage: String) async -> TabToolResult {
        tab.appendLog(logMessage); tab.flush()
        guard let compileCmd = await Self.offMain({ [ss = scriptService] in ss.compileCommand(name: "Selenium") }) else {
            return tabResult("Error: Selenium script not found", toolId: toolId)
        }
        // MUST run via executeTCC (in-process) so swift build inherits the
        // main app's TCC grants for ~/Documents access. The Launch Agent
        // path (executeForTab → userService.execute) runs in a separate TCC
        // context that can't getcwd() inside ~/Documents/AgentScript/agents/.
        let compileResult = await Self.executeTCC(command: compileCmd)
        if compileResult.status != 0 {
            tab.appendLog("❌ Compile failed: \(compileResult.output)")
            return tabResult(compileResult.output, toolId: toolId)
        }
        let cancelFlag = tab._cancelFlag
        let result = await scriptService.loadAndRunScriptViaProcess(
            name: "Selenium",
            arguments: args,
            captureStderr: false,
            isCancelled: { cancelFlag.value }
        ) { _ in }
        tab.appendLog(result.output); tab.flush()
        return tabResult(result.output, toolId: toolId)
    }

    /// Handle Selenium tool calls for tab tasks.
    func handleTabSeleniumTool(
        tab: ScriptTab, name: String, input: [String: Any], toolId: String
    ) async -> TabToolResult {

        switch name {
        case "selenium_start":
            let browser = input["browser"] as? String ?? "safari"
            let port = input["port"] as? Int ?? 7055
            let args = "{\"action\":\"start\",\"browser\":\"\(browser)\",\"port\":\(port)}"
            return await runSeleniumHelper(tab: tab, toolId: toolId, args: args, logMessage: "Starting Selenium session (\(browser))...")

        case "selenium_stop":
            let port = input["port"] as? Int ?? 7055
            let args = "{\"action\":\"stop\",\"port\":\(port)}"
            return await runSeleniumHelper(tab: tab, toolId: toolId, args: args, logMessage: "Stopping Selenium session...")

        case "selenium_navigate":
            guard let url = input["url"] as? String else {
                let errorMsg = "Error: URL required for selenium_navigate"
                tab.appendLog(errorMsg)
                return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": errorMsg], isComplete: false)
            }
            let port = input["port"] as? Int ?? 7055
            let args = "{\"action\":\"navigate\",\"url\":\"\(url)\",\"port\":\(port)}"
            return await runSeleniumHelper(tab: tab, toolId: toolId, args: args, logMessage: "Navigating to: \(url)...")

        case "selenium_find":
            let strategy = input["strategy"] as? String ?? "css"
            let value = input["value"] as? String ?? ""
            let port = input["port"] as? Int ?? 7055
            let args = "{\"action\":\"find\",\"strategy\":\"\(strategy)\",\"value\":\"\(value)\",\"port\":\(port)}"
            return await runSeleniumHelper(tab: tab, toolId: toolId, args: args, logMessage: "Finding element: \(strategy)=\(value)...")

        case "selenium_click":
            let strategy = input["strategy"] as? String ?? "css"
            let value = input["value"] as? String ?? ""
            let port = input["port"] as? Int ?? 7055
            let args = "{\"action\":\"click\",\"strategy\":\"\(strategy)\",\"value\":\"\(value)\",\"port\":\(port)}"
            return await runSeleniumHelper(tab: tab, toolId: toolId, args: args, logMessage: "Clicking element: \(strategy)=\(value)...")

        case "selenium_type":
            let strategy = input["strategy"] as? String ?? "css"
            let value = input["value"] as? String ?? ""
            let text = input["text"] as? String ?? ""
            let port = input["port"] as? Int ?? 7055
            let escapedText = WebAutomationService.escapeJSON(text)
            let escapedValue = WebAutomationService.escapeJSON(value)
            let args =
                "{\"action\":\"type\",\"strategy\":\"\(strategy)\","
                + "\"value\":\"\(escapedValue)\",\"text\":\"\(escapedText)\","
                + "\"port\":\(port)}"
            return await runSeleniumHelper(
                tab: tab,
                toolId: toolId,
                args: args,
                logMessage: "Typing \(text.count) chars into: \(strategy)=\(value)..."
            )

        case "selenium_execute":
            let script = input["script"] as? String ?? ""
            let port = input["port"] as? Int ?? 7055
            let escapedScript = WebAutomationService.escapeJSON(script)
            let args = "{\"action\":\"execute\",\"script\":\"\(escapedScript)\",\"port\":\(port)}"
            return await runSeleniumHelper(tab: tab, toolId: toolId, args: args, logMessage: "Executing JavaScript via Selenium...")

        case "selenium_screenshot":
            let filename = input["filename"] as? String ?? "selenium_\(Int(Date().timeIntervalSince1970)).png"
            let port = input["port"] as? Int ?? 7055
            let args = "{\"action\":\"screenshot\",\"filename\":\"\(filename)\",\"port\":\(port)}"
            return await runSeleniumHelper(tab: tab, toolId: toolId, args: args, logMessage: "Taking screenshot...")

        case "selenium_wait":
            let strategy = input["strategy"] as? String ?? "css"
            let value = input["value"] as? String ?? ""
            let timeout = input["timeout"] as? Double ?? 10.0
            let port = input["port"] as? Int ?? 7055
            let args =
                "{\"action\":\"waitFor\",\"strategy\":\"\(strategy)\",\"value\":\"\(value)\",\"timeout\":\(timeout),\"port\":\(port)}"
            return await runSeleniumHelper(tab: tab, toolId: toolId, args: args, logMessage: "Waiting for element: \(strategy)=\(value)...")

        default:
            let output = await executeNativeTool(name, input: input)
            tab.appendLog(output); tab.flush()
            return tabResult(output, toolId: toolId)
        }
    }
}
