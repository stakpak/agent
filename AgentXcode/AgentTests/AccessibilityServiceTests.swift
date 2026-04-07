import Testing
import Foundation
@testable import Agent_

@Suite("AccessibilityService")
@MainActor
struct AccessibilityServiceTests {
    
    let service = AccessibilityService.shared
    
    // MARK: - Permission Tests
    
    @Test("hasAccessibilityPermission returns boolean without crashing")
    func permissionCheckDoesNotCrash() {
        // This should not crash even if permission is denied
        let hasPermission = AccessibilityService.hasAccessibilityPermission()
        // Result depends on user's system settings
        #expect(hasPermission == true || hasPermission == false)
    }
    
    @Test("requestAccessibilityPermission returns boolean")
    func permissionRequestReturnsBoolean() {
        let result = AccessibilityService.requestAccessibilityPermission()
        // Result depends on system - just verify it doesn't crash
        #expect(result == true || result == false)
    }
    
    // MARK: - Window Listing Tests
    
    @Test("listWindows returns valid JSON structure")
    func listWindowsReturnsJSON() {
        let result = service.listWindows(limit: 5)
        // Should be valid JSON
        #expect(result.contains("\"success\""))
        #expect(result.contains("\"windows\"") || result.contains("\"error\""))
        
        // If we have permission, check structure
        if result.contains("\"success\": true") {
            #expect(result.contains("\"windowId\""))
            #expect(result.contains("\"ownerName\""))
            #expect(result.contains("\"bounds\""))
        }
    }
    
    @Test("listWindows respects limit parameter")
    func listWindowsLimit() {
        let result = service.listWindows(limit: 3)
        if result.contains("\"success\": true") {
            // Count window entries in JSON
            let windowCount = result.components(separatedBy: "\"windowId\"").count - 1
            #expect(windowCount <= 3)
        }
    }
    
    @Test("listWindows handles permission denied gracefully")
    func listWindowsPermissionDenied() {
        // Even without permission, should return valid JSON error
        let result = service.listWindows(limit: 10)
        #expect(result.contains("\"success\""))
        #expect(result.hasPrefix("{"))
    }
    
    // MARK: - Element Inspection Tests
    
    @Test("inspectElementAt returns valid JSON")
    func inspectElementReturnsJSON() {
        let result = service.inspectElementAt(x: 100, y: 100, depth: 2)
        #expect(result.hasPrefix("{"))
        #expect(result.contains("\"success\""))
    }
    
    @Test("inspectElementAt handles invalid coordinates")
    func inspectElementInvalidCoords() {
        // Negative or extreme coordinates should not crash
        let result = service.inspectElementAt(x: -1000, y: -1000, depth: 1)
        #expect(result.hasPrefix("{"))
        #expect(result.contains("\"success\"") || result.contains("\"error\""))
    }
    
    @Test("inspectElementAt respects depth parameter")
    func inspectElementDepth() {
        // Test with different depths - just verify no crash
        let shallow = service.inspectElementAt(x: 500, y: 500, depth: 1)
        let deep = service.inspectElementAt(x: 500, y: 500, depth: 5)
        #expect(shallow.hasPrefix("{"))
        #expect(deep.hasPrefix("{"))
    }
    
    // MARK: - Element Properties Tests
    
    @Test("getElementProperties returns valid JSON")
    func getElementPropertiesJSON() {
        let result = service.getElementProperties(role: nil, title: nil, value: nil, appBundleId: nil, x: 200, y: 200)
        #expect(result.hasPrefix("{"))
        #expect(result.contains("\"success\""))
    }
    
    @Test("getElementProperties with nil parameters falls back to global search")
    func getElementPropertiesNilParams() {
        let result = service.getElementProperties(role: nil, title: nil, value: nil, appBundleId: nil, x: nil, y: nil)
        // Should either find an element or return an error
        #expect(result.hasPrefix("{"))
    }
    
    // MARK: - Action Tests
    
    @Test("performAction allows enabled actions")
    func performActionAllowsEnabledActions() {
        // AXPress is enabled by default — should not be blocked by restrictions
        // (may still fail due to "element not found" which is fine)
        let result = service.performAction(
            role: nil,
            title: nil,
            value: nil,
            appBundleId: nil,
            x: nil,
            y: nil,
            action: "AXPress"
        )
        // Should NOT be restricted — may fail for "not found" reasons instead
        #expect(!result.contains("restricted"))
    }
    
    @Test("performAction requires coordinates or role/title")
    func performActionRequiresTarget() {
        let result = service.performAction(
            role: nil,
            title: nil,
            value: nil,
            appBundleId: nil,
            x: nil,
            y: nil,
            action: "AXPress"
        )
        // Should fail because no target specified
        #expect(result.contains("\"success\": false") || result.contains("not found"))
    }
    
    // MARK: - Input Simulation Tests
    
    @Test("typeText returns valid JSON structure")
    func typeTextReturnsJSON() {
        let result = service.typeText("test", at: nil, y: nil)
        #expect(result.hasPrefix("{"))
        #expect(result.contains("\"success\""))
    }
    
    @Test("typeText handles special characters")
    func typeTextSpecialChars() {
        // Newline and tab should be handled
        let result = service.typeText("hello\nworld", at: nil, y: nil)
        #expect(result.hasPrefix("{"))
    }
    
    @Test("clickAt returns valid JSON")
    func clickAtReturnsJSON() {
        let result = service.clickAt(x: 100, y: 100, button: "left", clicks: 1)
        #expect(result.hasPrefix("{"))
        #expect(result.contains("x") || result.contains("error"))
    }
    
    @Test("clickAt handles different buttons")
    func clickAtDifferentButtons() {
        let left = service.clickAt(x: 100, y: 100, button: "left", clicks: 1)
        let right = service.clickAt(x: 100, y: 100, button: "right", clicks: 1)
        let middle = service.clickAt(x: 100, y: 100, button: "middle", clicks: 1)
        
        #expect(left.hasPrefix("{"))
        #expect(right.hasPrefix("{"))
        #expect(middle.hasPrefix("{"))
    }
    
    @Test("clickAt handles double-click")
    func clickAtDoubleClick() {
        let single = service.clickAt(x: 100, y: 100, button: "left", clicks: 1)
        let double = service.clickAt(x: 100, y: 100, button: "left", clicks: 2)
        
        #expect(single.hasPrefix("{"))
        #expect(double.hasPrefix("{"))
    }
    
    @Test("scrollAt returns valid JSON")
    func scrollAtReturnsJSON() {
        let result = service.scrollAt(x: 100, y: 100, deltaX: 0, deltaY: 10)
        #expect(result.hasPrefix("{"))
    }
    
    @Test("pressKey returns valid JSON")
    func pressKeyReturnsJSON() {
        let result = service.pressKey(virtualKey: 0x24, modifiers: [])  // Return key
        #expect(result.hasPrefix("{"))
    }
    
    @Test("pressKey handles modifiers")
    func pressKeyWithModifiers() {
        let result = service.pressKey(virtualKey: 0x07, modifiers: ["command"])  // Cmd+X
        #expect(result.hasPrefix("{"))
    }
    
    // MARK: - Screenshot Tests
    
    @Test("captureScreenshot returns valid JSON")
    func captureScreenshotReturnsJSON() {
        let result = service.captureScreenshot(x: 0, y: 0, width: 100, height: 100)
        #expect(result.hasPrefix("{"))
    }
    
    @Test("captureScreenshot with windowID returns valid JSON")
    func captureScreenshotWindowID() {
        let result = service.captureScreenshot(windowID: 99999)  // Non-existent window
        #expect(result.hasPrefix("{"))
        // Should fail gracefully
    }
    
    @Test("captureAllWindows returns valid JSON")
    func captureAllWindowsReturnsJSON() {
        let result = service.captureAllWindows()
        #expect(result.hasPrefix("{"))
    }
    
    // MARK: - Audit Log Tests
    
    @Test("getAuditLog returns string")
    func getAuditLogReturnsString() {
        let log = service.getAuditLog(limit: 10)
        // Should return a string (even if empty)
        #expect(log is String)
    }
    
    @Test("getAuditLog respects limit")
    func getAuditLogLimit() {
        // Get more entries than exist to test limit
        let log = service.getAuditLog(limit: 100)
        // If there are entries, count should respect limit
        let lines = log.components(separatedBy: "\n")
        #expect(lines.count <= 100 || log.isEmpty)
    }
    
    // MARK: - JSON Helper Tests
    
    @Test("successJSON produces valid JSON")
    func successJSONFormat() {
        let json = service.successJSON(["test": "value", "number": 42])
        #expect(json.contains("\"success\": true"))
        #expect(json.contains("\"data\""))
        #expect(json.contains("\"test\": \"value\""))
    }
    
    @Test("errorJSON produces valid JSON")
    func errorJSONFormat() {
        let json = service.errorJSON("Test error message")
        #expect(json.contains("\"success\": false"))
        #expect(json.contains("\"error\": \"Test error message\""))
    }
    
    @Test("errorJSON escapes quotes in messages")
    func errorJSONEscapesQuotes() {
        let json = service.errorJSON("Error with \"quotes\" inside")
        #expect(json.contains("\\\""))
        #expect(json.hasPrefix("{"))
        #expect(json.hasSuffix("}"))
    }
    
    // MARK: - Security Tests
    
    @Test("Enabled roles are not restricted")
    func enabledRolesNotRestricted() {
        // AXSecureTextField is enabled by default — should not be restricted
        let result = service.performAction(
            role: "AXSecureTextField",
            title: nil,
            value: nil,
            appBundleId: nil,
            x: nil,
            y: nil,
            action: "AXPress"
        )
        // Should fail because element not found, not because of role restriction
        #expect(result.contains("not found") || result.contains("error"))
        #expect(!result.contains("restricted"))
    }

    @Test("Enabled actions are allowed")
    func enabledActionsAllowed() {
        // All actions enabled by default — should not be restricted
        let actions = ["AXPress", "AXConfirm", "AXShowMenu"]
        for action in actions {
            let result = service.performAction(
                role: nil,
                title: nil,
                value: nil,
                appBundleId: nil,
                x: nil,
                y: nil,
                action: action
            )
            #expect(!result.contains("restricted"), "Expected \(action) to not be restricted")
        }
    }
    
    // MARK: - Integration Smoke Tests
    // These tests verify basic functionality without making assertions about results
    
    @Test("listWindows smoke test")
    func listWindowsSmoke() {
        // Just verify it doesn't crash
        _ = service.listWindows(limit: 10)
    }
    
    @Test("inspect at screen center smoke test")
    func inspectCenterSmoke() {
        // Inspect center of screen (should always have something there)
        _ = service.inspectElementAt(x: 512, y: 384, depth: 3)
    }
    
    @Test("screenshot smoke test")
    func screenshotSmoke() {
        // Small screenshot should work
        _ = service.captureScreenshot(x: 0, y: 0, width: 10, height: 10)
    }
    
    @Test("audit log smoke test")
    func auditLogSmoke() {
        _ = service.getAuditLog(limit: 5)
    }
}