import AgentAccess
import AgentAudit
import Foundation
import AppKit

/// Unified web automation service that combines Accessibility, AppleScript/JS, and Selenium.
/// Auto-selects the best strategy based on the browser and operation.
/// Phase 2 Implementation: Unified API with caching and fuzzy matching.
final class WebAutomationService: @unchecked Sendable {
    static let shared = WebAutomationService()
    
    // MARK: - JavaScript Escaping

    /// Escape a string for embedding in JavaScript string literals (single or double quoted).
    static func escapeJS(_ str: String) -> String {
        str.replacingOccurrences(of: "\\", with: "\\\\")
           .replacingOccurrences(of: "\"", with: "\\\"")
           .replacingOccurrences(of: "'", with: "\\'")
           .replacingOccurrences(of: "\n", with: "\\n")
           .replacingOccurrences(of: "\r", with: "\\r")
           .replacingOccurrences(of: "\t", with: "\\t")
           .replacingOccurrences(of: "\0", with: "")
    }

    /// Escape JavaScript for embedding inside AppleScript `do JavaScript "..."`.
    /// AppleScript only needs `\` and `"` escaped — single quotes are fine as-is.
    static func escapeJSForAppleScript(_ str: String) -> String {
        str.replacingOccurrences(of: "\\", with: "\\\\")
           .replacingOccurrences(of: "\"", with: "\\\"")
           .replacingOccurrences(of: "\n", with: "\\n")
           .replacingOccurrences(of: "\r", with: "\\r")
           .replacingOccurrences(of: "\t", with: "\\t")
           .replacingOccurrences(of: "\0", with: "")
    }

    /// Properly escape a string for embedding in a JSON value (for Selenium args).
    /// Uses JSONSerialization for correctness.
    static func escapeJSON(_ str: String) -> String {
        if let data = try? JSONSerialization.data(withJSONObject: str),
           let json = String(data: data, encoding: .utf8) {
            // JSONSerialization wraps in quotes — strip them
            return String(json.dropFirst().dropLast())
        }
        // Fallback to manual escaping
        return escapeJS(str)
    }

    // MARK: - Element Cache
    
    /// Cache for element lookups to reduce repeated searches
    nonisolated(unsafe) var elementCache: [String: CachedElement] = [:]
    let cacheLock = NSLock()
    let cacheTTL: TimeInterval = automationMaxDelay
    
    struct CachedElement {
        let element: [String: Any]
        let timestamp: Date
        let role: String?
        let title: String?
        let value: String?
        let bounds: CGRect
        let source: ElementSource
    }
    
    enum ElementSource: String {
        case accessibility
        case javascript
        case selenium
    }
    
    enum BrowserType: String {
        case safari = "com.apple.Safari"
        case chrome = "com.google.Chrome"
        case firefox = "org.mozilla.firefox"
        case edge = "com.microsoft.edgemac"
    }
    
    // MARK: - Unified API
    
    /// Open a URL in the specified browser. Returns immediately after the URL is sent — no page load wait.
    func open(url: URL, browser: BrowserType = .safari, waitForLoad: Bool = false) async throws -> String {
        // Try AppleScript first (fastest, most reliable)
        if let result = try? await openViaAppleScript(url: url, browser: browser) {
            if waitForLoad {
                await waitForPageReady(browser: browser.rawValue, timeout: 3)
            }
            return result
        }

        // Fallback to opening via NSWorkspace
        NSWorkspace.shared.open(url)
        if waitForLoad {
            try? await Task.sleep(for: .seconds(1))
        }
        return "Opened \(url.absoluteString) in default browser"
    }

    /// Wait for the current page to finish loading (document.readyState == "complete")
    func waitForPageReady(browser: String? = nil, timeout: TimeInterval = 3) async {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let start = CFAbsoluteTimeGetCurrent()
        while CFAbsoluteTimeGetCurrent() - start < timeout {
            if let result = try? await executeJavaScript(script: "document.readyState", browser: browserId) as? String,
               result == "complete" {
                return
            }
            try? await Task.sleep(for: .milliseconds(300))
        }
    }

    /// Read the text content of the current web page
    func readPageContent(browser: String? = nil, maxLength: Int = 10000) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let js = "(function(){ var t = document.body.innerText; return t ? t.substring(0, \(maxLength)) : ''; })()"
        if let result = try? await executeJavaScript(script: js, browser: browserId) as? String {
            return result
        }
        return "Error: could not read page content"
    }

    /// Read the current page URL
    func getPageURL(browser: String? = nil) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        if let result = try? await executeJavaScript(script: "window.location.href", browser: browserId) as? String {
            return result
        }
        return "Error: could not get page URL"
    }

    /// Read the current page title
    func getPageTitle(browser: String? = nil) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        if let result = try? await executeJavaScript(script: "document.title", browser: browserId) as? String {
            return result
        }
        return "Error: could not get page title"
    }
    
    /// Find an element using the best available strategy
    /// - Parameters:
    ///   - selector: CSS selector, XPath, or accessibility identifier
    ///   - strategy: Auto, Accessibility, JavaScript, or Selenium
    ///   - timeout: Maximum wait time
    ///   - fuzzyThreshold: Minimum match score (0-1) for fuzzy matching
    /// - Returns: Element properties and source
    func findElement(
        selector: String,
        strategy: SelectorStrategy = .auto,
        timeout: TimeInterval = automationFinishTimeout,
        fuzzyThreshold: Double = 0.6,
        appBundleId: String? = nil
    ) async throws -> [String: Any] {
        // Check cache first
        let cacheKey = "find_\(selector)_\(appBundleId ?? "")"
        if let cached = getCachedElement(key: cacheKey) {
            return cached
        }
        
        var result: [String: Any]?
        var source: ElementSource = .accessibility
        
        switch strategy {
        case .auto:
            let browserId = appBundleId ?? detectActiveBrowser()
            let isBrowser = browserId != nil && ["com.apple.Safari", "com.google.Chrome", "org.mozilla.firefox", "com.microsoft.edgemac"].contains(browserId!)

            if isBrowser {
                // Web page: JS only (fast), skip accessibility (too slow on browser AX trees)
                if let jsResult = try? await findViaJavaScript(selector: selector, browser: browserId!) {
                    result = jsResult
                    source = .javascript
                }
            } else {
                // Native app: accessibility first
                if let element = try? await findViaAccessibility(selector: selector, timeout: min(timeout, 3), appBundleId: appBundleId) {
                    result = element
                    source = .accessibility
                }
            }

            // Fall back to Selenium if nothing found
            if result == nil {
                if let seleniumResult = try? await findViaSelenium(selector: selector, timeout: timeout) {
                    result = seleniumResult
                    source = .selenium
                }
            }
            
        case .accessibility:
            result = try await findViaAccessibility(selector: selector, timeout: timeout, appBundleId: appBundleId)
            source = .accessibility
            
        case .javascript:
            guard let browserId = appBundleId ?? detectActiveBrowser() else {
                throw WebAutomationError.browserNotFound
            }
            result = try await findViaJavaScript(selector: selector, browser: browserId)
            source = .javascript
            
        case .selenium:
            result = try await findViaSelenium(selector: selector, timeout: timeout)
            source = .selenium
        }
        
        guard var finalResult = result else {
            throw WebAutomationError.elementNotFound(selector)
        }
        
        finalResult["source"] = source.rawValue
        
        // Cache the result
        cacheElement(key: cacheKey, element: finalResult, source: source)
        
        return finalResult
    }
    
    /// Click an element using the best available strategy
    func click(selector: String, strategy: SelectorStrategy = .auto, appBundleId: String? = nil) async throws -> String {
        let browserId = appBundleId ?? detectActiveBrowser()
        let isBrowser = browserId != nil && ["com.apple.Safari", "com.google.Chrome", "org.mozilla.firefox", "com.microsoft.edgemac"].contains(browserId!)

        // For browsers, skip findElement and click directly via JS (much faster)
        if isBrowser && (strategy == .auto || strategy == .javascript) {
            return try await executeJavaScriptClick(selector: selector, browser: browserId!)
        }

        let element = try await findElement(selector: selector, strategy: strategy, appBundleId: appBundleId)

        guard let source = element["source"] as? String else {
            throw WebAutomationError.invalidState("No source in element")
        }

        switch ElementSource(rawValue: source) {
        case .accessibility:
            let role = element["role"] as? String
            let title = element["title"] as? String
            let result = await MainActor.run {
                AccessibilityService.shared.clickElement(
                    role: role,
                    title: title,
                    value: nil,
                    appBundleId: appBundleId,
                    timeout: automationFinishTimeout,
                    verify: false
                )
            }
            return result

        case .javascript:
            guard let bid = browserId else {
                throw WebAutomationError.browserNotFound
            }
            return try await executeJavaScriptClick(selector: selector, browser: bid)

        case .selenium:
            return try await seleniumClick(selector: selector)

        case .none:
            throw WebAutomationError.invalidState("Unknown source: \(source)")
        }
    }
    
    /// Type text into an element using the best available strategy
    func type(text: String, selector: String, strategy: SelectorStrategy = .auto, verify: Bool = true, appBundleId: String? = nil) async throws -> String {
        let browserId = appBundleId ?? detectActiveBrowser()
        let isBrowser = browserId != nil && ["com.apple.Safari", "com.google.Chrome", "org.mozilla.firefox", "com.microsoft.edgemac"].contains(browserId!)

        // For browsers, skip findElement and type directly via JS (much faster)
        if isBrowser && (strategy == .auto || strategy == .javascript) {
            return try await executeJavaScriptType(selector: selector, text: text, browser: browserId!)
        }

        let element = try await findElement(selector: selector, strategy: strategy, appBundleId: appBundleId)

        guard let source = element["source"] as? String else {
            throw WebAutomationError.invalidState("No source in element")
        }

        switch ElementSource(rawValue: source) {
        case .accessibility:
            let role = element["role"] as? String
            let title = element["title"] as? String
            return await MainActor.run {
                AccessibilityService.shared.typeTextIntoElement(
                    role: role,
                    title: title,
                    text: text,
                    appBundleId: appBundleId,
                    verify: verify
                )
            }

        case .javascript:
            guard let bid = browserId else {
                throw WebAutomationError.browserNotFound
            }
            return try await executeJavaScriptType(selector: selector, text: text, browser: bid)
            
        case .selenium:
            return try await seleniumType(selector: selector, text: text)
            
        case .none:
            throw WebAutomationError.invalidState("Unknown source: \(source)")
        }
    }
    
    /// Execute JavaScript in the browser
    func executeJavaScript(script: String, browser: String? = nil) async throws -> Any? {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        
        // Use AppleScript to execute JavaScript
        let appleScript: String
        switch browserId {
        case "com.apple.Safari":
            appleScript = """
            tell application "Safari"
                tell front document
                    do JavaScript "\(Self.escapeJSForAppleScript(script))"
                end tell
            end tell
            """
        case "org.mozilla.firefox":
            appleScript = """
            tell application "Firefox"
                tell front window
                    execute JavaScript "\(Self.escapeJSForAppleScript(script))"
                end tell
            end tell
            """
        default:
            // For Chrome and others, use Selenium
            return try await seleniumExecute(script: script)
        }
        
        let result = await Task.detached { () -> String? in
            var err: NSDictionary?
            guard let script = NSAppleScript(source: appleScript) else { return nil }
            let out = script.executeAndReturnError(&err)
            if let error = err {
                return "Error: \(error)"
            }
            return out.stringValue
        }.value

        return result
    }
    
    // MARK: - Strategy Implementations
    
    private func findViaAccessibility(selector: String, timeout: TimeInterval, appBundleId: String?) async throws -> [String: Any]? {
        // Parse selector to extract role/title/value hints
        let hints = parseSelector(selector)
        
        // Use adaptive wait from AccessibilityService
        let result = await MainActor.run {
            AccessibilityService.shared.waitForElementAdaptive(
                role: hints.role,
                title: hints.title,
                value: hints.value,
                appBundleId: appBundleId,
                timeout: timeout
            )
        }
        
        // Check if found
        if result.contains("\"success\": true") {
            // Parse JSON result
            if let data = result.data(using: .utf8),
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                return json
            }
        }
        
        return nil
    }
    
    private func findViaJavaScript(selector: String, browser: String) async throws -> [String: Any]? {
        // Escape selector for JavaScript
        let escapedSelector = Self.escapeJS(selector)

        // Determine if it's a CSS selector or XPath
        let isXPath = selector.hasPrefix("/") || selector.hasPrefix("./")
        
        let js: String
        if isXPath {
            js = """
            (function() {
                var result = document.evaluate('\(escapedSelector)', document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
                var el = result.singleNodeValue;
                if (!el) return null;
                var rect = el.getBoundingClientRect();
                return {
                    found: true,
                    tagName: el.tagName,
                    id: el.id || '',
                    className: el.className || '',
                    text: el.textContent ? el.textContent.substring(0, 200) : '',
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height
                };
            })()
            """
        } else {
            js = """
            (function() {
                var el = document.querySelector('\(escapedSelector)');
                if (!el) return null;
                var rect = el.getBoundingClientRect();
                return {
                    found: true,
                    tagName: el.tagName,
                    id: el.id || '',
                    className: el.className || '',
                    text: el.textContent ? el.textContent.substring(0, 200) : '',
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height
                };
            })()
            """
        }
        
        let result = try await executeJavaScript(script: js, browser: browser)
        
        if let dict = result as? [String: Any], let found = dict["found"] as? Bool, found {
            var element: [String: Any] = [
                "source": "javascript",
                "selector": selector,
                "tagName": dict["tagName"] ?? "",
                "text": dict["text"] ?? ""
            ]
            
            if let x = dict["x"] as? Double, let y = dict["y"] as? Double,
               let w = dict["width"] as? Double, let h = dict["height"] as? Double {
                element["bounds"] = ["x": x, "y": y, "width": w, "height": h]
            }
            
            return element
        }
        
        return nil
    }
    
    private func findViaSelenium(selector: String, timeout: TimeInterval) async throws -> [String: Any]? {
        // Note: Selenium operations are handled via Selenium AgentScript
        // This method returns nil to indicate Selenium should be called separately
        // The unified API will fall back to Accessibility/JS strategies
        return nil
    }
    
    // MARK: - AppleScript Helpers
    
    private func openViaAppleScript(url: URL, browser: BrowserType) async throws -> String {
        let script: String
        
        switch browser {
        case .safari:
            script = "tell application \"Safari\" to open location \"\(url.absoluteString)\""
        case .chrome:
            script = "tell application \"Google Chrome\" to open location \"\(url.absoluteString)\""
        case .firefox:
            script = "tell application \"Firefox\" to open location \"\(url.absoluteString)\""
        case .edge:
            script = "tell application \"Microsoft Edge\" to open location \"\(url.absoluteString)\""
        }
        
        let urlStr = url.absoluteString
        let browserName = browser.rawValue
        let result = await Task.detached { () -> String in
            var err: NSDictionary?
            guard let appleScript = NSAppleScript(source: script) else { return "Error: Could not create script" }
            _ = appleScript.executeAndReturnError(&err)
            if let error = err {
                return "Error: \(error)"
            }
            return "Opened \(urlStr) in \(browserName)"
        }.value
        
        if result.hasPrefix("Error:") {
            throw WebAutomationError.appleScriptError(result)
        }
        
        return result
    }
    
    private func executeJavaScriptClick(selector: String, browser: String) async throws -> String {
        // Handle jQuery-style :contains() or plain text — extract text, find by content, click by href/class
        if selector.contains(":contains(") {
            if let start = selector.range(of: ":contains(")?.upperBound {
                var text = String(selector[start...])
                text = text.trimmingCharacters(in: CharacterSet(charactersIn: "\"')"))
                if text.hasSuffix(")") { text = String(text.dropLast()) }
                text = text.trimmingCharacters(in: CharacterSet(charactersIn: "\"'"))
                // Use text search to find element, then click by its actual selector
                let escaped = Self.escapeJS(text)
                let js = """
                (function() {
                    var all = document.querySelectorAll('a, button, input[type=submit], [role=button]');
                    for (var i = 0; i < all.length; i++) {
                        if (all[i].textContent.toLowerCase().indexOf('\(escaped.lowercased())') >= 0) {
                            all[i].click();
                            return 'clicked';
                        }
                    }
                    return 'not found';
                })()
                """
                if let result = try? await executeJavaScript(script: js, browser: browser) as? String,
                   result == "clicked" {
                    return "Clicked element containing '\(text)'"
                }
            }
        }

        let escaped = Self.escapeJS(selector)
        let isXPath = selector.hasPrefix("/") || selector.hasPrefix("./")

        // First try: JS click (works for most buttons)
        let jsClick: String
        if isXPath {
            jsClick = """
            var result = document.evaluate('\(escaped)', document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
            var el = result.singleNodeValue;
            if (el) { el.click(); return 'clicked'; }
            return 'not found';
            """
        } else {
            jsClick = """
            (function() {
                var el = \(Self.querySelectorWithIframes("'\(escaped)'"));
                if (el) { el.click(); return 'clicked'; }
                return 'not found';
            })()
            """
        }

        if let result = try? await executeJavaScript(script: jsClick, browser: browser) as? String,
           result == "clicked" {
            return "Clicked element via JavaScript: \(selector)"
        }

        // Second try: dispatch mousedown/mouseup/click events (handles event delegation)
        let jsDispatch: String
        if isXPath {
            jsDispatch = """
            (function() {
                var result = document.evaluate('\(escaped)', document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
                var el = result.singleNodeValue;
                if (!el) return 'not found';
                el.dispatchEvent(new MouseEvent('mousedown', {bubbles:true,cancelable:true}));
                el.dispatchEvent(new MouseEvent('mouseup', {bubbles:true,cancelable:true}));
                el.dispatchEvent(new MouseEvent('click', {bubbles:true,cancelable:true}));
                return 'dispatched';
            })()
            """
        } else {
            jsDispatch = """
            (function() {
                var el = \(Self.querySelectorWithIframes("'\(escaped)'"));
                if (!el) return 'not found';
                el.dispatchEvent(new MouseEvent('mousedown', {bubbles:true,cancelable:true}));
                el.dispatchEvent(new MouseEvent('mouseup', {bubbles:true,cancelable:true}));
                el.dispatchEvent(new MouseEvent('click', {bubbles:true,cancelable:true}));
                return 'dispatched';
            })()
            """
        }

        if let result = try? await executeJavaScript(script: jsDispatch, browser: browser) as? String,
           result == "dispatched" {
            return "Clicked element via event dispatch: \(selector)"
        }

        // Third try: get element coordinates and do OS-level click via accessibility
        let jsCoords = isXPath ?
            """
            (function() {
                var result = document.evaluate('\(escaped)', document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
                var el = result.singleNodeValue;
                if (!el) return 'not found';
                var r = el.getBoundingClientRect();
                return Math.round(r.x + r.width/2) + ',' + Math.round(r.y + r.height/2);
            })()
            """ :
            """
            (function() {
                var el = \(Self.querySelectorWithIframes("'\(escaped)'"));
                if (!el) return 'not found';
                var r = el.getBoundingClientRect();
                return Math.round(r.x + r.width/2) + ',' + Math.round(r.y + r.height/2);
            })()
            """

        if let coordStr = try? await executeJavaScript(script: jsCoords, browser: browser) as? String,
           coordStr != "not found",
           let commaIdx = coordStr.firstIndex(of: ",") {
            let xStr = String(coordStr[..<commaIdx])
            let yStr = String(coordStr[coordStr.index(after: commaIdx)...])
            if let x = Double(xStr), let y = Double(yStr) {
                // Need to offset by browser window/toolbar position
                // Get browser window bounds via AppleScript
                let boundsJS = "JSON.stringify({scrollX: window.scrollX, scrollY: window.scrollY, screenX: window.screenX, screenY: window.screenY, outerHeight: window.outerHeight, innerHeight: window.innerHeight})"
                if let boundsStr = try? await executeJavaScript(script: boundsJS, browser: browser) as? String,
                   let boundsData = boundsStr.data(using: .utf8),
                   let bounds = try? JSONSerialization.jsonObject(with: boundsData) as? [String: Double] {
                    let screenX = bounds["screenX"] ?? 0
                    let screenY = bounds["screenY"] ?? 0
                    let outerH = bounds["outerHeight"] ?? 0
                    let innerH = bounds["innerHeight"] ?? 0
                    let toolbarH = outerH - innerH
                    let absX = screenX + x
                    let absY = screenY + toolbarH + y
                    _ = await MainActor.run { AccessibilityService.shared.clickAt(x: CGFloat(absX), y: CGFloat(absY)) }
                    return "Clicked element via OS click at (\(Int(absX)),\(Int(absY))): \(selector)"
                }
            }
        }

        return "Error: could not click element: \(selector)"
    }
    
    private func executeJavaScriptType(selector: String, text: String, browser: String) async throws -> String {
        let escapedText = Self.escapeJS(text)
        let escapedSel = Self.escapeJS(selector)
        let isXPath = selector.hasPrefix("/") || selector.hasPrefix("./")

        // Universal type function that handles:
        // 1. <input> / <textarea> — use React-compatible native setter
        // 2. contenteditable divs — use innerText + InputEvent (LinkedIn post, Gmail, Slack)
        // 3. [role="textbox"] — same as contenteditable
        // 4. Plain elements with .value — fallback
        let typeJS = """
        (function() {
            var sel = '\(escapedSel)';
            var text = '\(escapedText)';
            var el = \(isXPath ?
                "document.evaluate(sel, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue" :
                Self.querySelectorWithIframes("sel"));
            if (!el) return 'not found';

            el.focus();

            var tag = el.tagName.toUpperCase();
            var isEditable = el.isContentEditable || el.getAttribute('contenteditable') === 'true' || el.getAttribute('role') === 'textbox';

            if (isEditable) {
                // contenteditable: use execCommand to simulate real typing
                el.focus();
                // Clear existing content first
                if (el.innerText) { document.execCommand('selectAll', false, null); }
                // Insert text like a real keyboard
                var inserted = document.execCommand('insertText', false, text);
                if (!inserted) {
                    // Fallback: set innerText directly
                    el.innerText = text;
                    el.dispatchEvent(new InputEvent('input', {bubbles: true, inputType: 'insertText', data: text}));
                }
                return 'typed';
            }

            if (tag === 'INPUT' || tag === 'TEXTAREA') {
                // Simulate character-by-character typing (works on React/Vue/Angular)
                var proto = tag === 'INPUT' ? window.HTMLInputElement.prototype : window.HTMLTextAreaElement.prototype;
                var setter = Object.getOwnPropertyDescriptor(proto, 'value');
                // Clear existing value
                if (setter && setter.set) { setter.set.call(el, ''); }
                else { el.value = ''; }
                el.dispatchEvent(new Event('input', {bubbles: true}));
                // Type each character individually
                for (var i = 0; i < text.length; i++) {
                    var ch = text[i];
                    el.dispatchEvent(new KeyboardEvent('keydown', {key: ch, bubbles: true}));
                    // Set value to current prefix using native setter
                    var newVal = text.substring(0, i + 1);
                    if (setter && setter.set) { setter.set.call(el, newVal); }
                    else { el.value = newVal; }
                    el.dispatchEvent(new InputEvent('input', {bubbles: true, inputType: 'insertText', data: ch}));
                    el.dispatchEvent(new KeyboardEvent('keyup', {key: ch, bubbles: true}));
                }
                el.dispatchEvent(new Event('change', {bubbles: true}));
                return 'typed';
            }

            // Fallback: try execCommand (simulates real keyboard input), then .value
            el.focus();
            if (document.execCommand) {
                document.execCommand('selectAll', false, null);
                document.execCommand('insertText', false, text);
            } else if ('value' in el) {
                el.value = text;
                el.dispatchEvent(new Event('input', {bubbles: true}));
            } else {
                el.innerText = text;
                el.dispatchEvent(new InputEvent('input', {bubbles: true, inputType: 'insertText', data: text}));
            }
            return 'typed';
        })()
        """

        // Phase 1: Try JS-based typing (works for most sites)
        if let jsResult = try? await executeJavaScript(script: typeJS, browser: browser) as? String,
           jsResult == "typed" {
            // Verify the value was actually set
            let verifyJS = """
            (function() {
                var sel = '\(escapedSel)';
                var el = \(isXPath ?
                    "document.evaluate(sel, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue" :
                    Self.querySelectorWithIframes("sel"));
                if (!el) return '';
                return el.value || el.innerText || el.textContent || '';
            })()
            """
            if let val = try? await executeJavaScript(script: verifyJS, browser: browser) as? String,
               val.contains(text.prefix(5)) {
                return "Typed text via JavaScript into: \(selector)"
            }
        }

        // Phase 2: JS verify failed — retry with character-by-character approach
        let retryJS = """
        (function() {
            var sel = '\(escapedSel)';
            var text = '\(escapedText)';
            var el = \(isXPath ?
                "document.evaluate(sel, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue" :
                Self.querySelectorWithIframes("sel"));
            if (!el) return 'not found';
            el.focus();
            el.click();
            // Try execCommand first (works in contenteditable and some inputs in Safari)
            document.execCommand('selectAll', false, null);
            if (document.execCommand('insertText', false, text)) return 'typed';
            // Last resort: set value + char-by-char events
            el.value = '';
            for (var i = 0; i < text.length; i++) {
                el.value += text[i];
                el.dispatchEvent(new InputEvent('input', {bubbles: true, inputType: 'insertText', data: text[i]}));
            }
            return 'typed';
        })()
        """
        _ = try? await executeJavaScript(script: retryJS, browser: browser)
        return "Typed text into: \(selector)"
    }
    
    // MARK: - iframe Support

    /// JavaScript snippet that queries the main document and all same-origin iframes.
    /// Call with a quoted CSS selector string, e.g. querySelectorWithIframes("'button.submit'")
    static func querySelectorWithIframes(_ selectorExpr: String) -> String {
        """
        (function(sel) {
            var el = document.querySelector(sel);
            if (el) return el;
            var frames = document.querySelectorAll('iframe');
            for (var i = 0; i < frames.length; i++) {
                try {
                    var doc = frames[i].contentDocument;
                    if (doc) { el = doc.querySelector(sel); if (el) return el; }
                } catch(e) {}
            }
            return null;
        })(\(selectorExpr))
        """
    }
}

// MARK: - Supporting Types

enum SelectorStrategy: String {
    case auto
    case accessibility
    case javascript
    case selenium
}

enum WebAutomationError: Error, LocalizedError {
    case elementNotFound(String)
    case browserNotFound
    case timeout(String)
    case appleScriptError(String)
    case seleniumError(String)
    case invalidState(String)
    
    var errorDescription: String? {
        switch self {
        case .elementNotFound(let selector):
            return "Element not found: \(selector)"
        case .timeout(let msg):
            return "Timeout: \(msg)"
        case .browserNotFound:
            return "No active browser found"
        case .appleScriptError(let msg):
            return "AppleScript error: \(msg)"
        case .seleniumError(let msg):
            return "Selenium error: \(msg)"
        case .invalidState(let msg):
            return "Invalid state: \(msg)"
        }
    }
}
