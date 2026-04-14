@preconcurrency import Foundation
import AgentTools

// Web tool dispatcher
extension AgentViewModel {
    func handleMainWebTool(name: String, input: [String: Any]) async -> String {
        let ws = WebAutomationService.shared
        let selector = input["selector"] as? String ?? input["query"] as? String ?? ""
        let browser = input["browser"] as? String

        switch name {
        case "web_open":
            guard let urlStr = input["url"] as? String, let url = URL(string: urlStr) else { return "Error: invalid URL" }
            do { return try await ws.open(url: url) } catch { return "Error: \(error.localizedDescription)" }

        case "web_read_content":
            return await ws.readPageContent(browser: browser)

        case "web_get_url":
            return await ws.getPageURL(browser: browser)

        case "web_get_title":
            return await ws.getPageTitle(browser: browser)

        case "web_click":
            do { return try await ws.click(selector: selector, strategy: .javascript) } catch {
                return "Error: \(error.localizedDescription)" }

        case "web_type":
            let text = input["text"] as? String ?? ""
            do { return try await ws.type(text: text, selector: selector, strategy: .javascript) } catch {
                return "Error: \(error.localizedDescription)" }

        case "web_find":
            let isPlainText = !selector.contains(".") && !selector.contains("#") && !selector.contains("[") && !selector
                .contains(":") && !selector.contains("/") && !selector.contains(">")
            if isPlainText {
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
                                tag: el.tagName,
                                text: text.substring(0, 100),
                                href: el.href || '',
                                id: el.id || '',
                                className: (el.className || '').substring(0, 80)
                            });
                        }
                    }
                    return matches;
                })())
                """
                if let result = try? await ws.executeJavaScript(script: js) as? String, !result.isEmpty, result != "[]" { return result }
                return "No clickable elements found containing '\(selector)'"
            }
            do {
                let el = try await ws.findElement(selector: selector)
                if let data = try? JSONSerialization.data(withJSONObject: el, options: .prettyPrinted), let s = String(
                    data: data,
                    encoding: .utf8
                ) { return s }
                return "Found: \(el)"
            } catch { return "Error: \(error.localizedDescription)" }

        case "web_execute_js":
            let script = input["script"] as? String ?? input["query"] as? String ?? ""
            let wrappedScript: String
            if script.contains("return ") && !script.contains("JSON.stringify") {
                wrappedScript = "JSON.stringify((function(){" + script + "})())"
            } else if !script.contains("JSON.stringify") && (script.contains("querySelectorAll") || script.contains("document.")) {
                wrappedScript = "JSON.stringify(" + script + ")"
            } else {
                wrappedScript = script
            }
            if let result = try? await ws.executeJavaScript(script: wrappedScript, browser: browser) as? String, !result.isEmpty {
                return result
            }
            return "(no output)"

        case "web_scan":
            return await ws.scanInteractiveElements()

        case "web_scroll_to":
            return await ws.scrollToElement(selector: selector, browser: browser)

        case "web_select":
            return await ws.selectOption(
                selector: selector,
                value: input["value"] as? String,
                text: input["text"] as? String,
                index: input["index"] as? Int,
                browser: browser
            )

        case "web_submit":
            return await ws.submitForm(selector: selector.isEmpty ? nil : selector, browser: browser)

        case "web_navigate":
            return await ws.navigate(action: input["action"] as? String ?? "back", browser: browser)

        case "web_list_tabs":
            return await ws.listTabs(browser: browser)

        case "web_switch_tab":
            let title = input["title"] as? String
            let index = input["index"] as? Int
            return await ws.switchTab(browser: browser, index: index, titleContains: title)

        case "web_list_windows":
            return await ws.listWindows(browser: browser)

        case "web_switch_window":
            return await ws.switchWindow(browser: browser, index: input["index"] as? Int ?? 1)

        case "web_new_window":
            return await ws.newWindow(browser: browser, url: input["url"] as? String)

        case "web_close_window":
            return await ws.closeWindow(browser: browser, index: input["index"] as? Int ?? 1)

        case "web_wait_for_element":
            return await ws.waitForElement(selector: selector, browser: browser, timeout: input["timeout"] as? Double ?? 10)

        case "web_storage":
            return await ws.readStorage(type: input["storage_type"] as? String ?? "cookies", key: input["key"] as? String, browser: browser)

        case "web_upload":
            return await ws.triggerFileUpload(selector: selector, browser: browser)

        case "web_google_search":
            let query = input["query"] as? String ?? ""
            return await ws.safariGoogleSearch(query: query)

        default:
            return await executeNativeTool(name, input: input)
        }
    }
}
