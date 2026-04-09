//  TaskExecution+WebTools.swift Agent  Web Automation tool handlers for TaskExecution 

import Foundation

// MARK: - Web Automation Tool Execution

extension AgentViewModel {

    /// Handles web automation tool calls (web_open, web_find, web_click, web_type, etc.)
    func handleWebTool(name: String, input: [String: Any]) async -> String? {
        // web_open
        switch name {

        case "web_open":
            guard let urlString = input["url"] as? String,
                  let url = URL(string: urlString) else
            {
                return "Error: Invalid or missing URL"
            }
            let browserStr = input["browser"] as? String ?? "safari"
            let browser = WebAutomationService.BrowserType(rawValue: browserStr) ?? .safari
            do {
                return try await WebAutomationService.shared.open(url: url, browser: browser)
            } catch {
                return "Error: \(error.localizedDescription)"
            }

        // web_find
        case "web_find":
            let selector = input["selector"] as? String ?? ""
            let strategyStr = input["strategy"] as? String ?? "auto"
            let strategy = SelectorStrategy(rawValue: strategyStr) ?? .auto
            let timeout = input["timeout"] as? Double ?? 10.0
            let fuzzyThreshold = input["fuzzyThreshold"] as? Double ?? 0.6
            let appBundleId = input["appBundleId"] as? String
            do {
                let output = try await WebAutomationService.shared.findElement(
                    selector: selector, strategy: strategy, timeout: timeout,
                    fuzzyThreshold: fuzzyThreshold, appBundleId: appBundleId
                )
                if let jsonData = try? JSONSerialization.data(withJSONObject: output, options: .prettyPrinted),
                   let jsonStr = String(data: jsonData, encoding: .utf8)
                {
                    return jsonStr
                }
                return "Found element: \(output)"
            } catch {
                return "Error: \(error.localizedDescription)"
            }

        // web_click
        case "web_click":
            let selector = input["selector"] as? String ?? ""
            let strategyStr = input["strategy"] as? String ?? "auto"
            let strategy = SelectorStrategy(rawValue: strategyStr) ?? .auto
            let appBundleId = input["appBundleId"] as? String
            do {
                return try await WebAutomationService.shared.click(
                    selector: selector, strategy: strategy, appBundleId: appBundleId
                )
            } catch {
                return "Error: \(error.localizedDescription)"
            }

        // web_type
        case "web_type":
            let selector = input["selector"] as? String ?? ""
            let text = input["text"] as? String ?? ""
            let strategyStr = input["strategy"] as? String ?? "auto"
            let strategy = SelectorStrategy(rawValue: strategyStr) ?? .auto
            let verify = input["verify"] as? Bool ?? true
            let appBundleId = input["appBundleId"] as? String
            do {
                return try await WebAutomationService.shared.type(
                    text: text, selector: selector, strategy: strategy, verify: verify, appBundleId: appBundleId
                )
            } catch {
                return "Error: \(error.localizedDescription)"
            }

        // web_execute_js
        case "web_execute_js":
            let script = input["script"] as? String ?? ""
            let browser = input["browser"] as? String
            do {
                let result = try await WebAutomationService.shared.executeJavaScript(script: script, browser: browser)
                return result as? String ?? "Script executed"
            } catch {
                return "Error: \(error.localizedDescription)"
            }

        // web_google_search
        case "web_google_search":
            let query = input["query"] as? String ?? ""
            guard !query.isEmpty else { return "Error: query is required" }
            let maxResults = input["max_results"] as? Int ?? 3000
            return await WebAutomationService.shared.safariGoogleSearch(query: query, maxResults: maxResults)

        // web_get_url / web_get_title
        case "web_get_url":
            let browser = input["browser"] as? String
            return await WebAutomationService.shared.getPageURL(browser: browser)
        case "web_get_title":
            let browser = input["browser"] as? String
            return await WebAutomationService.shared.getPageTitle(browser: browser)
        // web_read_content
        case "web_read_content":
            let browser = input["browser"] as? String
            let maxLength = input["max_length"] as? Int ?? 10000
            return await WebAutomationService.shared.readPageContent(browser: browser, maxLength: maxLength)

        // web_scan — discover interactive elements on the page
        case "web_scan":
            return await WebAutomationService.shared.scanInteractiveElements()

        default:
            return nil
        }
    }
}
