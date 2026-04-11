
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import AgentAccess
import Cocoa




// MARK: - Native Tool Handler (Apple AI)

extension AgentViewModel {


    // MARK: - Native Tool Handler (Apple AI)

    /// Executes a tool call from Apple AI's Foundation Models native tool system.
    /// Routes to the same execution logic as TaskExecution tool handlers.
    func executeNativeTool(_ rawName: String, input rawInput: sending [String: Any]) async -> String {
        // Expand consolidated CRUDL tools into legacy tool names
        let (name, input) = Self.expandConsolidatedTool(name: rawName, input: rawInput)
        NativeToolContext.toolCallCount += 1

        // Prefix-matched tools
        if let result = await handleWebTool(name: name, input: input) { return result }
        if let result = await handleSeleniumTool(name: name, input: input) { return result }
        // Saved-script CRUDL tools. expandConsolidatedTool maps applescript(action:list) → list_apple_scripts etc. Leaf names live in handleSavedScriptTool, not the main switch.
        let savedScriptNames: Set<String> = [
            "list_apple_scripts", "run_apple_script", "save_apple_script", "delete_apple_script",
            "list_javascript", "run_javascript", "save_javascript", "delete_javascript",
        ]
        if savedScriptNames.contains(name) {
            return await handleSavedScriptTool(name: name, input: input)
        }
        // ax_ accessibility tools — already expanded. expandConsolidatedTool maps accessibility(action:X) → ax_X, so ax_ names arrive here already expanded.

        // Route to per-category helpers. Each returns `nil` if it doesn't
        // recognize the tool so the dispatcher can fall through to the next.
        if let result = await handleShellNativeTool(name: name, input: input) { return result }
        if let result = await handleAgentScriptNativeTool(name: name, input: input) { return result }
        if let result = await handleFileNativeTool(name: name, input: input) { return result }
        if let result = await handleMiscNativeTool(name: name, input: input) { return result }
        if let result = await handleConversationNativeTool(name: name, input: input) { return result }
        if let result = await handleXcodeNativeTool(name: name, input: input) { return result }
        if let result = await handleGitNativeTool(name: name, input: input) { return result }

        // Handle ax_ accessibility tools directly (avoid recursion through executeNativeTool)
        if name.hasPrefix("ax_") {
            let axAction = String(name.dropFirst(3))
            var axInput = input
            axInput["action"] = axAction
            return await handleAccessibilityAction(action: axAction, input: axInput)
        }
        return "⚠️ Tool '\(rawName)' (expanded: '\(name)') not handled. Recovery: call list_tools to see available tools, or check spelling."
    }

    /// Read-only accessibility actions that should NOT auto-launch the target app. These are queries — they should fail gracefully if the app isn't running, NOT silently spawn it.
    private static let readOnlyAxActions: Set<String> = [
        "list_windows",
        "inspect_element",
        "get_properties",
        "find_element",
        "get_children",
        "get_focused_element",
        "get_window_frame",
        "screenshot",
        "wait_for_element",
        "wait",
        "highlight_element"
    ]

    /// Direct accessibility dispatch — no recursion through executeNativeTool
    private func handleAccessibilityAction(action: String, input: [String: Any]) async -> String {
        let ax = AgentAccess.AccessibilityService.shared
        let role = input["role"] as? String
        let title = input["title"] as? String
        let value = input["value"] as? String
        // Resolve app name → bundle ID. Read-only queries use lookupBundleId (NO auto-launch); write actions use
        // resolveBundleId (auto-launches if not running, since you can't click a button on an app that isn't up). This prevents speculative reads from silently opening apps the user never asked for — most visibly the "Photo Booth keeps opening" bug.
        let appNameOrId = input["appBundleId"] as? String ?? input["app"] as? String ?? input["name"] as? String
        let app = Self.readOnlyAxActions.contains(action)
            ? ax.lookupBundleId(appNameOrId)
            : ax.resolveBundleId(appNameOrId)
        let x = (input["x"] as? Double).map { CGFloat($0) }
        let y = (input["y"] as? Double).map { CGFloat($0) }

        switch action {
        case "open_app":
            // Launch/activate app and return all interactive elements in one call
            return ax.openApp(input["appBundleId"] as? String ?? input["app"] as? String ?? input["name"] as? String)
        case "list_windows":
            // If app specified, filter to just that app's windows
            if let app = app {
                return ax.listWindows(limit: input["limit"] as? Int ?? 50, appBundleId: app)
            }
            return ax.listWindows(limit: input["limit"] as? Int ?? 50)
        case "inspect_element":
            // If role/title provided, find element first then inspect at its position
            if (role != nil || title != nil), x == nil, y == nil {
                return ax.getElementProperties(role: role, title: title, value: value, appBundleId: app, x: nil, y: nil)
            }
            return ax.inspectElementAt(x: x ?? 0, y: y ?? 0, depth: input["depth"] as? Int ?? 3)
        case "get_properties":
            return ax.getElementProperties(role: role, title: title, value: value, appBundleId: app, x: x, y: y)
        case "perform_action":
            return ax.performAction(
                role: role, title: title, value: value,
                appBundleId: app, x: x, y: y,
                action: input["ax_action"] as? String ?? "")
        case "type_text", "type_into_element":
            // AXorcist-only: typing requires an element. There is no "type at the
            // current focus" path — find the text field by role/title first.
            return ax.typeTextIntoElement(
                role: role, title: title,
                text: input["text"] as? String ?? "",
                appBundleId: app,
                verify: input["verify"] as? Bool ?? true)
        case "click", "click_element":
            // AXorcist-only. Coordinate-based click is not supported — provide role/title/value (and ideally
            // appBundleId) so the click goes through AXorcist's element-finder.
            return ax.clickElement(
                role: role, title: title, value: value,
                appBundleId: app,
                timeout: input["timeout"] as? Double ?? 5,
                verify: input["verify"] as? Bool ?? false)
        case "scroll", "scroll_to_element":
            // AXorcist-only: scroll to an element by role/title. The old coordinate
            // path through InputDriver was removed.
            return ax.scrollToElement(
                role: role, title: title, appBundleId: app)
        case "press_key":
            // press_key is no longer supported — AXorcist doesn't drive raw key events and the InputDriver path was
            // removed. Use clickElement for buttons or clickMenuItem for keyboard-shortcut menu commands.
            return """
                Error: press_key is removed. Find the relevant button \
                via accessibility(action:"click_element", \
                role:"AXButton", title:..., appBundleId:...) or invoke \
                the menu command via accessibility(action:"click_menu_item", \
                appBundleId:..., menuPath:"File > Save").
                """
        case "drag":
            // drag is no longer supported — see the AccessibilityService+Interaction
            // comment for the removal rationale and AXorcist-based alternatives.
            return """
                Error: drag is removed. For window move/resize use \
                accessibility(action:"set_window_frame", \
                appBundleId:..., x:, y:, width:, height:). \
                For sliders use accessibility(action:"set_properties", \
                role:"AXSlider", ...).
                """
        case "screenshot":
            // All three paths are async — they dispatch screencapture to a background queue so the main thread stays
            // responsive while the ~100ms screencapture process runs.
            let w = (input["width"] as? Double).map { CGFloat($0) }
            let h = (input["height"] as? Double).map { CGFloat($0) }
            if let wid = input["windowId"] as? Int, wid > 0 {
                return await ax.captureScreenshot(windowID: wid)
            } else if let x, let y, let w, let h {
                return await ax.captureScreenshot(x: x, y: y, width: w, height: h)
            } else {
                return await ax.captureAllWindows()
            }
        case "find_element":
            return ax.findElement(
                role: role, title: title, value: value,
                appBundleId: app,
                timeout: input["timeout"] as? Double ?? 5)
        case "get_focused_element":
            return ax.getFocusedElement(appBundleId: app)
        case "get_children":
            return ax.getChildren(
                role: role, title: title, value: value,
                appBundleId: app, x: x, y: y,
                depth: input["depth"] as? Int ?? 3)
        case "get_audit_log":
            return ax.getAuditLog(limit: input["limit"] as? Int ?? 50)
        case "wait_for_element":
            return ax.waitForElement(
                role: role, title: title, value: value,
                appBundleId: app,
                timeout: input["timeout"] as? Double ?? 10,
                pollInterval: input["pollInterval"] as? Double ?? 0.5)
        case "wait_adaptive":
            return ax.waitForElementAdaptive(
                role: role, title: title, value: value,
                appBundleId: app,
                timeout: input["timeout"] as? Double ?? 10)
        case "manage_app":
            let manageAction = input["sub_action"] as? String
                ?? { let a = input["action"] as? String ?? "list"; return a == "manage_app" ? "list" : a }()
            return ax.manageApp(
                action: manageAction,
                bundleId: input["bundleId"] as? String,
                name: input["name"] as? String ?? input["app"] as? String)
        case "set_window_frame":
            let sw = (input["width"] as? Double).map { CGFloat($0) }
            let sh = (input["height"] as? Double).map { CGFloat($0) }
            return ax.setWindowFrame(
                appBundleId: app, x: x, y: y,
                width: sw, height: sh)
        case "click_menu_item":
            return ax.clickMenuItem(
                appBundleId: app,
                menuPath: (input["menuPath"] as? String)?
                    .components(separatedBy: " > ") ?? [])
        case "get_window_frame":
            return ax.getWindowFrame(
                windowId: input["windowId"] as? Int ?? 0)
        case "highlight_element":
            return ax.highlightElement(
                role: role, title: title, value: value,
                appBundleId: app, x: x, y: y,
                duration: input["duration"] as? Double ?? 2,
                color: input["color"] as? String ?? "green")
        case "show_menu":
            return ax.showMenu(
                role: role, title: title, value: value,
                appBundleId: app, x: x, y: y)
        case "read_focused":
            return ax.readFocusedElement(appBundleId: app)
        case "set_properties":
            return ax.setProperties(
                role: role, title: title, value: value,
                appBundleId: app, x: x, y: y,
                properties: input["properties"] as? [String: Any] ?? [:])
        case "clipboard":
            let clipAction = input["action"] as? String ?? "read"
            switch clipAction {
            case "read":
                let pb = NSPasteboard.general
                if let text = pb.string(forType: .string) { return text }
                if pb.data(forType: .png) != nil || pb.data(forType: .tiff) != nil {
                    return "Clipboard contains an image (use action:paste to paste it)"
                }
                return "Clipboard is empty"
            case "write":
                let text = input["text"] as? String ?? ""
                guard !text.isEmpty else { return "Error: text is required" }
                let pb = NSPasteboard.general
                pb.clearContents()
                pb.setString(text, forType: .string)
                return "Copied to clipboard: \(text.prefix(100))"
            case "paste":
                // Cmd+V via AppleScript System Events. The old path used AXorcist InputDriver hotkey which is gone.
                // NSAppleScript runs in-process with TCC and produces a real synthesized keystroke without going through CGEvent directly.
                let pasteScript = "tell application \"System Events\" to keystroke \"v\" using command down"
                var asErr: NSDictionary?
                if let script = NSAppleScript(source: pasteScript) {
                    _ = script.executeAndReturnError(&asErr)
                    if let e = asErr { return "Paste failed: \(e)" }
                    return "Pasted clipboard contents"
                }
                return "Paste failed: could not create AppleScript"
            case "copy_image":
                let path = input["file_path"] as? String ?? ""
                guard !path.isEmpty else { return "Error: file_path is required" }
                guard let imageData = try? Data(contentsOf: URL(fileURLWithPath: path)) else {
                    return "Error: cannot read image at \(path)"
                }
                let pb = NSPasteboard.general
                pb.clearContents()
                let ext = (path as NSString).pathExtension.lowercased()
                pb.setData(imageData, forType: ext == "png" ? .png : .tiff)
                return "Image copied to clipboard from \(path)"
            default:
                return "Unknown clipboard action: \(clipAction). Use read, write, paste, or copy_image."
            }
        default:
            return "Unknown accessibility action: \(action)"
        }
    }

    /// Clean HTML to readable text. Strips script/style/noscript blocks (with content), HTML comments, tags, decodes entities, collapses whitespace, drops garbage lines.
    nonisolated static func cleanHTML(_ html: String) -> String {
        var s = html
        // Remove script/style/noscript/svg blocks WITH their content
        let blockTags = ["script", "style", "noscript", "svg", "iframe", "head"]
        for tag in blockTags {
            s = s.replacingOccurrences(
                of: "<\(tag)\\b[^>]*>[\\s\\S]*?</\(tag)>",
                with: " ",
                options: .regularExpression
            )
        }
        // Strip HTML comments
        s = s.replacingOccurrences(of: "<!--[\\s\\S]*?-->", with: " ", options: .regularExpression)
        // Replace block-level tags with newlines so paragraphs break
        let blockBreak = "(?i)</(p|div|li|tr|h[1-6]|br|article|section)>"
        s = s.replacingOccurrences(of: blockBreak, with: "\n", options: .regularExpression)
        s = s.replacingOccurrences(of: "(?i)<br\\s*/?>", with: "\n", options: .regularExpression)
        // Strip all remaining tags
        s = s.replacingOccurrences(of: "<[^>]+>", with: " ", options: .regularExpression)
        // Decode common HTML entities
        let entities: [(String, String)] = [
            ("&nbsp;", " "), ("&amp;", "&"), ("&lt;", "<"), ("&gt;", ">"),
            ("&quot;", "\""), ("&#39;", "'"), ("&apos;", "'"),
            ("&mdash;", "—"), ("&ndash;", "–"), ("&hellip;", "…"),
            ("&rsquo;", "'"), ("&lsquo;", "'"), ("&ldquo;", "\""), ("&rdquo;", "\""),
        ]
        for (k, v) in entities { s = s.replacingOccurrences(of: k, with: v) }
        // Decode numeric entities like &#1234; and &#xABCD;
        s = s.replacingOccurrences(of: "&#x?[0-9a-fA-F]+;", with: " ", options: .regularExpression)
        // Filter out garbage lines
        let lines = s.components(separatedBy: "\n")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { line in
                guard !line.isEmpty else { return false }
                // Skip CSS variable declarations and JS/JSON-like junk
                if line.hasPrefix("--") && line.contains(":") { return false }
                if line.hasPrefix("{") || line.hasPrefix("[") { return false }
                if line.hasPrefix(":root") || line.hasPrefix("@media") { return false }
                // Skip lines that are mostly punctuation/symbols
                let alphanumCount = line.unicodeScalars.filter { CharacterSet.alphanumerics.contains($0) }.count
                if alphanumCount * 3 < line.count { return false }
                return true
            }
        // Collapse runs of whitespace within each line
        let collapsed = lines.map { line -> String in
            line.replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
        }
        // Collapse runs of 3+ blank lines to 2
        var result = collapsed.joined(separator: "\n")
        result = result.replacingOccurrences(of: "\n{3,}", with: "\n\n", options: .regularExpression)
        return result.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
