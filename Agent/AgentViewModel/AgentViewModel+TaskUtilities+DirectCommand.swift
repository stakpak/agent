
@preconcurrency import Foundation
import AgentTools
import AgentAudit
import AgentMCP
import AgentD1F
import Cocoa




// MARK: - Task Utilities: Direct Command Execution

extension AgentViewModel {

    // MARK: - Direct Command Execution

    /// Execute a direct command matched by triage, without the LLM.
    /// When `tab` is provided, output is logged to that tab; otherwise to the main activity log.
    func executeDirectCommand(_ cmd: AppleIntelligenceMediator.DirectCommand, tab: ScriptTab? = nil) async -> String {
        let name = await Self.offMain { [ss = scriptService] in ss.resolveScriptName(cmd.argument) }

        // Resolve effective project folder: tab's folder > main folder (always resolve to directory)
        let rawFolder: String
        if let tab, !tab.projectFolder.isEmpty {
            rawFolder = tab.projectFolder
        } else {
            rawFolder = projectFolder
        }
        _ = AgentViewModel.resolvedWorkingDirectory(rawFolder)

        // Local helpers to route logging to the correct destination
        func log(_ message: String) {
            if let tab { tab.appendLog(message) } else { appendLog(message) }
        }
        func flush() {
            if let tab { tab.flush() } else { flushLog() }
        }

        switch cmd.name {
        case "list_agents":
            let (list, count) = await Self.offMain { [ss = scriptService] in
                (ss.numberedList(), ss.listScripts().count)
            }
            log("🦾 Agents: \(count) found")
            log(list)
            return list

        case "read_agent":
            guard let content = await Self.offMain({ [ss = scriptService] in ss.readScript(name: name) }) else {
                let available = await Self.offMain { [ss = scriptService] in ss.compactNameList() }
                let err = available.isEmpty
                    ? "Error: agent '\(name)' not found. No agents exist yet."
                    :
                    """
                    Error: agent '\(name)' not found. \
                    Available agents: \(available). \
                    Retry with the exact name \
                    (no 'script' or 'agent' prefix).
                    """
                log(err)
                return err
            }
            log("📖 Read: \(name)")
            log(AgentViewModel.codeFence(content, language: "swift"))
            return content

        case "delete_agent":
            let output = await Self.offMain { [ss = scriptService] in ss.deleteScript(name: name) }
            log(output)
            return output

        case "run_agent":
            // Only called when canRunDirectly returned true (no args needed)
            guard let compileCmd = await Self.offMain({ [ss = scriptService] in ss.compileCommand(name: name) }) else {
                let available = await Self.offMain { [ss = scriptService] in ss.compactNameList() }
                let err = available.isEmpty
                    ? "Error: agent '\(name)' not found. No agents exist yet — use agent_script(action:create) first."
                    :
                    """
                    Error: agent '\(name)' not found. \
                    Available agents: \(available). \
                    Retry with the exact name \
                    (no 'script' or 'agent' prefix).
                    """
                log(err)
                return err
            }
            // Skip compilation if dylib is up to date.
            // MUST run via executeTCC (in-process) so swift build inherits the
            // main app's TCC grants for ~/Documents access. The Launch Agent
            // path (userService.execute) runs in a separate TCC context that
            // can't getcwd() inside ~/Documents/AgentScript/agents/.
            if await Self.offMain({ [ss = scriptService] in !ss.isDylibCurrent(name: name) }) {
                log("🦾 Compiling: \(name)")
                flush()
                let compileResult = await Self.executeTCC(command: compileCmd)
                if compileResult.status != 0 {
                    log("Compile error:\n\(compileResult.output)")
                    return compileResult.output
                }
            }
            log("🦾 Running: \(name)")
            flush()
            RecentAgentsService.shared.recordRun(agentName: name, arguments: "", prompt: "run \(name)")
            let runResult = await scriptService.loadAndRunScriptViaProcess(
                name: name,
                projectFolder: tab?.projectFolder ?? projectFolder,
                onOutput: { [weak self] chunk in
                    Task { @MainActor in
                        if let tab {
                            tab.appendOutput(chunk)
                        } else {
                            self?.appendRawOutput(chunk)
                        }
                    }
                }
            )
            // Update agent menu status based on outcome
            let isUsage = runResult.output.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("Usage:")
            if isUsage || runResult.status != 0 {
                RecentAgentsService.shared.updateStatus(agentName: name, arguments: "", status: .failed)
            } else {
                RecentAgentsService.shared.updateStatus(agentName: name, arguments: "", status: .success)
            }
            log(runResult.output)
            return runResult.output

        case "google_search":
            let query = cmd.argument
            log("🔍 Google search: \(query)")
            flush()
            let output = await WebAutomationService.shared.safariGoogleSearch(query: query)
            return output

        case "web_open":
            let url = cmd.argument
            log("🌐 Opening: \(url)")
            flush()
            let fullURL = url.hasPrefix("http") ? url : "https://\(url)"
            guard let parsed = URL(string: fullURL) else {
                return "Error: Invalid URL '\(fullURL)'"
            }
            do {
                let output = try await WebAutomationService.shared.open(url: parsed)
                return output
            } catch {
                return "Error: \(error.localizedDescription)"
            }

        case "web_open_and_search":
            // Format: "url|||query"
            let parts = cmd.argument.components(separatedBy: "|||")
            let url = parts.first ?? ""
            let query = parts.count > 1 ? parts[1] : ""
            log("🌐 Opening: \(url)")
            flush()
            let fullURL = url.hasPrefix("http") ? url : "https://\(url)"
            if let parsed = URL(string: fullURL) {
                do { _ = try await WebAutomationService.shared.open(url: parsed) } catch {}
            }
            // Wait for page load
            try? await Task.sleep(for: .seconds(3))
            log("🔍 Searching page for: \(query)")
            flush()
            let searchResult = await WebAutomationService.shared.safariSiteSearch(query: query)
            return searchResult

        case "web_scan":
            log("🔍 Scanning interactive elements...")
            flush()
            let elements = await WebAutomationService.shared.scanInteractiveElements()
            return elements

        case "web_read":
            log("📖 Reading page...")
            flush()
            let url = await WebAutomationService.shared.getPageURL()
            let title = await WebAutomationService.shared.getPageTitle()
            let content = await WebAutomationService.shared.readPageContent(maxLength: 3000)
            return """
                {"url": "\(WebAutomationService.escapeJS(url))", \
                "title": "\(WebAutomationService.escapeJS(title))", \
                "content": "\(WebAutomationService.escapeJS(content))"}
                """

        case "web_click":
            let selector = cmd.argument
            log("👆 Clicking: \(selector)")
            flush()
            do {
                return try await WebAutomationService.shared.click(selector: selector, strategy: .javascript)
            } catch {
                return "Error: \(error.localizedDescription)"
            }

        case "web_type":
            // argument format: "selector|text"
            let parts = cmd.argument.components(separatedBy: "|")
            guard parts.count >= 2 else { return "Error: format is selector|text" }
            let selector = parts[0].trimmingCharacters(in: .whitespaces)
            let text = parts.dropFirst().joined(separator: "|").trimmingCharacters(in: .whitespaces)
            log("⌨️ Typing into \(selector): \(text.prefix(50))")
            flush()
            do {
                return try await WebAutomationService.shared.type(text: text, selector: selector, strategy: .javascript)
            } catch {
                return "Error: \(error.localizedDescription)"
            }

        case "web_page_search":
            let query = cmd.argument
            log("🔍 Page search: \(query)")
            flush()
            let js = "window.find('\(query.replacingOccurrences(of: "'", with: "\\'"))')"
            do {
                let result = try await WebAutomationService.shared.executeJavaScript(script: js)
                let output = (result as? String) ?? "Searched for '\(query)'"
                return output
            } catch {
                return "Error: \(error.localizedDescription)"
            }

        case "web_js":
            let script = cmd.argument
            log("📜 Running JS...")
            flush()
            do {
                let result = try await WebAutomationService.shared.executeJavaScript(script: script)
                return result as? String ?? "(no output)"
            } catch {
                return "Error: \(error.localizedDescription)"
            }

        default:
            return ""
        }
    }

    // MARK: - Main Task Web Tool Handler

    /// Handle web tools for the main task — mirrors TabHandlers+Web logic.
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
