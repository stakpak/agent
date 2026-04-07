@preconcurrency import Foundation
import AgentMCP
import AgentD1F
import Cocoa

extension AgentViewModel {

    /// Handle Web tool calls for tab tasks.
    func handleTabWebTool(
        tab: ScriptTab, name: String, input: [String: Any], toolId: String
    ) async -> TabToolResult {

        switch name {
        case "web_open":
            guard let urlString = input["url"] as? String,
                  let url = URL(string: urlString) else {
                let errorMsg = "Error: Invalid or missing URL"
                tab.appendLog(errorMsg)
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": errorMsg],
                    isComplete: false
                )
            }
            let browserStr = input["browser"] as? String ?? "safari"
            let browser = WebAutomationService.BrowserType(rawValue: browserStr) ?? .safari
            tab.appendLog("🌐 \(urlString) in \(browser.rawValue)...")
            tab.flush()
            do {
                let output = try await WebAutomationService.shared.open(url: url, browser: browser)
                tab.appendLog(output)
            } catch {
                tab.appendLog("❌ \(error.localizedDescription)")
            }
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": tab.logBuffer],
                isComplete: false
            )

        case "web_find":
            let selector = input["selector"] as? String ?? input["query"] as? String ?? ""
            let strategyStr = input["strategy"] as? String ?? "auto"
            let timeout = input["timeout"] as? Double ?? 10.0
            let fuzzyThreshold = input["fuzzyThreshold"] as? Double ?? 0.6
            let appBundleId = input["appBundleId"] as? String
            tab.appendLog("🔍 \(selector)...")
            tab.flush()
            // If it looks like plain text (not CSS selector), search for clickable elements containing that text
            let isPlainText = !selector.contains(".") && !selector.contains("#") && !selector.contains("[") && !selector.contains(":") && !selector.contains("/") && !selector.contains(">")
            var resultContent = ""
            if isPlainText {
                // Use JS to find elements containing the text
                let escaped = WebAutomationService.escapeJS(selector)
                let js = """
                JSON.stringify((function() {
                    var all = document.querySelectorAll('a, button, input[type=submit], [role=button]');
                    var matches = [];
                    for (var i = 0; i < all.length; i++) {
                        var el = all[i];
                        var text = el.textContent.trim();
                        if (text.toLowerCase().indexOf('\(escaped.lowercased())') >= 0) {
                            var rect = el.getBoundingClientRect();
                            matches.push({
                                tag: el.tagName, text: text.substring(0, 100),
                                href: el.href || '', id: el.id || '',
                                className: (el.className || '').substring(0, 80),
                                x: Math.round(rect.x), y: Math.round(rect.y),
                                width: Math.round(rect.width), height: Math.round(rect.height)
                            });
                        }
                    }
                    return matches;
                })())
                """
                if let result = try? await WebAutomationService.shared.executeJavaScript(script: js) as? String, !result.isEmpty, result != "[]" {
                    resultContent = result
                    tab.appendLog(result)
                } else {
                    resultContent = "No clickable elements found containing '\(selector)'"
                    tab.appendLog(resultContent)
                }
            } else {
                // CSS/XPath selector — use findElement
                let strategy = SelectorStrategy(rawValue: strategyStr) ?? .auto
                do {
                    let output = try await WebAutomationService.shared.findElement(
                        selector: selector, strategy: strategy, timeout: timeout,
                        fuzzyThreshold: fuzzyThreshold, appBundleId: appBundleId
                    )
                    if let jsonData = try? JSONSerialization.data(withJSONObject: output, options: .prettyPrinted),
                       let jsonStr = String(data: jsonData, encoding: .utf8) {
                        resultContent = jsonStr
                        tab.appendLog(jsonStr)
                    } else {
                        resultContent = "Found element: \(output)"
                        tab.appendLog(resultContent)
                    }
                } catch {
                    resultContent = "Error: \(error.localizedDescription)"
                    tab.appendLog(resultContent)
                }
            }
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": resultContent],
                isComplete: false
            )

        case "web_click":
            let selector = input["selector"] as? String ?? input["query"] as? String ?? ""
            let strategyStr = input["strategy"] as? String ?? "auto"
            let strategy = SelectorStrategy(rawValue: strategyStr) ?? .auto
            let appBundleId = input["appBundleId"] as? String
            tab.appendLog("👆 \(selector)...")
            tab.flush()
            do {
                let output = try await WebAutomationService.shared.click(
                    selector: selector, strategy: strategy, appBundleId: appBundleId
                )
                tab.appendLog(output)
            } catch {
                tab.appendLog("❌ \(error.localizedDescription)")
            }
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": tab.logBuffer],
                isComplete: false
            )

        case "web_type":
            let selector = input["selector"] as? String ?? input["query"] as? String ?? ""
            let text = input["text"] as? String ?? ""
            let strategyStr = input["strategy"] as? String ?? "auto"
            let strategy = SelectorStrategy(rawValue: strategyStr) ?? .auto
            let verify = input["verify"] as? Bool ?? true
            let appBundleId = input["appBundleId"] as? String
            tab.appendLog("⌨️ \(text.count) chars into: \(selector)...")
            tab.flush()
            do {
                let output = try await WebAutomationService.shared.type(
                    text: text, selector: selector, strategy: strategy, verify: verify, appBundleId: appBundleId
                )
                tab.appendLog(output)
            } catch {
                tab.appendLog("❌ \(error.localizedDescription)")
            }
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": tab.logBuffer],
                isComplete: false
            )

        case "web_execute_js":
            let script = input["script"] as? String ?? input["query"] as? String ?? ""
            let browser = input["browser"] as? String
            tab.appendLog("📜 JavaScript...")
            tab.flush()
            // Wrap script to ensure result is captured as JSON string
            // Safari's do JavaScript returns the last expression value, not return statements
            let wrappedScript: String
            if script.contains("return ") && !script.contains("JSON.stringify") {
                // Wrap IIFE-style scripts: replace return with JSON.stringify
                wrappedScript = "JSON.stringify((function(){" + script + "})())"
            } else if !script.contains("JSON.stringify") && (script.contains("querySelectorAll") || script.contains("document.")) {
                wrappedScript = "JSON.stringify(" + script + ")"
            } else {
                wrappedScript = script
            }
            var jsResult = "(no output)"
            do {
                let output = try await WebAutomationService.shared.executeJavaScript(script: wrappedScript, browser: browser)
                if let str = output as? String, !str.isEmpty {
                    jsResult = str
                } else if let val = output {
                    jsResult = String(describing: val)
                }
            } catch {
                jsResult = "Error: \(error.localizedDescription)"
            }
            tab.appendLog(jsResult)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": jsResult],
                isComplete: false
            )

        case "web_get_url":
            let browser = input["browser"] as? String
            tab.appendLog("🔗 page URL...")
            tab.flush()
            let url = await WebAutomationService.shared.getPageURL(browser: browser)
            tab.appendLog(url)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": url], isComplete: false)

        case "web_get_title":
            let browser = input["browser"] as? String
            tab.appendLog("📝 page title...")
            tab.flush()
            let title = await WebAutomationService.shared.getPageTitle(browser: browser)
            tab.appendLog(title)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": title], isComplete: false)

        case "web_read_content":
            let browser = input["browser"] as? String
            let maxLength = input["max_length"] as? Int ?? 10000
            tab.appendLog("📖 page content...")
            tab.flush()
            let content = await WebAutomationService.shared.readPageContent(browser: browser, maxLength: maxLength)
            tab.appendLog(String(content.prefix(500)) + (content.count > 500 ? "... (\(content.count) chars)" : ""))
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": content], isComplete: false)

        case "web_switch_tab":
            let browser = input["browser"] as? String
            let index = input["index"] as? Int
            let title = input["title"] as? String
            tab.appendLog("🔄 switch tab...")
            tab.flush()
            let output = await WebAutomationService.shared.switchTab(browser: browser, index: index, titleContains: title)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_list_tabs":
            let browser = input["browser"] as? String
            tab.appendLog("📋 tabs...")
            tab.flush()
            let output = await WebAutomationService.shared.listTabs(browser: browser)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_wait_for_element":
            let selector = input["selector"] as? String ?? input["query"] as? String ?? ""
            let browser = input["browser"] as? String
            let timeout = input["timeout"] as? Double ?? 10.0
            tab.appendLog("⏳ \(selector)...")
            tab.flush()
            let output = await WebAutomationService.shared.waitForElement(selector: selector, browser: browser, timeout: timeout)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_scroll_to":
            let selector = input["selector"] as? String ?? input["query"] as? String ?? ""
            let browser = input["browser"] as? String
            tab.appendLog("📜 \(selector)...")
            tab.flush()
            let output = await WebAutomationService.shared.scrollToElement(selector: selector, browser: browser)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_select":
            let selector = input["selector"] as? String ?? input["query"] as? String ?? ""
            let value = input["value"] as? String
            let text = input["text"] as? String
            let index = input["index"] as? Int
            let browser = input["browser"] as? String
            tab.appendLog("☑️ \(selector)...")
            tab.flush()
            let output = await WebAutomationService.shared.selectOption(selector: selector, value: value, text: text, index: index, browser: browser)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_upload":
            let selector = input["selector"] as? String ?? input["query"] as? String ?? ""
            let browser = input["browser"] as? String
            tab.appendLog("📤 upload: \(selector)...")
            tab.flush()
            let output = await WebAutomationService.shared.triggerFileUpload(selector: selector, browser: browser)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_storage":
            let storageType = input["storage_type"] as? String ?? "cookies"
            let key = input["key"] as? String
            let browser = input["browser"] as? String
            tab.appendLog("📖 \(storageType)...")
            tab.flush()
            let output = await WebAutomationService.shared.readStorage(type: storageType, key: key, browser: browser)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_submit":
            let selector = input["selector"] as? String
            let browser = input["browser"] as? String
            tab.appendLog("📤 form...")
            tab.flush()
            let output = await WebAutomationService.shared.submitForm(selector: selector, browser: browser)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_navigate":
            let action = input["action"] as? String ?? "back"
            let browser = input["browser"] as? String
            tab.appendLog("🧭 \(action)...")
            tab.flush()
            let output = await WebAutomationService.shared.navigate(action: action, browser: browser)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_list_windows":
            let browser = input["browser"] as? String
            tab.appendLog("📋 windows...")
            tab.flush()
            let output = await WebAutomationService.shared.listWindows(browser: browser)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_switch_window":
            let browser = input["browser"] as? String
            let index = input["index"] as? Int ?? 1
            tab.appendLog("🔄 window \(index)...")
            tab.flush()
            let output = await WebAutomationService.shared.switchWindow(browser: browser, index: index)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_new_window":
            let browser = input["browser"] as? String
            let url = input["url"] as? String
            tab.appendLog("🌐 new window...")
            tab.flush()
            let output = await WebAutomationService.shared.newWindow(browser: browser, url: url)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_close_window":
            let browser = input["browser"] as? String
            let index = input["index"] as? Int ?? 1
            tab.appendLog("🗑️ window \(index)...")
            tab.flush()
            let output = await WebAutomationService.shared.closeWindow(browser: browser, index: index)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_google_search":
            let query = input["query"] as? String ?? ""
            let maxResults = input["max_results"] as? Int ?? 3000
            guard !query.isEmpty else {
                let err = "Error: query is required"
                tab.appendLog(err); tab.flush()
                return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": err], isComplete: false)
            }
            tab.appendLog("🔍 Google search: \(query)...")
            tab.flush()
            let output = await WebAutomationService.shared.safariGoogleSearch(query: query, maxResults: maxResults)
            tab.appendLog(Self.preview(output, lines: 40))
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "web_scan":
            tab.appendLog("🔍 scanning interactive elements...")
            tab.flush()
            let elements = await WebAutomationService.shared.scanInteractiveElements()
            tab.appendLog(String(elements.prefix(2000)))
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": elements], isComplete: false)

        default:
        let output = await executeNativeTool(name, input: input)
        tab.appendLog(output); tab.flush()
        return tabResult(output, toolId: toolId)
        }
    }
}
