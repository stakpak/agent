//
//  TaskExecution+Selenium.swift
//  Agent
//
//  Selenium WebDriver tool handlers for TaskExecution
//

import Foundation

// MARK: - Selenium WebDriver Tool Execution

extension AgentViewModel {

    /// Handles Selenium WebDriver tool calls (selenium_start, selenium_stop, selenium_navigate, etc.)
    func handleSeleniumTool(name: String, input: [String: Any]) async -> String? {

        // Helper for Selenium operations
        func runSeleniumNative(action: String, args: String) async -> String {
            let fullArgs = args.isEmpty ? "{\"action\":\"\(action)\"}" : args
            guard let compileCmd = await Self.offMain({ [ss = scriptService] in ss.compileCommand(name: "Selenium") }) else {
                return "Error: Selenium script not found"
            }
            let compileResult = await Self.executeTCC(command: compileCmd)
            if compileResult.status != 0 {
                return "Compile failed: \(compileResult.output)"
            }
            let result = await scriptService.loadAndRunScript(
                name: "Selenium",
                arguments: fullArgs,
                captureStderr: false,
                isCancelled: nil
            ) { _ in }
            return result.output
        }

        // selenium_start
        switch name {

        case "selenium_start":
            let browser = input["browser"] as? String ?? "safari"
            let port = input["port"] as? Int ?? 7055
            let args = "{\"action\":\"start\",\"browser\":\"\(browser)\",\"port\":\(port)}"
            return await runSeleniumNative(action: "start", args: args)

        // selenium_stop
        case "selenium_stop":
            let port = input["port"] as? Int ?? 7055
            let args = "{\"action\":\"stop\",\"port\":\(port)}"
            return await runSeleniumNative(action: "stop", args: args)

        // selenium_navigate
        case "selenium_navigate":
            guard let url = input["url"] as? String else { return "Error: URL required" }
            let port = input["port"] as? Int ?? 7055
            let args = "{\"action\":\"navigate\",\"url\":\"\(url)\",\"port\":\(port)}"
            return await runSeleniumNative(action: "navigate", args: args)

        // selenium_find
        case "selenium_find":
            let strategy = input["strategy"] as? String ?? "css"
            let value = input["value"] as? String ?? ""
            let port = input["port"] as? Int ?? 7055
            let args = "{\"action\":\"find\",\"strategy\":\"\(strategy)\",\"value\":\"\(value)\",\"port\":\(port)}"
            return await runSeleniumNative(action: "find", args: args)

        // selenium_click
        case "selenium_click":
            let strategy = input["strategy"] as? String ?? "css"
            let value = input["value"] as? String ?? ""
            let port = input["port"] as? Int ?? 7055
            let args = "{\"action\":\"click\",\"strategy\":\"\(strategy)\",\"value\":\"\(value)\",\"port\":\(port)}"
            return await runSeleniumNative(action: "click", args: args)

        // selenium_type
        case "selenium_type":
            let strategy = input["strategy"] as? String ?? "css"
            let value = input["value"] as? String ?? ""
            let text = input["text"] as? String ?? ""
            let port = input["port"] as? Int ?? 7055
            let escapedText = text.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"")
            let args =
                "{\"action\":\"type\",\"strategy\":\"\(strategy)\",\"value\":\"\(value)\",\"text\":\"\(escapedText)\",\"port\":\(port)}"
            return await runSeleniumNative(action: "type", args: args)

        // selenium_execute
        case "selenium_execute":
            let script = input["script"] as? String ?? ""
            let port = input["port"] as? Int ?? 7055
            let escapedScript = script.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"")
            let args = "{\"action\":\"execute\",\"script\":\"\(escapedScript)\",\"port\":\(port)}"
            return await runSeleniumNative(action: "execute", args: args)

        // selenium_screenshot
        case "selenium_screenshot":
            let filename = input["filename"] as? String ?? "selenium_\(Int(Date().timeIntervalSince1970)).png"
            let port = input["port"] as? Int ?? 7055
            let args = "{\"action\":\"screenshot\",\"filename\":\"\(filename)\",\"port\":\(port)}"
            return await runSeleniumNative(action: "screenshot", args: args)

        // selenium_wait
        case "selenium_wait":
            let strategy = input["strategy"] as? String ?? "css"
            let value = input["value"] as? String ?? ""
            let timeout = input["timeout"] as? Double ?? 10.0
            let port = input["port"] as? Int ?? 7055
            let args =
                "{\"action\":\"waitFor\",\"strategy\":\"\(strategy)\",\"value\":\"\(value)\",\"timeout\":\(timeout),\"port\":\(port)}"
            return await runSeleniumNative(action: "waitFor", args: args)

        default:
            return nil
        }
    }
}
