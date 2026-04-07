import AgentAccess
@preconcurrency import Foundation
import AgentMCP
import AgentD1F
import Cocoa

extension AgentViewModel {

    /// Handle Accessibility tool calls for tab tasks.
    func handleTabAccessibilityTool(
        tab: ScriptTab, name: String, input: [String: Any], toolId: String
    ) async -> TabToolResult {

        // Block accessibility when Safari is frontmost — use web tool instead
        if let bid = NSWorkspace.shared.frontmostApplication?.bundleIdentifier, bid == "com.apple.Safari" {
            let msg = "Error: Safari is active. Use the web tool: web(action: \"scan\"), web(action: \"open\", url: \"...\"), web(action: \"type\", selector: \"...\", text: \"...\"), web(action: \"click\", selector: \"...\"), web(action: \"read_content\")."
            tab.appendLog(msg)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": msg], isComplete: false)
        }

        switch name {
        case "ax_check_permission":
            let hasPermission = AccessibilityService.hasAccessibilityPermission()
            let output = hasPermission ? "Accessibility permission: granted" : "Accessibility permission: NOT granted. Use ax_request_permission to prompt the user."
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_request_permission":
            tab.appendLog("♿️ Requesting Accessibility permission...")
            let granted = AccessibilityService.requestAccessibilityPermission()
            let output = granted ? "Accessibility permission granted!" : "Accessibility permission denied. Please enable it in System Settings > Privacy & Security > Accessibility."
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_list_windows":
            let limit = input["limit"] as? Int ?? 50
            tab.appendLog("📋 windows (limit: \(limit))...")
            tab.flush()
            let output = AccessibilityService.shared.listWindows(limit: limit)
            tab.appendLog(Self.preview(output, lines: 20))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_inspect_element":
            guard let xVal = input["x"] as? Double,
                  let yVal = input["y"] as? Double else {
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "Error: x and y coordinates are required"],
                    isComplete: false
                )
            }
            let x = CGFloat(xVal)
            let y = CGFloat(yVal)
            let depth = input["depth"] as? Int ?? 3
            tab.appendLog("♿️ Inspecting element at (\(x), \(y))...")
            tab.flush()
            let output = AccessibilityService.shared.inspectElementAt(x: x, y: y, depth: depth)
            tab.appendLog(Self.preview(output, lines: 30))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_get_properties":
            let role = input["role"] as? String
            let title = input["title"] as? String
            let value = input["value"] as? String
            let appBundleId = input["appBundleId"] as? String
            let x = (input["x"] as? Double).map { CGFloat($0) }
            let y = (input["y"] as? Double).map { CGFloat($0) }
            tab.appendLog("📋 element properties...")
            tab.flush()
            let output = 
                AccessibilityService.shared.getElementProperties(
                    role: role, title: title, value: value, appBundleId: appBundleId, x: x, y: y
                )
            tab.appendLog(Self.preview(output, lines: 30))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_perform_action":
            let action = input["action"] as? String ?? ""
            let role = input["role"] as? String
            let title = input["title"] as? String
            let value = input["value"] as? String
            let appBundleId = input["appBundleId"] as? String
            let x = (input["x"] as? Double).map { CGFloat($0) }
            let y = (input["y"] as? Double).map { CGFloat($0) }
            tab.appendLog("⚡ \(action)...")
            tab.flush()
            let output = 
                AccessibilityService.shared.performAction(
                    role: role, title: title, value: value, appBundleId: appBundleId, x: x, y: y,
                    action: action
                )
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_type_text":
            let text = input["text"] as? String ?? ""
            let x = (input["x"] as? Double).map { CGFloat($0) }
            let y = (input["y"] as? Double).map { CGFloat($0) }
            tab.appendLog("⌨️ \(text.count) characters...")
            tab.flush()
            let output = 
                AccessibilityService.shared.typeText(text, at: x, y: y)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_click":
            guard let xVal = input["x"] as? Double,
                  let yVal = input["y"] as? Double else {
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "Error: x and y coordinates are required"],
                    isComplete: false
                )
            }
            let x = CGFloat(xVal)
            let y = CGFloat(yVal)
            let button = input["button"] as? String ?? "left"
            let clicks = input["clicks"] as? Int ?? 1
            tab.appendLog("♿️ Clicking at (\(x), \(y))...")
            tab.flush()
            let output = 
                AccessibilityService.shared.clickAt(x: x, y: y, button: button, clicks: clicks)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_scroll":
            guard let xVal = input["x"] as? Double,
                  let yVal = input["y"] as? Double else {
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "Error: x and y coordinates are required"],
                    isComplete: false
                )
            }
            let x = CGFloat(xVal)
            let y = CGFloat(yVal)
            let deltaX = input["deltaX"] as? Int ?? 0
            let deltaY = input["deltaY"] as? Int ?? 0
            tab.appendLog("♿️ Scrolling at (\(x), \(y))...")
            tab.flush()
            let output = 
                AccessibilityService.shared.scrollAt(x: x, y: y, deltaX: deltaX, deltaY: deltaY)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_press_key":
            guard let keyCodeVal = input["keyCode"] as? Int else {
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "Error: keyCode is required"],
                    isComplete: false
                )
            }
            let keyCode = UInt16(keyCodeVal)
            let modifiers = input["modifiers"] as? [String] ?? []
            tab.appendLog("♿️ Pressing key code: \(keyCodeVal)...")
            tab.flush()
            let output = 
                AccessibilityService.shared.pressKey(virtualKey: keyCode, modifiers: modifiers)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_screenshot":
            let x = (input["x"] as? Double).map { CGFloat($0) }
            let y = (input["y"] as? Double).map { CGFloat($0) }
            let width = (input["width"] as? Double).map { CGFloat($0) }
            let height = (input["height"] as? Double).map { CGFloat($0) }
            let windowId = input["windowId"] as? Int

            tab.appendLog("📸 screenshot...")
            tab.flush()

            let output: String
            if let wid = windowId, wid > 0 {
                output = await Self.offMain {
                    AccessibilityService.shared.captureScreenshot(windowID: wid)
                }
            } else if let x = x, let y = y, let w = width, let h = height {
                output = await Self.offMain {
                    AccessibilityService.shared.captureScreenshot(x: x, y: y, width: w, height: h)
                }
            } else {
                output = await MainActor.run {
                    AccessibilityService.shared.captureAllWindows()
                }
            }

            if output.contains("\"path\"") {
                tab.appendLog("♿️ Screenshot captured successfully")
            } else {
                tab.appendLog(output)
            }
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_get_audit_log":
            let limit = input["limit"] as? Int ?? 50
            tab.appendLog("📋 audit log...")
            tab.flush()
            let output = 
                AccessibilityService.shared.getAuditLog(limit: limit)
            tab.appendLog(Self.preview(output, lines: 30))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_set_properties":
            guard let propertiesInput = input["properties"] as? [String: Any], !propertiesInput.isEmpty else {
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "Error: properties dictionary is required"],
                    isComplete: false
                )
            }
            let role = input["role"] as? String
            let title = input["title"] as? String
            let value = input["value"] as? String
            let appBundleId = input["appBundleId"] as? String
            let x = (input["x"] as? Double).map { CGFloat($0) }
            let y = (input["y"] as? Double).map { CGFloat($0) }
            tab.appendLog("⚙️ element properties...")
            tab.flush()
            // Serialize and deserialize to avoid Sendable issues
            let propertiesData = try? JSONSerialization.data(withJSONObject: propertiesInput)
            let output: String
            if let data = propertiesData,
               let properties = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                output = AccessibilityService.shared.setProperties(
                    role: role, title: title, value: value, appBundleId: appBundleId, x: x, y: y,
                    properties: properties
                )
            } else {
                output = "{\"success\": false, \"error\": \"Failed to serialize properties\"}"
            }
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_find_element":
            let role = input["role"] as? String
            let title = input["title"] as? String
            let value = input["value"] as? String
            let appBundleId = input["appBundleId"] as? String
            let timeout = input["timeout"] as? Double ?? 5.0
            tab.appendLog("🔍 Finding element...")
            tab.flush()
            let output = await MainActor.run {
                AccessibilityService.shared.findElement(
                    role: role, title: title, value: value, appBundleId: appBundleId, timeout: timeout
                )
            }
            tab.appendLog(Self.preview(output, lines: 30))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_get_focused_element":
            let appBundleId = input["appBundleId"] as? String
            tab.appendLog("🎯 focused element...")
            tab.flush()
            let output = 
                AccessibilityService.shared.getFocusedElement(appBundleId: appBundleId)
            tab.appendLog(Self.preview(output, lines: 30))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_get_children":
            let role = input["role"] as? String
            let title = input["title"] as? String
            let value = input["value"] as? String
            let appBundleId = input["appBundleId"] as? String
            let x = (input["x"] as? Double).map { CGFloat($0) }
            let y = (input["y"] as? Double).map { CGFloat($0) }
            let depth = input["depth"] as? Int ?? 3
            tab.appendLog("📋 element children...")
            tab.flush()
            let output = 
                AccessibilityService.shared.getChildren(
                    role: role, title: title, value: value, appBundleId: appBundleId, x: x, y: y, depth: depth
                )
            tab.appendLog(Self.preview(output, lines: 30))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_drag":
            guard let fromXVal = input["fromX"] as? Double,
                  let fromYVal = input["fromY"] as? Double,
                  let toXVal = input["toX"] as? Double,
                  let toYVal = input["toY"] as? Double else {
                return TabToolResult(
                    toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": "Error: fromX, fromY, toX, toY coordinates are required"],
                    isComplete: false
                )
            }
            let fromX = CGFloat(fromXVal)
            let fromY = CGFloat(fromYVal)
            let toX = CGFloat(toXVal)
            let toY = CGFloat(toYVal)
            let button = input["button"] as? String ?? "left"
            tab.appendLog("🖱️ drag (\(fromX), \(fromY)) → (\(toX), \(toY))...")
            tab.flush()
            let output = await MainActor.run {
                AccessibilityService.shared.drag(fromX: fromX, fromY: fromY, toX: toX, toY: toY, button: button)
            }
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_wait_for_element":
            let role = input["role"] as? String
            let title = input["title"] as? String
            let value = input["value"] as? String
            let appBundleId = input["appBundleId"] as? String
            let timeout = input["timeout"] as? Double ?? 10.0
            let pollInterval = input["pollInterval"] as? Double ?? 0.5
            tab.appendLog("⏳ element (timeout: \(timeout)s)...")
            tab.flush()
            let output = await MainActor.run {
                AccessibilityService.shared.waitForElement(
                    role: role, title: title, value: value, appBundleId: appBundleId, timeout: timeout, pollInterval: pollInterval
                )
            }
            tab.appendLog(Self.preview(output, lines: 30))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_click_element":
            let role = input["role"] as? String
            let title = input["title"] as? String
            let value = input["value"] as? String
            let appBundleId = input["appBundleId"] as? String
            let timeout = input["timeout"] as? Double ?? 5.0
            let verify = input["verify"] as? Bool ?? false
            tab.appendLog("👆 element (role: \(role ?? "any"), title: \(title ?? "any"))...")
            tab.flush()
            let output = await MainActor.run {
                AccessibilityService.shared.clickElement(
                    role: role, title: title, value: value, appBundleId: appBundleId, timeout: timeout, verify: verify
                )
            }
            tab.appendLog(Self.preview(output, lines: 30))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_wait_adaptive":
            let role = input["role"] as? String
            let title = input["title"] as? String
            let value = input["value"] as? String
            let appBundleId = input["appBundleId"] as? String
            let timeout = input["timeout"] as? Double ?? 10.0
            let initialDelay = input["initialDelay"] as? Double ?? 0.1
            let maxDelay = input["maxDelay"] as? Double ?? 1.0
            tab.appendLog("⏳ element (adaptive, timeout: \(timeout)s)...")
            tab.flush()
            let output = await MainActor.run {
                AccessibilityService.shared.waitForElementAdaptive(
                    role: role, title: title, value: value, appBundleId: appBundleId, timeout: timeout,
                    initialDelay: initialDelay, maxDelay: maxDelay
                )
            }
            tab.appendLog(Self.preview(output, lines: 30))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_type_into_element":
            let role = input["role"] as? String
            let title = input["title"] as? String
            let text = input["text"] as? String ?? ""
            let appBundleId = input["appBundleId"] as? String
            let verify = input["verify"] as? Bool ?? true
            tab.appendLog("⌨️ \(text.count) chars into element...")
            tab.flush()
            let output = await MainActor.run {
                AccessibilityService.shared.typeTextIntoElement(
                    role: role, title: title, text: text, appBundleId: appBundleId, verify: verify
                )
            }
            tab.appendLog(Self.preview(output, lines: 30))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_highlight_element":
            let role = input["role"] as? String
            let title = input["title"] as? String
            let value = input["value"] as? String
            let appBundleId = input["appBundleId"] as? String
            let x = (input["x"] as? Double).map { CGFloat($0) }
            let y = (input["y"] as? Double).map { CGFloat($0) }
            let duration = input["duration"] as? Double ?? 2.0
            let color = input["color"] as? String ?? "green"
            tab.appendLog("✨ element (duration: \(duration)s, color: \(color))...")
            tab.flush()
            let output = 
                AccessibilityService.shared.highlightElement(
                    role: role, title: title, value: value, appBundleId: appBundleId,
                    x: x, y: y, duration: duration, color: color
                )
            tab.appendLog(Self.preview(output, lines: 30))
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_get_window_frame":
            let windowId = input["windowId"] as? Int ?? 0
            tab.appendLog("📐 window \(windowId) frame...")
            tab.flush()
            let output = 
                AccessibilityService.shared.getWindowFrame(windowId: windowId)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_show_menu":
            let role = input["role"] as? String
            let title = input["title"] as? String
            let value = input["value"] as? String
            let appBundleId = input["appBundleId"] as? String
            let x = (input["x"] as? Double).map { CGFloat($0) }
            let y = (input["y"] as? Double).map { CGFloat($0) }
            tab.appendLog("📋 context menu...")
            tab.flush()
            let output = 
                AccessibilityService.shared.showMenu(
                    role: role, title: title, value: value, appBundleId: appBundleId, x: x, y: y
                )
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(
                toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output],
                isComplete: false
            )

        case "ax_click_menu_item":
            let app = input["app"] as? String
            let menuPath = input["menu_path"] as? [String] ?? []
            tab.appendLog("👆 menu: \(menuPath.joined(separator: " > "))...")
            tab.flush()
            let output = 
                AccessibilityService.shared.clickMenuItem(appBundleId: app, menuPath: menuPath)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "ax_set_window_frame":
            let app = input["app"] as? String
            let x = (input["x"] as? Double).map { CGFloat($0) }
            let y = (input["y"] as? Double).map { CGFloat($0) }
            let width = (input["width"] as? Double).map { CGFloat($0) }
            let height = (input["height"] as? Double).map { CGFloat($0) }
            tab.appendLog("⚙️ window frame...")
            tab.flush()
            let output = 
                AccessibilityService.shared.setWindowFrame(appBundleId: app, x: x, y: y, width: width, height: height)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "ax_manage_app":
            let action = input["action"] as? String ?? "list"
            let bundleId = input["bundleId"] as? String
            let appName = input["name"] as? String
            tab.appendLog("📱 \(action)...")
            tab.flush()
            let output = 
                AccessibilityService.shared.manageApp(action: action, bundleId: bundleId, name: appName)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "ax_scroll_to_element":
            let role = input["role"] as? String
            let title = input["title"] as? String
            let app = input["app"] as? String ?? input["appBundleId"] as? String
            tab.appendLog("📜 scroll to element...")
            tab.flush()
            let output = await MainActor.run {
                AccessibilityService.shared.scrollToElement(role: role, title: title, appBundleId: app)
            }
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        case "ax_read_focused":
            let app = input["app"] as? String ?? input["appBundleId"] as? String
            tab.appendLog("🎯 reading focused element...")
            tab.flush()
            let output = 
                AccessibilityService.shared.readFocusedElement(appBundleId: app)
            tab.appendLog(output)
            tab.flush()
            return TabToolResult(toolResult: ["type": "tool_result", "tool_use_id": toolId, "content": output], isComplete: false)

        default:
        let output = await executeNativeTool(name, input: input)
        tab.appendLog(output); tab.flush()
        return tabResult(output, toolId: toolId)
        }
    }
}
